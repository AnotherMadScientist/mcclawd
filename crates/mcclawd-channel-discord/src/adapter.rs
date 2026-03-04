//! Discord channel adapter.
//!
//! Wraps a Discord bot and implements the [`Channel`] trait for Discord.
//! Uses internal mpsc channels to decouple the serenity event loop from
//! the Channel trait's recv/send pattern.

use async_trait::async_trait;
use mcclawd_channels::envelope::{Envelope, Platform};
use mcclawd_channels::registry::ChannelCapabilities;
use mcclawd_channels::types::{ChannelKind, InboundMessage, OutboundChunk};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing;

use crate::error::DiscordError;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for the Discord channel adapter.
#[derive(Debug, Clone)]
pub struct DiscordConfig {
    /// Bot token from the Discord Developer Portal.
    pub bot_token: String,
    /// Optional allowlist of guild (server) IDs. If set, messages from other
    /// guilds are silently dropped.
    pub allowed_guild_ids: Option<Vec<u64>>,
    /// Optional allowlist of channel IDs. If set, messages from other channels
    /// are silently dropped.
    pub allowed_channel_ids: Option<Vec<u64>>,
}

// ---------------------------------------------------------------------------
// DiscordChannel
// ---------------------------------------------------------------------------

/// Discord channel adapter.
///
/// Uses serenity to receive events and send responses. Internally uses mpsc
/// channels to decouple the serenity event loop from the `Channel` trait's
/// `recv_envelope` / `send_chunk` pattern.
pub struct DiscordChannel {
    config: DiscordConfig,
    /// Receives normalized envelopes from the event handler.
    inbox_rx: mpsc::Receiver<Envelope>,
    /// Sender half -- cloned into the serenity event handler and also available
    /// via [`Self::sender`] for testing.
    inbox_tx: mpsc::Sender<Envelope>,
    /// Outbound chunks are sent via this channel to the serenity send loop.
    outbound_tx: mpsc::Sender<OutboundChunk>,
    /// The serenity send loop reads from here.
    outbound_rx: Option<mpsc::Receiver<OutboundChunk>>,
}

impl DiscordChannel {
    /// Create a new `DiscordChannel` with the given config.
    pub fn new(config: DiscordConfig) -> Self {
        let (inbox_tx, inbox_rx) = mpsc::channel(256);
        let (outbound_tx, outbound_rx) = mpsc::channel(256);
        Self {
            config,
            inbox_rx,
            inbox_tx,
            outbound_tx,
            outbound_rx: Some(outbound_rx),
        }
    }

    /// Get a clone of the inbound sender handle.
    ///
    /// Useful for testing: inject envelopes without a live Discord bot.
    pub fn sender(&self) -> mpsc::Sender<Envelope> {
        self.inbox_tx.clone()
    }

    /// Reference to the channel config.
    pub fn config(&self) -> &DiscordConfig {
        &self.config
    }

