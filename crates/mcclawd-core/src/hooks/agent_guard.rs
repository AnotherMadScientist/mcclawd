//! AgentGuard sidecar integration hook.
//!
//! Calls the AgentGuard HTTP sidecar to analyze tool inputs and outputs
//! for threats. Fails open on connection errors (never blocks when sidecar
//! is unavailable).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::pipeline::{PendingFinding, SecurityContext};
use super::SecurityHook;
use crate::McclawdError;

// ── Serde structs ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct AnalyzeRequest {
    text: String,
    context: String,
}

#[derive(Deserialize)]
struct AnalyzeResponse {
    threat_level: String,
    detections: Vec<Detection>,
    summary: String,
}

#[derive(Deserialize)]
struct Detection {
    category: String,
    pattern: String,
    confidence: f64,
}

// ── Hook ─────────────────────────────────────────────────────────────────────

/// SecurityHook that calls the AgentGuard sidecar via HTTP.
pub struct AgentGuardHook {
    client: reqwest::Client,
    base_url: String,
    context: Option<Arc<RwLock<SecurityContext>>>,
}

impl AgentGuardHook {
    /// Create a new hook pointing at `base_url` (e.g. `http://localhost:8082`).
    /// Uses a 2-second request timeout.
    pub fn new(base_url: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .expect("reqwest client build failed");

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            context: None,
        }
    }

    /// Attach the shared pipeline context so findings get persisted.
    pub fn with_context(mut self, context: Arc<RwLock<SecurityContext>>) -> Self {
        self.context = Some(context);
        self
    }

    pub fn name(&self) -> &str {
        "agent_guard"
    }

    /// Call `/analyze` and handle the response.
    ///
    /// - "dangerous" / "critical" → return `Err` (blocks the call)
    /// - "suspicious"             → warn, record findings, allow
    /// - "safe" / anything else   → allow
    /// - connection error         → fail-open (warn, allow)
    async fn analyze(&self, text: String, context_label: &str) -> crate::Result<()> {
        let url = format!("{}/analyze", self.base_url);

        let resp = match self
            .client
            .post(&url)
            .json(&AnalyzeRequest {
                text,
                context: context_label.to_string(),
            })
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    sidecar = %self.base_url,
                    "AgentGuard sidecar unreachable — failing open"
                );
                return Ok(());
            }
        };

        let body: AnalyzeResponse = match resp.json().await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "AgentGuard returned unparseable response — failing open"
                );
                return Ok(());
            }
        };

        match body.threat_level.as_str() {
            "dangerous" | "critical" => {
                self.push_findings(&body.detections, &body.threat_level).await;
                return Err(McclawdError::Tool(format!(
                    "AgentGuard blocked call: threat_level={} summary={}",
                    body.threat_level, body.summary
                )));
            }
            "suspicious" => {
                tracing::warn!(
                    threat_level = %body.threat_level,
                    summary = %body.summary,
                    detections = body.detections.len(),
                    "AgentGuard flagged suspicious content — allowing"
                );
                self.push_findings(&body.detections, &body.threat_level).await;
            }
            _ => {}
        }

        Ok(())
    }

    /// Push detections into the shared SecurityContext.
    async fn push_findings(&self, detections: &[Detection], threat_level: &str) {
        if let Some(ctx) = &self.context {
            let mut ctx = ctx.write().await;
            for d in detections {
                ctx.findings.push(PendingFinding {
                    finding_type: "agent_guard_detection".to_string(),
                    tag: format!("category:{}", d.category),
                    pattern_name: d.pattern.clone(),
                    confidence: d.confidence,
                    redacted_preview: None,
                    source_text: None,
                    match_offset: None,
                    match_length: None,
                });
            }
            ctx.elevate_threat(threat_level);
        }
    }
}

#[async_trait]
impl SecurityHook for AgentGuardHook {
    async fn before_tool_call(
        &self,
        _tool_name: &str,
        args: &serde_json::Value,
    ) -> crate::Result<()> {
        self.analyze(args.to_string(), "tool_input").await
    }

    async fn after_tool_call(
        &self,
        _tool_name: &str,
        result: &serde_json::Value,
    ) -> crate::Result<()> {
        self.analyze(result.to_string(), "tool_output").await
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_guard_hook_creation() {
        let hook = AgentGuardHook::new("http://localhost:8082");
        assert_eq!(hook.name(), "agent_guard");
    }

    #[test]
    fn test_base_url_trailing_slash_stripped() {
        let hook = AgentGuardHook::new("http://localhost:8082/");
        assert_eq!(hook.base_url, "http://localhost:8082");
    }

    #[tokio::test]
    async fn test_fail_open_on_connection_error() {
        // Nothing listening on port 19999 — should fail open (Ok).
        let hook = AgentGuardHook::new("http://127.0.0.1:19999");
        let args = serde_json::json!({"cmd": "ls"});
        let result = hook.before_tool_call("shell", &args).await;
        assert!(result.is_ok(), "expected fail-open, got {:?}", result);
    }
}
