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
use mcclawd_core::scanner::{self, ScanIssue, ScanResult, ScanStatus};

use super::state::AppState;

/// Installed skill info enriched with scan results.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InstalledSkillWithScan {
    #[serde(flatten)]
    pub info: InstalledSkillInfo,
    /// Whether this skill has a stub SKILL.md (< 500 bytes or no `## ` sections).
    pub is_stub: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scan_status: Option<ScanStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scan_issues: Option<Vec<scanner::ScanIssue>>,
}

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

/// Request body for creating a local skill.
#[derive(Debug, Deserialize)]
pub struct CreateSkillRequest {
    pub name: String,
    pub content: String,
}

/// POST /api/skills/create — write a new local SKILL.md to the managed skills dir.
pub async fn create_skill(
    State(state): State<AppState>,
    Json(body): Json<CreateSkillRequest>,
) -> impl IntoResponse {
    if body.name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "name is required"})),
        )
            .into_response();
    }

    let config = state.config.read().await;
    let skill_dir = config.skills.managed_dir.join(&body.name);
    drop(config);

    if let Err(e) = std::fs::create_dir_all(&skill_dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to create skill directory: {e}")})),
        )
            .into_response();
    }

    let skill_md_path = skill_dir.join("SKILL.md");
    if let Err(e) = std::fs::write(&skill_md_path, &body.content) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to write SKILL.md: {e}")})),
        )
            .into_response();
    }

    tracing::info!("Created local skill '{}' at {:?}", body.name, skill_md_path);
    (
        StatusCode::CREATED,
        Json(serde_json::json!({"name": body.name, "path": skill_md_path.to_string_lossy()})),
    )
        .into_response()
}

/// Helper: build a ClawHubClient + SkillInstaller from current config.
async fn build_installer(state: &AppState) -> (ClawHubClient, SkillInstaller) {
    let config = state.config.read().await;
    let client = ClawHubClient::new(&config.skills.clawhub_api);
    let installer = SkillInstaller::new(client.clone(), config.skills.managed_dir.clone());
    (client, installer)
}

