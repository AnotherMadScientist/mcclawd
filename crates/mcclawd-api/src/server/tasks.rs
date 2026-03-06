use axum::{
    body::Body,
    extract::{Multipart, Path, State},
    http::{header, StatusCode},
    response::Response,
    Json,
};
use base64::Engine;
use mcclawd_agent::engine::AgentEngine;
use mcclawd_agent::workspace::WorkspaceLoader;
use mcclawd_channels::{ChannelStatus, OutboundChunk};
use mcclawd_core::types::TaskId;
use mcclawd_tasks::manager::{TaskRecord, TaskStatus};
use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::OneOrMany;
use rig::completion::message::{
    DocumentSourceKind, Image, ImageMediaType, Message as RigMessage, MimeType,
    UserContent,
};
use rig::streaming::{StreamedAssistantContent, StreamingChat};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio_util::io::ReaderStream;

use super::state::AppState;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CreateTaskRequest {
    pub prompt: String,
    pub workspace: Option<String>,
    pub model: Option<String>,
    /// When true, create the task but do NOT auto-start the agent.
    /// The caller must upload attachments and then POST /api/tasks/{id}/message
    /// to trigger the agent. This prevents a race where the agent starts
    /// before attachment files are written to disk.
    #[serde(default)]
    pub delay_start: bool,
    /// Optional tags for categorizing and filtering tasks.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentMeta {
    pub name: String,
    pub size: u64,
    pub content_type: String,
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct TaskResponse {
    pub id: String,
    pub prompt: String,
    pub status: TaskStatus,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl From<&TaskRecord> for TaskResponse {
    fn from(r: &TaskRecord) -> Self {
        Self {
            id: r.id.0.clone(),
            prompt: r.prompt.clone(),
            status: r.status.clone(),
            tags: r.tags.clone(),
        }
    }
}

/// GET /api/tasks — list all tasks
pub async fn list_tasks(State(state): State<AppState>) -> Json<Vec<TaskResponse>> {
    let mgr = state.tasks.read().await;
    let tasks: Vec<TaskResponse> = mgr.all_tasks().iter().map(|t| TaskResponse::from(*t)).collect();
    Json(tasks)
}

/// POST /api/tasks — create a new task and spawn agent execution
pub async fn create_task(
    State(state): State<AppState>,
    Json(body): Json<CreateTaskRequest>,
) -> (StatusCode, Json<TaskResponse>) {
    // Sanitize user prompt to strip known injection patterns
    let sanitized = mcclawd_core::sanitize_prompt(&body.prompt);
    if sanitized.was_modified {
        tracing::warn!(
            patterns = ?sanitized.detected_patterns,
            "Prompt injection patterns detected and stripped from user input"
        );
    }
    let prompt = sanitized.text;
    let workspace_name = body.workspace.clone().unwrap_or_else(|| "default".to_string());

    // Create task record
    let tags = body.tags.unwrap_or_default();
    let id = {
        let mut mgr = state.tasks.write().await;
        mgr.start_task_with_tags(prompt.clone(), tags)
    };

    // When delay_start is set, persist as Pending (caller will trigger via sendMessage
    // after uploading attachments). Otherwise persist as Running.
    let initial_status = if body.delay_start { "Pending" } else { "Running" };
    // Retrieve tags from the task record for DB persistence
    let task_tags = {
        let mgr = state.tasks.read().await;
        mgr.get_task(&id).map(|t| t.tags.clone()).unwrap_or_default()
    };
    state.pg_save_task(&id, &prompt, initial_status, &task_tags).await;

    let resp = {
        let mgr = state.tasks.read().await;
        match mgr.get_task(&id) {
            Some(task) => TaskResponse::from(task),
            None => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(TaskResponse {
                        id: id.0,
                        prompt,
                        status: TaskStatus::Failed("Task creation failed".to_string()),
                        tags: Vec::new(),
                    }),
                );
            }
        }
    };

    // Create broadcast channel for streaming
    let tx = state.create_task_stream(&id).await;

    if !body.delay_start {
        // Spawn agent execution in background immediately
        let state_clone = state.clone();
        let task_id = id.clone();
        tokio::spawn(async move {
            run_agent(state_clone, task_id, &prompt, &workspace_name, tx).await;
        });
    }

    (StatusCode::CREATED, Json(resp))
}

