//! Agent supervisor — spawns, monitors, and restarts agents in sandbox containers.

use crate::sandbox::{ImageBuilder, SandboxOrchestrator};
use mcclawd_channels::OutboundChunk;
use mcclawd_core::skills::{LoadedSkill, SandboxConfig};
use mcclawd_core::types::TaskId;
use mcclawd_swarm::{SubtaskNode, SwarmConfig, SwarmCoordinator, SwarmResult, TaskDag};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

#[allow(dead_code)]
const MAX_RETRIES: u32 = 3;
#[allow(dead_code)]
const INITIAL_BACKOFF_SECS: u64 = 1;

/// Tracks a running agent task.
#[allow(dead_code)]
struct RunningAgent {
    container_id: String,
    task_id: TaskId,
    agent_id: String,
}

/// Supervises agent lifecycle: spawn, monitor, restart, cleanup.
pub struct AgentSupervisor {
    orchestrator: SandboxOrchestrator,
    image_builder: Arc<ImageBuilder>,
    sandbox_config: SandboxConfig,
    running: Arc<RwLock<HashMap<TaskId, RunningAgent>>>,
    max_concurrent: usize,
}

impl AgentSupervisor {
    pub fn new(
        orchestrator: SandboxOrchestrator,
        image_builder: Arc<ImageBuilder>,
        sandbox_config: SandboxConfig,
        max_concurrent: usize,
    ) -> Self {
        Self {
            orchestrator,
            image_builder,
            sandbox_config,
            running: Arc::new(RwLock::new(HashMap::new())),
            max_concurrent,
        }
    }

    /// Spawn an agent task in a sandbox container.
    pub async fn spawn_agent(
        &self,
        task_id: TaskId,
        agent_id: &str,
        skills: &[LoadedSkill],
        secrets: HashMap<String, String>,
        output_tx: broadcast::Sender<OutboundChunk>,
    ) -> anyhow::Result<()> {
        // Check concurrency limit
        let running_count = self.running.read().await.len();
        if running_count >= self.max_concurrent {
            anyhow::bail!(
                "max concurrent agents ({}) reached, {} running",
                self.max_concurrent,
                running_count
            );
        }

        // Build image with skill layers
        let image = self
            .image_builder
            .build_image(&self.sandbox_config.base_image, skills)
            .await?;

        // Create and start container
        let handle = self
            .orchestrator
            .create_container(
                &task_id,
                agent_id,
                &image,
                &self.sandbox_config,
                &secrets,
            )
            .await?;

        // Track running agent
        {
            let mut running = self.running.write().await;
            running.insert(
                task_id.clone(),
                RunningAgent {
                    container_id: handle.container_id.clone(),
                    task_id: task_id.clone(),
                    agent_id: agent_id.to_string(),
                },
            );
        }

        // Spawn monitoring task
        let orchestrator = self.orchestrator.clone();
        let container_id = handle.container_id.clone();
        let running = self.running.clone();
        let task_id_clone = task_id.clone();

        tokio::spawn(async move {
            // Wait for container to exit
            match orchestrator.wait_container(&container_id).await {
                Ok(exit_code) => {
                    if exit_code == 0 {
                        let _ = output_tx.send(OutboundChunk::Done);
                    } else {
                        let _ = output_tx.send(OutboundChunk::Error(format!(
                            "agent exited with code {exit_code}"
                        )));
                    }
                }
                Err(e) => {
                    let _ =
                        output_tx.send(OutboundChunk::Error(format!("monitor error: {e}")));
                }
            }

            // Cleanup
            let _ = orchestrator.cleanup_container(&container_id).await;
            let mut running = running.write().await;
            running.remove(&task_id_clone);
        });

        Ok(())
    }

    /// Stop an agent and cleanup its container.
    pub async fn stop_agent(&self, task_id: &TaskId) -> anyhow::Result<()> {
        let running = self.running.read().await;
        if let Some(agent) = running.get(task_id) {
            self.orchestrator
                .cleanup_container(&agent.container_id)
                .await?;
        }
        Ok(())
    }

    /// Number of currently running agents.
    pub async fn running_count(&self) -> usize {
        self.running.read().await.len()
    }

    /// Spawn a swarm execution with a placeholder 2-node test DAG.
    ///
    /// This is a Phase 2 placeholder — real wiring connects to the planner LLM
    /// which decomposes the prompt into a proper DAG.
    pub async fn spawn_swarm(&self, prompt: &str) -> anyhow::Result<SwarmResult> {
        tracing::info!(prompt, "Spawning swarm execution");

        let coordinator = SwarmCoordinator::new(SwarmConfig::default());

        // Build a simple 2-node test DAG: research → synthesize
        let mut dag = TaskDag::new();
        dag.add_subtask(SubtaskNode {
            id: "research".into(),
            prompt: format!("Research: {prompt}"),
            agent_role: "researcher".into(),
            input_keys: vec![],
            output_key: "research_out".into(),
        });
        dag.add_subtask(SubtaskNode {
            id: "synthesize".into(),
            prompt: format!("Synthesize research into answer for: {prompt}"),
            agent_role: "writer".into(),
            input_keys: vec!["research_out".into()],
            output_key: "synthesize_out".into(),
        });
        dag.add_dependency("research", "synthesize")
            .map_err(|e| anyhow::anyhow!("DAG error: {e}"))?;

        let result = coordinator
            .execute(prompt, &dag)
            .await
            .map_err(|e| anyhow::anyhow!("Swarm execution failed: {e}"))?;

        tracing::info!(
            duration_ms = result.total_duration_ms,
            subtasks = result.subtask_results.len(),
            "Swarm execution completed"
        );

        Ok(result)
    }
}
