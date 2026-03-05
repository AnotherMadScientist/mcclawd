//! Skills API route handlers — list, search, install, uninstall skills via ClawHub.
//!
//! Includes local catalog cache endpoints for fast UI browsing.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{
        sse::{Event, Sse},
        IntoResponse,
    },
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use std::convert::Infallible;

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
/// Falls back to installing from cached metadata when the registry is unreachable.
pub async fn install_skill(
    State(state): State<AppState>,
    Json(body): Json<InstallRequest>,
) -> Result<Json<InstalledSkillInfo>, impl IntoResponse> {
    let (_, installer) = build_installer(&state).await;
    let version = body.version.as_deref();

    // Try real registry first
    match installer.install_from_registry(&body.name, version).await {
        Ok(info) => return Ok(Json(info)),
        Err(e) => {
            tracing::warn!("Registry install failed for '{}', trying cache fallback: {e}", body.name);
        }
    }

    // Fallback: install from cached metadata (generates stub SKILL.md)
    let cache = build_cache(&state).await;
    match cache.get_skill(&body.name).await {
        Some(meta) => match installer.install_from_meta(&meta) {
            Ok(info) => Ok(Json(info)),
            Err(e) => {
                tracing::error!("Failed to install skill '{}' from cache: {e}", body.name);
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("Failed to install skill: {e}")})),
                ))
            }
        },
        None => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("Skill '{}' not found in registry or cache", body.name)})),
        )),
    }
}

/// GET /api/skills/{name}/content — read the full SKILL.md text.
/// Tries: 1) installed on disk, 2) download from ClawHub.
pub async fn skill_content(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, impl IntoResponse> {
    // 1. Try installed skill on disk
    let config = state.config.read().await;
    let skill_md_path = config.skills.managed_dir.join(&name).join("SKILL.md");
    let clawhub_api = config.skills.clawhub_api.clone();
    drop(config);

    if let Ok(content) = tokio::fs::read_to_string(&skill_md_path).await {
        return Ok(Json(serde_json::json!({ "name": name, "content": content })));
    }

    // 2. Try downloading from ClawHub (get version from cache first)
    let cache = build_cache(&state).await;
    let version = cache
        .get_skill(&name)
        .await
        .map(|s| s.version)
        .unwrap_or_else(|| "latest".to_string());

    let client = ClawHubClient::new(&clawhub_api);
    match client.download_skill_md(&name, &version).await {
        Ok(content) => Ok(Json(serde_json::json!({ "name": name, "content": content }))),
        Err(e) => {
            tracing::debug!("Could not fetch SKILL.md for '{name}': {e}");
            Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": format!("SKILL.md not available for '{name}'")})),
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

/// GET /api/skills/refresh-stream — SSE stream of refresh progress.
/// Sends events as batches arrive: {"fetched": N} and final {"done": true, "total": N}.
pub async fn refresh_catalog_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let cache = build_cache(&state).await;
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(64);

    tokio::spawn(async move {
        let tx2 = tx.clone();
        let result = cache
            .refresh_with_progress(|fetched, _batch| {
                let data = serde_json::json!({"fetched": fetched}).to_string();
                let _ = tx2.try_send(data);
            })
            .await;

        let final_data = match result {
            Ok(total) => serde_json::json!({"done": true, "total": total}).to_string(),
            Err(e) => serde_json::json!({"done": true, "total": 0, "error": e.to_string()}).to_string(),
        };
        let _ = tx.send(final_data).await;
    });

    let stream = futures_util::stream::unfold(rx, |mut rx| async {
        rx.recv().await.map(|data| (Ok(Event::default().data(data)), rx))
    });

    Sse::new(stream)
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
