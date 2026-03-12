//! WhatsApp channel adapter.
//!
//! Wraps the WhatsApp Cloud API and implements the [`Channel`] trait.
//! Uses internal mpsc channels to decouple the webhook handler from
//! the Channel trait's recv/send pattern.

use async_trait::async_trait;
use mcclawd_channels::envelope::{Envelope, Platform};
use mcclawd_channels::registry::ChannelCapabilities;
use mcclawd_channels::types::{ChannelKind, InboundMessage, OutboundChunk};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;
use tracing;

use crate::error::WhatsAppError;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for the WhatsApp channel adapter.
#[derive(Debug, Clone)]
pub struct WhatsAppConfig {
    /// WhatsApp Business phone number ID (from Meta Business dashboard).
    pub phone_number_id: String,
    /// Permanent access token for the WhatsApp Cloud API.
    pub access_token: String,
    /// Verify token for webhook registration (chosen by the operator).
    pub verify_token: String,
    /// Optional allowlist of phone numbers. If set, messages from other
    /// numbers are silently dropped. Numbers in E.164 format (e.g. "14155552671").
    pub allowed_numbers: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// State (for persistence)
// ---------------------------------------------------------------------------

/// Serializable snapshot of WhatsApp channel state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WhatsAppState {
    /// Timestamp of the last processed webhook message.
    pub last_message_ts: Option<String>,
}

// ---------------------------------------------------------------------------
// WhatsAppChannel
// ---------------------------------------------------------------------------

/// WhatsApp channel adapter.
///
/// Uses the WhatsApp Cloud API to receive webhooks and send responses.
/// Internally uses mpsc channels to decouple the webhook handler from
/// the `Channel` trait's `recv_envelope` / `send_chunk` pattern.
pub struct WhatsAppChannel {
    config: WhatsAppConfig,
    /// Receives normalized envelopes from the webhook handler.
    inbox_rx: mpsc::Receiver<Envelope>,
    /// Sender half -- cloned into the webhook handler and also available
    /// via [`Self::sender`] for testing.
    inbox_tx: mpsc::Sender<Envelope>,
    /// Outbound chunks are sent via this channel to the Cloud API send loop.
    outbound_tx: mpsc::Sender<OutboundChunk>,
    /// The Cloud API send loop reads from here.
    outbound_rx: Option<mpsc::Receiver<OutboundChunk>>,
    /// Persisted webhook state.
    state: RwLock<WhatsAppState>,
}

impl WhatsAppChannel {
    /// Create a new `WhatsAppChannel` with the given config.
    pub fn new(config: WhatsAppConfig) -> Self {
        let (inbox_tx, inbox_rx) = mpsc::channel(256);
        let (outbound_tx, outbound_rx) = mpsc::channel(256);
        Self {
            config,
            inbox_rx,
            inbox_tx,
            outbound_tx,
            outbound_rx: Some(outbound_rx),
            state: RwLock::new(WhatsAppState::default()),
        }
    }

    /// Get a clone of the inbound sender handle.
    ///
    /// Useful for testing: inject envelopes without a live webhook.
    pub fn sender(&self) -> mpsc::Sender<Envelope> {
        self.inbox_tx.clone()
    }

    /// Reference to the channel config.
    pub fn config(&self) -> &WhatsAppConfig {
        &self.config
    }

