//! OpenClaw config adoption — extract secrets and merge configs.
//!
//! OpenClaw JSON5 is the native config format. No conversion needed.
//! This module extracts secrets (bot tokens, credentials) from OpenClaw
//! channel configs so they can be stored securely via SecretStore.

use super::openclaw_config::*;
use std::collections::HashMap;

/// Result of extracting secrets from channel configs.
#[derive(Debug, Clone)]
pub struct SecretExtractionResult {
    /// Secrets that need to be imported: `(secret_name, plaintext_value)`.
    pub secrets: Vec<(String, String)>,
    /// Warnings about unsupported features.
    pub warnings: Vec<String>,
}

/// Extract secrets from OpenClaw channel configs for secure storage.
///
/// Bot tokens and credentials are extracted so they can be stored via
/// McClawd's SecretStore (AES-256-GCM-SIV) instead of sitting in plaintext config.
pub fn extract_channel_secrets(channels: &OpenClawChannels) -> SecretExtractionResult {
    let mut secrets = Vec::new();
    let mut warnings = Vec::new();

    if let Some(ref tg) = channels.telegram {
        if let Some(ref token) = tg.bot_token {
            secrets.push(("TELEGRAM_BOT_TOKEN".to_string(), token.clone()));
        }
        for key in tg.extra.keys() {
            warnings.push(format!(
                "Telegram: extra field '{}' not mapped (manual config needed)",
                key
            ));
        }
    }

    if let Some(ref dc) = channels.discord {
        if let Some(ref token) = dc.bot_token {
            secrets.push(("DISCORD_BOT_TOKEN".to_string(), token.clone()));
        }
        for key in dc.extra.keys() {
            warnings.push(format!(
                "Discord: extra field '{}' not mapped (manual config needed)",
                key
            ));
        }
    }

    if let Some(ref sl) = channels.slack {
        if let Some(ref token) = sl.bot_token {
            secrets.push(("SLACK_BOT_TOKEN".to_string(), token.clone()));
        }
        if let Some(ref app_token) = sl.app_token {
            secrets.push(("SLACK_APP_TOKEN".to_string(), app_token.clone()));
        }
        for key in sl.extra.keys() {
            warnings.push(format!(
                "Slack: extra field '{}' not mapped (manual config needed)",
                key
            ));
        }
    }

    if let Some(ref wa) = channels.whatsapp {
        if let Some(ref token) = wa.bot_token {
            secrets.push(("WHATSAPP_BOT_TOKEN".to_string(), token.clone()));
        }
        for key in wa.extra.keys() {
            warnings.push(format!(
                "WhatsApp: extra field '{}' not mapped (manual config needed)",
                key
            ));
        }
    }

    if let Some(ref em) = channels.email {
        if let Some(ref user) = em.username {
            secrets.push(("EMAIL_USERNAME".to_string(), user.clone()));
        }
        if let Some(ref pass) = em.password {
            secrets.push(("EMAIL_PASSWORD".to_string(), pass.clone()));
        }
    }

    SecretExtractionResult { secrets, warnings }
}

/// Validate MCP server configs — warn about command-based servers
/// that should use AgentGateway instead of direct execution.
pub fn validate_mcp_servers(servers: &HashMap<String, OpenClawMcpServer>) -> Vec<String> {
    let mut warnings = Vec::new();

    for (name, server) in servers {
        if server.url.is_some() {
            // URL-based servers route through AgentGateway — good
            continue;
        }
        if let Some(ref cmd) = server.command {
            let args_str = server
                .args
                .as_ref()
                .map(|a| a.join(" "))
                .unwrap_or_default();
            warnings.push(format!(
                "MCP server '{}' uses command '{}{}'. McClawd routes all MCP \
                 through AgentGateway — containerize this server or provide a URL.",
                name,
                cmd,
                if args_str.is_empty() {
                    String::new()
                } else {
                    format!(" {}", args_str)
                }
            ));
        } else {
            warnings.push(format!(
                "MCP server '{}' has neither url nor command — skipped",
                name
            ));
        }
    }

    warnings
}

/// Generate `mc skills install` commands for skill references.
pub fn skill_install_commands(skills: &[String]) -> Vec<String> {
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
    fn extract_telegram_token() {
        let result = extract_channel_secrets(&make_channels());
        assert_eq!(result.secrets.len(), 1);
        assert_eq!(result.secrets[0].0, "TELEGRAM_BOT_TOKEN");
        assert_eq!(result.secrets[0].1, "tg-token-123");
    }

    #[test]
    fn extract_empty_channels() {
        let channels = OpenClawChannels {
            telegram: None,
            discord: None,
            slack: None,
            whatsapp: None,
            email: None,
        };
        let result = extract_channel_secrets(&channels);
        assert!(result.secrets.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn extract_slack_with_app_token() {
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
        let result = extract_channel_secrets(&channels);
        assert_eq!(result.secrets.len(), 2);
        assert!(result
            .secrets
            .iter()
            .any(|(k, v)| k == "SLACK_BOT_TOKEN" && v == "sl-bot"));
        assert!(result
            .secrets
            .iter()
            .any(|(k, v)| k == "SLACK_APP_TOKEN" && v == "sl-app"));
    }

    #[test]
    fn extract_email_credentials() {
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
        let result = extract_channel_secrets(&channels);
        assert_eq!(result.secrets.len(), 2);
        assert!(result
            .secrets
            .iter()
            .any(|(k, _)| k == "EMAIL_USERNAME"));
        assert!(result
            .secrets
            .iter()
            .any(|(k, _)| k == "EMAIL_PASSWORD"));
    }

    #[test]
    fn channel_warns_on_extra_fields() {
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
        let result = extract_channel_secrets(&channels);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("guildId"));
        assert!(result.warnings[0].contains("Discord"));
    }

    #[test]
    fn validate_mcp_url_passthrough() {
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
        let warnings = validate_mcp_servers(&servers);
        assert!(warnings.is_empty());
    }

    #[test]
    fn validate_mcp_command_warns() {
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
        let warnings = validate_mcp_servers(&servers);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("local-tool"));
        assert!(warnings[0].contains("node"));
        assert!(warnings[0].contains("AgentGateway"));
    }

    #[test]
    fn validate_mcp_empty_server_warns() {
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
        let warnings = validate_mcp_servers(&servers);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("broken"));
        assert!(warnings[0].contains("neither url nor command"));
    }

    #[test]
    fn skill_install_commands_generates() {
        let skills = vec![
            "web-search".to_string(),
            "code-review".to_string(),
            "summarizer".to_string(),
        ];
        let cmds = skill_install_commands(&skills);
        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[0], "mc skills install web-search");
        assert_eq!(cmds[1], "mc skills install code-review");
        assert_eq!(cmds[2], "mc skills install summarizer");
    }

    #[test]
    fn skill_install_commands_empty() {
        let cmds = skill_install_commands(&[]);
        assert!(cmds.is_empty());
    }

    #[test]
    fn extract_all_channels_together() {
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
        let result = extract_channel_secrets(&channels);
        assert_eq!(result.secrets.len(), 4);
    }

    #[test]
    fn validate_multiple_mcp_servers() {
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
        let warnings = validate_mcp_servers(&servers);
        // One URL server passes, one command server warns
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("cmd-server"));
    }
}