/// Run the Rig agent and stream output via broadcast channel.
///
/// All execution runs inside Docker containers via the sandbox orchestrator.
/// Falls back to in-process host execution only when Docker is unavailable
/// (development convenience — not production).
async fn run_agent(
    state: AppState,
    task_id: TaskId,
    prompt: &str,
    workspace_name: &str,
    tx: tokio::sync::broadcast::Sender<OutboundChunk>,
) {
    // Try Docker-sandboxed execution first (production path)
    if let Ok(orch) = crate::sandbox::SandboxOrchestrator::new() {
        if orch.health_check().await {
            run_agent_sandboxed(state, task_id, prompt, workspace_name, tx).await;
            return;
        }
    }

    // Fallback: host execution when Docker is unavailable (dev only)
    tracing::warn!("Docker unavailable — falling back to host execution (dev mode)");
    run_agent_host(state, task_id, prompt, workspace_name, tx).await;
}

/// Run agent task inside a Docker sandbox container.
async fn run_agent_sandboxed(
    state: AppState,
    task_id: TaskId,
    prompt: &str,
    _workspace_name: &str,
    tx: tokio::sync::broadcast::Sender<OutboundChunk>,
) {
    use crate::sandbox::SandboxOrchestrator;

    state.send_and_persist(&task_id, &tx, OutboundChunk::UserMessage(prompt.to_string())).await;
    let _ = tx.send(OutboundChunk::TextDelta("Starting sandboxed agent...".to_string()));

    // Update status to Building
    {
        let mut mgr = state.tasks.write().await;
        mgr.running(&task_id);
    }
    state.pg_update_status(&task_id, "Building", None).await;
    let _ = tx.send(OutboundChunk::StatusIndicator(ChannelStatus::Processing));

    let config = state.config.read().await.clone();

    // Get sandbox orchestrator
    let orchestrator = match SandboxOrchestrator::new() {
        Ok(o) => o,
        Err(e) => {
            let msg = format!("Docker unavailable for sandbox: {e}");
            state.send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone())).await;
            state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
            let mut mgr = state.tasks.write().await;
            mgr.fail_task(&task_id, msg.clone());
            state.pg_update_status(&task_id, "Failed", Some(&msg)).await;
            return;
        }
    };

    // Build sandbox config from McclawdConfig
    let sandbox_cfg = mcclawd_core::skills::SandboxConfig {
        workspace_dir: config.workspaces_dir().to_string_lossy().to_string(),
        agentgateway_url: config.mcp.agentgateway_url.clone(),
        memory_limit: config.sandbox.memory_limit,
        cpu_limit: config.sandbox.cpu_limit,
        network: config.sandbox.network.clone(),
        ..Default::default()
    };

    // Collect secrets for container
    let mut secrets_map = std::collections::HashMap::new();
    if let Some(backend) = state.secrets.read().await.as_ref() {
        if let Ok(Some(key)) = backend.get("ANTHROPIC_API_KEY").await {
            secrets_map.insert("ANTHROPIC_API_KEY".to_string(), key);
        }
    }

    // Stream logs to broadcast channel
    let (log_tx, mut log_rx) = tokio::sync::mpsc::channel::<String>(256);
    let tx_clone = tx.clone();
    let task_id_clone = task_id.clone();
    let state_clone = state.clone();
    tokio::spawn(async move {
        while let Some(line) = log_rx.recv().await {
            state_clone.send_and_persist(
                &task_id_clone,
                &tx_clone,
                OutboundChunk::TextDelta(line),
            ).await;
        }
    });

    // Run the agent task in Docker
    let _ = tx.send(OutboundChunk::TextDelta("Building sandbox image...".to_string()));
    state.pg_update_status(&task_id, "Running", None).await;

    match orchestrator
        .run_agent_task(
            &task_id,
            &config.sandbox.base_image,
            prompt,
            &sandbox_cfg,
            &secrets_map,
            log_tx,
        )
        .await
    {
        Ok(exit_code) => {
            if exit_code == 0 {
                state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
                let mut mgr = state.tasks.write().await;
                mgr.complete_task(&task_id);
                state.pg_update_status(&task_id, "Completed", None).await;
            } else {
                let msg = format!("Sandbox agent exited with code {exit_code}");
                state.send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone())).await;
                state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
                let mut mgr = state.tasks.write().await;
                mgr.fail_task(&task_id, msg.clone());
                state.pg_update_status(&task_id, "Failed", Some(&msg)).await;
            }
        }
        Err(e) => {
            let msg = format!("Sandbox execution failed: {e}");
            state.send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone())).await;
            state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
            let mut mgr = state.tasks.write().await;
            mgr.fail_task(&task_id, msg.clone());
            state.pg_update_status(&task_id, "Failed", Some(&msg)).await;
        }
    }
}

