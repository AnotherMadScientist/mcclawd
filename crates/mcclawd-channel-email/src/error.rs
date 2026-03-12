//! Email channel adapter errors.

use thiserror::Error;

/// Errors specific to the Email channel adapter.
#[derive(Debug, Error)]
pub enum EmailError {
    /// An error from the IMAP connection.
    #[error("IMAP error: {0}")]
    Imap(String),

    /// An error from the SMTP connection.
    #[error("SMTP error: {0}")]
    Smtp(String),

    /// Failed to convert an email message to an Envelope.
    #[error("Message normalization failed: {0}")]
    Normalization(String),

    /// The email client is not connected.
    #[error("Email client not connected")]
    NotConnected,

    /// Authentication failed.
    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    /// Adapter not yet available (Phase 3).
    #[error("Not available: {0}")]
    NotAvailable(String),

    /// Passthrough for other errors.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
