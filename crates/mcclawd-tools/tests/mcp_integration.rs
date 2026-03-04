//! Integration tests for MCP tool discovery via AgentGateway.
//!
//! All MCP servers run as stdio subprocesses inside the AgentGateway container.
//! Connect via AgentGateway on port 3000.
//!
//! Requires: `docker compose up -d`
//! Run: `cargo test -p mcclawd-tools --test mcp_integration -- --ignored --nocapture`

use mcclawd_tools::mcp::McpClient;

#[tokio::test]
#[ignore]
async fn discovers_tools_via_agentgateway() {
    let client = McpClient::new("http://localhost:3000");
    let conn = client
        .connect()
        .await
        .expect("AgentGateway should be running on port 3000");

    let tools = conn.tools();
    assert!(!tools.is_empty(), "Should discover MCP tools");

    let names = conn.tool_names();
    println!("Discovered {} MCP tools: {:?}", tools.len(), names);

    // Should have tools from all 3 servers (filesystem, langextract, scrapling)
    assert!(
        names.iter().any(|n| n.contains("filesystem")),
        "Should find filesystem tools, got: {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n.contains("langextract")),
        "Should find langextract tools, got: {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n.contains("scrapling")),
        "Should find scrapling tools, got: {:?}",
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
        .expect("AgentGateway should be running on port 3000");

    let result = conn
        .call_tool(
            "filesystem_list_directory",
            serde_json::json!({ "path": "/data" }),
        )
        .await;

    println!("list_directory result: {:?}", result);
    assert!(result.is_ok(), "list_directory should succeed: {:?}", result);

    conn.shutdown().await.unwrap();
}
