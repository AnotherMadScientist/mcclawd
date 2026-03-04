use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Thread-safe shared memory for inter-worker communication.
///
/// Workers write their outputs here (keyed by `SubtaskNode::output_key`)
/// and downstream workers read their inputs from it.
#[derive(Clone)]
pub struct SharedMemory {
    store: Arc<DashMap<String, serde_json::Value>>,
}

impl SharedMemory {
    /// Create an empty shared memory store.
    pub fn new() -> Self {
        Self {
            store: Arc::new(DashMap::new()),
        }
    }

    /// Serialize `value` to JSON and insert it under `key`.
    pub fn set<T: Serialize>(&self, key: &str, value: T) {
        let json = serde_json::to_value(value).expect("SharedMemory::set: serialization failed");
        self.store.insert(key.to_owned(), json);
    }

    /// Retrieve and deserialize the value stored under `key`.
    ///
    /// Returns `None` if the key is missing or deserialization fails.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.store
            .get(key)
            .and_then(|entry| serde_json::from_value(entry.value().clone()).ok())
    }

    /// Check whether a key exists.
    pub fn contains(&self, key: &str) -> bool {
        self.store.contains_key(key)
    }

    /// Return a sorted list of all keys.
    pub fn keys(&self) -> Vec<String> {
        self.store.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Clone all entries into a plain `HashMap`.
    pub fn snapshot(&self) -> HashMap<String, serde_json::Value> {
        self.store
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect()
    }
}

impl Default for SharedMemory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_get_string() {
        let mem = SharedMemory::new();
        mem.set("greeting", "hello");
        let val: Option<String> = mem.get("greeting");
        assert_eq!(val, Some("hello".to_string()));
    }

    #[test]
    fn set_and_get_struct() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct Result {
            score: f64,
        }
        let mem = SharedMemory::new();
        mem.set("result", Result { score: 0.95 });
        let val: Option<Result> = mem.get("result");
        assert_eq!(val, Some(Result { score: 0.95 }));
    }

    #[test]
    fn get_missing_returns_none() {
        let mem = SharedMemory::new();
        let val: Option<String> = mem.get("nope");
        assert!(val.is_none());
    }

    #[test]
    fn contains_key() {
        let mem = SharedMemory::new();
        mem.set("x", 42i64);
        assert!(mem.contains("x"));
        assert!(!mem.contains("y"));
    }

    #[test]
    fn keys_list() {
        let mem = SharedMemory::new();
        mem.set("a", 1i64);
        mem.set("b", 2i64);
        let mut keys = mem.keys();
        keys.sort();
        assert_eq!(keys, vec!["a", "b"]);
    }

    #[test]
    fn snapshot_clones_all() {
        let mem = SharedMemory::new();
        mem.set("x", "hello");
        mem.set("y", 42i64);
        let snap = mem.snapshot();
        assert_eq!(snap.len(), 2);
    }
}
