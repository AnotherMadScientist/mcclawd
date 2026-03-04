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

/// PUT /api/config — stub, logs and returns 204
pub async fn put_config(
    Json(body): Json<serde_json::Value>,
) -> StatusCode {
    tracing::info!("Config update requested: {body}");
    StatusCode::NO_CONTENT
}
