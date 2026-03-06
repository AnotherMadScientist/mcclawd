//! Sandbox orchestrator — manages Docker containers for agent execution.

use bollard::container::{
    Config, CreateContainerOptions, LogsOptions, RemoveContainerOptions, StopContainerOptions,
    WaitContainerOptions,
};
use bollard::exec::{CreateExecOptions, StartExecOptions};
use bollard::models::{HostConfig, Mount, MountTypeEnum, RestartPolicy, RestartPolicyNameEnum};
use bollard::Docker;
use futures::StreamExt;
use mcclawd_core::skills::SandboxConfig;
use mcclawd_core::types::TaskId;
use std::collections::HashMap;
use tokio::sync::mpsc;

/// Everything an agent container needs to run.
/// Produced by McpPorter, consumed by SandboxOrchestrator.
#[derive(Debug, Clone)]
pub struct AgentEnvironment {
    /// Docker image tag, e.g. "mcclawd-agent:a1b2c3d4e5f6"
    pub image: String,
    /// Docker network name, e.g. "mcclawd_tools"
    pub network: String,
    /// AgentGateway URL accessible from inside the container
    pub gateway_url: String,
    /// Tool name prefixes the agent is allowed to use
    pub allowed_tools: Vec<String>,
    /// Combined skill context for the agent system prompt
    pub skill_context: String,
}

#[derive(Clone)]
pub struct SandboxOrchestrator {
    docker: Docker,
}

#[derive(Debug, Clone)]
pub struct SandboxHandle {
    pub container_id: String,
    pub task_id: TaskId,
}

