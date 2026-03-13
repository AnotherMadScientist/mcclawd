//! Sandbox orchestrator — manages Docker containers for agent execution.

use bollard::container::{
    AttachContainerOptions, Config, CreateContainerOptions, LogsOptions, RemoveContainerOptions,
    StopContainerOptions, WaitContainerOptions,
};
use bollard::exec::{CreateExecOptions, StartExecOptions};
use bollard::models::{HostConfig, Mount, MountTypeEnum, RestartPolicy, RestartPolicyNameEnum};
use bollard::Docker;
use futures::StreamExt;
use mcclawd_channels::OutboundChunk;
use mcclawd_core::skills::SandboxConfig;
use mcclawd_core::types::TaskId;
use std::collections::HashMap;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

/// Parse a Docker log line into an OutboundChunk.
/// Docker log lines may have an 8-byte binary header prefix for multiplexed streams.
/// We find the first JSON value and attempt to parse it as OutboundChunk.
///
/// Serde serializes enum variants as:
/// - Unit variants: `"Done"` (JSON string)
/// - Newtype variants: `{"TextDelta":"hello"}` (JSON object)
/// - Struct variants: `{"ToolStart":{"name":"web.search"}}` (JSON object)
pub fn parse_log_line(line: &str) -> Option<OutboundChunk> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Find first '{' (object variants) or '"' (unit variants like "Done")
    let obj_start = trimmed.find('{');
    let str_start = trimmed.find('"');

    let json_start = match (obj_start, str_start) {
        (Some(o), Some(s)) => Some(o.min(s)),
        (Some(o), None) => Some(o),
        (None, Some(s)) => Some(s),
        (None, None) => None,
    }?;

    serde_json::from_str(&trimmed[json_start..]).ok()
}

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
    /// LLM model identifier, e.g. "claude-haiku-4-5-20251001"
    pub model: String,
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

/// Handle to a long-lived container with stdin communication.
/// The container runs `agent-runner --server` and accepts JSON messages via stdin.
/// Responses come via docker logs (JSONL output), read by `stream_agent_output`.
#[derive(Clone)]
pub struct PersistentHandle {
    pub container_id: String,
    pub task_id: TaskId,
    /// Send JSON messages to container stdin (one JSON object per line).
    stdin_tx: tokio::sync::mpsc::Sender<String>,
    /// Tracks whether the background stdin writer is still alive.
    alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl PersistentHandle {
    /// Connect to a running container's stdin via attach.
    ///
    /// Attaches with both stdin AND stdout enabled. Stdout is drained by a background
    /// task to keep the HTTP upgrade connection alive — without an active reader,
    /// Docker/bollard closes the connection, silently breaking stdin writes.
    /// Actual output parsing is done separately via `stream_agent_output` (docker logs).
    pub async fn connect(
        docker: &Docker,
        container_id: String,
        task_id: TaskId,
    ) -> anyhow::Result<Self> {
        let results = docker
            .attach_container(
                &container_id,
                Some(AttachContainerOptions::<String> {
                    stdin: Some(true),
                    stdout: Some(true), // keep connection alive (drain output separately)
                    stderr: Some(true),
                    stream: Some(true),
                    ..Default::default()
                }),
            )
            .await?;

        let mut input = results.input;
        let mut output = results.output;
        let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<String>(32);
        let alive = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));

        // Background drain: consume attach output to keep the connection alive.
        // The actual output parsing uses docker.logs() in stream_agent_output().
        let drain_cid = container_id.clone();
        tokio::spawn(async move {
            while let Some(result) = output.next().await {
                match result {
                    Ok(_) => {} // discard — output handled by docker.logs()
                    Err(e) => {
                        tracing::debug!(
                            container_id = %drain_cid,
                            error = %e,
                            "Attach output drain ended"
                        );
                        break;
                    }
                }
            }
            tracing::debug!(container_id = %drain_cid, "Attach output drain exiting");
        });

