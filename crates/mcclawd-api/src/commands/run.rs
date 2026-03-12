use mcclawd_agent::engine::AgentEngine;
use mcclawd_agent::workspace::WorkspaceLoader;
use mcclawd_core::config::McclawdConfig;
use mcclawd_core::secrets::{EncryptedFileBackend, SecretBackend};
use mcclawd_swarm::{SwarmConfig, SwarmCoordinator, SwarmPlanner};
use rig::completion::Prompt;

pub async fn execute(prompt: &str, workspace_name: &str, swarm: bool) -> anyhow::Result<()> {
    let config = McclawdConfig::default();

    // Try daemon mode first (handles both swarm and single-agent)
    let daemon_port = 9090;
    if let Ok(true) = try_daemon(prompt, workspace_name, daemon_port).await {
        return Ok(());
    }

    // Fallback: in-process execution
    tracing::info!("Daemon not available, running in-process");

    if swarm {
        run_swarm(prompt, workspace_name, &config).await
    } else {
        run_in_process(prompt, workspace_name, &config).await
    }
}

/// Try to submit the task to the daemon via HTTP API.
/// Returns Ok(true) if daemon handled it, Ok(false) if daemon unavailable.
async fn try_daemon(prompt: &str, _workspace: &str, port: u16) -> anyhow::Result<bool> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    // Login first to get JWT
    let login_resp = client
        .post(format!("http://127.0.0.1:{port}/api/auth/login"))
        .json(&serde_json::json!({"password": "mcclawd-local-dev"}))
        .send()
        .await;

    let login_resp = match login_resp {
        Ok(r) => r,
        Err(_) => return Ok(false), // daemon not running
    };

    if !login_resp.status().is_success() {
        return Ok(false);
    }

    let token: serde_json::Value = login_resp.json().await?;
    let token = token["token"].as_str().unwrap_or("");

    // Create task
    let task_resp = client
        .post(format!("http://127.0.0.1:{port}/api/tasks"))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "prompt": prompt,
        }))
        .send()
        .await?;

    if !task_resp.status().is_success() {
        tracing::warn!("daemon rejected task: {}", task_resp.status());
        return Ok(false);
    }

    let task: serde_json::Value = task_resp.json().await?;
    let task_id = task["id"].as_str().unwrap_or("unknown");

    eprintln!("McClawd v0.5.0 — submitted to daemon (task {task_id})\n");
    eprintln!("Task submitted. View output at http://127.0.0.1:{port}/api/tasks/{task_id}");

    Ok(true)
}

/// Phase 0 in-process execution (fallback when daemon unavailable).
async fn run_in_process(
    prompt: &str,
    workspace_name: &str,
    config: &McclawdConfig,
) -> anyhow::Result<()> {
    let loader = WorkspaceLoader::new(config.workspaces_dir());
    let workspace = loader.load(workspace_name)?;
    tracing::info!(workspace = %workspace.name, "Loaded workspace");

    let passphrase = "mcclawd-local-dev";
    let secrets = EncryptedFileBackend::new(&config.secrets_path(), passphrase)?;
    let api_key = secrets
        .get("ANTHROPIC_API_KEY")
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!("ANTHROPIC_API_KEY not found. Run: mc secrets set ANTHROPIC_API_KEY")
        })?;
    tracing::info!("API key loaded from secrets");

    let max_turns = config.agent.max_turns;
    let (agent, _memory, _mcp_conns) =
        AgentEngine::build(workspace, &api_key, max_turns, config, None, &config.agent.model).await?;
    tracing::info!(max_turns, "Agent built");

    eprintln!("McClawd v0.5.0 — thinking...\n");
    let response = agent.prompt(prompt).await?;
    println!("{}", response);

    Ok(())
}

/// Swarm mode: decompose prompt into a DAG of subtasks and execute in parallel.
async fn run_swarm(
    prompt: &str,
    _workspace_name: &str,
    config: &McclawdConfig,
) -> anyhow::Result<()> {
    let passphrase = "mcclawd-local-dev";
    let secrets = EncryptedFileBackend::new(&config.secrets_path(), passphrase)?;
    let api_key = secrets
        .get("ANTHROPIC_API_KEY")
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!("ANTHROPIC_API_KEY not found. Run: mc secrets set ANTHROPIC_API_KEY")
        })?;

    eprintln!("McClawd v0.5.0 — swarm mode, planning...\n");

    let planner = SwarmPlanner::new(Some(config.agent.model.clone()), api_key);
    let dag = planner.decompose(prompt, &[]).await?;

    let waves = dag.topological_waves()?;
    let wave_count = waves.len();
    let subtask_count: usize = waves.iter().map(|w| w.len()).sum();
    eprintln!("Plan: {subtask_count} subtasks in {wave_count} waves. Executing...\n");

    let coordinator = SwarmCoordinator::new(SwarmConfig::default());
    let result = coordinator.execute(prompt, &dag).await?;

    println!("{}", result.final_output);

    eprintln!(
        "\nSwarm complete: {} subtasks, {}ms",
        result.subtask_results.len(),
        result.total_duration_ms
    );

    Ok(())
}
