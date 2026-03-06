use mcclawd_core::config::McclawdConfig;
use mcclawd_core::secrets::{EncryptedFileBackend, SecretBackend};
use std::io::{self, Write};

/// Keys we auto-import from .env into the vault.
const ENV_IMPORT_KEYS: &[&str] = &["ANTHROPIC_API_KEY", "ANTHROPIC_ADMIN_KEY"];

fn get_backend() -> anyhow::Result<EncryptedFileBackend> {
    let config = McclawdConfig::default();
    let vault_key_path = config.data_dir.join("vault.key");
    let passphrase = if vault_key_path.exists() {
        let bytes = std::fs::read(&vault_key_path)?;
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    } else {
        // Generate vault key if missing (first-time CLI use)
        let key: [u8; 32] = rand::random();
        if let Some(parent) = vault_key_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&vault_key_path, key)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                &vault_key_path,
                std::fs::Permissions::from_mode(0o600),
            )?;
        }
        key.iter().map(|b| format!("{b:02x}")).collect::<String>()
    };
    Ok(EncryptedFileBackend::new(
        &config.secrets_path(),
        &passphrase,
    )?)
}

/// Prompt user for yes/no confirmation. Returns true if confirmed.
/// In non-interactive mode (`auto_yes`), always returns true.
fn confirm(prompt: &str, auto_yes: bool) -> bool {
    if auto_yes {
        return true;
    }
    eprint!("{prompt} [Y/n] ");
    io::stderr().flush().ok();
    let mut buf = String::new();
    if io::stdin().read_line(&mut buf).is_err() {
        return false;
    }
    let answer = buf.trim().to_lowercase();
    answer.is_empty() || answer == "y" || answer == "yes"
}

pub async fn set(key: &str, inline_value: Option<&str>) -> anyhow::Result<()> {
    let value = if let Some(v) = inline_value {
        v.to_string()
    } else {
        eprint!("Enter value for {}: ", key);
        io::stderr().flush()?;
        let mut buf = String::new();
        io::stdin().read_line(&mut buf)?;
        buf.trim().to_string()
    };

    let backend = get_backend()?;
    backend.set(key, &value).await?;
    println!("Secret '{}' saved.", key);
    Ok(())
}

pub async fn get(key: &str) -> anyhow::Result<()> {
    let backend = get_backend()?;
    match backend.get(key).await? {
        Some(value) => {
            let masked = if value.len() > 8 {
                format!("{}...{}", &value[..4], &value[value.len() - 4..])
            } else {
                "****".to_string()
            };
            println!("{}: {}", key, masked);
        }
        None => println!("Secret '{}' not found.", key),
    }
    Ok(())
}

pub async fn list() -> anyhow::Result<()> {
    let backend = get_backend()?;
    let keys = backend.list().await?;
    if keys.is_empty() {
        println!("No secrets stored.");
    } else {
        println!("Stored secrets:");
        for key in keys {
            println!("  {}", key);
        }
    }
    Ok(())
}

pub async fn delete(key: &str) -> anyhow::Result<()> {
    let backend = get_backend()?;
    backend.delete(key).await?;
    println!("Secret '{}' deleted.", key);
    Ok(())
}

