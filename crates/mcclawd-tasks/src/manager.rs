use mcclawd_core::types::TaskId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Pending,
    Building,
    Running,
    Restarting { attempt: u32, next_retry_secs: u64 },
    SwarmRunning {
        swarm_id: String,
        wave: usize,
        total_waves: usize,
    },
    Completed,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: TaskId,
    pub prompt: String,
    pub status: TaskStatus,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Skills selected for this task (e.g. ["filesystem", "web-search"]).
    #[serde(default)]
    pub selected_skills: Vec<String>,
    /// Tool names explicitly allowed for this task.
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// Tool profile used for this task (e.g. "Coding", "Research").
    #[serde(default)]
    pub tool_profile: Option<String>,
    /// Combined SKILL.md context for the agent system prompt (persisted for container restart).
    #[serde(default)]
    pub skill_context: String,
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
        self.start_task_with_tags(prompt, Vec::new())
    }

    pub fn start_task_with_tags(&mut self, prompt: String, tags: Vec<String>) -> TaskId {
        let id = TaskId::new();
        self.tasks.push(TaskRecord {
            id: id.clone(),
            prompt,
            status: TaskStatus::Running,
            tags,
            selected_skills: Vec::new(),
            allowed_tools: Vec::new(),
            tool_profile: None,
            skill_context: String::new(),
        });
        id
    }

    pub fn create_task(&mut self, prompt: String) -> TaskId {
        self.create_task_with_tags(prompt, Vec::new())
    }

    pub fn create_task_with_tags(&mut self, prompt: String, tags: Vec<String>) -> TaskId {
        let id = TaskId::new();
        self.tasks.push(TaskRecord {
            id: id.clone(),
            prompt,
            status: TaskStatus::Pending,
            tags,
            selected_skills: Vec::new(),
            allowed_tools: Vec::new(),
            tool_profile: None,
            skill_context: String::new(),
        });
        id
    }

    /// Delete all tasks matching a given tag. Returns the IDs of deleted tasks.
    pub fn delete_by_tag(&mut self, tag: &str) -> Vec<TaskId> {
        let to_delete: Vec<TaskId> = self
            .tasks
            .iter()
            .filter(|t| t.tags.iter().any(|tg| tg == tag))
            .map(|t| t.id.clone())
            .collect();
        self.tasks.retain(|t| !t.tags.iter().any(|tg| tg == tag));
        to_delete
    }

    pub fn building(&mut self, id: &TaskId) {
        self.set_status(id, TaskStatus::Building);
    }

    pub fn running(&mut self, id: &TaskId) {
        self.set_status(id, TaskStatus::Running);
    }

    pub fn restarting(&mut self, id: &TaskId, attempt: u32, next_retry_secs: u64) {
        self.set_status(id, TaskStatus::Restarting { attempt, next_retry_secs });
    }

    pub fn swarm_running(&mut self, id: &TaskId, swarm_id: String, wave: usize, total_waves: usize) {
        self.set_status(
            id,
            TaskStatus::SwarmRunning {
                swarm_id,
                wave,
                total_waves,
            },
        );
    }

    fn set_status(&mut self, id: &TaskId, status: TaskStatus) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == *id) {
            task.status = status;
        }
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

    /// Restore a task from persistent storage (e.g. postgres hydration on startup).
    /// Inserts with the given ID — does NOT generate a new one.
    pub fn restore_task(&mut self, id: TaskId, prompt: String, status: TaskStatus) {
        // Avoid duplicates
        if self.tasks.iter().any(|t| t.id == id) {
            return;
        }
        self.tasks.push(TaskRecord {
            id,
            prompt,
            status,
            tags: Vec::new(),
            selected_skills: Vec::new(),
            allowed_tools: Vec::new(),
            tool_profile: None,
            skill_context: String::new(),
        });
    }

    /// Hydrate a task from DB with full metadata including tags, skills, tools, and skill context.
    pub fn hydrate_task(
        &mut self,
        id: TaskId,
        prompt: String,
        status: TaskStatus,
        tags: Vec<String>,
        selected_skills: Vec<String>,
        allowed_tools: Vec<String>,
        tool_profile: Option<String>,
        skill_context: String,
    ) {
        if self.tasks.iter().any(|t| t.id == id) {
            return;
        }
        self.tasks.push(TaskRecord {
            id,
            prompt,
            status,
            tags,
            selected_skills,
            allowed_tools,
            tool_profile,
            skill_context,
        });
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

    #[test]
    fn test_task_state_machine() {
        let mut mgr = TaskManager::new();
        let id = mgr.create_task("build something".to_string());
        assert!(matches!(mgr.get_task(&id).unwrap().status, TaskStatus::Pending));
        mgr.building(&id);
        assert!(matches!(mgr.get_task(&id).unwrap().status, TaskStatus::Building));
        mgr.running(&id);
        assert!(matches!(mgr.get_task(&id).unwrap().status, TaskStatus::Running));
        mgr.complete_task(&id);
        assert!(matches!(mgr.get_task(&id).unwrap().status, TaskStatus::Completed));
    }

    #[test]
    fn test_restarting_state() {
        let mut mgr = TaskManager::new();
        let id = mgr.create_task("crashy task".to_string());
        mgr.running(&id);
        mgr.restarting(&id, 1, 2);
        match &mgr.get_task(&id).unwrap().status {
            TaskStatus::Restarting { attempt, next_retry_secs } => {
                assert_eq!(*attempt, 1);
                assert_eq!(*next_retry_secs, 2);
            }
            other => panic!("expected Restarting, got {:?}", other),
        }
        mgr.running(&id);
        mgr.restarting(&id, 2, 4);
        mgr.fail_task(&id, "max retries exceeded".to_string());
        assert!(matches!(mgr.get_task(&id).unwrap().status, TaskStatus::Failed(_)));
    }

    #[test]
    fn test_swarm_running_state() {
        let mut mgr = TaskManager::new();
        let id = mgr.create_task("swarm task".to_string());
        mgr.swarm_running(&id, "swarm-abc".to_string(), 1, 3);
        match &mgr.get_task(&id).unwrap().status {
            TaskStatus::SwarmRunning {
                swarm_id,
                wave,
                total_waves,
            } => {
                assert_eq!(swarm_id, "swarm-abc");
                assert_eq!(*wave, 1);
                assert_eq!(*total_waves, 3);
            }
            other => panic!("expected SwarmRunning, got {:?}", other),
        }
    }

    #[test]
    fn test_swarm_running_serde_roundtrip() {
        let status = TaskStatus::SwarmRunning {
            swarm_id: "swarm-123".to_string(),
            wave: 2,
            total_waves: 5,
        };
        let json = serde_json::to_string(&status).expect("serialize");
        let deserialized: TaskStatus = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(status, deserialized);
    }
}
