//! AWS Secrets Manager backend (stub).
//!
//! Requires the `aws-secrets` feature flag and `aws-sdk-secretsmanager` crate.
//! Currently a stub — returns errors indicating the backend is not yet wired.

use async_trait::async_trait;

use super::SecretBackend;
use crate::McclawdError;

/// AWS Secrets Manager backend configuration.
pub struct AwsSecretConfig {
    /// AWS region (e.g. "us-east-1").
    pub region: String,
    /// Key prefix in Secrets Manager (e.g. "mcclawd/prod/").
    pub prefix: String,
}

/// AWS Secrets Manager backend.
///
/// Currently a stub — all methods return `NotImplemented` errors.
/// Will be wired when `aws-sdk-secretsmanager` is added as a dependency.
pub struct AwsSecretBackend {
    config: AwsSecretConfig,
}

impl AwsSecretBackend {
    pub fn new(config: AwsSecretConfig) -> Self {
        Self { config }
    }

    /// Return the configured region.
    pub fn region(&self) -> &str {
        &self.config.region
    }

    /// Return the configured prefix.
    pub fn prefix(&self) -> &str {
        &self.config.prefix
    }
}

#[async_trait]
impl SecretBackend for AwsSecretBackend {
    async fn get(&self, _key: &str) -> crate::Result<Option<String>> {
        Err(McclawdError::Secret(
            "AWS Secrets Manager backend not yet wired — add aws-sdk-secretsmanager dependency"
                .to_string(),
        ))
    }

    async fn set(&self, _key: &str, _value: &str) -> crate::Result<()> {
        Err(McclawdError::Secret(
            "AWS Secrets Manager backend not yet wired — add aws-sdk-secretsmanager dependency"
                .to_string(),
        ))
    }

    async fn delete(&self, _key: &str) -> crate::Result<()> {
        Err(McclawdError::Secret(
            "AWS Secrets Manager backend not yet wired — add aws-sdk-secretsmanager dependency"
                .to_string(),
        ))
    }

    async fn list(&self) -> crate::Result<Vec<String>> {
        Err(McclawdError::Secret(
            "AWS Secrets Manager backend not yet wired — add aws-sdk-secretsmanager dependency"
                .to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_backend() -> AwsSecretBackend {
        AwsSecretBackend::new(AwsSecretConfig {
            region: "us-east-1".to_string(),
            prefix: "mcclawd/test/".to_string(),
        })
    }

    #[test]
    fn config_creation() {
        let backend = make_backend();
        assert_eq!(backend.region(), "us-east-1");
        assert_eq!(backend.prefix(), "mcclawd/test/");
    }

    #[tokio::test]
    async fn get_returns_error() {
        let backend = make_backend();
        let res = backend.get("key").await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("not yet wired"));
    }

    #[tokio::test]
    async fn set_returns_error() {
        let backend = make_backend();
        let res = backend.set("key", "val").await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn delete_returns_error() {
        let backend = make_backend();
        let res = backend.delete("key").await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn list_returns_error() {
        let backend = make_backend();
        let res = backend.list().await;
        assert!(res.is_err());
    }
}