/// GET /api/skills — list installed skills.
/// Skips directories that have no SKILL.md file at all.
/// Marks stubs (< 500 bytes or no `## ` sections) with `is_stub: true`.
pub async fn list_installed(
    State(state): State<AppState>,
) -> Result<Json<Vec<InstalledSkillWithScan>>, impl IntoResponse> {
    let (_, installer) = build_installer(&state).await;
    let config = state.config.read().await;
    let managed_dir = config.skills.managed_dir.clone();
    drop(config);

    match installer.list_installed() {
        Ok(skills) => {
            let enriched: Vec<InstalledSkillWithScan> = skills
                .into_iter()
                .filter(|info| {
                    // Skip dirs that have no SKILL.md at all
                    let skill_md = managed_dir.join(&info.name).join("SKILL.md");
                    skill_md.exists()
                })
                .map(|info| {
                    let scan = state.scan_cache.get(&info.name).map(|r| r.clone());

                    // Detect stub: < 500 bytes or no `## ` section headers
                    let is_stub = {
                        let skill_md = managed_dir.join(&info.name).join("SKILL.md");
                        match std::fs::read_to_string(&skill_md) {
                            Ok(content) => content.len() < 500 || !content.contains("## "),
                            Err(_) => true,
                        }
                    };

                    InstalledSkillWithScan {
                        scan_status: scan.as_ref().map(|s| s.status.clone()),
                        scan_issues: scan.map(|s| s.issues.clone()),
                        is_stub,
                        info,
                    }
                })
                .collect();
            Ok(Json(enriched))
        }
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
        Ok(info) => {
            // Fire-and-forget: persist to Postgres
            let store = state.pg_store.clone();
            let name = body.name.clone();
            let ver = body.version.clone();
            let scan_cache = state.scan_cache.clone();
            let config = state.config.read().await;
            let skill_dir = config.skills.managed_dir.join(&name);
            let clawhub_api = config.skills.clawhub_api.clone();
            drop(config);
            tokio::spawn(async move {
                if let Err(e) = store.save_skill("admin", &name, ver.as_deref(), None).await {
                    tracing::warn!("Failed to persist installed skill to DB: {e}");
                }
                // Check if installed SKILL.md is a stub; upgrade before scanning
                if skill_dir.exists() {
                    let skill_md_path = skill_dir.join("SKILL.md");
                    let is_stub = if skill_md_path.exists() {
                        let content = tokio::fs::read_to_string(&skill_md_path).await.unwrap_or_default();
                        content.len() < 500 || !content.contains("## ")
                    } else {
                        true
                    };
                    if is_stub {
                        let client = mcclawd_core::clawhub::client::ClawHubClient::new(&clawhub_api);
                        if let Ok(content) = client.download_skill_md(&name, "latest").await {
                            let _ = tokio::fs::write(&skill_md_path, &content).await;
                            tracing::info!("Upgraded stub SKILL.md for '{name}' after install ({} bytes)", content.len());
                        }
                    }
                    // Auto-scan after install (with upgraded content if available)
                    if let Ok(result) = scanner::scan_skill(&skill_dir).await {
                        if let Ok(json_val) = serde_json::to_value(&result) {
                            if let Err(e) = store.save_scan_result("admin", &name, &json_val).await {
                                tracing::warn!("Failed to persist scan result: {e}");
                            }
                        }
                        scan_cache.insert(name, result);
                    }
                }
            });
            return Ok(Json(info));
        }
        Err(e) => {
            tracing::warn!("Registry install failed for '{}', trying cache fallback: {e}", body.name);
        }
    }

    // Fallback: install from cached metadata (generates stub SKILL.md)
    // Then try to upgrade stub with full content from ClawHub in background.
    let cache = build_cache(&state).await;
    match cache.get_skill(&body.name).await {
        Some(meta) => match installer.install_from_meta(&meta) {
            Ok(info) => {
                // Try to upgrade stub SKILL.md with full content from ClawHub
                let config2 = state.config.read().await;
                let skill_dir = config2.skills.managed_dir.join(&body.name);
                let clawhub_api = config2.skills.clawhub_api.clone();
                drop(config2);
                let name2 = body.name.clone();
                let scan_cache2 = state.scan_cache.clone();
                let store2 = state.pg_store.clone();
                tokio::spawn(async move {
                    let client = mcclawd_core::clawhub::client::ClawHubClient::new(&clawhub_api);
                    if let Ok(content) = client.download_skill_md(&name2, "latest").await {
                        let skill_md_path = skill_dir.join("SKILL.md");
                        if let Err(e) = tokio::fs::write(&skill_md_path, &content).await {
                            tracing::warn!("Failed to upgrade stub SKILL.md for '{name2}': {e}");
                            return;
                        }
                        tracing::info!("Upgraded stub SKILL.md for '{name2}' ({} bytes)", content.len());
                        // Auto-scan with full content
                        if let Ok(result) = scanner::scan_skill(&skill_dir).await {
                            if let Ok(json_val) = serde_json::to_value(&result) {
                                let _ = store2.save_scan_result("admin", &name2, &json_val).await;
                            }
                            scan_cache2.insert(name2, result);
                        }
                    }
                });
                Ok(Json(info))
            }
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
/// Tries: 1) installed on disk, 2) content cache, 3) download from ClawHub (and cache).
pub async fn skill_content(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, impl IntoResponse> {
    // 1. Try installed skill on disk
    let config = state.config.read().await;
    let skill_md_path = config.skills.managed_dir.join(&name).join("SKILL.md");
    drop(config);

    let disk_content = tokio::fs::read_to_string(&skill_md_path).await.ok();

    // If disk content looks like a stub (short, generated by install_from_cache),
    // prefer the full version from ClawHub cache/download.
    let is_stub = disk_content
        .as_ref()
        .map(|c| c.len() < 500 || !c.contains("## "))
        .unwrap_or(true);

    if !is_stub {
        if let Some(content) = disk_content.clone() {
            return Ok(Json(serde_json::json!({ "name": name, "content": content })));
        }
    }

    // 2. Try content cache, then download from ClawHub (caches on success)
    let cache = build_cache(&state).await;
    match cache.get_or_download_content(&name).await {
        Some(content) => Ok(Json(serde_json::json!({ "name": name, "content": content }))),
        None => {
            // Fall back to disk stub if ClawHub download failed
            if let Some(content) = disk_content {
                return Ok(Json(serde_json::json!({ "name": name, "content": content })));
            }
            tracing::debug!("SKILL.md not available for '{name}' (not installed, not cached, download failed)");
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
        Ok(()) => {
            // Fire-and-forget: remove from Postgres
            let store = state.pg_store.clone();
            let skill_name = name.clone();
            tokio::spawn(async move {
                if let Err(e) = store.delete_skill("admin", &skill_name).await {
                    tracing::warn!("Failed to delete skill from DB: {e}");
                }
            });
            Ok(StatusCode::NO_CONTENT)
        }
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

/// GET /api/skills/updates — check all installed skills for available updates (Gap 5).
pub async fn get_skill_updates(
    State(state): State<AppState>,
) -> Result<Json<Vec<mcclawd_core::clawhub::SkillUpdate>>, (StatusCode, Json<serde_json::Value>)> {
    let (_, installer) = build_installer(&state).await;
    match installer.check_for_updates().await {
        Ok(updates) => Ok(Json(updates)),
        Err(e) => {
            tracing::error!("Failed to check for skill updates: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("{e}")})),
            ))
        }
    }
}

/// GET /api/skills/{name}/scan — run security scan on a skill (installed or catalog).
/// Caches results in AppState.scan_cache (DashMap).
/// For uninstalled skills, downloads SKILL.md to a temp dir and scans there.
pub async fn scan_skill(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<ScanResult>, (StatusCode, Json<serde_json::Value>)> {
    // Always invalidate cache — user explicitly requested a fresh scan.
    // The cache is repopulated after the scan completes (line below: scan_cache.insert).
    state.scan_cache.remove(&name);

    // Try installed path first
    let config = state.config.read().await;
    let skill_path = config.skills.managed_dir.join(&name);
    let cache_dir = config.skills.cache_dir.clone();
    let clawhub_api = config.skills.clawhub_api.clone();
    drop(config);

    // Check if installed SKILL.md is a stub (< 500 bytes or no sections).
    // If so, try to download the full content first.
    let skill_md_file = skill_path.join("SKILL.md");
    let is_installed_stub = if skill_md_file.exists() {
        let content = tokio::fs::read_to_string(&skill_md_file).await.unwrap_or_default();
        content.len() < 500 || !content.contains("## ")
    } else {
        !skill_path.exists() // not installed at all
    };

    if is_installed_stub && skill_path.exists() {
        // Try to upgrade the stub with full content from ClawHub before scanning
        let client = mcclawd_core::clawhub::client::ClawHubClient::new(&clawhub_api.clone());
        match client.download_skill_md(&name, "latest").await {
            Ok(content) => {
                if let Err(e) = tokio::fs::write(&skill_md_file, &content).await {
                    tracing::warn!("Failed to write upgraded SKILL.md for '{name}': {e}");
                } else {
                    tracing::info!("Upgraded stub SKILL.md for scan of '{name}' ({} bytes)", content.len());
                }
            }
            Err(e) => {
                tracing::warn!("Could not download full SKILL.md for '{name}' from ClawHub: {e}");
                // Scanner will detect the stub and return NotScanned with explanation
            }
        }
    }

    let scan_path = if skill_path.exists() {
        skill_path
    } else {
        // Not installed — check if we have cached SKILL.md content
        let content_cache = cache_dir.join("skill_content").join(format!("{}.md", &name));
        if content_cache.exists() {
            // Create temp dir with SKILL.md for scanner
            let tmp = std::env::temp_dir().join(format!("mcclawd_scan_{}", &name));
            let _ = std::fs::create_dir_all(&tmp);
            let _ = std::fs::copy(&content_cache, tmp.join("SKILL.md"));
            tmp
        } else {
            // Try to download SKILL.md from ClawHub
            let client = mcclawd_core::clawhub::client::ClawHubClient::new(&clawhub_api);
            match client.download_skill_md(&name, "latest").await {
                Ok(content) => {
                    let tmp = std::env::temp_dir().join(format!("mcclawd_scan_{}", &name));
                    let _ = std::fs::create_dir_all(&tmp);
                    let _ = std::fs::write(tmp.join("SKILL.md"), &content);
                    tmp
                }
                Err(_) => {
                    // Can't get content — return NotScanned with explanation
                    let result = ScanResult {
                        status: ScanStatus::NotScanned,
                        issues: vec![ScanIssue {
                            code: "S003".to_string(),
                            severity: "info".to_string(),
                            description: format!(
                                "Skill '{}' is not installed and content could not be downloaded from ClawHub. \
                                 Install the skill first, then re-scan.",
                                name
                            ),
                        }],
                    };
                    return Ok(Json(result));
                }
            }
        }
    };

    match scanner::scan_skill(&scan_path).await {
        Ok(result) => {
            state.scan_cache.insert(name.clone(), result.clone());
            // Persist scan result to DB
            if let Ok(json_val) = serde_json::to_value(&result) {
                if let Err(e) = state.pg_store.save_scan_result("admin", &name, &json_val).await {
                    tracing::warn!("Failed to persist scan result: {e}");
                }
            }
            Ok(Json(result))
        }
        Err(e) => {
            tracing::error!("Scan failed for skill '{}': {e}", name);
            let result = ScanResult {
                status: ScanStatus::NotScanned,
                issues: vec![ScanIssue {
                    code: "S004".to_string(),
                    severity: "info".to_string(),
                    description: format!("Scan failed: {e}"),
                }],
            };
            Ok(Json(result))
        }
    }
}

/// POST /api/skills/{name}/preview-scan -- lightweight scan without full install.
/// Downloads SKILL.md from cache/ClawHub, runs basic_scan only (fast, no VT).
/// Returns result but does NOT persist to DB.
pub async fn preview_scan_skill(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<ScanResult>, (StatusCode, Json<serde_json::Value>)> {
    // Always invalidate cache — user explicitly requested a fresh scan.
    state.scan_cache.remove(&name);

    let config = state.config.read().await;
    let installed_path = config.skills.managed_dir.join(&name).join("SKILL.md");
    let cache_dir = config.skills.cache_dir.clone();
    let clawhub_api = config.skills.clawhub_api.clone();
    drop(config);

    let content = if installed_path.exists() {
        tokio::fs::read_to_string(&installed_path).await.ok()
    } else {
        // Try content cache
        let content_cache = cache_dir.join("skill_content").join(format!("{}.md", &name));
        if content_cache.exists() {
            tokio::fs::read_to_string(&content_cache).await.ok()
        } else {
            // Try downloading from ClawHub
            let client = mcclawd_core::clawhub::client::ClawHubClient::new(&clawhub_api);
            client.download_skill_md(&name, "latest").await.ok()
        }
    };

    match content {
        Some(content) => {
            let temp_dir = std::env::temp_dir().join(format!("mcclawd-preview-{name}"));
            let _ = tokio::fs::create_dir_all(&temp_dir).await;
            let _ = tokio::fs::write(temp_dir.join("SKILL.md"), &content).await;

            let result = scanner::basic_scan(&temp_dir)
                .await
                .unwrap_or(ScanResult {
                    status: ScanStatus::NotScanned,
                    issues: vec![],
                });

            let _ = tokio::fs::remove_dir_all(&temp_dir).await;
            Ok(Json(result))
        }
        None => {
            // Content unavailable (e.g. ClawHub 429 rate limit) — return NotScanned
            // instead of 404 so the frontend renders a neutral badge rather than an error.
            Ok(Json(ScanResult {
                status: ScanStatus::NotScanned,
                issues: vec![],
            }))
        }
    }
}

/// Response for the upgrade-stubs endpoint.
#[derive(Debug, serde::Serialize)]
pub struct UpgradeStubsResponse {
    pub upgraded: u32,
    pub failed: u32,
    pub skipped: u32,
    pub details: Vec<String>,
}

/// POST /api/skills/upgrade-stubs — attempt to download full SKILL.md for all stubs.
/// Iterates installed skill directories. For each stub or empty SKILL.md,
/// tries to download the full content from ClawHub (with retry).
pub async fn upgrade_stubs(
    State(state): State<AppState>,
) -> Json<UpgradeStubsResponse> {
    let config = state.config.read().await;
    let managed_dir = config.skills.managed_dir.clone();
    let clawhub_api = config.skills.clawhub_api.clone();
    drop(config);

    let mut upgraded = 0u32;
    let mut failed = 0u32;
    let mut skipped = 0u32;
    let mut details = Vec::new();

    let entries = match std::fs::read_dir(&managed_dir) {
        Ok(e) => e,
        Err(e) => {
            return Json(UpgradeStubsResponse {
                upgraded: 0,
                failed: 0,
                skipped: 0,
                details: vec![format!("Failed to read skills directory: {e}")],
            });
        }
    };

    let client = mcclawd_core::clawhub::client::ClawHubClient::new(&clawhub_api);

    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        let skill_md_path = entry.path().join("SKILL.md");

        // Determine if this needs upgrading
        let needs_upgrade = if !skill_md_path.exists() {
            true
        } else {
            match std::fs::read_to_string(&skill_md_path) {
                Ok(content) => content.len() < 500 || !content.contains("## "),
                Err(_) => true,
            }
        };

        if !needs_upgrade {
            skipped += 1;
            continue;
        }

        // Try downloading full content from ClawHub
        match client.download_skill_md(&name, "latest").await {
            Ok(content) => {
                if let Err(e) = tokio::fs::write(&skill_md_path, &content).await {
                    failed += 1;
                    details.push(format!("{name}: write failed: {e}"));
                } else {
                    upgraded += 1;
                    details.push(format!("{name}: upgraded ({} bytes)", content.len()));

                    // Re-scan with full content
                    let skill_dir = entry.path();
                    if let Ok(result) = scanner::scan_skill(&skill_dir).await {
                        if let Ok(json_val) = serde_json::to_value(&result) {
                            let _ = state
                                .pg_store
                                .save_scan_result("admin", &name, &json_val)
                                .await;
                        }
                        state.scan_cache.insert(name, result);
                    }
                }
            }
            Err(e) => {
                failed += 1;
                details.push(format!("{name}: download failed: {e}"));
            }
        }
    }

    tracing::info!(
        "Upgrade stubs complete: {upgraded} upgraded, {failed} failed, {skipped} skipped"
    );

    Json(UpgradeStubsResponse {
        upgraded,
        failed,
        skipped,
        details,
    })
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