        // Background writer: receives String messages, writes to container stdin
        let writer_alive = alive.clone();
        let writer_cid = container_id.clone();
        tokio::spawn(async move {
            while let Some(msg) = stdin_rx.recv().await {
                tracing::debug!(
                    container_id = %writer_cid,
                    msg_len = msg.len(),
                    "Writing to container stdin"
                );
                if let Err(e) = input.write_all(msg.as_bytes()).await {
                    tracing::warn!(
                        container_id = %writer_cid,
                        error = %e,
                        "Container stdin write error"
                    );
                    break;
                }
                if let Err(e) = input.write_all(b"\n").await {
                    tracing::warn!(
                        container_id = %writer_cid,
                        error = %e,
                        "Container stdin newline error"
                    );
                    break;
                }
                if let Err(e) = input.flush().await {
                    tracing::warn!(
                        container_id = %writer_cid,
                        error = %e,
                        "Container stdin flush error"
                    );
                    break;
                }
                tracing::debug!(container_id = %writer_cid, "Stdin write+flush succeeded");
            }
            writer_alive.store(false, std::sync::atomic::Ordering::SeqCst);
            tracing::warn!(container_id = %writer_cid, "Container stdin writer exiting — handle is now dead");
        });

        tracing::info!(
            container_id = %container_id,
            task_id = %task_id,
            "PersistentHandle connected (stdin + stdout drain)"
        );

