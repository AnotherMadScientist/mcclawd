//! Security hooks — DLP scanning, secret detection, audit logging, and hook pipeline.
//!
//! Phase 0: AuditHook (tracing-based logging).
//! Phase 3+: DLP scanning, entropy-based secret detection, structured audit events.

pub mod audit;
pub mod dlp;
pub mod pipeline;
pub mod secret_scanner;
pub mod security_event;
pub mod security_sidecar;
pub mod taint_trace;
pub mod user_hook;

use async_trait::async_trait;

// Re-export everything for backward compatibility
pub use audit::{AuditAction, AuditEvent, AuditHook, AuditSink, FileAuditSink, PgAuditSink, TracingAuditSink};
pub use dlp::{DlpAction, DlpConfig, DlpHook, DlpPattern};
pub use pipeline::{HookPipeline, PendingFinding, SecurityContext};
pub use secret_scanner::{SecretScannerConfig, SecretScannerHook};
pub use security_event::{DlpFinding, ScanDirection, SecurityAction, SecurityEvent, SecurityEventType, ThreatLevel};
pub use security_sidecar::SecuritySidecarHook;
pub use taint_trace::{TaintSpan, TaintTrace};
pub use user_hook::{UserHook, UserHookAction, UserHookConfig, UserHookTrigger, UserHookType};

/// Hook called before/after tool dispatch.
/// Phase 0: audit logging via tracing.
/// Phase 3+: DLP scanning, secret detection, taint tracking.
#[async_trait]
pub trait SecurityHook: Send + Sync {
    async fn before_tool_call(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> crate::Result<()>;
    async fn after_tool_call(
        &self,
        tool_name: &str,
        result: &serde_json::Value,
    ) -> crate::Result<()>;
}
