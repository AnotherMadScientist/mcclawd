//! Session management API route handlers.
//!
//! Sessions track conversations between a user and McClawd across channels.
//! Multi-channel session management is Phase 3b; currently sessions are
//! tracked per-task through the task system.

use axum::{extract::Path, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

/// Summary of a session for the list endpoint.
#[derive(Debug, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub channel_id: String,
    pub peer_id: String,
    pub platform: String,
    pub started_at: String,
    pub ended_at: Option<String>,
}

/// Error body returned as JSON.
#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

/// GET /api/sessions — list recent sessions.
///
/// Multi-channel sessions are not yet available. Task-level sessions
/// are accessible via the /api/tasks endpoints.
pub async fn list_sessions() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "sessions": [],
            "note": "Multi-channel session management is not yet available. Use /api/tasks for task-level session tracking."
        })),
    )
}

/// GET /api/sessions/{id} — get session details.
pub async fn get_session(Path(id): Path<String>) -> impl IntoResponse {
    tracing::debug!(session_id = %id, "Session lookup");
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: format!("Session '{id}' not found. Multi-channel sessions are not yet available."),
        }),
    )
}

/// GET /api/sessions/{id}/turns — get turns for a session.
pub async fn get_session_turns(Path(id): Path<String>) -> impl IntoResponse {
    tracing::debug!(session_id = %id, "Session turns lookup");
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: format!("Session '{id}' not found. Multi-channel sessions are not yet available."),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_sessions_returns_ok() {
        let result = list_sessions().await;
        let response = result.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_session_returns_not_found() {
        let result = get_session(Path("nonexistent".into())).await;
        let response = result.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_session_turns_returns_not_found() {
        let result = get_session_turns(Path("nonexistent".into())).await;
        let response = result.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
