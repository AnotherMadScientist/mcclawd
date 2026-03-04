//! HashiCorp Vault KV v2 secret backend.
//!
//! Lightweight implementation using `reqwest` directly (no `vaultrs` dependency).
//! Supports the KV v2 secrets engine API for get, set, delete, and list operations.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::SecretBackend;
use crate::McclawdError;

/// Configuration for the Vault secret backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultConfig {
    /// Vault server address (e.g. "http://127.0.0.1:8200").
    pub address: String,
    /// Environment variable name containing the Vault token (e.g. "VAULT_TOKEN").
    pub token_env: String,
    /// KV v2 mount point (default: "secret").
    pub mount: Option<String>,
    /// Key prefix (default: "mcclawd/").
    pub prefix: Option<String>,
}

/// HashiCorp Vault KV v2 secret backend.
pub struct VaultSecretBackend {
    config: VaultConfig,
    client: reqwest::Client,
}

/// Vault KV v2 read response structure.
#[derive(Debug, Deserialize)]
struct VaultReadResponse {
    data: VaultDataWrapper,
}

#[derive(Debug, Deserialize)]
struct VaultDataWrapper {
    data: std::collections::HashMap<String, String>,
}

/// Vault KV v2 list response structure.
#[derive(Debug, Deserialize)]
struct VaultListResponse {
    data: VaultListData,
}

#[derive(Debug, Deserialize)]
struct VaultListData {
    keys: Vec<String>,
}

impl VaultSecretBackend {
    /// Create a new Vault backend with the given configuration.
    pub fn new(config: VaultConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    /// Get the KV v2 mount point.
    fn mount(&self) -> &str {
        self.config.mount.as_deref().unwrap_or("secret")
    }

    /// Get the key prefix.
    fn prefix(&self) -> &str {
        self.config.prefix.as_deref().unwrap_or("mcclawd/")
    }

    /// Read the Vault token from the configured environment variable.
    fn token(&self) -> crate::Result<String> {
        std::env::var(&self.config.token_env).map_err(|_| {
            McclawdError::Secret(format!(
                "Vault token env var '{}' not set",
                self.config.token_env
            ))
        })
    }

    /// Build the full URL for a KV v2 data path.
    fn data_url(&self, path: &str) -> String {
        format!(
            "{}/v1/{}/data/{}{}",
            self.config.address.trim_end_matches('/'),
            self.mount(),
            self.prefix(),
            path
        )
    }

    /// Build the full URL for a KV v2 metadata path (used for list/delete).
    fn metadata_url(&self, path: &str) -> String {
        format!(
            "{}/v1/{}/metadata/{}{}",
            self.config.address.trim_end_matches('/'),
            self.mount(),
            self.prefix(),
            path
        )
    }
}

#[async_trait]
impl SecretBackend for VaultSecretBackend {
    /// Get a secret value from Vault KV v2.
    ///
    /// GET {address}/v1/{mount}/data/{prefix}{name}
    /// Parses response.data.data.value
    async fn get(&self, key: &str) -> crate::Result<Option<String>> {
        let token = self.token()?;
        let url = self.data_url(key);

        let response = self
            .client
            .get(&url)
            .header("X-Vault-Token", &token)
            .send()
            .await
            .map_err(|e| McclawdError::Secret(format!("Vault GET failed: {}", e)))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(McclawdError::Secret(format!(
                "Vault GET returned status {}",
                response.status()
            )));
        }

        let body: VaultReadResponse = response
            .json()
            .await
            .map_err(|e| McclawdError::Secret(format!("Vault response parse failed: {}", e)))?;

        Ok(body.data.data.get("value").cloned())
    }

    /// Set a secret value in Vault KV v2.
    ///
    /// POST {address}/v1/{mount}/data/{prefix}{name}
    /// Body: {"data": {"value": secret}}
    async fn set(&self, key: &str, value: &str) -> crate::Result<()> {
        let token = self.token()?;
        let url = self.data_url(key);

        let body = serde_json::json!({
            "data": {
                "value": value
            }
        });

        let response = self
            .client
            .post(&url)
            .header("X-Vault-Token", &token)
            .json(&body)
            .send()
            .await
            .map_err(|e| McclawdError::Secret(format!("Vault POST failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(McclawdError::Secret(format!(
                "Vault POST returned status {}",
                response.status()
            )));
        }

