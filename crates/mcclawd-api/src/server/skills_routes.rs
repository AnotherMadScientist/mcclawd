//! Skills API route handlers — list, search, install, uninstall skills via ClawHub.
//!
//! Includes local catalog cache endpoints for fast UI browsing.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use mcclawd_core::clawhub::cache::CachedSearchResult;
use mcclawd_core::clawhub::installer::{InstalledSkillInfo, SkillInstaller};
use mcclawd_core::clawhub::{ClawHubCache, ClawHubClient, ClawHubSearchResult};
use mcclawd_core::clawhub::ClawHubSkillMeta;

use super::state::AppState;

/// Query parameters for the search endpoint.
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default)]
    pub page: u64,
}

/// Request body for installing a skill.
#[derive(Debug, Deserialize)]
pub struct InstallRequest {
    pub name: String,
    pub version: Option<String>,
}

/// Helper: build a ClawHubClient + SkillInstaller from current config.
async fn build_installer(state: &AppState) -> (ClawHubClient, SkillInstaller) {
    let config = state.config.read().await;
    let client = ClawHubClient::new(&config.skills.clawhub_api);
    let installer = SkillInstaller::new(client.clone(), config.skills.managed_dir.clone());
    (client, installer)
}

/// GET /api/skills — list installed skills.
pub async fn list_installed(
    State(state): State<AppState>,
) -> Result<Json<Vec<InstalledSkillInfo>>, impl IntoResponse> {
    let (_, installer) = build_installer(&state).await;
    match installer.list_installed() {
        Ok(skills) => Ok(Json(skills)),
        Err(e) => {
            tracing::error!("Failed to list installed skills: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Failed to list installed skills: {e}")})),
            ))
        }
    }
}

/// GET /api/skills/search?q={query}&page={page} — search ClawHub registry.
pub async fn search_clawhub(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<ClawHubSearchResult>, impl IntoResponse> {
    let (client, _) = build_installer(&state).await;
    match client.search(&params.q, params.page).await {
        Ok(result) => Ok(Json(result)),
        Err(e) => {
            tracing::error!("ClawHub search failed: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("ClawHub search failed: {e}")})),
            ))
        }
    }
}

/// POST /api/skills/install — install a skill from the registry.
pub async fn install_skill(
    State(state): State<AppState>,
    Json(body): Json<InstallRequest>,
) -> Result<Json<InstalledSkillInfo>, impl IntoResponse> {
    let (_, installer) = build_installer(&state).await;
    let version = body.version.as_deref();
    match installer.install_from_registry(&body.name, version).await {
        Ok(info) => Ok(Json(info)),
        Err(e) => {
            tracing::error!("Failed to install skill '{}': {e}", body.name);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Failed to install skill: {e}")})),
            ))
        }
    }
}