impl SandboxOrchestrator {
    pub fn new() -> anyhow::Result<Self> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(Self { docker })
    }

    pub async fn create_container(
        &self,
        task_id: &TaskId,
        agent_id: &str,
        image: &str,
        sandbox_config: &SandboxConfig,
        secrets: &HashMap<String, String>,
    ) -> anyhow::Result<SandboxHandle> {
        let container_name = format!("mcclawd-{}-{}", agent_id, &task_id.0[..8]);

        let env: Vec<String> = vec![
            format!("MCCLAWD_AGENT_ID={agent_id}"),
            format!("MCCLAWD_TASK_ID={}", task_id),
            format!("MCCLAWD_MCP_URL={}", sandbox_config.agentgateway_url),
        ];

        let mut mounts = vec![Mount {
            target: Some("/workspace".to_string()),
            source: Some(sandbox_config.workspace_dir.clone()),
            typ: Some(MountTypeEnum::BIND),
            read_only: Some(false),
            ..Default::default()
        }];

        if !secrets.is_empty() {
            mounts.push(Mount {
                target: Some("/run/secrets".to_string()),
                typ: Some(MountTypeEnum::TMPFS),
                ..Default::default()
            });
        }

        let host_config = HostConfig {
            mounts: Some(mounts),
            memory: sandbox_config.memory_limit,
            nano_cpus: sandbox_config.cpu_limit,
            network_mode: Some(sandbox_config.network.clone()),
            ..Default::default()
        };

        let config = Config {
            image: Some(image.to_string()),
            env: Some(env),
            host_config: Some(host_config),
            working_dir: Some("/workspace".to_string()),
            ..Default::default()
        };

        let opts = CreateContainerOptions {
            name: container_name,
            platform: None,
        };

        let response = self.docker.create_container(Some(opts), config).await?;

        self.docker
            .start_container::<String>(&response.id, None)
            .await?;

        for (key, value) in secrets {
            self.write_secret_file(&response.id, key, value).await?;
        }

        Ok(SandboxHandle {
            container_id: response.id,
            task_id: task_id.clone(),
        })
    }

    async fn write_secret_file(
        &self,
        container_id: &str,
        key: &str,
        value: &str,
    ) -> anyhow::Result<()> {
        let cmd_str = format!("printf '%s' \"$SECRET_VALUE\" > /run/secrets/{key}");
        let env_str = format!("SECRET_VALUE={value}");

        let exec = self
            .docker
            .create_exec(
                container_id,
                CreateExecOptions {
                    cmd: Some(vec!["sh".to_string(), "-c".to_string(), cmd_str]),
                    env: Some(vec![env_str]),
                    ..Default::default()
                },
            )
            .await?;

        self.docker
            .start_exec(
                &exec.id,
                Some(StartExecOptions {
                    detach: true,
                    ..Default::default()
                }),
            )
            .await?;
        Ok(())
    }

    pub async fn stream_logs(
        &self,
        container_id: &str,
        tx: mpsc::Sender<String>,
    ) -> anyhow::Result<()> {
        let opts = LogsOptions::<String> {
            follow: true,
            stdout: true,
            stderr: true,
            ..Default::default()
        };

        let mut stream = self.docker.logs(container_id, Some(opts));

        while let Some(result) = stream.next().await {
            match result {
                Ok(output) => {
                    if tx.send(output.to_string()).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    tracing::warn!("log stream error: {e}");
                    break;
                }
            }
        }
        Ok(())
    }

    pub async fn wait_container(&self, container_id: &str) -> anyhow::Result<i64> {
        let opts = WaitContainerOptions {
            condition: "not-running",
        };
        let mut stream = self.docker.wait_container(container_id, Some(opts));

        if let Some(result) = stream.next().await {
            let response = result?;
            Ok(response.status_code)
        } else {
            anyhow::bail!("wait stream ended without result")
        }
    }

    pub async fn cleanup_container(&self, container_id: &str) -> anyhow::Result<()> {
        let _ = self
            .docker
            .stop_container(container_id, Some(StopContainerOptions { t: 5 }))
            .await;
        self.docker
            .remove_container(
                container_id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await?;
        Ok(())
    }

    pub async fn health_check(&self) -> bool {
        self.docker.ping().await.is_ok()
    }

    /// High-level method: run an agent task inside a Docker container.
    ///
    /// Orchestrates the full lifecycle: create container → start → stream logs → wait → cleanup.
    /// Returns a stream of log lines via the provided mpsc sender.
    /// The container runs the `mc run` binary with the given prompt.
    pub async fn run_agent_task(
        &self,
        task_id: &TaskId,
        image: &str,
        prompt: &str,
        sandbox_config: &SandboxConfig,
        secrets: &HashMap<String, String>,
        log_tx: mpsc::Sender<String>,
    ) -> anyhow::Result<i64> {
        // Create and start container
        let handle = self
            .create_container(task_id, "agent", image, sandbox_config, secrets)
            .await?;

        tracing::info!(
            task_id = %task_id,
            container_id = %handle.container_id,
            "Sandbox container started for agent task"
        );

        // Stream logs in background
        let docker = self.docker.clone();
        let cid = handle.container_id.clone();
        let log_handle = tokio::spawn(async move {
            let orch = SandboxOrchestrator { docker };
            if let Err(e) = orch.stream_logs(&cid, log_tx).await {
                tracing::warn!(error = %e, "Log streaming ended with error");
            }
        });

        // Wait for container to finish
        let exit_code = self.wait_container(&handle.container_id).await?;

        // Wait for logs to flush
        let _ = log_handle.await;

        // Cleanup
        if let Err(e) = self.cleanup_container(&handle.container_id).await {
            tracing::warn!(
                container_id = %handle.container_id,
                error = %e,
                "Failed to cleanup sandbox container"
            );
        }

        tracing::info!(
            task_id = %task_id,
            exit_code = exit_code,
            "Sandbox agent task completed"
        );

        Ok(exit_code)
    }

    /// Create an agent container using a fully-resolved `AgentEnvironment`.
    ///
    /// This is the McpPorter-aware path: image, network, gateway URL, and
    /// allowed tools all come from the resolved environment.
    pub async fn create_container_from_env(
        &self,
        task_id: &TaskId,
        agent_id: &str,
        agent_env: &AgentEnvironment,
        workspace_dir: &str,
        sandbox_config: &SandboxConfig,
        secrets: &HashMap<String, String>,
    ) -> anyhow::Result<SandboxHandle> {
        let container_name = format!("mcclawd-{}-{}", agent_id, &task_id.0[..8]);

        let allowed_tools_str = agent_env.allowed_tools.join(",");
        let env: Vec<String> = vec![
            format!("MCCLAWD_AGENT_ID={agent_id}"),
            format!("MCCLAWD_TASK_ID={}", task_id),
            format!("MCCLAWD_GATEWAY_URL={}", agent_env.gateway_url),
            format!("MCCLAWD_ALLOWED_TOOLS={allowed_tools_str}"),
        ];

        let mut mounts = vec![Mount {
            target: Some("/workspace".to_string()),
            source: Some(workspace_dir.to_string()),
            typ: Some(MountTypeEnum::BIND),
            read_only: Some(false),
            ..Default::default()
        }];

        if !secrets.is_empty() {
            mounts.push(Mount {
                target: Some("/run/secrets".to_string()),
                typ: Some(MountTypeEnum::TMPFS),
                ..Default::default()
            });
        }

        let host_config = HostConfig {
            mounts: Some(mounts),
            memory: sandbox_config.memory_limit,
            nano_cpus: sandbox_config.cpu_limit,
            network_mode: Some(agent_env.network.clone()),
            ..Default::default()
        };

        let config = Config {
            image: Some(agent_env.image.clone()),
            env: Some(env),
            host_config: Some(host_config),
            working_dir: Some("/workspace".to_string()),
            ..Default::default()
        };

        let opts = CreateContainerOptions {
            name: container_name,
            platform: None,
        };

        let response = self.docker.create_container(Some(opts), config).await?;

        self.docker
            .start_container::<String>(&response.id, None)
            .await?;

        for (key, value) in secrets {
            self.write_secret_file(&response.id, key, value).await?;
        }

        Ok(SandboxHandle {
            container_id: response.id,
            task_id: task_id.clone(),
        })
    }

    /// Create a long-lived system agent container with auto-restart policy.
    pub async fn create_system_agent_container(
        &self,
        agent_env: &AgentEnvironment,
        workspace_dir: &str,
        sandbox_config: &SandboxConfig,
    ) -> anyhow::Result<SandboxHandle> {
        let task_id = TaskId("system-agent".to_string());
        let container_name = "mcclawd-system-agent".to_string();

        let allowed_tools_str = agent_env.allowed_tools.join(",");
        let env: Vec<String> = vec![
            "MCCLAWD_AGENT_TYPE=system".to_string(),
            format!("MCCLAWD_GATEWAY_URL={}", agent_env.gateway_url),
            format!("MCCLAWD_ALLOWED_TOOLS={allowed_tools_str}"),
        ];

        let mounts = vec![Mount {
            target: Some("/workspace".to_string()),
            source: Some(workspace_dir.to_string()),
            typ: Some(MountTypeEnum::BIND),
            read_only: Some(false),
            ..Default::default()
        }];

        let host_config = HostConfig {
            mounts: Some(mounts),
            memory: sandbox_config.memory_limit,
            nano_cpus: sandbox_config.cpu_limit,
            network_mode: Some(agent_env.network.clone()),
            restart_policy: Some(RestartPolicy {
                name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
                maximum_retry_count: Some(0),
            }),
            ..Default::default()
        };

        let config = Config {
            image: Some(agent_env.image.clone()),
            env: Some(env),
            host_config: Some(host_config),
            working_dir: Some("/workspace".to_string()),
            ..Default::default()
        };

        // Remove stale system agent container if it exists
        let _ = self
            .docker
            .remove_container(
                &container_name,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;

        let opts = CreateContainerOptions {
            name: container_name,
            platform: None,
        };

        let response = self.docker.create_container(Some(opts), config).await?;

        self.docker
            .start_container::<String>(&response.id, None)
            .await?;

        tracing::info!(
            container_id = %response.id,
            image = %agent_env.image,
            "System agent container started"
        );

        Ok(SandboxHandle {
            container_id: response.id,
            task_id,
        })
    }
}
