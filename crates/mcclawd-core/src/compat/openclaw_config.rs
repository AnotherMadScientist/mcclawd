//! OpenClaw config parser — reads openclaw.json and .mcp.json formats.
//!
//! OpenClaw uses JSON config files:
//! - `openclaw.json` — main config with channels, MCP servers, skills
//! - `.mcp.json` — standalone MCP server definitions
//!
//! This module deserializes those formats so `migration.rs` can convert
//! them to McClawd's native TOML config.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// OpenClaw's main configuration format (openclaw.json).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawConfig {
    /// Top-level metadata (clawdbot, clawdis, openclaw version info).
    #[serde(alias = "metadata")]
    pub metadata: Option<OpenClawMetadata>,

    /// Channel configurations (telegram, discord, slack, whatsapp, email).
    pub channels: Option<OpenClawChannels>,

    /// MCP server definitions keyed by name.
    #[serde(alias = "mcpServers", alias = "mcp_servers")]
    pub mcp_servers: Option<HashMap<String, OpenClawMcpServer>>,

    /// Skill names to install from ClawHub.
    pub skills: Option<Vec<String>>,
}

/// Top-level metadata block.
#[derive(Debug, Clone, Deserialize)]
pub struct OpenClawMetadata {
    pub clawdbot: Option<serde_json::Value>,
    pub clawdis: Option<serde_json::Value>,
    pub openclaw: Option<serde_json::Value>,
}

/// Channel config container.
#[derive(Debug, Clone, Deserialize)]
pub struct OpenClawChannels {
    pub telegram: Option<OpenClawChannelConfig>,
    pub discord: Option<OpenClawChannelConfig>,
    pub slack: Option<OpenClawChannelConfig>,
    pub whatsapp: Option<OpenClawChannelConfig>,
    pub email: Option<OpenClawEmailConfig>,
}

/// Generic channel config (Telegram, Discord, Slack, WhatsApp).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawChannelConfig {
    pub bot_token: Option<String>,
    pub app_token: Option<String>,
    pub allowed_ids: Option<Vec<String>>,
    /// Catch-all for channel-specific fields.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Email-specific channel config.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawEmailConfig {
    pub imap_host: Option<String>,
    pub imap_port: Option<u16>,
    pub smtp_host: Option<String>,
    pub smtp_port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub from_address: Option<String>,
}

/// MCP server definition (from openclaw.json or .mcp.json).
#[derive(Debug, Clone, Deserialize)]
pub struct OpenClawMcpServer {
    /// Command to spawn (stdio transport).
    pub command: Option<String>,
    /// Arguments for the command.
    pub args: Option<Vec<String>>,
    /// Environment variables to pass.
    pub env: Option<HashMap<String, String>>,
    /// Direct URL (HTTP/SSE transport).
    pub url: Option<String>,
}

/// Load an OpenClaw config from a JSON file path.
pub fn load_openclaw_config(path: &Path) -> anyhow::Result<OpenClawConfig> {
    let content = std::fs::read_to_string(path)?;
    let config: OpenClawConfig = serde_json::from_str(&content)?;
    Ok(config)
}

/// Load a standalone `.mcp.json` file (just MCP server definitions).
pub fn load_mcp_json(path: &Path) -> anyhow::Result<HashMap<String, OpenClawMcpServer>> {
    let content = std::fs::read_to_string(path)?;
    let wrapper: McpJsonWrapper = serde_json::from_str(&content)?;
    // Support both { "mcpServers": { ... } } and flat { "server": { ... } } formats
    if let Some(servers) = wrapper.mcp_servers {
        Ok(servers)
    } else {
        // Try parsing as flat map
        let servers: HashMap<String, OpenClawMcpServer> = serde_json::from_str(&content)?;
        Ok(servers)
    }
}

