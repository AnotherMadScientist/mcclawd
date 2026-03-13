use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Persisted agent configuration (SOUL.md, AGENTS.md, USER.md, model settings).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub name: String,
    pub soul_md: Option<String>,
    pub agents_md: Option<String>,
    pub user_md: Option<String>,
    pub model_config: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Trait for agent configuration persistence backends.
///
/// Production: `PgAgentConfigStore` in `mcclawd-api::server::pg_store`.
/// Testing: `InMemoryAgentConfigStore` in `persistence::memory` (cfg(test) only).
#[async_trait]
pub trait AgentConfigStore: Send + Sync {
    /// Save (upsert) an agent configuration.
    async fn save_config(&self, config: &AgentConfig) -> crate::Result<()>;

    /// Look up a config by name.
    async fn get_config(&self, name: &str) -> crate::Result<Option<AgentConfig>>;

    /// List all stored agent configurations.
    async fn list_configs(&self) -> crate::Result<Vec<AgentConfig>>;

    /// Delete a config by name.
    async fn delete_config(&self, name: &str) -> crate::Result<()>;
}
