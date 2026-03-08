//! GuardedTool -- wraps a Rig Tool with HookPipeline security checks.

use mcclawd_core::hooks::HookPipeline;
use std::sync::Arc;

/// Wraps an inner tool, running HookPipeline before/after execution.
/// This ensures ALL tool calls go through security scanning regardless
/// of whether they're MCP tools or builtin tools.
pub struct GuardedTool<T> {
    pub inner: T,
    pub pipeline: Arc<HookPipeline>,
}

impl<T> GuardedTool<T> {
    pub fn new(inner: T, pipeline: Arc<HookPipeline>) -> Self {
        Self { inner, pipeline }
    }
}

// Note: We cannot implement rig::tool::Tool generically due to Rig's trait design
// (associated types, concrete error types). Instead, each tool type that needs
// guarding should be wrapped at registration time. The pipeline is called
// from tasks.rs streaming loop where we intercept ToolCall events.
//
// This struct serves as documentation and a holder for the pattern.
// The actual interception happens in tasks.rs where we have access to
// the streaming events before/after tool execution.
