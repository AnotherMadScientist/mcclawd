//! WhatsApp channel adapter errors.

use thiserror::Error;

/// Errors specific to the WhatsApp channel adapter.
#[derive(Debug, Error)]
pub enum WhatsAppError {
    /// An error from the WhatsApp Cloud API.
    #[error("WhatsApp API error: {0}")]
    Api(String),

    /// Failed to convert a WhatsApp message to an Envelope.
    #[error("Message normalization failed: {0}")]
    Normalization(String),

    /// Webhook verification failed.
    #[error("Webhook verification failed: {0}")]
    WebhookVerification(String),

    /// The channel is not connected to the WhatsApp API.
    #[error("Channel not connected")]
    NotConnected,

    /// Passthrough for other errors.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
