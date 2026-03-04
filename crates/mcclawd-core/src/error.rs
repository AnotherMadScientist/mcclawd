use thiserror::Error;

#[derive(Error, Debug)]
pub enum McclawdError {
    #[error("Config error: {0}")]
    Config(String),

    #[error("Secret error: {0}")]
    Secret(String),

    #[error("Identity error: {0}")]
    Identity(String),

    #[error("Agent error: {0}")]
    Agent(String),

    #[error("Tool error: {0}")]
    Tool(String),

    #[error("Channel error: {0}")]
    Channel(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, McclawdError>;
