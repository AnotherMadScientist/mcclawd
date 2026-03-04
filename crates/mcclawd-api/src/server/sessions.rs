//! Session management API route handlers — Phase 3b placeholders.

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

/// GET /api/sessions — list recent sessions (placeholder: returns empty array).
pub async fn list_sessions() -> Json<Vec<SessionInfo>> {
    // Phase 3b placeholder — will query SessionStore when wired up
    Json(vec![])
}

/// GET /api/sessions/{id} — get session details (placeholder: returns 404).
pub async fn get_session(Path(id): Path<String>) -> impl IntoResponse {
    tracing::debug!(session_id = %id, "Session lookup (placeholder — not found)");
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Session not found".into(),
        }),
    )
}

/// GET /api/sessions/{id}/turns — get turns for a session (placeholder: returns 404).
pub async fn get_session_turns(Path(id): Path<String>) -> impl IntoResponse {
    tracing::debug!(session_id = %id, "Session turns lookup (placeholder — not found)");
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Session not found".into(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_sessions_returns_empty_array() {
        let result = list_sessions().await;
        assert!(result.0.is_empty());
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
