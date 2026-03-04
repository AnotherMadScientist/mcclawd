//! Discord channel adapter for McClawd.
//!
//! This crate provides a [`DiscordChannel`] that implements the
//! [`mcclawd_channels::Channel`] trait, normalizing Discord messages
//! into platform-agnostic [`Envelope`](mcclawd_channels::envelope::Envelope)s.
//!
//! # Architecture
//!
//! ```text
//! Discord Gateway API
//!       |
//!   serenity event handler (background task)
//!       |
//!   DiscordMessage (intermediate type)
//!       |
//!   normalize() -> Envelope
//!       |
//!   mpsc::Sender<Envelope>  ──>  DiscordChannel.inbox_rx
//!       |
//!   Channel::recv_envelope()
//! ```

pub mod adapter;
pub mod error;
pub mod normalize;

pub use adapter::{DiscordChannel, DiscordConfig};
