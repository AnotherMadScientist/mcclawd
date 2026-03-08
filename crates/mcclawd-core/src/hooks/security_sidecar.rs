//! Security sidecar hook -- calls the mcclawd-security container via HTTP.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::SecurityHook;
use crate::McclawdError;

/// Response from the sidecar /scan endpoint.
#[derive(Debug, Deserialize)]
pub struct ScanResponse {
    pub detections: Vec<SidecarDetection>,
    pub tags: Vec<String>,
    pub threat_level: String,
    pub action: String,
    pub scan_time_ms: f64,
}

#[derive(Debug, Deserialize)]
pub struct SidecarDetection {
    pub detector: String,
    pub finding_type: String,
    pub tag: String,
    pub pattern_name: String,
    pub confidence: f64,
    pub redacted_preview: Option<String>,
}

/// Request body for the sidecar /scan endpoint.
#[derive(Debug, Serialize)]
struct ScanRequest {
    text: String,
    context: String,
    tool_name: Option<String>,
    trace_id: Option<String>,
    span_id: Option<String>,
}

/// Hook that calls the security sidecar container for deep analysis.
pub struct SecuritySidecarHook {
    client: reqwest::Client,
    base_url: String,
}

impl SecuritySidecarHook {
    pub fn new(base_url: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .pool_max_idle_per_host(4)
            .build()
            .expect("Failed to build reqwest client");
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Check if the sidecar is healthy.
    pub async fn health_check(&self) -> bool {
        self.client
            .get(format!("{}/health", self.base_url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    async fn scan(
        &self,
        text: &str,
        context: &str,
        tool_name: Option<&str>,
    ) -> Result<ScanResponse, McclawdError> {
        let req = ScanRequest {
            text: text.to_string(),
            context: context.to_string(),
            tool_name: tool_name.map(|s| s.to_string()),
            trace_id: None,
            span_id: None,
        };

        let resp = self
            .client
            .post(format!("{}/scan", self.base_url))
            .json(&req)
            .send()
            .await
            .map_err(|e| McclawdError::Config(format!("Security sidecar request failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(McclawdError::Config(format!(
                "Security sidecar returned {}",
                resp.status()
            )));
        }

        resp.json::<ScanResponse>()
            .await
            .map_err(|e| McclawdError::Config(format!("Security sidecar response parse error: {e}")))
    }
}

#[async_trait]
impl SecurityHook for SecuritySidecarHook {
    async fn before_tool_call(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> crate::Result<()> {
        let text = args.to_string();
        match self.scan(&text, "tool_args", Some(tool_name)).await {
            Ok(resp) => {
                if resp.action == "blocked" {
                    let tags: Vec<String> =
                        resp.detections.iter().map(|d| d.tag.clone()).collect();
                    tracing::warn!(
                        tool = %tool_name,
                        threat = %resp.threat_level,
                        tags = ?tags,
                        scan_ms = resp.scan_time_ms,
                        "Security sidecar BLOCKED tool call"
                    );
                    return Err(McclawdError::Config(format!(
                        "Tool call '{}' blocked by security sidecar: {} (tags: {})",
                        tool_name,
                        resp.threat_level,
                        tags.join(", ")
                    )));
                }
                if resp.action == "warned" {
                    tracing::warn!(
                        tool = %tool_name,
                        threat = %resp.threat_level,
                        tags = ?resp.tags,
                        "Security sidecar WARNING on tool call"
                    );
                }
                Ok(())
            }
            Err(e) => {
                // Fail-open: if sidecar is down, warn but allow
                tracing::warn!(
                    tool = %tool_name,
                    error = %e,
                    "Security sidecar unavailable, fail-open"
                );
                Ok(())
            }
        }
    }

    async fn after_tool_call(
        &self,
        tool_name: &str,
        result: &serde_json::Value,
    ) -> crate::Result<()> {
        let text = result.to_string();
        match self.scan(&text, "tool_result", Some(tool_name)).await {
            Ok(resp) => {
                if resp.action == "blocked" {
                    let tags: Vec<String> =
                        resp.detections.iter().map(|d| d.tag.clone()).collect();
                    tracing::warn!(
                        tool = %tool_name,
                        threat = %resp.threat_level,
                        tags = ?tags,
                        "Security sidecar BLOCKED tool result"
                    );
                    return Err(McclawdError::Config(format!(
                        "Tool result from '{}' blocked by security sidecar: {} (tags: {})",
                        tool_name,
                        resp.threat_level,
                        tags.join(", ")
                    )));
                }
                Ok(())
            }
            Err(e) => {
                tracing::warn!(
                    tool = %tool_name,
                    error = %e,
                    "Security sidecar unavailable (after), fail-open"
                );
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_with_default_url() {
        let hook = SecuritySidecarHook::new("http://localhost:8082");
        assert_eq!(hook.base_url, "http://localhost:8082");
    }

    #[test]
    fn trims_trailing_slash() {
        let hook = SecuritySidecarHook::new("http://localhost:8082/");
        assert_eq!(hook.base_url, "http://localhost:8082");
    }
}
