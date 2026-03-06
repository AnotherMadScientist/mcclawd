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

    let dependencies = sections
        .get("Dependencies")
        .map(|s| parse_list_items(s))
        .unwrap_or_default();

    let instructions = sections
        .get("Instructions")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let examples = sections
        .get("Examples")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let config_section = sections
        .get("Config")
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
        dependencies,
        instructions,
        examples,
        config_section,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_skill_md() -> &'static str {
        r#"# Skill: test-skill
version: 1.0.0
author: tester

## Description
A test skill for unit tests.

## Dependencies
- dep-a
- dep-b

## Instructions
Use this skill to do things.
Call the tools in order.

## MCP Tools
- test.tool_a
- test.tool_b

## Examples
Example 1: basic usage
```
test.tool_a input="hello"
```

## Config
KEY=value
TIMEOUT=30

## Install
```
pip install test-skill
```

## Context
Extra context for the agent.
"#
    }

    #[test]
    fn test_parse_dependencies_section() {
        let skill = parse_skill_md(sample_skill_md()).unwrap();
        assert_eq!(skill.dependencies, vec!["dep-a", "dep-b"]);
    }

    #[test]
    fn test_parse_no_dependencies() {
        let content = "# Skill: no-deps\nversion: 1.0.0\nauthor: x\n\n## Description\nA skill with no deps.\n";
        let skill = parse_skill_md(content).unwrap();
        assert!(skill.dependencies.is_empty());
    }

    #[test]
    fn test_parse_instructions_section() {
        let skill = parse_skill_md(sample_skill_md()).unwrap();
        assert!(skill.instructions.contains("Use this skill to do things."));
        assert!(skill.instructions.contains("Call the tools in order."));
    }

    #[test]
    fn test_parse_examples_section() {
        let skill = parse_skill_md(sample_skill_md()).unwrap();
        assert!(skill.examples.contains("Example 1: basic usage"));
    }

    #[test]
    fn test_parse_config_section() {
        let skill = parse_skill_md(sample_skill_md()).unwrap();
        assert!(skill.config_section.contains("KEY=value"));
        assert!(skill.config_section.contains("TIMEOUT=30"));
    }

    #[test]
    fn test_parse_all_sections() {
        let skill = parse_skill_md(sample_skill_md()).unwrap();
        assert_eq!(skill.name, "test-skill");
        assert_eq!(skill.version, "1.0.0");
        assert_eq!(skill.author, "tester");
        assert!(!skill.description.is_empty());
        assert_eq!(skill.dependencies.len(), 2);
        assert!(!skill.instructions.is_empty());
        assert!(!skill.examples.is_empty());
        assert!(!skill.config_section.is_empty());
        assert_eq!(skill.mcp_tools, vec!["test.tool_a", "test.tool_b"]);
        assert!(!skill.context.is_empty());
    }
}
