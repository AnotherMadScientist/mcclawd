//! Context assembly — builds the system prompt from workspace files + installed skills.
//!
//! Priority order: SOUL.md → IDENTITY.md → USER.md → AGENTS.md → TOOLS.md → HEARTBEAT.md → installed skills.
//! Each section is separated by a horizontal rule to give the LLM clear boundaries.

use crate::workspace::Workspace;
use std::path::{Path, PathBuf};

/// Assembles the system prompt from workspace markdown files.
pub struct ContextBuilder {
    workspace: Workspace,
    skills_dir: Option<PathBuf>,
    /// Max chars of skill content injected into the prompt (Gap 6 token budget).
    max_skill_chars: usize,
    /// Optional filter: only load these skill names from disk.
    /// None = load all skills (legacy behavior for system agent).
    /// Some(empty vec) = load NO skills.
    /// Some(vec!["a", "b"]) = load only those named skills.
    skill_filter: Option<Vec<String>>,
    /// Pre-built skill context string (from MCCLAWD_SKILL_CONTEXT env var in containers).
    /// When set, this overrides disk-based skill loading entirely.
    skill_context_override: Option<String>,
}

impl ContextBuilder {
    pub fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            skills_dir: None,
            max_skill_chars: 50_000,
            skill_filter: None,
            skill_context_override: None,
        }
    }

    /// Set the skills directory to load installed skills into the system prompt.
    pub fn with_skills_dir(mut self, dir: PathBuf) -> Self {
        self.skills_dir = Some(dir);
        self
    }

    /// Override the 50_000-char skill context budget (Gap 6).
    pub fn with_max_skill_chars(mut self, limit: usize) -> Self {
        self.max_skill_chars = limit;
        self
    }

    /// Filter which skills are loaded from disk.
    /// Empty slice = no skills loaded. Only matching skill names are included.
    pub fn with_skill_filter(mut self, filter: Vec<String>) -> Self {
        self.skill_filter = Some(filter);
        self
    }

    /// Override disk-based skill loading with a pre-built skill context string.
    /// Used by the runner binary when MCCLAWD_SKILL_CONTEXT env var is set.
    pub fn with_skill_context_override(mut self, context: String) -> Self {
        self.skill_context_override = Some(context);
        self
    }

    /// Build the system prompt from workspace files.
    /// Priority order: SOUL.md → IDENTITY.md → USER.md → AGENTS.md → TOOLS.md → HEARTBEAT.md → installed skills → capabilities.
    pub fn build_system_prompt(&self) -> String {
        let mut sections = vec![];

        // 1. SOUL.md (always first — defines identity)
        if let Some(soul) = &self.workspace.soul {
            sections.push(soul.clone());
        }

        // 2. IDENTITY.md (agent persona + capability summary)
        if let Some(identity) = &self.workspace.identity {
            sections.push(format!("\n---\n\n{}", identity));
        }

        // 3. USER.md (user preferences + context)
        if let Some(user) = &self.workspace.user {
            sections.push(format!("\n---\n\n{}", user));
        }

        // 4. AGENTS.md (informational in Phase 0 — skill assignments + swarm config)
        if let Some(agents) = &self.workspace.agents {
            sections.push(format!("\n---\n\n{}", agents));
        }

        // 5. TOOLS.md (tool usage guidelines)
        if let Some(tools) = &self.workspace.tools {
            sections.push(format!("\n---\n\n{}", tools));
        }

        // 6. HEARTBEAT.md (scheduled task context, if present)
        if let Some(heartbeat) = &self.workspace.heartbeat {
            sections.push(format!("\n---\n\n{}", heartbeat));
        }

        // 7. Installed skills — with relevance filter + char budget (Gap 6)
        // Priority: skill_context_override > skill_filter + disk loading
        if let Some(ref override_ctx) = self.skill_context_override {
            if !override_ctx.is_empty() {
                sections.push(format!(
                    "\n---\n\n## Installed Skills\n\nYou have the following skills available. Follow the instructions in each skill when relevant to the user's request.\n\n{}",
                    override_ctx
                ));
            }
        } else if let Some(skills_dir) = &self.skills_dir {
            // Check skill_filter: Some(empty) = no skills, None = all skills
            let should_load = match &self.skill_filter {
                Some(filter) => !filter.is_empty(),
                None => true, // No filter = load all (system agent behavior)
            };
            if should_load {
                let assigned = extract_assigned_skills(self.workspace.agents.as_deref());
                let skills_content = load_installed_skills(skills_dir, &assigned, self.max_skill_chars, self.skill_filter.as_deref());
                if !skills_content.is_empty() {
                    sections.push(format!(
                        "\n---\n\n## Installed Skills\n\nYou have the following skills available. Follow the instructions in each skill when relevant to the user's request.\n\n{}",
                        skills_content
                    ));
                }
            }
        }

        // 5. Response formatting instructions (only if we have content)
        if !sections.is_empty() {
            sections.push("\n---\n\n## Response Guidelines\n\nAlways include source links in your responses when referencing external information. Format sources as a \"Sources\" section at the end of your response with clickable markdown links:\n\n**Sources:**\n- [Title](URL)\n\nIf you used tools to retrieve information, cite the source URL. If no external sources were used, omit the Sources section.".to_string());
        }

        sections.join("\n")
    }
}

