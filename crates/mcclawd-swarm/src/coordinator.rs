use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::dag::TaskDag;
use crate::error::{Result, SwarmError};
use crate::merger::{MergeStrategy, OutputMerger, SubtaskResult};
use crate::shared_memory::SharedMemory;
use crate::worker::WorkerAgent;

/// Configuration for a swarm run.
#[derive(Debug, Clone)]
pub struct SwarmConfig {
    pub max_concurrent_workers: usize,
    pub worker_timeout: Duration,
    pub merge_strategy: MergeStrategy,
}

impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            max_concurrent_workers: 4,
            worker_timeout: Duration::from_secs(300),
            merge_strategy: MergeStrategy::Concatenate,
        }
    }
}

/// Result of a completed swarm execution.
#[derive(Debug)]
pub struct SwarmResult {
    pub final_output: String,
    pub subtask_results: HashMap<String, SubtaskResult>,
    pub total_duration_ms: u64,
}

/// Orchestrates multi-agent swarm execution.
pub struct SwarmCoordinator {
    config: SwarmConfig,
    shared_memory: SharedMemory,
}

impl SwarmCoordinator {
    pub fn new(config: SwarmConfig) -> Self {
        Self {
            config,
            shared_memory: SharedMemory::new(),
        }
    }

    /// Get a reference to shared memory (for inspection/testing).
    pub fn shared_memory(&self) -> &SharedMemory {
        &self.shared_memory
    }

    /// Execute a pre-built DAG through wave-based parallel execution.
    pub async fn execute(&self, prompt: &str, dag: &TaskDag) -> Result<SwarmResult> {
        let start = Instant::now();

        // Validate DAG
        dag.validate()?;

        // Get execution waves
        let waves = dag.topological_waves()?;
        let mut all_results: HashMap<String, SubtaskResult> = HashMap::new();

        // Execute each wave
        for (wave_idx, wave) in waves.iter().enumerate() {
            tracing::info!(
                "Executing wave {}/{}: {} tasks",
                wave_idx + 1,
                waves.len(),
                wave.len()
            );

            // Execute tasks in this wave concurrently (up to max_concurrent_workers)
            let mut handles = Vec::new();
            for subtask_id in wave {
                let subtask = dag
                    .subtask(subtask_id)
                    .ok_or_else(|| SwarmError::WorkerFailed {
                        subtask_id: subtask_id.clone(),
                        message: "Subtask not found in DAG".into(),
                    })?
                    .clone();
                let mem = self.shared_memory.clone();
                let timeout = self.config.worker_timeout;
                handles.push(tokio::spawn(async move {
                    let w = WorkerAgent::new(timeout);
                    w.execute(&subtask, &mem).await
                }));
            }

            // Collect results
            for handle in handles {
                let result = handle.await.map_err(|e| SwarmError::WorkerFailed {
                    subtask_id: "unknown".into(),
                    message: format!("Task join error: {e}"),
                })?;
                all_results.insert(result.subtask_id.clone(), result);
            }
        }

        // Flatten all waves for ordered IDs
        let ordered_ids: Vec<String> = waves.into_iter().flatten().collect();

        // Merge results
        let merger = OutputMerger::new(self.config.merge_strategy.clone());
        let final_output = merger.merge(prompt, &all_results, &ordered_ids).await?;

        Ok(SwarmResult {
            final_output,
            subtask_results: all_results,
            total_duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::{SubtaskNode, TaskDag};

    fn build_test_dag() -> TaskDag {
        let mut dag = TaskDag::new();
        dag.add_subtask(SubtaskNode {
            id: "research".into(),
            prompt: "Research topic".into(),
            agent_role: "researcher".into(),
            input_keys: vec![],
            output_key: "research_out".into(),
        });
        dag.add_subtask(SubtaskNode {
            id: "code".into(),
            prompt: "Write code".into(),
            agent_role: "coder".into(),
            input_keys: vec!["research_out".into()],
            output_key: "code_out".into(),
        });
        dag.add_dependency("research", "code").unwrap();
        dag
    }

    #[tokio::test]
    async fn execute_simple_dag() {
        let coordinator = SwarmCoordinator::new(SwarmConfig::default());
        let dag = build_test_dag();
        let result = coordinator.execute("Build something", &dag).await.unwrap();
        assert!(!result.final_output.is_empty());
        assert_eq!(result.subtask_results.len(), 2);
        // Both should be completed
        for r in result.subtask_results.values() {
            assert!(matches!(r.status, crate::merger::SubtaskStatus::Completed));
        }
    }

    #[tokio::test]
    async fn execute_writes_to_shared_memory() {
        let coordinator = SwarmCoordinator::new(SwarmConfig::default());
        let dag = build_test_dag();
        coordinator.execute("test", &dag).await.unwrap();
        // Shared memory should have outputs
        let research: Option<String> = coordinator.shared_memory().get("research_out");
        assert!(research.is_some());
        let code: Option<String> = coordinator.shared_memory().get("code_out");
        assert!(code.is_some());
    }

    #[tokio::test]
    async fn execute_parallel_wave() {
        let mut dag = TaskDag::new();
        // Three independent tasks in wave 1
        dag.add_subtask(SubtaskNode {
            id: "a".into(),
            prompt: "A".into(),
            agent_role: "worker".into(),
            input_keys: vec![],
            output_key: "a_out".into(),
        });
        dag.add_subtask(SubtaskNode {
            id: "b".into(),
            prompt: "B".into(),
            agent_role: "worker".into(),
            input_keys: vec![],
            output_key: "b_out".into(),
        });
        dag.add_subtask(SubtaskNode {
            id: "c".into(),
            prompt: "C".into(),
            agent_role: "worker".into(),
            input_keys: vec![],
            output_key: "c_out".into(),
        });
        // One task depends on all three
        dag.add_subtask(SubtaskNode {
            id: "merge".into(),
            prompt: "Merge".into(),
            agent_role: "merger".into(),
            input_keys: vec!["a_out".into(), "b_out".into(), "c_out".into()],
            output_key: "final".into(),
        });
        dag.add_dependency("a", "merge").unwrap();
        dag.add_dependency("b", "merge").unwrap();
        dag.add_dependency("c", "merge").unwrap();

        let coordinator = SwarmCoordinator::new(SwarmConfig::default());
        let result = coordinator.execute("test", &dag).await.unwrap();
        assert_eq!(result.subtask_results.len(), 4);
    }

    #[tokio::test]
    async fn execute_last_node_strategy() {
        let config = SwarmConfig {
            merge_strategy: MergeStrategy::LastNode,
            ..Default::default()
        };
        let coordinator = SwarmCoordinator::new(config);
        let dag = build_test_dag();
        let result = coordinator.execute("test", &dag).await.unwrap();
        // Should only contain the code output
        assert!(result.final_output.contains("coder"));
    }

    #[test]
    fn swarm_config_default() {
        let cfg = SwarmConfig::default();
        assert_eq!(cfg.max_concurrent_workers, 4);
        assert_eq!(cfg.worker_timeout, Duration::from_secs(300));
    }
}
