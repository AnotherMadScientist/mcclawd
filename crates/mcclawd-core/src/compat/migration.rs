//! Config migration — convert OpenClaw configs to McClawd TOML format.
//!
//! Each `migrate_*` function returns a result struct containing:
//! - Generated TOML config snippets
//! - Secrets that must be imported (name + plaintext value)
//! - Warnings about unsupported or partially-supported features

use super::openclaw_config::*;
use std::collections::HashMap;

/// Result of migrating channel configs.
#[derive(Debug, Clone)]
pub struct ChannelMigrationResult {
    /// TOML config snippet for channels.
    pub toml_config: String,
    /// Secrets that need to be imported: `(secret_name, plaintext_value)`.
    pub secrets_to_import: Vec<(String, String)>,
    /// Warnings about unsupported features.
    pub warnings: Vec<String>,
}

/// Result of migrating MCP server configs.
#[derive(Debug, Clone)]
pub struct McpMigrationResult {
    /// TOML config snippet for MCP servers.
    pub toml_config: String,
    /// Warnings about servers that couldn't be fully migrated.
    pub warnings: Vec<String>,
}

/// Migrate OpenClaw channel configs to McClawd TOML format.
///
/// For each channel with a bot_token, the token is extracted into
/// `secrets_to_import` and the TOML references the secret name instead
/// of embedding the raw token.
pub fn migrate_channels(channels: &OpenClawChannels) -> ChannelMigrationResult {
    let mut toml_lines = Vec::new();
    let mut secrets = Vec::new();
    let mut warnings = Vec::new();

    // Telegram
    if let Some(ref tg) = channels.telegram {
        toml_lines.push("[channels.telegram]".to_string());
        toml_lines.push("enabled = true".to_string());
        if let Some(ref token) = tg.bot_token {
            secrets.push(("TELEGRAM_BOT_TOKEN".to_string(), token.clone()));
            toml_lines.push("bot_token_secret = \"TELEGRAM_BOT_TOKEN\"".to_string());
        }
        if let Some(ref ids) = tg.allowed_ids {
            let quoted: Vec<String> = ids.iter().map(|id| format!("\"{}\"", id)).collect();
            toml_lines.push(format!("allowed_ids = [{}]", quoted.join(", ")));
        }
        for key in tg.extra.keys() {
            warnings.push(format!(
                "Telegram: extra field '{}' not mapped (manual config needed)",
                key
            ));
        }
        toml_lines.push(String::new());
    }

    // Discord
    if let Some(ref dc) = channels.discord {
        toml_lines.push("[channels.discord]".to_string());
        toml_lines.push("enabled = true".to_string());
        if let Some(ref token) = dc.bot_token {
            secrets.push(("DISCORD_BOT_TOKEN".to_string(), token.clone()));
            toml_lines.push("bot_token_secret = \"DISCORD_BOT_TOKEN\"".to_string());
        }
        if let Some(ref ids) = dc.allowed_ids {
            let quoted: Vec<String> = ids.iter().map(|id| format!("\"{}\"", id)).collect();
            toml_lines.push(format!("allowed_ids = [{}]", quoted.join(", ")));
        }
        for key in dc.extra.keys() {
            warnings.push(format!(
                "Discord: extra field '{}' not mapped (manual config needed)",
                key
            ));
        }
        toml_lines.push(String::new());
    }

    // Slack
    if let Some(ref sl) = channels.slack {
        toml_lines.push("[channels.slack]".to_string());
        toml_lines.push("enabled = true".to_string());
        if let Some(ref token) = sl.bot_token {
            secrets.push(("SLACK_BOT_TOKEN".to_string(), token.clone()));
            toml_lines.push("bot_token_secret = \"SLACK_BOT_TOKEN\"".to_string());
        }
        if let Some(ref app_token) = sl.app_token {
            secrets.push(("SLACK_APP_TOKEN".to_string(), app_token.clone()));
            toml_lines.push("app_token_secret = \"SLACK_APP_TOKEN\"".to_string());
        }
        if let Some(ref ids) = sl.allowed_ids {
            let quoted: Vec<String> = ids.iter().map(|id| format!("\"{}\"", id)).collect();
            toml_lines.push(format!("allowed_ids = [{}]", quoted.join(", ")));
        }
        for key in sl.extra.keys() {
            warnings.push(format!(
                "Slack: extra field '{}' not mapped (manual config needed)",
                key
            ));
        }
        toml_lines.push(String::new());
    }

    // WhatsApp
    if let Some(ref wa) = channels.whatsapp {
        toml_lines.push("[channels.whatsapp]".to_string());
        toml_lines.push("enabled = true".to_string());
        if let Some(ref token) = wa.bot_token {
            secrets.push(("WHATSAPP_BOT_TOKEN".to_string(), token.clone()));
            toml_lines.push("bot_token_secret = \"WHATSAPP_BOT_TOKEN\"".to_string());
        }
        if let Some(ref ids) = wa.allowed_ids {
            let quoted: Vec<String> = ids.iter().map(|id| format!("\"{}\"", id)).collect();
            toml_lines.push(format!("allowed_ids = [{}]", quoted.join(", ")));
        }
        for key in wa.extra.keys() {
            warnings.push(format!(
                "WhatsApp: extra field '{}' not mapped (manual config needed)",
                key
            ));
        }
        toml_lines.push(String::new());
    }

    // Email
    if let Some(ref em) = channels.email {
        toml_lines.push("[channels.email]".to_string());
        toml_lines.push("enabled = true".to_string());
        if let Some(ref host) = em.imap_host {
            toml_lines.push(format!("imap_host = \"{}\"", host));
        }
        if let Some(port) = em.imap_port {
            toml_lines.push(format!("imap_port = {}", port));
        }
        if let Some(ref host) = em.smtp_host {
            toml_lines.push(format!("smtp_host = \"{}\"", host));
        }
        if let Some(port) = em.smtp_port {
            toml_lines.push(format!("smtp_port = {}", port));
        }
        if let Some(ref from) = em.from_address {
            toml_lines.push(format!("from_address = \"{}\"", from));
        }
        if let Some(ref user) = em.username {
            secrets.push(("EMAIL_USERNAME".to_string(), user.clone()));
            toml_lines.push("username_secret = \"EMAIL_USERNAME\"".to_string());
        }
        if let Some(ref pass) = em.password {
            secrets.push(("EMAIL_PASSWORD".to_string(), pass.clone()));
            toml_lines.push("password_secret = \"EMAIL_PASSWORD\"".to_string());
        }
        toml_lines.push(String::new());
    }

    ChannelMigrationResult {
        toml_config: toml_lines.join("\n"),
        secrets_to_import: secrets,
        warnings,
    }
}

