//! Security hooks — DLP scanning, secret detection, audit logging, and hook pipeline.
//!
//! Phase 0: AuditHook (tracing-based logging).
//! Phase 3+: DLP scanning, entropy-based secret detection, structured audit events.

pub mod audit;
pub mod dlp;
pub mod pipeline;
pub mod secret_scanner;

use async_trait::async_trait;

// Re-export everything for backward compatibility
pub use audit::{AuditAction, AuditEvent, AuditHook, AuditSink, FileAuditSink, TracingAuditSink};
pub use dlp::{DlpAction, DlpConfig, DlpHook, DlpPattern};
pub use pipeline::HookPipeline;
pub use secret_scanner::{SecretScannerConfig, SecretScannerHook};

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
