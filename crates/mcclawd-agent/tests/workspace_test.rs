use mcclawd_agent::workspace::WorkspaceLoader;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_load_workspace_with_all_files() {
    let dir = TempDir::new().unwrap();
    let ws_dir = dir.path().join("workspaces").join("default");
    fs::create_dir_all(&ws_dir).unwrap();
    fs::write(ws_dir.join("SOUL.md"), "# Soul\nYou are McClawd.").unwrap();
    fs::write(
        ws_dir.join("AGENTS.md"),
        "# Agents\n## Default Skills\n- memory",
    )
    .unwrap();
    fs::write(ws_dir.join("USER.md"), "# User\nName: Test User").unwrap();

    let loader = WorkspaceLoader::new(dir.path().join("workspaces"));
    let ws = loader.load("default").unwrap();

    assert!(ws.soul.is_some());
    assert!(ws.agents.is_some());
    assert!(ws.user.is_some());
    assert!(ws.soul.unwrap().contains("McClawd"));
}

#[test]
fn test_load_workspace_missing_optional_files() {
    let dir = TempDir::new().unwrap();
    let ws_dir = dir.path().join("workspaces").join("minimal");
    fs::create_dir_all(&ws_dir).unwrap();
    fs::write(ws_dir.join("SOUL.md"), "# Soul\nMinimal agent.").unwrap();

    let loader = WorkspaceLoader::new(dir.path().join("workspaces"));
    let ws = loader.load("minimal").unwrap();

    assert!(ws.soul.is_some());
    assert!(ws.agents.is_none());
    assert!(ws.user.is_none());
}

#[test]
fn test_load_nonexistent_workspace_fails() {
    let dir = TempDir::new().unwrap();
    let loader = WorkspaceLoader::new(dir.path().join("workspaces"));
    let result = loader.load("nonexistent");
    assert!(result.is_err());
}
