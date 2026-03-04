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
fn test_load_config_from_toml() {
    let mut f = NamedTempFile::new().unwrap();
    writeln!(
        f,
        r#"
[agent]
max_turns = 10
model = "claude-opus-4-5"
"#
    )
    .unwrap();

    let config = McclawdConfig::load(f.path()).unwrap();
    assert_eq!(config.agent.max_turns, 10);
    assert_eq!(config.agent.model, "claude-opus-4-5");
}

#[test]
fn test_load_missing_config_returns_default() {
    let config =
        McclawdConfig::load(std::path::Path::new("/nonexistent/config.toml")).unwrap();
    assert_eq!(config.agent.max_turns, 20);
}
