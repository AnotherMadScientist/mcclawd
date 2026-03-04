use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    pub id: String,
    pub channel: ChannelKind,
    pub peer: Peer,
    pub content: MessageContent,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub id: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageContent {
    Text(String),
    Command { name: String, args: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutboundChunk {
    TextDelta(String),
    TextBlock(String),
    ToolStart { name: String },
    ToolEnd { name: String, summary: Option<String> },
    Done,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChannelKind {
    Cli,
    Web,
    Telegram,
    Discord,
    Custom(String),
}

impl std::fmt::Display for ChannelKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChannelKind::Cli => write!(f, "cli"),
            ChannelKind::Web => write!(f, "web"),
            ChannelKind::Telegram => write!(f, "telegram"),
            ChannelKind::Discord => write!(f, "discord"),
            ChannelKind::Custom(name) => write!(f, "{}", name),
        }
    }
}
