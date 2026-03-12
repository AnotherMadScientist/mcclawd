use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

pub mod aws;
pub mod encrypted_file;
pub mod env;
pub mod vault;

pub use aws::AwsSecretBackend;
pub use encrypted_file::EncryptedFileBackend;
pub use env::EnvSecretBackend;
pub use vault::VaultSecretBackend;

/// Metadata about a stored secret, returned by `list_with_metadata`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretMeta {
    pub key: String,
    /// Optional human-readable descriptor (e.g. "prod billing account").
    /// Shows up in logs and list output so operators know which key is which
    /// without exposing the value.
    pub descriptor: Option<String>,
}

/// Trait for secret storage backends.
/// Phase 0: EncryptedFileBackend (AES-256-GCM-SIV + argon2).
/// Phase 3: EnvSecretBackend (read-only), AwsSecretBackend (stub).
#[async_trait]
pub trait SecretBackend: Send + Sync {
    async fn get(&self, key: &str) -> crate::Result<Option<String>>;
    async fn set(&self, key: &str, value: &str) -> crate::Result<()>;
    async fn delete(&self, key: &str) -> crate::Result<()>;
    async fn list(&self) -> crate::Result<Vec<String>>;

    /// Set a secret with an optional human-readable descriptor.
    /// Default delegates to `set()` (ignores descriptor for backends that don't support it).
    async fn set_with_descriptor(
        &self,
        key: &str,
        value: &str,
        _descriptor: Option<&str>,
    ) -> crate::Result<()> {
        self.set(key, value).await
    }

    /// Get the descriptor for a secret (if the backend supports it).
    async fn get_descriptor(&self, _key: &str) -> crate::Result<Option<String>> {
        Ok(None)
    }

    /// List secrets with metadata (key + descriptor).
    /// Default builds from `list()` with no descriptors.
    async fn list_with_metadata(&self) -> crate::Result<Vec<SecretMeta>> {
        let keys = self.list().await?;
        Ok(keys
            .into_iter()
            .map(|key| SecretMeta {
                key,
                descriptor: None,
            })
            .collect())
    }
}

/// Regex matching `${SECRET_NAME}` tokens in env var values.
static TOKEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap());

/// Resolve `${SECRET_NAME}` tokens in a list of `KEY=VALUE` env strings.
///
/// Each value is scanned for `${...}` patterns. If the named secret exists in
/// the backend, the token is replaced with the secret value. Unresolved tokens
/// are left as-is (the container may resolve them from its own environment).
///
/// Returns the resolved env strings in the same order.
pub async fn resolve_secret_tokens(
    env_vars: &[String],
    backend: &dyn SecretBackend,
) -> crate::Result<Vec<String>> {
    let mut resolved = Vec::with_capacity(env_vars.len());

    for entry in env_vars {
        // Split on first '=' only — env values can contain '='
        let Some((key, value)) = entry.split_once('=') else {
            resolved.push(entry.clone());
            continue;
        };

        if !TOKEN_RE.is_match(value) {
            resolved.push(entry.clone());
            continue;
        }

        // Collect all unique secret names referenced in this value
        let mut new_value = value.to_string();
        for cap in TOKEN_RE.captures_iter(value) {
            let secret_name = &cap[1];
            let full_match = &cap[0];
            if let Some(secret_val) = backend.get(secret_name).await? {
                new_value = new_value.replace(full_match, &secret_val);
            }
        }

        resolved.push(format!("{key}={new_value}"));
    }

    Ok(resolved)
}