        Ok(())
    }

    /// Delete a secret from Vault KV v2.
    ///
    /// DELETE {address}/v1/{mount}/metadata/{prefix}{name}
    /// (Deletes all versions of the secret.)
    async fn delete(&self, key: &str) -> crate::Result<()> {
        let token = self.token()?;
        let url = self.metadata_url(key);

        let response = self
            .client
            .delete(&url)
            .header("X-Vault-Token", &token)
            .send()
            .await
            .map_err(|e| McclawdError::Secret(format!("Vault DELETE failed: {}", e)))?;

        if !response.status().is_success() && response.status() != reqwest::StatusCode::NOT_FOUND {
            return Err(McclawdError::Secret(format!(
                "Vault DELETE returned status {}",
                response.status()
            )));
        }

        Ok(())
    }

    /// List secret keys under the configured prefix in Vault KV v2.
    ///
    /// LIST {address}/v1/{mount}/metadata/{prefix}
    /// (Uses GET with ?list=true query parameter.)
    async fn list(&self) -> crate::Result<Vec<String>> {
        let token = self.token()?;
        let url = format!("{}?list=true", self.metadata_url(""));

        let response = self
            .client
            .get(&url)
            .header("X-Vault-Token", &token)
            .send()
            .await
            .map_err(|e| McclawdError::Secret(format!("Vault LIST failed: {}", e)))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(vec![]);
        }

        if !response.status().is_success() {
            return Err(McclawdError::Secret(format!(
                "Vault LIST returned status {}",
                response.status()
            )));
        }

        let body: VaultListResponse = response
            .json()
            .await
            .map_err(|e| McclawdError::Secret(format!("Vault list parse failed: {}", e)))?;

        Ok(body.data.keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config() -> VaultConfig {
        VaultConfig {
            address: "http://127.0.0.1:8200".to_string(),
            token_env: "VAULT_TOKEN".to_string(),
            mount: None,
            prefix: None,
        }
    }

    fn make_config_custom() -> VaultConfig {
        VaultConfig {
            address: "https://vault.example.com:8200".to_string(),
            token_env: "MY_VAULT_TOKEN".to_string(),
            mount: Some("kv".to_string()),
            prefix: Some("myapp/prod/".to_string()),
        }
    }

    #[test]
    fn config_defaults() {
        let backend = VaultSecretBackend::new(make_config());
        assert_eq!(backend.mount(), "secret");
        assert_eq!(backend.prefix(), "mcclawd/");
    }

    #[test]
    fn config_custom_mount_and_prefix() {
        let backend = VaultSecretBackend::new(make_config_custom());
        assert_eq!(backend.mount(), "kv");
        assert_eq!(backend.prefix(), "myapp/prod/");
    }

    #[test]
    fn data_url_default() {
        let backend = VaultSecretBackend::new(make_config());
        assert_eq!(
            backend.data_url("my-secret"),
            "http://127.0.0.1:8200/v1/secret/data/mcclawd/my-secret"
        );
    }

    #[test]
    fn data_url_custom() {
        let backend = VaultSecretBackend::new(make_config_custom());
        assert_eq!(
            backend.data_url("api-key"),
            "https://vault.example.com:8200/v1/kv/data/myapp/prod/api-key"
        );
    }

    #[test]
    fn metadata_url_default() {
        let backend = VaultSecretBackend::new(make_config());
        assert_eq!(
            backend.metadata_url("my-secret"),
            "http://127.0.0.1:8200/v1/secret/metadata/mcclawd/my-secret"
        );
    }

    #[test]
    fn metadata_url_custom() {
        let backend = VaultSecretBackend::new(make_config_custom());
        assert_eq!(
            backend.metadata_url("api-key"),
            "https://vault.example.com:8200/v1/kv/metadata/myapp/prod/api-key"
        );
    }

    #[test]
    fn trailing_slash_stripped_from_address() {
        let config = VaultConfig {
            address: "http://127.0.0.1:8200/".to_string(),
            token_env: "VAULT_TOKEN".to_string(),
            mount: None,
            prefix: None,
        };
        let backend = VaultSecretBackend::new(config);
        assert_eq!(
            backend.data_url("test"),
            "http://127.0.0.1:8200/v1/secret/data/mcclawd/test"
        );
    }

    #[test]
    fn token_missing_env_var() {
        let config = VaultConfig {
            address: "http://127.0.0.1:8200".to_string(),
            token_env: "MCCLAWD_TEST_VAULT_TOKEN_NONEXISTENT_12345".to_string(),
            mount: None,
            prefix: None,
        };
        let backend = VaultSecretBackend::new(config);
        let result = backend.token();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("MCCLAWD_TEST_VAULT_TOKEN_NONEXISTENT_12345"));
    }

    #[test]
    fn vault_read_response_deserialization() {
        let json = r#"{
            "data": {
                "data": {
                    "value": "my-secret-value"
                },
                "metadata": {
                    "version": 1
                }
            }
        }"#;
        let resp: VaultReadResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.data.get("value").unwrap(), "my-secret-value");
    }

    #[test]
    fn vault_list_response_deserialization() {
        let json = r#"{
            "data": {
                "keys": ["key1", "key2", "key3/"]
            }
        }"#;
        let resp: VaultListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.keys, vec!["key1", "key2", "key3/"]);
    }

    #[test]
    fn vault_list_response_empty() {
        let json = r#"{"data": {"keys": []}}"#;
        let resp: VaultListResponse = serde_json::from_str(json).unwrap();
        assert!(resp.data.keys.is_empty());
    }

    #[test]
    fn config_serialization_roundtrip() {
        let config = make_config_custom();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: VaultConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.address, "https://vault.example.com:8200");
        assert_eq!(parsed.token_env, "MY_VAULT_TOKEN");
        assert_eq!(parsed.mount.as_deref(), Some("kv"));
        assert_eq!(parsed.prefix.as_deref(), Some("myapp/prod/"));
    }

    #[test]
    fn config_deserialization_with_defaults() {
        let json = r#"{
            "address": "http://localhost:8200",
            "token_env": "VAULT_TOKEN"
        }"#;
        let config: VaultConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.address, "http://localhost:8200");
        assert!(config.mount.is_none());
        assert!(config.prefix.is_none());
    }

    #[test]
    fn metadata_url_empty_path() {
        let backend = VaultSecretBackend::new(make_config());
        assert_eq!(
            backend.metadata_url(""),
            "http://127.0.0.1:8200/v1/secret/metadata/mcclawd/"
        );
    }
}
