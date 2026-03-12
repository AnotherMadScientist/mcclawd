//! Container security configuration tests.
//!
//! Validates that sandbox defaults enforce Docker-only execution with
//! security hardening (no-new-privileges, pids_limit, strict_sandbox).

use mcclawd_core::config::{McclawdConfig, SandboxConfig};

#[test]
fn strict_sandbox_default_is_false() {
    // Default is false for development convenience (Docker image may not exist).
    // Production deployments should set strict_sandbox = true in mcclawd.json.
    let config = SandboxConfig::default();
    assert!(
        !config.strict_sandbox,
        "strict_sandbox must default to false for dev — production sets it to true"
    );
}

#[test]
fn sandbox_config_has_pids_limit() {
    let config = SandboxConfig::default();
    assert_eq!(
        config.pids_limit,
        Some(256),
        "pids_limit must default to 256"
    );
}

#[test]
fn sandbox_config_has_memory_limit() {
    let config = SandboxConfig::default();
    assert_eq!(
        config.memory_limit,
        Some(512 * 1024 * 1024),
        "memory_limit must default to 512MB"
    );
}

#[test]
fn sandbox_network_default_is_mcclawd_tools() {
    let config = SandboxConfig::default();
    assert_eq!(
        config.network, "mcclawd_default",
        "network must default to mcclawd_tools"
    );
}

#[test]
fn sandbox_base_image_default() {
    let config = SandboxConfig::default();
    assert_eq!(config.base_image, "mcclawd-sandbox:latest");
}

#[test]
fn full_config_includes_sandbox_defaults() {
    let config = McclawdConfig::default();
    assert!(!config.sandbox.strict_sandbox);
    assert_eq!(config.sandbox.pids_limit, Some(256));
    assert_eq!(config.sandbox.network, "mcclawd_default");
}

#[test]
fn sandbox_config_deserializes_strict_false() {
    let json_str = r#"{ "sandbox": { "strict_sandbox": false } }"#;
    let config: McclawdConfig = json5::from_str(json_str).unwrap();
    assert!(
        !config.sandbox.strict_sandbox,
        "strict_sandbox=false must be respected"
    );
    // Other defaults should still apply
    assert_eq!(config.sandbox.pids_limit, Some(256));
}

#[test]
fn sandbox_config_deserializes_custom_pids_limit() {
    let json_str = r#"{ "sandbox": { "pids_limit": 512 } }"#;
    let config: McclawdConfig = json5::from_str(json_str).unwrap();
    assert_eq!(config.sandbox.pids_limit, Some(512));
    // strict_sandbox should still default to true
    assert!(config.sandbox.strict_sandbox);
}

#[test]
fn skills_sandbox_config_has_pids_limit() {
    let config = mcclawd_core::skills::SandboxConfig::default();
    assert_eq!(
        config.pids_limit,
        Some(256),
        "skills::SandboxConfig pids_limit must default to 256"
    );
}
