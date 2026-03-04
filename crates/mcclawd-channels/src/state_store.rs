//! Channel state persistence layer.
//!
//! Provides the [`ChannelStateStore`] trait and two implementations:
//! - [`InMemoryStateStore`] — for testing.
//! - [`FileStateStore`] — file-backed, using atomic rename for durability.

use async_trait::async_trait;
use dashmap::DashMap;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Persistent store for channel connection state blobs.
///
/// Each channel kind (e.g. "telegram", "discord") maps to an opaque `Vec<u8>`
/// that the channel adapter serializes/deserializes itself.
#[async_trait]
pub trait ChannelStateStore: Send + Sync {
    /// Persist state for a channel kind.
    async fn save(&self, channel_kind: &str, state: Vec<u8>) -> anyhow::Result<()>;

    /// Load previously persisted state. Returns `None` if no state exists.
    async fn load(&self, channel_kind: &str) -> anyhow::Result<Option<Vec<u8>>>;

    /// Delete persisted state for a channel kind.
    async fn delete(&self, channel_kind: &str) -> anyhow::Result<()>;

    /// List channel kinds that have persisted state.
    async fn list(&self) -> anyhow::Result<Vec<String>>;
}

// ---------------------------------------------------------------------------
// InMemoryStateStore
// ---------------------------------------------------------------------------

/// In-memory implementation for testing. State does not survive process restart.
pub struct InMemoryStateStore {
    states: DashMap<String, Vec<u8>>,
}

impl InMemoryStateStore {
    /// Create a new empty in-memory store.
    pub fn new() -> Self {
        Self {
            states: DashMap::new(),
        }
    }
}

impl Default for InMemoryStateStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChannelStateStore for InMemoryStateStore {
    async fn save(&self, channel_kind: &str, state: Vec<u8>) -> anyhow::Result<()> {
        self.states.insert(channel_kind.to_string(), state);
        Ok(())
    }

    async fn load(&self, channel_kind: &str) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self.states.get(channel_kind).map(|v| v.value().clone()))
    }

    async fn delete(&self, channel_kind: &str) -> anyhow::Result<()> {
        self.states.remove(channel_kind);
        Ok(())
    }

    async fn list(&self) -> anyhow::Result<Vec<String>> {
        Ok(self.states.iter().map(|e| e.key().clone()).collect())
    }
}

// ---------------------------------------------------------------------------
// FileStateStore
// ---------------------------------------------------------------------------

/// File-backed state store. Stores state at `{base_dir}/{channel_kind}.state`.
///
/// Uses write-to-temp-then-rename for atomic updates.
pub struct FileStateStore {
    base_dir: PathBuf,
}

impl FileStateStore {
    /// Create a new file-backed store rooted at `base_dir`.
    /// The directory is created on first `save()` if it doesn't exist.
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Build the path for a given channel kind.
    fn state_path(&self, channel_kind: &str) -> PathBuf {
        self.base_dir.join(format!("{channel_kind}.state"))
    }
}

