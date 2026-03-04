//! Telegram channel adapter for McClawd.
//!
//! This crate provides a [`TelegramChannel`] that implements the
//! [`mcclawd_channels::Channel`] trait, normalizing Telegram messages
//! into platform-agnostic [`Envelope`](mcclawd_channels::envelope::Envelope)s.
//!
//! # Architecture
//!
//! ```text
//! Telegram Bot API
//!       |
//!   teloxide dispatcher (background task)
//!       |
//!   TelegramMessage (intermediate type)
//!       |
//!   normalize() -> Envelope
//!       |
//!   mpsc::Sender<Envelope>  ──>  TelegramChannel.inbox_rx
//!       |
//!   Channel::recv_envelope()
//! ```

pub mod adapter;
pub mod error;
pub mod normalize;
pub mod teloxide_handler;

pub use adapter::{TelegramChannel, TelegramConfig};
pub use teloxide_handler::{format_outbound, is_chat_allowed, parse_telegram_update};
