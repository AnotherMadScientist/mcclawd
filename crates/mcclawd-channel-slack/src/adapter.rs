//! Slack channel adapter.
//!
//! Wraps a Slack bot and implements the [`Channel`] trait for Slack.
//! Uses internal mpsc channels to decouple the Slack event loop from
//! the Channel trait's recv/send pattern.

use async_trait::async_trait;
use mcclawd_channels::envelope::{Envelope, Platform};
use mcclawd_channels::registry::ChannelCapabilities;
use mcclawd_channels::types::{ChannelKind, InboundMessage, OutboundChunk};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing;

use crate::error::SlackError;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for the Slack channel adapter.
#[derive(Debug, Clone)]
pub struct SlackConfig {
    /// Slack bot OAuth token (xoxb-...).
    pub bot_token: String,
    /// Slack app-level token for Socket Mode (xapp-...).
    /// If set, the adapter uses Socket Mode instead of Events API webhooks.
    pub app_token: Option<String>,
    /// Optional allowlist of channel IDs. If set, messages from other
    /// channels are silently dropped.
    pub allowed_channel_ids: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// SlackChannel
// ---------------------------------------------------------------------------

/// Slack channel adapter.
///
/// Uses the Slack Events API (or Socket Mode) to receive messages and the
/// Web API to send responses. Internally uses mpsc channels to decouple
/// the event loop from the `Channel` trait's `recv_envelope` / `send_chunk`
/// pattern.
pub struct SlackChannel {
    config: SlackConfig,
    /// Receives normalized envelopes from the event handler.
    inbox_rx: mpsc::Receiver<Envelope>,
    /// Sender half -- cloned into the event handler and also available
    /// via [`Self::sender`] for testing.
    inbox_tx: mpsc::Sender<Envelope>,
    /// Outbound chunks are sent via this channel to the Slack send loop.
    outbound_tx: mpsc::Sender<OutboundChunk>,
    /// The Slack send loop reads from here.
    outbound_rx: Option<mpsc::Receiver<OutboundChunk>>,
}

impl SlackChannel {
    /// Create a new `SlackChannel` with the given config.
    pub fn new(config: SlackConfig) -> Self {
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
    /// Useful for testing: inject envelopes without a live Slack bot.
    pub fn sender(&self) -> mpsc::Sender<Envelope> {
        self.inbox_tx.clone()
    }

    /// Reference to the channel config.
    pub fn config(&self) -> &SlackConfig {
        &self.config
    }

    /// Start the Slack event listener in the background.
    ///
    /// Returns a `JoinHandle` that resolves when the listener shuts down.
    /// The listener respects the provided `CancellationToken`.
    pub async fn start_listener(
        &self,
        _shutdown: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>, SlackError> {
        // TODO(phase-3): Wire up Slack event handler.
        //   1. If app_token is set, use Socket Mode via websocket
        //   2. Otherwise, expect Events API webhook delivery
        //   3. On each message event:
        //      a. Check allowed_channel_ids filter
        //      b. Convert to SlackMessage intermediate type
        //      c. Call normalize::normalize()
        //      d. Send the Envelope through self.inbox_tx
        //   4. Spawn the handler, select! with shutdown token
        let handle = tokio::spawn(async {
            tracing::info!("Slack listener placeholder -- not yet wired to slack-morphism");
        });
        Ok(handle)
    }

    /// Slack-specific capabilities.
    pub fn slack_capabilities() -> ChannelCapabilities {
        ChannelCapabilities {
            supports_streaming: false,
            supports_edit: true,
            supports_markdown: true,
            max_message_len: 40_000,
            supports_files: true,
            max_file_size: 1024 * 1024 * 1024, // 1 GB
        }
    }
}

// ---------------------------------------------------------------------------
// Channel trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl mcclawd_channels::Channel for SlackChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Slack
    }