/// Run agent in-process on the host (original behavior).
async fn run_agent_host(
    state: AppState,
    task_id: TaskId,
    prompt: &str,
    workspace_name: &str,
    tx: tokio::sync::broadcast::Sender<OutboundChunk>,
) {
    // Persist the user message for history replay (human/assistant turn separation)
    state.send_and_persist(&task_id, &tx, OutboundChunk::UserMessage(prompt.to_string())).await;

    // Helper: broadcast-only (transient status, not persisted to history)
    let broadcast = |tx: &tokio::sync::broadcast::Sender<OutboundChunk>, chunk: OutboundChunk| {
        let _ = tx.send(chunk);
    };

    broadcast(&tx, OutboundChunk::TextDelta("Starting agent...".to_string()));

    // 1. Load workspace
    let config = state.config.read().await.clone();
    let loader = WorkspaceLoader::new(config.workspaces_dir());
    let workspace = match loader.load(workspace_name) {
        Ok(w) => w,
        Err(e) => {
            let msg = format!("Failed to load workspace: {e}");
            state.send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone())).await;
            state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
            let mut mgr = state.tasks.write().await;
            mgr.fail_task(&task_id, msg.clone());
            state.pg_update_status(&task_id, "Failed", Some(&msg)).await;
            return;
        }
    };
    broadcast(&tx, OutboundChunk::TextDelta("Workspace loaded".to_string()));

    // 1b. Check for attached files — text files injected as prompt text,
    //     images sent as multimodal content parts via rig's UserContent::Image
    let attachment_files = attachment_paths(&state, &task_id.0).await;
    let mut text_augmented = prompt.to_string();
    let mut image_parts: Vec<UserContent> = Vec::new();

    if !attachment_files.is_empty() {
        text_augmented.push_str("\n\n## Attached Files\n\n");
        for path in &attachment_files {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let mime = mime_guess::from_path(path).first_or_octet_stream().to_string();

            if let Some(image_media_type) = ImageMediaType::from_mime_type(&mime) {
                // Image files: read bytes, base64-encode, add as multimodal content
                match tokio::fs::read(path).await {
                    Ok(bytes) => {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        image_parts.push(UserContent::Image(Image {
                            data: DocumentSourceKind::Base64(b64),
                            media_type: Some(image_media_type),
                            detail: None,
                            additional_params: None,
                        }));
                        text_augmented.push_str(&format!(
                            "### Image: {} ({}, {}KB — sent as image content)\n\n",
                            name, mime, bytes.len() / 1024
                        ));
                    }
                    Err(_) => {
                        text_augmented.push_str(&format!("### File: {} (could not read)\n\n", name));
                    }
                }
            } else if mime.starts_with("text/") || mime.contains("json") || mime.contains("xml")
                || mime.contains("markdown") || mime.contains("yaml") || mime.contains("toml")
                || mime.contains("csv") || mime.contains("javascript") || mime.contains("typescript")
            {
                // Text files: read and include content
                match tokio::fs::read_to_string(path).await {
                    Ok(content) => {
                        let truncated = if content.len() > 50_000 { &content[..50_000] } else { &content };
                        // Escape triple backticks to prevent code-block breakout injection
                        let safe_content = truncated.replace("```", "` ` `");
                        text_augmented.push_str(&format!("### File: {}\n\n```\n{}\n```\n\n", name, safe_content));
                    }
                    Err(_) => {
                        text_augmented.push_str(&format!("### File: {} (could not read)\n\n", name));
                    }
                }
            } else {
                text_augmented.push_str(&format!("### File: {} ({}, {} — binary file, content not included)\n\n", name, mime,
                    match tokio::fs::metadata(path).await {
                        Ok(m) => format!("{}KB", m.len() / 1024),
                        Err(_) => "unknown size".to_string(),
                    }
                ));
            }
        }
    }

    // Build the prompt message — multimodal if images attached, plain text otherwise
    let prompt_message: RigMessage = if image_parts.is_empty() {
        text_augmented.as_str().into()
    } else {
        let mut parts = vec![UserContent::text(&text_augmented)];
        parts.extend(image_parts);
        RigMessage::User {
            content: OneOrMany::many(parts).unwrap_or_else(|_| OneOrMany::one(UserContent::text(&text_augmented))),
        }
    };
    let prompt = text_augmented.as_str();

    // 2. Get API key — try vault first, fall back to env var directly
    let api_key = {
        let vault_key = {
            let secrets_guard = state.secrets.read().await;
            match secrets_guard.as_ref() {
                Some(backend) => backend.get("ANTHROPIC_API_KEY").await.ok().flatten(),
                None => None,
            }
        };
        // Prefer vault, fall back to env var (belt + suspenders)
        let key = vault_key
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());
        match key {
            Some(k) => k,
            None => {
                let msg = "ANTHROPIC_API_KEY not found. Set it in Config > Secrets or .env file.".to_string();
                state.send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone())).await;
                state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
                let mut mgr = state.tasks.write().await;
                mgr.fail_task(&task_id, msg.clone());
                state.pg_update_status(&task_id, "Failed", Some(&msg)).await;
                return;
            }
        }
    };
    broadcast(&tx, OutboundChunk::TextDelta("Credentials verified".to_string()));

    // 3. Build the agent
    broadcast(&tx, OutboundChunk::TextDelta("Building agent...".to_string()));
    let (agent, _memory, _mcp_conns) = match AgentEngine::build(workspace, &api_key, config.agent.max_turns, &config).await {
        Ok(result) => result,
        Err(e) => {
            let msg = format!("Failed to build agent: {e}");
            state.send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone())).await;
            state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
            let mut mgr = state.tasks.write().await;
            mgr.fail_task(&task_id, msg.clone());
            state.pg_update_status(&task_id, "Failed", Some(&msg)).await;
            return;
        }
    };

    // Report MCP tool availability
    let tool_count = _mcp_conns.iter().map(|b| b.tools.len()).sum::<usize>();
    if tool_count > 0 {
        broadcast(&tx, OutboundChunk::TextDelta(format!("{tool_count} MCP tools available")));
    } else {
        broadcast(&tx, OutboundChunk::TextDelta("No MCP tools connected (is AgentGateway running?)".to_string()));
    }

    // 4. Stream with conversation history — enables multi-turn follow-ups
    // Persist Processing so it replays on WS reconnect — frontend needs it to enter streaming mode
    state.send_and_persist(&task_id, &tx, OutboundChunk::StatusIndicator(ChannelStatus::Processing)).await;

    let chat_history = state.get_chat_history(&task_id).await;
    if !chat_history.is_empty() {
        tracing::info!(task_id = %task_id.0, turns = chat_history.len(), "Resuming with conversation history");
    }

    let mut stream = agent.stream_chat(prompt_message, chat_history.clone()).await;

    // Accumulate full response text for clean history persistence
    let mut accumulated_text = String::new();

    while let Some(item) = stream.next().await {
        match item {
            Ok(MultiTurnStreamItem::StreamAssistantItem(content)) => {
                match content {
                    StreamedAssistantContent::Text(text) => {
                        // Broadcast delta for live streaming UX (not persisted)
                        broadcast(&tx, OutboundChunk::TextDelta(text.text.clone()));
                        accumulated_text.push_str(&text.text);
                    }
                    StreamedAssistantContent::ToolCall { tool_call, .. } => {
                        // Persist tool calls so history shows them
                        state.send_and_persist(&task_id, &tx, OutboundChunk::ToolStart { name: tool_call.function.name.clone() }).await;
                    }
                    _ => {} // Reasoning, ToolCallDelta, Final, non_exhaustive
                }
            }
            Ok(MultiTurnStreamItem::StreamUserItem(_)) => {
                // Tool results auto-injected by Rig
            }
            Ok(MultiTurnStreamItem::FinalResponse(final_resp)) => {
                // Record token usage from the LLM response
                let usage = final_resp.usage();
                let model_name = config.agent.model.clone();
                let prompt_preview: String = prompt.chars().take(50).collect();
                let cost = mcclawd_core::providers::estimate_cost_usd(
                    &model_name,
                    usage.input_tokens,
                    usage.output_tokens,
                );
                {
                    let pool = state.provider_pool.read().await;
                    pool.record_usage_detailed(
                        "anthropic",
                        usage.total_tokens,
                        usage.input_tokens,
                        usage.output_tokens,
                        cost,
                        Some((&task_id.0, &prompt_preview, &model_name)),
                    );
                }
                tracing::info!(
                    task_id = %task_id.0,
                    model = %model_name,
                    input_tokens = usage.input_tokens,
                    output_tokens = usage.output_tokens,
                    cost_usd = cost,
                    "Usage recorded"
                );

                // Persist usage to database
                {
                    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
                    let model_entry = mcclawd_core::providers::ModelUsageEntry {
                        model: model_name.clone(),
                        input_tokens: usage.input_tokens,
                        output_tokens: usage.output_tokens,
                        total_tokens: usage.total_tokens,
                        estimated_cost_usd: cost,
                        request_count: 1,
                    };
                    let task_entry = mcclawd_core::providers::TaskUsageEntry {
                        task_id: task_id.0.clone(),
                        prompt_preview: prompt_preview.clone(),
                        model: model_name.clone(),
                        total_tokens: usage.total_tokens,
                        estimated_cost_usd: cost,
                    };
                    let store = state.pg_store.clone();
                    let today_c = today.clone();
                    // Spawn DB writes so they don't block streaming
                    tokio::spawn(async move {
                        if let Err(e) = store.upsert_daily_usage("admin", &today_c, cost, usage.total_tokens).await {
                            tracing::warn!("Failed to persist daily usage: {e}");
                        }
                        if let Err(e) = store.upsert_model_usage("admin", &model_entry).await {
                            tracing::warn!("Failed to persist model usage: {e}");
                        }
                        if let Err(e) = store.insert_task_usage("admin", &task_entry).await {
                            tracing::warn!("Failed to persist task usage: {e}");
                        }
                    });
                }

                // Persist the complete response as a TextBlock for clean history replay
                if !accumulated_text.is_empty() {
                    state.persist_only(&task_id, OutboundChunk::TextBlock(accumulated_text.clone())).await;
                }

                // Persist conversation history for follow-ups
                if let Some(history) = final_resp.history() {
                    state.set_chat_history(&task_id, history.to_vec()).await;
                    tracing::debug!(task_id = %task_id.0, messages = history.len(), "Chat history persisted");
                } else {
                    // Fallback: manually append user + assistant messages
                    let mut history = chat_history.clone();
                    history.push(RigMessage::user(prompt));
                    history.push(RigMessage::assistant(&accumulated_text));
                    state.set_chat_history(&task_id, history).await;
                    tracing::debug!(task_id = %task_id.0, "Chat history persisted (manual fallback)");
                }

                broadcast(&tx, OutboundChunk::StatusIndicator(ChannelStatus::Done));
                state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
                let mut mgr = state.tasks.write().await;
                mgr.complete_task(&task_id);
                state.pg_update_status(&task_id, "Completed", None).await;
            }
            Err(e) => {
                let msg = format!("Streaming error: {e}");
                state.send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone())).await;
                state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
                let mut mgr = state.tasks.write().await;
                mgr.fail_task(&task_id, msg.clone());
                state.pg_update_status(&task_id, "Failed", Some(&msg)).await;
                return;
            }
            _ => {} // non_exhaustive guard
        }
    }
}

