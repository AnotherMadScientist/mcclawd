//! Agent engine — builds a Rig agent from workspace context + tools.
//!
//! Rig handles the multi-turn ReAct loop natively via `.prompt().max_turns(N)`.
//! This module only *configures* the agent; it does not call any LLM APIs.

use crate::context::ContextBuilder;
use crate::mcp_integration::McpBundle;
use crate::workspace::Workspace;
use mcclawd_core::config::McclawdConfig;
use mcclawd_core::hooks::HookPipeline;
use mcclawd_tools::builtin::memory::{MemoryRecall, MemoryStore};
use mcclawd_tools::guarded_tool::GuardedTool;
use mcclawd_tools::system_tools::{CreateTask, NavigateTo};
use rig::agent::Agent;
use rig::client::CompletionClient;
use rig::providers::anthropic;
use std::sync::Arc;

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
        security_pipeline: Option<Arc<HookPipeline>>,
        model: &str,
    ) -> anyhow::Result<(McclawdAgent, MemoryStore, Vec<McpBundle>)> {
        Self::build_with_skill_filter(workspace, api_key, max_turns, config, security_pipeline, None, model).await
    }

    /// Build a task agent with an optional skill filter.
    /// When `skill_filter` is Some, only the named skills are loaded from disk.
    /// Some(empty vec) = no skills. None = all skills (legacy).
    pub async fn build_with_skill_filter(
        workspace: Workspace,
        api_key: &str,
        max_turns: usize,
        config: &McclawdConfig,
        security_pipeline: Option<Arc<HookPipeline>>,
        skill_filter: Option<Vec<String>>,
        model: &str,
    ) -> anyhow::Result<(McclawdAgent, MemoryStore, Vec<McpBundle>)> {
        let mut context = ContextBuilder::new(workspace)
            .with_skills_dir(config.skills.managed_dir.clone());
        if let Some(filter) = skill_filter {
            context = context.with_skill_filter(filter);
        }
        let system_prompt = context.build_system_prompt();

        let client = anthropic::Client::new(api_key)?;
        let memory_store = MemoryStore::new_shared();
        let memory_recall = MemoryRecall::from_shared(&memory_store);

        // Always wrap tools with GuardedTool — empty pipeline has zero overhead
        let pipeline =
            security_pipeline.unwrap_or_else(|| Arc::new(HookPipeline::new()));

        let mut builder = client
            .agent(model)
            .preamble(&system_prompt)
            .max_tokens(8192)
            .default_max_turns(max_turns)
            .tool(GuardedTool::new(memory_store.clone(), pipeline.clone()))
            .tool(GuardedTool::new(memory_recall, pipeline.clone()));

        // Wire in MCP tools: try env-var path first (inside container),
        // fall back to config-based connection (host/dev mode)
        let bundles = match crate::mcp_integration::connect_from_env().await? {
            Some(b) => b,
            None => crate::mcp_integration::connect_mcp_tools(config).await?,
        };
        for bundle in &bundles {
            builder = builder.rmcp_tools(bundle.tools.clone(), bundle.peer.clone());
        }

        let agent = builder.build();
        if !pipeline.is_empty() {
            tracing::info!(hooks = pipeline.len(), "Agent built with security pipeline");
        }
        Ok((agent, memory_store, bundles))
    }

    /// Build a system agent with minimal UI control tools only.
    ///
    /// The system agent is a restricted UI controller — it can only navigate
    /// pages and create tasks. No skill management, no secrets, no workspace
    /// editing (those are done through the UI directly). This minimizes the
    /// attack surface for prompt injection.
    pub async fn build_system_agent(
        api_key: &str,
        system_prompt: &str,
        model: &str,
    ) -> anyhow::Result<McclawdAgent> {
        let client = anthropic::Client::new(api_key)?;

        let agent = client
            .agent(model)
            .preamble(system_prompt)
            .max_tokens(4096)
            .default_max_turns(3)
            .tool(NavigateTo)
            .tool(CreateTask)
            .build();

        Ok(agent)
    }
}
