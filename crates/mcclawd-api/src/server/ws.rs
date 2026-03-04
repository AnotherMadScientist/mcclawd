use axum::{
    extract::{Path, State, WebSocketUpgrade},
    response::Response,
};
use axum::extract::ws::{Message, WebSocket};
use mcclawd_channels::OutboundChunk;
use mcclawd_core::types::TaskId;

use super::state::AppState;

/// GET /api/tasks/{id}/stream — WebSocket upgrade for task streaming
pub async fn task_stream(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    let task_id = TaskId(id);
    ws.on_upgrade(move |socket| handle_socket(socket, state, task_id))
}

async fn handle_socket(mut socket: WebSocket, state: AppState, task_id: TaskId) {
    // Subscribe to the task's broadcast channel
    let mut rx = match state.subscribe_task_stream(&task_id).await {
        Some(rx) => rx,
        None => {
            // Task doesn't have a stream (maybe already completed before WS connected)
            let chunk = OutboundChunk::Error("Task stream not found".to_string());
            if let Ok(json) = serde_json::to_string(&chunk) {
                let _ = socket.send(Message::Text(json.into())).await;
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
