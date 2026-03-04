use mcclawd_tools::mcp::McpClient;

#[tokio::test]
async fn mcp_client_creates_from_url() {
    let client = McpClient::new("http://localhost:3000");
    assert_eq!(client.url(), "http://localhost:3000");
}

#[tokio::test]
async fn mcp_client_connect_fails_when_no_server() {
    let client = McpClient::new("http://localhost:19999");
    let result = client.connect().await;
    assert!(result.is_err(), "Should fail when no server is running");
}
