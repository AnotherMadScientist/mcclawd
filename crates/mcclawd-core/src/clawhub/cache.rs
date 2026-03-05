//! File-based cache for ClawHub skill catalog metadata.
//!
//! Stores the full skill catalog locally so the UI can show skills instantly
//! without hitting the network on every request. The cache is persisted as
//! JSON on disk and refreshed on demand.

use super::client::{ClawHubClient, ClawHubSkillMeta};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Cached catalog stored as JSON on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedCatalog {
    /// All known skills from the registry.
    pub skills: Vec<ClawHubSkillMeta>,
    /// When the cache was last refreshed.
    pub last_refreshed: DateTime<Utc>,
    /// Total skills known in the registry.
    pub total: u64,
}

/// File-based cache for ClawHub skill metadata.
///
/// Stores catalog at `{cache_dir}/clawhub_catalog.json`.
/// Provides sync (background refresh) and search (local filter) operations.
pub struct ClawHubCache {
    cache_path: PathBuf,
    client: ClawHubClient,
    /// In-memory catalog, loaded from disk or refreshed from the registry.
    pub(crate) catalog: Arc<RwLock<Option<CachedCatalog>>>,
}

impl ClawHubCache {
    /// Create a new cache. `cache_dir` is typically `~/.mcclawd/cache/`.
    pub fn new(cache_dir: &Path, client: ClawHubClient) -> Self {
        let cache_path = cache_dir.join("clawhub_catalog.json");
        let catalog = Arc::new(RwLock::new(None));
        Self {
            cache_path,
            client,
            catalog,
        }
    }

    /// Load catalog from disk if it exists.
    pub async fn load_from_disk(&self) -> anyhow::Result<()> {
        if self.cache_path.exists() {
            let content = tokio::fs::read_to_string(&self.cache_path).await?;
            let cached: CachedCatalog = serde_json::from_str(&content)?;
            *self.catalog.write().await = Some(cached);
        }
        Ok(())
    }

    /// Save current catalog to disk (atomic: write temp + rename).
    async fn save_to_disk(&self, catalog: &CachedCatalog) -> anyhow::Result<()> {
        if let Some(parent) = self.cache_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let json = serde_json::to_string_pretty(catalog)?;
        let tmp = self.cache_path.with_extension("json.tmp");
        tokio::fs::write(&tmp, &json).await?;
        tokio::fs::rename(&tmp, &self.cache_path).await?;
        Ok(())
    }

    /// Refresh the cache from the ClawHub registry using cursor-based pagination.
    /// Returns the number of skills cached.
    pub async fn refresh(&self) -> anyhow::Result<usize> {
        self.refresh_with_progress(|_, _| {}).await
    }

    /// Refresh with a progress callback called after each batch: (fetched_so_far, batch_skills).
    pub async fn refresh_with_progress<F>(
        &self,
        mut on_batch: F,
    ) -> anyhow::Result<usize>
    where
        F: FnMut(usize, &[ClawHubSkillMeta]),
    {
        let mut all_skills = Vec::new();
        let mut cursor: Option<String> = None;
        let max_pages = 100;

        for _ in 0..max_pages {
            match self.client.list_skills(200, cursor.as_deref()).await {
                Ok(result) => {
                    let batch_len = result.skills.len();
                    all_skills.extend(result.skills.clone());

                    // Save incrementally and notify
                    let catalog = CachedCatalog {
                        skills: all_skills.clone(),
                        last_refreshed: Utc::now(),
                        total: all_skills.len() as u64,
                    };
                    let _ = self.save_to_disk(&catalog).await;
                    *self.catalog.write().await = Some(catalog);
                    on_batch(all_skills.len(), &result.skills);

                    if batch_len == 0 || result.next_cursor.is_none() {
                        break;
                    }
                    cursor = result.next_cursor;
                }
                Err(e) => {
                    tracing::warn!("ClawHub registry unreachable: {e}");
                    if all_skills.is_empty() {
                        anyhow::bail!("ClawHub registry unreachable: {e}");
                    }
                    break;
                }
            }
        }

        Ok(all_skills.len())
    }