    async fn start(
        &self,
        _inbound_tx: mpsc::Sender<InboundMessage>,
        _shutdown: CancellationToken,
    ) -> mcclawd_core::Result<()> {
        // Phase 0 start -- delegate to start_listener in real usage.
        tracing::info!("SlackChannel::start (Phase 0 stub)");
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
        Self::slack_capabilities()
    }

    fn platform(&self) -> Platform {
        Platform::Slack
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

    fn test_config() -> SlackConfig {
        SlackConfig {
            bot_token: "xoxb-FAKE-TOKEN".into(),
            app_token: None,
            allowed_channel_ids: None,
        }
    }

    fn test_envelope(text: &str) -> Envelope {
        Envelope {
            id: uuid::Uuid::new_v4().to_string(),
            peer: Peer {
                id: "U01234ABCDE".into(),
                display_name: Some("Test User".into()),
                platform: Platform::Slack,
            },
            thread: None,
            content: MessageContent::Text(text.into()),
            attachments: vec![],
            timestamp: Utc::now(),
            platform_meta: serde_json::json!({"channel_id": "C01ABCDEF", "ts": "1234567890.123456"}),
        }
    }

    #[tokio::test]
    async fn receive_injected_message() {
        let mut channel = SlackChannel::new(test_config());
        let sender = channel.sender();

        sender.send(test_envelope("Hello")).await.unwrap();
        let msg = channel.recv_envelope().await.unwrap().unwrap();

        assert!(matches!(&msg.content, MessageContent::Text(t) if t == "Hello"));
        assert_eq!(msg.peer.id, "U01234ABCDE");
    }

    #[tokio::test]
    async fn receive_multiple_messages_in_order() {
        let mut channel = SlackChannel::new(test_config());
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
        let mut channel = SlackChannel::new(test_config());
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
        let mut channel = SlackChannel::new(test_config());
        let mut outbound_rx = channel.outbound_rx.take().unwrap();

        channel.send_chunk(OutboundChunk::Done).await.unwrap();

        let chunk = outbound_rx.recv().await.unwrap();
        assert!(matches!(chunk, OutboundChunk::Done));
    }

    #[test]
    fn capabilities_are_correct() {
        let caps = SlackChannel::slack_capabilities();
        assert!(caps.supports_edit);
        assert!(caps.supports_markdown);
        assert!(caps.supports_files);
        assert!(!caps.supports_streaming);
        assert_eq!(caps.max_message_len, 40_000);
        assert_eq!(caps.max_file_size, 1024 * 1024 * 1024);
    }

    #[test]
    fn kind_is_slack() {
        let channel = SlackChannel::new(test_config());
        assert_eq!(channel.kind(), ChannelKind::Slack);
    }

    #[test]
    fn platform_is_slack() {
        let channel = SlackChannel::new(test_config());
        assert_eq!(channel.platform(), Platform::Slack);
    }

    #[test]
    fn config_creation() {
        let config = SlackConfig {
            bot_token: "xoxb-123".into(),
            app_token: Some("xapp-456".into()),
            allowed_channel_ids: Some(vec!["C01ABCDEF".into()]),
        };
        assert_eq!(config.bot_token, "xoxb-123");
        assert_eq!(config.app_token, Some("xapp-456".into()));
        assert_eq!(config.allowed_channel_ids.unwrap().len(), 1);
    }

    #[test]
    fn config_no_allowlist() {
        let config = SlackConfig {
            bot_token: "xoxb-token".into(),
            app_token: None,
            allowed_channel_ids: None,
        };
        assert!(config.allowed_channel_ids.is_none());
        assert!(config.app_token.is_none());
    }

    #[test]
    fn sender_clone_works() {
        let channel = SlackChannel::new(test_config());
        let s1 = channel.sender();
        let s2 = channel.sender();
        // Both senders are functional (not closed).
        assert!(!s1.is_closed());
        assert!(!s2.is_closed());
    }
}
