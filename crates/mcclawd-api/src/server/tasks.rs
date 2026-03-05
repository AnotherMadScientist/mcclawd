use axum::{
    body::Body,
    extract::{Multipart, Path, State},
    http::{header, StatusCode},
    response::Response,
    Json,
};
use mcclawd_agent::engine::AgentEngine;
use mcclawd_agent::workspace::WorkspaceLoader;
use mcclawd_channels::{ChannelStatus, OutboundChunk};
use mcclawd_core::types::TaskId;
use mcclawd_tasks::manager::{TaskRecord, TaskStatus};
use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::completion::message::Message as RigMessage;
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
}

impl From<&TaskRecord> for TaskResponse {
    fn from(r: &TaskRecord) -> Self {
        Self {
            id: r.id.0.clone(),
            prompt: r.prompt.clone(),
            status: r.status.clone(),
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
    let id = {
        let mut mgr = state.tasks.write().await;
        mgr.start_task(prompt.clone())
    };

    // Persist to postgres
    state.pg_save_task(&id, &prompt, "Running").await;

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
                    }),
                );
            }
        }
    };

    // Create broadcast channel for streaming
    let tx = state.create_task_stream(&id).await;

    // Spawn agent execution in background
    let state_clone = state.clone();
    let task_id = id.clone();
    tokio::spawn(async move {
        run_agent(state_clone, task_id, &prompt, &workspace_name, tx).await;
    });

    (StatusCode::CREATED, Json(resp))
}

/// Run the Rig agent and stream output via broadcast channel.
async fn run_agent(
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

    // 1b. Check for attached files and inject their content into the prompt
    let attachment_files = attachment_paths(&state, &task_id.0).await;
    let prompt = if !attachment_files.is_empty() {
        let mut augmented = prompt.to_string();
        augmented.push_str("\n\n## Attached Files\n\n");
        for path in &attachment_files {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let mime = mime_guess::from_path(path).first_or_octet_stream().to_string();
            if mime.starts_with("text/") || mime.contains("json") || mime.contains("xml")
                || mime.contains("markdown") || mime.contains("yaml") || mime.contains("toml")
                || mime.contains("csv") || mime.contains("javascript") || mime.contains("typescript")
            {
                // Text files: read and include content
                match tokio::fs::read_to_string(path).await {
                    Ok(content) => {
                        let truncated = if content.len() > 50_000 { &content[..50_000] } else { &content };
                        augmented.push_str(&format!("### File: {}\n\n```\n{}\n```\n\n", name, truncated));
                    }
                    Err(_) => {
                        augmented.push_str(&format!("### File: {} (could not read)\n\n", name));
                    }
                }
            } else {
                augmented.push_str(&format!("### File: {} ({}, {} — binary file, content not included)\n\n", name, mime,
                    match tokio::fs::metadata(path).await {
                        Ok(m) => format!("{}KB", m.len() / 1024),
                        Err(_) => "unknown size".to_string(),
                    }
                ));
            }
        }
        augmented
    } else {
        prompt.to_string()
    };
    let prompt = prompt.as_str();

    // 2. Get API key from secrets backend
    let api_key = {
        let secrets_guard = state.secrets.read().await;
        match secrets_guard.as_ref() {
            Some(backend) => match backend.get("ANTHROPIC_API_KEY").await {
                Ok(Some(key)) => key,
                Ok(None) => {
                    let msg = "ANTHROPIC_API_KEY not found in secrets. Add it via Config > Secrets.".to_string();
                    state.send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone())).await;
                    state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
                    let mut mgr = state.tasks.write().await;
                    mgr.fail_task(&task_id, msg.clone());
                    state.pg_update_status(&task_id, "Failed", Some(&msg)).await;
                    return;
                }
                Err(e) => {
                    let msg = format!("Failed to read secrets: {e}");
                    state.send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone())).await;
                    state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
                    let mut mgr = state.tasks.write().await;
                    mgr.fail_task(&task_id, msg.clone());
                    state.pg_update_status(&task_id, "Failed", Some(&msg)).await;
                    return;
                }
            },
            None => {
                let msg = "Secrets vault not unlocked. Please log out and log in again.".to_string();
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
    broadcast(&tx, OutboundChunk::StatusIndicator(ChannelStatus::Processing));

    let chat_history = state.get_chat_history(&task_id).await;
    if !chat_history.is_empty() {
        tracing::info!(task_id = %task_id.0, turns = chat_history.len(), "Resuming with conversation history");
    }

    let mut stream = agent.stream_chat(prompt, chat_history.clone()).await;

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

/// DELETE /api/tasks/{id} — cancel running task or remove completed/failed task
pub async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> StatusCode {
    let mut mgr = state.tasks.write().await;
    let task_id = TaskId(id);

    if let Some(task) = mgr.get_task(&task_id) {
        if matches!(task.status, TaskStatus::Running) {
            mgr.fail_task(&task_id, "Cancelled by user".to_string());
        }
    }

    mgr.delete_task(&task_id);
    drop(mgr);

    // Also delete from postgres (cascades to events + chat history)
    state.pg_delete_task(&task_id).await;

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

    if !attachment_infos.is_empty() {
        let chunk = OutboundChunk::Attachments(attachment_infos);
        let task_id_typed = TaskId(id.clone());
        if let Some(tx) = state.task_streams.read().await.get(&task_id_typed) {
            state.send_and_persist(&task_id_typed, tx, chunk).await;
        } else {
            state.persist_only(&task_id_typed, chunk).await;
        }
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
