use mcclawd_core::types::TaskId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskStatus {
    Running,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: TaskId,
    pub prompt: String,
    pub status: TaskStatus,
}

/// Task manager with history.
/// Phase 0: in-memory only (lost on restart).
/// Phase 1+: persist to disk.
pub struct TaskManager {
    tasks: Vec<TaskRecord>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self { tasks: Vec::new() }
    }

    pub fn start_task(&mut self, prompt: String) -> TaskId {
        let id = TaskId::new();
        self.tasks.push(TaskRecord {
            id: id.clone(),
            prompt,
            status: TaskStatus::Running,
        });
        id
    }

    pub fn complete_task(&mut self, id: &TaskId) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == *id) {
            task.status = TaskStatus::Completed;
        }
    }

    pub fn fail_task(&mut self, id: &TaskId, error: String) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == *id) {
            task.status = TaskStatus::Failed(error);
        }
    }

    pub fn delete_task(&mut self, id: &TaskId) -> bool {
        let len = self.tasks.len();
        self.tasks.retain(|t| t.id != *id);
        self.tasks.len() < len
    }

    pub fn current_task(&self) -> Option<&TaskRecord> {
        self.tasks.iter().rev().find(|t| matches!(t.status, TaskStatus::Running))
    }

    pub fn all_tasks(&self) -> Vec<&TaskRecord> {
        self.tasks.iter().collect()
    }

    pub fn get_task(&self, id: &TaskId) -> Option<&TaskRecord> {
        self.tasks.iter().find(|t| t.id == *id)
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
        assert!(mgr.current_task().is_none()); // no running task
        assert!(matches!(mgr.get_task(&id).unwrap().status, TaskStatus::Completed));
    }

    #[test]
    fn test_fail_task() {
        let mut mgr = TaskManager::new();
        let id = mgr.start_task("will fail".to_string());

        mgr.fail_task(&id, "something broke".to_string());
        let task = mgr.get_task(&id).unwrap();
        assert!(matches!(task.status, TaskStatus::Failed(_)));
        if let TaskStatus::Failed(ref msg) = task.status {
            assert_eq!(msg, "something broke");
        }
    }

    #[test]
    fn test_multiple_tasks() {
        let mut mgr = TaskManager::new();
        let id1 = mgr.start_task("task one".to_string());
        let id2 = mgr.start_task("task two".to_string());

        assert_eq!(mgr.all_tasks().len(), 2);
        mgr.complete_task(&id1);
        assert_eq!(mgr.all_tasks().len(), 2);
        assert!(mgr.get_task(&id1).is_some());
        assert!(mgr.get_task(&id2).is_some());
    }

    #[test]
    fn test_delete_task() {
        let mut mgr = TaskManager::new();
        let id1 = mgr.start_task("task one".to_string());
        let id2 = mgr.start_task("task two".to_string());

        assert!(mgr.delete_task(&id1));
        assert_eq!(mgr.all_tasks().len(), 1);
        assert!(mgr.get_task(&id1).is_none());
        assert!(mgr.get_task(&id2).is_some());
    }

    #[test]
    fn test_wrong_id_ignored() {
        let mut mgr = TaskManager::new();
        let id = mgr.start_task("real task".to_string());
        let wrong_id = TaskId::new();

        mgr.complete_task(&wrong_id);
        let task = mgr.get_task(&id).unwrap();
        assert!(matches!(task.status, TaskStatus::Running));

        mgr.complete_task(&id);
        let task = mgr.get_task(&id).unwrap();
        assert!(matches!(task.status, TaskStatus::Completed));
    }
}
