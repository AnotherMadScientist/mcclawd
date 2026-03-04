//! Discord adapter integration tests.
//!
//! These tests exercise the full normalize -> channel flow without requiring
//! a live Discord bot. They verify that messages flow correctly through the
//! mpsc channels and that the registry integration works.

use chrono::Utc;
use mcclawd_channel_discord::normalize::{normalize, DiscordAttachment, DiscordMessage};
use mcclawd_channel_discord::{DiscordChannel, DiscordConfig};
use mcclawd_channels::envelope::{MessageContent, Platform};
use mcclawd_channels::registry::{ChannelEntry, ChannelId, ChannelRegistry};
use mcclawd_channels::Channel;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_config() -> DiscordConfig {
    DiscordConfig {
        bot_token: "FAKE_TOKEN".into(),
        allowed_guild_ids: None,
        allowed_channel_ids: None,
    }
}

fn sample_discord_message(content: &str) -> DiscordMessage {
    DiscordMessage {
        message_id: "111222333".into(),
        channel_id: "444555666".into(),
        guild_id: Some("777888999".into()),
        author_id: "42".into(),
        author_name: "IntegrationUser".into(),
        content: content.into(),
        attachments: vec![],
        timestamp: Utc::now(),
    }
}

// ---------------------------------------------------------------------------
// Normalize then receive through channel
// ---------------------------------------------------------------------------

#[tokio::test]
async fn normalize_then_receive_through_channel() {
    let mut channel = DiscordChannel::new(test_config());
    let sender = channel.sender();

    let msg = sample_discord_message("Hello from integration test");
    let envelope = normalize(&msg);

    assert_eq!(envelope.peer.platform, Platform::Discord);

    sender.send(envelope).await.expect("send should succeed");

    let received = channel
        .recv_envelope()
        .await
        .expect("recv should not error")
        .expect("should receive the injected envelope");

    assert_eq!(received.peer.platform, Platform::Discord);
    assert!(matches!(
        received.content,
        MessageContent::Text(ref t) if t == "Hello from integration test"
    ));
    assert_eq!(received.peer.id, "42");
    assert_eq!(received.peer.display_name, Some("IntegrationUser".into()));
}

// ---------------------------------------------------------------------------
// Registry integration
// ---------------------------------------------------------------------------

#[test]
fn registry_integration() {
    let mut registry = ChannelRegistry::new();

    let entry = ChannelEntry {
        id: ChannelId::new("discord-main"),
        platform: Platform::Discord,
        capabilities: DiscordChannel::discord_capabilities(),
        enabled: true,
    };

    registry.register(entry);

    let looked_up = registry.get(&ChannelId::new("discord-main")).unwrap();
    assert_eq!(looked_up.platform, Platform::Discord);

    let caps = &looked_up.capabilities;
    assert!(caps.supports_edit);
    assert!(caps.supports_markdown);
    assert!(caps.supports_files);
    assert!(!caps.supports_streaming);
    assert_eq!(caps.max_message_len, 2000);
    assert_eq!(caps.max_file_size, 25 * 1024 * 1024);
}

// ---------------------------------------------------------------------------
// Multi-message ordering through full pipeline
// ---------------------------------------------------------------------------

#[tokio::test]
async fn multi_message_ordering_through_pipeline() {
    let mut channel = DiscordChannel::new(test_config());
    let sender = channel.sender();

    let messages = vec![
        sample_discord_message("first"),
        sample_discord_message("second"),
        sample_discord_message("third"),
    ];

    for msg in &messages {
        let envelope = normalize(msg);
        sender.send(envelope).await.unwrap();
    }

    let r1 = channel.recv_envelope().await.unwrap().unwrap();
    let r2 = channel.recv_envelope().await.unwrap().unwrap();
    let r3 = channel.recv_envelope().await.unwrap().unwrap();

    assert!(matches!(&r1.content, MessageContent::Text(t) if t == "first"));
    assert!(matches!(&r2.content, MessageContent::Text(t) if t == "second"));
    assert!(matches!(&r3.content, MessageContent::Text(t) if t == "third"));
}

// ---------------------------------------------------------------------------
// Media message flow
// ---------------------------------------------------------------------------

#[tokio::test]
async fn media_message_flow() {
    let mut channel = DiscordChannel::new(test_config());
    let sender = channel.sender();

    let mut msg = sample_discord_message("Check this file");
    msg.attachments = vec![
        DiscordAttachment {
            filename: "screenshot.png".into(),
            url: "https://cdn.discordapp.com/attachments/1/2/screenshot.png".into(),
            content_type: Some("image/png".into()),
        },
        DiscordAttachment {
            filename: "data.csv".into(),
            url: "https://cdn.discordapp.com/attachments/1/2/data.csv".into(),
            content_type: Some("text/csv".into()),
        },
    ];

    let envelope = normalize(&msg);
    assert_eq!(envelope.attachments.len(), 2);

    sender.send(envelope).await.unwrap();

    let received = channel.recv_envelope().await.unwrap().unwrap();

    assert_eq!(received.attachments.len(), 2);
    assert_eq!(
        received.attachments[0].filename,
        Some("screenshot.png".into())
    );
    assert_eq!(received.attachments[0].mime_type, "image/png");
    assert_eq!(received.attachments[1].filename, Some("data.csv".into()));
    assert_eq!(received.attachments[1].mime_type, "text/csv");
}

// ---------------------------------------------------------------------------
// Command normalization through channel
// ---------------------------------------------------------------------------

#[tokio::test]
async fn command_message_through_channel() {
    let mut channel = DiscordChannel::new(test_config());
    let sender = channel.sender();

    let msg = sample_discord_message("/summarize the last 10 messages");
    let envelope = normalize(&msg);

    sender.send(envelope).await.unwrap();

    let received = channel.recv_envelope().await.unwrap().unwrap();

    match &received.content {
        MessageContent::Command { name, args } => {
            assert_eq!(name, "summarize");
            assert_eq!(args, "the last 10 messages");
        }
        other => panic!("Expected Command, got {:?}", other),
    }
}