    /// Start the serenity event listener in the background.
    ///
    /// Returns a `JoinHandle` that resolves when the listener shuts down.
    /// The listener respects the provided `CancellationToken`.
    pub async fn start_listener(
        &self,
        _shutdown: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>, DiscordError> {
        // TODO(phase-3): Wire up serenity client.
        //   1. Create `serenity::Client::builder(token, intents)`
        //   2. Add an event handler that:
        //      a. Converts serenity::model::channel::Message -> DiscordMessage
        //      b. Calls normalize::normalize()
        //      c. Sends the Envelope through self.inbox_tx
        //   3. Spawn the client, select! with shutdown token
        let handle = tokio::spawn(async {
            tracing::info!("Discord listener placeholder -- not yet wired to serenity");
        });
        Ok(handle)
    }

    /// Discord-specific capabilities.
    pub fn discord_capabilities() -> ChannelCapabilities {
        ChannelCapabilities {
            supports_streaming: false,
            supports_edit: true,
            supports_markdown: true,
            max_message_len: 2000,
            supports_files: true,
            max_file_size: 25 * 1024 * 1024, // 25 MB
        }
    }
}

// ---------------------------------------------------------------------------
// Channel trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl mcclawd_channels::Channel for DiscordChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Discord
    }

    async fn start(
        &self,
        _inbound_tx: mpsc::Sender<InboundMessage>,
        _shutdown: CancellationToken,
    ) -> mcclawd_core::Result<()> {
        // Phase 0 start -- delegate to start_listener in real usage.
        tracing::info!("DiscordChannel::start (Phase 3 stub)");
        Ok(())
    }

    async fn send_chunk(&self, chunk: OutboundChunk) -> mcclawd_core::Result<()> {
        self.outbound_tx
            .send(chunk)
            .await
            .map_err(|e| mcclawd_core::McclawdError::Channel(format!("outbound send failed: {e}")))?;
        Ok(())
    }

    async fn recv_envelope(&mut self) -> anyhow::Result<Option<Envelope>> {
        Ok(self.inbox_rx.recv().await)
    }

    fn capabilities(&self) -> ChannelCapabilities {
        Self::discord_capabilities()
    }

    fn platform(&self) -> Platform {
        Platform::Discord
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use mcclawd_channels::envelope::{MessageContent, Peer};
    use mcclawd_channels::Channel;

    fn test_config() -> DiscordConfig {
        DiscordConfig {
            bot_token: "FAKE_DISCORD_TOKEN".into(),
            allowed_guild_ids: None,
            allowed_channel_ids: None,
        }
    }

    fn test_envelope(text: &str) -> Envelope {
        Envelope {
            id: uuid::Uuid::new_v4().to_string(),
            peer: Peer {
                id: "42".into(),
                display_name: Some("Test User".into()),
                platform: Platform::Discord,
            },
            thread: None,
            content: MessageContent::Text(text.into()),
            attachments: vec![],
            timestamp: Utc::now(),
            platform_meta: serde_json::json!({"channel_id": "123456"}),
        }
    }

    #[tokio::test]
    async fn receive_injected_message() {
        let mut channel = DiscordChannel::new(test_config());
        let sender = channel.sender();

        sender.send(test_envelope("Hello")).await.unwrap();
        let msg = channel.recv_envelope().await.unwrap().unwrap();

        assert!(matches!(&msg.content, MessageContent::Text(t) if t == "Hello"));
        assert_eq!(msg.peer.id, "42");
    }

    #[tokio::test]
    async fn receive_multiple_messages_in_order() {
        let mut channel = DiscordChannel::new(test_config());
        let sender = channel.sender();

        sender.send(test_envelope("first")).await.unwrap();
        sender.send(test_envelope("second")).await.unwrap();
        sender.send(test_envelope("third")).await.unwrap();

        let m1 = channel.recv_envelope().await.unwrap().unwrap();
        let m2 = channel.recv_envelope().await.unwrap().unwrap();
        let m3 = channel.recv_envelope().await.unwrap().unwrap();

        assert!(matches!(&m1.content, MessageContent::Text(t) if t == "first"));
        assert!(matches!(&m2.content, MessageContent::Text(t) if t == "second"));
        assert!(matches!(&m3.content, MessageContent::Text(t) if t == "third"));
    }

    #[tokio::test]
    async fn send_chunk_is_forwarded() {
        let mut channel = DiscordChannel::new(test_config());
        let mut outbound_rx = channel.outbound_rx.take().unwrap();

        channel
            .send_chunk(OutboundChunk::TextBlock("response".into()))
            .await
            .unwrap();

        let chunk = outbound_rx.recv().await.unwrap();
        assert!(matches!(chunk, OutboundChunk::TextBlock(t) if t == "response"));
    }

    #[tokio::test]
    async fn send_chunk_done_signal() {
        let mut channel = DiscordChannel::new(test_config());
        let mut outbound_rx = channel.outbound_rx.take().unwrap();

        channel.send_chunk(OutboundChunk::Done).await.unwrap();

        let chunk = outbound_rx.recv().await.unwrap();
        assert!(matches!(chunk, OutboundChunk::Done));
    }

    #[test]
    fn capabilities_are_correct() {
        let caps = DiscordChannel::discord_capabilities();
        assert!(caps.supports_edit);
        assert!(caps.supports_markdown);
        assert!(caps.supports_files);
        assert!(!caps.supports_streaming);
        assert_eq!(caps.max_message_len, 2000);
        assert_eq!(caps.max_file_size, 25 * 1024 * 1024);
    }

    #[test]
    fn kind_is_discord() {
        let channel = DiscordChannel::new(test_config());
        assert_eq!(channel.kind(), ChannelKind::Discord);
    }

    #[test]
    fn platform_is_discord() {
        let channel = DiscordChannel::new(test_config());
        assert_eq!(channel.platform(), Platform::Discord);
    }

    #[test]
    fn config_creation() {
        let config = DiscordConfig {
            bot_token: "my-token".into(),
            allowed_guild_ids: Some(vec![123, 456]),
            allowed_channel_ids: Some(vec![789]),
        };
        assert_eq!(config.bot_token, "my-token");
        assert_eq!(config.allowed_guild_ids.as_ref().unwrap().len(), 2);
        assert_eq!(config.allowed_channel_ids.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn config_no_allowlist() {
        let config = DiscordConfig {
            bot_token: "token".into(),
            allowed_guild_ids: None,
            allowed_channel_ids: None,
        };
        assert!(config.allowed_guild_ids.is_none());
        assert!(config.allowed_channel_ids.is_none());
    }

    #[test]
    fn sender_clone_works() {
        let channel = DiscordChannel::new(test_config());
        let s1 = channel.sender();
        let s2 = channel.sender();
        // Both senders are functional (not closed).
        assert!(!s1.is_closed());
        assert!(!s2.is_closed());
    }
}
