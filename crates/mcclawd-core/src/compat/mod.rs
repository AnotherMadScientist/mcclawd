//! OpenClaw compatibility — import `openclaw.json` and `.mcp.json` configs.
//!
//! Modules:
//! - `openclaw_config` — deserialisation types and loaders
//! - `migration` — convert OpenClaw configs to McClawd TOML format

pub mod migration;
pub mod openclaw_config;

pub use migration::{
    migrate_channels, migrate_mcp_servers, migrate_skills, ChannelMigrationResult,
    McpMigrationResult,
};
pub use openclaw_config::{
    load_mcp_json, load_openclaw_config, OpenClawChannelConfig, OpenClawChannels, OpenClawConfig,
    OpenClawEmailConfig, OpenClawMcpServer, OpenClawMetadata,
};
