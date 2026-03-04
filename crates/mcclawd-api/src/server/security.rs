//! Security hooks and backends API route handlers.

use axum::Json;
use serde::Serialize;

/// Summary of a security hook for the list endpoint.
#[derive(Debug, Serialize)]
pub struct HookInfo {
    pub name: String,
    pub enabled: bool,
}

/// Summary of a secret backend for the list endpoint.
#[derive(Debug, Serialize)]
pub struct BackendInfo {
    pub name: String,
    pub active: bool,
}

/// GET /api/security/hooks — list active security hooks.
pub async fn list_hooks() -> Json<Vec<HookInfo>> {
    Json(vec![
        HookInfo {
            name: "audit".to_string(),
            enabled: true,
        },
        HookInfo {
            name: "dlp".to_string(),
            enabled: true,
        },
        HookInfo {
            name: "secret_scanner".to_string(),
            enabled: true,
        },
    ])
}

/// GET /api/security/backends — list available secret backends.
pub async fn list_backends() -> Json<Vec<BackendInfo>> {
    Json(vec![
        BackendInfo {
            name: "encrypted_file".to_string(),
            active: true,
        },
        BackendInfo {
            name: "env".to_string(),
            active: false,
        },
        BackendInfo {
            name: "aws_secrets_manager".to_string(),
            active: false,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_hooks_returns_array() {
        let Json(hooks) = list_hooks().await;
        assert!(!hooks.is_empty());
        assert!(hooks.iter().any(|h| h.name == "audit"));
        assert!(hooks.iter().any(|h| h.name == "dlp"));
        assert!(hooks.iter().any(|h| h.name == "secret_scanner"));
    }

    #[tokio::test]
    async fn list_backends_returns_array() {
        let Json(backends) = list_backends().await;
        assert!(!backends.is_empty());
        assert!(backends.iter().any(|b| b.name == "encrypted_file"));
        assert!(backends.iter().any(|b| b.name == "env"));
        assert!(backends.iter().any(|b| b.name == "aws_secrets_manager"));
    }
}
