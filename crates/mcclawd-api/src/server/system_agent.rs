//! System agent — persistent always-on agent for UI control and app management.
//!
//! Endpoints:
//! - `POST /api/system-agent/chat` — send a message, stream response via existing WS
//! - `GET /api/system-agent/history` — get conversation history
//! - `DELETE /api/system-agent/history` — clear conversation history

use axum::{extract::State, http::StatusCode, Json};
use futures::StreamExt;
use mcclawd_agent::context::ContextBuilder;
use mcclawd_agent::workspace::WorkspaceLoader;
use mcclawd_channels::{ChannelStatus, OutboundChunk};
use mcclawd_core::types::TaskId;
use rig::agent::MultiTurnStreamItem;
use rig::completion::message::Message as RigMessage;
use rig::completion::message::ToolResultContent;
use rig::streaming::{StreamedAssistantContent, StreamedUserContent, StreamingChat};
use serde::{Deserialize, Serialize};

use super::state::AppState;

/// The fixed task ID used for the system agent session.
pub const SYSTEM_AGENT_TASK_ID: &str = "__system__";

// Uses the same model as the main agent engine.

#[derive(Debug, Deserialize)]
pub struct SystemChatRequest {
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct SystemChatResponse {
    pub task_id: String,
}

/// POST /api/system-agent/chat — send a message to the system agent
pub async fn chat(
    State(state): State<AppState>,
    Json(body): Json<SystemChatRequest>,
) -> Result<Json<SystemChatResponse>, StatusCode> {
    let sanitized = mcclawd_core::sanitize_prompt(&body.message);
    if sanitized.was_modified {
        tracing::warn!(
            patterns = ?sanitized.detected_patterns,
            "Prompt injection patterns detected in system agent message"
        );
    }
    let message = sanitized.text;
    let task_id = TaskId(SYSTEM_AGENT_TASK_ID.to_string());

    // Get or create broadcast channel
    let tx = {
        let streams = state.task_streams.read().await;
        streams.get(&task_id).cloned()
    };
    let tx = match tx {
        Some(tx) => tx,
        None => state.create_task_stream(&task_id).await,
    };

    // Spawn agent execution
    let state_clone = state.clone();
    let tid = task_id.clone();
    tokio::spawn(async move {
        run_system_agent(state_clone, tid, &message, tx).await;
    });

    Ok(Json(SystemChatResponse {
        task_id: SYSTEM_AGENT_TASK_ID.to_string(),
    }))
}

/// GET /api/system-agent/history — get system agent conversation history
pub async fn history(State(state): State<AppState>) -> Json<Vec<serde_json::Value>> {
    let task_id = TaskId(SYSTEM_AGENT_TASK_ID.to_string());
    let events = state.get_task_events(&task_id).await;
    let serialized: Vec<serde_json::Value> = events
        .iter()
        .filter_map(|e| serde_json::to_value(e).ok())
        .collect();
    Json(serialized)
}

/// DELETE /api/system-agent/history — clear system agent history
pub async fn clear_history(State(state): State<AppState>) -> StatusCode {
    let task_id = TaskId(SYSTEM_AGENT_TASK_ID.to_string());

    // Clear event history
    {
        let mut events = state.task_events.write().await;
        events.remove(&task_id);
    }
    // Clear chat history (in-memory + DB)
    {
        let mut history = state.task_chat_history.write().await;
        history.remove(&task_id);
    }
    {
        let store = state.pg_store.clone();
        let tid = task_id.0.clone();
        tokio::spawn(async move {
            let _ = store.set_chat_history(&tid, &[]).await;
        });
    }

    StatusCode::NO_CONTENT
}

/// Build system prompt for the system agent.
fn build_system_prompt(workspace_prompt: &str) -> String {
    format!(
        r#"{workspace_prompt}

---

## System Agent Instructions

You are McClawd's system agent — a minimal UI controller. You have exactly 2 tools:

1. `navigate_to` — navigate the app to a page
2. `create_task` — create a new agent task

**RULES (strict):**
- ALWAYS call a tool. Never respond with just text.
- You CANNOT install skills, manage secrets, edit workspace files, or run code.
- If the user asks for something outside your 2 tools, call `navigate_to` to take them to the right page.

**Pages for navigate_to:**
- `/` — task list (home)
- `/tasks/new` — new task form
- `/workspace` — workspace editor
- `/config` — settings
- `/config/skills` — skills browser
- `/config/secrets` — secrets management
- `/config/mcp` — MCP servers
- `/config/docker` — agent containers
- `/config/usage` — usage & spending

**Response style:** Call the tool, then confirm in 1 short sentence. Example: "Navigated to skills." or "Created task.""#
    )
}

/// Run the system agent with streaming output.
///
/// Always runs in-process (host mode) so it has access to the UI action tools
/// (navigate_to, create_task, install_skill, etc.). The system agent is a
/// lightweight UI controller — it doesn't need Docker sandboxing.
async fn run_system_agent(
    state: AppState,
    task_id: TaskId,
    prompt: &str,
    tx: tokio::sync::broadcast::Sender<OutboundChunk>,
) {
    tracing::info!("Running system agent in-process (host mode)");
    run_system_agent_host(state, task_id, prompt, tx).await;
}

/// Ensure the system agent persistent container is running and return its handle.
/// If already running, returns the existing handle. Otherwise creates a new persistent
/// container with --server mode, starts a background output reader + forwarder,
/// and stores the handle in AppState.
pub async fn ensure_system_agent_container(
    state: &AppState,
) -> anyhow::Result<crate::sandbox::PersistentHandle> {
    // Check if already running and healthy
    {
        let guard = state.system_agent.read().await;
        if let Some(handle) = guard.as_ref() {
            if handle.is_alive() {
                tracing::debug!(
                    container_id = %handle.container_id,
                    "Reusing healthy system agent handle"
                );
                return Ok(handle.clone());
            }
            tracing::warn!(
                container_id = %handle.container_id,
                "System agent handle is dead — will recreate"
            );
        }
    }
    // Clear dead handle before recreating
    *state.system_agent.write().await = None;

    let config = state.config.read().await.clone();

    // Get API key
    let api_key = {
        let secrets_guard = state.secrets.read().await;
        match secrets_guard.as_ref() {
            Some(backend) => backend
                .get("ANTHROPIC_API_KEY")
                .await
                .map_err(|e| anyhow::anyhow!("Failed to read secrets: {e}"))?
                .ok_or_else(|| anyhow::anyhow!("ANTHROPIC_API_KEY not found"))?,
            None => anyhow::bail!("Secrets vault not unlocked"),
        }
    };

    let mut secrets = std::collections::HashMap::new();
    secrets.insert("ANTHROPIC_API_KEY".to_string(), api_key);

    // Use McpPorter for correct Docker-internal gateway URL (http://agentgateway:3000).
    // CRITICAL: System agent gets base environment ONLY — no skills, no MCP tool injection.
    // Skills and MCP tools are exclusively for task agents.
    let agent_env = if let Some(ref porter) = state.mcp_porter {
        match porter.prepare_base_environment(&config).await {
            Ok(mut env) => {
                env.image = "mcclawd-runner:latest".to_string();
                env.model = config.agent.model.clone();
                tracing::info!(
                    gateway_url = %env.gateway_url,
                    "McpPorter resolved system agent base environment (no skills/MCP tools)"
                );
                env
            }
            Err(e) => {
                tracing::warn!("McpPorter failed for system agent, using fallback: {e}");
                crate::sandbox::AgentEnvironment {
                    image: "mcclawd-runner:latest".to_string(),
                    network: config.sandbox.network.clone(),
                    gateway_url: crate::sandbox::container::container_gateway_url(
                        &config.mcp.agentgateway_url,
                    ),
                    allowed_tools: vec![],
                    skill_context: String::new(),
                    model: config.agent.model.clone(),
                }
            }
        }
    } else {
        crate::sandbox::AgentEnvironment {
            image: "mcclawd-runner:latest".to_string(),
            network: config.sandbox.network.clone(),
            gateway_url: crate::sandbox::container::container_gateway_url(
                &config.mcp.agentgateway_url,
            ),
            allowed_tools: vec![],
            skill_context: String::new(),
            model: config.agent.model.clone(),
        }
    };

    let workspace_dir = config
        .workspaces_dir()
        .join("default")
        .to_string_lossy()
        .to_string();
    let sandbox_cfg = mcclawd_core::skills::SandboxConfig {
        workspace_dir: workspace_dir.clone(),
        agentgateway_url: config.mcp.agentgateway_url.clone(),
        memory_limit: config.sandbox.memory_limit,
        cpu_limit: config.sandbox.cpu_limit,
        network: config.sandbox.network.clone(),
        pids_limit: config.sandbox.pids_limit,
        ..Default::default()
    };

    let orch = crate::sandbox::SandboxOrchestrator::new()?;

    let task_id = TaskId(SYSTEM_AGENT_TASK_ID.to_string());
    let handle = orch
        .create_persistent_runner_container(
            &task_id,
            &agent_env,
            &workspace_dir,
            &sandbox_cfg,
            &secrets,
            config.agent.max_turns,
            Some("system"),
            None,
        )
        .await?;

    // Start background output reader → forwarder
    let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::channel::<OutboundChunk>(256);

    let reader_cid = handle.container_id.clone();
    tokio::spawn(async move {
        let reader_orch = crate::sandbox::SandboxOrchestrator::new().unwrap();
        if let Err(e) = reader_orch.stream_agent_output(&reader_cid, chunk_tx).await {
            tracing::warn!(error = %e, "System agent output reader ended");
        }
    });

    // Background forwarder: routes chunks to broadcast channel
    let fwd_state = state.clone();
    let fwd_task_id = task_id.clone();
    tokio::spawn(async move {
        while let Some(chunk) = chunk_rx.recv().await {
            // Look up current broadcast channel (may change on reconnect)
            let tx = {
                let streams = fwd_state.task_streams.read().await;
                streams.get(&fwd_task_id).cloned()
            };
            if let Some(tx) = tx {
                match &chunk {
                    OutboundChunk::Usage { .. } | OutboundChunk::GeneratedFiles(_) => {
                        // System agent: consume silently (no usage tracking needed)
                    }
                    OutboundChunk::ChatHistory(json) => {
                        // Persist chat history for multi-turn
                        if let Ok(messages) =
                            serde_json::from_str::<Vec<rig::completion::message::Message>>(json)
                        {
                            fwd_state.set_chat_history(&fwd_task_id, messages).await;
                        }
                    }
                    _ => {
                        fwd_state
                            .send_and_persist(&fwd_task_id, &tx, chunk)
                            .await;
                    }
                }
            }
        }
        tracing::info!("System agent forwarder exiting");
    });

    // Brief delay for the runner to initialize in --server mode
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Store handle
    *state.system_agent.write().await = Some(handle.clone());

    // Persist to Postgres for reconnection on restart
    {
        let store = state.pg_store.clone();
        let cid = handle.container_id.clone();
        let wdir = workspace_dir.clone();
        tokio::spawn(async move {
            if let Err(e) = store
                .save_persistent_container(&cid, "system-agent", "system", &wdir)
                .await
            {
                tracing::warn!("Failed to persist system agent container record: {e}");
            }
        });
    }

    tracing::info!(
        container_id = %handle.container_id,
        "System agent persistent container ready"
    );
    Ok(handle)
}

/// Run the system agent inside a Docker sandbox container (persistent).
/// The container stays running across messages — only created on first use.
async fn run_system_agent_sandboxed(
    state: AppState,
    task_id: TaskId,
    prompt: &str,
    tx: tokio::sync::broadcast::Sender<OutboundChunk>,
) {
    // Persist user message
    state
        .send_and_persist(
            &task_id,
            &tx,
            OutboundChunk::UserMessage(prompt.to_string()),
        )
        .await;
    let _ = tx.send(OutboundChunk::StatusIndicator(ChannelStatus::Processing));

    // Get or create persistent container
    let handle = match ensure_system_agent_container(&state).await {
        Ok(h) => h,
        Err(e) => {
            let msg = format!("Failed to start system agent container: {e}");
            state
                .send_and_persist(&task_id, &tx, OutboundChunk::Error(msg))
                .await;
            state
                .send_and_persist(&task_id, &tx, OutboundChunk::Done)
                .await;
            return;
        }
    };

    // Get chat history for multi-turn
    let chat_history = state.get_chat_history(&task_id).await;
    let history_json = if !chat_history.is_empty() {
        serde_json::to_string(&chat_history).ok()
    } else {
        None
    };

    // Send message to persistent container via stdin
    if let Err(e) = handle.send_chat(prompt, history_json.as_deref()).await {
        let msg = format!("Failed to send message to system agent: {e}");
        state
            .send_and_persist(&task_id, &tx, OutboundChunk::Error(msg))
            .await;
        state
            .send_and_persist(&task_id, &tx, OutboundChunk::Done)
            .await;
        // Container might be dead — clear handle so next message recreates it
        *state.system_agent.write().await = None;
        return;
    }

    // Response chunks are handled by the background forwarder (started in ensure_system_agent_container).
    // No need to wait for container exit — it stays running!
}

/// Run the system agent in-process (host mode fallback).
async fn run_system_agent_host(
    state: AppState,
    task_id: TaskId,
    prompt: &str,
    tx: tokio::sync::broadcast::Sender<OutboundChunk>,
) {
    // Persist user message
    state
        .send_and_persist(
            &task_id,
            &tx,
            OutboundChunk::UserMessage(prompt.to_string()),
        )
        .await;

    // Load workspace for context
    let config = state.config.read().await.clone();
    let loader = WorkspaceLoader::new(config.workspaces_dir());
    let workspace_prompt = match loader.load("default") {
        Ok(ws) => ContextBuilder::new(ws).build_system_prompt(),
        Err(_) => "You are McClawd, an AI assistant.".to_string(),
    };

    // Get API key
    let api_key = {
        let secrets_guard = state.secrets.read().await;
        match secrets_guard.as_ref() {
            Some(backend) => match backend.get("ANTHROPIC_API_KEY").await {
                Ok(Some(key)) => key,
                Ok(None) => {
                    let msg =
                        "ANTHROPIC_API_KEY not found. Add it via Config > Secrets.".to_string();
                    state
                        .send_and_persist(&task_id, &tx, OutboundChunk::Error(msg))
                        .await;
                    state
                        .send_and_persist(&task_id, &tx, OutboundChunk::Done)
                        .await;
                    return;
                }
                Err(e) => {
                    let msg = format!("Failed to read secrets: {e}");
                    state
                        .send_and_persist(&task_id, &tx, OutboundChunk::Error(msg))
                        .await;
                    state
                        .send_and_persist(&task_id, &tx, OutboundChunk::Done)
                        .await;
                    return;
                }
            },
            None => {
                let msg = "Secrets vault not unlocked. Please log out and log in again.".to_string();
                state
                    .send_and_persist(&task_id, &tx, OutboundChunk::Error(msg))
                    .await;
                state
                    .send_and_persist(&task_id, &tx, OutboundChunk::Done)
                    .await;
                return;
            }
        }
    };

    // Build agent with system tools
    let system_prompt = build_system_prompt(&workspace_prompt);
    let agent =
        match mcclawd_agent::engine::AgentEngine::build_system_agent(&api_key, &system_prompt, &config.agent.model)
            .await
        {
            Ok(a) => a,
            Err(e) => {
                let msg = format!("Failed to build system agent: {e}");
                state
                    .send_and_persist(&task_id, &tx, OutboundChunk::Error(msg))
                    .await;
                state
                    .send_and_persist(&task_id, &tx, OutboundChunk::Done)
                    .await;
                return;
            }
        };

    // Stream with conversation history
    let _ = tx.send(OutboundChunk::StatusIndicator(ChannelStatus::Processing));

    let chat_history = state.get_chat_history(&task_id).await;
    let mut stream = agent.stream_chat(prompt, chat_history.clone()).await;
    let mut accumulated_text = String::new();
    let mut last_tool_name = String::new();

    while let Some(item) = stream.next().await {
        match item {
            Ok(MultiTurnStreamItem::StreamAssistantItem(content)) => match content {
                StreamedAssistantContent::Text(text) => {
                    let _ = tx.send(OutboundChunk::TextDelta(text.text.clone()));
                    accumulated_text.push_str(&text.text);
                }
                StreamedAssistantContent::ToolCall { tool_call, .. } => {
                    last_tool_name = tool_call.function.name.clone();
                    state
                        .send_and_persist(
                            &task_id,
                            &tx,
                            OutboundChunk::ToolStart {
                                name: tool_call.function.name.clone(),
                            },
                        )
                        .await;
                }
                _ => {}
            },
            Ok(MultiTurnStreamItem::StreamUserItem(StreamedUserContent::ToolResult { tool_result, .. })) => {
                // Extract text from tool result and send as ToolEnd
                let summary = match tool_result.content.first() {
                    ToolResultContent::Text(t) => t.text.clone(),
                    _ => String::new(),
                };
                let tool_name = if last_tool_name.is_empty() {
                    "unknown".to_string()
                } else {
                    last_tool_name.clone()
                };
                state
                    .send_and_persist(
                        &task_id,
                        &tx,
                        OutboundChunk::ToolEnd {
                            name: tool_name,
                            summary: Some(summary),
                        },
                    )
                    .await;
            }
            Ok(MultiTurnStreamItem::FinalResponse(final_resp)) => {
                if !accumulated_text.is_empty() {
                    state
                        .persist_only(
                            &task_id,
                            OutboundChunk::TextBlock(accumulated_text.clone()),
                        )
                        .await;
                }

                // Persist conversation history
                if let Some(history) = final_resp.history() {
                    state.set_chat_history(&task_id, history.to_vec()).await;
                } else {
                    let mut history = chat_history.clone();
                    history.push(RigMessage::user(prompt));
                    history.push(RigMessage::assistant(&accumulated_text));
                    state.set_chat_history(&task_id, history).await;
                }

                let _ = tx.send(OutboundChunk::StatusIndicator(ChannelStatus::Done));
                state
                    .send_and_persist(&task_id, &tx, OutboundChunk::Done)
                    .await;
            }
            Err(e) => {
                let msg = format!("System agent error: {e}");
                state
                    .send_and_persist(&task_id, &tx, OutboundChunk::Error(msg))
                    .await;
                state
                    .send_and_persist(&task_id, &tx, OutboundChunk::Done)
                    .await;
                return;
            }
            _ => {}
        }
    }
}
