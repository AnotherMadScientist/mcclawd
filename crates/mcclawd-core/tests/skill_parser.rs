use mcclawd_core::skill_parser::parse_skill_md;

const SAMPLE_SKILL: &str = r#"# Skill: web-scraper
version: 1.0.0
author: mcclawd-team

## Description
Web scraping tools for extracting content from websites.

## MCP Tools
- scrapling
- langextract

## Install
```bash
pip install scrapling langextract
npm install -g puppeteer
```

## Context
You have access to web scraping tools. Use scrapling for fast extraction
and langextract for language-specific content parsing.
"#;

#[test]
fn parses_skill_md_name_and_version() {
    let skill = parse_skill_md(SAMPLE_SKILL).unwrap();
    assert_eq!(skill.name, "web-scraper");
    assert_eq!(skill.version, "1.0.0");
    assert_eq!(skill.author, "mcclawd-team");
}

#[test]
fn parses_skill_md_description() {
    let skill = parse_skill_md(SAMPLE_SKILL).unwrap();
    assert_eq!(
        skill.description,
        "Web scraping tools for extracting content from websites."
    );
}

#[test]
fn parses_skill_md_mcp_tools() {
    let skill = parse_skill_md(SAMPLE_SKILL).unwrap();
    assert_eq!(skill.mcp_tools, vec!["scrapling", "langextract"]);
}

#[test]
fn parses_skill_md_install_steps() {
    let skill = parse_skill_md(SAMPLE_SKILL).unwrap();
    assert_eq!(
        skill.install_steps,
        vec![
            "pip install scrapling langextract",
            "npm install -g puppeteer",
        ]
    );
}

#[test]
fn parses_skill_md_context() {
    let skill = parse_skill_md(SAMPLE_SKILL).unwrap();
    assert!(skill.context.contains("web scraping tools"));
    assert!(skill.context.contains("langextract"));
}

#[test]
fn returns_error_on_missing_header() {
    let bad = "## Description\nJust a description, no header.";
    assert!(parse_skill_md(bad).is_err());
}

#[test]
fn handles_minimal_skill() {
    let minimal =
        "# Skill: minimal\nversion: 0.1.0\nauthor: test\n\n## Description\nMinimal skill.\n";
    let skill = parse_skill_md(minimal).unwrap();
    assert_eq!(skill.name, "minimal");
    assert!(skill.mcp_tools.is_empty());
    assert!(skill.install_steps.is_empty());
    assert!(skill.context.is_empty());
}
