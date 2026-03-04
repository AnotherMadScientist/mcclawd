//! Email message normalization.
//!
//! Converts email-specific message types to the platform-agnostic [`Envelope`].
//! Uses intermediate types (`EmailMessage`, `EmailAttachment`) so normalization
//! logic can be unit-tested without live IMAP/SMTP connections.

use chrono::{DateTime, Utc};
use mcclawd_channels::envelope::{
    Attachment, Envelope, MediaRef, MessageContent, Peer, Platform, ThreadContext,
};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Intermediate types (testable without lettre / async-imap)
// ---------------------------------------------------------------------------

/// Intermediate representation of an email message for testable normalization.
#[derive(Debug, Clone)]
pub struct EmailMessage {
    /// RFC 2822 Message-ID header.
    pub message_id: String,
    /// Sender email address.
    pub from_address: String,
    /// Sender display name (from the `From:` header).
    pub from_name: Option<String>,
    /// Email subject line.
    pub subject: Option<String>,
    /// Plain text body (text/plain part).
    pub body_text: Option<String>,
    /// HTML body (text/html part).
    pub body_html: Option<String>,
    /// `In-Reply-To` header value for threading.
    pub in_reply_to: Option<String>,
    /// File attachments.
    pub attachments: Vec<EmailAttachment>,
    /// When the message was sent (from the `Date:` header).
    pub date: DateTime<Utc>,
}

