//! Discord channel adapter errors.

use thiserror::Error;

/// Errors specific to the Discord channel adapter.
#[derive(Debug, Error)]
pub enum DiscordError {
    /// An error from the Discord API.
    #[error("Discord API error: {0}")]
    Api(String),

    /// Failed to convert a Discord message to an Envelope.
    #[error("Message normalization failed: {0}")]
    Normalization(String),

    /// The bot is not connected to the Discord gateway.
    #[error("Bot not connected")]
    NotConnected,

    /// Passthrough for other errors.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
