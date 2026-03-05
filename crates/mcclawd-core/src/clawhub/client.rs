//! ClawHub registry API client for searching and downloading skills.
//!
//! Talks to the real ClawHub API at `https://clawhub.ai/api/v1/`.
//! Maps ClawHub response fields (slug, displayName, summary) to our
//! internal `ClawHubSkillMeta` format.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// ClawHub API client for searching and downloading skills.
#[derive(Debug, Clone)]
pub struct ClawHubClient {
    base_url: String,
    http: reqwest::Client,
}

/// Metadata about a skill in the ClawHub registry (our internal format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClawHubSkillMeta {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub downloads: u64,
    pub tags: Vec<String>,
    pub updated_at: String,
}

/// Paginated search results from the ClawHub registry (our internal format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClawHubSearchResult {
    pub skills: Vec<ClawHubSkillMeta>,
    pub total: u64,
    pub page: u64,
}

// ---------------------------------------------------------------------------
// ClawHub raw API response types (for deserialization only)
// ---------------------------------------------------------------------------

/// GET /api/v1/skills response
#[derive(Debug, Deserialize)]
struct RawListResponse {
    items: Vec<RawListItem>,
    #[serde(rename = "nextCursor")]
    next_cursor: Option<String>,
}

/// A single item in the list response.
#[derive(Debug, Deserialize)]
struct RawListItem {
    slug: String,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    summary: Option<String>,
    #[serde(default)]
    tags: HashMap<String, String>,
    #[serde(default)]
    stats: RawStats,
    #[serde(rename = "updatedAt")]
    updated_at: Option<u64>,
    #[serde(rename = "latestVersion")]
    latest_version: Option<RawLatestVersion>,
    #[allow(dead_code)]
    metadata: Option<serde_json::Value>,
}

/// GET /api/v1/search response
#[derive(Debug, Deserialize)]
struct RawSearchResponse {
    results: Vec<RawSearchResult>,
}

/// A single item in the search response.
#[derive(Debug, Deserialize)]
struct RawSearchResult {
    #[allow(dead_code)]
    score: Option<f64>,
    slug: String,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    summary: Option<String>,
    version: Option<String>,
    #[serde(rename = "updatedAt")]
    updated_at: Option<u64>,
}

