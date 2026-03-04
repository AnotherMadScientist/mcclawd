//! WhatsApp channel adapter for McClawd.
//!
//! This crate provides a [`WhatsAppChannel`] that implements the
//! [`mcclawd_channels::Channel`] trait, normalizing WhatsApp Cloud API
//! messages into platform-agnostic [`Envelope`](mcclawd_channels::envelope::Envelope)s.
//!
//! # Architecture
//!
//! ```text
//! WhatsApp Cloud API (webhook)
//!       |
//!   webhook handler (background task)
//!       |
//!   WhatsAppMessage (intermediate type)
//!       |
//!   normalize() -> Envelope
//!       |
//!   mpsc::Sender<Envelope>  -->  WhatsAppChannel.inbox_rx
//!       |
//!   Channel::recv_envelope()
//! ```

pub mod adapter;
pub mod error;
pub mod normalize;

pub use adapter::{WhatsAppChannel, WhatsAppConfig};
