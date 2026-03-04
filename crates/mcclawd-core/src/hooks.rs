use async_trait::async_trait;

/// Hook called before/after tool dispatch. Phase 0: audit logging via tracing.
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

/// Phase 0 implementation: logs tool calls via tracing.
pub struct AuditHook;

#[async_trait]
impl SecurityHook for AuditHook {
    async fn before_tool_call(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> crate::Result<()> {
        tracing::info!(tool = %tool_name, args = %args, "tool_call_start");
        Ok(())
    }

    async fn after_tool_call(
        &self,
        tool_name: &str,
        result: &serde_json::Value,
    ) -> crate::Result<()> {
        tracing::info!(tool = %tool_name, result_size = result.to_string().len(), "tool_call_end");
        Ok(())
    }
}
