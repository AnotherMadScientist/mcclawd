//! WhatsApp adapter integration tests.
//!
//! Tests that require a live WhatsApp Cloud API token are marked `#[ignore]`.
//! Run manually:
//!
//! ```bash
//! WHATSAPP_ACCESS_TOKEN=... cargo test -p mcclawd-channel-whatsapp --test whatsapp_integration -- --ignored
//! ```

use chrono::Utc;
use mcclawd_channel_whatsapp::normalize::{normalize, WhatsAppMedia, WhatsAppMessage};
use mcclawd_channel_whatsapp::{WhatsAppChannel, WhatsAppConfig};
use mcclawd_channels::envelope::{MessageContent, Platform};

// ---------------------------------------------------------------------------
// mpsc inject / receive flow
// ---------------------------------------------------------------------------

#[tokio::test]
async fn whatsapp_channel_mpsc_inject_and_receive() {
    let mut channel = WhatsAppChannel::new(WhatsAppConfig {
        phone_number_id: "123456789".into(),
        access_token: "fake_token".into(),
        verify_token: "verify".into(),
        allowed_numbers: None,
    });

    // Build a test envelope via normalize and inject through the sender
    let msg = WhatsAppMessage {
        message_id: "wamid.test001".into(),
        from: "14155552671".into(),
        from_name: Some("Test User".into()),
        text: Some("Hello from test".into()),
        media: None,
        timestamp: Utc::now(),
    };

    let envelope = normalize(&msg);
    assert_eq!(envelope.peer.platform, Platform::WhatsApp);

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

    assert_eq!(received.peer.platform, Platform::WhatsApp);
    assert!(matches!(received.content, MessageContent::Text(ref t) if t == "Hello from test"));
}

// ---------------------------------------------------------------------------
// Normalization of various message types (no API token needed)
// ---------------------------------------------------------------------------

#[test]
fn normalize_text_message() {
    let msg = WhatsAppMessage {
        message_id: "wamid.txt001".into(),
        from: "14155551234".into(),
        from_name: Some("Alice".into()),
        text: Some("plain text".into()),
        media: None,
        timestamp: Utc::now(),
    };

    let env = normalize(&msg);
    assert_eq!(env.peer.platform, Platform::WhatsApp);
    assert!(matches!(env.content, MessageContent::Text(ref t) if t == "plain text"));
    assert_eq!(env.peer.display_name, Some("Alice".into()));
    assert!(env.attachments.is_empty());
    assert!(env.thread.is_none());
}

#[test]
fn normalize_media_message() {
    let msg = WhatsAppMessage {
        message_id: "wamid.media001".into(),
        from: "14155559876".into(),
        from_name: None,
        text: None,
        media: Some(WhatsAppMedia {
            id: "media-id-001".into(),
            mime_type: "image/jpeg".into(),
            filename: None,
        }),
        timestamp: Utc::now(),
    };

    let env = normalize(&msg);
    assert!(!env.attachments.is_empty(), "media should produce attachment");
    let att = &env.attachments[0];
    assert_eq!(att.mime_type, "image/jpeg");
    assert!(att.filename.is_none());
}

#[test]
fn normalize_document_with_filename() {
    let msg = WhatsAppMessage {
        message_id: "wamid.doc001".into(),
        from: "14155554444".into(),
        from_name: Some("Bob".into()),
        text: Some("Here's the report".into()),
        media: Some(WhatsAppMedia {
            id: "doc-media-id".into(),
            mime_type: "application/pdf".into(),
            filename: Some("report.pdf".into()),
        }),
        timestamp: Utc::now(),
    };

    let env = normalize(&msg);
    assert!(!env.attachments.is_empty());
    let att = &env.attachments[0];
    assert_eq!(att.filename, Some("report.pdf".into()));
    assert_eq!(att.mime_type, "application/pdf");
}

#[test]
fn normalize_command_message() {
    let msg = WhatsAppMessage {
        message_id: "wamid.cmd001".into(),
        from: "14155551111".into(),
        from_name: None,
        text: Some("/help".into()),
        media: None,
        timestamp: Utc::now(),
    };

    let env = normalize(&msg);
    assert!(matches!(
        env.content,
        MessageContent::Command { ref name, ref args }
        if name == "help" && args.is_empty()
    ));
}

#[test]
fn normalize_empty_message_fallback() {
    let msg = WhatsAppMessage {
        message_id: "wamid.empty001".into(),
        from: "14155550000".into(),
        from_name: None,
        text: None,
        media: None,
        timestamp: Utc::now(),
    };

    let env = normalize(&msg);
    assert_eq!(env.peer.platform, Platform::WhatsApp);
    assert!(matches!(&env.content, MessageContent::Text(t) if t.is_empty()));
}

#[test]
fn normalize_preserves_peer_identity() {
    let msg = WhatsAppMessage {
        message_id: "wamid.peer001".into(),
        from: "447911123456".into(),
        from_name: Some("John Doe".into()),
        text: Some("test".into()),
        media: None,
        timestamp: Utc::now(),
    };

    let env = normalize(&msg);
    assert_eq!(env.peer.display_name, Some("John Doe".into()));
    assert_eq!(env.peer.platform, Platform::WhatsApp);
    assert_eq!(env.peer.id, "447911123456");
}

// ---------------------------------------------------------------------------
// Registry integration
// ---------------------------------------------------------------------------

#[test]
fn whatsapp_channel_capabilities() {
    let caps = WhatsAppChannel::whatsapp_capabilities();
    assert!(!caps.supports_streaming);
    assert!(!caps.supports_edit);
    assert!(!caps.supports_markdown);
    assert_eq!(caps.max_message_len, 4096);
    assert!(caps.supports_files);
    assert_eq!(caps.max_file_size, 100 * 1024 * 1024);
}

#[test]
fn whatsapp_channel_kind_and_platform() {
    use mcclawd_channels::Channel;
    use mcclawd_channels::types::ChannelKind;

    let channel = WhatsAppChannel::new(WhatsAppConfig {
        phone_number_id: "123".into(),
        access_token: "token".into(),
        verify_token: "verify".into(),
        allowed_numbers: None,
    });

    assert_eq!(channel.kind(), ChannelKind::WhatsApp);
    assert_eq!(channel.platform(), Platform::WhatsApp);
}
