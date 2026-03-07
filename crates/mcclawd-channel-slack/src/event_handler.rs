//! Slack Events API handler and event JSON parser.
//!
//! This module provides:
//!
//! 1. **Always available**: [`parse_slack_event`] parses raw Slack Events API
//!    `message` event JSON into [`SlackMessage`] without any SDK dependency.
//!    [`parse_url_verification`] handles the Slack URL verification challenge.
//!
//! 2. **Feature-gated (`live`)**: When the `live` feature is enabled, the
//!    [`live`] submodule provides slack-morphism integration.

use crate::normalize::{SlackFile, SlackMessage};

// ---------------------------------------------------------------------------
// Events API JSON parsing (always available, no SDK dependency)
// ---------------------------------------------------------------------------

/// Parse a Slack Events API `event_callback` payload into a [`SlackMessage`].
///
/// Expects the full outer payload:
/// ```json
/// {
///   "type": "event_callback",
///   "event": {
///     "type": "message",
///     "user": "U01234",
///     "text": "Hello!",
///     "ts": "1700000000.000100",
///     "channel": "C01234",
///     "thread_ts": "1700000000.000000",
///     "files": [...]
///   }
/// }
/// ```
///
/// Also accepts a bare event object (without the outer `event_callback` wrapper).
///
/// Returns `None` for:
/// - Non-message events
/// - Bot messages (`bot_id` present, or `subtype` is `bot_message`)
/// - Message subtypes like `message_changed`, `message_deleted`
pub fn parse_slack_event(json: &serde_json::Value) -> Option<SlackMessage> {
    // Extract the event object (from wrapper or bare).
    let event = json.get("event").unwrap_or(json);

    // Must be a "message" event type.
    let event_type = event.get("type").and_then(|v| v.as_str())?;
    if event_type != "message" {
        return None;
    }

    // Filter out bot messages.
    if event.get("bot_id").is_some() {
        return None;
    }
    if let Some(subtype) = event.get("subtype").and_then(|v| v.as_str()) {
        // Allow "file_share" subtype but reject others like "bot_message",
        // "message_changed", "message_deleted", etc.
        if subtype != "file_share" {
            return None;
        }
    }

    let ts = event.get("ts").and_then(|v| v.as_str())?.to_string();
    let channel_id = event
        .get("channel")
        .and_then(|v| v.as_str())?
        .to_string();
    let user_id = event.get("user").and_then(|v| v.as_str())?.to_string();
    let user_name = event
        .get("user_profile")
        .and_then(|p| p.get("display_name"))
        .and_then(|v| v.as_str())
        .or_else(|| {
            event
                .get("user_profile")
                .and_then(|p| p.get("real_name"))
                .and_then(|v| v.as_str())
        })
        .map(String::from);
    let text = event
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let thread_ts = event
        .get("thread_ts")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Parse file attachments.
    let files = event
        .get("files")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    Some(SlackFile {
                        name: f.get("name")?.as_str()?.to_string(),
                        url_private: f
                            .get("url_private_download")
                            .or_else(|| f.get("url_private"))
                            .and_then(|v| v.as_str())?
                            .to_string(),
                        mimetype: f
                            .get("mimetype")
                            .and_then(|v| v.as_str())
                            .unwrap_or("application/octet-stream")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Some(SlackMessage {
        ts,
        channel_id,
        user_id,
        user_name,
        text,
        thread_ts,
        files,
    })
}

/// Parse a Slack URL verification challenge from the Events API.
///
/// When Slack first sends events to your endpoint, it sends a verification:
/// ```json
/// { "type": "url_verification", "challenge": "abc123...", "token": "..." }
/// ```
///
/// Returns the challenge string if this is a URL verification request.
pub fn parse_url_verification(json: &serde_json::Value) -> Option<String> {
    let msg_type = json.get("type").and_then(|v| v.as_str())?;
    if msg_type != "url_verification" {
        return None;
    }
    json.get("challenge")
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Check if a channel ID is in the allowed list.
pub fn is_channel_allowed(channel_id: &str, allowed: &Option<Vec<String>>) -> bool {
    match allowed {
        None => true,
        Some(ids) => ids.iter().any(|id| id == channel_id),
    }
}

// ---------------------------------------------------------------------------
// Outbound formatting
// ---------------------------------------------------------------------------

use mcclawd_channels::types::{ChannelStatus, OutboundChunk};

/// Format an [`OutboundChunk`] as a Slack Web API JSON payload.
///
/// The `platform_meta` should contain `channel_id` from the inbound envelope.
/// Returns a JSON object suitable for `chat.postMessage`, etc.
/// Returns `None` for chunks that have no Slack representation.
pub fn format_outbound(
    chunk: &OutboundChunk,
    platform_meta: &serde_json::Value,
) -> Option<serde_json::Value> {
    let channel = platform_meta.get("channel_id")?.as_str()?;
    let thread_ts = platform_meta
        .get("thread_ts")
        .and_then(|v| v.as_str());

    match chunk {
        OutboundChunk::TextBlock(text) | OutboundChunk::TextDelta(text) => {
            let mut payload = serde_json::json!({
                "method": "chat.postMessage",
                "channel": channel,
                "text": text,
            });
            if let Some(ts) = thread_ts {
                payload["thread_ts"] = serde_json::Value::String(ts.to_string());
            }
            Some(payload)
        }
        OutboundChunk::Media {
            mime_type,
            data,
            caption,
        } => {
            let mut payload = serde_json::json!({
                "method": "files.uploadV2",
                "channel": channel,
                "file_size": data.len(),
                "content_type": mime_type,
            });
            if let Some(c) = caption {
                payload["initial_comment"] = serde_json::Value::String(c.clone());
            }
            if let Some(ts) = thread_ts {
                payload["thread_ts"] = serde_json::Value::String(ts.to_string());
            }
            Some(payload)
        }
        OutboundChunk::Buttons { text, buttons } => {
            // Slack uses Block Kit for interactive elements.
            let mut blocks = vec![serde_json::json!({
                "type": "section",
                "text": {
                    "type": "mrkdwn",
                    "text": text,
                },
            })];

            let actions: Vec<serde_json::Value> = buttons
                .iter()
                .flatten()
                .map(|btn| {
                    if let Some(ref url) = btn.url {
                        serde_json::json!({
                            "type": "button",
                            "text": { "type": "plain_text", "text": btn.label },
                            "url": url,
                        })
                    } else {
                        serde_json::json!({
                            "type": "button",
                            "text": { "type": "plain_text", "text": btn.label },
                            "action_id": btn.callback_data.clone().unwrap_or_else(|| btn.label.clone()),
                        })
                    }
                })
                .collect();

            blocks.push(serde_json::json!({
                "type": "actions",
                "elements": actions,
            }));

            let mut payload = serde_json::json!({
                "method": "chat.postMessage",
                "channel": channel,
                "text": text,
                "blocks": blocks,
            });
            if let Some(ts) = thread_ts {
                payload["thread_ts"] = serde_json::Value::String(ts.to_string());
            }
            Some(payload)
        }
        OutboundChunk::StatusIndicator(status) => match status {
            ChannelStatus::Done => None,
            _ => None, // Slack doesn't have a native typing indicator API for bots
        },
        OutboundChunk::Error(msg) => {
            let mut payload = serde_json::json!({
                "method": "chat.postMessage",
                "channel": channel,
                "text": format!(":warning: Error: {}", msg),
            });
            if let Some(ts) = thread_ts {
                payload["thread_ts"] = serde_json::Value::String(ts.to_string());
            }
            Some(payload)
        }
        OutboundChunk::ToolStart { .. } => None,
        OutboundChunk::ToolEnd { name, summary } => {
            if let Some(s) = summary {
                let mut payload = serde_json::json!({
                    "method": "chat.postMessage",
                    "channel": channel,
                    "text": format!("_{}: {}_", name, s),
                });
                if let Some(ts) = thread_ts {
                    payload["thread_ts"] = serde_json::Value::String(ts.to_string());
                }
                Some(payload)
            } else {
                None
            }
        }
        OutboundChunk::UserMessage(_) | OutboundChunk::Attachments(_) | OutboundChunk::Done | OutboundChunk::Usage { .. } | OutboundChunk::ChatHistory(_) | OutboundChunk::GeneratedFiles(_) => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_message_event() {
        let payload = json!({
            "type": "event_callback",
            "event": {
                "type": "message",
                "user": "U01234ABCDE",
                "text": "Hello, Slack!",
                "ts": "1700000000.000100",
                "channel": "C01234ABCDE"
            }
        });

        let msg = parse_slack_event(&payload).unwrap();
        assert_eq!(msg.ts, "1700000000.000100");
        assert_eq!(msg.channel_id, "C01234ABCDE");
        assert_eq!(msg.user_id, "U01234ABCDE");
        assert_eq!(msg.text, "Hello, Slack!");
        assert!(msg.thread_ts.is_none());
        assert!(msg.files.is_empty());
    }

    #[test]
    fn parse_threaded_message() {
        let payload = json!({
            "type": "event_callback",
            "event": {
                "type": "message",
                "user": "U01234ABCDE",
                "text": "Reply in thread",
                "ts": "1700000001.000200",
                "thread_ts": "1700000000.000100",
                "channel": "C01234ABCDE"
            }
        });

        let msg = parse_slack_event(&payload).unwrap();
        assert_eq!(msg.thread_ts, Some("1700000000.000100".into()));
    }

    #[test]
    fn parse_file_shared_event() {
        let payload = json!({
            "type": "event_callback",
            "event": {
                "type": "message",
                "subtype": "file_share",
                "user": "U01234ABCDE",
                "text": "Uploaded a file",
                "ts": "1700000002.000300",
                "channel": "C01234ABCDE",
                "files": [
                    {
                        "id": "F01234",
                        "name": "report.pdf",
                        "mimetype": "application/pdf",
                        "url_private_download": "https://files.slack.com/files-pri/report.pdf"
                    }
                ]
            }
        });

        let msg = parse_slack_event(&payload).unwrap();
        assert_eq!(msg.files.len(), 1);
        assert_eq!(msg.files[0].name, "report.pdf");
        assert_eq!(msg.files[0].mimetype, "application/pdf");
    }

    #[test]
    fn url_verification() {
        let payload = json!({
            "type": "url_verification",
            "challenge": "abc123xyz",
            "token": "fake_token"
        });

        let challenge = parse_url_verification(&payload).unwrap();
        assert_eq!(challenge, "abc123xyz");
    }

    #[test]
    fn url_verification_wrong_type() {
        let payload = json!({
            "type": "event_callback",
            "event": { "type": "message" }
        });

        assert!(parse_url_verification(&payload).is_none());
    }

    #[test]
    fn bot_messages_filtered() {
        let payload = json!({
            "type": "event_callback",
            "event": {
                "type": "message",
                "bot_id": "B01234",
                "text": "I am a bot",
                "ts": "1700000003.000400",
                "channel": "C01234ABCDE"
            }
        });

        assert!(parse_slack_event(&payload).is_none());
    }

    #[test]
    fn bot_message_subtype_filtered() {
        let payload = json!({
            "type": "event_callback",
            "event": {
                "type": "message",
                "subtype": "bot_message",
                "user": "U01234ABCDE",
                "text": "bot msg",
                "ts": "1700000003.000400",
                "channel": "C01234ABCDE"
            }
        });

        assert!(parse_slack_event(&payload).is_none());
    }

    #[test]
    fn message_changed_filtered() {
        let payload = json!({
            "type": "event_callback",
            "event": {
                "type": "message",
                "subtype": "message_changed",
                "ts": "1700000003.000400",
                "channel": "C01234ABCDE"
            }
        });

        assert!(parse_slack_event(&payload).is_none());
    }

    #[test]
    fn non_message_event_filtered() {
        let payload = json!({
            "type": "event_callback",
            "event": {
                "type": "reaction_added",
                "user": "U01234",
                "reaction": "thumbsup"
            }
        });

        assert!(parse_slack_event(&payload).is_none());
    }

    #[test]
    fn parse_bare_event_object() {
        let event = json!({
            "type": "message",
            "user": "U01234ABCDE",
            "text": "bare event",
            "ts": "1700000004.000500",
            "channel": "C01234ABCDE"
        });

        let msg = parse_slack_event(&event).unwrap();
        assert_eq!(msg.text, "bare event");
    }

    #[test]
    fn channel_allowed_filtering() {
        let allowed = Some(vec!["C01234".to_string(), "C05678".to_string()]);
        assert!(is_channel_allowed("C01234", &allowed));
        assert!(!is_channel_allowed("C99999", &allowed));

        let no_filter: Option<Vec<String>> = None;
        assert!(is_channel_allowed("C99999", &no_filter));
    }

    #[test]
    fn parse_with_user_profile() {
        let payload = json!({
            "type": "event_callback",
            "event": {
                "type": "message",
                "user": "U01234ABCDE",
                "text": "Hello",
                "ts": "1700000005.000600",
                "channel": "C01234ABCDE",
                "user_profile": {
                    "display_name": "Alice Smith",
                    "real_name": "Alice"
                }
            }
        });

        let msg = parse_slack_event(&payload).unwrap();
        assert_eq!(msg.user_name, Some("Alice Smith".into()));
    }

    // -----------------------------------------------------------------------
    // Outbound formatting tests
    // -----------------------------------------------------------------------

    #[test]
    fn format_text_block() {
        let chunk = OutboundChunk::TextBlock("Hello!".into());
        let meta = json!({"channel_id": "C01234"});
        let payload = format_outbound(&chunk, &meta).unwrap();
        assert_eq!(payload["method"], "chat.postMessage");
        assert_eq!(payload["channel"], "C01234");
        assert_eq!(payload["text"], "Hello!");
    }

    #[test]
    fn format_text_in_thread() {
        let chunk = OutboundChunk::TextBlock("Reply".into());
        let meta = json!({"channel_id": "C01234", "thread_ts": "1700000000.000100"});
        let payload = format_outbound(&chunk, &meta).unwrap();
        assert_eq!(payload["thread_ts"], "1700000000.000100");
    }

    #[test]
    fn format_buttons_block_kit() {
        use mcclawd_channels::types::InlineButton;
        let chunk = OutboundChunk::Buttons {
            text: "Choose:".into(),
            buttons: vec![vec![InlineButton {
                label: "OK".into(),
                callback_data: Some("ok".into()),
                url: None,
            }]],
        };
        let meta = json!({"channel_id": "C01234"});
        let payload = format_outbound(&chunk, &meta).unwrap();
        assert!(payload["blocks"].is_array());
        let actions = &payload["blocks"][1];
        assert_eq!(actions["type"], "actions");
    }

    #[test]
    fn format_status_returns_none() {
        let chunk = OutboundChunk::StatusIndicator(ChannelStatus::Typing);
        let meta = json!({"channel_id": "C01234"});
        // Slack doesn't have a bot typing indicator
        assert!(format_outbound(&chunk, &meta).is_none());
    }

    #[test]
    fn format_done_returns_none() {
        let chunk = OutboundChunk::Done;
        let meta = json!({"channel_id": "C01234"});
        assert!(format_outbound(&chunk, &meta).is_none());
    }

    #[test]
    fn format_missing_channel_returns_none() {
        let chunk = OutboundChunk::TextBlock("Hello".into());
        let meta = json!({});
        assert!(format_outbound(&chunk, &meta).is_none());
    }
}
