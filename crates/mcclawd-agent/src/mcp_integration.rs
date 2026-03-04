//! MCP tool integration — connects to AgentGateway and provides tools for the Rig agent.

use anyhow::Result;
use mcclawd_core::config::McclawdConfig;
use mcclawd_tools::mcp::McpClient;

/// Connect to AgentGateway and return MCP tools + peer for the Rig agent.
/// Returns None if AgentGateway is unavailable (agent runs without MCP tools).
pub async fn connect_mcp_tools(
    config: &McclawdConfig,
) -> Result<Option<(Vec<rmcp::model::Tool>, rmcp::service::Peer<rmcp::service::RoleClient>)>> {
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
            Ok(Some((tools, peer)))
        }
        Err(e) => {
            tracing::warn!(
                "AgentGateway not available at {}, running without MCP tools: {e}",
                config.mcp.agentgateway_url
            );
            Ok(None)
        }
    }
}
