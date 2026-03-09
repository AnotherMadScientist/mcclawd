//! Structured audit logging hook with pluggable sinks.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::pipeline::SecurityContext;
use super::SecurityHook;

/// Action being audited.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum AuditAction {
    PreCall,
    PostCall,
}

/// A structured audit event recorded by the AuditHook.
#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {
    pub timestamp: DateTime<Utc>,
    pub tool_name: String,
    pub action: AuditAction,
    pub args_summary: String,
    pub result_size: usize,
    pub duration_ms: Option<u64>,
    pub dlp_flags: Vec<String>,
    // Enriched fields — populated from SecurityContext when available.
    pub task_id: Option<String>,
    pub event_type: String,
    pub threat_level: String,
    pub action_taken: String,
    pub direction: String,
}

impl AuditEvent {
    fn new_pre(tool_name: &str, args: &serde_json::Value) -> Self {
        Self {
            timestamp: Utc::now(),
            tool_name: tool_name.to_string(),
            action: AuditAction::PreCall,
            args_summary: args.to_string(),
            result_size: 0,
            duration_ms: None,
            dlp_flags: vec![],
            task_id: None,
            event_type: "audit".to_string(),
            threat_level: "safe".to_string(),
            action_taken: "allowed".to_string(),
            direction: "inbound".to_string(),
        }
    }

    fn new_post(tool_name: &str, result: &serde_json::Value) -> Self {
        let result_str = result.to_string();
        Self {
            timestamp: Utc::now(),
            tool_name: tool_name.to_string(),
            action: AuditAction::PostCall,
            args_summary: String::new(),
            result_size: result_str.len(),
            duration_ms: None,
            dlp_flags: vec![],
            task_id: None,
            event_type: "audit".to_string(),
            threat_level: "safe".to_string(),
            action_taken: "allowed".to_string(),
            direction: "outbound".to_string(),
        }
    }
}

/// Trait for audit event sinks — where events are recorded.
pub trait AuditSink: Send + Sync {
    fn record(&self, event: &AuditEvent);
}

/// Default sink: writes structured events via tracing::info.
pub struct TracingAuditSink;

impl AuditSink for TracingAuditSink {
    fn record(&self, event: &AuditEvent) {
        tracing::info!(
            tool = %event.tool_name,
            action = ?event.action,
            args_summary = %event.args_summary,
            result_size = event.result_size,
            duration_ms = ?event.duration_ms,
            dlp_flags = ?event.dlp_flags,
            task_id = ?event.task_id,
            threat_level = %event.threat_level,
            "audit_event"
        );
    }
}

/// File-based sink: appends JSONL to a file path.
pub struct FileAuditSink {
    path: PathBuf,
}

impl FileAuditSink {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl AuditSink for FileAuditSink {
    fn record(&self, event: &AuditEvent) {
        if let Ok(line) = serde_json::to_string(event) {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
            {
                let _ = writeln!(f, "{}", line);
            }
        }
    }
}

/// Structured audit hook that records events through a pluggable sink.
pub struct AuditHook {
    sink: Arc<dyn AuditSink>,
    /// Shared pipeline context — read to enrich events with task_id + findings.
    context: Option<Arc<RwLock<SecurityContext>>>,
}

impl AuditHook {
    /// Create an AuditHook with a custom sink (no context).
    pub fn new(sink: Arc<dyn AuditSink>) -> Self {
        Self { sink, context: None }
    }

    /// Attach the shared pipeline context to enrich persisted events.
    pub fn with_context(mut self, context: Arc<RwLock<SecurityContext>>) -> Self {
        self.context = Some(context);
        self
    }

    /// Create an AuditHook with the default TracingAuditSink.
    pub fn with_tracing() -> Self {
        Self {
            sink: Arc::new(TracingAuditSink),
            context: None,
        }
    }

