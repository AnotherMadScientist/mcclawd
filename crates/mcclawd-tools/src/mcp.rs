//! MCP client connection wrapper.
//!
//! Phase 0: placeholder stubs. The full MCP infrastructure is being implemented
//! in a separate workstream and will land in Phase 1.

/// Placeholder for an MCP client connection (stdio or SSE transport).
pub struct McpConnection;

impl McpConnection {
    /// Connect to an MCP server over SSE (placeholder).
    pub async fn connect_sse(_url: &str) -> anyhow::Result<()> {
        tracing::info!(url = %_url, "MCP SSE connection placeholder — not yet implemented");
        Ok(())
    }

    /// Connect to an MCP server over stdio (placeholder).
    pub async fn connect_stdio(_command: &str, _args: &[&str]) -> anyhow::Result<()> {
        tracing::info!(command = %_command, "MCP stdio connection placeholder — not yet implemented");
        Ok(())
    }
}
