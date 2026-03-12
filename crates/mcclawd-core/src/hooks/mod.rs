//! Security hooks — DLP scanning, secret detection, redaction tokenization, audit logging.
//!
//! Hook pipeline order:
//! 1. `RedactionTokenizer` — replaces sensitive data with `{TYPE:LABEL:…SUFFIX}` tokens
//! 2. `DlpHook` — 109 regex patterns for secrets, PII, injection
//! 3. `SecretScannerHook` — Shannon entropy-based detection
//! 4. `SecuritySidecarHook` — external sidecar container (prompt injection)
//! 5. `AuditHook` — persists all findings to Postgres (always runs last)

pub mod agent_guard;
pub mod audit;
pub mod dlp;
pub mod pipeline;
pub mod redaction_vault;
pub mod secret_scanner;
pub mod secret_tokenizer;
pub mod security_event;
pub mod security_sidecar;
pub mod taint_trace;
pub mod user_hook;

use async_trait::async_trait;

// Re-export everything for backward compatibility
pub use agent_guard::AgentGuardHook;
pub use audit::{AuditAction, AuditEvent, AuditHook, AuditSink, FileAuditSink, PgAuditSink, TracingAuditSink};
pub use dlp::{DlpAction, DlpConfig, DlpHook, DlpPattern, DlpPatternInfo};
pub use pipeline::{HookPipeline, PendingFinding, SecurityContext};
pub use redaction_vault::{RedactionEntry, RedactionType, RedactionVault};
pub use secret_scanner::{SecretScannerConfig, SecretScannerHook};
pub use secret_tokenizer::RedactionTokenizer;
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
