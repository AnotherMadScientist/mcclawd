use axum::{
    extract::{Path, Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use axum::extract::ws::{Message, WebSocket};
use jsonwebtoken::{decode, DecodingKey, Validation};
use mcclawd_channels::OutboundChunk;
use mcclawd_core::types::TaskId;
use serde::Deserialize;

use super::auth::Claims;
use super::state::AppState;

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    pub token: Option<String>,
    /// When set to "1", skip replaying persisted history (used on follow-up reconnects).
    pub skip_history: Option<String>,
}

/// GET /api/tasks/{id}/stream?token=JWT — WebSocket upgrade for task streaming
pub async fn task_stream(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    // Validate JWT from query param (browsers can't send headers on WS)
    if let Some(token) = &query.token {
        let validation = Validation::default();
        if decode::<Claims>(
            token,
            &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
            &validation,
        )
        .is_err()
        {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    } else {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let task_id = TaskId(id);
    let skip_history = query.skip_history.as_deref() == Some("1");
    ws.on_upgrade(move |socket| handle_socket(socket, state, task_id, skip_history))
}

async fn handle_socket(mut socket: WebSocket, state: AppState, task_id: TaskId, skip_history: bool) {
    // Replay any persisted event history first (fixes "history lost on revisit")
    // Skipped on follow-up reconnects where the client already has the history.
    let history = state.get_task_events(&task_id).await;
    if !skip_history {
        for chunk in &history {
            if let Ok(json) = serde_json::to_string(chunk) {
                if socket.send(Message::Text(json.into())).await.is_err() {
                    return;
                }
            }
        }

        // If history already contains Done, the task is finished — no need to subscribe
        if history.iter().any(|c| matches!(c, OutboundChunk::Done)) {
            return;
        }
    }

    // Subscribe to the task's broadcast channel for live updates
    let mut rx = match state.subscribe_task_stream(&task_id).await {
        Some(rx) => rx,
        None => {
            // No active stream and no history — task may have been cleaned up
            if history.is_empty() {
                let chunk = OutboundChunk::Error("Task stream not found".to_string());
                if let Ok(json) = serde_json::to_string(&chunk) {
                    let _ = socket.send(Message::Text(json.into())).await;
                }
            }
            let done = OutboundChunk::Done;
            if let Ok(json) = serde_json::to_string(&done) {
                let _ = socket.send(Message::Text(json.into())).await;
            }
            return;
        }
    };

    // Forward all broadcast chunks to the WebSocket client
    loop {
        match rx.recv().await {
            Ok(chunk) => {
                let is_done = matches!(chunk, OutboundChunk::Done);
                let json = match serde_json::to_string(&chunk) {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::error!("Failed to serialize chunk: {e}");
                        break;
                    }
                };

                if socket.send(Message::Text(json.into())).await.is_err() {
                    tracing::warn!("WebSocket client disconnected");
                    break;
                }

                if is_done {
                    break;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("WebSocket client lagged {n} messages");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                // Sender dropped, send Done
                let done = OutboundChunk::Done;
                if let Ok(json) = serde_json::to_string(&done) {
                    let _ = socket.send(Message::Text(json.into())).await;
                }
                break;
            }
        }
    }
}
