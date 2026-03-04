use mcclawd_core::config::McclawdConfig;
use mcclawd_core::secrets::{EncryptedFileBackend, SecretBackend};
use std::io::{self, Write};

fn get_backend() -> anyhow::Result<EncryptedFileBackend> {
    let config = McclawdConfig::default();
    // Phase 0: hardcoded passphrase for local dev.
    // Phase 1+: prompt or derive from keychain.
    let passphrase = "mcclawd-local-dev";
    Ok(EncryptedFileBackend::new(
        &config.secrets_path(),
        passphrase,
    )?)
}

pub async fn set(key: &str) -> anyhow::Result<()> {
    eprint!("Enter value for {}: ", key);
    io::stderr().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let value = value.trim();

    let backend = get_backend()?;
    backend.set(key, value).await?;
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
