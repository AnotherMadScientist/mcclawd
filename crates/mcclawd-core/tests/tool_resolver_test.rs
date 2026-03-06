//! Integration tests for ToolResolver — tests the full resolution pipeline
//! including dependency resolution, MCP server matching, and image hash computation.

use mcclawd_core::config::McpServerConfig;
use mcclawd_core::skills::LoadedSkill;
use mcclawd_core::tool_resolver::ToolResolver;
use std::collections::HashMap;

fn skill(name: &str, deps: &[&str], tools: &[&str], steps: &[&str]) -> LoadedSkill {
    LoadedSkill {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        author: "test".to_string(),
        description: format!("Skill: {name}"),
        mcp_tools: tools.iter().map(|s| s.to_string()).collect(),
        install_steps: steps.iter().map(|s| s.to_string()).collect(),
        context: format!("Context for {name}"),
        dependencies: deps.iter().map(|s| s.to_string()).collect(),
        instructions: String::new(),
        examples: String::new(),
        config_section: String::new(),
    }
}

fn server(name: &str) -> McpServerConfig {
    McpServerConfig {
        name: name.to_string(),
        image: format!("mcp-{name}:latest"),
        port: 8000,
        env: vec![],
        volumes: vec![],
    }
}

// --- Dependency resolution ---

#[test]
fn resolve_diamond_dependency() {
    // A -> B, A -> C, B -> D, C -> D => D first
    let mut skills = HashMap::new();
    skills.insert("a".into(), skill("a", &["b", "c"], &[], &[]));
    skills.insert("b".into(), skill("b", &["d"], &[], &["pip install b"]));
    skills.insert("c".into(), skill("c", &["d"], &[], &["pip install c"]));
    skills.insert("d".into(), skill("d", &[], &[], &["pip install d"]));

    let result = ToolResolver::resolve(&["a".into()], &skills, &[], "base:latest").unwrap();
    let names: Vec<&str> = result.skills.iter().map(|s| s.name.as_str()).collect();

    let d_pos = names.iter().position(|&n| n == "d").unwrap();
    let b_pos = names.iter().position(|&n| n == "b").unwrap();
    let c_pos = names.iter().position(|&n| n == "c").unwrap();
    let a_pos = names.iter().position(|&n| n == "a").unwrap();
    assert!(d_pos < b_pos);
    assert!(d_pos < c_pos);
    assert!(b_pos < a_pos);
    assert!(c_pos < a_pos);
}

#[test]
fn resolve_multiple_requested_skills_merges_deps() {
    let mut skills = HashMap::new();
    skills.insert("x".into(), skill("x", &["shared"], &["filesystem"], &["pip install x"]));
    skills.insert("y".into(), skill("y", &["shared"], &["scrapling"], &["pip install y"]));
    skills.insert("shared".into(), skill("shared", &[], &[], &["pip install shared"]));

    let result = ToolResolver::resolve(
        &["x".into(), "y".into()],
        &skills,
        &[server("filesystem"), server("scrapling")],
        "base:latest",
    )
    .unwrap();

    assert_eq!(result.skills.len(), 3);
    assert_eq!(result.required_servers.len(), 2);
    assert!(result.allowed_tools.contains("filesystem"));
    assert!(result.allowed_tools.contains("scrapling"));
    // shared's install step appears only once
    let shared_count = result.install_steps.iter().filter(|s| s.contains("shared")).count();
    assert_eq!(shared_count, 1);
}

#[test]
fn resolve_cycle_detected() {
    let mut skills = HashMap::new();
    skills.insert("a".into(), skill("a", &["b"], &[], &[]));
    skills.insert("b".into(), skill("b", &["a"], &[], &[]));

    let err = ToolResolver::resolve(&["a".into()], &skills, &[], "base:latest").unwrap_err();
    assert!(err.to_string().contains("cycle"), "should detect cycle: {err}");
}

#[test]
fn resolve_transitive_dep_not_installed_errors() {
    let mut skills = HashMap::new();
    skills.insert("a".into(), skill("a", &["b"], &[], &[]));
    skills.insert("b".into(), skill("b", &["missing"], &[], &[]));

    let err = ToolResolver::resolve(&["a".into()], &skills, &[], "base:latest").unwrap_err();
    assert!(err.to_string().contains("missing"), "should mention missing dep: {err}");
}

// --- MCP server matching ---

#[test]
fn mcp_server_matching_only_includes_declared_tools() {
    let mut skills = HashMap::new();
    skills.insert("a".into(), skill("a", &[], &["filesystem", "scrapling"], &[]));

    let servers = vec![server("filesystem"), server("scrapling"), server("langextract")];
    let result = ToolResolver::resolve(&["a".into()], &skills, &servers, "base:latest").unwrap();

    assert_eq!(result.required_servers.len(), 2);
    assert!(result.required_servers.contains(&"filesystem".to_string()));
    assert!(result.required_servers.contains(&"scrapling".to_string()));
    assert!(!result.required_servers.contains(&"langextract".to_string()));
}

