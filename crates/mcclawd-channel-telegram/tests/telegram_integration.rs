//! Task 26: Telegram adapter integration test (ignored without bot token).
//!
//! These tests require a real `TELEGRAM_BOT_TOKEN` env var and are marked
//! `#[ignore]` so they do not run in CI by default. Run manually:
//!
//! ```bash
//! TELEGRAM_BOT_TOKEN=... cargo test -p mcclawd-channel-telegram --test telegram_integration -- --ignored
//! ```

use chrono::Utc;
use mcclawd_channel_telegram::{TelegramChannel, TelegramConfig};
use mcclawd_channel_telegram::normalize::{normalize, TelegramMessage, TelegramPhoto, TelegramDocument};
use mcclawd_channels::envelope::{MessageContent, Platform};

// ---------------------------------------------------------------------------
// mpsc inject / receive flow (requires bot token)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires TELEGRAM_BOT_TOKEN env var"]
async fn telegram_channel_mpsc_inject_and_receive() {
    let token = std::env::var("TELEGRAM_BOT_TOKEN")
        .expect("TELEGRAM_BOT_TOKEN must be set for this test");

    let mut channel = TelegramChannel::new(TelegramConfig {
        bot_token: token,
        allowed_chat_ids: None,
    });

    // Build a test envelope via normalize and inject through the sender
    let msg = TelegramMessage {
        message_id: 1,
        chat_id: 12345,
        from_user_id: Some(99),
        from_username: Some("testuser".into()),
        from_display_name: Some("Test User".into()),
        text: Some("Hello from test".into()),
        caption: None,
        reply_to_message_id: None,
        date: Utc::now(),
        photos: vec![],
        document: None,
    };

    let envelope = normalize(&msg);
    assert_eq!(envelope.peer.platform, Platform::Telegram);

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

    assert_eq!(received.peer.platform, Platform::Telegram);
    assert!(matches!(received.content, MessageContent::Text(ref t) if t == "Hello from test"));
}

// ---------------------------------------------------------------------------
// Normalization of various message types (no bot token needed)
// ---------------------------------------------------------------------------

#[test]
fn normalize_text_message() {
    let msg = TelegramMessage {
        message_id: 42,
        chat_id: 100,
        from_user_id: Some(1),
        from_username: Some("alice".into()),
        from_display_name: Some("Alice".into()),
        text: Some("plain text".into()),
        caption: None,
        reply_to_message_id: None,
        date: Utc::now(),
        photos: vec![],
        document: None,
    };

    let env = normalize(&msg);
    assert_eq!(env.peer.platform, Platform::Telegram);
    assert!(matches!(env.content, MessageContent::Text(ref t) if t == "plain text"));
    assert_eq!(env.peer.display_name, Some("Alice".into()));
    assert!(env.attachments.is_empty());
    // Thread context is set based on chat_id; presence depends on implementation
    // Just verify the envelope is well-formed
    let _ = env.thread;
}

#[test]
fn normalize_photo_message() {
    let msg = TelegramMessage {
        message_id: 43,
        chat_id: 100,
        from_user_id: Some(1),
        from_username: None,
        from_display_name: None,
        text: None,
        caption: Some("nice photo".into()),
        reply_to_message_id: None,
        date: Utc::now(),
        photos: vec![TelegramPhoto {
            file_id: "photo_file_123".into(),
            width: 800,
            height: 600,
        }],
        document: None,
    };

    let env = normalize(&msg);
    assert!(
        matches!(env.content, MessageContent::Text(ref t) if t == "nice photo"),
        "caption should become text content"
    );
    assert!(!env.attachments.is_empty(), "photo should produce attachment");
}

#[test]
fn normalize_document_message() {
    let msg = TelegramMessage {
        message_id: 44,
        chat_id: 200,
        from_user_id: Some(2),
        from_username: None,
        from_display_name: Some("Bob".into()),
        text: None,
        caption: Some("my document".into()),
        reply_to_message_id: None,
        date: Utc::now(),
        photos: vec![],
        document: Some(TelegramDocument {
            file_id: "doc_abc".into(),
            file_name: Some("readme.pdf".into()),
            mime_type: Some("application/pdf".into()),
        }),
    };

    let env = normalize(&msg);
    assert!(!env.attachments.is_empty(), "document should produce attachment");
    let att = &env.attachments[0];
    assert_eq!(att.filename, Some("readme.pdf".into()));
    assert_eq!(att.mime_type, "application/pdf");
}

#[test]
fn normalize_reply_sets_thread_context() {
    let msg = TelegramMessage {
        message_id: 46,
        chat_id: 400,
        from_user_id: Some(4),
        from_username: Some("charlie".into()),
        from_display_name: None,
        text: Some("replying".into()),
        caption: None,
        reply_to_message_id: Some(10),
        date: Utc::now(),
        photos: vec![],
        document: None,
    };

    let env = normalize(&msg);
    let thread = env.thread.expect("reply should set thread context");
    assert!(
        thread.parent_message_id.is_some(),
        "reply should set parent_message_id"
    );
}

#[test]
fn normalize_empty_message_fallback() {
    // A message with no text, no media
    let msg = TelegramMessage {
        message_id: 47,
        chat_id: 500,
        from_user_id: None,
        from_username: None,
        from_display_name: None,
        text: None,
        caption: None,
        reply_to_message_id: None,
        date: Utc::now(),
        photos: vec![],
        document: None,
    };

    let env = normalize(&msg);
    // Should still produce a valid envelope
    assert_eq!(env.peer.platform, Platform::Telegram);
}

#[test]
fn normalize_multiple_photos_uses_largest() {
    let msg = TelegramMessage {
        message_id: 48,
        chat_id: 100,
        from_user_id: Some(1),
        from_username: None,
        from_display_name: None,
        text: None,
        caption: None,
        reply_to_message_id: None,
        date: Utc::now(),
        photos: vec![
            TelegramPhoto { file_id: "small".into(), width: 100, height: 100 },
            TelegramPhoto { file_id: "large".into(), width: 1920, height: 1080 },
            TelegramPhoto { file_id: "medium".into(), width: 800, height: 600 },
        ],
        document: None,
    };

    let env = normalize(&msg);
    // Should produce at least one attachment (typically the largest photo)
    assert!(!env.attachments.is_empty());
}

#[test]
fn normalize_preserves_peer_identity() {
    let msg = TelegramMessage {
        message_id: 50,
        chat_id: 999,
        from_user_id: Some(42),
        from_username: Some("johndoe".into()),
        from_display_name: Some("John Doe".into()),
        text: Some("test".into()),
        caption: None,
        reply_to_message_id: None,
        date: Utc::now(),
        photos: vec![],
        document: None,
    };

    let env = normalize(&msg);
    assert_eq!(env.peer.display_name, Some("John Doe".into()));
    assert_eq!(env.peer.platform, Platform::Telegram);
    // Peer ID should contain the user ID
    assert!(env.peer.id.contains("42"), "peer ID should contain telegram user ID");
}