/// Extract skill names assigned in AGENTS.md `## Skill Assignments` section.
/// Returns empty vec if none found — caller injects all skills in that case.
fn extract_assigned_skills(agents_md: Option<&str>) -> Vec<String> {
    let content = match agents_md {
        Some(c) => c,
        None => return vec![],
    };
    let mut in_section = false;
    let mut assigned = Vec::new();
    for line in content.lines() {
        if line.trim_start().starts_with("## Skill Assignments") {
            in_section = true;
            continue;
        }
        if in_section {
            if line.starts_with("## ") { break; }
            if let Some(name) = line.trim().strip_prefix("- ") {
                let name = name.trim().to_string();
                if !name.is_empty() { assigned.push(name); }
            }
        }
    }
    assigned
}

/// Load installed skills with relevance filter and character budget (Gap 6).
/// Assigned skills appear first; total output capped at `max_chars`.
/// When `skill_filter` is Some, only skills matching the filter names are loaded.
fn load_installed_skills(skills_dir: &Path, assigned: &[String], max_chars: usize, skill_filter: Option<&[String]>) -> String {
    let entries = match std::fs::read_dir(skills_dir) {
        Ok(e) => e,
        Err(_) => return String::new(),
    };

    let mut skills: Vec<(String, String)> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            // Apply skill filter: skip skills not in the filter list
            if let Some(filter) = skill_filter {
                if !filter.iter().any(|f| f == &name) {
                    return None;
                }
            }
            let content = std::fs::read_to_string(entry.path().join("SKILL.md")).ok()?;
            Some((name, content))
        })
        .collect();

    skills.sort_by(|a, b| a.0.cmp(&b.0));

    // Priority: assigned first, then rest
    let (mut priority, rest): (Vec<_>, Vec<_>) = if assigned.is_empty() {
        (skills, vec![])
    } else {
        skills.into_iter().partition(|(n, _)| assigned.contains(n))
    };
    priority.extend(rest);

    let mut output = String::new();
    let mut budget = max_chars;
    let mut truncated = 0usize;

    for (name, content) in &priority {
        let block = format!("### Skill: {}\n\n{}", name, content.trim());
        let sep = if output.is_empty() { "" } else { "\n\n---\n\n" };
        let addition = format!("{}{}", sep, block);
        if addition.len() <= budget {
            output.push_str(&addition);
            budget = budget.saturating_sub(addition.len());
        } else {
            // Inject summary only
            let desc = extract_skill_summary(content.trim());
            let summary = format!("{}\n\n*(instructions omitted — context budget)*", format!("### Skill: {}\n\n{}", name, desc));
            let sum_add = format!("{}{}", sep, summary);
            if sum_add.len() <= budget {
                output.push_str(&sum_add);
                budget = budget.saturating_sub(sum_add.len());
            }
            truncated += 1;
        }
    }

    if truncated > 0 {
        output.push_str(&format!("\n\n*{} skill(s) truncated due to context budget.*", truncated));
    }
    output
}

fn extract_skill_summary(content: &str) -> String {
    let mut in_desc = false;
    for line in content.lines() {
        if line.starts_with("## Description") { in_desc = true; continue; }
        if in_desc {
            if line.starts_with("## ") { break; }
            let t = line.trim();
            if !t.is_empty() { return t.to_string(); }
        }
    }
    content.lines().find(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .unwrap_or("(no description)").trim().to_string()
}
