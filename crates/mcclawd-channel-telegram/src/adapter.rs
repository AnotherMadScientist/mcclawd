//! Telegram channel adapter.
//!
//! Wraps a teloxide bot and implements the [`Channel`] trait for Telegram.
//! Uses internal mpsc channels to decouple the teloxide update loop from
//! the Channel trait's recv/send pattern.

use async_trait::async_trait;
use mcclawd_channels::envelope::{Envelope, Platform};
use mcclawd_channels::registry::ChannelCapabilities;
use mcclawd_channels::types::{ChannelKind, InboundMessage, OutboundChunk};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing;

use crate::error::TelegramError;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for the Telegram channel adapter.
#[derive(Debug, Clone)]
pub struct TelegramConfig {
    /// Bot API token from @BotFather.
    pub bot_token: String,
    /// Optional allowlist of chat IDs. If set, messages from other chats are
    /// silently dropped.
    pub allowed_chat_ids: Option<Vec<i64>>,
}

// ---------------------------------------------------------------------------
// State (for persistence)
// ---------------------------------------------------------------------------

/// Serializable snapshot of Telegram channel state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramState {
    /// The last update_id successfully processed. Polling resumes from
    /// `last_update_id + 1` to avoid re-processing.
    pub last_update_id: i64,
}

// ---------------------------------------------------------------------------
// TelegramChannel
// ---------------------------------------------------------------------------

/// Telegram channel adapter.
///
/// Uses teloxide to receive updates and send responses. Internally uses mpsc
/// channels to decouple the teloxide update loop from the `Channel` trait's
/// `recv_envelope` / `send_chunk` pattern.
pub struct TelegramChannel {
    config: TelegramConfig,
    /// Receives normalized envelopes from the update handler.
    inbox_rx: mpsc::Receiver<Envelope>,
    /// Sender half — cloned into the teloxide dispatcher and also available
    /// via [`Self::sender`] for testing.
    inbox_tx: mpsc::Sender<Envelope>,
    /// Outbound chunks are sent via this channel to the teloxide send loop.
    outbound_tx: mpsc::Sender<OutboundChunk>,
    /// The teloxide send loop reads from here.
    outbound_rx: Option<mpsc::Receiver<OutboundChunk>>,
    /// Last processed update ID (persisted across restarts).
    last_update_id: AtomicI64,
}

impl TelegramChannel {
    /// Create a new `TelegramChannel` with the given config.
    pub fn new(config: TelegramConfig) -> Self {
        let (inbox_tx, inbox_rx) = mpsc::channel(256);
        let (outbound_tx, outbound_rx) = mpsc::channel(256);
        Self {
            config,
            inbox_rx,
            inbox_tx,
            outbound_tx,
            outbound_rx: Some(outbound_rx),
            last_update_id: AtomicI64::new(0),
        }
    }

    /// Get a clone of the inbound sender handle.
    ///
    /// Useful for testing: inject envelopes without a live Telegram bot.
    pub fn sender(&self) -> mpsc::Sender<Envelope> {
        self.inbox_tx.clone()
    }

    /// Reference to the channel config.
    pub fn config(&self) -> &TelegramConfig {
        &self.config
    }

