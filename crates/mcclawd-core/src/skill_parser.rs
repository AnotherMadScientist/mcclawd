//! Parser for ClawHub SKILL.md format.
//!
//! Supports two header formats:
//! 1. Old format: `# Skill: <name>` first line, then `version:`, `author:` lines
//! 2. Frontmatter format: `---` delimited YAML block with name/version/author/description/tags

use crate::error::McclawdError;
use crate::skills::LoadedSkill;

/// Parse a SKILL.md string into a LoadedSkill.
///
/// Detects whether the content uses YAML frontmatter (`---` delimited block)
/// or the legacy `# Skill: <name>` header format and parses accordingly.
pub fn parse_skill_md(content: &str) -> crate::Result<LoadedSkill> {
    let lines: Vec<&str> = content.lines().collect();

    if lines.is_empty() {
        return Err(McclawdError::Config("SKILL.md is empty".to_string()));
    }

    let (name, version, author, fm_description, fm_tags) =
        if lines[0].trim() == "---" {
            parse_frontmatter(&lines)?
        } else if let Some(n) = lines[0].strip_prefix("# Skill: ") {
            parse_legacy_header(n.trim(), &lines)?
        } else {
            return Err(McclawdError::Config(
                "SKILL.md must start with '---' (frontmatter) or '# Skill: <name>'".to_string(),
            ));
        };

    // Split content by ## sections
    let sections = split_sections(content);

    // Frontmatter description takes precedence if no ## Description section exists
    let description = sections
        .get("Description")
        .or(sections.get("Purpose"))
        .map(|s| s.trim().to_string())
        .unwrap_or(fm_description);

    let mcp_tools = sections
        .get("MCP Tools")
        .or(sections.get("Tools"))
        .map(|s| parse_list_items(s))
        .unwrap_or_default();

    let install_steps = sections
        .get("Install")
        .or(sections.get("Installation"))
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
        .or(sections.get("Configuration"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // If we got tags from frontmatter but no ## Dependencies section,
    // don't overwrite dependencies (tags != deps). Tags are informational only.
    let _ = fm_tags; // Currently unused beyond parsing validation

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

/// Parse YAML frontmatter between `---` delimiters.
/// Returns (name, version, author, description, tags).
/// Uses simple line-by-line parsing — no YAML crate needed.
fn parse_frontmatter(
    lines: &[&str],
) -> crate::Result<(String, String, String, String, Vec<String>)> {
    // Find the closing ---
    let end = lines
        .iter()
        .skip(1)
        .position(|l| l.trim() == "---")
        .map(|i| i + 1) // offset for the skip(1)
        .ok_or_else(|| {
            McclawdError::Config("SKILL.md frontmatter: missing closing '---'".to_string())
        })?;

    let mut name = String::new();
    let mut version = String::new();
    let mut author = String::new();
    let mut description = String::new();
    let mut tags = Vec::new();
    let mut in_tags = false;

    for &line in &lines[1..end] {
        let trimmed = line.trim();

        // Handle multi-line tags list
        if in_tags {
            if let Some(tag) = trimmed.strip_prefix("- ") {
                tags.push(tag.trim().to_string());
                continue;
            } else {
                in_tags = false;
                // Fall through to parse this line as a key-value
            }
        }

        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "name" => name = value.to_string(),
                "version" => version = value.to_string(),
                "author" => author = value.to_string(),
                "description" => description = value.to_string(),
                "tags" => {
                    if value.is_empty() {
                        // Multi-line tags (next lines are `- tag`)
                        in_tags = true;
                    } else {
                        // Inline tags: `tags: [web, ai]` or `tags: web, ai`
                        let cleaned = value.trim_start_matches('[').trim_end_matches(']');
                        tags = cleaned
                            .split(',')
                            .map(|t| t.trim().to_string())
                            .filter(|t| !t.is_empty())
                            .collect();
                    }
                }
                _ => {} // Ignore unknown frontmatter keys
            }
        }
    }

    if name.is_empty() {
        // Try to find name from the first `# Title` line after frontmatter
        for &line in &lines[end + 1..] {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(title) = trimmed.strip_prefix("# ") {
                // Convert title to slug-like name
                name = title
                    .trim()
                    .to_lowercase()
                    .replace(' ', "-")
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '-')
                    .collect();
                break;
            }
            break; // Stop at first non-empty, non-heading line
        }
    }

    if name.is_empty() {
        return Err(McclawdError::Config(
            "SKILL.md frontmatter: 'name' field is required (or a # Title after frontmatter)"
                .to_string(),
        ));
    }

    Ok((name, version, author, description, tags))
}

