//! Integration test: connects to AgentGateway and discovers MCP tools.
//!
//! Requires: `docker compose up -d` on localhost:3000.
//! Run: `cargo test -p mcclawd-tools --test mcp_integration -- --ignored --nocapture`

use mcclawd_tools::mcp::McpClient;

#[tokio::test]
#[ignore]
async fn discovers_mcp_tools_from_agentgateway() {
    let client = McpClient::new("http://localhost:3000");
    let conn = client
        .connect()
        .await
        .expect("AgentGateway should be running (docker compose up -d)");

    let tools = conn.tools();
    assert!(!tools.is_empty(), "Should discover MCP tools");

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
    let client = McpClient::new("http://localhost:3000");
    let conn = client
        .connect()
        .await
        .expect("AgentGateway should be running");

    let result = conn
        .call_tool("list_directory", serde_json::json!({ "path": "/data" }))
        .await;

    println!("list_directory result: {:?}", result);
    assert!(result.is_ok(), "list_directory should succeed: {:?}", result);

    conn.shutdown().await.unwrap();
}