    /// Search the cached catalog locally (case-insensitive substring match on name, description, tags).
    pub async fn search(&self, query: &str, page: u64, per_page: u64) -> CachedSearchResult {
        let catalog = self.catalog.read().await;
        match catalog.as_ref() {
            None => CachedSearchResult {
                skills: vec![],
                total: 0,
                page,
                cached: false,
                last_refreshed: None,
            },
            Some(cat) => {
                let query_lower = query.to_lowercase();
                let matches: Vec<_> = if query.is_empty() {
                    cat.skills.clone()
                } else {
                    cat.skills
                        .iter()
                        .filter(|s| {
                            s.name.to_lowercase().contains(&query_lower)
                                || s.description.to_lowercase().contains(&query_lower)
                                || s.tags
                                    .iter()
                                    .any(|t| t.to_lowercase().contains(&query_lower))
                                || s.author.to_lowercase().contains(&query_lower)
                        })
                        .cloned()
                        .collect()
                };
                let total = matches.len() as u64;
                let start = (page * per_page) as usize;
                let skills = matches.into_iter().skip(start).take(per_page as usize).collect();
                CachedSearchResult {
                    skills,
                    total,
                    page,
                    cached: true,
                    last_refreshed: Some(cat.last_refreshed),
                }
            }
        }
    }

    /// Get a single skill's metadata by name from the cache.
    pub async fn get_skill(&self, name: &str) -> Option<ClawHubSkillMeta> {
        let catalog = self.catalog.read().await;
        catalog
            .as_ref()
            .and_then(|cat| cat.skills.iter().find(|s| s.name == name).cloned())
    }

    /// Check if the cache is stale (older than the given duration).
    pub async fn is_stale(&self, max_age: std::time::Duration) -> bool {
        let catalog = self.catalog.read().await;
        match catalog.as_ref() {
            None => true,
            Some(cat) => {
                let age = Utc::now().signed_duration_since(cat.last_refreshed);
                age.to_std().unwrap_or(std::time::Duration::MAX) > max_age
            }
        }
    }

    /// Get cache statistics.
    pub async fn stats(&self) -> CacheStats {
        let catalog = self.catalog.read().await;
        match catalog.as_ref() {
            None => CacheStats {
                skill_count: 0,
                last_refreshed: None,
                cache_path: self.cache_path.clone(),
            },
            Some(cat) => CacheStats {
                skill_count: cat.skills.len(),
                last_refreshed: Some(cat.last_refreshed),
                cache_path: self.cache_path.clone(),
            },
        }
    }

    /// Set the catalog directly (for testing).
    #[cfg(test)]
    pub async fn set_catalog_for_test(&self, catalog: CachedCatalog) {
        *self.catalog.write().await = Some(catalog);
    }
}

/// Result from a cached local search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedSearchResult {
    pub skills: Vec<ClawHubSkillMeta>,
    pub total: u64,
    pub page: u64,
    pub cached: bool,
    pub last_refreshed: Option<DateTime<Utc>>,
}

