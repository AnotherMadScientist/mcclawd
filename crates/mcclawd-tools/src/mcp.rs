//! MCP client — connects to AgentGateway via StreamableHttp, discovers and calls tools.
//!
//! Usage:
//! ```rust,ignore
//! let client = McpClient::new("http://localhost:3000");
//! let conn = client.connect().await?;
//! let tool_names = conn.tool_names();
//! conn.shutdown().await?;
//! ```

use anyhow::{Context, Result};
use rmcp::{
    model::{CallToolRequestParam, CallToolResult, Tool as McpToolDef},
    service::{Peer, RoleClient, RunningService, ServiceExt},
    transport::StreamableHttpClientTransport,
};

/// Client that connects to AgentGateway and discovers MCP tools.
pub struct McpClient {
    url: String,
}

/// Active MCP connection with discovered tools.
pub struct McpConnection {
    service: RunningService<RoleClient, ()>,
    tools: Vec<McpToolDef>,
}

impl McpClient {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Connect to AgentGateway and discover available MCP tools.
    pub async fn connect(&self) -> Result<McpConnection> {
        let transport = StreamableHttpClientTransport::from_uri(self.url.clone());

        let service = ()
            .serve(transport)
            .await
            .map_err(|e| anyhow::anyhow!("failed to connect to AgentGateway: {e}"))?;

        let tool_list = service
            .peer()
            .list_tools(None)
            .await
            .context("failed to list MCP tools")?;

        let tools = tool_list.tools;
        tracing::info!("Discovered {} MCP tools from AgentGateway", tools.len());
        for tool in &tools {
            tracing::debug!(
                "  tool: {} — {}",
                tool.name,
                tool.description.as_deref().unwrap_or("")
            );
        }

        Ok(McpConnection { service, tools })
    }
}

impl McpConnection {
    pub fn tools(&self) -> &[McpToolDef] {
        &self.tools
    }

    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.tools.iter().map(|t| t.name.to_string()).collect()
    }

    /// Get a reference to the peer for Rig integration.
    pub fn peer(&self) -> &Peer<RoleClient> {
        self.service.peer()
    }

    /// Call a tool by name with JSON arguments.
    pub async fn call_tool(&self, name: &str, args: serde_json::Value) -> Result<String> {
        let params = CallToolRequestParam {
            name: name.to_string().into(),
            arguments: args.as_object().cloned(),
            task: None,
        };

        let result: CallToolResult = self
            .service
            .peer()
            .call_tool(params)
            .await
            .context("MCP tool call failed")?;

        let text: String = result
            .content
            .iter()
            .filter_map(|c| {
                if let rmcp::model::RawContent::Text(t) = &c.raw {
                    Some(t.text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(text)
    }

    /// Shut down the MCP connection gracefully.
    pub async fn shutdown(self) -> Result<()> {
        self.service
            .cancel()
            .await
            .map_err(|e| anyhow::anyhow!("shutdown error: {e}"))?;
        Ok(())
    }
}
