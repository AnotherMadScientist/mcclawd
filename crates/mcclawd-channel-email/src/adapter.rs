//! Email channel adapter.
//!
//! Wraps IMAP (inbound) and SMTP (outbound) and implements the [`Channel`]
//! trait for Email. Uses internal mpsc channels to decouple the IMAP poll
//! loop from the Channel trait's recv/send pattern.

use async_trait::async_trait;
use mcclawd_channels::envelope::{Envelope, Platform};
use mcclawd_channels::registry::ChannelCapabilities;
use mcclawd_channels::types::{ChannelKind, InboundMessage, OutboundChunk};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing;

use crate::error::EmailError;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for the Email channel adapter.
#[derive(Debug, Clone)]
pub struct EmailConfig {
    /// IMAP server hostname.
    pub imap_host: String,
    /// IMAP server port (typically 993 for TLS).
    pub imap_port: u16,
    /// SMTP server hostname.
    pub smtp_host: String,
    /// SMTP server port (typically 587 for STARTTLS, 465 for TLS).
    pub smtp_port: u16,
    /// Login username (usually the email address).
    pub username: String,
    /// Login password or app-specific password.
    pub password: String,
    /// The `From:` address for outbound emails.
    pub from_address: String,
    /// Optional allowlist of sender addresses. If set, emails from other
    /// senders are silently dropped.
    pub allowed_senders: Option<Vec<String>>,
    /// How often to poll IMAP for new messages, in seconds.
    pub poll_interval_secs: u64,
}

// ---------------------------------------------------------------------------
// State (for persistence)
// ---------------------------------------------------------------------------

/// Serializable snapshot of Email channel state (IMAP cursor).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailState {
    /// IMAP UIDVALIDITY — changes when UIDs are no longer valid.
    pub uid_validity: u32,
    /// Last fetched IMAP UID. Polling resumes from `last_uid + 1`.
    pub last_uid: u32,
}

// ---------------------------------------------------------------------------
// EmailChannel
// ---------------------------------------------------------------------------

/// Email channel adapter.
///
/// Uses IMAP to receive emails and SMTP to send responses. Internally uses
/// mpsc channels to decouple the IMAP poll loop from the `Channel` trait's
/// `recv_envelope` / `send_chunk` pattern.
pub struct EmailChannel {
    config: EmailConfig,
    /// Receives normalized envelopes from the IMAP poll loop.
    inbox_rx: mpsc::Receiver<Envelope>,
    /// Sender half -- cloned into the IMAP poll task and also available
    /// via [`Self::sender`] for testing.
    inbox_tx: mpsc::Sender<Envelope>,
    /// Outbound chunks are sent via this channel to the SMTP send loop.
    outbound_tx: mpsc::Sender<OutboundChunk>,
    /// The SMTP send loop reads from here.
    outbound_rx: Option<mpsc::Receiver<OutboundChunk>>,
    /// IMAP UIDVALIDITY (persisted across restarts).
    uid_validity: AtomicU32,
    /// Last fetched IMAP UID (persisted across restarts).
    last_uid: AtomicU32,
}

impl EmailChannel {
    /// Create a new `EmailChannel` with the given config.
    pub fn new(config: EmailConfig) -> Self {
        let (inbox_tx, inbox_rx) = mpsc::channel(256);
        let (outbound_tx, outbound_rx) = mpsc::channel(256);
        Self {
            config,
            inbox_rx,
            inbox_tx,
            outbound_tx,
            outbound_rx: Some(outbound_rx),
            uid_validity: AtomicU32::new(0),
            last_uid: AtomicU32::new(0),
        }
    }

    /// Get a clone of the inbound sender handle.
    ///
    /// Useful for testing: inject envelopes without a live IMAP connection.
    pub fn sender(&self) -> mpsc::Sender<Envelope> {
        self.inbox_tx.clone()
    }

    /// Reference to the channel config.
    pub fn config(&self) -> &EmailConfig {
        &self.config
    }

