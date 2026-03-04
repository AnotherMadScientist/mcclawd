use axum::{extract::State, Json};
use mcclawd_core::config::McpServerConfig;

use super::state::AppState;

/// GET /api/mcp/servers — returns MCP servers from config
pub async fn list_mcp_servers(State(state): State<AppState>) -> Json<Vec<McpServerConfig>> {
    let config = state.config.read().await;
    Json(config.mcp.servers.clone())
}
