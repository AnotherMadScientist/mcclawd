//! WhatsApp message normalization.
//!
//! Converts WhatsApp-specific message types to the platform-agnostic [`Envelope`].
//! Uses intermediate types (`WhatsAppMessage`, `WhatsAppMedia`) so normalization
//! logic can be unit-tested without a live WhatsApp webhook or Cloud API.

use chrono::{DateTime, Utc};
use mcclawd_channels::envelope::{
    Attachment, Envelope, MediaRef, MessageContent, Peer, Platform, ThreadContext,
};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Intermediate types (testable without WhatsApp Cloud API)
// ---------------------------------------------------------------------------

/// Intermediate representation of a WhatsApp message for testable normalization.
#[derive(Debug, Clone)]
pub struct WhatsAppMessage {
    /// WhatsApp message ID (wamid.*).
    pub message_id: String,
    /// Sender phone number (E.164 format, e.g. "14155552671").
    pub from: String,
    /// Sender profile name, if available.
    pub from_name: Option<String>,
    /// Text body of the message, if any.
    pub text: Option<String>,
    /// Media attachment, if any.
    pub media: Option<WhatsAppMedia>,
    /// When the message was sent.
    pub timestamp: DateTime<Utc>,
}

/// A media attachment from WhatsApp.
#[derive(Debug, Clone)]
pub struct WhatsAppMedia {
    /// WhatsApp media ID (used to download via Cloud API).
    pub id: String,
    /// MIME type of the media (e.g. "image/jpeg", "application/pdf").
    pub mime_type: String,
    /// Original filename, if provided (documents only).
    pub filename: Option<String>,
}

// ---------------------------------------------------------------------------
// Normalization
// ---------------------------------------------------------------------------

