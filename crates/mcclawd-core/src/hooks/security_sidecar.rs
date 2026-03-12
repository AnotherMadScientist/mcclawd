//! Security sidecar hook -- calls the mcclawd-security container via HTTP.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::pipeline::{PendingFinding, SecurityContext};
use super::SecurityHook;
use crate::McclawdError;
use std::sync::Arc;
use tokio::sync::RwLock;

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
///
/// Uses Presidio, detect-secrets, and custom patterns for PII/secret/injection detection.
/// Pushes all detections into the shared SecurityContext for taint trace tracking.
pub struct SecuritySidecarHook {
    client: reqwest::Client,
    base_url: String,
    context: Option<Arc<RwLock<SecurityContext>>>,
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
            context: None,
        }
    }

    /// Attach shared security context for pushing findings into taint trace.
    pub fn with_context(mut self, context: Arc<RwLock<SecurityContext>>) -> Self {
        self.context = Some(context);
        self
    }

    /// Push sidecar detections into the shared SecurityContext.
    async fn push_findings(&self, detections: &[SidecarDetection], threat_level: &str) {
        if let Some(ref ctx) = self.context {
            let mut guard = ctx.write().await;
            for d in detections {
                guard.findings.push(PendingFinding {
                    finding_type: d.finding_type.clone(),
                    tag: d.tag.clone(),
                    pattern_name: d.pattern_name.clone(),
                    confidence: d.confidence,
                    redacted_preview: d.redacted_preview.clone(),
                    source_text: None,
                    match_offset: None,
                    match_length: None,
                });
            }
            guard.elevate_threat(threat_level);
        }
    }

    /// Evaluate the accumulated taint trace via the sidecar's /trace/evaluate endpoint.
    pub async fn evaluate_taint_trace(
        &self,
        trace: &super::taint_trace::TaintTrace,
    ) -> Result<Vec<serde_json::Value>, McclawdError> {
        let messages = trace.to_invariant_messages();
        let req = serde_json::json!({
            "messages": messages,
            "trace_id": trace.trace_id,
        });

        let resp = self
            .client
            .post(format!("{}/trace/evaluate", self.base_url))
            .json(&req)
            .send()
            .await
            .map_err(|e| McclawdError::Config(format!("Trace eval request failed: {e}")))?;

        if !resp.status().is_success() {
            return Ok(vec![]); // Fail-open: evaluation endpoint may not be available
        }

        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let violations = body
            .get("violations")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(violations)
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
                // Push all detections into SecurityContext for taint tracking
                self.push_findings(&resp.detections, &resp.threat_level).await;

                if resp.action == "blocked" {
                    let tags: Vec<String> =
                        resp.detections.iter().map(|d| d.tag.clone()).collect();
                    tracing::warn!(
                        tool = %tool_name,
                        threat = %resp.threat_level,
                        detections = resp.detections.len(),
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
                        detections = resp.detections.len(),
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
                // Push detections for taint tracking (output scanning)
                self.push_findings(&resp.detections, &resp.threat_level).await;

                if resp.action == "blocked" {
                    let tags: Vec<String> =
                        resp.detections.iter().map(|d| d.tag.clone()).collect();
                    tracing::warn!(
                        tool = %tool_name,
                        threat = %resp.threat_level,
                        detections = resp.detections.len(),
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
