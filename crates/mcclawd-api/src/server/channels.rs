//! Channel management API route handlers — Phase 3 placeholders.

use axum::{extract::Path, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

/// Summary of a channel for the list endpoint.
#[derive(Debug, Serialize)]
pub struct ChannelInfo {
    pub id: String,
    pub platform: String,
    pub enabled: bool,
    pub capabilities: CapabilitiesInfo,
}

/// Channel capabilities exposed via the API.
#[derive(Debug, Serialize)]
pub struct CapabilitiesInfo {
    pub supports_streaming: bool,
    pub supports_edit: bool,
    pub supports_markdown: bool,
    pub max_message_len: usize,
    pub supports_files: bool,
    pub max_file_size: u64,
}

/// Request body for POST /api/channels/:id/test
#[derive(Debug, Deserialize)]
pub struct TestMessageRequest {
    pub message: String,
}

/// Error body returned as JSON.
#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

/// GET /api/channels — list all registered channels (placeholder: returns empty array).
pub async fn list_channels() -> Json<Vec<ChannelInfo>> {
    // Phase 3 placeholder — will query ChannelRegistry when wired up
    Json(vec![])
}

/// GET /api/channels/{id} — get channel by ID (placeholder: returns 404).
pub async fn get_channel(Path(id): Path<String>) -> impl IntoResponse {
    // Phase 3 placeholder — will look up channel by ID when registry is wired
    tracing::debug!(channel_id = %id, "Channel lookup (placeholder — not found)");
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Channel not found".into(),
        }),
    )
}

/// POST /api/channels/{id}/test — send a test message to a channel (placeholder: returns 404).
pub async fn test_channel(
    Path(id): Path<String>,
    Json(payload): Json<TestMessageRequest>,
) -> impl IntoResponse {
    // Phase 3 placeholder — will dispatch test message when channels are wired
    tracing::debug!(
        channel_id = %id,
        message = %payload.message,
        "Channel test (placeholder — not found)"
    );
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Channel not found".into(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_channels_returns_empty_array() {
        let result = list_channels().await;
        assert!(result.0.is_empty());
    }

    #[tokio::test]
    async fn get_channel_returns_not_found() {
        let result = get_channel(Path("nonexistent".into())).await;
        let response = result.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_message_returns_not_found() {
        let req = TestMessageRequest {
            message: "hello".into(),
        };
        let result = test_channel(Path("nonexistent".into()), Json(req)).await;
        let response = result.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
