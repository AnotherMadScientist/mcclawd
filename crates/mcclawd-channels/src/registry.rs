//! Phase 2 Channel Registry.
//!
//! The `ChannelRegistry` manages active channels: registration, lookup,
//! capability queries, and lifecycle. This module provides the core types;
//! the full async registry with mpsc routing is added when channel adapters land.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::envelope::Platform;

// ---------------------------------------------------------------------------
// ChannelId
// ---------------------------------------------------------------------------

/// Opaque identifier for a registered channel instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelId(pub String);

impl ChannelId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for ChannelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// ChannelCapabilities
// ---------------------------------------------------------------------------

/// Declares what a channel supports. Used by the outbound router to decide
/// formatting (e.g. skip markdown on SMS, split long messages).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelCapabilities {
    /// Whether the channel supports streaming / incremental output.
    pub supports_streaming: bool,
    /// Whether the channel supports editing previously sent messages.
    pub supports_edit: bool,
    /// Whether the channel supports markdown formatting.
    pub supports_markdown: bool,
    /// Maximum message length in characters (0 = unlimited).
    pub max_message_len: usize,
    /// Whether the channel supports file attachments.
    pub supports_files: bool,
    /// Maximum file size in bytes (0 = unlimited).
    pub max_file_size: u64,
}

impl Default for ChannelCapabilities {
    fn default() -> Self {
        Self {
            supports_streaming: false,
            supports_edit: false,
            supports_markdown: true,
            max_message_len: 0,
            supports_files: false,
            max_file_size: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// ChannelEntry
// ---------------------------------------------------------------------------

/// A registered channel in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelEntry {
    /// Unique identifier for this channel instance.
    pub id: ChannelId,
    /// Which platform this channel connects to.
    pub platform: Platform,
    /// What the channel supports.
    pub capabilities: ChannelCapabilities,
    /// Whether the channel is currently enabled.
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// ChannelRegistry
// ---------------------------------------------------------------------------

/// Central registry of all known channel instances.
///
/// Phase 2 foundation: stores static metadata. The full async registry
/// (with mpsc routing, health checks, and lifecycle) builds on top of this.
#[derive(Debug)]
pub struct ChannelRegistry {
    channels: HashMap<ChannelId, ChannelEntry>,
}

impl ChannelRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            channels: HashMap::new(),
        }
    }

    /// Register a channel. Returns `false` if the ID was already taken.
    pub fn register(&mut self, entry: ChannelEntry) -> bool {
        if self.channels.contains_key(&entry.id) {
            return false;
        }
        self.channels.insert(entry.id.clone(), entry);
        true
    }

    /// Remove a channel by ID. Returns the entry if it existed.
    pub fn unregister(&mut self, id: &ChannelId) -> Option<ChannelEntry> {
        self.channels.remove(id)
    }

    /// Look up a channel by ID.
    pub fn get(&self, id: &ChannelId) -> Option<&ChannelEntry> {
        self.channels.get(id)
    }

    /// List all registered channels.
    pub fn list(&self) -> Vec<&ChannelEntry> {
        self.channels.values().collect()
    }

    /// Number of registered channels.
    pub fn len(&self) -> usize {
        self.channels.len()
    }

    /// Whether the registry has no channels.
    pub fn is_empty(&self) -> bool {
        self.channels.is_empty()
    }
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(id: &str, platform: Platform) -> ChannelEntry {
        ChannelEntry {
            id: ChannelId::new(id),
            platform,
            capabilities: ChannelCapabilities::default(),
            enabled: true,
        }
    }

    #[test]
    fn register_and_get() {
        let mut reg = ChannelRegistry::new();
        let entry = make_entry("tg-main", Platform::Telegram);
        assert!(reg.register(entry));
        assert_eq!(reg.len(), 1);

        let found = reg.get(&ChannelId::new("tg-main"));
        assert!(found.is_some());
        assert_eq!(found.unwrap().platform, Platform::Telegram);
    }

    #[test]
    fn register_duplicate_returns_false() {
        let mut reg = ChannelRegistry::new();
        let entry1 = make_entry("ch-1", Platform::Cli);
        let entry2 = make_entry("ch-1", Platform::Web);
        assert!(reg.register(entry1));
        assert!(!reg.register(entry2));
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn unregister_removes_entry() {
        let mut reg = ChannelRegistry::new();
        reg.register(make_entry("ch-a", Platform::Discord));
        reg.register(make_entry("ch-b", Platform::Slack));
        assert_eq!(reg.len(), 2);

        let removed = reg.unregister(&ChannelId::new("ch-a"));
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().platform, Platform::Discord);
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn unregister_missing_returns_none() {
        let mut reg = ChannelRegistry::new();
        assert!(reg.unregister(&ChannelId::new("nonexistent")).is_none());
    }

    #[test]
    fn list_returns_all() {
        let mut reg = ChannelRegistry::new();
        reg.register(make_entry("a", Platform::Cli));
        reg.register(make_entry("b", Platform::Web));
        reg.register(make_entry("c", Platform::Email));
        assert_eq!(reg.list().len(), 3);
    }

    #[test]
    fn capabilities_defaults() {
        let caps = ChannelCapabilities::default();
        assert!(!caps.supports_streaming);
        assert!(!caps.supports_edit);
        assert!(caps.supports_markdown);
        assert_eq!(caps.max_message_len, 0);
        assert!(!caps.supports_files);
        assert_eq!(caps.max_file_size, 0);
    }

    #[test]
    fn capabilities_serde_roundtrip() {
        let caps = ChannelCapabilities {
            supports_streaming: true,
            supports_edit: true,
            supports_markdown: false,
            max_message_len: 4096,
            supports_files: true,
            max_file_size: 50 * 1024 * 1024,
        };
        let json = serde_json::to_string(&caps).expect("serialize");
        let back: ChannelCapabilities = serde_json::from_str(&json).expect("deserialize");
        assert!(back.supports_streaming);
        assert_eq!(back.max_message_len, 4096);
        assert_eq!(back.max_file_size, 50 * 1024 * 1024);
    }

    #[test]
    fn channel_id_display() {
        let id = ChannelId::new("telegram-prod");
        assert_eq!(id.to_string(), "telegram-prod");
    }

    #[test]
    fn empty_registry() {
        let reg = ChannelRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.list().is_empty());
    }
}
