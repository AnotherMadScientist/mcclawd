//! Structured audit logging hook with pluggable sinks.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;

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
}

impl AuditHook {
    /// Create an AuditHook with a custom sink.
    pub fn new(sink: Arc<dyn AuditSink>) -> Self {
        Self { sink }
    }

    /// Create an AuditHook with the default TracingAuditSink.
    pub fn with_tracing() -> Self {
        Self {
            sink: Arc::new(TracingAuditSink),
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
        let event = AuditEvent {
            timestamp: Utc::now(),
            tool_name: tool_name.to_string(),
            action: AuditAction::PreCall,
            args_summary: args.to_string(),
            result_size: 0,
            duration_ms: None,
            dlp_flags: vec![],
        };
        self.sink.record(&event);
        Ok(())
    }

    async fn after_tool_call(
        &self,
        tool_name: &str,
        result: &serde_json::Value,
    ) -> crate::Result<()> {
        let result_str = result.to_string();
        let event = AuditEvent {
            timestamp: Utc::now(),
            tool_name: tool_name.to_string(),
            action: AuditAction::PostCall,
            args_summary: String::new(),
            result_size: result_str.len(),
            duration_ms: None,
            dlp_flags: vec![],
        };
        self.sink.record(&event);
        Ok(())
    }
}

/// PostgreSQL-backed audit sink -- writes security events to the database.
/// Requires a sqlx::PgPool. Events are written synchronously (per-event).
/// For production, consider adding batch insert with a background flush task.
pub struct PgAuditSink {
    pool: sqlx::PgPool,
}

impl PgAuditSink {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

impl AuditSink for PgAuditSink {
    fn record(&self, event: &AuditEvent) {
        let pool = self.pool.clone();
        let event = event.clone();
        // Spawn async write -- AuditSink::record is sync, so we fire-and-forget
        tokio::spawn(async move {
            let details = serde_json::json!({
                "args_summary": event.args_summary,
                "result_size": event.result_size,
                "duration_ms": event.duration_ms,
                "dlp_flags": event.dlp_flags,
            });
            let action_str = match event.action {
                AuditAction::PreCall => "pre_call",
                AuditAction::PostCall => "post_call",
            };
            if let Err(e) = sqlx::query(
                "INSERT INTO security_events (event_type, tool_name, direction, details, action_taken)
                 VALUES ('audit', $1, $2, $3, 'allowed')",
            )
            .bind(&event.tool_name)
            .bind(action_str)
            .bind(&details)
            .execute(&pool)
            .await
            {
                tracing::warn!(error = %e, "Failed to write audit event to postgres");
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
}