/// Wrapper for .mcp.json which may have a top-level `mcpServers` key.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpJsonWrapper {
    pub mcp_servers: Option<HashMap<String, OpenClawMcpServer>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_json(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn parse_minimal_config() {
        let f = write_temp_json("{}");
        let cfg = load_openclaw_config(f.path()).unwrap();
        assert!(cfg.metadata.is_none());
        assert!(cfg.channels.is_none());
        assert!(cfg.mcp_servers.is_none());
        assert!(cfg.skills.is_none());
    }

    #[test]
    fn parse_metadata_only() {
        let f = write_temp_json(r#"{"metadata": {"openclaw": "1.0"}}"#);
        let cfg = load_openclaw_config(f.path()).unwrap();
        assert!(cfg.metadata.is_some());
        let meta = cfg.metadata.unwrap();
        assert!(meta.openclaw.is_some());
        assert!(meta.clawdbot.is_none());
    }

    #[test]
    fn parse_skills_list() {
        let f = write_temp_json(r#"{"skills": ["web-search", "code-review", "summarizer"]}"#);
        let cfg = load_openclaw_config(f.path()).unwrap();
        let skills = cfg.skills.unwrap();
        assert_eq!(skills.len(), 3);
        assert_eq!(skills[0], "web-search");
        assert_eq!(skills[2], "summarizer");
    }

    #[test]
    fn parse_telegram_channel() {
        let json = r#"{
            "channels": {
                "telegram": {
                    "botToken": "123:ABC",
                    "allowedIds": ["111", "222"]
                }
            }
        }"#;
        let f = write_temp_json(json);
        let cfg = load_openclaw_config(f.path()).unwrap();
        let channels = cfg.channels.unwrap();
        let tg = channels.telegram.unwrap();
        assert_eq!(tg.bot_token.unwrap(), "123:ABC");
        assert_eq!(tg.allowed_ids.unwrap(), vec!["111", "222"]);
    }

    #[test]
    fn parse_discord_channel_with_extra_fields() {
        let json = r#"{
            "channels": {
                "discord": {
                    "botToken": "discord-token",
                    "guildId": "12345"
                }
            }
        }"#;
        let f = write_temp_json(json);
        let cfg = load_openclaw_config(f.path()).unwrap();
        let discord = cfg.channels.unwrap().discord.unwrap();
        assert_eq!(discord.bot_token.unwrap(), "discord-token");
        assert_eq!(
            discord.extra.get("guildId").unwrap(),
            &serde_json::json!("12345")
        );
    }

    #[test]
    fn parse_email_channel() {
        let json = r#"{
            "channels": {
                "email": {
                    "imapHost": "imap.example.com",
                    "imapPort": 993,
                    "smtpHost": "smtp.example.com",
                    "smtpPort": 587,
                    "username": "bot@example.com",
                    "password": "secret",
                    "fromAddress": "bot@example.com"
                }
            }
        }"#;
        let f = write_temp_json(json);
        let cfg = load_openclaw_config(f.path()).unwrap();
        let email = cfg.channels.unwrap().email.unwrap();
        assert_eq!(email.imap_host.unwrap(), "imap.example.com");
        assert_eq!(email.imap_port.unwrap(), 993);
        assert_eq!(email.smtp_host.unwrap(), "smtp.example.com");
        assert_eq!(email.smtp_port.unwrap(), 587);
        assert_eq!(email.username.unwrap(), "bot@example.com");
        assert_eq!(email.password.unwrap(), "secret");
        assert_eq!(email.from_address.unwrap(), "bot@example.com");
    }

    #[test]
    fn parse_full_config_with_all_channels() {
        let json = r#"{
            "metadata": {
                "clawdbot": {"version": "2.0"},
                "openclaw": "1.5"
            },
            "channels": {
                "telegram": {"botToken": "tg-token"},
                "discord": {"botToken": "dc-token"},
                "slack": {"botToken": "sl-token", "appToken": "sl-app"},
                "whatsapp": {"botToken": "wa-token"}
            },
            "mcpServers": {
                "search": {"url": "http://localhost:8001"},
                "code": {"command": "node", "args": ["server.js"]}
            },
            "skills": ["search", "code-review"]
        }"#;
        let f = write_temp_json(json);
        let cfg = load_openclaw_config(f.path()).unwrap();

        assert!(cfg.metadata.is_some());
        let channels = cfg.channels.unwrap();
        assert!(channels.telegram.is_some());
        assert!(channels.discord.is_some());
        assert!(channels.slack.is_some());
        assert!(channels.whatsapp.is_some());

        let slack = channels.slack.unwrap();
        assert_eq!(slack.bot_token.unwrap(), "sl-token");
        assert_eq!(slack.app_token.unwrap(), "sl-app");

        let servers = cfg.mcp_servers.unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(
            servers.get("search").unwrap().url.as_deref(),
            Some("http://localhost:8001")
        );

        let code = servers.get("code").unwrap();
        assert_eq!(code.command.as_deref(), Some("node"));
        assert_eq!(
            code.args.as_deref(),
            Some(vec!["server.js".to_string()].as_slice())
        );

        assert_eq!(cfg.skills.unwrap(), vec!["search", "code-review"]);
    }

    #[test]
    fn parse_mcp_json_with_wrapper() {
        let json = r#"{
            "mcpServers": {
                "langextract": {
                    "url": "http://localhost:8001",
                    "env": {"API_KEY": "abc123"}
                },
                "scrapling": {
                    "command": "python",
                    "args": ["-m", "scrapling_server"]
                }
            }
        }"#;
        let f = write_temp_json(json);
        let servers = load_mcp_json(f.path()).unwrap();
        assert_eq!(servers.len(), 2);
        let le = servers.get("langextract").unwrap();
        assert_eq!(le.url.as_deref(), Some("http://localhost:8001"));
        let env = le.env.as_ref().unwrap();
        assert_eq!(env.get("API_KEY").unwrap(), "abc123");
    }

    #[test]
    fn parse_mcp_json_flat_format() {
        let json = r#"{
            "search": {
                "url": "http://localhost:9000"
            }
        }"#;
        let f = write_temp_json(json);
        let servers = load_mcp_json(f.path()).unwrap();
        assert_eq!(servers.len(), 1);
        assert!(servers.contains_key("search"));
    }

    #[test]
    fn parse_mcp_server_with_env_vars() {
        let json = r#"{
            "mcpServers": {
                "custom": {
                    "command": "cargo",
                    "args": ["run", "--release"],
                    "env": {"RUST_LOG": "debug", "PORT": "3000"}
                }
            }
        }"#;
        let f = write_temp_json(json);
        let cfg = load_openclaw_config(f.path()).unwrap();
        let servers = cfg.mcp_servers.unwrap();
        let custom = servers.get("custom").unwrap();
        let env = custom.env.as_ref().unwrap();
        assert_eq!(env.len(), 2);
        assert_eq!(env.get("RUST_LOG").unwrap(), "debug");
    }

    #[test]
    fn parse_camel_case_mcp_servers_alias() {
        // Ensure both camelCase and snake_case work
        let json = r#"{"mcp_servers": {"test": {"url": "http://test"}}}"#;
        let f = write_temp_json(json);
        let cfg = load_openclaw_config(f.path()).unwrap();
        let servers = cfg.mcp_servers.unwrap();
        assert!(servers.contains_key("test"));
    }

    #[test]
    fn parse_missing_fields_gracefully() {
        let json = r#"{
            "channels": {
                "telegram": {}
            },
            "mcpServers": {
                "empty": {}
            }
        }"#;
        let f = write_temp_json(json);
        let cfg = load_openclaw_config(f.path()).unwrap();
        let tg = cfg.channels.unwrap().telegram.unwrap();
        assert!(tg.bot_token.is_none());
        assert!(tg.app_token.is_none());
        assert!(tg.allowed_ids.is_none());

        let empty = cfg.mcp_servers.unwrap();
        let server = empty.get("empty").unwrap();
        assert!(server.command.is_none());
        assert!(server.url.is_none());
    }

    #[test]
    fn load_nonexistent_file_errors() {
        let result = load_openclaw_config(Path::new("/tmp/nonexistent_openclaw_config.json"));
        assert!(result.is_err());
    }

    #[test]
    fn parse_invalid_json_errors() {
        let f = write_temp_json("{ not valid json }");
        let result = load_openclaw_config(f.path());
        assert!(result.is_err());
    }
}