    /// Read the shared context and enrich the event.
    async fn enrich(&self, event: &mut AuditEvent) {
        if let Some(ctx) = &self.context {
            let ctx = ctx.read().await;
            event.task_id = ctx.task_id.clone();
            event.threat_level = ctx.threat_level.clone();
            event.dlp_flags = ctx
                .findings
                .iter()
                .map(|f| f.pattern_name.clone())
                .collect();
            // Determine event_type from findings
            if ctx.findings.iter().any(|f| f.finding_type == "secret_detected") {
                event.event_type = "secret_detected".to_string();
            } else if !ctx.findings.is_empty() {
                event.event_type = "dlp_match".to_string();
            }
            // Determine action_taken
            if ctx.was_blocked {
                event.action_taken = "blocked".to_string();
            } else if !ctx.findings.is_empty() {
                event.action_taken = "warned".to_string();
            }
        }
    }
}

#[async_trait]
impl SecurityHook for AuditHook {
    async fn before_tool_call(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> crate::Result<()> {
        let mut event = AuditEvent::new_pre(tool_name, args);
        self.enrich(&mut event).await;
        self.sink.record(&event);
        Ok(())
    }

    async fn after_tool_call(
        &self,
        tool_name: &str,
        result: &serde_json::Value,
    ) -> crate::Result<()> {
        let mut event = AuditEvent::new_post(tool_name, result);
        self.enrich(&mut event).await;
        self.sink.record(&event);
        Ok(())
    }
}

/// PostgreSQL-backed audit sink — writes security events + DLP findings to the database.
pub struct PgAuditSink {
    pool: sqlx::PgPool,
    /// Shared pipeline context — read inside the spawned task to persist findings.
    context: Option<Arc<RwLock<SecurityContext>>>,
}

impl PgAuditSink {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool, context: None }
    }

    /// Attach the shared pipeline context so findings get persisted alongside the event.
    pub fn with_context(mut self, context: Arc<RwLock<SecurityContext>>) -> Self {
        self.context = Some(context);
        self
    }
}

