//! Hook pipeline — chains multiple SecurityHooks into one.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::SecurityHook;

/// A finding collected during a scan pass — written to the DB by AuditHook.
#[derive(Debug, Clone)]
pub struct PendingFinding {
    pub finding_type: String,
    pub tag: String,
    pub pattern_name: String,
    pub confidence: f64,
    pub redacted_preview: Option<String>,
}

/// Shared mutable context threaded through the pipeline for one tool call.
/// Hooks push findings here; AuditHook reads them to persist everything.
#[derive(Debug, Default)]
pub struct SecurityContext {
    pub task_id: Option<String>,
    pub user_id: String,
    /// Accumulated findings from DlpHook, SecretScannerHook, SecuritySidecarHook.
    pub findings: Vec<PendingFinding>,
    /// Highest threat level seen ("safe", "suspicious", "dangerous", "critical").
    pub threat_level: String,
    /// Whether any hook blocked the call.
    pub was_blocked: bool,
}

impl SecurityContext {
    pub fn new() -> Self {
        Self {
            user_id: "admin".to_string(),
            threat_level: "safe".to_string(),
            ..Default::default()
        }
    }

    /// Elevate threat level if the new level is higher.
    pub fn elevate_threat(&mut self, level: &str) {
        let rank = |l: &str| match l {
            "critical" => 3,
            "dangerous" => 2,
            "suspicious" => 1,
            _ => 0,
        };
        if rank(level) > rank(&self.threat_level) {
            self.threat_level = level.to_string();
        }
    }
}

/// Chains multiple security hooks into a single SecurityHook.
///
/// - `before_tool_call`: runs all hooks in order; first error stops the chain.
/// - `after_tool_call`: runs all hooks; collects all results (doesn't stop on error).
pub struct HookPipeline {
    hooks: Vec<Arc<dyn SecurityHook>>,
    /// Shared context — set before each tool call, read by AuditHook.
    pub context: Arc<RwLock<SecurityContext>>,
}

impl HookPipeline {
    pub fn new() -> Self {
        Self {
            hooks: Vec::new(),
            context: Arc::new(RwLock::new(SecurityContext::new())),
        }
    }

    pub fn add(mut self, hook: Arc<dyn SecurityHook>) -> Self {
        self.hooks.push(hook);
        self
    }

    pub fn len(&self) -> usize {
        self.hooks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }

    /// Call before each tool invocation to associate events with the current task.
    pub async fn set_task_context(&self, task_id: &str) {
        let mut ctx = self.context.write().await;
        ctx.task_id = Some(task_id.to_string());
        // Clear findings from the previous call.
        ctx.findings.clear();
        ctx.threat_level = "safe".to_string();
        ctx.was_blocked = false;
    }

    /// Instantiate user-defined hooks from config and append them after existing hooks.
    ///
    /// User hooks always run last — after DLP, secret scanner, and audit hooks.
    pub fn add_user_hooks(
        mut self,
        configs: Vec<super::user_hook::UserHookConfig>,
    ) -> crate::Result<Self> {
        for cfg in configs {
            let hook = super::user_hook::UserHook::new(cfg)?;
            self.hooks.push(Arc::new(hook));
        }
        Ok(self)
    }
}

