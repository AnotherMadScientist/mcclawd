//! Agent engine — builds a Rig agent from workspace context + tools.
//!
//! Rig handles the multi-turn ReAct loop natively via `.prompt().max_turns(N)`.
//! This module only *configures* the agent; it does not call any LLM APIs.

use crate::context::ContextBuilder;
use crate::workspace::Workspace;
use mcclawd_core::config::McclawdConfig;
use mcclawd_tools::builtin::memory::{MemoryRecall, MemoryStore};
use rig::agent::Agent;
use rig::client::CompletionClient;
use rig::providers::anthropic::{self, completion::CLAUDE_4_SONNET};

/// The concrete Anthropic completion-model type used throughout McClawd.
pub type AnthropicModel = anthropic::completion::CompletionModel;

/// The concrete Agent type returned by [`AgentEngine::build`].
/// `()` is the default (no PromptHook).
pub type McclawdAgent = Agent<AnthropicModel>;

/// Builds a configured Rig agent from a workspace and API key.
pub struct AgentEngine;

impl AgentEngine {
    /// Create a Rig agent configured with workspace context, memory tools,
    /// and optionally MCP tools from AgentGateway.
    ///
    /// The returned `MemoryStore` shares its backing `DashMap` with the
    /// `MemoryRecall` tool already registered on the agent, so callers can
    /// inspect session memory after the run completes.
    ///
    /// # Errors
    /// Returns an error if the Anthropic client cannot be constructed
    /// (e.g. the API key contains invalid header characters).
    pub async fn build(
        workspace: Workspace,
        api_key: &str,
        max_turns: usize,
        config: &McclawdConfig,
    ) -> anyhow::Result<(McclawdAgent, MemoryStore)> {
        let context = ContextBuilder::new(workspace);
        let system_prompt = context.build_system_prompt();

        let client = anthropic::Client::new(api_key)?;
        let memory_store = MemoryStore::new_shared();
        let memory_recall = MemoryRecall::from_shared(&memory_store);

        let mut builder = client
            .agent(CLAUDE_4_SONNET)
            .preamble(&system_prompt)
            .max_tokens(8192)
            .default_max_turns(max_turns)
            .tool(memory_store.clone())
            .tool(memory_recall);

        // Wire in MCP tools from AgentGateway if available
        if let Some((tools, peer)) =
            crate::mcp_integration::connect_mcp_tools(config).await?
        {
            builder = builder.rmcp_tools(tools, peer);
        }

        let agent = builder.build();
        Ok((agent, memory_store))
    }
}
