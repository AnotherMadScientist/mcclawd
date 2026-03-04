use mcclawd_core::types::TaskId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskStatus {
    Running,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct TaskRecord {
    pub id: TaskId,
    pub prompt: String,
    pub status: TaskStatus,
}

/// Phase 0: single interactive task at a time.
/// Phase 2: concurrent tasks with interactive + background modes.
pub struct TaskManager {
    current: Option<TaskRecord>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self { current: None }
    }

    pub fn start_task(&mut self, prompt: String) -> TaskId {
        let id = TaskId::new();
        self.current = Some(TaskRecord {
            id: id.clone(),
            prompt,
            status: TaskStatus::Running,
        });
        id
    }

    pub fn complete_task(&mut self, id: &TaskId) {
        if let Some(ref mut task) = self.current {
            if task.id == *id {
                task.status = TaskStatus::Completed;
            }
        }
    }

    pub fn fail_task(&mut self, id: &TaskId, error: String) {
        if let Some(ref mut task) = self.current {
            if task.id == *id {
                task.status = TaskStatus::Failed(error);
            }
        }
    }

    pub fn current_task(&self) -> Option<&TaskRecord> {
        self.current.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_start_and_complete_task() {
        let mut mgr = TaskManager::new();
        assert!(mgr.current_task().is_none());

        let id = mgr.start_task("do something".to_string());
        let task = mgr.current_task().unwrap();
        assert_eq!(task.id, id);
        assert_eq!(task.prompt, "do something");
        assert!(matches!(task.status, TaskStatus::Running));

        mgr.complete_task(&id);
        let task = mgr.current_task().unwrap();
        assert!(matches!(task.status, TaskStatus::Completed));
    }

    #[test]
    fn test_fail_task() {
        let mut mgr = TaskManager::new();
        let id = mgr.start_task("will fail".to_string());

        mgr.fail_task(&id, "something broke".to_string());
        let task = mgr.current_task().unwrap();
        assert!(matches!(task.status, TaskStatus::Failed(_)));
        if let TaskStatus::Failed(ref msg) = task.status {
            assert_eq!(msg, "something broke");
        }
    }

    #[test]
    fn test_wrong_id_ignored() {
        let mut mgr = TaskManager::new();
        let id = mgr.start_task("real task".to_string());
        let wrong_id = TaskId::new();

        mgr.complete_task(&wrong_id);
        let task = mgr.current_task().unwrap();
        assert!(matches!(task.status, TaskStatus::Running));

        mgr.complete_task(&id);
        let task = mgr.current_task().unwrap();
        assert!(matches!(task.status, TaskStatus::Completed));
    }
}
