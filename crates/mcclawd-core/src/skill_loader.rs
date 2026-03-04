//! Skill loader — discovers and resolves skills from `.mcclawd/skills/` directory.

use std::path::PathBuf;

use crate::skill_parser::parse_skill_md;
use crate::skills::LoadedSkill;

/// Discovers skills from disk and resolves per-agent skill sets.
pub struct SkillLoader {
    root: PathBuf,
}

impl SkillLoader {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn skills_dir(&self) -> PathBuf {
        self.root.join(".mcclawd").join("skills")
    }

    /// Discover all installed skills by reading SKILL.md files.
    pub fn discover_all(&self) -> crate::Result<Vec<LoadedSkill>> {
        let skills_dir = self.skills_dir();
        if !skills_dir.exists() {
            return Ok(Vec::new());
        }

        let mut skills = Vec::new();
        let entries = std::fs::read_dir(&skills_dir).map_err(|e| {
            crate::error::McclawdError::Config(format!("failed to read skills directory: {e}"))
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                crate::error::McclawdError::Config(format!("failed to read entry: {e}"))
            })?;
            let skill_md = entry.path().join("SKILL.md");
            if skill_md.exists() {
                let content = std::fs::read_to_string(&skill_md).map_err(|e| {
                    crate::error::McclawdError::Config(format!(
                        "failed to read {}: {e}",
                        skill_md.display()
                    ))
                })?;
                let skill = parse_skill_md(&content)?;
                skills.push(skill);
            }
        }

        Ok(skills)
    }

    /// Resolve skills assigned to a specific agent by reading AGENTS.md.
    pub fn resolve_for_agent(&self, agent_id: &str) -> crate::Result<Vec<LoadedSkill>> {
        let agents_md = self.root.join(".mcclawd").join("AGENTS.md");
        if !agents_md.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(&agents_md).map_err(|e| {
            crate::error::McclawdError::Config(format!("failed to read AGENTS.md: {e}"))
        })?;

        let skill_names = parse_agent_skills(&content, agent_id);
        let all_skills = self.discover_all()?;

        Ok(all_skills
            .into_iter()
            .filter(|s| skill_names.contains(&s.name))
            .collect())
    }
}

/// Parse AGENTS.md to extract skill names for a given agent.
fn parse_agent_skills(content: &str, agent_id: &str) -> Vec<String> {
    let agent_header = format!("# Agent: {agent_id}");
    let mut in_agent = false;
    let mut in_skills = false;
    let mut skills = Vec::new();

    for line in content.lines() {
        if line.starts_with("# Agent: ") {
            in_agent = line.trim() == agent_header;
            in_skills = false;
            continue;
        }
        if !in_agent {
            continue;
        }
        if line.trim() == "## Skills" {
            in_skills = true;
            continue;
        }
        if line.starts_with("## ") {
            in_skills = false;
            continue;
        }
        if in_skills {
            if let Some(name) = line.trim().strip_prefix("- ") {
                let name = name.trim();
                if !name.is_empty() {
                    skills.push(name.to_string());
                }
            }
        }
    }

    skills
}