        Ok(Self {
            container_id,
            task_id,
            stdin_tx,
            alive,
        })
    }

    /// Check if the background stdin writer is still alive.
    pub fn is_alive(&self) -> bool {
        self.alive.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Send a chat message to the container. Response comes via the log stream.
    pub async fn send_chat(
        &self,
        prompt: &str,
        history_json: Option<&str>,
    ) -> anyhow::Result<()> {
        if !self.is_alive() {
            anyhow::bail!(
                "Container stdin writer is dead (container_id={})",
                self.container_id
            );
        }
        let msg = serde_json::json!({
            "type": "chat",
            "prompt": prompt,
            "history_json": history_json,
        });
        tracing::info!(
            container_id = %self.container_id,
            prompt_len = prompt.len(),
            "Sending chat to container"
        );
        self.stdin_tx
            .send(serde_json::to_string(&msg)?)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send to container stdin: {e}"))?;
        Ok(())
    }

    /// Request graceful shutdown.
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        let msg = r#"{"type":"shutdown"}"#.to_string();
        let _ = self.stdin_tx.send(msg).await;
        Ok(())
    }
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
            security_opt: Some(vec!["no-new-privileges".to_string()]),
            pids_limit: sandbox_config.pids_limit,
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

    /// Write a JWT identity token to /run/identity/token inside the container.
    async fn write_identity_token(
        &self,
        container_id: &str,
        token: &str,
    ) -> anyhow::Result<()> {
        let cmd_str = "printf '%s' \"$TOKEN_VALUE\" > /run/identity/token".to_string();
        let env_str = format!("TOKEN_VALUE={token}");

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

        tracing::debug!(container_id, "Identity token written to /run/identity/token");
        Ok(())
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
        _prompt: &str,
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
            format!("MCCLAWD_MODEL={}", agent_env.model),
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
            security_opt: Some(vec!["no-new-privileges".to_string()]),
            pids_limit: sandbox_config.pids_limit,
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

    /// Stream agent output from a container, parsing JSONL stdout into OutboundChunk.
    /// Non-JSON lines (stderr, tracing) are forwarded to host tracing.
    /// Parsed chunks are sent via the mpsc sender for the caller to handle
    /// (e.g., broadcast to WebSocket clients + persist).
    pub async fn stream_agent_output(
        &self,
        container_id: &str,
        chunk_tx: mpsc::Sender<OutboundChunk>,
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
                    let line = output.to_string();
                    if let Some(chunk) = parse_log_line(&line) {
                        if chunk_tx.send(chunk).await.is_err() {
                            break; // receiver dropped
                        }
                    } else if !line.trim().is_empty() {
                        tracing::debug!(container_id, line = %line.trim(), "agent log");
                    }
                }
                Err(e) => {
                    tracing::warn!("agent log stream error: {e}");
                    break;
                }
            }
        }
        Ok(())
    }

    /// Create a runner container for executing agent tasks via the JSONL protocol.
    ///
    /// Unlike `create_container` which runs the `mc run` binary directly, this method
    /// creates a container running `mcclawd-runner` which outputs structured JSONL
    /// that the host parses via `stream_agent_output`.
    pub async fn create_runner_container(
        &self,
        task_id: &TaskId,
        prompt: &str,
        agent_env: &AgentEnvironment,
        workspace_dir: &str,
        sandbox_config: &SandboxConfig,
        secrets: &HashMap<String, String>,
        max_turns: usize,
        history_json: Option<&str>,
        agent_type: Option<&str>,
        attachments_dir: Option<&str>,
    ) -> anyhow::Result<SandboxHandle> {
        let container_name = format!("mcclawd-runner-{}", &task_id.0[..8]);

        let allowed_tools_str = agent_env.allowed_tools.join(",");
        let mut env: Vec<String> = vec![
            format!("MCCLAWD_TASK_ID={}", task_id),
            format!("MCCLAWD_GATEWAY_URL={}", agent_env.gateway_url),
            format!("MCCLAWD_ALLOWED_TOOLS={allowed_tools_str}"),
            format!("MCCLAWD_MAX_TURNS={max_turns}"),
            format!("MCCLAWD_MODEL={}", agent_env.model),
        ];

        if let Some(at) = agent_type {
            env.push(format!("MCCLAWD_AGENT_TYPE={at}"));
        }

        // Pass skill context so the runner knows which skills to load
        if !agent_env.skill_context.is_empty() {
            env.push(format!("MCCLAWD_SKILL_CONTEXT={}", agent_env.skill_context));
        }

        // For short prompts, pass as env var; for large prompts, mount as file
        let prompt_as_file = prompt.len() > 32_768;
        if !prompt_as_file {
            env.push(format!("MCCLAWD_PROMPT={prompt}"));
        }

        let mut mounts = vec![Mount {
            target: Some("/workspace".to_string()),
            source: Some(workspace_dir.to_string()),
            typ: Some(MountTypeEnum::BIND),
            read_only: Some(false),
            ..Default::default()
        }];

        if !secrets.is_empty() || prompt_as_file || history_json.is_some() {
            mounts.push(Mount {
                target: Some("/run/secrets".to_string()),
                typ: Some(MountTypeEnum::TMPFS),
                ..Default::default()
            });
        }

        // Mount attachments directory (read-only) if provided
        if let Some(att_dir) = attachments_dir {
            mounts.push(Mount {
                target: Some("/attachments".to_string()),
                source: Some(att_dir.to_string()),
                typ: Some(MountTypeEnum::BIND),
                read_only: Some(true),
                ..Default::default()
            });
        }

        let host_config = HostConfig {
            mounts: Some(mounts),
            memory: sandbox_config.memory_limit,
            nano_cpus: sandbox_config.cpu_limit,
            network_mode: Some(agent_env.network.clone()),
            security_opt: Some(vec!["no-new-privileges".to_string()]),
            pids_limit: sandbox_config.pids_limit,
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

        // Write secrets into tmpfs
        for (key, value) in secrets {
            self.write_secret_file(&response.id, key, value).await?;
        }

        // Write large prompt as file
        if prompt_as_file {
            self.write_secret_file(&response.id, "prompt.txt", prompt)
                .await?;
            // Set env var pointing to the file
            let exec = self
                .docker
                .create_exec(
                    &response.id,
                    CreateExecOptions {
                        cmd: Some(vec![
                            "sh".to_string(),
                            "-c".to_string(),
                            "echo /run/secrets/prompt.txt > /run/secrets/prompt_path".to_string(),
                        ]),
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
        }

        // Write conversation history if provided
        if let Some(history) = history_json {
            self.write_secret_file(&response.id, "history.json", history)
                .await?;
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
            format!("MCCLAWD_MODEL={}", agent_env.model),
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
            security_opt: Some(vec!["no-new-privileges".to_string()]),
            pids_limit: sandbox_config.pids_limit,
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

    /// Create a long-lived runner container with --server mode and stdin support.
    /// The container stays running, accepting JSON messages via stdin.
    /// Responses are emitted as JSONL on stdout (read via `stream_agent_output`).
    pub async fn create_persistent_runner_container(
        &self,
        task_id: &TaskId,
        agent_env: &AgentEnvironment,
        workspace_dir: &str,
        sandbox_config: &SandboxConfig,
        secrets: &HashMap<String, String>,
        max_turns: usize,
        agent_type: Option<&str>,
        attachments_dir: Option<&str>,
    ) -> anyhow::Result<PersistentHandle> {
        let container_name = format!(
            "mcclawd-persistent-{}",
            &task_id.0[..std::cmp::min(task_id.0.len(), 12)]
        );

        let allowed_tools_str = agent_env.allowed_tools.join(",");
        let mut env: Vec<String> = vec![
            format!("MCCLAWD_TASK_ID={}", task_id),
            format!("MCCLAWD_GATEWAY_URL={}", agent_env.gateway_url),
            format!("MCCLAWD_ALLOWED_TOOLS={allowed_tools_str}"),
            format!("MCCLAWD_MAX_TURNS={max_turns}"),
            format!("MCCLAWD_MODEL={}", agent_env.model),
        ];

        if let Some(at) = agent_type {
            env.push(format!("MCCLAWD_AGENT_TYPE={at}"));
        }

        // Pass skill context so the runner knows which skills to load
        if !agent_env.skill_context.is_empty() {
            env.push(format!("MCCLAWD_SKILL_CONTEXT={}", agent_env.skill_context));
        }

        // Identity token path for agent-to-host JWT authentication
        env.push("MCCLAWD_IDENTITY_TOKEN_PATH=/run/identity/token".to_string());

        let mut mounts = vec![Mount {
            target: Some("/workspace".to_string()),
            source: Some(workspace_dir.to_string()),
            typ: Some(MountTypeEnum::BIND),
            read_only: Some(false),
            ..Default::default()
        }];

        // Always need tmpfs for secrets
        mounts.push(Mount {
            target: Some("/run/secrets".to_string()),
            typ: Some(MountTypeEnum::TMPFS),
            ..Default::default()
        });

        // Identity token mount — JWT for agent-to-host authentication
        mounts.push(Mount {
            target: Some("/run/identity".to_string()),
            typ: Some(MountTypeEnum::TMPFS),
            ..Default::default()
        });

        if let Some(att_dir) = attachments_dir {
            mounts.push(Mount {
                target: Some("/attachments".to_string()),
                source: Some(att_dir.to_string()),
                typ: Some(MountTypeEnum::BIND),
                read_only: Some(true),
                ..Default::default()
            });
        }

        let host_config = HostConfig {
            mounts: Some(mounts),
            memory: sandbox_config.memory_limit,
            nano_cpus: sandbox_config.cpu_limit,
            network_mode: Some(agent_env.network.clone()),
            restart_policy: Some(RestartPolicy {
                name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
                maximum_retry_count: Some(0),
            }),
            security_opt: Some(vec!["no-new-privileges".to_string()]),
            pids_limit: sandbox_config.pids_limit,
            ..Default::default()
        };

        let config = Config {
            image: Some(agent_env.image.clone()),
            env: Some(env),
            host_config: Some(host_config),
            working_dir: Some("/workspace".to_string()),
            open_stdin: Some(true),
            stdin_once: Some(false),
            attach_stdin: Some(true),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            cmd: Some(vec!["--server".to_string()]),
            ..Default::default()
        };

        // Remove stale container with same name if it exists
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

        // Write secrets into tmpfs (skip internal keys)
        for (key, value) in secrets {
            if key.starts_with("__") {
                continue;
            }
            self.write_secret_file(&response.id, key, value).await?;
        }

        // Write identity token to /run/identity/token if provided
        if let Some(token) = secrets.get("__identity_token") {
            self.write_identity_token(&response.id, token).await?;
        }

        tracing::info!(
            container_id = %response.id,
            task_id = %task_id,
            "Persistent runner container started (--server mode)"
        );

        // Connect to container stdin
        let handle =
            PersistentHandle::connect(&self.docker, response.id, task_id.clone()).await?;

        Ok(handle)
    }
}

/// Convert a host-facing gateway URL to one accessible from inside Docker containers.
/// Replaces `localhost` and `127.0.0.1` with Docker DNS name `agentgateway`.
pub fn container_gateway_url(host_url: &str) -> String {
    host_url
        .replace("localhost", "agentgateway")
        .replace("127.0.0.1", "agentgateway")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_text_delta() {
        let line = r#"{"TextDelta":"hello"}"#;
        let chunk = parse_log_line(line).unwrap();
        assert!(matches!(chunk, OutboundChunk::TextDelta(t) if t == "hello"));
    }

    #[test]
    fn parse_with_prefix() {
        // Docker binary log prefix followed by valid JSONL
        let line = format!(
            "\x01\x00\x00\x00\x00\x00\x00\x0f{}",
            r#"{"TextDelta":"hi"}"#
        );
        let chunk = parse_log_line(&line).unwrap();
        assert!(matches!(chunk, OutboundChunk::TextDelta(t) if t == "hi"));
    }

    #[test]
    fn parse_done_variant() {
        // Done serializes as "Done" (unit variant = JSON string)
        let line = r#""Done""#;
        let chunk = parse_log_line(line).unwrap();
        assert!(matches!(chunk, OutboundChunk::Done));
    }

    #[test]
    fn parse_done_with_docker_prefix() {
        // Docker binary prefix + unit variant
        let line = format!("\x01\x00\x00\x00\x00\x00\x00\x06{}", r#""Done""#);
        let chunk = parse_log_line(&line).unwrap();
        assert!(matches!(chunk, OutboundChunk::Done));
    }

    #[test]
    fn parse_empty_line() {
        assert!(parse_log_line("").is_none());
        assert!(parse_log_line("   ").is_none());
    }

    #[test]
    fn parse_non_json() {
        assert!(parse_log_line("just some text").is_none());
    }

    #[test]
    fn parse_non_outbound_json() {
        let line = r#"{"timestamp":"2026-03-06","level":"INFO"}"#;
        assert!(parse_log_line(line).is_none());
    }

    #[test]
    fn parse_tool_start() {
        let line = r#"{"ToolStart":{"name":"web.search"}}"#;
        let chunk = parse_log_line(line).unwrap();
        assert!(matches!(chunk, OutboundChunk::ToolStart { name } if name == "web.search"));
    }

    #[test]
    fn parse_usage() {
        let line = r#"{"Usage":{"input_tokens":100,"output_tokens":50,"total_tokens":150,"model":"claude-sonnet-4-5"}}"#;
        let chunk = parse_log_line(line).unwrap();
        match chunk {
            OutboundChunk::Usage { input_tokens, output_tokens, total_tokens, model } => {
                assert_eq!(input_tokens, 100);
                assert_eq!(output_tokens, 50);
                assert_eq!(total_tokens, 150);
                assert_eq!(model.as_deref(), Some("claude-sonnet-4-5"));
            }
            other => panic!("Expected Usage, got {other:?}"),
        }
    }

    #[test]
    fn parse_chat_history() {
        let line = r#"{"ChatHistory":"[]"}"#;
        let chunk = parse_log_line(line).unwrap();
        assert!(matches!(chunk, OutboundChunk::ChatHistory(h) if h == "[]"));
    }

    #[test]
    fn container_gateway_url_replaces_localhost() {
        assert_eq!(
            container_gateway_url("http://localhost:3000"),
            "http://agentgateway:3000"
        );
    }

    #[test]
    fn container_gateway_url_replaces_127() {
        assert_eq!(
            container_gateway_url("http://127.0.0.1:3000"),
            "http://agentgateway:3000"
        );
    }

    #[test]
    fn container_gateway_url_preserves_custom_host() {
        assert_eq!(
            container_gateway_url("http://custom-host:5000"),
            "http://custom-host:5000"
        );
    }

    #[test]
    fn parse_generated_files() {
        let line = r#"{"GeneratedFiles":[{"name":"report.pdf","size":1024,"content_type":"application/pdf","url":"/api/tasks/t1/files/report.pdf"}]}"#;
        let chunk = parse_log_line(line).unwrap();
        match chunk {
            OutboundChunk::GeneratedFiles(files) => {
                assert_eq!(files.len(), 1);
                assert_eq!(files[0].name, "report.pdf");
                assert_eq!(files[0].size, 1024);
            }
            other => panic!("Expected GeneratedFiles, got {other:?}"),
        }
    }
}
