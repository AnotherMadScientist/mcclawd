//! Slack message normalization.
//!
//! Converts Slack-specific message types to the platform-agnostic [`Envelope`].
//! Uses intermediate types (`SlackMessage`, `SlackFile`) so normalization logic
//! can be unit-tested without a live Slack bot or slack-morphism dependency.

use mcclawd_channels::envelope::{
    Attachment, Envelope, MediaRef, MessageContent, Peer, Platform, ThreadContext,
};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Intermediate types (testable without slack-morphism)
// ---------------------------------------------------------------------------

/// Intermediate representation of a Slack message for testable normalization.
#[derive(Debug, Clone)]
pub struct SlackMessage {
    /// Slack message timestamp (unique message ID within a channel).
    pub ts: String,
    /// Slack channel ID (e.g. "C01234ABCDE").
    pub channel_id: String,
    /// Slack user ID of the sender (e.g. "U01234ABCDE").
    pub user_id: String,
    /// Display name of the sender, if known.
    pub user_name: Option<String>,
    /// Text content of the message.
    pub text: String,
    /// Thread timestamp — if present, this message is in a thread.
    pub thread_ts: Option<String>,
    /// File attachments.
    pub files: Vec<SlackFile>,
}

/// A file attachment from Slack.
#[derive(Debug, Clone)]
pub struct SlackFile {
    /// Original filename.
    pub name: String,
    /// Private download URL (requires bot token for access).
    pub url_private: String,
    /// MIME type of the file.
    pub mimetype: String,
}

// ---------------------------------------------------------------------------
// Normalization
// ---------------------------------------------------------------------------

