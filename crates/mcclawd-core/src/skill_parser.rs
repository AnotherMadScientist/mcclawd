//! Parser for ClawHub SKILL.md format.

use crate::error::McclawdError;
use crate::skills::LoadedSkill;

/// Parse a SKILL.md string into a LoadedSkill.
pub fn parse_skill_md(content: &str) -> crate::Result<LoadedSkill> {
    let lines: Vec<&str> = content.lines().collect();

    // Parse header: "# Skill: <name>"
    let first_line = lines
        .first()
        .ok_or_else(|| McclawdError::Config("SKILL.md is empty".to_string()))?;

    let name = first_line
        .strip_prefix("# Skill: ")
        .ok_or_else(|| {
            McclawdError::Config("SKILL.md must start with '# Skill: <name>'".to_string())
        })?
        .trim()
        .to_string();

    // Parse YAML-like frontmatter (version, author)
    let mut version = String::new();
    let mut author = String::new();

    for line in lines.iter().skip(1) {
        if line.starts_with("version: ") {
            version = line.strip_prefix("version: ").unwrap().trim().to_string();
        } else if line.starts_with("author: ") {
            author = line.strip_prefix("author: ").unwrap().trim().to_string();
        } else if line.starts_with("## ") {
            break;
        }
    }

    // Split content by ## sections
    let sections = split_sections(content);

    let description = sections
        .get("Description")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let mcp_tools = sections
        .get("MCP Tools")
        .map(|s| parse_list_items(s))
        .unwrap_or_default();

    let install_steps = sections
        .get("Install")
        .map(|s| parse_code_block(s))
        .unwrap_or_default();

    let context = sections
        .get("Context")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    Ok(LoadedSkill {
        name,
        version,
        author,
        description,
        mcp_tools,
        install_steps,
        context,
    })
}

/// Split markdown by `## Section` headers. Returns map of section_name -> content.
fn split_sections(content: &str) -> std::collections::HashMap<String, String> {
    let mut sections = std::collections::HashMap::new();
    let mut current_section: Option<String> = None;
    let mut current_content = String::new();

    for line in content.lines() {
        if let Some(header) = line.strip_prefix("## ") {
            if let Some(section) = current_section.take() {
                sections.insert(section, current_content.clone());
            }
            current_section = Some(header.trim().to_string());
            current_content.clear();
        } else if current_section.is_some() {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    if let Some(section) = current_section {
        sections.insert(section, current_content);
    }

    sections
}

/// Parse "- item" list lines.
fn parse_list_items(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- ").map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .collect()
}

/// Extract commands from a fenced code block.
fn parse_code_block(content: &str) -> Vec<String> {
    let mut in_block = false;
    let mut commands = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_block = !in_block;
            continue;
        }
        if in_block && !trimmed.is_empty() {
            commands.push(trimmed.to_string());
        }
    }

    commands
}
