//! Phase 1 integration tests — skills, task state machine, sandbox basics.
//!
//! Non-Docker tests run by default. Docker tests are #[ignore]d.

use mcclawd_core::skill_parser::parse_skill_md;
use mcclawd_core::SkillLoader;
use mcclawd_tasks::manager::TaskStatus;
use mcclawd_tasks::TaskManager;
use std::fs;
use tempfile::TempDir;

// ── Skill tests ──────────────────────────────────────────────────────

#[test]
fn skill_roundtrip_parse_and_load() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = tmp
        .path()
        .join(".mcclawd")
        .join("skills")
        .join("web-tools");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "\
# Skill: web-tools
version: 2.0.0
author: test

## Description
Web tools.

## MCP Tools
- scrapling
- langextract

## Install
```bash
pip install scrapling
```

## Context
Use web tools.
",
    )
    .unwrap();

    let loader = SkillLoader::new(tmp.path().to_path_buf());
    let skills = loader.discover_all().unwrap();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "web-tools");
    assert_eq!(skills[0].version, "2.0.0");
    assert_eq!(skills[0].author, "test");
    assert_eq!(skills[0].description, "Web tools.");
    assert_eq!(skills[0].mcp_tools, vec!["scrapling", "langextract"]);
    assert_eq!(skills[0].install_steps, vec!["pip install scrapling"]);
    assert_eq!(skills[0].context, "Use web tools.");
}

#[test]
fn skill_loader_returns_empty_when_no_skills_dir() {
    let tmp = TempDir::new().unwrap();
    let loader = SkillLoader::new(tmp.path().to_path_buf());
    let skills = loader.discover_all().unwrap();
    assert!(skills.is_empty());
}

#[test]
fn skill_loader_discovers_multiple_skills() {
    let tmp = TempDir::new().unwrap();
    let skills_root = tmp.path().join(".mcclawd").join("skills");

    // Skill 1
    let s1 = skills_root.join("alpha");
    fs::create_dir_all(&s1).unwrap();
    fs::write(
        s1.join("SKILL.md"),
        "# Skill: alpha\nversion: 1.0.0\nauthor: a\n\n## Description\nAlpha.\n",
    )
    .unwrap();

    // Skill 2
    let s2 = skills_root.join("beta");
    fs::create_dir_all(&s2).unwrap();
    fs::write(
        s2.join("SKILL.md"),
        "# Skill: beta\nversion: 2.0.0\nauthor: b\n\n## Description\nBeta.\n",
    )
    .unwrap();

    let loader = SkillLoader::new(tmp.path().to_path_buf());
    let mut skills = loader.discover_all().unwrap();
    skills.sort_by(|a, b| a.name.cmp(&b.name));

    assert_eq!(skills.len(), 2);
    assert_eq!(skills[0].name, "alpha");
    assert_eq!(skills[1].name, "beta");
}

#[test]
fn skill_parser_roundtrip() {
    let content = "\
# Skill: test-skill
version: 1.0.0
author: mcclawd-team

## Description
Test skill for integration tests.

## MCP Tools
- filesystem

## Install
```bash
echo \"test-skill installed\"
```

## Context
You have access to filesystem tools for testing.
";
    let skill = parse_skill_md(content).unwrap();
    assert_eq!(skill.name, "test-skill");
    assert_eq!(skill.version, "1.0.0");
    assert_eq!(skill.author, "mcclawd-team");
    assert_eq!(skill.description, "Test skill for integration tests.");
    assert_eq!(skill.mcp_tools, vec!["filesystem"]);
    assert_eq!(
        skill.install_steps,
        vec!["echo \"test-skill installed\""]
    );
    assert_eq!(
        skill.context,
        "You have access to filesystem tools for testing."
    );
}

// ── Task state machine tests ─────────────────────────────────────────

#[test]
fn task_full_lifecycle() {
    let mut mgr = TaskManager::new();

    // Pending -> Building -> Running -> Completed
    let id = mgr.create_task("test prompt".to_string());
    let task = mgr.get_task(&id).unwrap();
    assert!(
        matches!(task.status, TaskStatus::Pending),
        "expected Pending, got {:?}",
        task.status
    );

    mgr.building(&id);
    let task = mgr.get_task(&id).unwrap();
    assert!(
        matches!(task.status, TaskStatus::Building),
        "expected Building, got {:?}",
        task.status
    );

    mgr.running(&id);
    let task = mgr.get_task(&id).unwrap();
    assert!(
        matches!(task.status, TaskStatus::Running),
        "expected Running, got {:?}",
        task.status
    );

    mgr.complete_task(&id);
    let task = mgr.get_task(&id).unwrap();
    assert!(
        matches!(task.status, TaskStatus::Completed),
        "expected Completed, got {:?}",
        task.status
    );
}

#[test]
fn task_crash_restart_cycle() {
    let mut mgr = TaskManager::new();

    let id = mgr.create_task("crashy".to_string());
    mgr.building(&id);
    mgr.running(&id);

    // Simulate crash -> restart with backoff
    mgr.restarting(&id, 1, 1);
    match &mgr.get_task(&id).unwrap().status {
        TaskStatus::Restarting {
            attempt,
            next_retry_secs,
        } => {
            assert_eq!(*attempt, 1);
            assert_eq!(*next_retry_secs, 1);
        }
        other => panic!("expected Restarting, got {:?}", other),
    }

    mgr.running(&id);
    mgr.restarting(&id, 2, 2);
    match &mgr.get_task(&id).unwrap().status {
        TaskStatus::Restarting {
            attempt,
            next_retry_secs,
        } => {
            assert_eq!(*attempt, 2);
            assert_eq!(*next_retry_secs, 2);
        }
        other => panic!("expected Restarting, got {:?}", other),
    }

    mgr.running(&id);
    mgr.restarting(&id, 3, 4);

    // Max retries -> Failed
    mgr.fail_task(&id, "max retries exceeded".to_string());
    let task = mgr.get_task(&id).unwrap();
    assert!(
        matches!(task.status, TaskStatus::Failed(_)),
        "expected Failed, got {:?}",
        task.status
    );
    if let TaskStatus::Failed(ref msg) = task.status {
        assert_eq!(msg, "max retries exceeded");
    }
}

#[test]
fn task_direct_fail_from_pending() {
    let mut mgr = TaskManager::new();
    let id = mgr.create_task("doomed".to_string());

    mgr.fail_task(&id, "rejected by policy".to_string());
    let task = mgr.get_task(&id).unwrap();
    assert!(matches!(task.status, TaskStatus::Failed(_)));
    if let TaskStatus::Failed(ref msg) = task.status {
        assert_eq!(msg, "rejected by policy");
    }
}

#[test]
fn task_manager_handles_unknown_id() {
    let mut mgr = TaskManager::new();
    let id = mgr.create_task("real task".to_string());

    // Operations on a nonexistent ID should not panic
    let fake_id = mcclawd_core::types::TaskId::new();
    mgr.building(&fake_id);
    mgr.running(&fake_id);
    mgr.complete_task(&fake_id);
    mgr.fail_task(&fake_id, "nope".to_string());

    // Real task unaffected
    let task = mgr.get_task(&id).unwrap();
    assert!(matches!(task.status, TaskStatus::Pending));
    assert!(mgr.get_task(&fake_id).is_none());
}
