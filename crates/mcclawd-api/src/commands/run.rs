use mcclawd_agent::engine::AgentEngine;
use mcclawd_agent::workspace::WorkspaceLoader;
use mcclawd_core::config::McclawdConfig;
use mcclawd_core::secrets::{EncryptedFileBackend, SecretBackend};
use rig::completion::Prompt;

pub async fn execute(prompt: &str, workspace_name: &str) -> anyhow::Result<()> {
    let config = McclawdConfig::default();

    // 1. Load workspace
    let loader = WorkspaceLoader::new(config.workspaces_dir());
    let workspace = loader.load(workspace_name)?;
    tracing::info!(workspace = %workspace.name, "Loaded workspace");

    // 2. Get API key from secrets
    let passphrase = "mcclawd-local-dev";
    let secrets = EncryptedFileBackend::new(&config.secrets_path(), passphrase)?;
    let api_key = secrets
        .get("ANTHROPIC_API_KEY")
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!("ANTHROPIC_API_KEY not found. Run: mc secrets set ANTHROPIC_API_KEY")
        })?;
    tracing::info!("API key loaded from secrets");

    // 3. Build agent via Rig
    let max_turns = config.agent.max_turns;
    let (agent, _memory) = AgentEngine::build(workspace, &api_key, max_turns)?;
    tracing::info!(max_turns, "Agent built");

    // 4. Run prompt (non-streaming for Phase 0 simplicity)
    eprintln!("McClawd v0.1.0 — thinking...\n");

    let response = agent.prompt(prompt).await?;
    println!("{}", response);

    Ok(())
}
