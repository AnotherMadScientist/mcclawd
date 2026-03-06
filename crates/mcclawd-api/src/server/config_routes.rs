use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use mcclawd_core::McclawdConfig;
use serde::Deserialize;

use super::state::AppState;

/// GET /api/config — returns current config as JSON
pub async fn get_config(State(state): State<AppState>) -> Json<McclawdConfig> {
    let config = state.config.read().await;
    Json(config.clone())
}

/// Request body for PUT /api/config — all fields optional (partial update / merge).
#[derive(Debug, Deserialize)]
pub struct ConfigUpdate {
    pub model: Option<String>,
    pub max_turns: Option<usize>,
    pub default_workspace: Option<String>,
}

/// PUT /api/config — validate, merge into existing config, persist to disk, update in-memory.
pub async fn put_config(
    State(state): State<AppState>,
    Json(body): Json<ConfigUpdate>,
) -> Result<Json<McclawdConfig>, (StatusCode, String)> {
    // --- Validate ---
    if let Some(ref model) = body.model {
        if model.trim().is_empty() {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                "model must be a non-empty string".to_string(),
            ));
        }
    }
    if let Some(max_turns) = body.max_turns {
        if !(1..=100).contains(&max_turns) {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                "max_turns must be between 1 and 100".to_string(),
            ));
        }
    }
    if let Some(ref ws) = body.default_workspace {
        if ws.trim().is_empty() {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                "default_workspace must be non-empty".to_string(),
            ));
        }
    }

    // --- Merge into existing config ---
    let mut config = state.config.write().await;

    if let Some(model) = body.model {
        config.agent.model = model;
    }
    if let Some(max_turns) = body.max_turns {
        config.agent.max_turns = max_turns;
    }
    if let Some(ws) = body.default_workspace {
        config.agent.default_workspace = ws;
    }

    // --- Persist to disk ---
    if let Some(ref config_path) = state.config_path {
        config
            .save(config_path)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    // Fire-and-forget: persist to Postgres
    let store = state.pg_store.clone();
    let config_value = serde_json::to_value(&*config).unwrap_or_default();
    tokio::spawn(async move {
        if let Err(e) = store.save_config("admin", "main", &config_value).await {
            tracing::warn!("Failed to persist config to DB: {e}");
        }
    });

    Ok(Json(config.clone()))
}
