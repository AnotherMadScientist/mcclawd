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
use rig::completion::Prompt;
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
    state.send_and_persist(&task_id, &tx, OutboundChunk::TextDelta("Starting agent...".to_string())).await;

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
            mgr.fail_task(&task_id, msg);
            return;
        }
    };
    state.send_and_persist(&task_id, &tx, OutboundChunk::TextDelta("Workspace loaded".to_string())).await;

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
                    mgr.fail_task(&task_id, msg);
                    return;
                }
                Err(e) => {
                    let msg = format!("Failed to read secrets: {e}");
                    state.send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone())).await;
                    state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
                    let mut mgr = state.tasks.write().await;
                    mgr.fail_task(&task_id, msg);
                    return;
                }
            },
            None => {
                let msg = "Secrets vault not unlocked. Please log out and log in again.".to_string();
                state.send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone())).await;
                state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
                let mut mgr = state.tasks.write().await;
                mgr.fail_task(&task_id, msg);
                return;
            }
        }
    };
    state.send_and_persist(&task_id, &tx, OutboundChunk::TextDelta("Credentials verified".to_string())).await;

    // 3. Build the agent
    state.send_and_persist(&task_id, &tx, OutboundChunk::TextDelta("Building agent...".to_string())).await;
    let (agent, _memory, _mcp_conns) = match AgentEngine::build(workspace, &api_key, config.agent.max_turns, &config).await {
        Ok(result) => result,
        Err(e) => {
            let msg = format!("Failed to build agent: {e}");
            state.send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone())).await;
            state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
            let mut mgr = state.tasks.write().await;
            mgr.fail_task(&task_id, msg);
            return;
        }
    };

    // 4. Run the prompt (non-streaming — Rig returns full response)
    state.send_and_persist(&task_id, &tx, OutboundChunk::StatusIndicator(ChannelStatus::Processing)).await;

    match agent.prompt(prompt).await {
        Ok(response) => {
            state.send_and_persist(&task_id, &tx, OutboundChunk::StatusIndicator(ChannelStatus::Done)).await;
            state.send_and_persist(&task_id, &tx, OutboundChunk::TextBlock(response)).await;
            state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
            let mut mgr = state.tasks.write().await;
            mgr.complete_task(&task_id);
        }
        Err(e) => {
            let msg = format!("Agent error: {e}");
            state.send_and_persist(&task_id, &tx, OutboundChunk::StatusIndicator(ChannelStatus::Done)).await;
            state.send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone())).await;
            state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
            let mut mgr = state.tasks.write().await;
            mgr.fail_task(&task_id, msg);
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
    StatusCode::NO_CONTENT
}
