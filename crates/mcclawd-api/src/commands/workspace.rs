use mcclawd_agent::workspace::WorkspaceLoader;
use mcclawd_core::config::McclawdConfig;

pub async fn init(name: &str) -> anyhow::Result<()> {
    let config = McclawdConfig::default();
    let loader = WorkspaceLoader::new(config.workspaces_dir());
    let path = loader.scaffold(name)?;
    println!("Workspace '{}' created at {}", name, path.display());
    Ok(())
}

pub async fn list() -> anyhow::Result<()> {
    let config = McclawdConfig::default();
    let ws_dir = config.workspaces_dir();
    if !ws_dir.exists() {
        println!("No workspaces found. Run `mc workspace init` to create one.");
        return Ok(());
    }
    let mut found = false;
    for entry in std::fs::read_dir(ws_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            if !found {
                println!("Workspaces:");
                found = true;
            }
            println!("  {}", entry.file_name().to_string_lossy());
        }
    }
    if !found {
        println!("No workspaces found. Run `mc workspace init` to create one.");
    }
    Ok(())
}
