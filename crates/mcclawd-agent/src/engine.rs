//! Agent engine — builds a Rig agent from workspace context + tools.
//!
//! Rig handles the multi-turn ReAct loop natively via `.prompt().max_turns(N)`.
//! This module only *configures* the agent; it does not call any LLM APIs.

use crate::context::ContextBuilder;
use crate::mcp_integration::McpBundle;
use crate::workspace::Workspace;
use mcclawd_core::config::McclawdConfig;
use mcclawd_tools::builtin::memory::{MemoryRecall, MemoryStore};
use mcclawd_tools::system_tools::{
    CreateTask, InstallSkill, ListSkills, ManageSecret, NavigateTo, ReadWorkspace, UninstallSkill,
    UpdateWorkspace,
};
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
    /// and MCP tools from directly-connected MCP servers.
    ///
    /// The returned `MemoryStore` shares its backing `DashMap` with the
    /// `MemoryRecall` tool already registered on the agent, so callers can
    /// inspect session memory after the run completes.
    ///
    /// The returned `Vec<McpBundle>` must be kept alive for the agent's
    /// lifetime — dropping them closes the underlying MCP connections.
    ///
    /// # Errors
    /// Returns an error if the Anthropic client cannot be constructed
    /// (e.g. the API key contains invalid header characters).
    pub async fn build(
        workspace: Workspace,
        api_key: &str,
        max_turns: usize,
        config: &McclawdConfig,
    ) -> anyhow::Result<(McclawdAgent, MemoryStore, Vec<McpBundle>)> {
        let context = ContextBuilder::new(workspace)
            .with_skills_dir(config.skills.managed_dir.clone());
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

        // Wire in MCP tools from each directly-connected server
        let bundles = crate::mcp_integration::connect_mcp_tools(config).await?;
        for bundle in &bundles {
            builder = builder.rmcp_tools(bundle.tools.clone(), bundle.peer.clone());
        }

        let agent = builder.build();
        Ok((agent, memory_store, bundles))
    }

    /// Build a system agent with UI control tools (no MCP, no memory tools).
    ///
    /// Used by the always-on system agent that handles voice/text commands
    /// for navigation, task creation, skill management, etc.
    pub async fn build_system_agent(
        api_key: &str,
        system_prompt: &str,
    ) -> anyhow::Result<McclawdAgent> {
        let client = anthropic::Client::new(api_key)?;

        let agent = client
            .agent(CLAUDE_4_SONNET)
            .preamble(system_prompt)
            .max_tokens(4096)
            .default_max_turns(5)
            .tool(NavigateTo)
            .tool(CreateTask)
            .tool(InstallSkill)
            .tool(UninstallSkill)
            .tool(ListSkills)
            .tool(ManageSecret)
            .tool(ReadWorkspace)
            .tool(UpdateWorkspace)
            .build();

        Ok(agent)
    }
}
