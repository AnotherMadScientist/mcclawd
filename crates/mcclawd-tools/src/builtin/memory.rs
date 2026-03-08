//! Builtin memory tools — session-scoped key-value store.
//!
//! `memory_store` — persist a key-value pair in working memory.
//! `memory_recall` — retrieve a value by key from working memory.
//!
//! Both tools share the same `Arc<DashMap>` so the agent can store and recall
//! within a single session. The backing store is ephemeral — it lives only as
//! long as the agent process.

use dashmap::DashMap;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

// ----------------------------------------------------------------
// Error
// ----------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
#[error("Memory error: {0}")]
pub struct MemoryError(String);

// ----------------------------------------------------------------
// Shared backing store
// ----------------------------------------------------------------

/// In-memory key-value store shared between `MemoryStore` and `MemoryRecall`.
pub type MemoryBackend = Arc<DashMap<String, String>>;

// ----------------------------------------------------------------
// memory_store
// ----------------------------------------------------------------

#[derive(Deserialize, Serialize)]
pub struct StoreArgs {
    pub key: String,
    pub value: String,
}

/// Tool that writes a key-value pair into session memory.
#[derive(Serialize, Deserialize, Clone)]
pub struct MemoryStore {
    #[serde(skip)]
    pub store: MemoryBackend,
}

impl MemoryStore {
    /// Create a new `MemoryStore` with its own backing `DashMap`.
    pub fn new_shared() -> Self {
        Self {
            store: Arc::new(DashMap::new()),
        }
    }

    /// Create a `MemoryStore` wrapping an existing backend.
    pub fn with_backend(store: MemoryBackend) -> Self {
        Self { store }
    }
}

impl Tool for MemoryStore {
    const NAME: &'static str = "memory_store";
    type Error = MemoryError;
    type Args = StoreArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "memory_store".to_string(),
            description: "Store a key-value pair in working memory for the current session."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "key":   { "type": "string", "description": "The key to store" },
                    "value": { "type": "string", "description": "The value to store" }
                },
                "required": ["key", "value"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.store.insert(args.key.clone(), args.value);
        Ok(format!("Stored key '{}'", args.key))
    }
}

// ----------------------------------------------------------------
// memory_recall
// ----------------------------------------------------------------

#[derive(Deserialize, Serialize)]
pub struct RecallArgs {
    pub key: String,
}

/// Tool that reads a value from session memory by key.
#[derive(Serialize, Deserialize, Clone)]
pub struct MemoryRecall {
    #[serde(skip)]
    pub store: MemoryBackend,
}

impl MemoryRecall {
    /// Create a `MemoryRecall` sharing the same backend as `memory_store`.
    pub fn from_shared(memory_store: &MemoryStore) -> Self {
        Self {
            store: memory_store.store.clone(),
        }
    }

    /// Create a `MemoryRecall` wrapping an existing backend.
    pub fn with_backend(store: MemoryBackend) -> Self {
        Self { store }
    }
}

impl Tool for MemoryRecall {
    const NAME: &'static str = "memory_recall";
    type Error = MemoryError;
    type Args = RecallArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "memory_recall".to_string(),
            description: "Recall a value from working memory by key.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "The key to recall" }
                },
                "required": ["key"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        match self.store.get(&args.key) {
            Some(value) => Ok(value.value().clone()),
            None => Ok(format!("No value found for key '{}'", args.key)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn store_and_recall_roundtrip() {
        let store = MemoryStore::new_shared();
        let recall = MemoryRecall::from_shared(&store);

        let res = store
            .call(StoreArgs {
                key: "color".into(),
                value: "blue".into(),
            })
            .await
            .unwrap();
        assert!(res.contains("Stored"));

        let res = recall.call(RecallArgs { key: "color".into() }).await.unwrap();
        assert_eq!(res, "blue");
    }

    #[tokio::test]
    async fn recall_missing_key() {
        let store = MemoryStore::new_shared();
        let recall = MemoryRecall::from_shared(&store);

        let res = recall
            .call(RecallArgs {
                key: "nope".into(),
            })
            .await
            .unwrap();
        assert!(res.contains("No value found"));
    }
}
