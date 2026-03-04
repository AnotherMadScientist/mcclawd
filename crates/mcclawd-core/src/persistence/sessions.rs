use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A conversation session tied to a channel + peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub channel_id: String,
    pub peer_id: String,
    pub platform: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub metadata: serde_json::Value,
}

/// Role of a single turn within a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TurnRole {
    User,
    Assistant,
    System,
    Tool,
}

/// A single conversational turn (message) in a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    pub id: String,
    pub session_id: String,
    pub role: TurnRole,
    pub content: String,
    pub tool_calls: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

/// Trait for session + turn persistence backends.
///
/// Phase 0: `InMemorySessionStore` (dev/testing).
/// Future: Postgres-backed implementation via sqlx.
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Create a new session for the given channel/peer/platform.
    async fn create_session(
        &self,
        channel_id: &str,
        peer_id: &str,
        platform: &str,
    ) -> crate::Result<Session>;

    /// Mark a session as ended (sets `ended_at`).
    async fn end_session(&self, session_id: &str) -> crate::Result<()>;

    /// Look up a session by ID.
    async fn get_session(&self, session_id: &str) -> crate::Result<Option<Session>>;

    /// Append a turn to a session.
    async fn add_turn(
        &self,
        session_id: &str,
        role: TurnRole,
        content: &str,
        tool_calls: Option<serde_json::Value>,
    ) -> crate::Result<Turn>;

    /// Get all turns for a session, ordered by `created_at` ascending.
    async fn get_turns(&self, session_id: &str) -> crate::Result<Vec<Turn>>;

    /// Get the most recent sessions for a peer, ordered by `started_at` descending.
    async fn get_recent_sessions(
        &self,
        peer_id: &str,
        limit: usize,
    ) -> crate::Result<Vec<Session>>;
}