impl AuditSink for PgAuditSink {
    fn record(&self, event: &AuditEvent) {
        let pool = self.pool.clone();
        let event = event.clone();
        let context = self.context.clone();

        // Spawn async write — AuditSink::record is sync, so we fire-and-forget.
        tokio::spawn(async move {
            // Snapshot findings from context so we can persist them.
            let (task_id, findings, threat_level, action_taken) = if let Some(ctx) = context {
                let ctx = ctx.read().await;
                let findings = ctx.findings.clone();
                let threat_level = ctx.threat_level.clone();
                let action_taken = if ctx.was_blocked {
                    "blocked".to_string()
                } else if !findings.is_empty() {
                    "warned".to_string()
                } else {
                    "allowed".to_string()
                };
                (ctx.task_id.clone(), findings, threat_level, action_taken)
            } else {
                (
                    event.task_id.clone(),
                    vec![],
                    event.threat_level.clone(),
                    event.action_taken.clone(),
                )
            };

            // Only persist events that have actual security findings AND a non-allowed action.
            // The audit log is for warnings, blocks, and redactions — not clean scans.
            if findings.is_empty() || action_taken == "allowed" {
                return;
            }

            let direction = match event.action {
                AuditAction::PreCall => "inbound",
                AuditAction::PostCall => "outbound",
            };

            let details = serde_json::json!({
                "args_summary": event.args_summary,
                "result_size": event.result_size,
                "duration_ms": event.duration_ms,
                "dlp_flags": event.dlp_flags,
            });

            // Determine event_type: prefer specific types over generic "audit".
            let event_type = if findings.iter().any(|f| f.finding_type == "secret_detected") {
                "secret_detected"
            } else if findings.iter().any(|f| f.finding_type == "dlp_match") {
                "dlp_match"
            } else {
                "audit"
            };

            let threat_level_opt = if threat_level == "safe" {
                None
            } else {
                Some(threat_level.as_str())
            };

            let security_event_id: Option<i64> = match sqlx::query_scalar::<_, i64>(
                "INSERT INTO security_events \
                 (task_id, user_id, event_type, tool_name, direction, threat_level, details, action_taken) \
                 VALUES ($1, 'admin', $2, $3, $4, $5, $6, $7) \
                 RETURNING id",
            )
            .bind(task_id.as_deref())
            .bind(event_type)
            .bind(&event.tool_name)
            .bind(direction)
            .bind(threat_level_opt)
            .bind(&details)
            .bind(&action_taken)
            .fetch_one(&pool)
            .await
            {
                Ok(id) => Some(id),
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to write audit event to postgres");
                    None
                }
            };

            // Insert per-finding rows if we have a valid security_event_id.
            if let Some(event_id) = security_event_id {
                for finding in &findings {
                    if let Err(e) = sqlx::query(
                        "INSERT INTO dlp_findings \
                         (security_event_id, finding_type, tag, pattern_name, confidence, redacted_preview, source_text, match_offset, match_length) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
                    )
                    .bind(event_id)
                    .bind(&finding.finding_type)
                    .bind(&finding.tag)
                    .bind(&finding.pattern_name)
                    .bind(finding.confidence as f32)
                    .bind(finding.redacted_preview.as_deref())
                    .bind(finding.source_text.as_deref())
                    .bind(finding.match_offset)
                    .bind(finding.match_length)
                    .execute(&pool)
                    .await
                    {
                        tracing::warn!(error = %e, "Failed to write DLP finding to postgres");
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Test sink that collects events for assertion.
    struct CollectorSink {
        events: Mutex<Vec<AuditEvent>>,
    }

    impl CollectorSink {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }
    }

    impl AuditSink for CollectorSink {
        fn record(&self, event: &AuditEvent) {
            self.events.lock().unwrap().push(event.clone());
        }
    }

    #[tokio::test]
    async fn audit_hook_records_before_event() {
        let sink = Arc::new(CollectorSink::new());
        let hook = AuditHook::new(sink.clone());
        let args = serde_json::json!({"file": "test.txt"});

        hook.before_tool_call("read_file", &args).await.unwrap();

        let events = sink.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].tool_name, "read_file");
        assert_eq!(events[0].action, AuditAction::PreCall);
        assert!(events[0].args_summary.contains("test.txt"));
    }

    #[tokio::test]
    async fn audit_hook_records_after_event() {
        let sink = Arc::new(CollectorSink::new());
        let hook = AuditHook::new(sink.clone());
        let result = serde_json::json!({"content": "hello world"});

        hook.after_tool_call("read_file", &result).await.unwrap();

        let events = sink.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, AuditAction::PostCall);
        assert!(events[0].result_size > 0);
    }

    #[tokio::test]
    async fn tracing_audit_sink_does_not_panic() {
        let hook = AuditHook::with_tracing();
        let args = serde_json::json!({"key": "value"});
        // Should not panic
        hook.before_tool_call("test_tool", &args).await.unwrap();
        hook.after_tool_call("test_tool", &args).await.unwrap();
    }

    #[tokio::test]
    async fn file_audit_sink_writes_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");
        let sink = Arc::new(FileAuditSink::new(path.clone()));
        let hook = AuditHook::new(sink);

        let args = serde_json::json!({"cmd": "ls"});
        hook.before_tool_call("exec", &args).await.unwrap();
        hook.after_tool_call("exec", &serde_json::json!("ok"))
            .await
            .unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 2);

        // Each line should be valid JSON
        for line in &lines {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(parsed.get("tool_name").is_some());
        }
    }

    #[tokio::test]
    async fn audit_hook_enriches_from_context() {
        let ctx = Arc::new(RwLock::new(SecurityContext::new()));
        {
            let mut ctx = ctx.write().await;
            ctx.task_id = Some("task-xyz".to_string());
            ctx.threat_level = "suspicious".to_string();
            ctx.findings.push(super::super::pipeline::PendingFinding {
                finding_type: "dlp_match".to_string(),
                tag: "dlp:credit_card_number".to_string(),
                pattern_name: "Credit Card Number".to_string(),
                confidence: 1.0,
                redacted_preview: None,
                source_text: None,
                match_offset: None,
                match_length: None,
            });
        }

        let sink = Arc::new(CollectorSink::new());
        let hook = AuditHook::new(sink.clone()).with_context(ctx);
        hook.before_tool_call("send_payment", &serde_json::json!({}))
            .await
            .unwrap();

        let events = sink.events.lock().unwrap();
        assert_eq!(events[0].task_id, Some("task-xyz".to_string()));
        assert_eq!(events[0].threat_level, "suspicious");
        assert_eq!(events[0].event_type, "dlp_match");
        assert_eq!(events[0].dlp_flags, vec!["Credit Card Number"]);
    }
}
