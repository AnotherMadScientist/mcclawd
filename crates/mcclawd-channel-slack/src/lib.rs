//! Slack channel adapter for McClawd.
//!
//! This crate provides a [`SlackChannel`] that implements the
//! [`mcclawd_channels::Channel`] trait, normalizing Slack messages
//! into platform-agnostic [`Envelope`](mcclawd_channels::envelope::Envelope)s.
//!
//! # Architecture
//!
//! ```text
//! Slack Events API / Socket Mode
//!       |
//!   event handler (background task)
//!       |
//!   SlackMessage (intermediate type)
//!       |
//!   normalize() -> Envelope
//!       |
//!   mpsc::Sender<Envelope>  -->  SlackChannel.inbox_rx
//!       |
//!   Channel::recv_envelope()
//! ```

pub mod adapter;
pub mod error;
pub mod normalize;

pub use adapter::{SlackChannel, SlackConfig};
