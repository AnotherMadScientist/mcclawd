//! Hot-reload config via file watcher.
//!
//! Watches a config file for changes and publishes validated updates
//! through a `tokio::sync::watch` channel. Invalid config is logged
//! and skipped (the previous valid config is retained).

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::watch;

use crate::config::McclawdConfig;

/// Watches a configuration file and publishes validated updates.
pub struct ConfigWatcher {
    config_path: PathBuf,
    tx: watch::Sender<McclawdConfig>,
}

impl ConfigWatcher {
    /// Create a new config watcher.
    ///
    /// Loads the initial config from disk and returns `(watcher, receiver)`.
    /// The receiver can be cloned and shared across tasks to get the latest config.
    pub fn new(config_path: PathBuf) -> anyhow::Result<(Self, watch::Receiver<McclawdConfig>)> {
        let initial = McclawdConfig::load(&config_path)?;
        let (tx, rx) = watch::channel(initial);
        Ok((Self { config_path, tx }, rx))
    }

    /// Start watching for config changes.
    ///
    /// Debounces at 500ms: after a change is detected, waits 500ms for
    /// additional changes before reloading. Validates new config before
    /// publishing. Bad config = tracing::warn + keep old.
    ///
    /// Runs until the `shutdown` token is cancelled.
    pub async fn watch(&self, shutdown: tokio::sync::mpsc::Receiver<()>) -> anyhow::Result<()> {
        // Bridge from notify's sync callback to async via mpsc.
        let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel::<Event>(64);

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = notify_tx.blocking_send(event);
                }
            },
            notify::Config::default(),
        )?;

        // Watch the parent directory (some editors replace the file atomically).
        let watch_path = self
            .config_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        watcher.watch(watch_path, RecursiveMode::NonRecursive)?;

        let mut shutdown = shutdown;

        loop {
            tokio::select! {
                Some(event) = notify_rx.recv() => {
                    // Only react to modify/create events on our config file.
                    if !self.is_relevant_event(&event) {
                        continue;
                    }

                    // Debounce: wait 500ms for more changes.
                    tokio::time::sleep(Duration::from_millis(500)).await;

                    // Drain any queued events during debounce.
                    while notify_rx.try_recv().is_ok() {}

                    // Reload and validate.
                    self.reload_config();
                }
                _ = shutdown.recv() => {
                    tracing::info!("Config watcher shutting down");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Reload config from disk. On success, publish to watchers.
    /// On failure, log a warning and keep the old config.
    fn reload_config(&self) {
        match McclawdConfig::load(&self.config_path) {
            Ok(new_config) => {
                tracing::info!("Config reloaded from {}", self.config_path.display());
                if self.tx.send(new_config).is_err() {
                    tracing::warn!("No config receivers — config update dropped");
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to reload config from {}: {} — keeping previous config",
                    self.config_path.display(),
                    e
                );
            }
        }
    }

    /// Check if a notify event is relevant to our config file.
    fn is_relevant_event(&self, event: &Event) -> bool {
        matches!(
            event.kind,
            EventKind::Modify(_) | EventKind::Create(_)
        ) && event
            .paths
            .iter()
            .any(|p| p == &self.config_path)
    }

    /// Manually trigger a config reload (useful for API-driven reloads).
    pub fn trigger_reload(&self) -> anyhow::Result<()> {
        let new_config = McclawdConfig::load(&self.config_path)?;
        self.tx
            .send(new_config)
            .map_err(|_| anyhow::anyhow!("No config receivers"))?;
        Ok(())
    }

    /// Get the current config path.
    pub fn config_path(&self) -> &PathBuf {
        &self.config_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_valid_config(path: &std::path::Path) {
        let mut f = std::fs::File::create(path).unwrap();
        writeln!(
            f,
            r#"
[agent]
max_turns = 10
model = "claude-sonnet-4-5"
default_workspace = "test"
"#
        )
        .unwrap();
    }

    fn write_updated_config(path: &std::path::Path) {
        let mut f = std::fs::File::create(path).unwrap();
        writeln!(
            f,
            r#"
[agent]
max_turns = 42
model = "gpt-4"
default_workspace = "updated"
"#
        )
        .unwrap();
    }

    fn write_invalid_config(path: &std::path::Path) {
        let mut f = std::fs::File::create(path).unwrap();
        writeln!(f, "this is {{ not valid toml").unwrap();
    }

    #[test]
    fn new_creates_with_initial_config() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        write_valid_config(&config_path);

        let (watcher, rx) = ConfigWatcher::new(config_path.clone()).unwrap();
        let config = rx.borrow();
        assert_eq!(config.agent.max_turns, 10);
        assert_eq!(config.agent.model, "claude-sonnet-4-5");
        assert_eq!(watcher.config_path(), &config_path);
    }

    #[test]
    fn new_with_missing_file_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("nonexistent.toml");

        let (_watcher, rx) = ConfigWatcher::new(config_path).unwrap();
        let config = rx.borrow();
        // Should use default values.
        assert_eq!(config.agent.max_turns, 20);
        assert_eq!(config.agent.model, "claude-sonnet-4-5");
    }

    #[test]
    fn trigger_reload_updates_config() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        write_valid_config(&config_path);

        let (watcher, rx) = ConfigWatcher::new(config_path.clone()).unwrap();
        assert_eq!(rx.borrow().agent.max_turns, 10);

        // Update the file and trigger reload.
        write_updated_config(&config_path);
        watcher.trigger_reload().unwrap();

        assert_eq!(rx.borrow().agent.max_turns, 42);
        assert_eq!(rx.borrow().agent.model, "gpt-4");
    }

    #[test]
    fn trigger_reload_invalid_config_errors() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        write_valid_config(&config_path);

        let (watcher, rx) = ConfigWatcher::new(config_path.clone()).unwrap();
        assert_eq!(rx.borrow().agent.max_turns, 10);

        // Write invalid config.
        write_invalid_config(&config_path);
        let result = watcher.trigger_reload();
        assert!(result.is_err());

        // Config should remain unchanged.
        assert_eq!(rx.borrow().agent.max_turns, 10);
    }

    #[test]
    fn reload_config_invalid_keeps_old() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        write_valid_config(&config_path);

        let (watcher, rx) = ConfigWatcher::new(config_path.clone()).unwrap();
        assert_eq!(rx.borrow().agent.max_turns, 10);

        // Write invalid config and call internal reload.
        write_invalid_config(&config_path);
        watcher.reload_config();

        // Should still have old config.
        assert_eq!(rx.borrow().agent.max_turns, 10);
    }

    #[test]
    fn reload_config_valid_publishes_new() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        write_valid_config(&config_path);

        let (watcher, rx) = ConfigWatcher::new(config_path.clone()).unwrap();
        assert_eq!(rx.borrow().agent.max_turns, 10);

        write_updated_config(&config_path);
        watcher.reload_config();

        assert_eq!(rx.borrow().agent.max_turns, 42);
    }

    #[test]
    fn is_relevant_event_modify() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        write_valid_config(&config_path);

        let (watcher, _rx) = ConfigWatcher::new(config_path.clone()).unwrap();

        let event = Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Content,
            )),
            paths: vec![config_path.clone()],
            attrs: Default::default(),
        };
        assert!(watcher.is_relevant_event(&event));
    }

    #[test]
    fn is_relevant_event_wrong_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        write_valid_config(&config_path);

        let (watcher, _rx) = ConfigWatcher::new(config_path).unwrap();

        let event = Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Content,
            )),
            paths: vec![dir.path().join("other.toml")],
            attrs: Default::default(),
        };
        assert!(!watcher.is_relevant_event(&event));
    }

    #[test]
    fn is_relevant_event_remove_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        write_valid_config(&config_path);

        let (watcher, _rx) = ConfigWatcher::new(config_path.clone()).unwrap();

        let event = Event {
            kind: EventKind::Remove(notify::event::RemoveKind::File),
            paths: vec![config_path],
            attrs: Default::default(),
        };
        assert!(!watcher.is_relevant_event(&event));
    }
}
