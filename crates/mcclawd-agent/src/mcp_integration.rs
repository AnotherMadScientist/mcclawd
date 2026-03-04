//! MCP tool integration — connects to AgentGateway and provides tools for the Rig agent.

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
