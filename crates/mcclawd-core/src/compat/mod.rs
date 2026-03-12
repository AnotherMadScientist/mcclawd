//! OpenClaw-native config — `openclaw.json` and `.mcp.json` are the native formats.
//!
//! Modules:
//! - `openclaw_config` — deserialisation types and loaders (JSON5)
//! - `migration` — secret extraction from OpenClaw channel configs

pub mod migration;
pub mod openclaw_config;

pub use migration::{
    extract_channel_secrets, skill_install_commands, validate_mcp_servers, SecretExtractionResult,
};
pub use openclaw_config::{
    load_mcp_json, load_openclaw_config, OpenClawChannelConfig, OpenClawChannels, OpenClawConfig,
    OpenClawEmailConfig, OpenClawMcpServer, OpenClawMetadata,
};
