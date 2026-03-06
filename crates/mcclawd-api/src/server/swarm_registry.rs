//! Swarm registry — tracks active and completed swarm runs.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

// ---------------------------------------------------------------------------
// SwarmRunStatus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state")]
pub enum SwarmRunStatus {
    Planning,
    Running { wave: usize, total_waves: usize },
    Completed,
    Cancelled,
    Failed { error: String },
}

impl SwarmRunStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Planning => "planning",
            Self::Running { .. } => "running",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed { .. } => "failed",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Cancelled | Self::Failed { .. }
        )
    }
}

// ---------------------------------------------------------------------------
// SwarmRun
// ---------------------------------------------------------------------------

/// A swarm execution record.
pub struct SwarmRun {
    pub id: String,
    pub prompt: String,
    pub status: SwarmRunStatus,
    pub result: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub cancel_token: CancellationToken,
}

/// Serializable summary for API responses.
#[derive(Debug, Serialize)]
pub struct SwarmRunSummary {
    pub id: String,
    pub prompt: String,
    pub status: SwarmRunStatus,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Detailed swarm info for API responses.
#[derive(Debug, Serialize)]
pub struct SwarmRunDetail {
    pub id: String,
    pub prompt: String,
    pub status: SwarmRunStatus,
    pub result: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// SwarmRegistry
// ---------------------------------------------------------------------------

/// In-memory registry of swarm runs.
#[derive(Clone)]
pub struct SwarmRegistry {
    runs: Arc<DashMap<String, SwarmRun>>,
}

impl SwarmRegistry {
    pub fn new() -> Self {
        Self {
            runs: Arc::new(DashMap::new()),
        }
    }

    /// Register a new swarm run. Returns the CancellationToken for the caller.
    pub fn register(&self, id: String, prompt: String) -> CancellationToken {
        let token = CancellationToken::new();
        let run = SwarmRun {
            id: id.clone(),
            prompt,
            status: SwarmRunStatus::Planning,
            result: None,
            created_at: Utc::now(),
            completed_at: None,
            cancel_token: token.clone(),
        };
        self.runs.insert(id, run);
        token
    }

    /// Update a swarm's status.
    pub fn update_status(&self, id: &str, status: SwarmRunStatus) {
        if let Some(mut run) = self.runs.get_mut(id) {
            if status.is_terminal() {
                run.completed_at = Some(Utc::now());
            }
            run.status = status;
        }
    }

    /// Set the final result of a completed swarm.
    pub fn set_result(&self, id: &str, result: String) {
        if let Some(mut run) = self.runs.get_mut(id) {
            run.result = Some(result);
        }
    }

    /// Cancel a swarm run.
    pub fn cancel(&self, id: &str) -> bool {
        if let Some(mut run) = self.runs.get_mut(id) {
            if run.status.is_terminal() {
                return false;
            }
            run.cancel_token.cancel();
            run.status = SwarmRunStatus::Cancelled;
            run.completed_at = Some(Utc::now());
            true
        } else {
            false
        }
    }

    /// Get a summary of a swarm run.
    pub fn get(&self, id: &str) -> Option<SwarmRunDetail> {
        self.runs.get(id).map(|run| SwarmRunDetail {
            id: run.id.clone(),
            prompt: run.prompt.clone(),
            status: run.status.clone(),
            result: run.result.clone(),
            created_at: run.created_at,
            completed_at: run.completed_at,
        })
    }

    /// Restore a swarm run from the database (startup hydration).
    /// Only restores terminal runs — active runs cannot be resumed.
    pub fn restore_run(&self, id: String, status_str: &str, result: Option<String>) {
        let status = match status_str {
            "completed" => SwarmRunStatus::Completed,
            "cancelled" => SwarmRunStatus::Cancelled,
            "failed" => SwarmRunStatus::Failed {
                error: result.clone().unwrap_or_default(),
            },
            _ => return, // Skip non-terminal runs (planning/running) — stale from crash
        };
        let run = SwarmRun {
            id: id.clone(),
            prompt: String::new(), // prompt not stored in load_swarm_runs return
            status,
            result,
            created_at: Utc::now(), // approximate — DB has real timestamp
            completed_at: Some(Utc::now()),
            cancel_token: CancellationToken::new(),
        };
        self.runs.insert(id, run);
    }

    /// List all swarm runs.
    pub fn list(&self) -> Vec<SwarmRunSummary> {
        self.runs
            .iter()
            .map(|entry| {
                let run = entry.value();
                SwarmRunSummary {
                    id: run.id.clone(),
                    prompt: run.prompt.clone(),
                    status: run.status.clone(),
                    created_at: run.created_at,
                    completed_at: run.completed_at,
                }
            })
            .collect()
    }
}

impl Default for SwarmRegistry {
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

    #[test]
    fn test_registry_add_and_get() {
        let registry = SwarmRegistry::new();
        let _token = registry.register("sw-1".into(), "test prompt".into());

        let detail = registry.get("sw-1").unwrap();
        assert_eq!(detail.prompt, "test prompt");
        assert_eq!(detail.status.as_str(), "planning");
    }

    #[test]
    fn test_registry_list_returns_all() {
        let registry = SwarmRegistry::new();
        let _t1 = registry.register("sw-1".into(), "prompt 1".into());
        let _t2 = registry.register("sw-2".into(), "prompt 2".into());

        let list = registry.list();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_registry_cancel_sets_status() {
        let registry = SwarmRegistry::new();
        let token = registry.register("sw-1".into(), "test".into());

        assert!(registry.cancel("sw-1"));
        assert!(token.is_cancelled());

        let detail = registry.get("sw-1").unwrap();
        assert_eq!(detail.status.as_str(), "cancelled");
        assert!(detail.completed_at.is_some());
    }

    #[test]
    fn test_registry_cancel_terminal_returns_false() {
        let registry = SwarmRegistry::new();
        let _token = registry.register("sw-1".into(), "test".into());

        registry.update_status("sw-1", SwarmRunStatus::Completed);
        assert!(!registry.cancel("sw-1"));
    }

    #[test]
    fn test_registry_completed_run_has_result() {
        let registry = SwarmRegistry::new();
        let _token = registry.register("sw-1".into(), "test".into());

        registry.update_status("sw-1", SwarmRunStatus::Completed);
        registry.set_result("sw-1", "final output".into());

        let detail = registry.get("sw-1").unwrap();
        assert_eq!(detail.result.as_deref(), Some("final output"));
        assert!(detail.completed_at.is_some());
    }

    #[test]
    fn test_registry_update_running_status() {
        let registry = SwarmRegistry::new();
        let _token = registry.register("sw-1".into(), "test".into());

        registry.update_status(
            "sw-1",
            SwarmRunStatus::Running {
                wave: 2,
                total_waves: 5,
            },
        );

        let detail = registry.get("sw-1").unwrap();
        match detail.status {
            SwarmRunStatus::Running { wave, total_waves } => {
                assert_eq!(wave, 2);
                assert_eq!(total_waves, 5);
            }
            _ => panic!("Expected Running status"),
        }
    }
}