    /// Start the teloxide update listener in the background.
    ///
    /// Returns a `JoinHandle` that resolves when the listener shuts down.
    /// The listener respects the provided `CancellationToken`.
    pub async fn start_listener(
        &self,
        _shutdown: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>, TelegramError> {
        // TODO(phase-2): Wire up teloxide dispatcher.
        //   1. Create `teloxide::Bot::new(self.config.bot_token)`
        //   2. Build a `Dispatcher` with an update handler that:
        //      a. Converts teloxide::types::Message → TelegramMessage
        //      b. Calls normalize::normalize()
        //      c. Sends the Envelope through self.inbox_tx
        //   3. Spawn the dispatcher, select! with shutdown token
        let handle = tokio::spawn(async {
            tracing::info!("Telegram listener placeholder — not yet wired to teloxide");
        });
        Ok(handle)
    }

    /// Telegram-specific capabilities.
    pub fn telegram_capabilities() -> ChannelCapabilities {
        ChannelCapabilities {
            supports_streaming: false, // Telegram edits are rate-limited
            supports_edit: true,
            supports_markdown: true,
            max_message_len: 4096,
            supports_files: true,
            max_file_size: 50 * 1024 * 1024, // 50 MB
        }
    }
}

// ---------------------------------------------------------------------------
// Channel trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl mcclawd_channels::Channel for TelegramChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Telegram
    }

    async fn start(
        &self,
        _inbound_tx: mpsc::Sender<InboundMessage>,
        _shutdown: CancellationToken,
    ) -> mcclawd_core::Result<()> {
        // Phase 0 start — delegate to start_listener in real usage.
        tracing::info!("TelegramChannel::start (Phase 0 stub)");
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
        Self::telegram_capabilities()
    }

    fn platform(&self) -> Platform {
        Platform::Telegram
    }

    async fn save_state(&self) -> anyhow::Result<Option<Vec<u8>>> {
        let state = TelegramState {
            last_update_id: self.last_update_id.load(Ordering::Relaxed),
        };
        let bytes = serde_json::to_vec(&state)?;
        Ok(Some(bytes))
    }

    async fn restore_state(&self, state: Option<Vec<u8>>) -> anyhow::Result<()> {
        if let Some(data) = state {
            match serde_json::from_slice::<TelegramState>(&data) {
                Ok(s) => {
                    self.last_update_id.store(s.last_update_id, Ordering::Relaxed);
                    tracing::info!(last_update_id = s.last_update_id, "Telegram state restored");
                }
                Err(e) => {
                    tracing::warn!("Corrupt Telegram state, starting fresh: {e}");
                }
            }
        }
        Ok(())
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

    fn test_config() -> TelegramConfig {
        TelegramConfig {
            bot_token: "123:FAKE_TOKEN".into(),
            allowed_chat_ids: None,
        }
    }

    fn test_envelope(text: &str) -> Envelope {
        Envelope {
            id: uuid::Uuid::new_v4().to_string(),
            peer: Peer {
                id: "42".into(),
                display_name: Some("Test User".into()),
                platform: Platform::Telegram,
            },
            thread: None,
            content: MessageContent::Text(text.into()),
            attachments: vec![],
            timestamp: Utc::now(),
            platform_meta: serde_json::json!({"chat_id": -1001234}),
        }
    }

    #[tokio::test]
    async fn receive_injected_message() {
        let mut channel = TelegramChannel::new(test_config());
        let sender = channel.sender();

        sender.send(test_envelope("Hello")).await.unwrap();
        let msg = channel.recv_envelope().await.unwrap().unwrap();

        assert!(matches!(&msg.content, MessageContent::Text(t) if t == "Hello"));
        assert_eq!(msg.peer.id, "42");
    }

    #[tokio::test]
    async fn receive_multiple_messages_in_order() {
        let mut channel = TelegramChannel::new(test_config());
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
        let mut channel = TelegramChannel::new(test_config());
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
        let mut channel = TelegramChannel::new(test_config());
        let mut outbound_rx = channel.outbound_rx.take().unwrap();

        channel.send_chunk(OutboundChunk::Done).await.unwrap();

        let chunk = outbound_rx.recv().await.unwrap();
        assert!(matches!(chunk, OutboundChunk::Done));
    }

    #[test]
    fn capabilities_are_correct() {
        let caps = TelegramChannel::telegram_capabilities();
        assert!(caps.supports_edit);
        assert!(caps.supports_markdown);
        assert!(caps.supports_files);
        assert!(!caps.supports_streaming);
        assert_eq!(caps.max_message_len, 4096);
        assert_eq!(caps.max_file_size, 50 * 1024 * 1024);
    }

    #[test]
    fn kind_is_telegram() {
        let channel = TelegramChannel::new(test_config());
        assert_eq!(channel.kind(), ChannelKind::Telegram);
    }

    #[test]
    fn platform_is_telegram() {
        let channel = TelegramChannel::new(test_config());
        assert_eq!(channel.platform(), Platform::Telegram);
    }

    #[test]
    fn config_creation() {
        let config = TelegramConfig {
            bot_token: "123:ABC".into(),
            allowed_chat_ids: Some(vec![-1001234]),
        };
        assert_eq!(config.bot_token, "123:ABC");
        assert_eq!(config.allowed_chat_ids.unwrap().len(), 1);
    }

    #[test]
    fn config_no_allowlist() {
        let config = TelegramConfig {
            bot_token: "token".into(),
            allowed_chat_ids: None,
        };
        assert!(config.allowed_chat_ids.is_none());
    }

    #[test]
    fn sender_clone_works() {
        let channel = TelegramChannel::new(test_config());
        let s1 = channel.sender();
        let s2 = channel.sender();
        // Both senders are functional (not closed).
        assert!(!s1.is_closed());
        assert!(!s2.is_closed());
    }

    #[tokio::test]
    async fn save_restore_state_roundtrip() {
        let channel = TelegramChannel::new(test_config());
        channel.last_update_id.store(42, Ordering::Relaxed);

        let saved = channel.save_state().await.unwrap().unwrap();
        let state: TelegramState = serde_json::from_slice(&saved).unwrap();
        assert_eq!(state.last_update_id, 42);

        // Restore into a fresh channel
        let channel2 = TelegramChannel::new(test_config());
        channel2.restore_state(Some(saved)).await.unwrap();
        assert_eq!(channel2.last_update_id.load(Ordering::Relaxed), 42);
    }

    #[tokio::test]
    async fn restore_none_state_is_ok() {
        let channel = TelegramChannel::new(test_config());
        channel.restore_state(None).await.unwrap();
        assert_eq!(channel.last_update_id.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn restore_corrupt_state_is_ok() {
        let channel = TelegramChannel::new(test_config());
        channel.restore_state(Some(b"not json".to_vec())).await.unwrap();
        assert_eq!(channel.last_update_id.load(Ordering::Relaxed), 0);
    }
}