/// GET /api/v1/skills/:slug response
#[derive(Debug, Deserialize)]
struct RawDetailResponse {
    skill: RawDetailSkill,
    #[serde(rename = "latestVersion")]
    latest_version: Option<RawLatestVersion>,
    owner: Option<RawOwner>,
    #[allow(dead_code)]
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct RawDetailSkill {
    slug: String,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    summary: Option<String>,
    #[serde(default)]
    tags: HashMap<String, String>,
    #[serde(default)]
    stats: RawStats,
    #[serde(rename = "updatedAt")]
    updated_at: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct RawStats {
    #[serde(default)]
    downloads: u64,
    #[serde(default, rename = "installsAllTime")]
    installs_all_time: u64,
    #[serde(default)]
    stars: u64,
    #[serde(default)]
    versions: u64,
}

#[derive(Debug, Deserialize)]
struct RawLatestVersion {
    version: String,
    #[allow(dead_code)]
    #[serde(rename = "createdAt")]
    created_at: Option<u64>,
    #[allow(dead_code)]
    changelog: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawOwner {
    handle: Option<String>,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    #[allow(dead_code)]
    image: Option<String>,
}

// ---------------------------------------------------------------------------
// Mapping helpers
// ---------------------------------------------------------------------------

fn epoch_ms_to_iso(ms: Option<u64>) -> String {
    match ms {
        Some(ts) => {
            let secs = (ts / 1000) as i64;
            let nanos = ((ts % 1000) * 1_000_000) as u32;
            match chrono::DateTime::from_timestamp(secs, nanos) {
                Some(dt) => dt.to_rfc3339(),
                None => "unknown".to_string(),
            }
        }
        None => "unknown".to_string(),
    }
}

fn extract_tags(tags: &HashMap<String, String>) -> Vec<String> {
    tags.keys()
        .filter(|k| k.as_str() != "latest")
        .cloned()
        .collect()
}

impl RawListItem {
    fn into_meta(self) -> ClawHubSkillMeta {
        let version = self
            .latest_version
            .as_ref()
            .map(|v| v.version.clone())
            .or_else(|| self.tags.get("latest").cloned())
            .unwrap_or_else(|| "0.0.0".to_string());
        ClawHubSkillMeta {
            name: self.slug,
            version,
            author: String::new(), // list endpoint doesn't include owner
            description: self.summary.unwrap_or_default(),
            downloads: self.stats.downloads + self.stats.installs_all_time,
            tags: extract_tags(&self.tags),
            updated_at: epoch_ms_to_iso(self.updated_at),
        }
    }
}

impl RawSearchResult {
    fn into_meta(self) -> ClawHubSkillMeta {
        ClawHubSkillMeta {
            name: self.slug,
            version: self.version.unwrap_or_else(|| "latest".to_string()),
            author: String::new(),
            description: self.summary.unwrap_or_default(),
            downloads: 0,
            tags: vec![],
            updated_at: epoch_ms_to_iso(self.updated_at),
        }
    }
}

// ---------------------------------------------------------------------------
// Client implementation
// ---------------------------------------------------------------------------

/// Result of listing skills — includes a cursor for pagination.
pub struct ListResult {
    pub skills: Vec<ClawHubSkillMeta>,
    pub next_cursor: Option<String>,
}

impl ClawHubClient {
    /// Create a new ClawHub client pointing at the given registry URL.
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Create a client with a custom reqwest::Client (useful for testing).
    pub fn with_http(base_url: &str, http: reqwest::Client) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
        }
    }

    /// List skills from the registry with cursor-based pagination.
    /// GET /api/v1/skills?limit={limit}&sort=updated&cursor={cursor}
    pub async fn list_skills(
        &self,
        limit: u32,
        cursor: Option<&str>,
    ) -> anyhow::Result<ListResult> {
        let mut url = format!(
            "{}/api/v1/skills?limit={}&sort=updated",
            self.base_url, limit
        );
        if let Some(c) = cursor {
            url.push_str("&cursor=");
            url.push_str(c);
        }

        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("ClawHub list request failed: {e}"))?;

        if !resp.status().is_success() {
            anyhow::bail!(
                "ClawHub list failed with status {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            );
        }

        let raw: RawListResponse = resp
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse ClawHub list response: {e}"))?;

        Ok(ListResult {
            skills: raw.items.into_iter().map(|i| i.into_meta()).collect(),
            next_cursor: raw.next_cursor,
        })
    }

    /// Search for skills matching a query (vector search).
    /// GET /api/v1/search?q={query}&limit={limit}
    pub async fn search(&self, query: &str, page: u64) -> anyhow::Result<ClawHubSearchResult> {
        let limit = 20;
        let url = format!(
            "{}/api/v1/search?q={}&limit={}",
            self.base_url,
            urlencoding::encode(query),
            limit
        );
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("ClawHub search request failed: {e}"))?;

        if !resp.status().is_success() {
            anyhow::bail!(
                "ClawHub search failed with status {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            );
        }

        let raw: RawSearchResponse = resp
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse ClawHub search response: {e}"))?;

        let skills: Vec<ClawHubSkillMeta> =
            raw.results.into_iter().map(|r| r.into_meta()).collect();
        let total = skills.len() as u64;

        Ok(ClawHubSearchResult {
            skills,
            total,
            page,
        })
    }

    /// Get metadata for a specific skill.
    /// GET /api/v1/skills/{slug}
    pub async fn get_skill(
        &self,
        name: &str,
        _version: Option<&str>,
    ) -> anyhow::Result<ClawHubSkillMeta> {
        let url = format!("{}/api/v1/skills/{}", self.base_url, name);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("ClawHub get_skill request failed: {e}"))?;

        if !resp.status().is_success() {
            anyhow::bail!(
                "Skill '{}' not found in ClawHub (status {})",
                name,
                resp.status()
            );
        }

        let raw: RawDetailResponse = resp
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse ClawHub skill response: {e}"))?;

        let version = raw
            .latest_version
            .as_ref()
            .map(|v| v.version.clone())
            .or_else(|| raw.skill.tags.get("latest").cloned())
            .unwrap_or_else(|| "0.0.0".to_string());

        let author = raw
            .owner
            .as_ref()
            .and_then(|o| o.display_name.as_deref().or(o.handle.as_deref()))
            .unwrap_or("unknown")
            .to_string();

        Ok(ClawHubSkillMeta {
            name: raw.skill.slug,
            version,
            author,
            description: raw.skill.summary.unwrap_or_default(),
            downloads: raw.skill.stats.downloads + raw.skill.stats.installs_all_time,
            tags: extract_tags(&raw.skill.tags),
            updated_at: epoch_ms_to_iso(raw.skill.updated_at),
        })
    }

    /// Download a skill package (tar.gz bytes).
    /// GET /api/v1/download?slug={slug}&version={version}
    pub async fn download_skill(&self, name: &str, version: &str) -> anyhow::Result<Vec<u8>> {
        let url = format!(
            "{}/api/v1/download?slug={}&version={}",
            self.base_url, name, version
        );
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("ClawHub download request failed: {e}"))?;

        if !resp.status().is_success() {
            anyhow::bail!(
                "Failed to download skill '{}' v{} (status {})",
                name,
                version,
                resp.status()
            );
        }

        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| anyhow::anyhow!("Failed to read skill download bytes: {e}"))
    }

    /// Download a skill package and extract just the SKILL.md text content.
    /// The package is a ZIP file; we extract SKILL.md from it in-memory.
    pub async fn download_skill_md(&self, name: &str, version: &str) -> anyhow::Result<String> {
        let bytes = self.download_skill(name, version).await?;
        let cursor = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor)
            .map_err(|e| anyhow::anyhow!("Failed to open skill package as ZIP: {e}"))?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            if file.name() == "SKILL.md" || file.name().ends_with("/SKILL.md") {
                let mut content = String::new();
                std::io::Read::read_to_string(&mut file, &mut content)?;
                return Ok(content);
            }
        }
        anyhow::bail!("SKILL.md not found in downloaded package for '{name}'")
    }

    /// Get the base URL of this client.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