/// Convert a [`SlackMessage`] to a normalized [`Envelope`].
///
/// - Maps `/command args` text to `MessageContent::Command`.
/// - Maps thread_ts to `ThreadContext` with thread_id = thread_ts.
/// - Stores `channel_id`, `ts`, and `thread_ts` in `platform_meta` for outbound routing.
pub fn normalize(msg: &SlackMessage) -> Envelope {
    let peer = Peer {
        id: msg.user_id.clone(),
        display_name: msg.user_name.clone(),
        platform: Platform::Slack,
    };

    // Content: detect slash commands (text starting with "/").
    let content = if msg.text.starts_with('/') {
        let mut parts = msg.text.splitn(2, ' ');
        let name = parts.next().unwrap_or("").to_string();
        let args = parts.next().unwrap_or("").to_string();
        MessageContent::Command { name, args }
    } else {
        MessageContent::Text(msg.text.clone())
    };

    // Attachments: map SlackFile -> Attachment.
    let attachments: Vec<Attachment> = msg
        .files
        .iter()
        .map(|f| Attachment {
            filename: Some(f.name.clone()),
            mime_type: f.mimetype.clone(),
            media_ref: MediaRef::Url(f.url_private.clone()),
        })
        .collect();

    // Thread context from thread_ts.
    let thread = msg.thread_ts.as_ref().map(|ts| ThreadContext {
        thread_id: ts.clone(),
        parent_message_id: None,
    });

    // Platform metadata for outbound routing.
    let platform_meta = serde_json::json!({
        "channel_id": msg.channel_id,
        "ts": msg.ts,
        "thread_ts": msg.thread_ts,
    });

    Envelope {
        id: Uuid::new_v4().to_string(),
        peer,
        thread,
        content,
        attachments,
        timestamp: chrono::Utc::now(),
        platform_meta,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_text_message() -> SlackMessage {
        SlackMessage {
            ts: "1234567890.123456".into(),
            channel_id: "C01ABCDEF".into(),
            user_id: "U01234ABCDE".into(),
            user_name: Some("testuser".into()),
            text: "Hello from Slack!".into(),
            thread_ts: None,
            files: vec![],
        }
    }

    #[test]
    fn normalize_text_message() {
        let msg = sample_text_message();
        let env = normalize(&msg);

        assert_eq!(env.peer.platform, Platform::Slack);
        assert_eq!(env.peer.id, "U01234ABCDE");
        assert_eq!(env.peer.display_name, Some("testuser".into()));
        assert!(matches!(&env.content, MessageContent::Text(t) if t == "Hello from Slack!"));
        assert!(env.attachments.is_empty());
        assert!(env.thread.is_none());
    }

    #[test]
    fn normalize_threaded_message() {
        let mut msg = sample_text_message();
        msg.thread_ts = Some("1234567890.000001".into());

        let env = normalize(&msg);

        assert!(env.thread.is_some());
        let thread = env.thread.unwrap();
        assert_eq!(thread.thread_id, "1234567890.000001");
        assert!(thread.parent_message_id.is_none());
    }

    #[test]
    fn normalize_command_message() {
        let mut msg = sample_text_message();
        msg.text = "/deploy production --force".into();

        let env = normalize(&msg);

        match &env.content {
            MessageContent::Command { name, args } => {
                assert_eq!(name, "/deploy");
                assert_eq!(args, "production --force");
            }
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn normalize_command_no_args() {
        let mut msg = sample_text_message();
        msg.text = "/status".into();

        let env = normalize(&msg);

        match &env.content {
            MessageContent::Command { name, args } => {
                assert_eq!(name, "/status");
                assert_eq!(args, "");
            }
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn normalize_single_file() {
        let mut msg = sample_text_message();
        msg.files = vec![SlackFile {
            name: "report.pdf".into(),
            url_private: "https://files.slack.com/files-pri/T01/report.pdf".into(),
            mimetype: "application/pdf".into(),
        }];

        let env = normalize(&msg);

        assert_eq!(env.attachments.len(), 1);
        assert_eq!(env.attachments[0].filename, Some("report.pdf".into()));
        assert_eq!(env.attachments[0].mime_type, "application/pdf");
        assert!(matches!(
            &env.attachments[0].media_ref,
            MediaRef::Url(url) if url == "https://files.slack.com/files-pri/T01/report.pdf"
        ));
    }

    #[test]
    fn normalize_multiple_files() {
        let mut msg = sample_text_message();
        msg.files = vec![
            SlackFile {
                name: "image.png".into(),
                url_private: "https://files.slack.com/files-pri/T01/image.png".into(),
                mimetype: "image/png".into(),
            },
            SlackFile {
                name: "data.csv".into(),
                url_private: "https://files.slack.com/files-pri/T01/data.csv".into(),
                mimetype: "text/csv".into(),
            },
            SlackFile {
                name: "notes.txt".into(),
                url_private: "https://files.slack.com/files-pri/T01/notes.txt".into(),
                mimetype: "text/plain".into(),
            },
        ];

        let env = normalize(&msg);

        assert_eq!(env.attachments.len(), 3);
        assert_eq!(env.attachments[0].filename, Some("image.png".into()));
        assert_eq!(env.attachments[1].filename, Some("data.csv".into()));
        assert_eq!(env.attachments[2].filename, Some("notes.txt".into()));
    }

    #[test]
    fn normalize_no_thread_has_no_thread_context() {
        let msg = sample_text_message();
        let env = normalize(&msg);
        assert!(env.thread.is_none());
    }

    #[test]
    fn normalize_platform_meta_contains_channel_and_ts() {
        let msg = sample_text_message();
        let env = normalize(&msg);

        assert_eq!(env.platform_meta["channel_id"], "C01ABCDEF");
        assert_eq!(env.platform_meta["ts"], "1234567890.123456");
        assert!(env.platform_meta["thread_ts"].is_null());
    }

    #[test]
    fn normalize_platform_meta_with_thread_ts() {
        let mut msg = sample_text_message();
        msg.thread_ts = Some("1234567890.000001".into());

        let env = normalize(&msg);

        assert_eq!(env.platform_meta["thread_ts"], "1234567890.000001");
    }

    #[test]
    fn normalize_generates_unique_ids() {
        let msg = sample_text_message();
        let env1 = normalize(&msg);
        let env2 = normalize(&msg);
        assert_ne!(env1.id, env2.id);
    }

    #[test]
    fn normalize_no_display_name() {
        let mut msg = sample_text_message();
        msg.user_name = None;

        let env = normalize(&msg);

        assert_eq!(env.peer.display_name, None);
        assert_eq!(env.peer.id, "U01234ABCDE");
    }

    #[test]
    fn normalize_empty_text() {
        let mut msg = sample_text_message();
        msg.text = "".into();

        let env = normalize(&msg);
        assert!(matches!(&env.content, MessageContent::Text(t) if t.is_empty()));
    }
}
