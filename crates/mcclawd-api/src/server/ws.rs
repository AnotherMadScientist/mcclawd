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

async fn handle_socket(mut socket: WebSocket, _state: AppState, _task_id: TaskId) {
    let chunks = vec![
        OutboundChunk::TextDelta("Thinking about your request...".to_string()),
        OutboundChunk::ToolStart {
            name: "memory.recall".to_string(),
        },
        OutboundChunk::ToolEnd {
            name: "memory.recall".to_string(),
            summary: Some("No memories found".to_string()),
        },
        OutboundChunk::TextBlock("Based on my analysis, here is the result.".to_string()),
        OutboundChunk::Done,
    ];

    for chunk in chunks {
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

        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
}
