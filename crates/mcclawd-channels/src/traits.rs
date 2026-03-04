use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::envelope::{Envelope, Platform};
use crate::registry::ChannelCapabilities;
use crate::types::*;

// ---------------------------------------------------------------------------
// ChannelStartContext
// ---------------------------------------------------------------------------

/// Everything a channel needs to start. Bundles deps so adding new ones
/// doesn't change the trait signature.
pub struct ChannelStartContext {
    /// Sender for inbound messages.
    pub inbound_tx: mpsc::Sender<InboundMessage>,
    /// Token to signal graceful shutdown.
    pub shutdown: CancellationToken,
}

// ---------------------------------------------------------------------------
// Channel trait
// ---------------------------------------------------------------------------

/// The core channel abstraction. Each communication platform implements this
/// trait to provide inbound message reception and outbound chunk delivery.
///
/// Phase 2 adds envelope-based methods (`recv_envelope`, `platform`,
/// `capabilities`) alongside the original Phase 0 methods for backward
/// compatibility. Channels should override the new methods as they migrate
/// to the normalized Envelope pipeline.
///
/// Phase 4 adds state persistence (`save_state`, `restore_state`) and
/// `start_with_context` for bundled dependency injection.
#[async_trait]
pub trait Channel: Send + Sync + 'static {
    // -----------------------------------------------------------------------
    // Phase 0 methods (kept for backward compatibility)
    // -----------------------------------------------------------------------

    /// The channel kind identifier (Phase 0).
    fn kind(&self) -> ChannelKind;

    /// Start the channel, sending inbound messages to `inbound_tx`.
    async fn start(
        &self,
        inbound_tx: mpsc::Sender<InboundMessage>,
        shutdown: CancellationToken,
    ) -> mcclawd_core::Result<()>;

    /// Send an outbound chunk to this channel.
    async fn send_chunk(&self, chunk: OutboundChunk) -> mcclawd_core::Result<()>;

    // -----------------------------------------------------------------------
    // Phase 2 methods (envelope-based, with defaults for backward compat)
    // -----------------------------------------------------------------------

    /// Receive the next inbound message as a normalized Envelope.
    ///
    /// Default implementation returns `Ok(None)` — channels override this
    /// once they migrate to the envelope pipeline.
    async fn recv_envelope(&mut self) -> anyhow::Result<Option<Envelope>> {
        Ok(None)
    }

    /// Get channel capabilities (what this channel supports).
    ///
    /// Default returns conservative capabilities. Override for accurate
    /// reporting so the outbound router can make formatting decisions.
    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities::default()
    }

    /// Channel platform identifier (Phase 2).
    ///
    /// Default maps from `kind()` for backward compatibility.
    fn platform(&self) -> Platform {
        match self.kind() {
            ChannelKind::Cli => Platform::Cli,
            ChannelKind::Web => Platform::Web,
            ChannelKind::Telegram => Platform::Telegram,
            ChannelKind::Discord => Platform::Discord,
            ChannelKind::Slack => Platform::Slack,
            ChannelKind::WhatsApp => Platform::WhatsApp,
            ChannelKind::Email => Platform::Email,
            ChannelKind::Custom(_) => Platform::Cli, // fallback
        }
    }

    // -----------------------------------------------------------------------
    // Phase 4 methods (state persistence + context-based start)
    // -----------------------------------------------------------------------

    /// Save channel connection state (e.g. Discord sequence, IMAP UID cursor).
    /// Returns `None` for stateless channels.
    async fn save_state(&self) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(None)
    }

    /// Restore channel connection state from a previous run.
    /// Called before `start()`. Corrupt/`None` state = fresh init.
    async fn restore_state(&self, _state: Option<Vec<u8>>) -> anyhow::Result<()> {
        Ok(())
    }

    /// Start the channel using a context bundle. Default delegates to legacy `start()`.
    async fn start_with_context(&self, ctx: ChannelStartContext) -> anyhow::Result<()> {
        self.start(ctx.inbound_tx, ctx.shutdown)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))
    }
}
