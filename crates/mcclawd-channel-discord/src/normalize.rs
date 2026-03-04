//! Discord message normalization.
//!
//! Converts Discord-specific message types to the platform-agnostic [`Envelope`].
//! Uses intermediate types (`DiscordMessage`, `DiscordAttachment`) so normalization
//! logic can be unit-tested without a live Discord bot or serenity dependency.

use chrono::{DateTime, Utc};
use mcclawd_channels::envelope::{
    Attachment, Envelope, MediaRef, MessageContent, Peer, Platform, ThreadContext,
};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Intermediate types (testable without serenity)
// ---------------------------------------------------------------------------

/// Intermediate representation of a Discord message for testable normalization.
#[derive(Debug, Clone)]
pub struct DiscordMessage {
    /// Discord message snowflake ID.
    pub message_id: String,
    /// Discord channel snowflake ID.
    pub channel_id: String,
    /// Discord guild (server) snowflake ID. `None` for DMs.
    pub guild_id: Option<String>,
    /// Discord user snowflake ID of the author.
    pub author_id: String,
    /// Display name of the author.
    pub author_name: String,
    /// Text content of the message.
    pub content: String,
    /// File attachments on the message.
    pub attachments: Vec<DiscordAttachment>,
    /// When the message was sent.
    pub timestamp: DateTime<Utc>,
}

/// A file attachment from a Discord message.
#[derive(Debug, Clone)]
pub struct DiscordAttachment {
    /// Original filename.
    pub filename: String,
    /// CDN URL to download the attachment.
    pub url: String,
    /// MIME content type, if known.
    pub content_type: Option<String>,
}

// ---------------------------------------------------------------------------
// Normalization
// ---------------------------------------------------------------------------