/// Parse the legacy `# Skill: <name>` header format.
/// Returns (name, version, author, description="", tags=[]).
fn parse_legacy_header(
    name: &str,
    lines: &[&str],
) -> crate::Result<(String, String, String, String, Vec<String>)> {
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

    Ok((name.to_string(), version, author, String::new(), Vec::new()))
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

    fn sample_frontmatter_md() -> &'static str {
        r#"---
name: my-cool-skill
version: 2.1.0
author: janedoe
description: A skill that does cool things
tags:
  - web
  - automation
---
# My Cool Skill

## Purpose
This skill automates web tasks.

## Instructions
Follow the steps carefully.

## Tools
- web.fetch
- web.parse

## Examples
Example: fetch a page
```
web.fetch url="https://example.com"
```

## Config
TIMEOUT=60
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

    // --- Frontmatter format tests ---

    #[test]
    fn test_frontmatter_basic() {
        let skill = parse_skill_md(sample_frontmatter_md()).unwrap();
        assert_eq!(skill.name, "my-cool-skill");
        assert_eq!(skill.version, "2.1.0");
        assert_eq!(skill.author, "janedoe");
    }

    #[test]
    fn test_frontmatter_description_from_purpose() {
        let skill = parse_skill_md(sample_frontmatter_md()).unwrap();
        // ## Purpose section maps to description
        assert!(skill.description.contains("automates web tasks"));
    }

    #[test]
    fn test_frontmatter_tools_section() {
        let skill = parse_skill_md(sample_frontmatter_md()).unwrap();
        // ## Tools is an alias for ## MCP Tools
        assert_eq!(skill.mcp_tools, vec!["web.fetch", "web.parse"]);
    }

    #[test]
    fn test_frontmatter_instructions() {
        let skill = parse_skill_md(sample_frontmatter_md()).unwrap();
        assert!(skill.instructions.contains("Follow the steps carefully"));
    }

    #[test]
    fn test_frontmatter_config() {
        let skill = parse_skill_md(sample_frontmatter_md()).unwrap();
        assert!(skill.config_section.contains("TIMEOUT=60"));
    }

    #[test]
    fn test_frontmatter_inline_tags() {
        let content = "---\nname: tagged\nversion: 1.0.0\ntags: [web, ai, tools]\n---\n# Tagged\n\n## Description\nHas tags.\n";
        let skill = parse_skill_md(content).unwrap();
        assert_eq!(skill.name, "tagged");
    }

    #[test]
    fn test_frontmatter_name_from_title() {
        // No `name:` in frontmatter, should derive from # Title
        let content = "---\nversion: 1.0.0\nauthor: someone\n---\n# My Great Skill\n\n## Description\nDerives name from title.\n";
        let skill = parse_skill_md(content).unwrap();
        assert_eq!(skill.name, "my-great-skill");
        assert_eq!(skill.version, "1.0.0");
    }

    #[test]
    fn test_frontmatter_missing_name_and_title_errors() {
        let content = "---\nversion: 1.0.0\n---\n\nNo heading here.\n";
        let result = parse_skill_md(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_frontmatter_missing_closing_delimiter() {
        let content = "---\nname: broken\nversion: 1.0.0\n\n## Description\nNo closing delimiter.\n";
        let result = parse_skill_md(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_frontmatter_description_fallback() {
        // Frontmatter description used when no ## Description or ## Purpose section
        let content =
            "---\nname: fb-skill\ndescription: From frontmatter\n---\n# Fallback\n\n## Instructions\nDo stuff.\n";
        let skill = parse_skill_md(content).unwrap();
        assert_eq!(skill.description, "From frontmatter");
    }

    #[test]
    fn test_frontmatter_description_overridden_by_section() {
        // ## Description section takes precedence over frontmatter description
        let content = "---\nname: override\ndescription: From frontmatter\n---\n# Override\n\n## Description\nFrom section.\n";
        let skill = parse_skill_md(content).unwrap();
        assert_eq!(skill.description, "From section.");
    }

    #[test]
    fn test_empty_content_errors() {
        let result = parse_skill_md("");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_first_line_errors() {
        let result = parse_skill_md("Some random text\n## Description\nStuff\n");
        assert!(result.is_err());
    }

    #[test]
    fn test_frontmatter_minimal() {
        let content = "---\nname: minimal\n---\n";
        let skill = parse_skill_md(content).unwrap();
        assert_eq!(skill.name, "minimal");
        assert_eq!(skill.version, "");
        assert_eq!(skill.author, "");
        assert_eq!(skill.description, "");
    }

    #[test]
    fn test_doc_analyzer_skill_parses() {
        let content = r#"---
name: doc-analyzer
version: 2.0.0
author: mcclawd-team
description: Use when the user uploads documents for analysis.
tags:
  - documents
  - analysis
---
# Document Analyzer

## Description
Analyze uploaded documents by extracting content with MCP tools.

## MCP Tools
- filesystem
- langextract
- scrapling

## Context
You are a document analysis expert.

## Instructions
When the user uploads a document:
1. List attachments
2. Extract content
3. Produce analysis
"#;
        let skill = parse_skill_md(content).unwrap();
        assert_eq!(skill.name, "doc-analyzer");
        assert_eq!(skill.version, "2.0.0");
        assert_eq!(skill.author, "mcclawd-team");
        // ## Description overrides frontmatter description
        assert!(skill.description.contains("Analyze uploaded documents"));
        assert_eq!(skill.mcp_tools, vec!["filesystem", "langextract", "scrapling"]);
        assert!(skill.context.contains("document analysis expert"));
        assert!(skill.instructions.contains("List attachments"));
    }

    #[test]
    fn test_doc_analyzer_mcp_tools_match_gateway() {
        // Verify that the doc-analyzer skill's MCP tool names match the
        // AgentGateway target names (filesystem, langextract, scrapling)
        let content = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join(".mcclawd/skills/doc-analyzer/SKILL.md"),
        );
        if let Ok(content) = content {
            let skill = parse_skill_md(&content).unwrap();
            assert!(skill.mcp_tools.contains(&"filesystem".to_string()));
            assert!(skill.mcp_tools.contains(&"langextract".to_string()));
            assert!(skill.mcp_tools.contains(&"scrapling".to_string()));
        }
        // Skip if file doesn't exist (CI without workspace)
    }
}