/// GET /api/tasks/{id} — get single task
pub async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TaskResponse>, StatusCode> {
    let mgr = state.tasks.read().await;
    let task_id = TaskId(id);
    match mgr.get_task(&task_id) {
        Some(task) => Ok(Json(TaskResponse::from(task))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub message: String,
    /// If set, truncate chat history to this many messages before appending.
    /// Used for edit/retry: discard everything after the edited message.
    #[serde(default)]
    pub truncate_history_to: Option<usize>,
}

/// POST /api/tasks/{id}/message — send a follow-up message to an existing task
pub async fn send_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SendMessageRequest>,
) -> Result<StatusCode, StatusCode> {
    let sanitized = mcclawd_core::sanitize_prompt(&body.message);
    if sanitized.was_modified {
        tracing::warn!(
            patterns = ?sanitized.detected_patterns,
            "Prompt injection patterns detected and stripped from follow-up message"
        );
    }
    let message = sanitized.text;
    let task_id = TaskId(id);

    // Verify task exists
    {
        let mgr = state.tasks.read().await;
        if mgr.get_task(&task_id).is_none() {
            return Err(StatusCode::NOT_FOUND);
        }
    }

    // Get existing broadcast channel, or create a new one if the old one was dropped
    let tx = {
        let streams = state.task_streams.read().await;
        streams.get(&task_id).cloned()
    };
    let tx = match tx {
        Some(tx) => tx,
        None => state.create_task_stream(&task_id).await,
    };

    // Truncate chat history if requested (edit/retry — discard messages after edit point)
    if let Some(keep) = body.truncate_history_to {
        let mut history = state.get_chat_history(&task_id).await;
        if keep < history.len() {
            tracing::info!(task_id = %task_id.0, keep, total = history.len(), "Truncating chat history for edit/retry");
            history.truncate(keep);
            state.set_chat_history(&task_id, history).await;
        }
        // Also truncate persisted events
        let mut events = state.get_task_events(&task_id).await;
        // Keep only events up to the Nth UserMessage
        let mut user_msg_count = 0;
        let mut cut_at = events.len();
        for (i, ev) in events.iter().enumerate() {
            if matches!(ev, mcclawd_channels::OutboundChunk::UserMessage(_)) {
                user_msg_count += 1;
                // keep = number of chat turns (user+assistant pairs) to keep
                // Each UserMessage starts a new turn
                if user_msg_count > keep / 2 {
                    cut_at = i;
                    break;
                }
            }
        }
        if cut_at < events.len() {
            events.truncate(cut_at);
            state.set_task_events(&task_id, events).await;
        }
    }

    // Mark task as running again
    {
        let mut mgr = state.tasks.write().await;
        mgr.running(&task_id);
    }
    state.pg_update_status(&task_id, "Running", None).await;

    // Spawn agent execution for the follow-up message
    let workspace_name = "default".to_string();
    let state_clone = state.clone();
    let tid = task_id.clone();
    tokio::spawn(async move {
        run_agent(state_clone, tid, &message, &workspace_name, tx).await;
    });

    Ok(StatusCode::ACCEPTED)
}

#[derive(Debug, Deserialize)]
pub struct DeleteAllQuery {
    /// Optional status filter: "Completed", "Failed", "Running", "Pending"
    pub status: Option<String>,
    /// Optional tag filter: delete only tasks with this tag
    pub tag: Option<String>,
}

/// DELETE /api/tasks — delete all tasks (or filter by ?status=... and/or ?tag=...)
pub async fn delete_all_tasks(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<DeleteAllQuery>,
) -> Json<serde_json::Value> {
    let mut mgr = state.tasks.write().await;

    // If tag filter is provided, use the dedicated delete_by_tag method
    // (optionally combined with status filter)
    let to_delete: Vec<TaskId> = if let Some(ref tag) = query.tag {
        let all = mgr.all_tasks().iter().map(|t| (t.id.clone(), t.status.clone(), t.tags.clone())).collect::<Vec<_>>();
        all.into_iter()
            .filter(|(_, status, tags)| {
                let tag_match = tags.iter().any(|tg| tg == tag);
                let status_match = if let Some(ref filter) = query.status {
                    let status_str = match status {
                        TaskStatus::Pending => "Pending",
                        TaskStatus::Building => "Building",
                        TaskStatus::Running => "Running",
                        TaskStatus::Completed => "Completed",
                        TaskStatus::Failed(_) => "Failed",
                        TaskStatus::Restarting { .. } => "Restarting",
                        TaskStatus::SwarmRunning { .. } => "SwarmRunning",
                    };
                    status_str.eq_ignore_ascii_case(filter)
                } else {
                    true
                };
                tag_match && status_match
            })
            .map(|(id, _, _)| id)
            .collect()
    } else {
        let all = mgr.all_tasks().iter().map(|t| (t.id.clone(), t.status.clone())).collect::<Vec<_>>();
        all.into_iter()
            .filter(|(_, status)| {
                if let Some(ref filter) = query.status {
                    let status_str = match status {
                        TaskStatus::Pending => "Pending",
                        TaskStatus::Building => "Building",
                        TaskStatus::Running => "Running",
                        TaskStatus::Completed => "Completed",
                        TaskStatus::Failed(_) => "Failed",
                        TaskStatus::Restarting { .. } => "Restarting",
                        TaskStatus::SwarmRunning { .. } => "SwarmRunning",
                    };
                    status_str.eq_ignore_ascii_case(filter)
                } else {
                    true
                }
            })
            .map(|(id, _)| id)
            .collect()
    };

    let count = to_delete.len();
    for id in &to_delete {
        mgr.delete_task(id);
    }
    drop(mgr);

    // Bulk-delete from Postgres by tag (fire-and-forget) when tag filter is present
    if let Some(ref tag) = query.tag {
        let store = state.pg_store.clone();
        let tag_c = tag.clone();
        tokio::spawn(async move {
            if let Err(e) = store.delete_tasks_by_tag("admin", Some(&tag_c)).await {
                tracing::warn!(error = %e, "Failed to bulk-delete tasks by tag from DB");
            }
        });
    }

    // Clean up associated state for each deleted task
    for id in &to_delete {
        // Individual pg delete only when no tag filter (tag filter handled in bulk above)
        if query.tag.is_none() {
            state.pg_delete_task(id).await;
        }
        state.task_streams.write().await.remove(id);
        state.task_chat_history.write().await.remove(id);
        state.task_events.write().await.remove(id);
    }

    Json(serde_json::json!({ "deleted": count }))
}

/// DELETE /api/tasks/{id} — cancel running task or remove completed/failed task
pub async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> StatusCode {
    let mut mgr = state.tasks.write().await;
    let task_id = TaskId(id);

    // Return 404 if task doesn't exist
    let task = match mgr.get_task(&task_id) {
        Some(t) => t.clone(),
        None => return StatusCode::NOT_FOUND,
    };

    if matches!(task.status, TaskStatus::Running) {
        mgr.fail_task(&task_id, "Cancelled by user".to_string());
    }

    mgr.delete_task(&task_id);
    drop(mgr);

    // Also delete from postgres (cascades to events + chat history)
    state.pg_delete_task(&task_id).await;

    // Clean up broadcast channel
    state.task_streams.write().await.remove(&task_id);

    // Clean up chat history
    state.task_chat_history.write().await.remove(&task_id);

    StatusCode::NO_CONTENT
}

// ── Attachments ─────────────────────────────────────────────────────────────

/// Sanitize a filename: strip path separators and `..` to prevent traversal.
fn sanitize_filename(name: &str) -> String {
    name.replace(['/', '\\'], "")
        .replace("..", "")
        .trim()
        .to_string()
}

/// Resolve the attachments directory for a given task.
async fn attachments_dir(state: &AppState, task_id: &str) -> PathBuf {
    let config = state.config.read().await;
    config.data_dir.join("tasks").join(task_id).join("attachments")
}

/// POST /api/tasks/{id}/attachments — upload one or more files
pub async fn upload_attachments(
    State(state): State<AppState>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<Vec<AttachmentMeta>>, StatusCode> {
    let dir = attachments_dir(&state, &id).await;
    tokio::fs::create_dir_all(&dir).await.map_err(|e| {
        tracing::error!(error = %e, "Failed to create attachments dir");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut results = Vec::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let original_name = field.file_name().unwrap_or("unnamed").to_string();
        let safe_name = sanitize_filename(&original_name);
        if safe_name.is_empty() {
            continue;
        }

        let content_type = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();

        let data = field.bytes().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to read multipart field");
            StatusCode::BAD_REQUEST
        })?;

        let file_path = dir.join(&safe_name);
        tokio::fs::write(&file_path, &data).await.map_err(|e| {
            tracing::error!(error = %e, "Failed to write attachment");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        results.push(AttachmentMeta {
            name: safe_name.clone(),
            size: data.len() as u64,
            content_type,
            url: format!("/api/tasks/{id}/attachments/{safe_name}"),
        });
    }

    tracing::info!(task_id = %id, count = results.len(), "Attachments uploaded");

    // Emit attachment event to the task stream for conversation history
    let attachment_infos: Vec<mcclawd_channels::AttachmentInfo> = results
        .iter()
        .map(|a| mcclawd_channels::AttachmentInfo {
            name: a.name.clone(),
            size: a.size,
            content_type: a.content_type.clone(),
            url: a.url.clone(),
        })
        .collect();

    // Persist attachment info for history replay, but do NOT broadcast —
    // the frontend already shows local thumbnails during the current session.
    // Broadcasting would cause duplicates (Bug: image attachments shown twice).
    if !attachment_infos.is_empty() {
        let chunk = OutboundChunk::Attachments(attachment_infos);
        let task_id_typed = TaskId(id.clone());
        state.persist_only(&task_id_typed, chunk).await;
    }

    Ok(Json(results))
}

/// GET /api/tasks/{id}/attachments — list all attachments for a task
pub async fn list_attachments(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<AttachmentMeta>>, StatusCode> {
    let dir = attachments_dir(&state, &id).await;
    if !dir.exists() {
        return Ok(Json(Vec::new()));
    }

    let mut entries = tokio::fs::read_dir(&dir).await.map_err(|e| {
        tracing::error!(error = %e, "Failed to read attachments dir");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut results = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(meta) = entry.metadata().await {
            if meta.is_file() {
                let name = entry.file_name().to_string_lossy().to_string();
                let content_type = mime_guess::from_path(&name)
                    .first_or_octet_stream()
                    .to_string();
                results.push(AttachmentMeta {
                    url: format!("/api/tasks/{id}/attachments/{name}"),
                    name,
                    size: meta.len(),
                    content_type,
                });
            }
        }
    }

    Ok(Json(results))
}

/// GET /api/tasks/{id}/attachments/{filename} — download/serve a single attachment
pub async fn download_attachment(
    State(state): State<AppState>,
    Path((id, filename)): Path<(String, String)>,
) -> Result<Response<Body>, StatusCode> {
    let safe_name = sanitize_filename(&filename);
    if safe_name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let dir = attachments_dir(&state, &id).await;
    let file_path = dir.join(&safe_name);

    let file = tokio::fs::File::open(&file_path).await.map_err(|_| StatusCode::NOT_FOUND)?;
    let stream = ReaderStream::new(file);

    let content_type = mime_guess::from_path(&safe_name)
        .first_or_octet_stream()
        .to_string();

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("inline; filename=\"{safe_name}\""),
        )
        .body(Body::from_stream(stream))
        .unwrap())
}

/// List attachment file paths for a task (used to inject into agent context).
pub async fn attachment_paths(state: &AppState, task_id: &str) -> Vec<PathBuf> {
    let dir = attachments_dir(state, task_id).await;
    if !dir.exists() {
        return Vec::new();
    }
    let mut paths = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.metadata().await.map(|m| m.is_file()).unwrap_or(false) {
                paths.push(entry.path());
            }
        }
    }
    paths
}
