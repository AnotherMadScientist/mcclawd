use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use mcclawd_core::McclawdConfig;
use serde::{Deserialize, Serialize};

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
    pub default_tool_profile: Option<mcclawd_core::config::ToolProfile>,
}

/// PUT /api/config — validate, merge into existing config, persist to disk, update in-memory.
pub async fn put_config(
    State(state): State<AppState>,
    Json(body): Json<ConfigUpdate>,
) -> Result<Json<McclawdConfig>, (StatusCode, String)> {
    // --- Merge into a clone, validate, then commit ---
    let mut config = state.config.write().await;
    let mut updated = config.clone();

    if let Some(model) = body.model {
        updated.agent.model = model;
    }
    if let Some(max_turns) = body.max_turns {
        updated.agent.max_turns = max_turns;
    }
    if let Some(ws) = body.default_workspace {
        updated.agent.default_workspace = ws;
    }
    if let Some(tp) = body.default_tool_profile {
        updated.agent.default_tool_profile = tp;
    }

    // --- Validate merged config ---
    updated
        .validate()
        .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;

    *config = updated;

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

// ---------------------------------------------------------------------------
// Per-key config routes: GET/PUT/DELETE /api/config/:key
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ConfigEntry {
    pub key: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct ConfigValueBody {
    pub value: serde_json::Value,
}

/// GET /api/config/keys — list all raw config key-value pairs from DB.
pub async fn list_config_keys(
    State(state): State<AppState>,
) -> Result<Json<Vec<ConfigEntry>>, (StatusCode, String)> {
    let rows = state
        .pg_store
        .load_config("admin")
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let entries = rows.into_iter().map(|(k, v)| ConfigEntry { key: k, value: v }).collect();
    Ok(Json(entries))
}

/// GET /api/config/keys/:key — get a single config value from DB.
pub async fn get_config_key(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<ConfigEntry>, (StatusCode, String)> {
    match state.pg_store.get_config_key("admin", &key).await {
        Ok(Some(value)) => Ok(Json(ConfigEntry { key, value })),
        Ok(None) => Err((StatusCode::NOT_FOUND, format!("config key '{key}' not found"))),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

/// PUT /api/config/keys/:key — upsert a single config value in DB.
pub async fn put_config_key(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(body): Json<ConfigValueBody>,
) -> Result<Json<ConfigEntry>, (StatusCode, String)> {
    state
        .pg_store
        .save_config("admin", &key, &body.value)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(ConfigEntry { key, value: body.value }))
}

/// DELETE /api/config/keys/:key — remove a config key from DB.
pub async fn delete_config_key(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .pg_store
        .delete_config_key("admin", &key)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}
