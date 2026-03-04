//! Phase 3 integration tests — verify all 5 channel adapters work together.
//!
//! These tests exercise the full channel lifecycle: creation, registration,
//! normalization, inbound/outbound routing, and capability validation for
//! Telegram, Discord, Slack, WhatsApp, and Email adapters.

use chrono::Utc;
use mcclawd_channels::envelope::{Envelope, MessageContent, Peer, Platform};
use mcclawd_channels::registry::{ChannelEntry, ChannelId, ChannelRegistry};
use mcclawd_channels::types::{ChannelKind, OutboundChunk};
use mcclawd_channels::Channel;

// Channel adapters
use mcclawd_channel_discord::{DiscordChannel, DiscordConfig};
use mcclawd_channel_email::{EmailChannel, EmailConfig};
use mcclawd_channel_slack::{SlackChannel, SlackConfig};
use mcclawd_channel_telegram::{TelegramChannel, TelegramConfig};
use mcclawd_channel_whatsapp::{WhatsAppChannel, WhatsAppConfig};

// Normalize functions and intermediate message types
use mcclawd_channel_discord::normalize::{normalize as discord_normalize, DiscordMessage};
use mcclawd_channel_email::normalize::{normalize as email_normalize, EmailMessage};
use mcclawd_channel_slack::normalize::{normalize as slack_normalize, SlackMessage};
use mcclawd_channel_telegram::normalize::{normalize as telegram_normalize, TelegramMessage};
use mcclawd_channel_whatsapp::normalize::{normalize as whatsapp_normalize, WhatsAppMessage};

// ---------------------------------------------------------------------------
// Helper: config factories
// ---------------------------------------------------------------------------

fn telegram_config() -> TelegramConfig {
    TelegramConfig {
        bot_token: "FAKE_TG_TOKEN".into(),
        allowed_chat_ids: None,
    }
}

fn discord_config() -> DiscordConfig {
    DiscordConfig {
        bot_token: "FAKE_DISCORD_TOKEN".into(),
        allowed_guild_ids: None,
        allowed_channel_ids: None,
    }
}

fn slack_config() -> SlackConfig {
    SlackConfig {
        bot_token: "xoxb-FAKE-SLACK-TOKEN".into(),
        app_token: None,
        allowed_channel_ids: None,
    }
}

fn whatsapp_config() -> WhatsAppConfig {
    WhatsAppConfig {
        phone_number_id: "123456789".into(),
        access_token: "FAKE_WA_ACCESS_TOKEN".into(),
        verify_token: "my_verify_token".into(),
        allowed_numbers: None,
    }
}

