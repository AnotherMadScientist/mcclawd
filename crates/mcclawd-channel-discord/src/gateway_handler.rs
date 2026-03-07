//! Live Discord gateway handler and event JSON parser.
//!
//! This module provides two layers:
//!
//! 1. **Always available**: [`parse_discord_event`] parses raw Discord Gateway
//!    `MESSAGE_CREATE` event JSON into [`DiscordMessage`] without any SDK
//!    dependency. This enables testing and webhook-based integrations.
//!
//! 2. **Feature-gated (`live`)**: When the `live` feature is enabled, the
//!    [`live`] submodule provides a serenity `EventHandler` impl that converts
//!    gateway events into [`DiscordMessage`] and pumps them through
//!    `normalize() -> Envelope` into the channel's inbox.

use chrono::{DateTime, TimeZone, Utc};

use crate::normalize::{DiscordAttachment, DiscordMessage};

// ---------------------------------------------------------------------------
// Gateway event JSON parsing (always available, no SDK dependency)
// ---------------------------------------------------------------------------

/// Parse a Discord Gateway `MESSAGE_CREATE` event JSON into a [`DiscordMessage`].
///
/// Accepts the `d` (data) payload from a gateway event, i.e. the message object.
/// Bot messages are filtered out (returns `None`).
///
/// # JSON format
/// ```json
/// {
///   "id": "123456789",
///   "channel_id": "987654321",
///   "guild_id": "111222333",
///   "author": { "id": "444555666", "username": "alice", "bot": false },
///   "content": "Hello!",
///   "timestamp": "2024-01-15T10:30:00.000000+00:00",
///   "attachments": [...]
/// }
/// ```
pub fn parse_discord_event(json: &serde_json::Value) -> Option<DiscordMessage> {
    let author = json.get("author")?;

    // Filter out bot messages.
    if author
        .get("bot")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return None;
    }

    let message_id = json.get("id")?.as_str()?.to_string();
    let channel_id = json.get("channel_id")?.as_str()?.to_string();
    let guild_id = json
        .get("guild_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let author_id = author.get("id")?.as_str()?.to_string();
    let author_name = author
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let content = json
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Parse ISO 8601 timestamp.
    let timestamp = json
        .get("timestamp")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);

    // Parse attachments.
    let attachments = json
        .get("attachments")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    Some(DiscordAttachment {
                        filename: a.get("filename")?.as_str()?.to_string(),
                        url: a.get("url")?.as_str()?.to_string(),
                        content_type: a
                            .get("content_type")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Some(DiscordMessage {
        message_id,
        channel_id,
        guild_id,
        author_id,
        author_name,
        content,
        attachments,
        timestamp,
    })
}

/// Check if a guild/channel is in the allowed lists.
/// Returns `true` if no filter is set or the IDs are in the allowed lists.
pub fn is_message_allowed(
    guild_id: &Option<String>,
    channel_id: &str,
    allowed_guild_ids: &Option<Vec<u64>>,
    allowed_channel_ids: &Option<Vec<u64>>,
) -> bool {
    if let Some(guilds) = allowed_guild_ids {
        match guild_id {
            Some(gid) => {
                if let Ok(id) = gid.parse::<u64>() {
                    if !guilds.contains(&id) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            None => return false, // DM, but guild filter is set
        }
    }

    if let Some(channels) = allowed_channel_ids {
        if let Ok(id) = channel_id.parse::<u64>() {
            if !channels.contains(&id) {
                return false;
            }
        } else {
            return false;
        }
    }

    true
}

// ---------------------------------------------------------------------------
// Outbound formatting
// ---------------------------------------------------------------------------

use mcclawd_channels::types::{ChannelStatus, OutboundChunk};

/// Format an [`OutboundChunk`] as a Discord API JSON payload.
///
/// The `platform_meta` should contain `channel_id` from the inbound envelope.
/// Returns a JSON object suitable for the Discord REST API.
/// Returns `None` for chunks that have no Discord representation.
pub fn format_outbound(
    chunk: &OutboundChunk,
    platform_meta: &serde_json::Value,
) -> Option<serde_json::Value> {
    let channel_id = platform_meta.get("channel_id")?.as_str()?;

    match chunk {
        OutboundChunk::TextBlock(text) | OutboundChunk::TextDelta(text) => {
            Some(serde_json::json!({
                "endpoint": format!("/channels/{}/messages", channel_id),
                "method": "POST",
                "body": {
                    "content": text,
                },
            }))
        }
        OutboundChunk::Media {
            mime_type,
            data,
            caption,
        } => {
            Some(serde_json::json!({
                "endpoint": format!("/channels/{}/messages", channel_id),
                "method": "POST",
                "body": {
                    "content": caption.clone().unwrap_or_default(),
                },
                "file": {
                    "size": data.len(),
                    "content_type": mime_type,
                },
            }))
        }
        OutboundChunk::Buttons { text, buttons } => {
            // Discord uses message components (action rows with buttons).
            let components: Vec<serde_json::Value> = buttons
                .iter()
                .map(|row| {
                    let row_buttons: Vec<serde_json::Value> = row
                        .iter()
                        .map(|btn| {
                            if let Some(ref url) = btn.url {
                                serde_json::json!({
                                    "type": 2,
                                    "style": 5, // Link
                                    "label": btn.label,
                                    "url": url,
                                })
                            } else {
                                serde_json::json!({
                                    "type": 2,
                                    "style": 1, // Primary
                                    "label": btn.label,
                                    "custom_id": btn.callback_data.clone().unwrap_or_else(|| btn.label.clone()),
                                })
                            }
                        })
                        .collect();
                    serde_json::json!({
                        "type": 1, // ActionRow
                        "components": row_buttons,
                    })
                })
                .collect();
            Some(serde_json::json!({
                "endpoint": format!("/channels/{}/messages", channel_id),
                "method": "POST",
                "body": {
                    "content": text,
                    "components": components,
                },
            }))
        }
        OutboundChunk::StatusIndicator(status) => {
            match status {
                ChannelStatus::Done => None,
                _ => Some(serde_json::json!({
                    "endpoint": format!("/channels/{}/typing", channel_id),
                    "method": "POST",
                })),
            }
        }
        OutboundChunk::Error(msg) => Some(serde_json::json!({
            "endpoint": format!("/channels/{}/messages", channel_id),
            "method": "POST",
            "body": {
                "content": format!("**Error:** {}", msg),
            },
        })),
        OutboundChunk::ToolStart { .. } => {
            Some(serde_json::json!({
                "endpoint": format!("/channels/{}/typing", channel_id),
                "method": "POST",
            }))
        }
        OutboundChunk::ToolEnd { name, summary } => {
            if let Some(s) = summary {
                Some(serde_json::json!({
                    "endpoint": format!("/channels/{}/messages", channel_id),
                    "method": "POST",
                    "body": {
                        "content": format!("*{}: {}*", name, s),
                    },
                }))
            } else {
                None
            }
        }
        OutboundChunk::UserMessage(_) | OutboundChunk::Done | OutboundChunk::Attachments(_) | OutboundChunk::Usage { .. } | OutboundChunk::ChatHistory(_) | OutboundChunk::GeneratedFiles(_) => None,
    }
}

// ---------------------------------------------------------------------------
// Live serenity handler (feature-gated)
// ---------------------------------------------------------------------------

#[cfg(feature = "live")]
pub mod live {
    //! Live serenity gateway event handler.
    //!
    //! Converts serenity `Message` events → [`DiscordMessage`] → `normalize()` → [`Envelope`].

    use serenity::async_trait;
    use serenity::model::channel::Message;
    use serenity::model::gateway::Ready;
    use serenity::prelude::*;
    use tokio::sync::mpsc;
    use tracing;

    use crate::normalize::{normalize, DiscordAttachment, DiscordMessage};
    use mcclawd_channels::envelope::Envelope;

    /// Serenity event handler that converts messages to Envelopes.
    pub struct DiscordHandler {
        pub inbox_tx: mpsc::Sender<Envelope>,
        pub allowed_guild_ids: Option<Vec<u64>>,
        pub allowed_channel_ids: Option<Vec<u64>>,
    }

    #[async_trait]
    impl EventHandler for DiscordHandler {
        async fn message(&self, _ctx: Context, msg: Message) {
            // Skip bot messages.
            if msg.author.bot {
                return;
            }

            // Check guild/channel filters.
            let guild_id = msg.guild_id.map(|g| g.to_string());
            let channel_id = msg.channel_id.to_string();
            if !super::is_message_allowed(
                &guild_id,
                &channel_id,
                &self.allowed_guild_ids,
                &self.allowed_channel_ids,
            ) {
                return;
            }

            let discord_msg = DiscordMessage {
                message_id: msg.id.to_string(),
                channel_id,
                guild_id,
                author_id: msg.author.id.to_string(),
                author_name: msg.author.name.clone(),
                content: msg.content.clone(),
                attachments: msg
                    .attachments
                    .iter()
                    .map(|a| DiscordAttachment {
                        filename: a.filename.clone(),
                        url: a.url.clone(),
                        content_type: a.content_type.clone(),
                    })
                    .collect(),
                timestamp: *msg.timestamp,
            };

            let envelope = normalize(&discord_msg);
            if let Err(e) = self.inbox_tx.send(envelope).await {
                tracing::error!("Failed to send Discord envelope: {}", e);
            }
        }

        async fn ready(&self, _ctx: Context, ready: Ready) {
            tracing::info!("Discord bot connected as {}", ready.user.name);
        }
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
    fn parse_text_event() {
        let event = json!({
            "id": "123456789012345678",
            "channel_id": "987654321012345678",
            "guild_id": "111222333444555666",
            "author": {
                "id": "444555666777888999",
                "username": "alice",
                "discriminator": "0001",
                "bot": false
            },
            "content": "Hello, Discord!",
            "timestamp": "2024-01-15T10:30:00.000000+00:00",
            "attachments": []
        });

        let msg = parse_discord_event(&event).unwrap();
        assert_eq!(msg.message_id, "123456789012345678");
        assert_eq!(msg.channel_id, "987654321012345678");
        assert_eq!(msg.guild_id, Some("111222333444555666".into()));
        assert_eq!(msg.author_id, "444555666777888999");
        assert_eq!(msg.author_name, "alice");
        assert_eq!(msg.content, "Hello, Discord!");
        assert!(msg.attachments.is_empty());
    }

    #[test]
    fn parse_with_attachments() {
        let event = json!({
            "id": "111",
            "channel_id": "222",
            "author": { "id": "333", "username": "bob", "bot": false },
            "content": "Check this file",
            "timestamp": "2024-01-15T10:30:00+00:00",
            "attachments": [
                {
                    "id": "att1",
                    "filename": "image.png",
                    "url": "https://cdn.discord.com/attachments/image.png",
                    "content_type": "image/png",
                    "size": 12345
                }
            ]
        });

        let msg = parse_discord_event(&event).unwrap();
        assert_eq!(msg.attachments.len(), 1);
        assert_eq!(msg.attachments[0].filename, "image.png");
        assert_eq!(msg.attachments[0].content_type, Some("image/png".into()));
    }

    #[test]
    fn bot_messages_filtered_out() {
        let event = json!({
            "id": "111",
            "channel_id": "222",
            "author": { "id": "333", "username": "botuser", "bot": true },
            "content": "I am a bot",
            "timestamp": "2024-01-15T10:30:00+00:00",
            "attachments": []
        });

        assert!(parse_discord_event(&event).is_none());
    }

    #[test]
    fn missing_fields_returns_none() {
        // No author
        let bad1 = json!({"id": "1", "channel_id": "2", "content": "x"});
        assert!(parse_discord_event(&bad1).is_none());

        // No id
        let bad2 = json!({"channel_id": "2", "author": {"id": "3", "username": "a"}});
        assert!(parse_discord_event(&bad2).is_none());

        // Empty
        let bad3 = json!({});
        assert!(parse_discord_event(&bad3).is_none());
    }

    #[test]
    fn parse_dm_no_guild_id() {
        let event = json!({
            "id": "111",
            "channel_id": "222",
            "author": { "id": "333", "username": "carol", "bot": false },
            "content": "DM message",
            "timestamp": "2024-01-15T10:30:00+00:00",
            "attachments": []
        });

        let msg = parse_discord_event(&event).unwrap();
        assert!(msg.guild_id.is_none());
    }

    #[test]
    fn message_allowed_no_filters() {
        assert!(is_message_allowed(
            &Some("123".into()),
            "456",
            &None,
            &None,
        ));
    }

    #[test]
    fn message_allowed_guild_filter() {
        let guilds = Some(vec![123]);
        assert!(is_message_allowed(
            &Some("123".into()),
            "456",
            &guilds,
            &None,
        ));
        assert!(!is_message_allowed(
            &Some("999".into()),
            "456",
            &guilds,
            &None,
        ));
        // DM with guild filter rejects
        assert!(!is_message_allowed(&None, "456", &guilds, &None));
    }

    #[test]
    fn message_allowed_channel_filter() {
        let channels = Some(vec![456]);
        assert!(is_message_allowed(
            &Some("123".into()),
            "456",
            &None,
            &channels,
        ));
        assert!(!is_message_allowed(
            &Some("123".into()),
            "789",
            &None,
            &channels,
        ));
    }

    // -----------------------------------------------------------------------
    // Outbound formatting tests
    // -----------------------------------------------------------------------

    #[test]
    fn format_text_block() {
        let chunk = OutboundChunk::TextBlock("Hello!".into());
        let meta = json!({"channel_id": "123456"});
        let payload = format_outbound(&chunk, &meta).unwrap();
        assert_eq!(payload["body"]["content"], "Hello!");
        assert!(payload["endpoint"].as_str().unwrap().contains("123456"));
    }

    #[test]
    fn format_media() {
        let chunk = OutboundChunk::Media {
            mime_type: "image/png".into(),
            data: vec![1, 2, 3],
            caption: Some("A picture".into()),
        };
        let meta = json!({"channel_id": "123456"});
        let payload = format_outbound(&chunk, &meta).unwrap();
        assert_eq!(payload["body"]["content"], "A picture");
        assert!(payload["file"]["size"].is_number());
    }

    #[test]
    fn format_buttons_with_components() {
        use mcclawd_channels::types::InlineButton;
        let chunk = OutboundChunk::Buttons {
            text: "Choose:".into(),
            buttons: vec![vec![InlineButton {
                label: "Click".into(),
                callback_data: Some("click_id".into()),
                url: None,
            }]],
        };
        let meta = json!({"channel_id": "123456"});
        let payload = format_outbound(&chunk, &meta).unwrap();
        let comps = &payload["body"]["components"];
        assert_eq!(comps[0]["type"], 1); // ActionRow
        assert_eq!(comps[0]["components"][0]["label"], "Click");
    }

    #[test]
    fn format_status_typing() {
        let chunk = OutboundChunk::StatusIndicator(ChannelStatus::Typing);
        let meta = json!({"channel_id": "123456"});
        let payload = format_outbound(&chunk, &meta).unwrap();
        assert!(payload["endpoint"].as_str().unwrap().contains("typing"));
    }

    #[test]
    fn format_done_returns_none() {
        let chunk = OutboundChunk::Done;
        let meta = json!({"channel_id": "123456"});
        assert!(format_outbound(&chunk, &meta).is_none());
    }

    #[test]
    fn format_missing_channel_id_returns_none() {
        let chunk = OutboundChunk::TextBlock("Hello".into());
        let meta = json!({});
        assert!(format_outbound(&chunk, &meta).is_none());
    }
}
