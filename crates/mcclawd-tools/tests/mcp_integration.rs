//! Integration tests for MCP tool discovery via rmcp StreamableHttp.
//!
//! Direct tests: Connect to supergateway MCP servers on exposed ports.
//!   Requires: `docker run -d --rm -v mcclawd_mcclawd-data:/data -p 8003:8000 mcclawd-mcp-filesystem`
//!
//! AgentGateway tests: Connect via AgentGateway proxy on port 3000.
//!   Requires: `docker compose up -d`
//!   Note: AgentGateway proxy may have session/connection issues with rmcp 0.13.
//!
//! Run: `cargo test -p mcclawd-tools --test mcp_integration -- --ignored --nocapture`

use mcclawd_tools::mcp::McpClient;

#[tokio::test]
#[ignore]
async fn discovers_tools_from_supergateway_direct() {
    let client = McpClient::new("http://localhost:8003");
    let conn = client
        .connect()
        .await
        .expect("Supergateway filesystem should be running on port 8003");

    let tools = conn.tools();
    assert!(!tools.is_empty(), "Should discover filesystem tools");

    let names = conn.tool_names();
    println!("Discovered {} MCP tools: {:?}", tools.len(), names);

    assert!(
        names
            .iter()
            .any(|n| n.contains("read") || n.contains("list") || n.contains("directory")),
        "Should find filesystem tools, got: {:?}",
        names
    );

    conn.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn filesystem_tool_lists_data_directory() {
    let client = McpClient::new("http://localhost:8003");
    let conn = client
        .connect()
        .await
        .expect("Supergateway filesystem should be running on port 8003");

    let result = conn
        .call_tool("list_directory", serde_json::json!({ "path": "/data" }))
        .await;

    println!("list_directory result: {:?}", result);
    assert!(result.is_ok(), "list_directory should succeed: {:?}", result);

    conn.shutdown().await.unwrap();
}
