//! MCP tool integration — connects to AgentGateway and provides tools for the Rig agent.
//!
//! Two connection paths:
//! - `connect_from_env()`: Inside a Docker container, reads MCCLAWD_GATEWAY_URL
//!   and MCCLAWD_ALLOWED_TOOLS from environment. Filters tools by allowed prefixes.
//! - `connect_mcp_tools()`: Direct connection using config (host/dev mode fallback).

use anyhow::Result;
use mcclawd_core::config::McclawdConfig;
use mcclawd_tools::mcp::McpClient;

/// A live MCP connection bundle: tools, peer, and the underlying connection
/// (kept alive so the peer remains valid).
pub struct McpBundle {
    pub tools: Vec<rmcp::model::Tool>,
    pub peer: rmcp::service::Peer<rmcp::service::RoleClient>,
    _conn: mcclawd_tools::mcp::McpConnection,
}

/// Connect to AgentGateway from inside a container using environment variables.
///
/// Reads `MCCLAWD_GATEWAY_URL` for the gateway address and `MCCLAWD_ALLOWED_TOOLS`
/// (comma-separated prefixes) to filter which tools the agent can access.
///
/// Returns `None` if the env vars are not set (not running in a container).
pub async fn connect_from_env() -> Result<Option<Vec<McpBundle>>> {
    let gateway_url = match std::env::var("MCCLAWD_GATEWAY_URL") {
        Ok(url) => url,
        Err(_) => return Ok(None),
    };

    let allowed_tools: Vec<String> = std::env::var("MCCLAWD_ALLOWED_TOOLS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let client = McpClient::new(&gateway_url);
    match client.connect().await {
        Ok(conn) => {
            let all_tools = conn.tools().to_vec();
            let peer = conn.peer().clone();

            // Filter tools by allowed prefixes (or allow all if "*" or empty)
            let allow_all = allowed_tools.is_empty()
                || allowed_tools.iter().any(|t| t == "*");

            let filtered_tools: Vec<rmcp::model::Tool> = if allow_all {
                all_tools
            } else {
                all_tools
                    .into_iter()
                    .filter(|tool| {
                        let name: &str = &tool.name;
                        allowed_tools.iter().any(|prefix| name.starts_with(prefix))
                    })
                    .collect()
            };

            tracing::info!(
                "Connected to AgentGateway at {gateway_url}, {} tools available ({} after filtering)",
                conn.tool_count(),
                filtered_tools.len(),
            );

            Ok(Some(vec![McpBundle {
                tools: filtered_tools,
                peer,
                _conn: conn,
            }]))
        }
        Err(e) => {
            tracing::warn!(
                "AgentGateway not available at {gateway_url}: {e}"
            );
            Ok(Some(vec![]))
        }
    }
}

/// Connect to AgentGateway and return MCP tools + peer for the Rig agent.
/// Returns empty vec if AgentGateway is unavailable (agent runs without MCP tools).
pub async fn connect_mcp_tools(config: &McclawdConfig) -> Result<Vec<McpBundle>> {
    let client = McpClient::new(&config.mcp.agentgateway_url);
    match client.connect().await {
        Ok(conn) => {
            tracing::info!(
                "Connected to AgentGateway at {}, {} MCP tools available",
                config.mcp.agentgateway_url,
                conn.tool_count()
            );
            let tools = conn.tools().to_vec();
            let peer = conn.peer().clone();
            Ok(vec![McpBundle {
                tools,
                peer,
                _conn: conn,
            }])
        }
        Err(e) => {
            tracing::warn!(
                "AgentGateway not available at {}, running without MCP tools: {e}",
                config.mcp.agentgateway_url
            );
            Ok(vec![])
        }
    }
}
