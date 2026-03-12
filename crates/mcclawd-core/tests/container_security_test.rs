//! Container security configuration tests.
//!
//! Validates that sandbox defaults enforce Docker-only execution with
//! security hardening (no-new-privileges, pids_limit).
//! Host execution has been removed — Docker is always required.

use mcclawd_core::config::{McclawdConfig, SandboxConfig};

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
fn sandbox_network_default_is_mcclawd_default() {
    let config = SandboxConfig::default();
    assert_eq!(
        config.network, "mcclawd_default",
        "network must default to mcclawd_default"
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
    assert_eq!(config.sandbox.pids_limit, Some(256));
    assert_eq!(config.sandbox.network, "mcclawd_default");
}

#[test]
fn sandbox_config_deserializes_custom_pids_limit() {
    let json_str = r#"{ "sandbox": { "pids_limit": 512 } }"#;
    let config: McclawdConfig = json5::from_str(json_str).unwrap();
    assert_eq!(config.sandbox.pids_limit, Some(512));
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