/// Simple percent-encoding module for URL query parameters.
mod urlencoding {
    pub fn encode(input: &str) -> String {
        let mut result = String::with_capacity(input.len());
        for byte in input.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    result.push(byte as char);
                }
                _ => {
                    result.push_str(&format!("%{:02X}", byte));
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_construction() {
        let client = ClawHubClient::new("https://clawhub.ai");
        assert_eq!(client.base_url(), "https://clawhub.ai");
    }

    #[test]
    fn test_client_strips_trailing_slash() {
        let client = ClawHubClient::new("https://clawhub.ai/");
        assert_eq!(client.base_url(), "https://clawhub.ai");
    }

    #[test]
    fn test_skill_meta_serde_roundtrip() {
        let meta = ClawHubSkillMeta {
            name: "code-review".to_string(),
            version: "1.2.0".to_string(),
            author: "macleodlabs".to_string(),
            description: "Automated code review skill".to_string(),
            downloads: 1234,
            tags: vec!["review".to_string(), "quality".to_string()],
            updated_at: "2025-01-15T10:30:00Z".to_string(),
        };

        let json = serde_json::to_string(&meta).unwrap();
        let parsed: ClawHubSkillMeta = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.name, "code-review");
        assert_eq!(parsed.version, "1.2.0");
        assert_eq!(parsed.author, "macleodlabs");
        assert_eq!(parsed.downloads, 1234);
    }

    #[test]
    fn test_search_result_serde_roundtrip() {
        let result = ClawHubSearchResult {
            skills: vec![ClawHubSkillMeta {
                name: "code-review".to_string(),
                version: "1.0.0".to_string(),
                author: "alice".to_string(),
                description: "Code review".to_string(),
                downloads: 100,
                tags: vec!["review".to_string()],
                updated_at: "2025-01-01T00:00:00Z".to_string(),
            }],
            total: 42,
            page: 0,
        };

        let json = serde_json::to_string(&result).unwrap();
        let parsed: ClawHubSearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.skills.len(), 1);
        assert_eq!(parsed.total, 42);
    }

    #[test]
    fn test_epoch_ms_to_iso() {
        let iso = epoch_ms_to_iso(Some(1772594548523));
        assert!(iso.starts_with("2026"));
        assert_eq!(epoch_ms_to_iso(None), "unknown");
    }

    #[test]
    fn test_extract_tags() {
        let mut tags = HashMap::new();
        tags.insert("latest".to_string(), "1.0.0".to_string());
        tags.insert("memory".to_string(), "1.0.0".to_string());
        tags.insert("vector".to_string(), "1.0.0".to_string());
        let result = extract_tags(&tags);
        assert_eq!(result.len(), 2);
        assert!(!result.contains(&"latest".to_string()));
    }

    #[test]
    fn test_raw_list_item_into_meta() {
        let item = RawListItem {
            slug: "test-skill".to_string(),
            display_name: Some("Test Skill".to_string()),
            summary: Some("A test skill".to_string()),
            tags: {
                let mut m = HashMap::new();
                m.insert("latest".to_string(), "1.2.3".to_string());
                m.insert("testing".to_string(), "1.0.0".to_string());
                m
            },
            stats: RawStats {
                downloads: 100,
                installs_all_time: 50,
                stars: 5,
                versions: 3,
            },
            updated_at: Some(1772594548523),
            latest_version: Some(RawLatestVersion {
                version: "1.2.3".to_string(),
                created_at: None,
                changelog: None,
            }),
            metadata: None,
        };
        let meta = item.into_meta();
        assert_eq!(meta.name, "test-skill");
        assert_eq!(meta.version, "1.2.3");
        assert_eq!(meta.description, "A test skill");
        assert_eq!(meta.downloads, 150);
        assert_eq!(meta.tags, vec!["testing"]);
    }

    #[test]
    fn test_raw_search_result_into_meta() {
        let result = RawSearchResult {
            score: Some(3.5),
            slug: "memory-store".to_string(),
            display_name: Some("Memory Store".to_string()),
            summary: Some("Store memories".to_string()),
            version: None,
            updated_at: Some(1772594548523),
        };
        let meta = result.into_meta();
        assert_eq!(meta.name, "memory-store");
        assert_eq!(meta.version, "latest");
        assert_eq!(meta.description, "Store memories");
    }

    #[test]
    fn test_urlencoding() {
        assert_eq!(urlencoding::encode("hello world"), "hello%20world");
        assert_eq!(urlencoding::encode("code-review"), "code-review");
        assert_eq!(urlencoding::encode("a+b=c"), "a%2Bb%3Dc");
    }

    #[test]
    fn test_client_with_custom_http() {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();
        let client = ClawHubClient::with_http("https://custom.registry.io", http);
        assert_eq!(client.base_url(), "https://custom.registry.io");
    }
}