#[async_trait]
impl ChannelStateStore for FileStateStore {
    async fn save(&self, channel_kind: &str, state: Vec<u8>) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(&self.base_dir).await?;
        let target = self.state_path(channel_kind);
        let tmp = self.base_dir.join(format!(".{channel_kind}.state.tmp"));
        tokio::fs::write(&tmp, &state).await?;
        tokio::fs::rename(&tmp, &target).await?;
        Ok(())
    }

    async fn load(&self, channel_kind: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let path = self.state_path(channel_kind);
        match tokio::fs::read(&path).await {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn delete(&self, channel_kind: &str) -> anyhow::Result<()> {
        let path = self.state_path(channel_kind);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()), // idempotent
            Err(e) => Err(e.into()),
        }
    }

    async fn list(&self) -> anyhow::Result<Vec<String>> {
        let mut kinds = Vec::new();
        let mut entries = match tokio::fs::read_dir(&self.base_dir).await {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(kinds),
            Err(e) => return Err(e.into()),
        };
        while let Some(entry) = entries.next_entry().await? {
            if let Some(name) = entry.file_name().to_str() {
                if let Some(kind) = name.strip_suffix(".state") {
                    // Skip temp files
                    if !kind.starts_with('.') {
                        kinds.push(kind.to_string());
                    }
                }
            }
        }
        kinds.sort();
        Ok(kinds)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_save_and_load() {
        let store = InMemoryStateStore::new();
        store.save("telegram", b"hello".to_vec()).await.unwrap();
        let loaded = store.load("telegram").await.unwrap();
        assert_eq!(loaded, Some(b"hello".to_vec()));
    }

    #[tokio::test]
    async fn in_memory_load_missing() {
        let store = InMemoryStateStore::new();
        let loaded = store.load("nonexistent").await.unwrap();
        assert!(loaded.is_none());
    }

    #[tokio::test]
    async fn in_memory_delete() {
        let store = InMemoryStateStore::new();
        store.save("discord", b"state".to_vec()).await.unwrap();
        store.delete("discord").await.unwrap();
        assert!(store.load("discord").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn in_memory_delete_missing_is_ok() {
        let store = InMemoryStateStore::new();
        // Should not error
        store.delete("nonexistent").await.unwrap();
    }

    #[tokio::test]
    async fn in_memory_list() {
        let store = InMemoryStateStore::new();
        store.save("telegram", b"t".to_vec()).await.unwrap();
        store.save("discord", b"d".to_vec()).await.unwrap();
        let mut kinds = store.list().await.unwrap();
        kinds.sort();
        assert_eq!(kinds, vec!["discord", "telegram"]);
    }

    #[tokio::test]
    async fn in_memory_overwrite() {
        let store = InMemoryStateStore::new();
        store.save("slack", b"v1".to_vec()).await.unwrap();
        store.save("slack", b"v2".to_vec()).await.unwrap();
        assert_eq!(store.load("slack").await.unwrap(), Some(b"v2".to_vec()));
    }

    #[tokio::test]
    async fn file_store_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStateStore::new(dir.path().to_path_buf());
        store.save("telegram", b"data".to_vec()).await.unwrap();
        let loaded = store.load("telegram").await.unwrap();
        assert_eq!(loaded, Some(b"data".to_vec()));
    }

    #[tokio::test]
    async fn file_store_load_missing() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStateStore::new(dir.path().to_path_buf());
        assert!(store.load("nonexistent").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn file_store_delete() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStateStore::new(dir.path().to_path_buf());
        store.save("email", b"state".to_vec()).await.unwrap();
        store.delete("email").await.unwrap();
        assert!(store.load("email").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn file_store_delete_missing_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStateStore::new(dir.path().to_path_buf());
        store.delete("nonexistent").await.unwrap();
    }

    #[tokio::test]
    async fn file_store_list() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStateStore::new(dir.path().to_path_buf());
        store.save("slack", b"s".to_vec()).await.unwrap();
        store.save("whatsapp", b"w".to_vec()).await.unwrap();
        let kinds = store.list().await.unwrap();
        assert_eq!(kinds, vec!["slack", "whatsapp"]);
    }

    #[tokio::test]
    async fn file_store_list_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStateStore::new(dir.path().join("nonexistent"));
        let kinds = store.list().await.unwrap();
        assert!(kinds.is_empty());
    }

    #[tokio::test]
    async fn file_store_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStateStore::new(dir.path().to_path_buf());
        store.save("discord", b"v1".to_vec()).await.unwrap();
        store.save("discord", b"v2".to_vec()).await.unwrap();
        assert_eq!(
            store.load("discord").await.unwrap(),
            Some(b"v2".to_vec())
        );
    }

    #[tokio::test]
    async fn file_store_creates_base_dir() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("deep").join("nested");
        let store = FileStateStore::new(nested.clone());
        store.save("telegram", b"data".to_vec()).await.unwrap();
        assert!(nested.exists());
        assert_eq!(
            store.load("telegram").await.unwrap(),
            Some(b"data".to_vec())
        );
    }
}
