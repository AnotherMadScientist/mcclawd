//! MCP server API route handlers — list, add, remove, restart, status.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use mcclawd_core::config::McpServerConfig;
use serde::Deserialize;

use super::mcp_lifecycle::McpServerStatus;
use super::state::AppState;

/// GET /api/mcp/servers — returns MCP servers from config.
pub async fn list_mcp_servers(State(state): State<AppState>) -> Json<Vec<McpServerConfig>> {
    let config = state.config.read().await;
    Json(config.mcp.servers.clone())
}

/// Request body for adding a new MCP server.
#[derive(Debug, Deserialize)]
pub struct AddMcpServerRequest {
    pub name: String,
    pub image: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub volumes: Vec<String>,
}

fn default_port() -> u16 {
    8080
}

/// POST /api/mcp/servers — add a new MCP server to config and optionally start it.
pub async fn add_mcp_server(
    State(state): State<AppState>,
    Json(req): Json<AddMcpServerRequest>,
) -> Result<(StatusCode, Json<McpServerConfig>), (StatusCode, String)> {
    // Check for duplicate name
    let mut config = state.config.write().await;
    if config.mcp.servers.iter().any(|s| s.name == req.name) {
        return Err((
            StatusCode::CONFLICT,
            format!("MCP server '{}' already exists", req.name),
        ));
    }

    let server_config = McpServerConfig {
        name: req.name,
        image: req.image,
        port: req.port,
        env: req.env,
        volumes: req.volumes,
    };

    config.mcp.servers.push(server_config.clone());

    // Persist config to disk
    if let Some(ref config_path) = state.config_path {
        config
            .save(config_path)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to save config: {e}")))?;
    }

    // Try to start the container if Docker is available
    if let Some(ref lifecycle) = state.mcp_lifecycle {
        if let Err(e) = lifecycle.start_server(&server_config).await {
            tracing::warn!(
                server = %server_config.name,
                error = %e,
                "Failed to start MCP server container (config still saved)"
            );
        }
    }

    // Fire-and-forget: persist to Postgres
    let store = state.pg_store.clone();
    let srv_name = server_config.name.clone();
    let config_json = serde_json::to_value(&server_config).unwrap_or_default();
    tokio::spawn(async move {
        if let Err(e) = store.save_mcp_server("admin", &srv_name, &config_json).await {
            tracing::warn!("Failed to persist MCP server to DB: {e}");
        }
    });

    Ok((StatusCode::CREATED, Json(server_config)))
}

/// DELETE /api/mcp/servers/{name} — remove an MCP server from config and stop its container.
pub async fn remove_mcp_server(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut config = state.config.write().await;
    let initial_len = config.mcp.servers.len();
    config.mcp.servers.retain(|s| s.name != name);

    if config.mcp.servers.len() == initial_len {
        return Err((
            StatusCode::NOT_FOUND,
            format!("MCP server '{}' not found", name),
        ));
    }

    // Persist config to disk
    if let Some(ref config_path) = state.config_path {
        config
            .save(config_path)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to save config: {e}")))?;
    }

    // Stop and remove the container if Docker is available
    if let Some(ref lifecycle) = state.mcp_lifecycle {
        if let Err(e) = lifecycle.remove_server(&name).await {
            tracing::warn!(
                server = %name,
                error = %e,
                "Failed to remove MCP server container (config already updated)"
            );
        }
    }

    // Fire-and-forget: remove from Postgres
    let store = state.pg_store.clone();
    let mcp_name = name.clone();
    tokio::spawn(async move {
        if let Err(e) = store.delete_mcp_server("admin", &mcp_name).await {
            tracing::warn!("Failed to delete MCP server from DB: {e}");
        }
    });

    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/mcp/servers/{name}/restart — restart an MCP server container.
pub async fn restart_mcp_server(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let config = state.config.read().await;
    let server_config = config
        .mcp
        .servers
        .iter()
        .find(|s| s.name == name)
        .cloned();

    let server_config = match server_config {
        Some(c) => c,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                format!("MCP server '{}' not found", name),
            ));
        }
    };
    drop(config);

    match &state.mcp_lifecycle {
        Some(lifecycle) => {
            lifecycle
                .restart_server(&server_config)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to restart: {e}"),
                    )
                })?;
            Ok(StatusCode::NO_CONTENT)
        }
        None => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "Docker not available".to_string(),
        )),
    }
}

/// GET /api/mcp/servers/{name}/status — get container status for an MCP server.
pub async fn mcp_server_status(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<McpServerStatus>, (StatusCode, String)> {
    let config = state.config.read().await;
    let server_config = config
        .mcp
        .servers
        .iter()
        .find(|s| s.name == name)
        .cloned();

    let server_config = match server_config {
        Some(c) => c,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                format!("MCP server '{}' not found", name),
            ));
        }
    };
    drop(config);

    match &state.mcp_lifecycle {
        Some(lifecycle) => {
            let status = lifecycle.server_status(&name, &server_config).await;
            Ok(Json(status))
        }
        None => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "Docker not available".to_string(),
        )),
    }
}
