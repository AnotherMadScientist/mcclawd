//! Slack adapter integration tests.
//!
//! These tests exercise the normalize -> channel flow without requiring
//! a live Slack bot token. The mpsc-inject test is marked `#[ignore]`
//! since it requires `SLACK_BOT_TOKEN` to be set.
//!
//! Run manually:
//!
//! ```bash
//! SLACK_BOT_TOKEN=xoxb-... cargo test -p mcclawd-channel-slack --test slack_integration -- --ignored
//! ```

use mcclawd_channel_slack::normalize::{normalize, SlackFile, SlackMessage};
use mcclawd_channel_slack::{SlackChannel, SlackConfig};
use mcclawd_channels::envelope::{MessageContent, Platform};

// ---------------------------------------------------------------------------
// mpsc inject / receive flow (requires bot token)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires SLACK_BOT_TOKEN env var"]
async fn slack_channel_mpsc_inject_and_receive() {
    let token =
        std::env::var("SLACK_BOT_TOKEN").expect("SLACK_BOT_TOKEN must be set for this test");

    let mut channel = SlackChannel::new(SlackConfig {
        bot_token: token,
        app_token: None,
        allowed_channel_ids: None,
    });

    // Build a test envelope via normalize and inject through the sender
    let msg = SlackMessage {
        ts: "1234567890.123456".into(),
        channel_id: "C01ABCDEF".into(),
        user_id: "U99".into(),
        user_name: Some("testuser".into()),
        text: "Hello from test".into(),
        thread_ts: None,
        files: vec![],
    };

    let envelope = normalize(&msg);
    assert_eq!(envelope.peer.platform, Platform::Slack);

    // Inject via the sender half
    let sender = channel.sender();
    sender.send(envelope).await.expect("send should succeed");

    // Receive from the channel's inbox
    use mcclawd_channels::Channel;
    let received = channel
        .recv_envelope()
        .await
        .expect("recv should not error")
        .expect("should receive the injected envelope");

    assert_eq!(received.peer.platform, Platform::Slack);
    assert!(matches!(received.content, MessageContent::Text(ref t) if t == "Hello from test"));
}

// ---------------------------------------------------------------------------
// Normalization of various message types (no bot token needed)
// ---------------------------------------------------------------------------

#[test]
fn normalize_text_message() {
    let msg = SlackMessage {
        ts: "1234567890.123456".into(),
        channel_id: "C01ABCDEF".into(),
        user_id: "U01".into(),
        user_name: Some("Alice".into()),
        text: "plain text".into(),
        thread_ts: None,
        files: vec![],
    };

    let env = normalize(&msg);
    assert_eq!(env.peer.platform, Platform::Slack);
    assert!(matches!(env.content, MessageContent::Text(ref t) if t == "plain text"));
    assert_eq!(env.peer.display_name, Some("Alice".into()));
    assert!(env.attachments.is_empty());
    let _ = env.thread;
}

#[test]
fn normalize_file_message() {
    let msg = SlackMessage {
        ts: "1234567890.123457".into(),
        channel_id: "C01ABCDEF".into(),
        user_id: "U02".into(),
        user_name: Some("Bob".into()),
        text: "here is a file".into(),
        thread_ts: None,
        files: vec![SlackFile {
            name: "readme.pdf".into(),
            url_private: "https://files.slack.com/files-pri/T01/readme.pdf".into(),
            mimetype: "application/pdf".into(),
        }],
    };

    let env = normalize(&msg);
    assert!(!env.attachments.is_empty(), "file should produce attachment");
    let att = &env.attachments[0];
    assert_eq!(att.filename, Some("readme.pdf".into()));
    assert_eq!(att.mime_type, "application/pdf");
}

#[test]
fn normalize_threaded_message() {
    let msg = SlackMessage {
        ts: "1234567890.123458".into(),
        channel_id: "C01ABCDEF".into(),
        user_id: "U03".into(),
        user_name: Some("Charlie".into()),
        text: "replying in thread".into(),
        thread_ts: Some("1234567890.000001".into()),
        files: vec![],
    };

    let env = normalize(&msg);
    let thread = env.thread.expect("thread_ts should set thread context");
    assert_eq!(thread.thread_id, "1234567890.000001");
    assert!(thread.parent_message_id.is_none());
}

#[test]
fn normalize_command_message() {
    let msg = SlackMessage {
        ts: "1234567890.123459".into(),
        channel_id: "C01ABCDEF".into(),
        user_id: "U04".into(),
        user_name: None,
        text: "/deploy staging".into(),
        thread_ts: None,
        files: vec![],
    };

    let env = normalize(&msg);
    match &env.content {
        MessageContent::Command { name, args } => {
            assert_eq!(name, "/deploy");
            assert_eq!(args, "staging");
        }
        other => panic!("expected Command, got {:?}", other),
    }
}

#[test]
fn normalize_empty_message_fallback() {
    let msg = SlackMessage {
        ts: "1234567890.123460".into(),
        channel_id: "C01ABCDEF".into(),
        user_id: "U05".into(),
        user_name: None,
        text: "".into(),
        thread_ts: None,
        files: vec![],
    };

    let env = normalize(&msg);
    assert_eq!(env.peer.platform, Platform::Slack);
    assert!(matches!(&env.content, MessageContent::Text(t) if t.is_empty()));
}

#[test]
fn normalize_multiple_files() {
    let msg = SlackMessage {
        ts: "1234567890.123461".into(),
        channel_id: "C01ABCDEF".into(),
        user_id: "U06".into(),
        user_name: Some("Dave".into()),
        text: "multiple attachments".into(),
        thread_ts: None,
        files: vec![
            SlackFile {
                name: "photo.png".into(),
                url_private: "https://files.slack.com/files-pri/T01/photo.png".into(),
                mimetype: "image/png".into(),
            },
            SlackFile {
                name: "doc.pdf".into(),
                url_private: "https://files.slack.com/files-pri/T01/doc.pdf".into(),
                mimetype: "application/pdf".into(),
            },
        ],
    };

    let env = normalize(&msg);
    assert_eq!(env.attachments.len(), 2);
    assert_eq!(env.attachments[0].filename, Some("photo.png".into()));
    assert_eq!(env.attachments[1].filename, Some("doc.pdf".into()));
}

#[test]
fn normalize_preserves_peer_identity() {
    let msg = SlackMessage {
        ts: "1234567890.123462".into(),
        channel_id: "C01ABCDEF".into(),
        user_id: "U42".into(),
        user_name: Some("John Doe".into()),
        text: "test".into(),
        thread_ts: None,
        files: vec![],
    };

    let env = normalize(&msg);
    assert_eq!(env.peer.display_name, Some("John Doe".into()));
    assert_eq!(env.peer.platform, Platform::Slack);
    assert_eq!(env.peer.id, "U42");
}

#[test]
fn normalize_platform_meta_roundtrip() {
    let msg = SlackMessage {
        ts: "1234567890.123463".into(),
        channel_id: "C99ZZZYYX".into(),
        user_id: "U01".into(),
        user_name: None,
        text: "meta test".into(),
        thread_ts: Some("1234567890.000001".into()),
        files: vec![],
    };

    let env = normalize(&msg);
    assert_eq!(env.platform_meta["channel_id"], "C99ZZZYYX");
    assert_eq!(env.platform_meta["ts"], "1234567890.123463");
    assert_eq!(env.platform_meta["thread_ts"], "1234567890.000001");
}
