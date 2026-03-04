//! Context assembly — builds the system prompt from workspace files.
//!
//! Priority order: SOUL.md → USER.md → AGENTS.md.
//! Each section is separated by a horizontal rule to give the LLM clear boundaries.

use crate::workspace::Workspace;

/// Assembles the system prompt from workspace markdown files.
pub struct ContextBuilder {
    workspace: Workspace,
}

impl ContextBuilder {
    pub fn new(workspace: Workspace) -> Self {
        Self { workspace }
    }

    /// Build the system prompt from workspace files.
    /// Priority order: SOUL.md → USER.md → AGENTS.md → capabilities.
    pub fn build_system_prompt(&self) -> String {
        let mut sections = vec![];

        // 1. SOUL.md (always first — defines identity)
        if let Some(soul) = &self.workspace.soul {
            sections.push(soul.clone());
        }

        // 2. USER.md (user preferences + context)
        if let Some(user) = &self.workspace.user {
            sections.push(format!("\n---\n\n{}", user));
        }

        // 3. AGENTS.md (informational in Phase 0 — skill assignments + swarm config)
        if let Some(agents) = &self.workspace.agents {
            sections.push(format!("\n---\n\n{}", agents));
        }

        sections.join("\n")
    }
}