/// Convert a [`WhatsAppMessage`] to a normalized [`Envelope`].
///
/// - `peer.id` is the sender's phone number.
/// - `peer.display_name` is the sender's profile name.
/// - `peer.platform` is `Platform::WhatsApp`.
/// - `thread` is always `None` (WhatsApp doesn't expose thread IDs in the
///   same way as Telegram/Slack).
/// - `content` is `Text(text)` or `Command { name, args }` if text starts
///   with `/`.
/// - `attachments`: if `media` is present, includes a single attachment with
///   `MediaRef::PlatformId(media.id)`.
/// - `platform_meta` stores `message_id` for outbound routing.
pub fn normalize(msg: &WhatsAppMessage) -> Envelope {
    let peer = Peer {
        id: msg.from.clone(),
        display_name: msg.from_name.clone(),
        platform: Platform::WhatsApp,
    };

    // Determine content: text, command, or empty fallback.
    let raw_text = msg.text.clone().unwrap_or_default();
    let content = if raw_text.starts_with('/') {
        // Parse as command: "/ask what time is it" -> name="ask", args="what time is it"
        let trimmed = &raw_text[1..]; // strip leading '/'
        let (name, args) = match trimmed.split_once(' ') {
            Some((n, a)) => (n.to_string(), a.to_string()),
            None => (trimmed.to_string(), String::new()),
        };
        MessageContent::Command { name, args }
    } else {
        MessageContent::Text(raw_text)
    };

    // Attachments from media.
    let attachments = match &msg.media {
        Some(media) => vec![Attachment {
            filename: media.filename.clone(),
            mime_type: media.mime_type.clone(),
            media_ref: MediaRef::PlatformId(media.id.clone()),
        }],
        None => vec![],
    };

    // WhatsApp doesn't have threads in the Telegram/Slack sense.
    let thread: Option<ThreadContext> = None;

    // Platform metadata for outbound routing.
    let platform_meta = serde_json::json!({
        "message_id": msg.message_id,
    });

    Envelope {
        id: Uuid::new_v4().to_string(),
        peer,
        thread,
        content,
        attachments,
        timestamp: msg.timestamp,
        platform_meta,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_text_message() -> WhatsAppMessage {
        WhatsAppMessage {
            message_id: "wamid.abc123".into(),
            from: "14155552671".into(),
            from_name: Some("Alice Smith".into()),
            text: Some("Hello, agent!".into()),
            media: None,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn normalize_text_message() {
        let msg = sample_text_message();
        let env = normalize(&msg);

        assert_eq!(env.peer.platform, Platform::WhatsApp);
        assert_eq!(env.peer.id, "14155552671");
        assert_eq!(env.peer.display_name, Some("Alice Smith".into()));
        assert!(matches!(&env.content, MessageContent::Text(t) if t == "Hello, agent!"));
        assert!(env.attachments.is_empty());
        assert!(env.thread.is_none());
    }

    #[test]
    fn normalize_media_message() {
        let msg = WhatsAppMessage {
            message_id: "wamid.media456".into(),
            from: "14155559999".into(),
            from_name: Some("Bob".into()),
            text: None,
            media: Some(WhatsAppMedia {
                id: "media-id-789".into(),
                mime_type: "image/jpeg".into(),
                filename: None,
            }),
            timestamp: Utc::now(),
        };

        let env = normalize(&msg);

        assert_eq!(env.attachments.len(), 1);
        assert_eq!(env.attachments[0].mime_type, "image/jpeg");
        assert!(env.attachments[0].filename.is_none());
        assert!(
            matches!(&env.attachments[0].media_ref, MediaRef::PlatformId(id) if id == "media-id-789")
        );
        // No text -> empty string
        assert!(matches!(&env.content, MessageContent::Text(t) if t.is_empty()));
    }

    #[test]
    fn normalize_command_message() {
        let msg = WhatsAppMessage {
            message_id: "wamid.cmd001".into(),
            from: "14155551234".into(),
            from_name: None,
            text: Some("/ask what time is it".into()),
            media: None,
            timestamp: Utc::now(),
        };

        let env = normalize(&msg);

        assert!(matches!(
            &env.content,
            MessageContent::Command { name, args }
            if name == "ask" && args == "what time is it"
        ));
    }

    #[test]
    fn normalize_command_no_args() {
        let msg = WhatsAppMessage {
            message_id: "wamid.cmd002".into(),
            from: "14155551234".into(),
            from_name: None,
            text: Some("/start".into()),
            media: None,
            timestamp: Utc::now(),
        };

        let env = normalize(&msg);

        assert!(matches!(
            &env.content,
            MessageContent::Command { name, args }
            if name == "start" && args.is_empty()
        ));
    }

    #[test]
    fn normalize_no_text_no_media() {
        let msg = WhatsAppMessage {
            message_id: "wamid.empty".into(),
            from: "14155550000".into(),
            from_name: None,
            text: None,
            media: None,
            timestamp: Utc::now(),
        };

        let env = normalize(&msg);

        assert!(matches!(&env.content, MessageContent::Text(t) if t.is_empty()));
        assert!(env.attachments.is_empty());
    }

    #[test]
    fn normalize_text_with_media() {
        let msg = WhatsAppMessage {
            message_id: "wamid.both001".into(),
            from: "14155553333".into(),
            from_name: Some("Charlie".into()),
            text: Some("Check this document".into()),
            media: Some(WhatsAppMedia {
                id: "doc-media-id".into(),
                mime_type: "application/pdf".into(),
                filename: Some("report.pdf".into()),
            }),
            timestamp: Utc::now(),
        };

        let env = normalize(&msg);

        assert!(matches!(&env.content, MessageContent::Text(t) if t == "Check this document"));
        assert_eq!(env.attachments.len(), 1);
        assert_eq!(env.attachments[0].filename, Some("report.pdf".into()));
        assert_eq!(env.attachments[0].mime_type, "application/pdf");
        assert!(
            matches!(&env.attachments[0].media_ref, MediaRef::PlatformId(id) if id == "doc-media-id")
        );
    }

    #[test]
    fn normalize_platform_meta_contains_message_id() {
        let msg = sample_text_message();
        let env = normalize(&msg);

        assert_eq!(env.platform_meta["message_id"], "wamid.abc123");
    }

    #[test]
    fn normalize_thread_is_always_none() {
        let msg = sample_text_message();
        let env = normalize(&msg);
        assert!(env.thread.is_none());
    }

    #[test]
    fn normalize_generates_unique_ids() {
        let msg = sample_text_message();
        let env1 = normalize(&msg);
        let env2 = normalize(&msg);
        assert_ne!(env1.id, env2.id);
    }

    #[test]
    fn normalize_missing_sender_name() {
        let msg = WhatsAppMessage {
            message_id: "wamid.noname".into(),
            from: "14155550001".into(),
            from_name: None,
            text: Some("anonymous".into()),
            media: None,
            timestamp: Utc::now(),
        };

        let env = normalize(&msg);

        assert_eq!(env.peer.id, "14155550001");
        assert_eq!(env.peer.display_name, None);
    }
}
