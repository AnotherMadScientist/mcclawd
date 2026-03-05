//! Context assembly — builds the system prompt from workspace files + installed skills.
//!
//! Priority order: SOUL.md → USER.md → AGENTS.md → installed skills.
//! Each section is separated by a horizontal rule to give the LLM clear boundaries.

use crate::workspace::Workspace;
use std::path::{Path, PathBuf};

/// Assembles the system prompt from workspace markdown files.
pub struct ContextBuilder {
    workspace: Workspace,
    skills_dir: Option<PathBuf>,
}

impl ContextBuilder {
    pub fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            skills_dir: None,
        }
    }

    /// Set the skills directory to load installed skills into the system prompt.
    pub fn with_skills_dir(mut self, dir: PathBuf) -> Self {
        self.skills_dir = Some(dir);
        self
    }

    /// Build the system prompt from workspace files.
    /// Priority order: SOUL.md → USER.md → AGENTS.md → installed skills → capabilities.
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

        // 4. Installed skills (inject SKILL.md content)
        if let Some(skills_dir) = &self.skills_dir {
            let skills_content = load_installed_skills(skills_dir);
            if !skills_content.is_empty() {
                sections.push(format!(
                    "\n---\n\n## Installed Skills\n\nYou have the following skills available. Follow the instructions in each skill when relevant to the user's request.\n\n{}",
                    skills_content
                ));
            }
        }

        // 5. Response formatting instructions
        sections.push("\n---\n\n## Response Guidelines\n\nAlways include source links in your responses when referencing external information. Format sources as a \"Sources\" section at the end of your response with clickable markdown links:\n\n**Sources:**\n- [Title](URL)\n\nIf you used tools to retrieve information, cite the source URL. If no external sources were used, omit the Sources section.".to_string());

        sections.join("\n")
    }
}

/// Load all installed skills' SKILL.md content from the skills directory.
/// Returns a combined string with each skill separated by a horizontal rule.
fn load_installed_skills(skills_dir: &Path) -> String {
    let mut skill_sections = vec![];

    let entries = match std::fs::read_dir(skills_dir) {
        Ok(entries) => entries,
        Err(_) => return String::new(),
    };

    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let skill_md = entry.path().join("SKILL.md");
        if let Ok(content) = std::fs::read_to_string(&skill_md) {
            let name = entry.file_name().to_string_lossy().to_string();
            skill_sections.push(format!("### Skill: {}\n\n{}", name, content.trim()));
        }
    }

    skill_sections.join("\n\n---\n\n")
}
