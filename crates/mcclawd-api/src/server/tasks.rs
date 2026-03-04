use axum::{
    extract::{Path, State},
    http::StatusCode,
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

use super::state::AppState;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CreateTaskRequest {
    pub prompt: String,
    pub workspace: Option<String>,
    pub model: Option<String>,
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