/// Convert a [`DiscordMessage`] to a normalized [`Envelope`].
///
/// - Maps `/command args` messages to `MessageContent::Command`.
/// - Stores `channel_id`, `guild_id`, and `message_id` in `platform_meta`.
/// - Attachments are mapped to `Attachment` with `MediaRef::Url`.
pub fn normalize(msg: &DiscordMessage) -> Envelope {
    let peer = Peer {
        id: msg.author_id.clone(),
        display_name: Some(msg.author_name.clone()),
        platform: Platform::Discord,
    };

    // Parse content: if it starts with "/" treat as a command.
    let content = if msg.content.starts_with('/') {
        let trimmed = &msg.content[1..];
        let mut parts = trimmed.splitn(2, ' ');
        let name = parts.next().unwrap_or("").to_string();
        let args = parts.next().unwrap_or("").to_string();
        MessageContent::Command { name, args }
    } else {
        MessageContent::Text(msg.content.clone())
    };

    // Map attachments.
    let attachments: Vec<Attachment> = msg
        .attachments
        .iter()
        .map(|a| Attachment {
            filename: Some(a.filename.clone()),
            mime_type: a
                .content_type
                .clone()
                .unwrap_or_else(|| "application/octet-stream".to_string()),
            media_ref: MediaRef::Url(a.url.clone()),
        })
        .collect();

    // Thread context: always set channel_id as thread_id.
    let thread = Some(ThreadContext {
        thread_id: msg.channel_id.clone(),
        parent_message_id: None,
    });

    // Platform metadata for outbound routing.
    let platform_meta = serde_json::json!({
        "channel_id": msg.channel_id,
        "guild_id": msg.guild_id,
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

    fn sample_text_message() -> DiscordMessage {
        DiscordMessage {
            message_id: "1234567890".into(),
            channel_id: "9876543210".into(),
            guild_id: Some("1111111111".into()),
            author_id: "42".into(),
            author_name: "TestUser".into(),
            content: "Hello, agent!".into(),
            attachments: vec![],
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn normalize_text_message() {
        let msg = sample_text_message();
        let env = normalize(&msg);

        assert_eq!(env.peer.platform, Platform::Discord);
        assert_eq!(env.peer.id, "42");
        assert_eq!(env.peer.display_name, Some("TestUser".into()));
        assert!(matches!(&env.content, MessageContent::Text(t) if t == "Hello, agent!"));
        assert!(env.attachments.is_empty());
        assert!(env.thread.is_some());
    }

    #[test]
    fn normalize_command_parsing() {
        let mut msg = sample_text_message();
        msg.content = "/ask what time is it".into();

        let env = normalize(&msg);

        match &env.content {
            MessageContent::Command { name, args } => {
                assert_eq!(name, "ask");
                assert_eq!(args, "what time is it");
            }
            other => panic!("Expected Command, got {:?}", other),
        }
    }

    #[test]
    fn normalize_command_no_args() {
        let mut msg = sample_text_message();
        msg.content = "/help".into();

        let env = normalize(&msg);

        match &env.content {
            MessageContent::Command { name, args } => {
                assert_eq!(name, "help");
                assert_eq!(args, "");
            }
            other => panic!("Expected Command, got {:?}", other),
        }
    }

    #[test]
    fn normalize_attachments() {
        let mut msg = sample_text_message();
        msg.attachments = vec![DiscordAttachment {
            filename: "report.pdf".into(),
            url: "https://cdn.discordapp.com/attachments/123/456/report.pdf".into(),
            content_type: Some("application/pdf".into()),
        }];

        let env = normalize(&msg);

        assert_eq!(env.attachments.len(), 1);
        assert_eq!(env.attachments[0].filename, Some("report.pdf".into()));
        assert_eq!(env.attachments[0].mime_type, "application/pdf");
        assert!(matches!(
            &env.attachments[0].media_ref,
            MediaRef::Url(u) if u.contains("report.pdf")
        ));
    }

    #[test]
    fn normalize_attachment_without_content_type_defaults_to_octet_stream() {
        let mut msg = sample_text_message();
        msg.attachments = vec![DiscordAttachment {
            filename: "data.bin".into(),
            url: "https://cdn.discordapp.com/attachments/123/456/data.bin".into(),
            content_type: None,
        }];

        let env = normalize(&msg);

        assert_eq!(env.attachments[0].mime_type, "application/octet-stream");
    }

    #[test]
    fn normalize_empty_content() {
        let mut msg = sample_text_message();
        msg.content = "".into();

        let env = normalize(&msg);

        assert!(matches!(&env.content, MessageContent::Text(t) if t.is_empty()));
    }

    #[test]
    fn normalize_no_guild_dm() {
        let mut msg = sample_text_message();
        msg.guild_id = None;

        let env = normalize(&msg);

        assert_eq!(env.peer.platform, Platform::Discord);
        assert!(env.platform_meta["guild_id"].is_null());
    }

    #[test]
    fn normalize_multiple_attachments() {
        let mut msg = sample_text_message();
        msg.attachments = vec![
            DiscordAttachment {
                filename: "image.png".into(),
                url: "https://cdn.discordapp.com/attachments/1/2/image.png".into(),
                content_type: Some("image/png".into()),
            },
            DiscordAttachment {
                filename: "doc.pdf".into(),
                url: "https://cdn.discordapp.com/attachments/1/2/doc.pdf".into(),
                content_type: Some("application/pdf".into()),
            },
            DiscordAttachment {
                filename: "unknown.dat".into(),
                url: "https://cdn.discordapp.com/attachments/1/2/unknown.dat".into(),
                content_type: None,
            },
        ];

        let env = normalize(&msg);

        assert_eq!(env.attachments.len(), 3);
        assert_eq!(env.attachments[0].mime_type, "image/png");
        assert_eq!(env.attachments[1].mime_type, "application/pdf");
        assert_eq!(env.attachments[2].mime_type, "application/octet-stream");
    }

    #[test]
    fn normalize_platform_meta_contains_ids() {
        let msg = sample_text_message();
        let env = normalize(&msg);

        assert_eq!(env.platform_meta["channel_id"], "9876543210");
        assert_eq!(env.platform_meta["guild_id"], "1111111111");
        assert_eq!(env.platform_meta["message_id"], "1234567890");
    }

    #[test]
    fn normalize_generates_unique_ids() {
        let msg = sample_text_message();
        let env1 = normalize(&msg);
        let env2 = normalize(&msg);
        assert_ne!(env1.id, env2.id);
    }

    #[test]
    fn normalize_thread_context_uses_channel_id() {
        let msg = sample_text_message();
        let env = normalize(&msg);

        let thread = env.thread.unwrap();
        assert_eq!(thread.thread_id, "9876543210");
        assert!(thread.parent_message_id.is_none());
    }
}
