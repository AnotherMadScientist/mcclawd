//! GuardedTool — wraps a Rig Tool with HookPipeline security checks.
//!
//! Every tool registered on an agent should be wrapped with `GuardedTool` when
//! a security pipeline is active. This runs `before_tool_call` / `after_tool_call`
//! hooks (DLP, secret scanning, audit) around every tool invocation.
//!
//! # Behavior
//! - `before_tool_call` error → tool call is **blocked** (not executed).
//! - `after_tool_call` error → logged as warning, result still returned (**fail-open**).

use mcclawd_core::hooks::{HookPipeline, SecurityHook};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Serialize;
use std::sync::Arc;

// ----------------------------------------------------------------
// Error
// ----------------------------------------------------------------

/// Error type for guarded tool calls — either the security pipeline blocked
/// the call, or the inner tool itself returned an error.
#[derive(Debug, thiserror::Error)]
pub enum GuardedToolError<E: std::error::Error> {
    /// The security pipeline blocked the tool call.
    #[error("Security pipeline blocked tool call: {0}")]
    Blocked(String),
    /// The inner tool returned an error.
    #[error(transparent)]
    Inner(E),
}

// ----------------------------------------------------------------
// GuardedTool
// ----------------------------------------------------------------

/// Wraps an inner tool `T`, running `HookPipeline` before/after execution.
///
/// This ensures ALL tool calls go through security scanning regardless
/// of whether they are MCP tools or builtin tools.
pub struct GuardedTool<T> {
    inner: T,
    pipeline: Arc<HookPipeline>,
}

impl<T> GuardedTool<T> {
    /// Create a new guarded wrapper around `inner` with the given security pipeline.
    pub fn new(inner: T, pipeline: Arc<HookPipeline>) -> Self {
        Self { inner, pipeline }
    }
}

impl<T: Tool> Tool for GuardedTool<T>
where
    T::Args: Serialize,
    T::Output: Send,
{
    const NAME: &'static str = T::NAME;
    type Error = GuardedToolError<T::Error>;
    type Args = T::Args;
    type Output = T::Output;

    async fn definition(&self, prompt: String) -> ToolDefinition {
        self.inner.definition(prompt).await
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let tool_name = T::NAME;

        // Serialize args to JSON for the security pipeline
        let args_json = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);

        // Run before_tool_call — if it errors, block the tool
        if let Err(e) = self.pipeline.before_tool_call(tool_name, &args_json).await {
            return Err(GuardedToolError::Blocked(e.to_string()));
        }

        // Execute the inner tool
        let result = self.inner.call(args).await.map_err(GuardedToolError::Inner)?;

        // Run after_tool_call — fail-open (log warning, still return result)
        let result_json = serde_json::to_value(&result).unwrap_or(serde_json::Value::Null);
        if let Err(e) = self
            .pipeline
            .after_tool_call(tool_name, &result_json)
            .await
        {
            tracing::warn!(
                tool = tool_name,
                error = %e,
                "after_tool_call hook failed (fail-open)"
            );
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcclawd_core::hooks::HookPipeline;
    use mcclawd_core::McclawdError;
    use rig::completion::ToolDefinition;
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    // -- Test tool ----------------------------------------------------------

    #[derive(Deserialize, Serialize)]
    struct EchoArgs {
        text: String,
    }

    #[derive(Debug, thiserror::Error)]
    #[error("echo error")]
    struct EchoError;

    #[derive(Serialize, Deserialize)]
    struct EchoTool;

    impl Tool for EchoTool {
        const NAME: &'static str = "echo";
        type Error = EchoError;
        type Args = EchoArgs;
        type Output = String;

        async fn definition(&self, _prompt: String) -> ToolDefinition {
            ToolDefinition {
                name: "echo".to_string(),
                description: "Echo the input text".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" }
                    },
                    "required": ["text"]
                }),
            }
        }

        async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
            Ok(args.text)
        }
    }

    // -- Blocking hook ------------------------------------------------------

    struct BlockingHook;

    #[async_trait::async_trait]
    impl SecurityHook for BlockingHook {
        async fn before_tool_call(
            &self,
            _tool_name: &str,
            _args: &serde_json::Value,
        ) -> mcclawd_core::Result<()> {
            Err(McclawdError::Tool("DLP blocked sensitive data".into()))
        }
        async fn after_tool_call(
            &self,
            _tool_name: &str,
            _result: &serde_json::Value,
        ) -> mcclawd_core::Result<()> {
            Ok(())
        }
    }

    // -- Tracking hook (records calls) --------------------------------------

    struct TrackingHook {
        before_called: AtomicBool,
        after_called: AtomicBool,
    }

    impl TrackingHook {
        fn new() -> Self {
            Self {
                before_called: AtomicBool::new(false),
                after_called: AtomicBool::new(false),
            }
        }
    }

    #[async_trait::async_trait]
    impl SecurityHook for TrackingHook {
        async fn before_tool_call(
            &self,
            _tool_name: &str,
            _args: &serde_json::Value,
        ) -> mcclawd_core::Result<()> {
            self.before_called.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn after_tool_call(
            &self,
            _tool_name: &str,
            _result: &serde_json::Value,
        ) -> mcclawd_core::Result<()> {
            self.after_called.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    // -- Tests --------------------------------------------------------------

    #[tokio::test]
    async fn guarded_tool_passes_through_on_empty_pipeline() {
        let pipeline = Arc::new(HookPipeline::new());
        let guarded = GuardedTool::new(EchoTool, pipeline);

        let result = guarded
            .call(EchoArgs {
                text: "hello".into(),
            })
            .await
            .unwrap();
        assert_eq!(result, "hello");
    }

    #[tokio::test]
    async fn guarded_tool_blocked_by_before_hook() {
        let pipeline = Arc::new(HookPipeline::new().add(Arc::new(BlockingHook)));
        let guarded = GuardedTool::new(EchoTool, pipeline);

        let result = guarded
            .call(EchoArgs {
                text: "secret".into(),
            })
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("blocked"));
    }

    #[tokio::test]
    async fn guarded_tool_runs_both_hooks() {
        let tracker = Arc::new(TrackingHook::new());
        let pipeline = Arc::new(HookPipeline::new().add(tracker.clone()));
        let guarded = GuardedTool::new(EchoTool, pipeline);

        let result = guarded
            .call(EchoArgs {
                text: "test".into(),
            })
            .await
            .unwrap();
        assert_eq!(result, "test");
        assert!(tracker.before_called.load(Ordering::SeqCst));
        assert!(tracker.after_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn guarded_tool_delegates_name_and_definition() {
        let pipeline = Arc::new(HookPipeline::new());
        let guarded = GuardedTool::new(EchoTool, pipeline);

        assert_eq!(GuardedTool::<EchoTool>::NAME, "echo");
        let def = guarded.definition("test".into()).await;
        assert_eq!(def.name, "echo");
    }
}