fn email_config() -> EmailConfig {
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

// ---------------------------------------------------------------------------
// Helper: build a test Envelope for injection
// ---------------------------------------------------------------------------

fn make_envelope(platform: Platform, peer_id: &str, text: &str) -> Envelope {
    Envelope {
        id: uuid::Uuid::new_v4().to_string(),
        peer: Peer {
            id: peer_id.into(),
            display_name: Some("Test User".into()),
            platform,
        },
        thread: None,
        content: MessageContent::Text(text.into()),
        attachments: vec![],
        timestamp: Utc::now(),
        platform_meta: serde_json::Value::Null,
    }
}

// ---------------------------------------------------------------------------
// 1. Register all five channels in a ChannelRegistry
// ---------------------------------------------------------------------------

#[test]
fn test_register_all_five_channels() {
    let mut registry = ChannelRegistry::new();

    let entries = vec![
        ChannelEntry {
            id: ChannelId::new("telegram-prod"),
            platform: Platform::Telegram,
            capabilities: TelegramChannel::telegram_capabilities(),
            enabled: true,
        },
        ChannelEntry {
            id: ChannelId::new("discord-prod"),
            platform: Platform::Discord,
            capabilities: DiscordChannel::discord_capabilities(),
            enabled: true,
        },
        ChannelEntry {
            id: ChannelId::new("slack-prod"),
            platform: Platform::Slack,
            capabilities: SlackChannel::slack_capabilities(),
            enabled: true,
        },
        ChannelEntry {
            id: ChannelId::new("whatsapp-prod"),
            platform: Platform::WhatsApp,
            capabilities: WhatsAppChannel::whatsapp_capabilities(),
            enabled: true,
        },
        ChannelEntry {
            id: ChannelId::new("email-prod"),
            platform: Platform::Email,
            capabilities: EmailChannel::email_capabilities(),
            enabled: true,
        },
    ];

    for entry in entries {
        assert!(registry.register(entry), "registration should succeed");
    }

    assert_eq!(registry.len(), 5);

    // Verify each platform is present.
    let platforms: Vec<Platform> = registry.list().iter().map(|e| e.platform.clone()).collect();
    assert!(platforms.contains(&Platform::Telegram));
    assert!(platforms.contains(&Platform::Discord));
    assert!(platforms.contains(&Platform::Slack));
    assert!(platforms.contains(&Platform::WhatsApp));
    assert!(platforms.contains(&Platform::Email));
}

// ---------------------------------------------------------------------------
// 2. Capabilities are distinct and sensible per channel
// ---------------------------------------------------------------------------

#[test]
fn test_capabilities_are_distinct() {
    let tg = TelegramChannel::telegram_capabilities();
    let dc = DiscordChannel::discord_capabilities();
    let sl = SlackChannel::slack_capabilities();
    let wa = WhatsAppChannel::whatsapp_capabilities();
    let em = EmailChannel::email_capabilities();

    // Telegram: 4096 char limit, supports edit + markdown
    assert_eq!(tg.max_message_len, 4096);
    assert!(tg.supports_edit);
    assert!(tg.supports_markdown);

    // Discord: 2000 char limit, supports edit + markdown
    assert_eq!(dc.max_message_len, 2000);
    assert!(dc.supports_edit);
    assert!(dc.supports_markdown);

    // Slack: 40000 char limit, supports edit + markdown
    assert_eq!(sl.max_message_len, 40_000);
    assert!(sl.supports_edit);
    assert!(sl.supports_markdown);

    // WhatsApp: 4096 char limit, no edit, no markdown
    assert_eq!(wa.max_message_len, 4096);
    assert!(!wa.supports_edit);
    assert!(!wa.supports_markdown);

    // Email: 0 (unlimited), no edit, no markdown
    assert_eq!(em.max_message_len, 0);
    assert!(!em.supports_edit);
    assert!(!em.supports_markdown);

    // All five should support files.
    assert!(tg.supports_files);
    assert!(dc.supports_files);
    assert!(sl.supports_files);
    assert!(wa.supports_files);
    assert!(em.supports_files);

    // None of them support streaming (all store-and-forward or rate-limited).
    assert!(!tg.supports_streaming);
    assert!(!dc.supports_streaming);
    assert!(!sl.supports_streaming);
    assert!(!wa.supports_streaming);
    assert!(!em.supports_streaming);
}

// ---------------------------------------------------------------------------
// 3. Normalize from each platform produces correct Platform
// ---------------------------------------------------------------------------

#[test]
fn test_normalize_from_each_platform() {
    // Telegram
    let tg_msg = TelegramMessage {
        message_id: 1,
        chat_id: 12345,
        from_user_id: Some(99),
        from_username: Some("tg_user".into()),
        from_display_name: Some("TG User".into()),
        text: Some("Hello from Telegram".into()),
        caption: None,
        reply_to_message_id: None,
        date: Utc::now(),
        photos: vec![],
        document: None,
    };
    let tg_env = telegram_normalize(&tg_msg);
    assert_eq!(tg_env.peer.platform, Platform::Telegram);
    assert!(matches!(&tg_env.content, MessageContent::Text(t) if t == "Hello from Telegram"));

    // Discord
    let dc_msg = DiscordMessage {
        message_id: "111222333".into(),
        channel_id: "444555666".into(),
        guild_id: Some("777888999".into()),
        author_id: "100200300".into(),
        author_name: "DC User".into(),
        content: "Hello from Discord".into(),
        attachments: vec![],
        timestamp: Utc::now(),
    };
    let dc_env = discord_normalize(&dc_msg);
    assert_eq!(dc_env.peer.platform, Platform::Discord);
    assert!(matches!(&dc_env.content, MessageContent::Text(t) if t == "Hello from Discord"));

    // Slack
    let sl_msg = SlackMessage {
        ts: "1700000000.000001".into(),
        channel_id: "C01234ABCDE".into(),
        user_id: "U01234ABCDE".into(),
        user_name: Some("Slack User".into()),
        text: "Hello from Slack".into(),
        thread_ts: None,
        files: vec![],
    };
    let sl_env = slack_normalize(&sl_msg);
    assert_eq!(sl_env.peer.platform, Platform::Slack);
    assert!(matches!(&sl_env.content, MessageContent::Text(t) if t == "Hello from Slack"));

    // WhatsApp
    let wa_msg = WhatsAppMessage {
        message_id: "wamid.test123".into(),
        from: "14155552671".into(),
        from_name: Some("WA User".into()),
        text: Some("Hello from WhatsApp".into()),
        media: None,
        timestamp: Utc::now(),
    };
    let wa_env = whatsapp_normalize(&wa_msg);
    assert_eq!(wa_env.peer.platform, Platform::WhatsApp);
    assert!(matches!(&wa_env.content, MessageContent::Text(t) if t == "Hello from WhatsApp"));

    // Email
    let em_msg = EmailMessage {
        message_id: "<test@example.com>".into(),
        from_address: "alice@example.com".into(),
        from_name: Some("Alice".into()),
        subject: Some("Test Subject".into()),
        body_text: Some("Hello from Email".into()),
        body_html: None,
        in_reply_to: None,
        attachments: vec![],
        date: Utc::now(),
    };
    let em_env = email_normalize(&em_msg);
    assert_eq!(em_env.peer.platform, Platform::Email);
    assert!(matches!(&em_env.content, MessageContent::Text(t) if t.contains("Hello from Email")));
}

// ---------------------------------------------------------------------------
// 4. Outbound routing — send_chunk to each channel
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_outbound_routing_to_each_channel() {
    // Verify send_chunk succeeds for all channel types.
    // The outbound_rx is private, so we test through the Channel trait that
    // send_chunk completes without error (the internal mpsc has capacity).

    let tg = TelegramChannel::new(telegram_config());
    tg.send_chunk(OutboundChunk::TextBlock("tg response".into()))
        .await
        .unwrap();
    tg.send_chunk(OutboundChunk::Done).await.unwrap();

    let dc = DiscordChannel::new(discord_config());
    dc.send_chunk(OutboundChunk::TextBlock("dc response".into()))
        .await
        .unwrap();
    dc.send_chunk(OutboundChunk::Done).await.unwrap();

    let sl = SlackChannel::new(slack_config());
    sl.send_chunk(OutboundChunk::TextBlock("sl response".into()))
        .await
        .unwrap();
    sl.send_chunk(OutboundChunk::Done).await.unwrap();

    let wa = WhatsAppChannel::new(whatsapp_config());
    wa.send_chunk(OutboundChunk::TextBlock("wa response".into()))
        .await
        .unwrap();
    wa.send_chunk(OutboundChunk::Done).await.unwrap();

    let em = EmailChannel::new(email_config());
    em.send_chunk(OutboundChunk::TextBlock("em response".into()))
        .await
        .unwrap();
    em.send_chunk(OutboundChunk::Done).await.unwrap();
}

// ---------------------------------------------------------------------------
// 5. Inbound envelope injection and retrieval
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_inbound_envelope_from_each_channel() {
    // Telegram
    {
        let mut channel = TelegramChannel::new(telegram_config());
        let sender = channel.sender();
        let env = make_envelope(Platform::Telegram, "tg_user_42", "Hello TG");
        sender.send(env).await.unwrap();
        let received = channel.recv_envelope().await.unwrap().unwrap();
        assert_eq!(received.peer.platform, Platform::Telegram);
        assert!(matches!(&received.content, MessageContent::Text(t) if t == "Hello TG"));
    }

    // Discord
    {
        let mut channel = DiscordChannel::new(discord_config());
        let sender = channel.sender();
        let env = make_envelope(Platform::Discord, "dc_user_42", "Hello DC");
        sender.send(env).await.unwrap();
        let received = channel.recv_envelope().await.unwrap().unwrap();
        assert_eq!(received.peer.platform, Platform::Discord);
        assert!(matches!(&received.content, MessageContent::Text(t) if t == "Hello DC"));
    }

    // Slack
    {
        let mut channel = SlackChannel::new(slack_config());
        let sender = channel.sender();
        let env = make_envelope(Platform::Slack, "sl_user_42", "Hello SL");
        sender.send(env).await.unwrap();
        let received = channel.recv_envelope().await.unwrap().unwrap();
        assert_eq!(received.peer.platform, Platform::Slack);
        assert!(matches!(&received.content, MessageContent::Text(t) if t == "Hello SL"));
    }

    // WhatsApp
    {
        let mut channel = WhatsAppChannel::new(whatsapp_config());
        let sender = channel.sender();
        let env = make_envelope(Platform::WhatsApp, "wa_user_42", "Hello WA");
        sender.send(env).await.unwrap();
        let received = channel.recv_envelope().await.unwrap().unwrap();
        assert_eq!(received.peer.platform, Platform::WhatsApp);
        assert!(matches!(&received.content, MessageContent::Text(t) if t == "Hello WA"));
    }

    // Email
    {
        let mut channel = EmailChannel::new(email_config());
        let sender = channel.sender();
        let env = make_envelope(Platform::Email, "em_user_42", "Hello EM");
        sender.send(env).await.unwrap();
        let received = channel.recv_envelope().await.unwrap().unwrap();
        assert_eq!(received.peer.platform, Platform::Email);
        assert!(matches!(&received.content, MessageContent::Text(t) if t == "Hello EM"));
    }
}

// ---------------------------------------------------------------------------
// 6. ChannelKind <-> Platform mapping
// ---------------------------------------------------------------------------

#[test]
fn test_channel_kind_platform_mapping() {
    // Each adapter's kind() and platform() should match the expected values.
    let tg = TelegramChannel::new(telegram_config());
    assert_eq!(tg.kind(), ChannelKind::Telegram);
    assert_eq!(tg.platform(), Platform::Telegram);

    let dc = DiscordChannel::new(discord_config());
    assert_eq!(dc.kind(), ChannelKind::Discord);
    assert_eq!(dc.platform(), Platform::Discord);

    let sl = SlackChannel::new(slack_config());
    assert_eq!(sl.kind(), ChannelKind::Slack);
    assert_eq!(sl.platform(), Platform::Slack);

    let wa = WhatsAppChannel::new(whatsapp_config());
    assert_eq!(wa.kind(), ChannelKind::WhatsApp);
    assert_eq!(wa.platform(), Platform::WhatsApp);

    let em = EmailChannel::new(email_config());
    assert_eq!(em.kind(), ChannelKind::Email);
    assert_eq!(em.platform(), Platform::Email);

    // Verify Display impls are consistent.
    assert_eq!(ChannelKind::Telegram.to_string(), "telegram");
    assert_eq!(ChannelKind::Discord.to_string(), "discord");
    assert_eq!(ChannelKind::Slack.to_string(), "slack");
    assert_eq!(ChannelKind::WhatsApp.to_string(), "whatsapp");
    assert_eq!(ChannelKind::Email.to_string(), "email");
}

// ---------------------------------------------------------------------------
// 7. Mixed message types across channels
// ---------------------------------------------------------------------------

#[test]
fn test_mixed_message_types_across_channels() {
    // -- Telegram: text messages always produce Text (command parsing is
    //    done in the pipeline, not in the normalize function).
    let tg_text = TelegramMessage {
        message_id: 10,
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
    let tg_env = telegram_normalize(&tg_text);
    assert!(matches!(&tg_env.content, MessageContent::Text(t) if t == "plain text"));

    // Telegram: even /commands are normalized as Text (pipeline handles dispatch)
    let tg_cmd = TelegramMessage {
        message_id: 11,
        chat_id: 100,
        from_user_id: Some(1),
        from_username: Some("alice".into()),
        from_display_name: Some("Alice".into()),
        text: Some("/start hello world".into()),
        caption: None,
        reply_to_message_id: None,
        date: Utc::now(),
        photos: vec![],
        document: None,
    };
    let tg_cmd_env = telegram_normalize(&tg_cmd);
    assert!(
        matches!(&tg_cmd_env.content, MessageContent::Text(t) if t == "/start hello world"),
        "Telegram normalize does not parse commands — that happens in the pipeline"
    );

    // -- Discord: text message
    let dc_text = DiscordMessage {
        message_id: "20".into(),
        channel_id: "200".into(),
        guild_id: Some("2000".into()),
        author_id: "2".into(),
        author_name: "Bob".into(),
        content: "just text".into(),
        attachments: vec![],
        timestamp: Utc::now(),
    };
    let dc_env = discord_normalize(&dc_text);
    assert!(matches!(&dc_env.content, MessageContent::Text(t) if t == "just text"));

    // Discord: /command produces Command with name stripped of "/"
    let dc_cmd = DiscordMessage {
        message_id: "21".into(),
        channel_id: "200".into(),
        guild_id: Some("2000".into()),
        author_id: "2".into(),
        author_name: "Bob".into(),
        content: "/help arg1 arg2".into(),
        attachments: vec![],
        timestamp: Utc::now(),
    };
    let dc_cmd_env = discord_normalize(&dc_cmd);
    assert!(matches!(
        &dc_cmd_env.content,
        MessageContent::Command { name, .. } if name == "help"
    ));

    // -- Slack: text message
    let sl_text = SlackMessage {
        ts: "1700000001.000001".into(),
        channel_id: "C0SLACK".into(),
        user_id: "U0SLACK".into(),
        user_name: Some("Charlie".into()),
        text: "slack text".into(),
        thread_ts: None,
        files: vec![],
    };
    let sl_env = slack_normalize(&sl_text);
    assert!(matches!(&sl_env.content, MessageContent::Text(t) if t == "slack text"));

    // Slack: /command keeps the "/" in the name field
    let sl_cmd = SlackMessage {
        ts: "1700000002.000001".into(),
        channel_id: "C0SLACK".into(),
        user_id: "U0SLACK".into(),
        user_name: Some("Charlie".into()),
        text: "/remind do something".into(),
        thread_ts: None,
        files: vec![],
    };
    let sl_cmd_env = slack_normalize(&sl_cmd);
    assert!(matches!(
        &sl_cmd_env.content,
        MessageContent::Command { name, .. } if name == "/remind"
    ));

    // -- WhatsApp: text message (no command parsing in normalize)
    let wa_text = WhatsAppMessage {
        message_id: "wamid.30".into(),
        from: "14155551234".into(),
        from_name: Some("Dave".into()),
        text: Some("wa plain text".into()),
        media: None,
        timestamp: Utc::now(),
    };
    let wa_env = whatsapp_normalize(&wa_text);
    assert!(matches!(&wa_env.content, MessageContent::Text(t) if t == "wa plain text"));

    // -- Email: text with subject in platform_meta
    let em_text = EmailMessage {
        message_id: "<msg30@example.com>".into(),
        from_address: "eve@example.com".into(),
        from_name: Some("Eve".into()),
        subject: Some("Important".into()),
        body_text: Some("email body".into()),
        body_html: None,
        in_reply_to: None,
        attachments: vec![],
        date: Utc::now(),
    };
    let em_env = email_normalize(&em_text);
    assert!(matches!(&em_env.content, MessageContent::Text(t) if t.contains("email body")));
    assert_eq!(em_env.peer.platform, Platform::Email);

    // Verify all normalized envelopes have unique IDs (UUID v4).
    let ids = vec![
        &tg_env.id,
        &dc_env.id,
        &sl_env.id,
        &wa_env.id,
        &em_env.id,
    ];
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            assert_ne!(ids[i], ids[j], "envelope IDs must be unique");
        }
    }
}
