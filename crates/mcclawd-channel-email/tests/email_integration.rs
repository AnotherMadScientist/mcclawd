//! Email adapter integration tests.
//!
//! These tests verify the normalize -> channel flow, registry integration,
//! threaded replies, and attachment handling without requiring live IMAP/SMTP.

use chrono::Utc;
use mcclawd_channel_email::normalize::{normalize, EmailAttachment, EmailMessage};
use mcclawd_channel_email::{EmailChannel, EmailConfig};
use mcclawd_channels::envelope::{MessageContent, Platform};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_config() -> EmailConfig {
    EmailConfig {
        imap_host: "imap.example.com".into(),
        imap_port: 993,
        smtp_host: "smtp.example.com".into(),
        smtp_port: 587,
        username: "bot@example.com".into(),
        password: "secret".into(),
        from_address: "bot@example.com".into(),
        allowed_senders: None,
        poll_interval_secs: 30,
    }
}

fn sample_email() -> EmailMessage {
    EmailMessage {
        message_id: "<msg001@example.com>".into(),
        from_address: "sender@example.com".into(),
        from_name: Some("Test Sender".into()),
        subject: Some("Test Subject".into()),
        body_text: Some("Hello from integration test".into()),
        body_html: None,
        in_reply_to: None,
        attachments: vec![],
        date: Utc::now(),
    }
}

// ---------------------------------------------------------------------------
// normalize -> channel flow
// ---------------------------------------------------------------------------

#[tokio::test]
async fn normalize_and_inject_into_channel() {
    let mut channel = EmailChannel::new(test_config());
    let sender = channel.sender();

    let msg = sample_email();
    let envelope = normalize(&msg);

    assert_eq!(envelope.peer.platform, Platform::Email);
    assert_eq!(envelope.peer.id, "sender@example.com");

    sender.send(envelope).await.expect("send should succeed");

    use mcclawd_channels::Channel;
    let received = channel
        .recv_envelope()
        .await
        .expect("recv should not error")
        .expect("should receive the injected envelope");

    assert_eq!(received.peer.platform, Platform::Email);
    assert!(
        matches!(received.content, MessageContent::Text(ref t) if t == "Hello from integration test")
    );
}

// ---------------------------------------------------------------------------
// Channel capabilities
// ---------------------------------------------------------------------------

#[test]
fn email_capabilities_match_expected() {
    let caps = EmailChannel::email_capabilities();
    assert!(!caps.supports_streaming, "email does not support streaming");
    assert!(!caps.supports_edit, "cannot edit sent emails");
    assert!(!caps.supports_markdown, "email uses plain text");
    assert_eq!(caps.max_message_len, 0, "email has no message length limit");
    assert!(caps.supports_files, "email supports attachments");
    assert_eq!(
        caps.max_file_size,
        25 * 1024 * 1024,
        "SMTP attachment limit is 25MB"
    );
}

// ---------------------------------------------------------------------------
// Threaded reply
// ---------------------------------------------------------------------------

#[tokio::test]
async fn threaded_reply_preserves_context() {
    let mut channel = EmailChannel::new(test_config());
    let sender = channel.sender();

    let mut msg = sample_email();
    msg.in_reply_to = Some("<parent@example.com>".into());
    msg.body_text = Some("This is a reply".into());

    let envelope = normalize(&msg);
    sender.send(envelope).await.unwrap();

    use mcclawd_channels::Channel;
    let received = channel.recv_envelope().await.unwrap().unwrap();

    let thread = received.thread.expect("reply should have thread context");
    assert_eq!(thread.thread_id, "<parent@example.com>");
}

// ---------------------------------------------------------------------------
// Attachments
// ---------------------------------------------------------------------------

#[tokio::test]
async fn attachments_flow_through_channel() {
    let mut channel = EmailChannel::new(test_config());
    let sender = channel.sender();

    let mut msg = sample_email();
    msg.attachments = vec![
        EmailAttachment {
            filename: "document.pdf".into(),
            content_type: "application/pdf".into(),
            data: vec![0x25, 0x50, 0x44, 0x46],
        },
        EmailAttachment {
            filename: "image.png".into(),
            content_type: "image/png".into(),
            data: vec![0x89, 0x50, 0x4E, 0x47],
        },
    ];

    let envelope = normalize(&msg);
    assert_eq!(envelope.attachments.len(), 2);

    sender.send(envelope).await.unwrap();

    use mcclawd_channels::Channel;
    let received = channel.recv_envelope().await.unwrap().unwrap();

    assert_eq!(received.attachments.len(), 2);
    assert_eq!(
        received.attachments[0].filename,
        Some("document.pdf".into())
    );
    assert_eq!(received.attachments[0].mime_type, "application/pdf");
    assert_eq!(received.attachments[1].filename, Some("image.png".into()));
    assert_eq!(received.attachments[1].mime_type, "image/png");
}

// ---------------------------------------------------------------------------
// Platform identity
// ---------------------------------------------------------------------------

#[test]
fn channel_kind_and_platform() {
    use mcclawd_channels::Channel;
    let channel = EmailChannel::new(test_config());
    assert_eq!(channel.kind(), mcclawd_channels::types::ChannelKind::Email);
    assert_eq!(channel.platform(), Platform::Email);
}

// ---------------------------------------------------------------------------
// Multiple senders
// ---------------------------------------------------------------------------

#[tokio::test]
async fn multiple_senders_can_inject() {
    let mut channel = EmailChannel::new(test_config());
    let s1 = channel.sender();
    let s2 = channel.sender();

    let msg1 = sample_email();
    let mut msg2 = sample_email();
    msg2.from_address = "other@example.com".into();
    msg2.body_text = Some("From second sender".into());

    s1.send(normalize(&msg1)).await.unwrap();
    s2.send(normalize(&msg2)).await.unwrap();

    use mcclawd_channels::Channel;
    let r1 = channel.recv_envelope().await.unwrap().unwrap();
    let r2 = channel.recv_envelope().await.unwrap().unwrap();

    assert_eq!(r1.peer.id, "sender@example.com");
    assert_eq!(r2.peer.id, "other@example.com");
}

// ---------------------------------------------------------------------------
// Outbound chunk flow
// ---------------------------------------------------------------------------

#[tokio::test]
async fn outbound_send_chunk_succeeds() {
    use mcclawd_channels::Channel;

    let channel = EmailChannel::new(test_config());

    // send_chunk should succeed — the internal outbound mpsc is open.
    channel
        .send_chunk(mcclawd_channels::types::OutboundChunk::TextBlock(
            "Email reply body".into(),
        ))
        .await
        .unwrap();

    // Send Done signal as well.
    channel
        .send_chunk(mcclawd_channels::types::OutboundChunk::Done)
        .await
        .unwrap();
}

// ---------------------------------------------------------------------------
// Platform meta preservation
// ---------------------------------------------------------------------------

#[test]
fn platform_meta_has_message_id_and_subject() {
    let msg = sample_email();
    let env = normalize(&msg);

    assert_eq!(env.platform_meta["message_id"], "<msg001@example.com>");
    assert_eq!(env.platform_meta["subject"], "Test Subject");
}
