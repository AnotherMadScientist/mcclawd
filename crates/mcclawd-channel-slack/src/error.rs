//! Slack channel adapter errors.

use thiserror::Error;

/// Errors specific to the Slack channel adapter.
#[derive(Debug, Error)]
pub enum SlackError {
    /// An error from the Slack API.
    #[error("Slack API error: {0}")]
    Api(String),

    /// Failed to convert a Slack message to an Envelope.
    #[error("Message normalization failed: {0}")]
    Normalization(String),

    /// The bot is not connected to the Slack API.
    #[error("Bot not connected")]
    NotConnected,

    /// Channel not in allowlist.
    #[error("Channel {0} not in allowed list")]
    ChannelNotAllowed(String),

    /// Adapter not yet available (Phase 3).
    #[error("Not available: {0}")]
    NotAvailable(String),

    /// Passthrough for other errors.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