/// Initialize vault, import .env keys, and list contents.
///
/// - Ensures vault.key exists (creates if missing)
/// - Self-heals corrupted secrets.enc
/// - Loads env_file (or .env in cwd) then imports known keys (with confirmation unless -y)
pub async fn init(env_file: Option<&str>, auto_yes: bool) -> anyhow::Result<()> {
    // Load .env file explicitly (dotenvy in main.rs loads cwd/.env, but user may specify a path)
    if let Some(path) = env_file {
        match dotenvy::from_filename(path) {
            Ok(p) => println!("Loaded env file: {}", p.display()),
            Err(e) => println!("Warning: could not load {path}: {e}"),
        }
    }
    let config = McclawdConfig::default();
    let vault_key_path = config.data_dir.join("vault.key");
    let secrets_path = config.secrets_path();

    // Ensure vault.key exists
    if !vault_key_path.exists() {
        let key: [u8; 32] = rand::random();
        if let Some(parent) = vault_key_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&vault_key_path, key)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                &vault_key_path,
                std::fs::Permissions::from_mode(0o600),
            )?;
        }
        println!("Created new vault.key");
    }

    // Try to open vault — if secrets.enc is stale, delete and recreate
    let backend = match get_backend() {
        Ok(b) => {
            println!("Vault unlocked successfully.");
            b
        }
        Err(_) => {
            // secrets.enc is corrupted or mismatched with vault.key
            if secrets_path.exists() {
                std::fs::remove_file(&secrets_path)?;
                println!("Deleted stale secrets.enc (vault.key mismatch).");
            }
            let b = get_backend()?;
            println!("Created fresh vault.");
            b
        }
    };

    // Collect importable keys from environment (loaded by dotenvy in main.rs)
    let mut found_keys: Vec<(&str, String)> = Vec::new();
    for &env_key in ENV_IMPORT_KEYS {
        if let Ok(val) = std::env::var(env_key) {
            if !val.is_empty() {
                found_keys.push((env_key, val));
            }
        }
    }

    if found_keys.is_empty() {
        println!("\nNo importable keys found in environment/.env");
        println!("Set them in .env or export them:");
        for &key in ENV_IMPORT_KEYS {
            println!("  {key}=sk-...");
        }
    } else {
        // Show what we found and ask for confirmation
        println!("\nFound {} key(s) in environment/.env:", found_keys.len());
        for (key, val) in &found_keys {
            let masked = if val.len() > 12 {
                format!("{}...{}", &val[..6], &val[val.len() - 4..])
            } else {
                "****".to_string()
            };
            // Check if already in vault with same value
            let status = match backend.get(key).await {
                Ok(Some(existing)) if &existing == val => " (up to date)",
                Ok(Some(_)) => " (will update)",
                _ => " (new)",
            };
            println!("  {key} = {masked}{status}");
        }

        if confirm("\nImport these keys into the vault?", auto_yes) {
            for (key, val) in &found_keys {
                match backend.get(key).await {
                    Ok(Some(existing)) if &existing == val => {
                        // Already up to date, skip
                    }
                    _ => {
                        backend.set(key, val).await?;
                        println!("  {key} imported.");
                    }
                }
            }
            println!("Import complete.");
        } else {
            println!("Skipped import.");
        }
    }

    // List all secrets
    let keys = backend.list().await?;
    if keys.is_empty() {
        println!("\nVault is empty. Set secrets with: mc secrets set <KEY>");
    } else {
        println!("\nSecrets in vault:");
        for key in &keys {
            println!("  {}", key);
        }
    }

    Ok(())
}

/// Reset vault completely: deletes vault.key + secrets.enc.
/// This is destructive — all secrets are lost. Requires confirmation unless -y.
pub async fn reset(auto_yes: bool) -> anyhow::Result<()> {
    let config = McclawdConfig::default();
    let vault_key_path = config.data_dir.join("vault.key");
    let secrets_path = config.secrets_path();

    let has_vault = vault_key_path.exists();
    let has_secrets = secrets_path.exists();

    if !has_vault && !has_secrets {
        println!("No vault found — nothing to reset.");
        return Ok(());
    }

    // Show what will be deleted
    println!("This will permanently delete:");
    if has_vault {
        println!("  {}", vault_key_path.display());
    }
    if has_secrets {
        // Show how many secrets will be lost
        if let Ok(backend) = get_backend() {
            if let Ok(keys) = backend.list().await {
                if !keys.is_empty() {
                    println!("  {} ({} secret{})", secrets_path.display(), keys.len(), if keys.len() == 1 { "" } else { "s" });
                    for key in &keys {
                        println!("    - {key}");
                    }
                } else {
                    println!("  {} (empty)", secrets_path.display());
                }
            } else {
                println!("  {} (unreadable)", secrets_path.display());
            }
        } else {
            println!("  {} (corrupted)", secrets_path.display());
        }
    }

    if !confirm("\nAre you sure? This cannot be undone.", auto_yes) {
        println!("Aborted.");
        return Ok(());
    }

    if has_secrets {
        std::fs::remove_file(&secrets_path)?;
        println!("Deleted secrets.enc");
    }
    if has_vault {
        std::fs::remove_file(&vault_key_path)?;
        println!("Deleted vault.key");
    }

    println!("\nVault reset complete. Run `mc secrets init` to create a new vault.");
    Ok(())
}
