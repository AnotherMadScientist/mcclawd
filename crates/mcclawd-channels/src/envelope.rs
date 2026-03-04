//! Phase 2 Envelope message format.
//!
//! The `Envelope` is the normalized message type that flows through the inbound pipeline.
//! Every channel adapter produces Envelopes; the pipeline never sees platform-specific types.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Platform
// ---------------------------------------------------------------------------

/// Supported communication platforms.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Platform {
    Cli,
    Web,
    Telegram,
    Discord,
    Slack,
    Matrix,
    Email,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::Cli => write!(f, "cli"),
            Platform::Web => write!(f, "web"),
            Platform::Telegram => write!(f, "telegram"),
            Platform::Discord => write!(f, "discord"),
            Platform::Slack => write!(f, "slack"),
            Platform::Matrix => write!(f, "matrix"),
            Platform::Email => write!(f, "email"),
        }
    }
}

// ---------------------------------------------------------------------------
// Peer
// ---------------------------------------------------------------------------

/// Normalized peer identity across all platforms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    /// Platform-scoped user identifier (e.g. Telegram user_id, Discord user_id).
    pub id: String,
    /// Human-readable display name.
    pub display_name: Option<String>,
    /// Which platform this peer is on.
    pub platform: Platform,
}

// ---------------------------------------------------------------------------
// ThreadContext
// ---------------------------------------------------------------------------

/// Conversation threading metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadContext {
    /// Platform-specific thread or conversation ID.
    pub thread_id: String,
    /// ID of the message being replied to, if any.
    pub parent_message_id: Option<String>,
}

// ---------------------------------------------------------------------------
// MessageContent
// ---------------------------------------------------------------------------

/// The payload of an inbound message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageContent {
    /// Plain text or markdown.
    Text(String),
    /// Bot command (e.g. /start, /ask).
    Command { name: String, args: String },
    /// Voice message with a media reference.
    Voice(MediaRef),
    /// Location share.
    Location { lat: f64, lon: f64 },
}

// ---------------------------------------------------------------------------
// Media types
// ---------------------------------------------------------------------------

/// Reference to media content. Can be local, remote, or platform-managed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MediaRef {
    /// Path on the local filesystem.
    Local(PathBuf),
    /// HTTP(S) URL.
    Url(String),
    /// Opaque platform-specific identifier (e.g. Telegram file_id).
    PlatformId(String),
}

/// A file or media attachment on a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    /// Original filename, if known.
    pub filename: Option<String>,
    /// MIME type (e.g. "image/png").
    pub mime_type: String,
    /// Where the media bytes live.
    pub media_ref: MediaRef,
}

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

/// Normalized message flowing through the inbound pipeline.
///
/// Every channel adapter produces these; the pipeline never sees
/// platform-specific types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// Globally unique message ID.
    pub id: String,
    /// Who sent this message.
    pub peer: Peer,
    /// Conversation threading context.
    pub thread: Option<ThreadContext>,
    /// The actual message payload.
    pub content: MessageContent,
    /// Attached media (images, files, voice notes).
    pub attachments: Vec<Attachment>,
    /// When the message was created (UTC).
    pub timestamp: DateTime<Utc>,
    /// Opaque platform-specific metadata preserved for outbound routing.
    /// Serialised as a JSON value so each adapter can store whatever it needs.
    pub platform_meta: serde_json::Value,
}

impl Envelope {
    /// Create a new `Envelope` with a generated UUID and the current timestamp.
    pub fn new(peer: Peer, content: MessageContent) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            peer,
            thread: None,
            content,
            attachments: Vec::new(),
            timestamp: Utc::now(),
            platform_meta: serde_json::Value::Null,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_serde_roundtrip() {
        let envelope = Envelope {
            id: "msg-001".into(),
            peer: Peer {
                id: "user-42".into(),
                display_name: Some("Alice".into()),
                platform: Platform::Telegram,
            },
            thread: Some(ThreadContext {
                thread_id: "thread-1".into(),
                parent_message_id: Some("msg-000".into()),
            }),
            content: MessageContent::Text("Hello, world!".into()),
            attachments: vec![Attachment {
                filename: Some("photo.png".into()),
                mime_type: "image/png".into(),
                media_ref: MediaRef::Url("https://example.com/photo.png".into()),
            }],
            timestamp: Utc::now(),
            platform_meta: serde_json::json!({"chat_id": 12345}),
        };

        let json = serde_json::to_string(&envelope).expect("serialize");
        let back: Envelope = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.id, "msg-001");
        assert_eq!(back.peer.id, "user-42");
        assert_eq!(back.peer.platform, Platform::Telegram);
        assert!(back.thread.is_some());
        assert_eq!(back.attachments.len(), 1);
    }

    #[test]
    fn platform_all_variants_display() {
        let variants = vec![
            (Platform::Cli, "cli"),
            (Platform::Web, "web"),
            (Platform::Telegram, "telegram"),
            (Platform::Discord, "discord"),
            (Platform::Slack, "slack"),
            (Platform::Matrix, "matrix"),
            (Platform::Email, "email"),
        ];
        for (platform, expected) in variants {
            assert_eq!(platform.to_string(), expected);
        }
    }

    #[test]
    fn platform_serde_roundtrip() {
        let original = Platform::Matrix;
        let json = serde_json::to_string(&original).expect("serialize");
        let back: Platform = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }

    #[test]
    fn message_content_variants_serde() {
        let text = MessageContent::Text("hi".into());
        let json = serde_json::to_string(&text).unwrap();
        let _: MessageContent = serde_json::from_str(&json).unwrap();

        let cmd = MessageContent::Command {
            name: "ask".into(),
            args: "what time is it".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let _: MessageContent = serde_json::from_str(&json).unwrap();

        let voice = MessageContent::Voice(MediaRef::PlatformId("file-abc".into()));
        let json = serde_json::to_string(&voice).unwrap();
        let _: MessageContent = serde_json::from_str(&json).unwrap();

        let loc = MessageContent::Location {
            lat: 51.5074,
            lon: -0.1278,
        };
        let json = serde_json::to_string(&loc).unwrap();
        let _: MessageContent = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn media_ref_variants() {
        let local = MediaRef::Local(PathBuf::from("/tmp/voice.ogg"));
        let json = serde_json::to_string(&local).unwrap();
        let _: MediaRef = serde_json::from_str(&json).unwrap();

        let url = MediaRef::Url("https://cdn.example.com/file.pdf".into());
        let json = serde_json::to_string(&url).unwrap();
        let _: MediaRef = serde_json::from_str(&json).unwrap();

        let pid = MediaRef::PlatformId("telegram-file-id-xyz".into());
        let json = serde_json::to_string(&pid).unwrap();
        let _: MediaRef = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn envelope_new_helper() {
        let peer = Peer {
            id: "u1".into(),
            display_name: None,
            platform: Platform::Cli,
        };
        let env = Envelope::new(peer, MessageContent::Text("test".into()));
        assert!(!env.id.is_empty());
        assert!(env.thread.is_none());
        assert!(env.attachments.is_empty());
        assert_eq!(env.platform_meta, serde_json::Value::Null);
    }
}