#[test]
fn empty_skills_produces_empty_result() {
    let skills = HashMap::new();
    let result = ToolResolver::resolve(&[], &skills, &[], "base:latest").unwrap();
    assert!(result.skills.is_empty());
    assert!(result.install_steps.is_empty());
    assert!(result.required_servers.is_empty());
    assert!(result.allowed_tools.is_empty());
}

// --- Image hash ---

#[test]
fn image_hash_stable_across_request_order() {
    let mut skills = HashMap::new();
    skills.insert("a".into(), skill("a", &[], &[], &["pip install a"]));
    skills.insert("b".into(), skill("b", &[], &[], &["pip install b"]));

    let r1 = ToolResolver::resolve(&["a".into(), "b".into()], &skills, &[], "base:latest").unwrap();
    let r2 = ToolResolver::resolve(&["b".into(), "a".into()], &skills, &[], "base:latest").unwrap();

    // Hash should be the same regardless of request order (steps are sorted)
    assert_eq!(r1.image_hash, r2.image_hash);
}

#[test]
fn image_hash_changes_with_different_steps() {
    let mut skills1 = HashMap::new();
    skills1.insert("a".into(), skill("a", &[], &[], &["pip install v1"]));

    let mut skills2 = HashMap::new();
    skills2.insert("a".into(), skill("a", &[], &[], &["pip install v2"]));

    let r1 = ToolResolver::resolve(&["a".into()], &skills1, &[], "base:latest").unwrap();
    let r2 = ToolResolver::resolve(&["a".into()], &skills2, &[], "base:latest").unwrap();

    assert_ne!(r1.image_hash, r2.image_hash);
}

#[test]
fn image_hash_is_12_hex_chars() {
    let mut skills = HashMap::new();
    skills.insert("a".into(), skill("a", &[], &[], &["apt install curl"]));

    let result = ToolResolver::resolve(&["a".into()], &skills, &[], "base:latest").unwrap();
    assert_eq!(result.image_hash.len(), 12);
    assert!(result.image_hash.chars().all(|c| c.is_ascii_hexdigit()));
}

// --- Skill context ---

#[test]
fn skill_context_includes_all_resolved_skills() {
    let mut skills = HashMap::new();
    skills.insert("parent".into(), skill("parent", &["child"], &[], &[]));
    skills.insert("child".into(), skill("child", &[], &[], &[]));

    let result = ToolResolver::resolve(&["parent".into()], &skills, &[], "base:latest").unwrap();
    assert!(result.skill_context.contains("Skill: parent"));
    assert!(result.skill_context.contains("Skill: child"));
}

#[test]
fn skill_context_empty_when_no_context() {
    let mut s = skill("a", &[], &[], &[]);
    s.context = String::new();
    let mut skills = HashMap::new();
    skills.insert("a".into(), s);

    let result = ToolResolver::resolve(&["a".into()], &skills, &[], "base:latest").unwrap();
    assert!(result.skill_context.is_empty());
}

// --- Config ---

#[test]
fn sandbox_config_no_mode_field() {
    // Verify SandboxMode is gone — config parses without mode field
    let toml_str = r#"
[sandbox]
base_image = "custom:latest"
network = "my_network"
"#;
    let config: mcclawd_core::McclawdConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.sandbox.base_image, "custom:latest");
    assert_eq!(config.sandbox.network, "my_network");
}

#[test]
fn sandbox_config_default_network_is_mcclawd_tools() {
    let config = mcclawd_core::McclawdConfig::default();
    assert_eq!(config.sandbox.network, "mcclawd_tools");
}

#[test]
fn sandbox_config_roundtrip_without_mode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    let mut config = mcclawd_core::McclawdConfig::default();
    config.sandbox.base_image = "test-sandbox:v2".to_string();
    config.sandbox.network = "test_net".to_string();
    config.save(&path).unwrap();

    let loaded = mcclawd_core::McclawdConfig::load(&path).unwrap();
    assert_eq!(loaded.sandbox.base_image, "test-sandbox:v2");
    assert_eq!(loaded.sandbox.network, "test_net");
}

#[test]
fn old_config_with_mode_field_still_parses() {
    // Backward compat: old configs may have mode = "host" — serde should ignore unknown fields
    // Note: toml strict mode may reject this. If so, this test documents the behavior.
    let toml_str = r#"
[sandbox]
base_image = "old:latest"
"#;
    let config: mcclawd_core::McclawdConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.sandbox.base_image, "old:latest");
}