/// Cache statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub skill_count: usize,
    pub last_refreshed: Option<DateTime<Utc>>,
    pub cache_path: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_skill(name: &str, desc: &str, tags: &[&str], author: &str) -> ClawHubSkillMeta {
        ClawHubSkillMeta {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            author: author.to_string(),
            description: desc.to_string(),
            downloads: 100,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            updated_at: "2025-01-01".to_string(),
        }
    }

    fn make_test_catalog() -> CachedCatalog {
        CachedCatalog {
            skills: vec![
                make_skill("memory-store", "Store and recall memories", &["memory", "storage"], "clawhub"),
                make_skill("web-scraper", "Scrape web pages", &["web", "scraping"], "community"),
                make_skill("code-runner", "Run code snippets", &["code", "execution"], "clawhub"),
                make_skill("file-manager", "Manage files on disk", &["files", "storage"], "community"),
                make_skill("api-caller", "Call external APIs", &["api", "http"], "clawhub"),
            ],
            last_refreshed: Utc::now(),
            total: 5,
        }
    }

    #[test]
    fn test_cached_catalog_serde_roundtrip() {
        let catalog = make_test_catalog();
        let json = serde_json::to_string(&catalog).unwrap();
        let parsed: CachedCatalog = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.skills.len(), 5);
        assert_eq!(parsed.total, 5);
        assert_eq!(parsed.skills[0].name, "memory-store");
    }

    #[tokio::test]
    async fn test_search_empty_catalog() {
        let client = ClawHubClient::new("http://localhost:9999");
        let cache = ClawHubCache::new(Path::new("/tmp/test-cache-empty"), client);
        let result = cache.search("anything", 0, 20).await;
        assert_eq!(result.skills.len(), 0);
        assert_eq!(result.total, 0);
        assert!(!result.cached);
        assert!(result.last_refreshed.is_none());
    }

    #[tokio::test]
    async fn test_search_filters_by_name() {
        let client = ClawHubClient::new("http://localhost:9999");
        let cache = ClawHubCache::new(Path::new("/tmp/test-cache-name"), client);
        cache.set_catalog_for_test(make_test_catalog()).await;

        let result = cache.search("memory", 0, 20).await;
        assert_eq!(result.total, 1);
        assert_eq!(result.skills[0].name, "memory-store");
        assert!(result.cached);
    }

    #[tokio::test]
    async fn test_search_filters_by_tag() {
        let client = ClawHubClient::new("http://localhost:9999");
        let cache = ClawHubCache::new(Path::new("/tmp/test-cache-tag"), client);
        cache.set_catalog_for_test(make_test_catalog()).await;

        let result = cache.search("storage", 0, 20).await;
        assert_eq!(result.total, 2);
        let names: Vec<_> = result.skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"memory-store"));
        assert!(names.contains(&"file-manager"));
    }

    #[tokio::test]
    async fn test_search_pagination() {
        let client = ClawHubClient::new("http://localhost:9999");
        let cache = ClawHubCache::new(Path::new("/tmp/test-cache-page"), client);
        cache.set_catalog_for_test(make_test_catalog()).await;

        // All 5 skills, page size 2
        let page0 = cache.search("", 0, 2).await;
        assert_eq!(page0.total, 5);
        assert_eq!(page0.skills.len(), 2);
        assert_eq!(page0.page, 0);

        let page1 = cache.search("", 1, 2).await;
        assert_eq!(page1.total, 5);
        assert_eq!(page1.skills.len(), 2);
        assert_eq!(page1.page, 1);

        let page2 = cache.search("", 2, 2).await;
        assert_eq!(page2.total, 5);
        assert_eq!(page2.skills.len(), 1); // last page has 1
        assert_eq!(page2.page, 2);

        // Beyond last page
        let page3 = cache.search("", 3, 2).await;
        assert_eq!(page3.skills.len(), 0);
    }

    #[tokio::test]
    async fn test_get_skill_from_cache() {
        let client = ClawHubClient::new("http://localhost:9999");
        let cache = ClawHubCache::new(Path::new("/tmp/test-cache-get"), client);
        cache.set_catalog_for_test(make_test_catalog()).await;

        let skill = cache.get_skill("web-scraper").await;
        assert!(skill.is_some());
        assert_eq!(skill.unwrap().name, "web-scraper");

        let missing = cache.get_skill("nonexistent").await;
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn test_is_stale_empty() {
        let client = ClawHubClient::new("http://localhost:9999");
        let cache = ClawHubCache::new(Path::new("/tmp/test-cache-stale"), client);
        assert!(cache.is_stale(std::time::Duration::from_secs(3600)).await);
    }

    #[tokio::test]
    async fn test_cache_stats_empty() {
        let client = ClawHubClient::new("http://localhost:9999");
        let cache = ClawHubCache::new(Path::new("/tmp/test-cache-stats"), client);
        let stats = cache.stats().await;
        assert_eq!(stats.skill_count, 0);
        assert!(stats.last_refreshed.is_none());
    }

    #[tokio::test]
    async fn test_cache_stats_with_data() {
        let client = ClawHubClient::new("http://localhost:9999");
        let cache = ClawHubCache::new(Path::new("/tmp/test-cache-stats2"), client);
        cache.set_catalog_for_test(make_test_catalog()).await;
        let stats = cache.stats().await;
        assert_eq!(stats.skill_count, 5);
        assert!(stats.last_refreshed.is_some());
    }

    #[tokio::test]
    async fn test_search_by_author() {
        let client = ClawHubClient::new("http://localhost:9999");
        let cache = ClawHubCache::new(Path::new("/tmp/test-cache-author"), client);
        cache.set_catalog_for_test(make_test_catalog()).await;

        let result = cache.search("community", 0, 20).await;
        assert_eq!(result.total, 2);
    }

    #[tokio::test]
    async fn test_search_case_insensitive() {
        let client = ClawHubClient::new("http://localhost:9999");
        let cache = ClawHubCache::new(Path::new("/tmp/test-cache-case"), client);
        cache.set_catalog_for_test(make_test_catalog()).await;

        let result = cache.search("MEMORY", 0, 20).await;
        assert_eq!(result.total, 1);
        assert_eq!(result.skills[0].name, "memory-store");
    }

    #[tokio::test]
    async fn test_is_stale_with_fresh_data() {
        let client = ClawHubClient::new("http://localhost:9999");
        let cache = ClawHubCache::new(Path::new("/tmp/test-cache-fresh"), client);
        cache.set_catalog_for_test(make_test_catalog()).await;
        // Just set, so it should not be stale for 1 hour
        assert!(!cache.is_stale(std::time::Duration::from_secs(3600)).await);
    }
}
