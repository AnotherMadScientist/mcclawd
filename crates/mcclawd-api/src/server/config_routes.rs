use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use mcclawd_core::McclawdConfig;

use super::state::AppState;

/// GET /api/config — returns current config as JSON
pub async fn get_config(State(state): State<AppState>) -> Json<McclawdConfig> {
    let config = state.config.read().await;
    Json(config.clone())
}

/// PUT /api/config — not yet implemented (Phase 1)
pub async fn put_config(
    Json(_body): Json<serde_json::Value>,
) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
