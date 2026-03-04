//! Environment variable secret backend (read-only).
//!
//! Reads secrets from environment variables, optionally filtered by prefix.

use async_trait::async_trait;

use super::SecretBackend;
use crate::McclawdError;

/// Read-only secret backend that reads from environment variables.
pub struct EnvSecretBackend {
    /// Optional prefix prepended to key lookups (e.g. "MCCLAWD_").
    prefix: Option<String>,
}

impl EnvSecretBackend {
    /// Create a new env backend with an optional prefix.
    pub fn new(prefix: Option<String>) -> Self {
        Self { prefix }
    }

    /// Create with no prefix (bare env var names).
    pub fn without_prefix() -> Self {
        Self { prefix: None }
    }

    /// Create with a prefix (e.g. "MCCLAWD_").
    pub fn with_prefix(prefix: impl Into<String>) -> Self {
        Self {
            prefix: Some(prefix.into()),
        }
    }

    fn full_key(&self, key: &str) -> String {
        match &self.prefix {
            Some(p) => format!("{}{}", p, key),
            None => key.to_string(),
        }
    }
}

#[async_trait]
impl SecretBackend for EnvSecretBackend {
    async fn get(&self, key: &str) -> crate::Result<Option<String>> {
        let full_key = self.full_key(key);
        match std::env::var(&full_key) {
            Ok(val) => Ok(Some(val)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(e) => Err(McclawdError::Secret(format!(
                "Failed to read env var '{}': {}",
                full_key, e
            ))),
        }
    }

    async fn set(&self, _key: &str, _value: &str) -> crate::Result<()> {
        Err(McclawdError::Secret(
            "EnvSecretBackend is read-only".to_string(),
        ))
    }

    async fn delete(&self, _key: &str) -> crate::Result<()> {
        Err(McclawdError::Secret(
            "EnvSecretBackend is read-only".to_string(),
        ))
    }

    async fn list(&self) -> crate::Result<Vec<String>> {
        match &self.prefix {
            Some(prefix) => {
                let keys: Vec<String> = std::env::vars()
                    .filter_map(|(k, _)| k.strip_prefix(prefix).map(|s| s.to_string()))
                    .collect();
                Ok(keys)
            }
            None => Ok(vec![]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_existing_env_var() {
        // PATH should always exist
        let backend = EnvSecretBackend::without_prefix();
        let val = backend.get("PATH").await.unwrap();
        assert!(val.is_some());
    }

    #[tokio::test]
    async fn get_missing_env_var() {
        let backend = EnvSecretBackend::without_prefix();
        let val = backend
            .get("MCCLAWD_TEST_NONEXISTENT_VAR_12345")
            .await
            .unwrap();
        assert!(val.is_none());
    }

    #[tokio::test]
    async fn set_returns_error() {
        let backend = EnvSecretBackend::without_prefix();
        let res = backend.set("key", "value").await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("read-only"));
    }

    #[tokio::test]
    async fn delete_returns_error() {
        let backend = EnvSecretBackend::without_prefix();
        let res = backend.delete("key").await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("read-only"));
    }

    #[tokio::test]
    async fn with_prefix() {
        // SAFETY: test-only, single-threaded access to env var
        unsafe { std::env::set_var("MCCTEST_SECRET1", "val1") };
        let backend = EnvSecretBackend::with_prefix("MCCTEST_");
        let val = backend.get("SECRET1").await.unwrap();
        assert_eq!(val, Some("val1".to_string()));
        unsafe { std::env::remove_var("MCCTEST_SECRET1") };
    }
}
