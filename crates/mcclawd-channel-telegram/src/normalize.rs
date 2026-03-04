//! Telegram message normalization.
//!
//! Converts Telegram-specific message types to the platform-agnostic [`Envelope`].
//! Uses intermediate types (`TelegramMessage`, etc.) so normalization logic can be
//! unit-tested without a live Telegram bot or teloxide dependency.

use chrono::{DateTime, Utc};
use mcclawd_channels::envelope::{Attachment, Envelope, MediaRef, MessageContent, Peer, Platform, ThreadContext};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Intermediate types (testable without teloxide)
// ---------------------------------------------------------------------------

/// Intermediate representation of a Telegram message for testable normalization.
#[derive(Debug, Clone)]
pub struct TelegramMessage {
    /// Telegram message ID within the chat.
    pub message_id: i64,
    /// Telegram chat ID.
    pub chat_id: i64,
    /// Telegram user ID of the sender, if any.
    pub from_user_id: Option<i64>,
    /// Telegram username (without @).
    pub from_username: Option<String>,
    /// Display name of the sender.
    pub from_display_name: Option<String>,
    /// Text content of the message.
    pub text: Option<String>,
    /// Caption for media messages.
    pub caption: Option<String>,
    /// ID of the message being replied to.
    pub reply_to_message_id: Option<i64>,
    /// When the message was sent.
    pub date: DateTime<Utc>,
    /// Photo sizes (Telegram sends multiple resolutions).
    pub photos: Vec<TelegramPhoto>,
    /// Document attachment, if any.
    pub document: Option<TelegramDocument>,
}

