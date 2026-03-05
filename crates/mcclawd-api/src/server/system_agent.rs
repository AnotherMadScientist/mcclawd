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
    // Clear chat history
    {
        let mut history = state.task_chat_history.write().await;
        history.remove(&task_id);
    }

    StatusCode::NO_CONTENT
}

/// Build system prompt for the system agent.
fn build_system_prompt(workspace_prompt: &str) -> String {
    format!(
        r#"{workspace_prompt}

---

## System Agent Instructions

You are McClawd's system agent — an always-on assistant that helps users control the application through natural language commands. You have access to tools for:

1. **Navigation**: Navigate the UI to any page (tasks, skills, workspace, config, secrets)
2. **Task Management**: Create new agent tasks
3. **Skill Management**: Install, uninstall, and list skills from ClawHub
4. **Secret Management**: Set, delete, and list secrets (API keys, credentials)
5. **Workspace Management**: Read and update workspace files (SOUL.md, AGENTS.md, USER.md)

When the user asks you to do something, use the appropriate tool. Be concise in your responses.
When navigating, use the tool — don't just tell the user to navigate manually.
When managing skills or secrets, use the tools to perform the action.

Keep responses short and action-oriented. Confirm what you did, don't explain what you could do."#
    )
}

/// Run the system agent with streaming output.
async fn run_system_agent(
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
        match mcclawd_agent::engine::AgentEngine::build_system_agent(&api_key, &system_prompt)
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