/// DELETE /api/skills/{name} — uninstall a skill.
pub async fn uninstall_skill(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, impl IntoResponse> {
    let (_, installer) = build_installer(&state).await;
    match installer.uninstall(&name) {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            tracing::error!("Failed to uninstall skill '{}': {e}", name);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Failed to uninstall skill: {e}")})),
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Catalog cache endpoints
// ---------------------------------------------------------------------------

/// Query parameters for the catalog browse endpoint.
#[derive(Debug, Deserialize)]
pub struct CatalogQuery {
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub page: u64,
    #[serde(default = "default_per_page")]
    pub per_page: u64,
}

fn default_per_page() -> u64 {
    20
}

/// Helper: build a ClawHubCache from current config.
async fn build_cache(state: &AppState) -> ClawHubCache {
    let config = state.config.read().await;
    let client = ClawHubClient::new(&config.skills.clawhub_api);
    let cache = ClawHubCache::new(&config.skills.cache_dir, client);
    // Best-effort load from disk; ignore errors (cache miss is fine).
    let _ = cache.load_from_disk().await;
    cache
}

/// GET /api/skills/catalog?q=&page=&per_page= — browse cached catalog (local search, fast).
pub async fn browse_catalog(
    State(state): State<AppState>,
    Query(params): Query<CatalogQuery>,
) -> Json<CachedSearchResult> {
    let cache = build_cache(&state).await;
    let result = cache.search(&params.q, params.page, params.per_page).await;
    Json(result)
}

/// GET /api/skills/catalog/{name} — get detail for a single skill from cache.
pub async fn skill_detail(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<ClawHubSkillMeta>, impl IntoResponse> {
    let cache = build_cache(&state).await;
    match cache.get_skill(&name).await {
        Some(skill) => Ok(Json(skill)),
        None => {
            Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": format!("Skill '{}' not found in cache. Try refreshing the catalog.", name)})),
            ))
        }
    }
}

/// POST /api/skills/refresh — trigger background cache refresh from ClawHub.
pub async fn refresh_catalog(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let cache = build_cache(&state).await;
    match cache.refresh().await {
        Ok(count) => Json(serde_json::json!({
            "refreshed": count,
        })),
        Err(e) => {
            tracing::warn!("Catalog refresh failed (ClawHub may be unreachable): {e}");
            Json(serde_json::json!({
                "refreshed": 0,
                "error": format!("ClawHub unreachable: {e}"),
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_query_defaults() {
        let json = r#"{"q": "memory"}"#;
        let query: SearchQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.q, "memory");
        assert_eq!(query.page, 0);
    }

    #[test]
    fn test_search_query_with_page() {
        let json = r#"{"q": "web-scraper", "page": 3}"#;
        let query: SearchQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.q, "web-scraper");
        assert_eq!(query.page, 3);
    }

    #[test]
    fn test_install_request_without_version() {
        let json = r#"{"name": "my-skill"}"#;
        let req: InstallRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "my-skill");
        assert!(req.version.is_none());
    }

    #[test]
    fn test_install_request_with_version() {
        let json = r#"{"name": "my-skill", "version": "1.2.0"}"#;
        let req: InstallRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "my-skill");
        assert_eq!(req.version.as_deref(), Some("1.2.0"));
    }

    #[test]
    fn test_installed_skill_info_serialization() {
        use mcclawd_core::clawhub::installer::SkillSource;
        let info = InstalledSkillInfo {
            name: "test-skill".to_string(),
            version: "0.1.0".to_string(),
            source: SkillSource::Registry {
                registry_url: "https://api.clawhub.com".to_string(),
            },
            installed_at: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("test-skill"));
        assert!(json.contains("0.1.0"));
        assert!(json.contains("Registry"));
    }

    #[test]
    fn test_clawhub_search_result_serialization() {
        use mcclawd_core::clawhub::{ClawHubSearchResult, ClawHubSkillMeta};
        let result = ClawHubSearchResult {
            skills: vec![ClawHubSkillMeta {
                name: "memory-store".to_string(),
                version: "1.0.0".to_string(),
                author: "clawhub".to_string(),
                description: "A memory skill".to_string(),
                downloads: 42,
                tags: vec!["memory".to_string()],
                updated_at: "2025-01-01".to_string(),
            }],
            total: 1,
            page: 0,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("memory-store"));
        assert!(json.contains("\"total\":1"));

        // Round-trip
        let parsed: ClawHubSearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.skills.len(), 1);
        assert_eq!(parsed.total, 1);
    }

    #[test]
    fn test_catalog_query_defaults() {
        let json = r#"{}"#;
        let query: CatalogQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.q, "");
        assert_eq!(query.page, 0);
        assert_eq!(query.per_page, 20);
    }

    #[test]
    fn test_catalog_query_with_params() {
        let json = r#"{"q": "memory", "page": 2, "per_page": 10}"#;
        let query: CatalogQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.q, "memory");
        assert_eq!(query.page, 2);
        assert_eq!(query.per_page, 10);
    }
}