/// A file attachment from an email message.
#[derive(Debug, Clone)]
pub struct EmailAttachment {
    /// Original filename from the MIME part.
    pub filename: String,
    /// MIME content type (e.g. "application/pdf").
    pub content_type: String,
    /// Raw attachment data.
    pub data: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Normalization
// ---------------------------------------------------------------------------

/// Convert an [`EmailMessage`] to a normalized [`Envelope`].
///
/// - Prefers `body_text` over `body_html` for the content field.
/// - Maps `in_reply_to` to [`ThreadContext`] for conversation threading.
/// - Stores `message_id` and `subject` in `platform_meta` for outbound routing.
/// - Attachments use `MediaRef::PlatformId` with a placeholder reference.
pub fn normalize(msg: &EmailMessage) -> Envelope {
    let peer = Peer {
        id: msg.from_address.clone(),
        display_name: msg.from_name.clone(),
        platform: Platform::Email,
    };

    // Text: prefer plain text, fall back to HTML, then empty string.
    let text = msg
        .body_text
        .clone()
        .or_else(|| msg.body_html.clone())
        .unwrap_or_default();
    let content = MessageContent::Text(text);

    // Attachments: map EmailAttachment -> Attachment with placeholder MediaRef.
    let attachments = msg
        .attachments
        .iter()
        .map(|att| Attachment {
            filename: Some(att.filename.clone()),
            mime_type: att.content_type.clone(),
            media_ref: MediaRef::PlatformId(format!("email-att-{}", att.filename)),
        })
        .collect();

    // Thread context from In-Reply-To header.
    let thread = msg.in_reply_to.as_ref().map(|reply_id| ThreadContext {
        thread_id: reply_id.clone(),
        parent_message_id: None,
    });

    // Platform metadata for outbound routing.
    let platform_meta = serde_json::json!({
        "message_id": msg.message_id,
        "subject": msg.subject,
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

    fn sample_text_email() -> EmailMessage {
        EmailMessage {
            message_id: "<abc123@example.com>".into(),
            from_address: "alice@example.com".into(),
            from_name: Some("Alice Smith".into()),
            subject: Some("Hello from Alice".into()),
            body_text: Some("Hi there, this is a plain text email.".into()),
            body_html: None,
            in_reply_to: None,
            attachments: vec![],
            date: Utc::now(),
        }
    }

    #[test]
    fn normalize_plain_text_email() {
        let msg = sample_text_email();
        let env = normalize(&msg);

        assert_eq!(env.peer.platform, Platform::Email);
        assert_eq!(env.peer.id, "alice@example.com");
        assert_eq!(env.peer.display_name, Some("Alice Smith".into()));
        assert!(
            matches!(&env.content, MessageContent::Text(t) if t == "Hi there, this is a plain text email.")
        );
        assert!(env.attachments.is_empty());
        assert!(env.thread.is_none());
    }

    #[test]
    fn normalize_subject_in_platform_meta() {
        let msg = sample_text_email();
        let env = normalize(&msg);

        assert_eq!(env.platform_meta["message_id"], "<abc123@example.com>");
        assert_eq!(env.platform_meta["subject"], "Hello from Alice");
    }

    #[test]
    fn normalize_reply_threading() {
        let mut msg = sample_text_email();
        msg.in_reply_to = Some("<parent456@example.com>".into());

        let env = normalize(&msg);

        assert!(env.thread.is_some());
        let thread = env.thread.unwrap();
        assert_eq!(thread.thread_id, "<parent456@example.com>");
        assert!(thread.parent_message_id.is_none());
    }

    #[test]
    fn normalize_attachments() {
        let mut msg = sample_text_email();
        msg.attachments = vec![
            EmailAttachment {
                filename: "report.pdf".into(),
                content_type: "application/pdf".into(),
                data: vec![0x25, 0x50, 0x44, 0x46], // %PDF
            },
            EmailAttachment {
                filename: "photo.jpg".into(),
                content_type: "image/jpeg".into(),
                data: vec![0xFF, 0xD8, 0xFF, 0xE0],
            },
        ];

        let env = normalize(&msg);

        assert_eq!(env.attachments.len(), 2);
        assert_eq!(env.attachments[0].filename, Some("report.pdf".into()));
        assert_eq!(env.attachments[0].mime_type, "application/pdf");
        assert!(matches!(
            &env.attachments[0].media_ref,
            MediaRef::PlatformId(id) if id == "email-att-report.pdf"
        ));
        assert_eq!(env.attachments[1].filename, Some("photo.jpg".into()));
        assert_eq!(env.attachments[1].mime_type, "image/jpeg");
        assert!(matches!(
            &env.attachments[1].media_ref,
            MediaRef::PlatformId(id) if id == "email-att-photo.jpg"
        ));
    }

    #[test]
    fn normalize_html_fallback() {
        let mut msg = sample_text_email();
        msg.body_text = None;
        msg.body_html = Some("<p>Hello from HTML</p>".into());

        let env = normalize(&msg);

        assert!(
            matches!(&env.content, MessageContent::Text(t) if t == "<p>Hello from HTML</p>")
        );
    }

    #[test]
    fn normalize_empty_body() {
        let mut msg = sample_text_email();
        msg.body_text = None;
        msg.body_html = None;

        let env = normalize(&msg);

        assert!(matches!(&env.content, MessageContent::Text(t) if t.is_empty()));
    }

    #[test]
    fn normalize_prefers_plain_text_over_html() {
        let mut msg = sample_text_email();
        msg.body_text = Some("Plain text version".into());
        msg.body_html = Some("<p>HTML version</p>".into());

        let env = normalize(&msg);

        assert!(
            matches!(&env.content, MessageContent::Text(t) if t == "Plain text version")
        );
    }

    #[test]
    fn normalize_generates_unique_ids() {
        let msg = sample_text_email();
        let env1 = normalize(&msg);
        let env2 = normalize(&msg);
        assert_ne!(env1.id, env2.id);
    }

    #[test]
    fn normalize_no_reply_has_no_thread() {
        let msg = sample_text_email();
        let env = normalize(&msg);
        assert!(env.thread.is_none());
    }

    #[test]
    fn normalize_no_display_name() {
        let mut msg = sample_text_email();
        msg.from_name = None;

        let env = normalize(&msg);

        assert_eq!(env.peer.id, "alice@example.com");
        assert_eq!(env.peer.display_name, None);
    }

    #[test]
    fn normalize_no_subject() {
        let mut msg = sample_text_email();
        msg.subject = None;

        let env = normalize(&msg);

        assert!(env.platform_meta["subject"].is_null());
    }
}
