//! `mc import openclaw [path]` — import OpenClaw config into McClawd.

use mcclawd_core::compat::{
    extract_channel_secrets, load_mcp_json, load_openclaw_config, skill_install_commands,
    validate_mcp_servers,
};
use std::path::PathBuf;

/// Execute the `mc import openclaw` command.
///
/// 1. Determine config path: explicit argument, or `~/.openclaw/openclaw.json`.
/// 2. Extract secrets from channel configs for secure storage.
/// 3. Validate MCP server configs (warn about command-based servers).
/// 4. Print secrets to import and skill install commands.
pub async fn import_openclaw(path: Option<&str>) -> anyhow::Result<()> {
    let config_path = resolve_config_path(path)?;

    println!("Importing OpenClaw config from: {}", config_path.display());
    println!();

    let config = load_openclaw_config(&config_path)?;

    let mut all_warnings: Vec<String> = Vec::new();
    let mut all_secrets: Vec<(String, String)> = Vec::new();

    // Extract secrets from channels
    if let Some(ref channels) = config.channels {
        let result = extract_channel_secrets(channels);
        all_secrets.extend(result.secrets);
        all_warnings.extend(result.warnings);
    }

    // Validate MCP servers (warn about command-based servers)
    if let Some(ref servers) = config.mcp_servers {
        let warnings = validate_mcp_servers(servers);
        all_warnings.extend(warnings);
    }

    // Also check for .mcp.json in the current directory
    let mcp_json_path = PathBuf::from(".mcp.json");
    if mcp_json_path.exists() {
        println!("Found .mcp.json in current directory, validating MCP servers...");
        match load_mcp_json(&mcp_json_path) {
            Ok(servers) => {
                let warnings = validate_mcp_servers(&servers);
                all_warnings.extend(warnings);
            }
            Err(e) => {
                all_warnings.push(format!("Failed to parse .mcp.json: {}", e));
            }
        }
    }

    // Print skill install commands
    if let Some(ref skills) = config.skills {
        let skill_cmds = skill_install_commands(skills);
        if !skill_cmds.is_empty() {
            println!("--- Skills to install ---");
            for cmd in &skill_cmds {
                println!("  {}", cmd);
            }
            println!();
        }
    }

    // Print secrets to import
    if !all_secrets.is_empty() {
        println!("--- Secrets to import ---");
        println!("Run the following commands to store secrets securely:");
        println!();
        for (name, _) in &all_secrets {
            println!("  mc secrets set {}", name);
        }
        println!();
        println!(
            "({} secret(s) found — values not printed for security)",
            all_secrets.len()
        );
        println!();
    }

    // Print config adoption note
    println!("--- Config ---");
    println!(
        "OpenClaw JSON5 is McClawd's native format. Copy to ~/.mcclawd/mcclawd.json"
    );
    println!("or use directly: mc run --config {}", config_path.display());
    println!();

    // Print warnings
    if !all_warnings.is_empty() {
        println!("--- Warnings ---");
        for w in &all_warnings {
            println!("  [!] {}", w);
        }
        println!();
    }

    println!("Import complete.");
    Ok(())
}

/// Resolve the OpenClaw config file path.
fn resolve_config_path(explicit: Option<&str>) -> anyhow::Result<PathBuf> {
    if let Some(p) = explicit {
        let path = PathBuf::from(p);
        if !path.exists() {
            anyhow::bail!("Config file not found: {}", path.display());
        }
        return Ok(path);
    }

    // Default: ~/.openclaw/openclaw.json
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".openclaw").join("openclaw.json");
        if path.exists() {
            return Ok(path);
        }
    }

    // Check current directory
    let local = PathBuf::from("openclaw.json");
    if local.exists() {
        return Ok(local);
    }

    anyhow::bail!(
        "No OpenClaw config found. Provide a path or place openclaw.json in:\n  \
         - ~/.openclaw/openclaw.json\n  \
         - ./openclaw.json"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn resolve_explicit_path_works() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"{}").unwrap();
        let path = resolve_config_path(Some(f.path().to_str().unwrap())).unwrap();
        assert_eq!(path, f.path());
    }

    #[test]
    fn resolve_missing_explicit_path_errors() {
        let result = resolve_config_path(Some("/tmp/nonexistent_openclaw_test.json"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("not found"));
    }

    #[tokio::test]
    async fn import_minimal_config() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"{}").unwrap();
        let result = import_openclaw(Some(f.path().to_str().unwrap())).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn import_full_config() {
        let json = r#"{
            "channels": {
                "telegram": {"botToken": "test-token"}
            },
            "mcpServers": {
                "search": {"url": "http://localhost:8001"}
            },
            "skills": ["web-search"]
        }"#;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(json.as_bytes()).unwrap();
        let result = import_openclaw(Some(f.path().to_str().unwrap())).await;
        assert!(result.is_ok());
    }
}
