use std::time::{Duration, Instant};

use crate::dag::SubtaskNode;
use crate::merger::{SubtaskResult, SubtaskStatus};
use crate::shared_memory::SharedMemory;

/// A worker that executes a single subtask.
pub struct WorkerAgent {
    /// Timeout for this worker (enforced in production with tokio::time::timeout)
    #[allow(dead_code)]
    timeout: Duration,
}

impl WorkerAgent {
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }

    /// Execute a subtask: read inputs from shared memory, run the agent, write output.
    ///
    /// In production, this spawns a Rig agent with the subtask's prompt and tools.
    /// For testing, use `execute_with_fn()` to provide a custom executor.
    pub async fn execute(&self, subtask: &SubtaskNode, shared_memory: &SharedMemory) -> SubtaskResult {
        let start = Instant::now();

        // Gather inputs from shared memory
        let inputs: Vec<String> = subtask
            .input_keys
            .iter()
            .filter_map(|key| shared_memory.get::<String>(key))
            .collect();

        // Placeholder: in production, would create a Rig agent and run it
        // For now, just echo the prompt with inputs
        let output = if inputs.is_empty() {
            format!(
                "[Worker:{role}] Completed: {prompt}",
                role = subtask.agent_role,
                prompt = subtask.prompt
            )
        } else {
            format!(
                "[Worker:{role}] Completed: {prompt} (inputs: {inputs})",
                role = subtask.agent_role,
                prompt = subtask.prompt,
                inputs = inputs.join(", ")
            )
        };

        // Write output to shared memory
        shared_memory.set(&subtask.output_key, &output);

        SubtaskResult {
            subtask_id: subtask.id.clone(),
            agent_role: subtask.agent_role.clone(),
            output: Some(output),
            status: SubtaskStatus::Completed,
            duration_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Execute with a custom function (for testing).
    pub async fn execute_with_fn<F>(
        &self,
        subtask: &SubtaskNode,
        shared_memory: &SharedMemory,
        func: F,
    ) -> SubtaskResult
    where
        F: FnOnce(&SubtaskNode, &SharedMemory) -> std::result::Result<String, String>,
    {
        let start = Instant::now();
        match func(subtask, shared_memory) {
            Ok(output) => {
                shared_memory.set(&subtask.output_key, &output);
                SubtaskResult {
                    subtask_id: subtask.id.clone(),
                    agent_role: subtask.agent_role.clone(),
                    output: Some(output),
                    status: SubtaskStatus::Completed,
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            }
            Err(msg) => SubtaskResult {
                subtask_id: subtask.id.clone(),
                agent_role: subtask.agent_role.clone(),
                output: None,
                status: SubtaskStatus::Failed(msg),
                duration_ms: start.elapsed().as_millis() as u64,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::SubtaskNode;

    fn test_node(id: &str, role: &str) -> SubtaskNode {
        SubtaskNode {
            id: id.into(),
            prompt: format!("Do {}", id),
            agent_role: role.into(),
            input_keys: vec![],
            output_key: format!("{}_out", id),
        }
    }

    #[tokio::test]
    async fn execute_writes_to_shared_memory() {
        let worker = WorkerAgent::new(Duration::from_secs(30));
        let mem = SharedMemory::new();
        let node = test_node("research", "researcher");
        let result = worker.execute(&node, &mem).await;
        assert!(matches!(result.status, SubtaskStatus::Completed));
        assert!(result.output.is_some());
        // Output should be written to shared memory
        let stored: Option<String> = mem.get("research_out");
        assert!(stored.is_some());
    }

    #[tokio::test]
    async fn execute_reads_inputs() {
        let worker = WorkerAgent::new(Duration::from_secs(30));
        let mem = SharedMemory::new();
        mem.set("prev_out", "previous result");
        let mut node = test_node("code", "coder");
        node.input_keys = vec!["prev_out".into()];
        let result = worker.execute(&node, &mem).await;
        assert!(result.output.as_ref().unwrap().contains("previous result"));
    }

    #[tokio::test]
    async fn execute_with_custom_fn_success() {
        let worker = WorkerAgent::new(Duration::from_secs(30));
        let mem = SharedMemory::new();
        let node = test_node("task", "worker");
        let result = worker
            .execute_with_fn(&node, &mem, |n, _| {
                Ok(format!("Custom output for {}", n.id))
            })
            .await;
        assert!(matches!(result.status, SubtaskStatus::Completed));
        assert!(result.output.unwrap().contains("Custom output"));
    }

    #[tokio::test]
    async fn execute_with_custom_fn_failure() {
        let worker = WorkerAgent::new(Duration::from_secs(30));
        let mem = SharedMemory::new();
        let node = test_node("task", "worker");
        let result = worker
            .execute_with_fn(&node, &mem, |_, _| Err("Something went wrong".into()))
            .await;
        assert!(matches!(result.status, SubtaskStatus::Failed(_)));
        assert!(result.output.is_none());
    }
}
