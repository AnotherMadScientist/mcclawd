use mcclawd_core::config::McclawdConfig;
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_default_config() {
    let config = McclawdConfig::default();
    assert_eq!(config.agent.max_turns, 20);
    assert_eq!(config.agent.default_workspace, "default");
}

#[test]
fn test_load_config_from_json5() {
    let mut f = NamedTempFile::new().unwrap();
    writeln!(
        f,
        r#"{{
            "agent": {{
                "max_turns": 10,
                "model": "claude-opus-4-5"
            }}
        }}"#
    )
    .unwrap();

    let config = McclawdConfig::load(f.path()).unwrap();
    assert_eq!(config.agent.max_turns, 10);
    assert_eq!(config.agent.model, "claude-opus-4-5");
}

#[test]
fn test_load_missing_config_returns_default() {
    let config =
        McclawdConfig::load(std::path::Path::new("/nonexistent/mcclawd.json")).unwrap();
    assert_eq!(config.agent.max_turns, 20);
}

#[test]
fn config_has_agentgateway_url_default() {
    let config = McclawdConfig::default();
    assert_eq!(config.mcp.agentgateway_url, "http://localhost:3000");
}

#[test]
fn config_parses_agentgateway_url() {
    let json_str = r#"{ "mcp": { "agentgateway_url": "http://custom-host:9090" } }"#;
    let config: McclawdConfig = json5::from_str(json_str).unwrap();
    assert_eq!(config.mcp.agentgateway_url, "http://custom-host:9090");
}

#[test]
fn config_has_default_mcp_servers() {
    let config = McclawdConfig::default();
    assert_eq!(config.mcp.servers.len(), 3);
    assert!(config.mcp.servers.iter().any(|s| s.name == "langextract"));
    assert!(config.mcp.servers.iter().any(|s| s.name == "scrapling"));
    assert!(config.mcp.servers.iter().any(|s| s.name == "filesystem"));
}

#[test]
fn mcp_server_config_has_image_and_port() {
    let config = McclawdConfig::default();
    let fs = config
        .mcp
        .servers
        .iter()
        .find(|s| s.name == "filesystem")
        .unwrap();
    assert!(fs.image.contains("mcp-filesystem"));
    assert_eq!(fs.port, 8003);
}