impl Default for HookPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SecurityHook for HookPipeline {
    async fn before_tool_call(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> crate::Result<()> {
        // Run ALL hooks (so AuditHook always persists findings), return first error.
        // Previous fail-fast behavior skipped AuditHook when DlpHook blocked,
        // causing detected findings to never reach the database.
        let mut first_error = None;
        for hook in &self.hooks {
            if let Err(e) = hook.before_tool_call(tool_name, args).await {
                if first_error.is_none() {
                    self.context.write().await.was_blocked = true;
                    first_error = Some(e);
                }
            }
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    async fn after_tool_call(
        &self,
        tool_name: &str,
        result: &serde_json::Value,
    ) -> crate::Result<()> {
        // Run all hooks, collect errors but don't stop
        let mut first_error = None;
        for hook in &self.hooks {
            if let Err(e) = hook.after_tool_call(tool_name, result).await {
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::McclawdError;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A hook that always succeeds and increments a counter.
    struct CountingHook {
        before_count: AtomicUsize,
        after_count: AtomicUsize,
    }

    impl CountingHook {
        fn new() -> Self {
            Self {
                before_count: AtomicUsize::new(0),
                after_count: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl SecurityHook for CountingHook {
        async fn before_tool_call(
            &self,
            _tool_name: &str,
            _args: &serde_json::Value,
        ) -> crate::Result<()> {
            self.before_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn after_tool_call(
            &self,
            _tool_name: &str,
            _result: &serde_json::Value,
        ) -> crate::Result<()> {
            self.after_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// A hook that always fails.
    struct FailingHook;

    #[async_trait]
    impl SecurityHook for FailingHook {
        async fn before_tool_call(
            &self,
            _tool_name: &str,
            _args: &serde_json::Value,
        ) -> crate::Result<()> {
            Err(McclawdError::Tool("hook failed".into()))
        }
        async fn after_tool_call(
            &self,
            _tool_name: &str,
            _result: &serde_json::Value,
        ) -> crate::Result<()> {
            Err(McclawdError::Tool("hook failed".into()))
        }
    }

    #[tokio::test]
    async fn empty_pipeline_passes() {
        let pipeline = HookPipeline::new();
        assert!(pipeline.is_empty());
        let args = serde_json::json!({});
        assert!(pipeline.before_tool_call("t", &args).await.is_ok());
        assert!(pipeline.after_tool_call("t", &args).await.is_ok());
    }

    #[tokio::test]
    async fn single_hook_works() {
        let counter = Arc::new(CountingHook::new());
        let pipeline = HookPipeline::new().add(counter.clone());
        assert_eq!(pipeline.len(), 1);

        let args = serde_json::json!({});
        pipeline.before_tool_call("t", &args).await.unwrap();
        pipeline.after_tool_call("t", &args).await.unwrap();

        assert_eq!(counter.before_count.load(Ordering::SeqCst), 1);
        assert_eq!(counter.after_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn multiple_hooks_run_in_order() {
        let c1 = Arc::new(CountingHook::new());
        let c2 = Arc::new(CountingHook::new());
        let pipeline = HookPipeline::new().add(c1.clone()).add(c2.clone());
        assert_eq!(pipeline.len(), 2);

        let args = serde_json::json!({});
        pipeline.before_tool_call("t", &args).await.unwrap();

        assert_eq!(c1.before_count.load(Ordering::SeqCst), 1);
        assert_eq!(c2.before_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn before_runs_all_hooks_even_after_error() {
        let counter = Arc::new(CountingHook::new());
        let pipeline = HookPipeline::new()
            .add(Arc::new(FailingHook))
            .add(counter.clone());

        let args = serde_json::json!({});
        let res = pipeline.before_tool_call("t", &args).await;
        assert!(res.is_err());
        // All hooks run (run-all semantics) so AuditHook always persists findings
        assert_eq!(counter.before_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn set_task_context_updates_task_id() {
        let pipeline = HookPipeline::new();
        pipeline.set_task_context("task-abc-123").await;
        let ctx = pipeline.context.read().await;
        assert_eq!(ctx.task_id.as_deref(), Some("task-abc-123"));
    }

    #[tokio::test]
    async fn set_task_context_clears_findings() {
        let pipeline = HookPipeline::new();
        {
            let mut ctx = pipeline.context.write().await;
            ctx.findings.push(PendingFinding {
                finding_type: "dlp_match".to_string(),
                tag: "test".to_string(),
                pattern_name: "Test Pattern".to_string(),
                confidence: 1.0,
                redacted_preview: None,
            });
        }
        pipeline.set_task_context("task-new").await;
        let ctx = pipeline.context.read().await;
        assert!(ctx.findings.is_empty());
        assert_eq!(ctx.threat_level, "safe");
    }

    #[tokio::test]
    async fn add_user_hooks_appends_after_builtin() {
        use crate::hooks::user_hook::{UserHookAction, UserHookConfig, UserHookTrigger, UserHookType};
        use std::collections::HashMap;

        let counter = Arc::new(CountingHook::new());

        let user_cfg = UserHookConfig {
            name: "allow-hook".to_string(),
            trigger: UserHookTrigger::BeforeToolCall,
            hook_type: UserHookType::Shell,
            command: Some("true".to_string()),
            url: None,
            method: "POST".to_string(),
            headers: HashMap::new(),
            pattern: None,
            action: UserHookAction::Allow,
            message: None,
            timeout_ms: 1000,
            enabled: true,
        };

        let pipeline = HookPipeline::new()
            .add(counter.clone())
            .add_user_hooks(vec![user_cfg])
            .unwrap();

        // 1 built-in + 1 user hook
        assert_eq!(pipeline.len(), 2);

        let args = serde_json::json!({});
        pipeline.before_tool_call("t", &args).await.unwrap();
        // Built-in counter hook ran
        assert_eq!(counter.before_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn add_user_hooks_block_stops_before_chain() {
        use crate::hooks::user_hook::{UserHookAction, UserHookConfig, UserHookTrigger, UserHookType};
        use std::collections::HashMap;

        let user_cfg = UserHookConfig {
            name: "block-hook".to_string(),
            trigger: UserHookTrigger::BeforeToolCall,
            hook_type: UserHookType::Shell,
            command: Some("true".to_string()),
            url: None,
            method: "POST".to_string(),
            headers: HashMap::new(),
            pattern: None,
            action: UserHookAction::Block,
            message: Some("nope".to_string()),
            timeout_ms: 1000,
            enabled: true,
        };

        let pipeline = HookPipeline::new()
            .add_user_hooks(vec![user_cfg])
            .unwrap();

        let args = serde_json::json!({});
        let res = pipeline.before_tool_call("t", &args).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("nope"));
    }

    #[tokio::test]
    async fn empty_user_hooks_pass_through() {
        let pipeline = HookPipeline::new().add_user_hooks(vec![]).unwrap();
        assert!(pipeline.is_empty());
        let args = serde_json::json!({});
        assert!(pipeline.before_tool_call("t", &args).await.is_ok());
    }

    #[tokio::test]
    async fn after_call_runs_all_hooks() {
        let counter = Arc::new(CountingHook::new());
        let pipeline = HookPipeline::new()
            .add(Arc::new(FailingHook))
            .add(counter.clone());

        let args = serde_json::json!({});
        let res = pipeline.after_tool_call("t", &args).await;
        // Should return error from failing hook
        assert!(res.is_err());
        // But second hook should still have been called
        assert_eq!(counter.after_count.load(Ordering::SeqCst), 1);
    }
}
