//! Telegram channel adapter errors.

use thiserror::Error;

/// Errors specific to the Telegram channel adapter.
#[derive(Debug, Error)]
pub enum TelegramError {
    /// An error from the Telegram Bot API.
    #[error("Telegram API error: {0}")]
    Api(String),

    /// Failed to convert a Telegram message to an Envelope.
    #[error("Message normalization failed: {0}")]
    Normalization(String),

    /// The bot is not connected to the Telegram API.
    #[error("Bot not connected")]
    NotConnected,

    /// Passthrough for other errors.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
