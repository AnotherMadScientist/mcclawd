//! Provider pool and config reload API route handlers.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;

use mcclawd_core::providers::{ProviderKind, ProviderPoolConfig, UsageSummary};

use super::state::AppState;

/// Summary of a provider for the list endpoint.
#[derive(Debug, Serialize)]
pub struct ProviderInfo {
    pub name: String,
    pub kind: ProviderKind,
    pub models: Vec<String>,
    pub enabled: bool,
    pub priority: u8,
}

/// GET /api/providers -- list providers from pool config.
pub async fn list_providers(State(state): State<AppState>) -> Json<Vec<ProviderInfo>> {
    let config = state.config.read().await;
    let pool_config = state.provider_pool_config(&config);

    let providers = pool_config
        .providers
        .iter()
        .map(|p| ProviderInfo {
            name: p.name.clone(),
            kind: p.kind.clone(),
            models: p.models.clone(),
            enabled: p.enabled,
            priority: p.priority,
        })
        .collect();

    Json(providers)
}

/// GET /api/providers/usage -- current usage summary.
pub async fn provider_usage(State(state): State<AppState>) -> Json<UsageSummary> {
    let pool = state.provider_pool.read().await;
    Json(pool.get_usage())
}

/// Response for config reload endpoint.
#[derive(Debug, Serialize)]
pub struct ReloadResponse {
    pub status: String,
    pub message: String,
}

/// POST /api/config/reload -- trigger config reload from disk.
pub async fn reload_config(
    State(state): State<AppState>,
) -> Result<Json<ReloadResponse>, StatusCode> {
    match state.reload_config().await {
        Ok(()) => Ok(Json(ReloadResponse {
            status: "ok".to_string(),
            message: "Configuration reloaded successfully".to_string(),
        })),
        Err(e) => {
            tracing::error!("Config reload failed: {}", e);
            Ok(Json(ReloadResponse {
                status: "error".to_string(),
                message: format!("Config reload failed: {}", e),
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcclawd_core::providers::{BudgetConfig, ProviderEntry, ProviderPool};

    #[test]
    fn provider_info_serialization() {
        let info = ProviderInfo {
            name: "anthropic".to_string(),
            kind: ProviderKind::Anthropic,
            models: vec!["claude-sonnet-4-5".to_string()],
            enabled: true,
            priority: 10,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("anthropic"));
        assert!(json.contains("Anthropic"));
        assert!(json.contains("claude-sonnet-4-5"));
    }

    #[test]
    fn reload_response_serialization() {
        let resp = ReloadResponse {
            status: "ok".to_string(),
            message: "Configuration reloaded successfully".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("ok"));
        assert!(json.contains("Configuration reloaded"));
    }

    #[test]
    fn provider_info_empty_models() {
        let info = ProviderInfo {
            name: "test".to_string(),
            kind: ProviderKind::Ollama,
            models: vec![],
            enabled: false,
            priority: 100,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"models\":[]"));
    }

    #[test]
    fn pool_config_default_is_empty() {
        let config = ProviderPoolConfig {
            providers: vec![],
            budget: None,
            fallback_order: None,
        };
        assert!(config.providers.is_empty());
        assert!(config.budget.is_none());
    }
}