/// Migrate OpenClaw MCP server configs to McClawd TOML format.
///
/// Servers with a `url` map directly. Servers with `command` + `args`
/// generate a warning since McClawd uses Docker images for MCP servers.
pub fn migrate_mcp_servers(servers: &HashMap<String, OpenClawMcpServer>) -> McpMigrationResult {
    let mut toml_lines = Vec::new();
    let mut warnings = Vec::new();

    for (name, server) in servers {
        if let Some(ref url) = server.url {
            // URL-based server maps directly
            toml_lines.push(format!("[[mcp.servers]]"));
            toml_lines.push(format!("name = \"{}\"", name));
            toml_lines.push(format!("url = \"{}\"", url));

            if let Some(ref env) = server.env {
                let env_strs: Vec<String> = env
                    .iter()
                    .map(|(k, v)| format!("\"{}={}\"", k, v))
                    .collect();
                toml_lines.push(format!("env = [{}]", env_strs.join(", ")));
            }
            toml_lines.push(String::new());
        } else if let Some(ref cmd) = server.command {
            // Command-based server — warn that McClawd uses Docker
            let args_str = server
                .args
                .as_ref()
                .map(|a| a.join(" "))
                .unwrap_or_default();
            warnings.push(format!(
                "MCP server '{}' uses command '{}{}'. McClawd runs MCP servers in Docker containers. \
                 Consider containerizing this server and configuring via [mcp.servers] with an image.",
                name,
                cmd,
                if args_str.is_empty() {
                    String::new()
                } else {
                    format!(" {}", args_str)
                }
            ));
            // Still emit a commented-out config for reference
            toml_lines.push(format!("# MCP server '{}' (needs containerization)", name));
            toml_lines.push(format!("# command: {} {}", cmd, args_str));
            toml_lines.push(format!("# [[mcp.servers]]"));
            toml_lines.push(format!("# name = \"{}\"", name));
            toml_lines.push(format!("# image = \"<docker-image-for-{}>\"", name));
            toml_lines.push(String::new());
        } else {
            warnings.push(format!(
                "MCP server '{}' has neither url nor command — skipped",
                name
            ));
        }
    }

    McpMigrationResult {
        toml_config: toml_lines.join("\n"),
        warnings,
    }
}

