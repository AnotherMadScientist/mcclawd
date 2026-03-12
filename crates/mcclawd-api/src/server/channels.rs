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

/// GET /api/channels — list all registered channels.
///
/// Returns the CLI channel (always available) and any configured external channels.
/// External channel adapters (Discord, Slack, WhatsApp, Email) are Phase 3.
pub async fn list_channels() -> Json<Vec<ChannelInfo>> {
    Json(vec![ChannelInfo {
        id: "cli".to_string(),
        platform: "cli".to_string(),
        enabled: true,
        capabilities: CapabilitiesInfo {
            supports_streaming: true,
            supports_edit: false,
            supports_markdown: true,
            max_message_len: usize::MAX,
            supports_files: false,
            max_file_size: 0,
        },
    }])
}

/// GET /api/channels/{id} — get channel by ID.
pub async fn get_channel(Path(id): Path<String>) -> impl IntoResponse {
    if id == "cli" {
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "id": "cli",
                "platform": "cli",
                "enabled": true,
                "status": "connected",
            })),
        );
    }
    tracing::debug!(channel_id = %id, "Channel not found");
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": format!("Channel '{id}' not found. External channels (Discord, Slack, WhatsApp, Email) are not yet available."),
        })),
    )
}

/// POST /api/channels/{id}/test — send a test message to a channel.
pub async fn test_channel(
    Path(id): Path<String>,
    Json(_payload): Json<TestMessageRequest>,
) -> impl IntoResponse {
    tracing::debug!(channel_id = %id, "Channel test requested");
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(ErrorResponse {
            error: format!("Channel test for '{id}' is not yet available. External channel adapters are Phase 3."),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_channels_returns_cli() {
        let result = list_channels().await;
        assert_eq!(result.0.len(), 1);
        assert_eq!(result.0[0].id, "cli");
        assert!(result.0[0].enabled);
    }

    #[tokio::test]
    async fn get_cli_channel_returns_ok() {
        let result = get_channel(Path("cli".into())).await;
        let response = result.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_unknown_channel_returns_not_found() {
        let result = get_channel(Path("discord".into())).await;
        let response = result.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_message_returns_not_implemented() {
        let req = TestMessageRequest {
            message: "hello".into(),
        };
        let result = test_channel(Path("slack".into()), Json(req)).await;
        let response = result.into_response();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    }
}
