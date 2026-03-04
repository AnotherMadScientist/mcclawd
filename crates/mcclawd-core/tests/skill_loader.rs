// crates/mcclawd-core/tests/skill_loader.rs

use mcclawd_core::skill_loader::SkillLoader;
use std::fs;
use tempfile::TempDir;

fn create_test_skill(dir: &std::path::Path, name: &str) {
    let skill_dir = dir.join(".mcclawd").join("skills").join(name);
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        format!(
            "# Skill: {name}\nversion: 1.0.0\nauthor: test\n\n## Description\nTest skill {name}.\n\n## MCP Tools\n- {name}_tool\n\n## Install\n```bash\necho install-{name}\n```\n\n## Context\nContext for {name}.\n"
        ),
    )
    .unwrap();
}

fn create_agents_md(dir: &std::path::Path, skills: &[&str]) {
    let workspace_dir = dir.join(".mcclawd");
    fs::create_dir_all(&workspace_dir).unwrap();
    let skill_list = skills
        .iter()
        .map(|s| format!("- {s}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        workspace_dir.join("AGENTS.md"),
        format!("# Agent: default\n\n## Skills\n{skill_list}\n"),
    )
    .unwrap();
}

#[test]
fn discovers_installed_skills() {
    let tmp = TempDir::new().unwrap();
    create_test_skill(tmp.path(), "alpha");
    create_test_skill(tmp.path(), "beta");

    let loader = SkillLoader::new(tmp.path().to_path_buf());
    let skills = loader.discover_all().unwrap();

    assert_eq!(skills.len(), 2);
    let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));
}

#[test]
fn resolves_skills_for_agent() {
    let tmp = TempDir::new().unwrap();
    create_test_skill(tmp.path(), "alpha");
    create_test_skill(tmp.path(), "beta");
    create_test_skill(tmp.path(), "gamma");
    create_agents_md(tmp.path(), &["alpha", "gamma"]);

    let loader = SkillLoader::new(tmp.path().to_path_buf());
    let skills = loader.resolve_for_agent("default").unwrap();

    assert_eq!(skills.len(), 2);
    let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"gamma"));
    assert!(!names.contains(&"beta"));
}

#[test]
fn returns_empty_when_no_skills_dir() {
    let tmp = TempDir::new().unwrap();
    let loader = SkillLoader::new(tmp.path().to_path_buf());
    let skills = loader.discover_all().unwrap();
    assert!(skills.is_empty());
}