    /// Start the webhook listener in the background.
    ///
    /// Returns a `JoinHandle` that resolves when the listener shuts down.
    /// The listener respects the provided `CancellationToken`.
    pub async fn start_webhook(
        &self,
        _shutdown: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>, WhatsAppError> {
        // Phase 3: Wire up webhook handler.
        //   1. Start an Axum/Actix HTTP server on a configured port
        //   2. Handle GET /webhook for verification (verify_token)
        //   3. Handle POST /webhook for incoming messages:
        //      a. Parse webhook payload → WhatsAppMessage
        //      b. Call normalize::normalize()
        //      c. Send the Envelope through self.inbox_tx
        //   4. For outbound: read from outbound_rx, POST to Cloud API
        //      https://graph.facebook.com/v21.0/{phone_number_id}/messages
        Err(WhatsAppError::NotAvailable(
            "WhatsApp webhook requires Cloud API integration (Phase 3)".into(),
        ))
    }

    /// WhatsApp-specific capabilities.
    pub fn whatsapp_capabilities() -> ChannelCapabilities {
        ChannelCapabilities {
            supports_streaming: false, // WhatsApp has no edit-in-place
            supports_edit: false,      // Messages cannot be edited once sent
            supports_markdown: false,  // WhatsApp uses its own formatting (*bold*, _italic_)
            max_message_len: 4096,
            supports_files: true,
            max_file_size: 100 * 1024 * 1024, // 100 MB
        }
    }
}

// ---------------------------------------------------------------------------
// Channel trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl mcclawd_channels::Channel for WhatsAppChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::WhatsApp
    }

    async fn start(
        &self,
        _inbound_tx: mpsc::Sender<InboundMessage>,
        _shutdown: CancellationToken,
    ) -> mcclawd_core::Result<()> {
        Err(mcclawd_core::McclawdError::Channel(
            "WhatsApp channel adapter is not yet available (Phase 3). Configure Cloud API webhook to enable.".into(),
        ))
    }

    async fn send_chunk(&self, chunk: OutboundChunk) -> mcclawd_core::Result<()> {
        self.outbound_tx
            .send(chunk)
            .await
            .map_err(|e| {
                mcclawd_core::McclawdError::Channel(format!("outbound send failed: {e}"))
            })?;
        Ok(())
    }

    async fn recv_envelope(&mut self) -> anyhow::Result<Option<Envelope>> {
        Ok(self.inbox_rx.recv().await)
    }

    fn capabilities(&self) -> ChannelCapabilities {
        Self::whatsapp_capabilities()
    }

    fn platform(&self) -> Platform {
        Platform::WhatsApp
    }

    async fn save_state(&self) -> anyhow::Result<Option<Vec<u8>>> {
        let state = self.state.read().await.clone();
        let bytes = serde_json::to_vec(&state)?;
        Ok(Some(bytes))
    }

    async fn restore_state(&self, state: Option<Vec<u8>>) -> anyhow::Result<()> {
        if let Some(data) = state {
            match serde_json::from_slice::<WhatsAppState>(&data) {
                Ok(s) => {
                    *self.state.write().await = s.clone();
                    tracing::info!(last_message_ts = ?s.last_message_ts, "WhatsApp state restored");
                }
                Err(e) => {
                    tracing::warn!("Corrupt WhatsApp state, starting fresh: {e}");
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

    fn test_config() -> WhatsAppConfig {
        WhatsAppConfig {
            phone_number_id: "123456789".into(),
            access_token: "FAKE_ACCESS_TOKEN".into(),
            verify_token: "my_verify_token".into(),
            allowed_numbers: None,
        }
    }

    fn test_envelope(text: &str) -> Envelope {
        Envelope {
            id: uuid::Uuid::new_v4().to_string(),
            peer: Peer {
                id: "14155552671".into(),
                display_name: Some("Test User".into()),
                platform: Platform::WhatsApp,
            },
            thread: None,
            content: MessageContent::Text(text.into()),
            attachments: vec![],
            timestamp: Utc::now(),
            platform_meta: serde_json::json!({"message_id": "wamid.test123"}),
        }
    }

    #[tokio::test]
    async fn receive_injected_message() {
        let mut channel = WhatsAppChannel::new(test_config());
        let sender = channel.sender();

        sender.send(test_envelope("Hello")).await.unwrap();
        let msg = channel.recv_envelope().await.unwrap().unwrap();

        assert!(matches!(&msg.content, MessageContent::Text(t) if t == "Hello"));
        assert_eq!(msg.peer.id, "14155552671");
    }

    #[tokio::test]
    async fn receive_multiple_messages_in_order() {
        let mut channel = WhatsAppChannel::new(test_config());
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
        let mut channel = WhatsAppChannel::new(test_config());
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
        let mut channel = WhatsAppChannel::new(test_config());
        let mut outbound_rx = channel.outbound_rx.take().unwrap();

        channel.send_chunk(OutboundChunk::Done).await.unwrap();

        let chunk = outbound_rx.recv().await.unwrap();
        assert!(matches!(chunk, OutboundChunk::Done));
    }

    #[test]
    fn capabilities_are_correct() {
        let caps = WhatsAppChannel::whatsapp_capabilities();
        assert!(!caps.supports_edit);
        assert!(!caps.supports_markdown);
        assert!(caps.supports_files);
        assert!(!caps.supports_streaming);
        assert_eq!(caps.max_message_len, 4096);
        assert_eq!(caps.max_file_size, 100 * 1024 * 1024);
    }

    #[test]
    fn kind_is_whatsapp() {
        let channel = WhatsAppChannel::new(test_config());
        assert_eq!(channel.kind(), ChannelKind::WhatsApp);
    }

    #[test]
    fn platform_is_whatsapp() {
        let channel = WhatsAppChannel::new(test_config());
        assert_eq!(channel.platform(), Platform::WhatsApp);
    }

    #[test]
    fn config_creation() {
        let config = WhatsAppConfig {
            phone_number_id: "123".into(),
            access_token: "token".into(),
            verify_token: "verify".into(),
            allowed_numbers: Some(vec!["14155552671".into()]),
        };
        assert_eq!(config.phone_number_id, "123");
        assert_eq!(config.allowed_numbers.unwrap().len(), 1);
    }

    #[test]
    fn config_no_allowlist() {
        let config = WhatsAppConfig {
            phone_number_id: "123".into(),
            access_token: "token".into(),
            verify_token: "verify".into(),
            allowed_numbers: None,
        };
        assert!(config.allowed_numbers.is_none());
    }

    #[test]
    fn sender_clone_works() {
        let channel = WhatsAppChannel::new(test_config());
        let s1 = channel.sender();
        let s2 = channel.sender();
        // Both senders are functional (not closed).
        assert!(!s1.is_closed());
        assert!(!s2.is_closed());
    }

    #[tokio::test]
    async fn save_restore_state_roundtrip() {
        let channel = WhatsAppChannel::new(test_config());
        {
            let mut state = channel.state.write().await;
            state.last_message_ts = Some("1709000000".into());
        }

        let saved = channel.save_state().await.unwrap().unwrap();
        let state: WhatsAppState = serde_json::from_slice(&saved).unwrap();
        assert_eq!(state.last_message_ts, Some("1709000000".into()));

        let channel2 = WhatsAppChannel::new(test_config());
        channel2.restore_state(Some(saved)).await.unwrap();
        let restored = channel2.state.read().await;
        assert_eq!(restored.last_message_ts, Some("1709000000".into()));
    }

    #[tokio::test]
    async fn restore_none_state_is_ok() {
        let channel = WhatsAppChannel::new(test_config());
        channel.restore_state(None).await.unwrap();
        let state = channel.state.read().await;
        assert!(state.last_message_ts.is_none());
    }

    #[tokio::test]
    async fn restore_corrupt_state_is_ok() {
        let channel = WhatsAppChannel::new(test_config());
        channel.restore_state(Some(b"nope".to_vec())).await.unwrap();
        let state = channel.state.read().await;
        assert!(state.last_message_ts.is_none());
    }
}