/// A single photo size from Telegram (multiple resolutions per photo message).
#[derive(Debug, Clone)]
pub struct TelegramPhoto {
    /// Telegram file_id for downloading.
    pub file_id: String,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// A document attachment from Telegram.
#[derive(Debug, Clone)]
pub struct TelegramDocument {
    /// Telegram file_id for downloading.
    pub file_id: String,
    /// Original filename, if provided.
    pub file_name: Option<String>,
    /// MIME type, if known.
    pub mime_type: Option<String>,
}

// ---------------------------------------------------------------------------
// Normalization
// ---------------------------------------------------------------------------

/// Convert a [`TelegramMessage`] to a normalized [`Envelope`].
///
/// - Picks the largest photo (last in the `photos` vec, as Telegram orders small→large).
/// - Uses `caption` as text content for media messages when `text` is absent.
/// - Stores `chat_id` and `message_id` in `platform_meta` for outbound routing.
pub fn normalize(msg: &TelegramMessage) -> Envelope {
    let peer = Peer {
        id: msg
            .from_user_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        display_name: msg.from_display_name.clone(),
        platform: Platform::Telegram,
    };

    // Text: prefer `text`, fall back to `caption`, then empty string.
    let text = msg
        .text
        .clone()
        .or_else(|| msg.caption.clone())
        .unwrap_or_default();
    let content = MessageContent::Text(text);

    // Attachments: largest photo + optional document.
    let mut attachments = Vec::new();

    if let Some(photo) = msg.photos.last() {
        attachments.push(Attachment {
            filename: None,
            mime_type: "image/jpeg".to_string(),
            media_ref: MediaRef::PlatformId(photo.file_id.clone()),
        });
    }

    if let Some(doc) = &msg.document {
        attachments.push(Attachment {
            filename: doc.file_name.clone(),
            mime_type: doc
                .mime_type
                .clone()
                .unwrap_or_else(|| "application/octet-stream".to_string()),
            media_ref: MediaRef::PlatformId(doc.file_id.clone()),
        });
    }

    // Thread context from reply_to.
    let thread = msg.reply_to_message_id.map(|reply_id| ThreadContext {
        thread_id: msg.chat_id.to_string(),
        parent_message_id: Some(reply_id.to_string()),
    });

    // Platform metadata for outbound routing.
    let platform_meta = serde_json::json!({
        "chat_id": msg.chat_id,
        "message_id": msg.message_id,
        "username": msg.from_username,
    });

    Envelope {
        id: Uuid::new_v4().to_string(),
        peer,
        thread,
        content,
        attachments,
        timestamp: msg.date,
        platform_meta,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_text_message() -> TelegramMessage {
        TelegramMessage {
            message_id: 42,
            chat_id: -1001234567890,
            from_user_id: Some(123456),
            from_username: Some("testuser".into()),
            from_display_name: Some("Test User".into()),
            text: Some("Hello, agent!".into()),
            caption: None,
            reply_to_message_id: None,
            date: Utc::now(),
            photos: vec![],
            document: None,
        }
    }

    #[test]
    fn normalize_text_message() {
        let msg = sample_text_message();
        let env = normalize(&msg);

        assert_eq!(env.peer.platform, Platform::Telegram);
        assert_eq!(env.peer.id, "123456");
        assert_eq!(env.peer.display_name, Some("Test User".into()));
        assert!(matches!(&env.content, MessageContent::Text(t) if t == "Hello, agent!"));
        assert!(env.attachments.is_empty());
        assert!(env.thread.is_none());
    }

    #[test]
    fn normalize_photo_message_picks_largest() {
        let mut msg = sample_text_message();
        msg.text = None;
        msg.caption = Some("Check this out".into());
        msg.photos = vec![
            TelegramPhoto {
                file_id: "small".into(),
                width: 100,
                height: 100,
            },
            TelegramPhoto {
                file_id: "large".into(),
                width: 800,
                height: 600,
            },
        ];

        let env = normalize(&msg);

        // Caption becomes text content.
        assert!(matches!(&env.content, MessageContent::Text(t) if t == "Check this out"));
        // Only the largest photo is attached.
        assert_eq!(env.attachments.len(), 1);
        assert!(
            matches!(&env.attachments[0].media_ref, MediaRef::PlatformId(id) if id == "large")
        );
        assert_eq!(env.attachments[0].mime_type, "image/jpeg");
    }

    #[test]
    fn normalize_document_message() {
        let mut msg = sample_text_message();
        msg.text = None;
        msg.caption = Some("Here's a file".into());
        msg.document = Some(TelegramDocument {
            file_id: "doc123".into(),
            file_name: Some("report.pdf".into()),
            mime_type: Some("application/pdf".into()),
        });

        let env = normalize(&msg);

        assert_eq!(env.attachments.len(), 1);
        assert_eq!(env.attachments[0].filename, Some("report.pdf".into()));
        assert_eq!(env.attachments[0].mime_type, "application/pdf");
        assert!(
            matches!(&env.attachments[0].media_ref, MediaRef::PlatformId(id) if id == "doc123")
        );
    }

    #[test]
    fn normalize_document_without_mime_defaults_to_octet_stream() {
        let mut msg = sample_text_message();
        msg.text = None;
        msg.document = Some(TelegramDocument {
            file_id: "bin123".into(),
            file_name: Some("data.bin".into()),
            mime_type: None,
        });

        let env = normalize(&msg);

        assert_eq!(env.attachments[0].mime_type, "application/octet-stream");
    }

    #[test]
    fn normalize_reply_message_has_thread_context() {
        let mut msg = sample_text_message();
        msg.reply_to_message_id = Some(10);

        let env = normalize(&msg);

        assert!(env.thread.is_some());
        let thread = env.thread.unwrap();
        assert_eq!(thread.thread_id, "-1001234567890");
        assert_eq!(thread.parent_message_id, Some("10".into()));
    }

    #[test]
    fn normalize_no_reply_has_no_thread() {
        let msg = sample_text_message();
        let env = normalize(&msg);
        assert!(env.thread.is_none());
    }

    #[test]
    fn normalize_platform_meta_contains_chat_id() {
        let msg = sample_text_message();
        let env = normalize(&msg);

        assert_eq!(env.platform_meta["chat_id"], -1001234567890_i64);
        assert_eq!(env.platform_meta["message_id"], 42);
        assert_eq!(env.platform_meta["username"], "testuser");
    }

    #[test]
    fn normalize_missing_sender_uses_empty_id() {
        let mut msg = sample_text_message();
        msg.from_user_id = None;
        msg.from_display_name = None;

        let env = normalize(&msg);

        assert_eq!(env.peer.id, "");
        assert_eq!(env.peer.display_name, None);
    }

    #[test]
    fn normalize_generates_unique_ids() {
        let msg = sample_text_message();
        let env1 = normalize(&msg);
        let env2 = normalize(&msg);
        assert_ne!(env1.id, env2.id);
    }

    #[test]
    fn normalize_empty_text_and_no_caption() {
        let mut msg = sample_text_message();
        msg.text = None;
        msg.caption = None;

        let env = normalize(&msg);
        assert!(matches!(&env.content, MessageContent::Text(t) if t.is_empty()));
    }

    #[test]
    fn normalize_photo_and_document_together() {
        let mut msg = sample_text_message();
        msg.photos = vec![TelegramPhoto {
            file_id: "photo1".into(),
            width: 400,
            height: 300,
        }];
        msg.document = Some(TelegramDocument {
            file_id: "doc1".into(),
            file_name: Some("readme.txt".into()),
            mime_type: Some("text/plain".into()),
        });

        let env = normalize(&msg);

        // Both photo and document are attached.
        assert_eq!(env.attachments.len(), 2);
        assert!(
            matches!(&env.attachments[0].media_ref, MediaRef::PlatformId(id) if id == "photo1")
        );
        assert!(
            matches!(&env.attachments[1].media_ref, MediaRef::PlatformId(id) if id == "doc1")
        );
    }
}
