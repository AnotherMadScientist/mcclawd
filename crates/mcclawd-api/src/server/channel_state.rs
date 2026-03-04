//! Channel state persistence API route handlers.

use axum::{extract::Path, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

/// Summary of a channel with persisted state.
#[derive(Debug, Serialize)]
pub struct ChannelStateInfo {
    pub channel_kind: String,
}

/// Error body returned as JSON.
#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

/// GET /api/channels/state -- list channels with persisted state.
///
/// Returns a JSON array of channel kinds that have saved state.
pub async fn list_channel_states() -> Json<Vec<ChannelStateInfo>> {
    // Phase 4 placeholder -- will query ChannelStateStore when wired into AppState.
    // For now returns an empty list to prove the route is live.
    Json(vec![])
}

/// DELETE /api/channels/state/:kind -- clear persisted state for a channel.
pub async fn delete_channel_state(Path(kind): Path<String>) -> impl IntoResponse {
    tracing::debug!(channel_kind = %kind, "Channel state delete (placeholder -- no store wired)");
    // Phase 4 placeholder -- will call ChannelStateStore::delete when wired.
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: format!("No persisted state for channel '{kind}'"),
        }),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_channel_states_returns_empty() {
        let result = list_channel_states().await;
        assert!(result.0.is_empty());
    }
}
