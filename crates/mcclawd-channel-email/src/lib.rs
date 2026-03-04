//! Email channel adapter for McClawd.
//!
//! This crate provides an [`EmailChannel`] that implements the
//! [`mcclawd_channels::Channel`] trait, normalizing email messages
//! into platform-agnostic [`Envelope`](mcclawd_channels::envelope::Envelope)s.
//!
//! # Architecture
//!
//! ```text
//! IMAP Server (inbound)
//!       |
//!   IMAP poll loop (background task)
//!       |
//!   EmailMessage (intermediate type)
//!       |
//!   normalize() -> Envelope
//!       |
//!   mpsc::Sender<Envelope>  -->  EmailChannel.inbox_rx
//!       |
//!   Channel::recv_envelope()
//!
//! SMTP Server (outbound)
//!       ^
//!   EmailChannel.send_chunk()
//!       ^
//!   OutboundChunk from agent pipeline
//! ```

pub mod adapter;
pub mod error;
pub mod normalize;

pub use adapter::{EmailChannel, EmailConfig};
