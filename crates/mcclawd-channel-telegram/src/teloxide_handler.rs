//! Live teloxide dispatcher wiring and webhook JSON parser.
//!
//! This module provides two layers:
//!
//! 1. **Always available**: [`parse_telegram_update`] parses raw Telegram Bot API
//!    JSON (webhook or getUpdates response) into [`TelegramMessage`] without any
//!    SDK dependency. This enables testing and webhook-based deployments.
//!
//! 2. **Feature-gated (`live`)**: When the `live` feature is enabled, the
//!    [`live`] submodule provides a teloxide dispatcher handler that converts
//!    `teloxide::types::Message` into [`TelegramMessage`] and pumps it through
//!    `normalize() -> Envelope` into the channel's inbox.

use chrono::{DateTime, TimeZone, Utc};

use crate::normalize::{TelegramDocument, TelegramMessage, TelegramPhoto};

// ---------------------------------------------------------------------------
// Webhook / raw JSON parsing (always available, no SDK dependency)
// ---------------------------------------------------------------------------

/// Parse a raw Telegram Bot API update JSON into a [`TelegramMessage`].
///
/// Accepts either a full update object `{"update_id":..., "message":{...}}`
/// or a bare message object `{"message_id":..., "chat":{...}, ...}`.
///
/// Returns `None` if the JSON does not contain a parseable message
/// (e.g. it is a callback_query, edited_message, etc.).
pub fn parse_telegram_update(json: &serde_json::Value) -> Option<TelegramMessage> {
    // Try top-level "message" key first (full update envelope).
    let message = json
        .get("message")
        .or_else(|| {
            // Bare message object: must have "message_id" and "chat".
            if json.get("message_id").is_some() && json.get("chat").is_some() {
                Some(json)
            } else {
                None
            }
        })?;

    let message_id = message.get("message_id")?.as_i64()?;
    let chat = message.get("chat")?;
    let chat_id = chat.get("id")?.as_i64()?;

    // Sender ("from" object).
    let from = message.get("from");
    let from_user_id = from.and_then(|f| f.get("id")).and_then(|v| v.as_i64());
    let from_username = from
        .and_then(|f| f.get("username"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let from_display_name = from.and_then(|f| {
        let first = f.get("first_name").and_then(|v| v.as_str()).unwrap_or("");
        let last = f.get("last_name").and_then(|v| v.as_str()).unwrap_or("");
        let name = format!("{} {}", first, last).trim().to_string();
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    });

    // Text and caption.
    let text = message
        .get("text")
        .and_then(|v| v.as_str())
        .map(String::from);
    let caption = message
        .get("caption")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Reply-to.
    let reply_to_message_id = message
        .get("reply_to_message")
        .and_then(|r| r.get("message_id"))
        .and_then(|v| v.as_i64());

    // Timestamp.
    let date_unix = message.get("date").and_then(|v| v.as_i64()).unwrap_or(0);
    let date: DateTime<Utc> = Utc
        .timestamp_opt(date_unix, 0)
        .single()
        .unwrap_or_else(Utc::now);

    // Photos (array of PhotoSize objects, ordered small → large).
    let photos: Vec<TelegramPhoto> = message
        .get("photo")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    Some(TelegramPhoto {
                        file_id: p.get("file_id")?.as_str()?.to_string(),
                        width: p.get("width")?.as_u64()? as u32,
                        height: p.get("height")?.as_u64()? as u32,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    // Document.
    let document = message.get("document").and_then(|d| {
        Some(TelegramDocument {
            file_id: d.get("file_id")?.as_str()?.to_string(),
            file_name: d.get("file_name").and_then(|v| v.as_str()).map(String::from),
            mime_type: d.get("mime_type").and_then(|v| v.as_str()).map(String::from),
        })
    });

    Some(TelegramMessage {
        message_id,
        chat_id,
        from_user_id,
        from_username,
        from_display_name,
        text,
        caption,
        reply_to_message_id,
        date,
        photos,
        document,
    })
}

/// Check if a chat ID is in the allowed list.
/// Returns `true` if `allowed` is `None` (all chats allowed) or if `chat_id` is in the list.
pub fn is_chat_allowed(chat_id: i64, allowed: &Option<Vec<i64>>) -> bool {
    match allowed {
        None => true,
        Some(ids) => ids.contains(&chat_id),
    }
}

// ---------------------------------------------------------------------------
// Outbound formatting
// ---------------------------------------------------------------------------

use mcclawd_channels::types::{ChannelStatus, OutboundChunk};

/// Format an [`OutboundChunk`] as a Telegram Bot API JSON payload.
///
/// The `platform_meta` should contain `chat_id` from the inbound envelope.
/// Returns a JSON object suitable for `sendMessage`, `sendPhoto`, etc.
/// Returns `None` for chunks that have no Telegram representation (e.g. `ToolStart`).
pub fn format_outbound(
    chunk: &OutboundChunk,
    platform_meta: &serde_json::Value,
) -> Option<serde_json::Value> {
    let chat_id = platform_meta.get("chat_id")?;

    match chunk {
        OutboundChunk::TextBlock(text) | OutboundChunk::TextDelta(text) => {
            Some(serde_json::json!({
                "method": "sendMessage",
                "chat_id": chat_id,
                "text": text,
                "parse_mode": "Markdown",
            }))
        }
        OutboundChunk::Media {
            mime_type,
            data,
            caption,
        } => {
            let data_len = data.len();
            let method = if mime_type.starts_with("image/") {
                "sendPhoto"
            } else if mime_type.starts_with("audio/") {
                "sendAudio"
            } else if mime_type.starts_with("video/") {
                "sendVideo"
            } else {
                "sendDocument"
            };
            Some(serde_json::json!({
                "method": method,
                "chat_id": chat_id,
                "file_size": data_len,
                "mime_type": mime_type,
                "caption": caption,
            }))
        }
        OutboundChunk::Buttons { text, buttons } => {
            let keyboard: Vec<Vec<serde_json::Value>> = buttons
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|btn| {
                            let mut obj = serde_json::json!({
                                "text": btn.label,
                            });
                            if let Some(ref cb) = btn.callback_data {
                                obj["callback_data"] = serde_json::Value::String(cb.clone());
                            }
                            if let Some(ref url) = btn.url {
                                obj["url"] = serde_json::Value::String(url.clone());
                            }
                            obj
                        })
                        .collect()
                })
                .collect();
            Some(serde_json::json!({
                "method": "sendMessage",
                "chat_id": chat_id,
                "text": text,
                "reply_markup": {
                    "inline_keyboard": keyboard,
                },
            }))
        }
        OutboundChunk::StatusIndicator(status) => {
            let action = match status {
                ChannelStatus::Typing | ChannelStatus::Processing => "typing",
                ChannelStatus::UploadingMedia => "upload_document",
                ChannelStatus::Done => return None,
            };
            Some(serde_json::json!({
                "method": "sendChatAction",
                "chat_id": chat_id,
                "action": action,
            }))
        }
        OutboundChunk::Error(msg) => Some(serde_json::json!({
            "method": "sendMessage",
            "chat_id": chat_id,
            "text": format!("Error: {}", msg),
        })),
        OutboundChunk::ToolStart { name } => Some(serde_json::json!({
            "method": "sendChatAction",
            "chat_id": chat_id,
            "action": "typing",
            "_tool": name,
        })),
        OutboundChunk::ToolEnd { name, summary } => {
            if let Some(s) = summary {
                Some(serde_json::json!({
                    "method": "sendMessage",
                    "chat_id": chat_id,
                    "text": format!("_{}: {}_", name, s),
                    "parse_mode": "Markdown",
                }))
            } else {
                None
            }
        }
        OutboundChunk::Done => None,
    }
}

// ---------------------------------------------------------------------------
// Live teloxide handler (feature-gated)
// ---------------------------------------------------------------------------

#[cfg(feature = "live")]
pub mod live {
    //! Live teloxide dispatcher wiring.
    //!
    //! Converts `teloxide::types::Message` → [`TelegramMessage`] → `normalize()` → [`Envelope`].

    use chrono::{DateTime, Utc};
    use teloxide::prelude::*;
    use tokio::sync::mpsc;

    use crate::normalize::{normalize, TelegramDocument, TelegramMessage, TelegramPhoto};
    use mcclawd_channels::envelope::Envelope;

    /// Convert a `teloxide::types::Message` into our intermediate [`TelegramMessage`].
    ///
    /// Returns `None` if the message has no sender (e.g. channel posts without `from`).
    pub fn convert_teloxide_message(msg: &Message) -> Option<TelegramMessage> {
        let from = msg.from.as_ref();

        let photos: Vec<TelegramPhoto> = msg
            .photo()
            .map(|sizes| {
                sizes
                    .iter()
                    .map(|ps| TelegramPhoto {
                        file_id: ps.file.id.clone(),
                        width: ps.width,
                        height: ps.height,
                    })
                    .collect()
            })
            .unwrap_or_default();

        let document = msg.document().map(|d| TelegramDocument {
            file_id: d.file.id.clone(),
            file_name: d.file_name.clone(),
            mime_type: d.mime_type.as_ref().map(|m| m.to_string()),
        });

        let date: DateTime<Utc> = msg.date.into();

        Some(TelegramMessage {
            message_id: msg.id.0 as i64,
            chat_id: msg.chat.id.0,
            from_user_id: from.map(|u| u.id.0 as i64),
            from_username: from.and_then(|u| u.username.clone()),
            from_display_name: from.map(|u| {
                let name = format!(
                    "{} {}",
                    u.first_name,
                    u.last_name.as_deref().unwrap_or("")
                )
                .trim()
                .to_string();
                name
            }),
            text: msg.text().map(String::from),
            caption: msg.caption().map(String::from),
            reply_to_message_id: msg
                .reply_to_message()
                .map(|r| r.id.0 as i64),
            date,
            photos,
            document,
        })
    }

    /// Process a teloxide message: convert, normalize, and send to the inbox.
    ///
    /// Returns `Ok(())` if the message was sent (or skipped), `Err` on send failure.
    pub async fn handle_message(
        msg: Message,
        inbox_tx: mpsc::Sender<Envelope>,
        allowed_chat_ids: Option<Vec<i64>>,
    ) -> Result<(), String> {
        // Filter by allowed chat IDs.
        if !super::is_chat_allowed(msg.chat.id.0, &allowed_chat_ids) {
            return Ok(());
        }

        let telegram_msg = match convert_teloxide_message(&msg) {
            Some(m) => m,
            None => return Ok(()),
        };

        let envelope = normalize(&telegram_msg);
        inbox_tx
            .send(envelope)
            .await
            .map_err(|e| format!("Failed to send envelope: {}", e))
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
    fn parse_text_message() {
        let update = json!({
            "update_id": 123456789,
            "message": {
                "message_id": 42,
                "from": {
                    "id": 123456,
                    "is_bot": false,
                    "first_name": "Test",
                    "last_name": "User",
                    "username": "testuser"
                },
                "chat": {
                    "id": -1001234567890_i64,
                    "type": "supergroup"
                },
                "date": 1700000000,
                "text": "Hello, bot!"
            }
        });

        let msg = parse_telegram_update(&update).unwrap();
        assert_eq!(msg.message_id, 42);
        assert_eq!(msg.chat_id, -1001234567890);
        assert_eq!(msg.from_user_id, Some(123456));
        assert_eq!(msg.from_username, Some("testuser".into()));
        assert_eq!(msg.from_display_name, Some("Test User".into()));
        assert_eq!(msg.text, Some("Hello, bot!".into()));
        assert!(msg.photos.is_empty());
        assert!(msg.document.is_none());
        assert!(msg.reply_to_message_id.is_none());
    }

    #[test]
    fn parse_photo_message() {
        let update = json!({
            "message": {
                "message_id": 43,
                "from": { "id": 100, "is_bot": false, "first_name": "Alice" },
                "chat": { "id": 200, "type": "private" },
                "date": 1700000001,
                "caption": "Look at this",
                "photo": [
                    { "file_id": "small_id", "file_unique_id": "s", "width": 90, "height": 90 },
                    { "file_id": "large_id", "file_unique_id": "l", "width": 800, "height": 600 }
                ]
            }
        });

        let msg = parse_telegram_update(&update).unwrap();
        assert_eq!(msg.message_id, 43);
        assert_eq!(msg.caption, Some("Look at this".into()));
        assert!(msg.text.is_none());
        assert_eq!(msg.photos.len(), 2);
        assert_eq!(msg.photos[0].file_id, "small_id");
        assert_eq!(msg.photos[1].file_id, "large_id");
        assert_eq!(msg.photos[1].width, 800);
    }

    #[test]
    fn parse_document_message() {
        let update = json!({
            "message": {
                "message_id": 44,
                "from": { "id": 100, "is_bot": false, "first_name": "Bob" },
                "chat": { "id": 300, "type": "private" },
                "date": 1700000002,
                "document": {
                    "file_id": "doc_file_id",
                    "file_unique_id": "du",
                    "file_name": "report.pdf",
                    "mime_type": "application/pdf",
                    "file_size": 12345
                },
                "caption": "Here's the report"
            }
        });

        let msg = parse_telegram_update(&update).unwrap();
        let doc = msg.document.unwrap();
        assert_eq!(doc.file_id, "doc_file_id");
        assert_eq!(doc.file_name, Some("report.pdf".into()));
        assert_eq!(doc.mime_type, Some("application/pdf".into()));
    }

    #[test]
    fn parse_command_message() {
        let update = json!({
            "message": {
                "message_id": 45,
                "from": { "id": 100, "is_bot": false, "first_name": "Carol" },
                "chat": { "id": 400, "type": "private" },
                "date": 1700000003,
                "text": "/start hello world"
            }
        });

        let msg = parse_telegram_update(&update).unwrap();
        assert_eq!(msg.text, Some("/start hello world".into()));
    }

    #[test]
    fn parse_reply_message() {
        let update = json!({
            "message": {
                "message_id": 46,
                "from": { "id": 100, "is_bot": false, "first_name": "Dave" },
                "chat": { "id": 500, "type": "private" },
                "date": 1700000004,
                "text": "This is a reply",
                "reply_to_message": {
                    "message_id": 40,
                    "from": { "id": 200, "is_bot": true, "first_name": "Bot" },
                    "chat": { "id": 500, "type": "private" },
                    "date": 1700000000,
                    "text": "Original message"
                }
            }
        });

        let msg = parse_telegram_update(&update).unwrap();
        assert_eq!(msg.reply_to_message_id, Some(40));
    }

    #[test]
    fn parse_missing_fields_returns_none() {
        // No message_id
        let bad1 = json!({"message": {"chat": {"id": 1}}});
        assert!(parse_telegram_update(&bad1).is_none());

        // No chat
        let bad2 = json!({"message": {"message_id": 1}});
        assert!(parse_telegram_update(&bad2).is_none());

        // Completely empty
        let bad3 = json!({});
        assert!(parse_telegram_update(&bad3).is_none());

        // Non-message update (e.g. callback_query)
        let bad4 = json!({"update_id": 1, "callback_query": {}});
        assert!(parse_telegram_update(&bad4).is_none());
    }

    #[test]
    fn parse_bare_message_object() {
        let bare = json!({
            "message_id": 99,
            "from": { "id": 100, "is_bot": false, "first_name": "Eve" },
            "chat": { "id": 600, "type": "private" },
            "date": 1700000005,
            "text": "bare message"
        });

        let msg = parse_telegram_update(&bare).unwrap();
        assert_eq!(msg.message_id, 99);
        assert_eq!(msg.text, Some("bare message".into()));
    }

    #[test]
    fn chat_id_filtering() {
        let allowed: Option<Vec<i64>> = Some(vec![100, 200, 300]);
        assert!(is_chat_allowed(100, &allowed));
        assert!(is_chat_allowed(200, &allowed));
        assert!(!is_chat_allowed(999, &allowed));

        // None means all allowed.
        let no_filter: Option<Vec<i64>> = None;
        assert!(is_chat_allowed(999, &no_filter));
    }

    #[test]
    fn parse_message_without_sender() {
        let update = json!({
            "message": {
                "message_id": 50,
                "chat": { "id": 700, "type": "channel" },
                "date": 1700000006,
                "text": "Channel post"
            }
        });

        let msg = parse_telegram_update(&update).unwrap();
        assert!(msg.from_user_id.is_none());
        assert!(msg.from_username.is_none());
        assert!(msg.from_display_name.is_none());
    }

    // -----------------------------------------------------------------------
    // Outbound formatting tests
    // -----------------------------------------------------------------------

    #[test]
    fn format_text_block() {
        let chunk = OutboundChunk::TextBlock("Hello!".into());
        let meta = json!({"chat_id": 12345});
        let payload = format_outbound(&chunk, &meta).unwrap();
        assert_eq!(payload["method"], "sendMessage");
        assert_eq!(payload["chat_id"], 12345);
        assert_eq!(payload["text"], "Hello!");
    }

    #[test]
    fn format_media_image() {
        let chunk = OutboundChunk::Media {
            mime_type: "image/png".into(),
            data: vec![0x89, 0x50, 0x4e, 0x47],
            caption: Some("A picture".into()),
        };
        let meta = json!({"chat_id": 12345});
        let payload = format_outbound(&chunk, &meta).unwrap();
        assert_eq!(payload["method"], "sendPhoto");
        assert_eq!(payload["caption"], "A picture");
    }

    #[test]
    fn format_media_document() {
        let chunk = OutboundChunk::Media {
            mime_type: "application/pdf".into(),
            data: vec![0x25, 0x50, 0x44, 0x46],
            caption: None,
        };
        let meta = json!({"chat_id": 12345});
        let payload = format_outbound(&chunk, &meta).unwrap();
        assert_eq!(payload["method"], "sendDocument");
    }

    #[test]
    fn format_buttons() {
        use mcclawd_channels::types::InlineButton;
        let chunk = OutboundChunk::Buttons {
            text: "Choose:".into(),
            buttons: vec![vec![
                InlineButton {
                    label: "Yes".into(),
                    callback_data: Some("yes".into()),
                    url: None,
                },
                InlineButton {
                    label: "No".into(),
                    callback_data: Some("no".into()),
                    url: None,
                },
            ]],
        };
        let meta = json!({"chat_id": 12345});
        let payload = format_outbound(&chunk, &meta).unwrap();
        assert_eq!(payload["method"], "sendMessage");
        assert_eq!(payload["text"], "Choose:");
        let kb = &payload["reply_markup"]["inline_keyboard"];
        assert_eq!(kb[0][0]["text"], "Yes");
        assert_eq!(kb[0][1]["callback_data"], "no");
    }

    #[test]
    fn format_status_typing() {
        let chunk = OutboundChunk::StatusIndicator(ChannelStatus::Typing);
        let meta = json!({"chat_id": 12345});
        let payload = format_outbound(&chunk, &meta).unwrap();
        assert_eq!(payload["method"], "sendChatAction");
        assert_eq!(payload["action"], "typing");
    }

    #[test]
    fn format_status_done_returns_none() {
        let chunk = OutboundChunk::StatusIndicator(ChannelStatus::Done);
        let meta = json!({"chat_id": 12345});
        assert!(format_outbound(&chunk, &meta).is_none());
    }

    #[test]
    fn format_done_returns_none() {
        let chunk = OutboundChunk::Done;
        let meta = json!({"chat_id": 12345});
        assert!(format_outbound(&chunk, &meta).is_none());
    }

    #[test]
    fn format_missing_chat_id_returns_none() {
        let chunk = OutboundChunk::TextBlock("Hello".into());
        let meta = json!({});
        assert!(format_outbound(&chunk, &meta).is_none());
    }
}
