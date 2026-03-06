//! Task scheduler — cron-based recurring task creation.
//!
//! Phase 0: in-memory schedule storage with tokio interval check.
//! Phase 1+: persist schedules to disk/Postgres.

use chrono::{DateTime, Utc};
use cron::Schedule;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// ScheduledTask
// ---------------------------------------------------------------------------

/// A recurring task definition with a cron expression.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    /// Standard cron expression (6 fields: sec min hour day month weekday).
    /// Example: "0 */5 * * * *" (every 5 minutes)
    pub cron_expression: String,
    pub prompt: String,
    pub workspace: Option<String>,
    pub enabled: bool,
    pub last_run: Option<DateTime<Utc>>,
    pub next_run: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Request to create a new scheduled task.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateScheduleRequest {
    pub name: String,
    pub cron_expression: String,
    pub prompt: String,
    #[serde(default)]
    pub workspace: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

// ---------------------------------------------------------------------------
// TaskScheduler
// ---------------------------------------------------------------------------

/// In-memory scheduler that checks for due tasks on a configurable interval.
#[derive(Clone)]
pub struct TaskScheduler {
    schedules: Arc<RwLock<HashMap<String, ScheduledTask>>>,
}

impl TaskScheduler {
    pub fn new() -> Self {
        Self {
            schedules: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Validate a cron expression. Returns error message if invalid.
    pub fn validate_cron(expression: &str) -> Result<(), String> {
        Schedule::from_str(expression)
            .map(|_| ())
            .map_err(|e| format!("Invalid cron expression: {e}"))
    }

    /// Compute the next run time from a cron expression relative to `after`.
    pub fn next_run_after(expression: &str, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        Schedule::from_str(expression)
            .ok()
            .and_then(|schedule| schedule.after(&after).next())
    }

    /// Add a new schedule. Returns the created ScheduledTask.
    pub async fn add_schedule(&self, req: CreateScheduleRequest) -> Result<ScheduledTask, String> {
        Self::validate_cron(&req.cron_expression)?;

        let now = Utc::now();
        let next_run = if req.enabled {
            Self::next_run_after(&req.cron_expression, now)
        } else {
            None
        };

        let task = ScheduledTask {
            id: Uuid::new_v4().to_string(),
            name: req.name,
            cron_expression: req.cron_expression,
            prompt: req.prompt,
            workspace: req.workspace,
            enabled: req.enabled,
            last_run: None,
            next_run,
            created_at: now,
        };

        let mut schedules = self.schedules.write().await;
        schedules.insert(task.id.clone(), task.clone());
        Ok(task)
    }

    /// Restore a schedule from the database (startup hydration).
    /// Uses the provided ID instead of generating a new one.
    pub async fn restore_schedule(&self, id: String, req: CreateScheduleRequest) {
        let now = Utc::now();
        let next_run = if req.enabled {
            Self::next_run_after(&req.cron_expression, now)
        } else {
            None
        };

        let task = ScheduledTask {
            id,
            name: req.name,
            cron_expression: req.cron_expression,
            prompt: req.prompt,
            workspace: req.workspace,
            enabled: req.enabled,
            last_run: None,
            next_run,
            created_at: now,
        };

        let mut schedules = self.schedules.write().await;
        schedules.insert(task.id.clone(), task);
    }

    /// Remove a schedule by ID. Returns true if it existed.
    pub async fn remove_schedule(&self, id: &str) -> bool {
        let mut schedules = self.schedules.write().await;
        schedules.remove(id).is_some()
    }

    /// List all schedules.
    pub async fn list_schedules(&self) -> Vec<ScheduledTask> {
        let schedules = self.schedules.read().await;
        schedules.values().cloned().collect()
    }

    /// Get a schedule by ID.
    pub async fn get_schedule(&self, id: &str) -> Option<ScheduledTask> {
        let schedules = self.schedules.read().await;
        schedules.get(id).cloned()
    }

    /// Toggle a schedule's enabled state. Returns the updated task or None if not found.
    pub async fn toggle_schedule(&self, id: &str) -> Option<ScheduledTask> {
        let mut schedules = self.schedules.write().await;
        if let Some(task) = schedules.get_mut(id) {
            task.enabled = !task.enabled;
            if task.enabled {
                task.next_run = Self::next_run_after(&task.cron_expression, Utc::now());
            } else {
                task.next_run = None;
            }
            Some(task.clone())
        } else {
            None
        }
    }

    /// Check all schedules and return IDs + prompts of tasks that are due.
    /// Updates `last_run` and `next_run` for triggered tasks.
    pub async fn check_due_tasks(&self) -> Vec<(String, String, Option<String>)> {
        let now = Utc::now();
        let mut due = Vec::new();
        let mut schedules = self.schedules.write().await;

        for task in schedules.values_mut() {
            if !task.enabled {
                continue;
            }
            if let Some(next) = task.next_run {
                if next <= now {
                    due.push((task.id.clone(), task.prompt.clone(), task.workspace.clone()));
                    task.last_run = Some(now);
                    task.next_run = Self::next_run_after(&task.cron_expression, now);
                }
            }
        }

        due
    }

    /// Start the background scheduler loop. Checks every `interval_secs` seconds.
    /// The callback `on_trigger` is called for each due task with (schedule_id, prompt, workspace).
    pub fn start_background_loop(
        self,
        interval_secs: u64,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            loop {
                interval.tick().await;
                let due_tasks = self.check_due_tasks().await;
                for (schedule_id, prompt, workspace) in due_tasks {
                    tracing::info!(
                        schedule_id = %schedule_id,
                        prompt = %prompt,
                        workspace = ?workspace,
                        "Scheduled task triggered — would create task"
                    );
                    // Phase 1+: Actually create a TaskRecord via TaskManager here
                }
            }
        })
    }
}

impl Default for TaskScheduler {
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
    fn test_cron_parse_valid() {
        assert!(TaskScheduler::validate_cron("0 */5 * * * *").is_ok());
        assert!(TaskScheduler::validate_cron("0 0 * * * *").is_ok());
        assert!(TaskScheduler::validate_cron("0 30 9 * * Mon-Fri").is_ok());
    }

    #[test]
    fn test_cron_parse_invalid() {
        assert!(TaskScheduler::validate_cron("not a cron").is_err());
        assert!(TaskScheduler::validate_cron("").is_err());
        assert!(TaskScheduler::validate_cron("* * *").is_err());
    }

    #[test]
    fn test_next_run_calculation() {
        let now = Utc::now();
        let next = TaskScheduler::next_run_after("0 * * * * *", now);
        assert!(next.is_some());
        let next = next.unwrap();
        assert!(next > now);
        // Every minute — next run should be within 60 seconds
        assert!((next - now).num_seconds() <= 60);
    }

    #[tokio::test]
    async fn test_schedule_crud() {
        let scheduler = TaskScheduler::new();

        // Create
        let req = CreateScheduleRequest {
            name: "test".into(),
            cron_expression: "0 */5 * * * *".into(),
            prompt: "do something".into(),
            workspace: None,
            enabled: true,
        };
        let task = scheduler.add_schedule(req).await.unwrap();
        assert_eq!(task.name, "test");
        assert!(task.next_run.is_some());

        // List
        let list = scheduler.list_schedules().await;
        assert_eq!(list.len(), 1);

        // Get
        let got = scheduler.get_schedule(&task.id).await;
        assert!(got.is_some());

        // Toggle
        let toggled = scheduler.toggle_schedule(&task.id).await.unwrap();
        assert!(!toggled.enabled);
        assert!(toggled.next_run.is_none());

        // Toggle back
        let toggled = scheduler.toggle_schedule(&task.id).await.unwrap();
        assert!(toggled.enabled);

        // Delete
        assert!(scheduler.remove_schedule(&task.id).await);
        assert!(scheduler.list_schedules().await.is_empty());
    }

    #[tokio::test]
    async fn test_scheduler_skips_disabled_tasks() {
        let scheduler = TaskScheduler::new();

        let req = CreateScheduleRequest {
            name: "disabled".into(),
            cron_expression: "0 * * * * *".into(),
            prompt: "should not run".into(),
            workspace: None,
            enabled: false,
        };
        let _task = scheduler.add_schedule(req).await.unwrap();

        let due = scheduler.check_due_tasks().await;
        assert!(due.is_empty());
    }

    #[tokio::test]
    async fn test_add_schedule_invalid_cron() {
        let scheduler = TaskScheduler::new();

        let req = CreateScheduleRequest {
            name: "bad".into(),
            cron_expression: "invalid".into(),
            prompt: "nope".into(),
            workspace: None,
            enabled: true,
        };
        assert!(scheduler.add_schedule(req).await.is_err());
    }

    #[tokio::test]
    async fn test_scheduler_updates_last_run() {
        let scheduler = TaskScheduler::new();

        // Use "every second" so it's always due
        let req = CreateScheduleRequest {
            name: "frequent".into(),
            cron_expression: "* * * * * *".into(),
            prompt: "tick".into(),
            workspace: None,
            enabled: true,
        };
        let task = scheduler.add_schedule(req).await.unwrap();

        // Force next_run to the past so it triggers
        {
            let mut schedules = scheduler.schedules.write().await;
            if let Some(t) = schedules.get_mut(&task.id) {
                t.next_run = Some(Utc::now() - chrono::Duration::seconds(1));
            }
        }

        let due = scheduler.check_due_tasks().await;
        assert_eq!(due.len(), 1);

        // Verify last_run was updated
        let updated = scheduler.get_schedule(&task.id).await.unwrap();
        assert!(updated.last_run.is_some());
    }
}