/// Migrate skill references to `mc skills install` commands.
pub fn migrate_skills(skills: &[String]) -> Vec<String> {
    skills
        .iter()
        .map(|s| format!("mc skills install {}", s))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_channels() -> OpenClawChannels {
        OpenClawChannels {
            telegram: Some(OpenClawChannelConfig {
                bot_token: Some("tg-token-123".to_string()),
                app_token: None,
                allowed_ids: Some(vec!["111".to_string(), "222".to_string()]),
                extra: HashMap::new(),
            }),
            discord: None,
            slack: None,
            whatsapp: None,
            email: None,
        }
    }

    #[test]
    fn migrate_telegram_extracts_token() {
        let result = migrate_channels(&make_channels());
        assert_eq!(result.secrets_to_import.len(), 1);
        assert_eq!(result.secrets_to_import[0].0, "TELEGRAM_BOT_TOKEN");
        assert_eq!(result.secrets_to_import[0].1, "tg-token-123");
        assert!(result.toml_config.contains("[channels.telegram]"));
        assert!(result
            .toml_config
            .contains("bot_token_secret = \"TELEGRAM_BOT_TOKEN\""));
        assert!(result.toml_config.contains("allowed_ids"));
    }

    #[test]
    fn migrate_empty_channels() {
        let channels = OpenClawChannels {
            telegram: None,
            discord: None,
            slack: None,
            whatsapp: None,
            email: None,
        };
        let result = migrate_channels(&channels);
        assert!(result.secrets_to_import.is_empty());
        assert!(result.warnings.is_empty());
        assert!(result.toml_config.is_empty());
    }

    #[test]
    fn migrate_slack_with_app_token() {
        let channels = OpenClawChannels {
            telegram: None,
            discord: None,
            slack: Some(OpenClawChannelConfig {
                bot_token: Some("sl-bot".to_string()),
                app_token: Some("sl-app".to_string()),
                allowed_ids: None,
                extra: HashMap::new(),
            }),
            whatsapp: None,
            email: None,
        };
        let result = migrate_channels(&channels);
        assert_eq!(result.secrets_to_import.len(), 2);
        assert!(result
            .secrets_to_import
            .iter()
            .any(|(k, v)| k == "SLACK_BOT_TOKEN" && v == "sl-bot"));
        assert!(result
            .secrets_to_import
            .iter()
            .any(|(k, v)| k == "SLACK_APP_TOKEN" && v == "sl-app"));
        assert!(result.toml_config.contains("[channels.slack]"));
    }

    #[test]
    fn migrate_email_extracts_credentials() {
        let channels = OpenClawChannels {
            telegram: None,
            discord: None,
            slack: None,
            whatsapp: None,
            email: Some(OpenClawEmailConfig {
                imap_host: Some("imap.test.com".to_string()),
                imap_port: Some(993),
                smtp_host: Some("smtp.test.com".to_string()),
                smtp_port: Some(587),
                username: Some("user@test.com".to_string()),
                password: Some("pass123".to_string()),
                from_address: Some("bot@test.com".to_string()),
            }),
        };
        let result = migrate_channels(&channels);
        assert_eq!(result.secrets_to_import.len(), 2);
        assert!(result
            .secrets_to_import
            .iter()
            .any(|(k, _)| k == "EMAIL_USERNAME"));
        assert!(result
            .secrets_to_import
            .iter()
            .any(|(k, _)| k == "EMAIL_PASSWORD"));
        assert!(result.toml_config.contains("imap_host = \"imap.test.com\""));
        assert!(result.toml_config.contains("imap_port = 993"));
        assert!(result.toml_config.contains("smtp_port = 587"));
        assert!(result
            .toml_config
            .contains("from_address = \"bot@test.com\""));
    }

    #[test]
    fn migrate_channel_warns_on_extra_fields() {
        let mut extra = HashMap::new();
        extra.insert("guildId".to_string(), serde_json::json!("12345"));
        let channels = OpenClawChannels {
            telegram: None,
            discord: Some(OpenClawChannelConfig {
                bot_token: Some("dc-tok".to_string()),
                app_token: None,
                allowed_ids: None,
                extra,
            }),
            slack: None,
            whatsapp: None,
            email: None,
        };
        let result = migrate_channels(&channels);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("guildId"));
        assert!(result.warnings[0].contains("Discord"));
    }

    #[test]
    fn migrate_mcp_url_passthrough() {
        let mut servers = HashMap::new();
        servers.insert(
            "search".to_string(),
            OpenClawMcpServer {
                command: None,
                args: None,
                env: Some(HashMap::from([("KEY".to_string(), "val".to_string())])),
                url: Some("http://localhost:8001".to_string()),
            },
        );
        let result = migrate_mcp_servers(&servers);
        assert!(result.toml_config.contains("[[mcp.servers]]"));
        assert!(result.toml_config.contains("name = \"search\""));
        assert!(result
            .toml_config
            .contains("url = \"http://localhost:8001\""));
        assert!(result.toml_config.contains("env = [\"KEY=val\"]"));
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn migrate_mcp_command_warns() {
        let mut servers = HashMap::new();
        servers.insert(
            "local-tool".to_string(),
            OpenClawMcpServer {
                command: Some("node".to_string()),
                args: Some(vec!["server.js".to_string()]),
                env: None,
                url: None,
            },
        );
        let result = migrate_mcp_servers(&servers);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("local-tool"));
        assert!(result.warnings[0].contains("node"));
        assert!(result.warnings[0].contains("Docker"));
        // Should have commented-out config
        assert!(result.toml_config.contains("# command: node server.js"));
    }

    #[test]
    fn migrate_mcp_empty_server_warns() {
        let mut servers = HashMap::new();
        servers.insert(
            "broken".to_string(),
            OpenClawMcpServer {
                command: None,
                args: None,
                env: None,
                url: None,
            },
        );
        let result = migrate_mcp_servers(&servers);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("broken"));
        assert!(result.warnings[0].contains("neither url nor command"));
    }

    #[test]
    fn migrate_skills_generates_install_commands() {
        let skills = vec![
            "web-search".to_string(),
            "code-review".to_string(),
            "summarizer".to_string(),
        ];
        let cmds = migrate_skills(&skills);
        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[0], "mc skills install web-search");
        assert_eq!(cmds[1], "mc skills install code-review");
        assert_eq!(cmds[2], "mc skills install summarizer");
    }

    #[test]
    fn migrate_skills_empty() {
        let cmds = migrate_skills(&[]);
        assert!(cmds.is_empty());
    }

    #[test]
    fn migrate_all_channels_together() {
        let channels = OpenClawChannels {
            telegram: Some(OpenClawChannelConfig {
                bot_token: Some("tg".to_string()),
                app_token: None,
                allowed_ids: None,
                extra: HashMap::new(),
            }),
            discord: Some(OpenClawChannelConfig {
                bot_token: Some("dc".to_string()),
                app_token: None,
                allowed_ids: None,
                extra: HashMap::new(),
            }),
            slack: Some(OpenClawChannelConfig {
                bot_token: Some("sl".to_string()),
                app_token: None,
                allowed_ids: None,
                extra: HashMap::new(),
            }),
            whatsapp: Some(OpenClawChannelConfig {
                bot_token: Some("wa".to_string()),
                app_token: None,
                allowed_ids: None,
                extra: HashMap::new(),
            }),
            email: None,
        };
        let result = migrate_channels(&channels);
        assert_eq!(result.secrets_to_import.len(), 4);
        assert!(result.toml_config.contains("[channels.telegram]"));
        assert!(result.toml_config.contains("[channels.discord]"));
        assert!(result.toml_config.contains("[channels.slack]"));
        assert!(result.toml_config.contains("[channels.whatsapp]"));
    }

    #[test]
    fn migrate_multiple_mcp_servers() {
        let mut servers = HashMap::new();
        servers.insert(
            "url-server".to_string(),
            OpenClawMcpServer {
                command: None,
                args: None,
                env: None,
                url: Some("http://localhost:9000".to_string()),
            },
        );
        servers.insert(
            "cmd-server".to_string(),
            OpenClawMcpServer {
                command: Some("python".to_string()),
                args: Some(vec!["-m".to_string(), "server".to_string()]),
                env: None,
                url: None,
            },
        );
        let result = migrate_mcp_servers(&servers);
        // One URL server migrates clean, one command server warns
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("cmd-server"));
        assert!(result.toml_config.contains("name = \"url-server\""));
    }
}