    /// Start the IMAP poll listener in the background.
    ///
    /// Returns a `JoinHandle` that resolves when the listener shuts down.
    /// The listener respects the provided `CancellationToken`.
    pub async fn start_listener(
        &self,
        _shutdown: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>, EmailError> {
        // TODO(phase-3): Wire up IMAP polling.
        //   1. Connect to IMAP server using async-imap
        //   2. Poll INBOX at `config.poll_interval_secs` intervals
        //   3. For each new message:
        //      a. Parse MIME parts into EmailMessage
        //      b. Call normalize::normalize()
        //      c. Send the Envelope through self.inbox_tx
        //   4. select! with shutdown token
        let handle = tokio::spawn(async {
            tracing::info!("Email IMAP listener placeholder -- not yet wired to async-imap");
        });
        Ok(handle)
    }

    /// Email-specific capabilities.
    pub fn email_capabilities() -> ChannelCapabilities {
        ChannelCapabilities {
            supports_streaming: false,      // Email is store-and-forward
            supports_edit: false,            // Cannot edit sent emails
            supports_markdown: false,        // Plain text emails are safest
            max_message_len: 0,              // Unlimited (no practical limit)
            supports_files: true,            // MIME attachments
            max_file_size: 25 * 1024 * 1024, // 25 MB SMTP limit
        }
    }
}

// ---------------------------------------------------------------------------
// Channel trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl mcclawd_channels::Channel for EmailChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Email
    }

    async fn start(
        &self,
        _inbound_tx: mpsc::Sender<InboundMessage>,
        _shutdown: CancellationToken,
    ) -> mcclawd_core::Result<()> {
        // Phase 0 start -- delegate to start_listener in real usage.
        tracing::info!("EmailChannel::start (Phase 0 stub)");
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
        Self::email_capabilities()
    }

    fn platform(&self) -> Platform {
        Platform::Email
    }

    async fn save_state(&self) -> anyhow::Result<Option<Vec<u8>>> {
        let state = EmailState {
            uid_validity: self.uid_validity.load(Ordering::Relaxed),
            last_uid: self.last_uid.load(Ordering::Relaxed),
        };
        let bytes = serde_json::to_vec(&state)?;
        Ok(Some(bytes))
    }

    async fn restore_state(&self, state: Option<Vec<u8>>) -> anyhow::Result<()> {
        if let Some(data) = state {
            match serde_json::from_slice::<EmailState>(&data) {
                Ok(s) => {
                    self.uid_validity.store(s.uid_validity, Ordering::Relaxed);
                    self.last_uid.store(s.last_uid, Ordering::Relaxed);
                    tracing::info!(
                        uid_validity = s.uid_validity,
                        last_uid = s.last_uid,
                        "Email state restored"
                    );
                }
                Err(e) => {
                    tracing::warn!("Corrupt Email state, starting fresh: {e}");
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

    fn test_envelope(text: &str) -> Envelope {
        Envelope {
            id: uuid::Uuid::new_v4().to_string(),
            peer: Peer {
                id: "alice@example.com".into(),
                display_name: Some("Alice".into()),
                platform: Platform::Email,
            },
            thread: None,
            content: MessageContent::Text(text.into()),
            attachments: vec![],
            timestamp: Utc::now(),
            platform_meta: serde_json::json!({
                "message_id": "<test@example.com>",
                "subject": "Test Subject",
            }),
        }
    }

    #[tokio::test]
    async fn receive_injected_message() {
        let mut channel = EmailChannel::new(test_config());
        let sender = channel.sender();

        sender.send(test_envelope("Hello via email")).await.unwrap();
        let msg = channel.recv_envelope().await.unwrap().unwrap();

        assert!(matches!(&msg.content, MessageContent::Text(t) if t == "Hello via email"));
        assert_eq!(msg.peer.id, "alice@example.com");
    }

    #[tokio::test]
    async fn receive_multiple_messages_in_order() {
        let mut channel = EmailChannel::new(test_config());
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
        let mut channel = EmailChannel::new(test_config());
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
        let mut channel = EmailChannel::new(test_config());
        let mut outbound_rx = channel.outbound_rx.take().unwrap();

        channel.send_chunk(OutboundChunk::Done).await.unwrap();

        let chunk = outbound_rx.recv().await.unwrap();
        assert!(matches!(chunk, OutboundChunk::Done));
    }

    #[test]
    fn capabilities_are_correct() {
        let caps = EmailChannel::email_capabilities();
        assert!(!caps.supports_edit);
        assert!(!caps.supports_markdown);
        assert!(!caps.supports_streaming);
        assert!(caps.supports_files);
        assert_eq!(caps.max_message_len, 0); // unlimited
        assert_eq!(caps.max_file_size, 25 * 1024 * 1024); // 25 MB
    }

    #[test]
    fn kind_is_email() {
        let channel = EmailChannel::new(test_config());
        assert_eq!(channel.kind(), ChannelKind::Email);
    }

    #[test]
    fn platform_is_email() {
        let channel = EmailChannel::new(test_config());
        assert_eq!(channel.platform(), Platform::Email);
    }

    #[test]
    fn config_creation() {
        let config = EmailConfig {
            imap_host: "imap.gmail.com".into(),
            imap_port: 993,
            smtp_host: "smtp.gmail.com".into(),
            smtp_port: 587,
            username: "user@gmail.com".into(),
            password: "app-password".into(),
            from_address: "user@gmail.com".into(),
            allowed_senders: Some(vec!["boss@company.com".into()]),
            poll_interval_secs: 60,
        };
        assert_eq!(config.imap_host, "imap.gmail.com");
        assert_eq!(config.imap_port, 993);
        assert_eq!(config.smtp_host, "smtp.gmail.com");
        assert_eq!(config.smtp_port, 587);
        assert_eq!(config.allowed_senders.unwrap().len(), 1);
        assert_eq!(config.poll_interval_secs, 60);
    }

    #[test]
    fn config_no_allowlist() {
        let config = EmailConfig {
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            username: "user".into(),
            password: "pass".into(),
            from_address: "user@example.com".into(),
            allowed_senders: None,
            poll_interval_secs: 30,
        };
        assert!(config.allowed_senders.is_none());
    }

    #[test]
    fn sender_clone_works() {
        let channel = EmailChannel::new(test_config());
        let s1 = channel.sender();
        let s2 = channel.sender();
        // Both senders are functional (not closed).
        assert!(!s1.is_closed());
        assert!(!s2.is_closed());
    }

    #[tokio::test]
    async fn save_restore_state_roundtrip() {
        let channel = EmailChannel::new(test_config());
        channel.uid_validity.store(12345, Ordering::Relaxed);
        channel.last_uid.store(678, Ordering::Relaxed);

        let saved = channel.save_state().await.unwrap().unwrap();
        let state: EmailState = serde_json::from_slice(&saved).unwrap();
        assert_eq!(state.uid_validity, 12345);
        assert_eq!(state.last_uid, 678);

        let channel2 = EmailChannel::new(test_config());
        channel2.restore_state(Some(saved)).await.unwrap();
        assert_eq!(channel2.uid_validity.load(Ordering::Relaxed), 12345);
        assert_eq!(channel2.last_uid.load(Ordering::Relaxed), 678);
    }

    #[tokio::test]
    async fn restore_none_state_is_ok() {
        let channel = EmailChannel::new(test_config());
        channel.restore_state(None).await.unwrap();
        assert_eq!(channel.uid_validity.load(Ordering::Relaxed), 0);
        assert_eq!(channel.last_uid.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn restore_corrupt_state_is_ok() {
        let channel = EmailChannel::new(test_config());
        channel.restore_state(Some(b"xxx".to_vec())).await.unwrap();
        assert_eq!(channel.uid_validity.load(Ordering::Relaxed), 0);
    }
}
