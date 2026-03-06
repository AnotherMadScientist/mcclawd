use mcclawd_agent::context::ContextBuilder;
use mcclawd_agent::workspace::Workspace;
use std::path::PathBuf;

#[test]
fn test_context_builds_system_prompt_with_soul_first() {
    let ws = Workspace {
        name: "test".to_string(),
        soul: Some("You are a test agent.".to_string()),
        agents: Some("# Agents\n## Default Skills\n- memory".to_string()),
        user: Some("# User\nName: Alice".to_string()),
        identity: None,
        tools: None,
        heartbeat: None,
        path: PathBuf::from("/tmp"),
    };

    let builder = ContextBuilder::new(ws);
    let prompt = builder.build_system_prompt();

    // SOUL.md comes first
    assert!(prompt.starts_with("You are a test agent."));
    // USER.md is included
    assert!(prompt.contains("Alice"));
    // AGENTS.md is included
    assert!(prompt.contains("memory"));
    // Sections are separated by horizontal rules
    assert!(prompt.contains("---"));
}

#[test]
fn test_context_handles_missing_optional_files() {
    let ws = Workspace {
        name: "minimal".to_string(),
        soul: Some("Minimal agent.".to_string()),
        agents: None,
        user: None,
        identity: None,
        tools: None,
        heartbeat: None,
        path: PathBuf::from("/tmp"),
    };

    let builder = ContextBuilder::new(ws);
    let prompt = builder.build_system_prompt();
    assert!(prompt.contains("Minimal agent."));
    // Response guidelines are always appended when there's content
    assert!(prompt.contains("Response Guidelines"));
}

#[test]
fn test_context_empty_workspace() {
    let ws = Workspace {
        name: "empty".to_string(),
        soul: None,
        agents: None,
        user: None,
        identity: None,
        tools: None,
        heartbeat: None,
        path: PathBuf::from("/tmp"),
    };

    let builder = ContextBuilder::new(ws);
    let prompt = builder.build_system_prompt();
    assert!(prompt.is_empty());
}

#[test]
fn test_context_preserves_section_order() {
    let ws = Workspace {
        name: "ordered".to_string(),
        soul: Some("SOUL_MARKER".to_string()),
        agents: Some("AGENTS_MARKER".to_string()),
        user: Some("USER_MARKER".to_string()),
        identity: Some("IDENTITY_MARKER".to_string()),
        tools: Some("TOOLS_MARKER".to_string()),
        heartbeat: Some("HEARTBEAT_MARKER".to_string()),
        path: PathBuf::from("/tmp"),
    };

    let builder = ContextBuilder::new(ws);
    let prompt = builder.build_system_prompt();

    let soul_pos = prompt.find("SOUL_MARKER").unwrap();
    let identity_pos = prompt.find("IDENTITY_MARKER").unwrap();
    let user_pos = prompt.find("USER_MARKER").unwrap();
    let agents_pos = prompt.find("AGENTS_MARKER").unwrap();
    let tools_pos = prompt.find("TOOLS_MARKER").unwrap();
    let heartbeat_pos = prompt.find("HEARTBEAT_MARKER").unwrap();

    // Order must be: SOUL → IDENTITY → USER → AGENTS → TOOLS → HEARTBEAT
    assert!(soul_pos < identity_pos, "SOUL must come before IDENTITY");
    assert!(identity_pos < user_pos, "IDENTITY must come before USER");
    assert!(user_pos < agents_pos, "USER must come before AGENTS");
    assert!(agents_pos < tools_pos, "AGENTS must come before TOOLS");
    assert!(tools_pos < heartbeat_pos, "TOOLS must come before HEARTBEAT");
}

#[test]
fn test_context_includes_new_workspace_files() {
    let ws = Workspace {
        name: "full".to_string(),
        soul: Some("SOUL".to_string()),
        agents: None,
        user: None,
        identity: Some("IDENTITY_CONTENT".to_string()),
        tools: Some("TOOLS_CONTENT".to_string()),
        heartbeat: Some("HEARTBEAT_CONTENT".to_string()),
        path: PathBuf::from("/tmp"),
    };

    let builder = ContextBuilder::new(ws);
    let prompt = builder.build_system_prompt();
    assert!(prompt.contains("IDENTITY_CONTENT"));
    assert!(prompt.contains("TOOLS_CONTENT"));
    assert!(prompt.contains("HEARTBEAT_CONTENT"));
}
