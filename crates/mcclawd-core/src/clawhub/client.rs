//! ClawHub registry API client for searching and downloading skills.

use serde::{Deserialize, Serialize};

/// ClawHub API client for searching and downloading skills.
#[derive(Debug, Clone)]
pub struct ClawHubClient {
    base_url: String,
    http: reqwest::Client,
}

/// Metadata about a skill in the ClawHub registry.
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

/// Paginated search results from the ClawHub registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClawHubSearchResult {
    pub skills: Vec<ClawHubSkillMeta>,
    pub total: u64,
    pub page: u64,
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

    /// Search for skills matching a query.
    /// GET /api/v1/skills/search?q={query}&page={page}
    pub async fn search(&self, query: &str, page: u64) -> anyhow::Result<ClawHubSearchResult> {
        let url = self.api_url(&format!(
            "/skills/search?q={}&page={}",
            urlencoding::encode(query),
            page
        ));
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

        resp.json::<ClawHubSearchResult>()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse ClawHub search response: {e}"))
    }

    /// Get metadata for a specific skill.
    /// GET /api/v1/skills/{name}
    /// If version is Some, GET /api/v1/skills/{name}/versions/{version}
    pub async fn get_skill(
        &self,
        name: &str,
        version: Option<&str>,
    ) -> anyhow::Result<ClawHubSkillMeta> {
        let path = match version {
            Some(v) => format!("/skills/{}/versions/{}", name, v),
            None => format!("/skills/{}", name),
        };
        let url = self.api_url(&path);
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

        resp.json::<ClawHubSkillMeta>()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse ClawHub skill response: {e}"))
    }

    /// Download a skill package (tar.gz bytes).
    /// GET /api/v1/skills/{name}/versions/{version}/download
    pub async fn download_skill(&self, name: &str, version: &str) -> anyhow::Result<Vec<u8>> {
        let url = self.api_url(&format!(
            "/skills/{}/versions/{}/download",
            name, version
        ));
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

    /// Build URL for an API endpoint.
    pub(crate) fn api_url(&self, path: &str) -> String {
        format!("{}/api/v1{}", self.base_url, path)
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
        let client = ClawHubClient::new("https://api.clawhub.com");
        assert_eq!(client.base_url(), "https://api.clawhub.com");
    }

    #[test]
    fn test_client_strips_trailing_slash() {
        let client = ClawHubClient::new("https://api.clawhub.com/");
        assert_eq!(client.base_url(), "https://api.clawhub.com");
    }

    #[test]
    fn test_api_url_search() {
        let client = ClawHubClient::new("https://api.clawhub.com");
        let url = client.api_url("/skills/search?q=test&page=0");
        assert_eq!(
            url,
            "https://api.clawhub.com/api/v1/skills/search?q=test&page=0"
        );
    }

    #[test]
    fn test_api_url_skill() {
        let client = ClawHubClient::new("https://api.clawhub.com");
        let url = client.api_url("/skills/code-review");
        assert_eq!(
            url,
            "https://api.clawhub.com/api/v1/skills/code-review"
        );
    }

    #[test]
    fn test_api_url_skill_version() {
        let client = ClawHubClient::new("https://api.clawhub.com");
        let url = client.api_url("/skills/code-review/versions/1.0.0");
        assert_eq!(
            url,
            "https://api.clawhub.com/api/v1/skills/code-review/versions/1.0.0"
        );
    }

    #[test]
    fn test_api_url_download() {
        let client = ClawHubClient::new("http://localhost:8080");
        let url = client.api_url("/skills/code-review/versions/1.0.0/download");
        assert_eq!(
            url,
            "http://localhost:8080/api/v1/skills/code-review/versions/1.0.0/download"
        );
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
        assert_eq!(parsed.description, "Automated code review skill");
        assert_eq!(parsed.downloads, 1234);
        assert_eq!(parsed.tags, vec!["review", "quality"]);
        assert_eq!(parsed.updated_at, "2025-01-15T10:30:00Z");
    }

    #[test]
    fn test_search_result_serde_roundtrip() {
        let result = ClawHubSearchResult {
            skills: vec![
                ClawHubSkillMeta {
                    name: "code-review".to_string(),
                    version: "1.0.0".to_string(),
                    author: "alice".to_string(),
                    description: "Code review".to_string(),
                    downloads: 100,
                    tags: vec!["review".to_string()],
                    updated_at: "2025-01-01T00:00:00Z".to_string(),
                },
                ClawHubSkillMeta {
                    name: "test-runner".to_string(),
                    version: "2.0.0".to_string(),
                    author: "bob".to_string(),
                    description: "Test runner".to_string(),
                    downloads: 200,
                    tags: vec!["testing".to_string()],
                    updated_at: "2025-02-01T00:00:00Z".to_string(),
                },
            ],
            total: 42,
            page: 0,
        };

        let json = serde_json::to_string(&result).unwrap();
        let parsed: ClawHubSearchResult = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.skills.len(), 2);
        assert_eq!(parsed.total, 42);
        assert_eq!(parsed.page, 0);
        assert_eq!(parsed.skills[0].name, "code-review");
        assert_eq!(parsed.skills[1].name, "test-runner");
    }

    #[test]
    fn test_search_result_empty() {
        let json = r#"{"skills": [], "total": 0, "page": 0}"#;
        let parsed: ClawHubSearchResult = serde_json::from_str(json).unwrap();
        assert!(parsed.skills.is_empty());
        assert_eq!(parsed.total, 0);
    }

    #[test]
    fn test_skill_meta_from_json() {
        let json = r#"{
            "name": "web-scraper",
            "version": "0.5.0",
            "author": "clawhub",
            "description": "Web scraping skill with MCP tools",
            "downloads": 999,
            "tags": ["web", "scraping", "mcp"],
            "updated_at": "2025-03-10T12:00:00Z"
        }"#;

        let meta: ClawHubSkillMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.name, "web-scraper");
        assert_eq!(meta.tags.len(), 3);
        assert_eq!(meta.downloads, 999);
    }

    #[test]
    fn test_urlencoding() {
        assert_eq!(urlencoding::encode("hello world"), "hello%20world");
        assert_eq!(urlencoding::encode("code-review"), "code-review");
        assert_eq!(urlencoding::encode("a+b=c"), "a%2Bb%3Dc");
        assert_eq!(urlencoding::encode("simple"), "simple");
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
