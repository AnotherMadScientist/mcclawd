use axum::extract::DefaultBodyLimit;
use axum::http::{self, HeaderValue};
use std::fs;
use std::process;
use std::sync::Arc;

use crate::sandbox::container::PersistentHandle;
use crate::sandbox::{ImageBuilder, SandboxOrchestrator};
use crate::server::pg_store::PgTaskStore;
use crate::server::runner_build;
use crate::server::{routes, state::AppState};
use crate::supervisor::AgentSupervisor;
use mcclawd_core::secrets::{EncryptedFileBackend, SecretBackend};
use mcclawd_core::skills::SandboxConfig;
use mcclawd_core::types::TaskId;
use mcclawd_core::McclawdConfig;
use mcclawd_tasks::manager::TaskStatus;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

/// Kill any stale server process from a previous run, then wait for the port to be released.
fn kill_stale_server(port: u16) {
    let pid_path = pid_file_path();
    if !pid_path.exists() {
        return;
    }

    let pid_str = match fs::read_to_string(&pid_path) {
        Ok(s) => s.trim().to_string(),
        Err(e) => {
            tracing::warn!("Could not read PID file: {e}");
            let _ = fs::remove_file(&pid_path);
            return;
        }
    };

    let pid: u32 = match pid_str.parse() {
        Ok(p) => p,
        Err(_) => {
            tracing::warn!("Invalid PID in daemon.pid: {pid_str:?}");
            let _ = fs::remove_file(&pid_path);
            return;
        }
    };

    // Never kill ourselves
    if pid == process::id() {
        let _ = fs::remove_file(&pid_path);
        return;
    }

    // Check if process is alive (signal 0 = existence check)
    let alive = unsafe { libc::kill(pid as libc::pid_t, 0) } == 0;
    if !alive {
        tracing::info!("Stale PID file (process {pid} not running), removing");
        let _ = fs::remove_file(&pid_path);
    } else {
        tracing::info!("Killing stale server process {pid} (SIGTERM)");
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }

        // Wait 500ms for graceful shutdown
        std::thread::sleep(std::time::Duration::from_millis(500));

        // If still alive, force kill
        if unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
            tracing::warn!("Process {pid} did not exit after SIGTERM, sending SIGKILL");
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGKILL);
            }
        }

        let _ = fs::remove_file(&pid_path);
    }

    // Wait up to 3 seconds for the port to be released
    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    for i in 0..6 {
        match std::net::TcpListener::bind(addr) {
            Ok(_listener) => {
                // Port is free — drop the listener immediately so execute() can bind it
                tracing::info!("Port {port} is free");
                return;
            }
            Err(_) if i < 5 => {
                tracing::info!("Port {port} still in use, waiting 500ms...");
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            Err(e) => {
                tracing::warn!("Port {port} still bound after 3s: {e}");
                return;
            }
        }
    }
}

/// Wait for SIGTERM or Ctrl-C, then initiate graceful shutdown.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C, shutting down gracefully");
        }
        _ = terminate => {
            tracing::info!("Received SIGTERM, shutting down gracefully");
        }
    }
}

fn pid_file_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".mcclawd")
        .join("daemon.pid")
}

fn write_pid_file() -> anyhow::Result<()> {
    let path = pid_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, process::id().to_string())?;
    Ok(())
}

fn remove_pid_file() {
    let _ = fs::remove_file(pid_file_path());
}

/// Map a postgres task row (status string + error_message) to a TaskStatus enum.
pub fn row_to_status(status: &str, error_message: Option<&str>) -> TaskStatus {
    match status {
        "Pending" => TaskStatus::Pending,
        "Building" => TaskStatus::Building,
        "Running" => TaskStatus::Running,
        "Completed" => TaskStatus::Completed,
        "Failed" => TaskStatus::Failed(error_message.unwrap_or("unknown error").to_string()),
        _ => TaskStatus::Running, // default for unknown statuses
    }
}

/// Connect to PostgreSQL with retry (required dependency).
/// Retries 3 times with exponential backoff (1s, 2s, 4s) before failing.
async fn connect_postgres(database_url: &str) -> anyhow::Result<PgTaskStore> {
    let mut last_err = None;
    for attempt in 0..3 {
        if attempt > 0 {
            let delay = std::time::Duration::from_secs(1 << attempt);
            tracing::info!("Retrying PostgreSQL connection in {}s (attempt {}/3)...", delay.as_secs(), attempt + 1);
            tokio::time::sleep(delay).await;
        }
        let connect_result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(10)
                .connect(database_url),
        )
        .await;

        match connect_result {
            Ok(Ok(pool)) => {
                sqlx::migrate!("../mcclawd-core/migrations")
                    .run(&pool)
                    .await
                    .map_err(|e| anyhow::anyhow!("PostgreSQL migration failed: {e}"))?;
                tracing::info!("PostgreSQL connected, migrations applied");
                return Ok(PgTaskStore::new(pool));
            }
            Ok(Err(e)) => {
                tracing::warn!("PostgreSQL connection attempt {} failed: {e}", attempt + 1);
                last_err = Some(format!("connection error: {e}"));
            }
            Err(_) => {
                tracing::warn!("PostgreSQL connection attempt {} timed out", attempt + 1);
                last_err = Some("connection timed out after 5s".to_string());
            }
        }
    }
    Err(anyhow::anyhow!(
        "PostgreSQL is required but unavailable after 3 attempts: {}. \
         Set database_url in ~/.mcclawd/config.toml or run: docker compose up -d postgres",
        last_err.unwrap_or_default()
    ))
}

pub async fn execute(port: u16) -> anyhow::Result<()> {
    kill_stale_server(port);

    let config_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".mcclawd")
        .join("config.toml");
    let config = McclawdConfig::load(&config_path)?;

    // PostgreSQL is a required dependency — fail loudly if unavailable.
    // Priority: DATABASE_URL env var > config.toml > localhost fallback.
    // In Docker Compose, set DATABASE_URL=postgresql://mcclawd:mcclawd@postgres:5432/mcclawd
    // to use the service name on the internal network.
    let database_url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| config.database_url.clone())
        .unwrap_or_else(|| {
            "postgresql://mcclawd:mcclawd@localhost:5432/mcclawd".to_string()
        });
    let pg_store = connect_postgres(&database_url).await?;

    // Initialize supervisor if Docker is available
    let supervisor = match SandboxOrchestrator::new() {
        Ok(orchestrator) => {
            if orchestrator.health_check().await {
                let docker = bollard::Docker::connect_with_local_defaults()
                    .expect("Docker connection for ImageBuilder");
                let image_builder = Arc::new(ImageBuilder::new(docker));
                let sandbox_config = SandboxConfig::default();
                let supervisor = AgentSupervisor::new(
                    orchestrator,
                    image_builder,
                    sandbox_config,
                    4, // max concurrent agents
                );
                tracing::info!("Docker sandbox available");
                Some(Arc::new(supervisor))
            } else {
                tracing::warn!("Docker not available, running without sandbox");
                None
            }
        }
        Err(e) => {
            tracing::warn!("Docker not available: {e}");
            None
        }
    };

    let mut state = AppState::new(config, supervisor, pg_store.clone())?;

    // Hydrate usage data from database on startup
    match (
        pg_store.load_daily_usage().await,
        pg_store.load_model_usage().await,
        pg_store.load_task_usage().await,
    ) {
        (Ok(daily), Ok(models), Ok(tasks)) => {
            let pool = state.provider_pool.read().await;
            let n_daily = daily.len();
            let n_models = models.len();
            let n_tasks = tasks.len();
            pool.hydrate_usage(daily, models, tasks);
            tracing::info!(
                daily_records = n_daily,
                model_records = n_models,
                task_records = n_tasks,
                "Usage data hydrated from database"
            );
        }
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
            tracing::warn!("Failed to load usage data from database: {e}");
        }
    }

    // Open vault and auto-seed API keys from .env on every startup.
    // Vault is long-lived on disk — survives server restarts and re-registrations.
    // If vault.key is missing, create it. If secrets.enc is missing or corrupt, recreate.
    // Then seed ANTHROPIC_API_KEY and ANTHROPIC_ADMIN_KEY from env if not already present.
    {
        let (data_dir, secrets_path) = {
            let c = state.config.read().await;
            (c.data_dir.clone(), c.secrets_path())
        };
        let vault_key_path = data_dir.join("vault.key");

        // Ensure vault.key exists (create if missing — first-time or after full reset)
        if !vault_key_path.exists() {
            let key: [u8; 32] = rand::random();
            if let Some(parent) = vault_key_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            fs::write(&vault_key_path, key).map_err(|e| {
                anyhow::anyhow!("Failed to create vault.key: {e}")
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(
                    &vault_key_path,
                    fs::Permissions::from_mode(0o600),
                );
            }
            tracing::info!("Created vault.key (first-time setup)");
        }

        // Read vault key and open/create secrets.enc
        match fs::read(&vault_key_path) {
            Ok(vault_key_bytes) => {
                let passphrase: String =
                    vault_key_bytes.iter().map(|b| format!("{b:02x}")).collect();

                let backend = if secrets_path.exists() {
                    match EncryptedFileBackend::new(&secrets_path, &passphrase) {
                        Ok(b) => {
                            tracing::info!("Vault unlocked");
                            b
                        }
                        Err(e) => {
                            // secrets.enc is corrupted or key mismatch — recreate
                            tracing::warn!("Vault decrypt failed ({e}), recreating secrets.enc");
                            let _ = fs::remove_file(&secrets_path);
                            EncryptedFileBackend::new_empty(&secrets_path, &passphrase)
                                .map_err(|e| anyhow::anyhow!("Failed to create vault: {e}"))?
                        }
                    }
                } else {
                    tracing::info!("Creating new secrets vault");
                    EncryptedFileBackend::new_empty(&secrets_path, &passphrase)
                        .map_err(|e| anyhow::anyhow!("Failed to create vault: {e}"))?
                };

                // Auto-seed API keys from environment (idempotent — skips if already present)
                for env_key in &["ANTHROPIC_API_KEY", "ANTHROPIC_ADMIN_KEY"] {
                    if let Ok(val) = std::env::var(env_key) {
                        if !val.is_empty() {
                            match backend.get(env_key).await {
                                Ok(Some(existing)) if existing == val => {}
                                _ => {
                                    if let Err(e) = backend.set(env_key, &val).await {
                                        tracing::warn!("Failed to seed {env_key}: {e}");
                                    } else {
                                        tracing::info!("{env_key} seeded into vault from environment");
                                    }
                                }
                            }
                        }
                    }
                }

                let mut secrets = state.secrets.write().await;
                *secrets = Some(Arc::new(backend));
            }
            Err(e) => {
                tracing::error!("vault.key unreadable: {e}");
            }
        }
    }

    // Hydrate in-memory TaskManager from postgres on startup
    match pg_store.list_tasks().await {
        Ok(rows) => {
            let mut mgr = state.tasks.write().await;
            for (id, prompt, status, error_message, _tags) in &rows {
                let task_id = TaskId(id.clone());
                let task_status = row_to_status(status, error_message.as_deref());
                mgr.restore_task(task_id, prompt.clone(), task_status);
            }
            tracing::info!(count = rows.len(), "Restored {} tasks from postgres", rows.len());
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load tasks from postgres");
        }
    }

    // Hydrate scan cache from postgres
    match pg_store.load_scan_cache("admin").await {
        Ok(rows) => {
            for (skill_name, result_json) in rows {
                if let Ok(result) = serde_json::from_value::<mcclawd_core::scanner::ScanResult>(result_json) {
                    state.scan_cache.insert(skill_name, result);
                }
            }
            tracing::info!(count = state.scan_cache.len(), "Scan cache hydrated from database");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load scan cache from database");
        }
    }

    // Hydrate scheduled tasks from postgres
    match pg_store.load_scheduled_tasks("admin").await {
        Ok(rows) => {
            let count = rows.len();
            for (id, name, cron_expr, prompt, workspace, enabled) in rows {
                let req = mcclawd_tasks::scheduler::CreateScheduleRequest {
                    name,
                    cron_expression: cron_expr,
                    prompt,
                    workspace,
                    enabled,
                };
                // Restore directly into the scheduler with the original ID
                state.scheduler.restore_schedule(id, req).await;
            }
            tracing::info!(count, "Scheduled tasks hydrated from database");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load scheduled tasks from database");
        }
    }

    // Hydrate swarm run history from postgres (read-only — completed runs for display)
    match pg_store.load_swarm_runs("admin").await {
        Ok(rows) => {
            let count = rows.len();
            for (id, _name, status, result) in &rows {
                let result_str = result.as_ref().and_then(|v| v.as_str().map(|s| s.to_string()));
                state.swarm_registry.restore_run(id.clone(), status, result_str);
            }
            tracing::info!(count, "Swarm runs hydrated from database");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load swarm runs from database");
        }
    }

    state.config_path = Some(config_path);

    // Reconnect to persistent containers that survived a restart.
    // Containers have restart_policy=unless-stopped, so they keep running
    // even when the API server restarts (cargo-watch, crash, etc.).
    match pg_store.load_persistent_containers().await {
        Ok(rows) if !rows.is_empty() => {
            let count = rows.len();
            let reconnect_state = state.clone();
            tokio::spawn(async move {
                reconnect_persistent_containers(reconnect_state, rows).await;
            });
            tracing::info!(count, "Reconnecting to persistent containers in background");
        }
        Ok(_) => {} // no containers to reconnect
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load persistent containers from database");
        }
    }

    // Auto-build runner image in background if Docker is available and image doesn't exist.
    // Once ready, pre-initialize the system agent broadcast channel so WS clients connect instantly.
    if state.supervisor.is_some() {
        let project_root = std::env::current_dir().unwrap_or_else(|_| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .unwrap_or_default()
        });
        runner_build::spawn_runner_build(state.runner_build.clone(), project_root);
        tracing::info!("Runner image build check started in background");

        // Pre-create system agent broadcast channel so WS connections don't race.
        let sys_state = state.clone();
        tokio::spawn(async move {
            use crate::server::system_agent::SYSTEM_AGENT_TASK_ID;
            use mcclawd_core::types::TaskId;

            let task_id = TaskId(SYSTEM_AGENT_TASK_ID.to_string());
            sys_state.create_task_stream(&task_id).await;

            // Wait for image (up to 10 min for first build)
            let ready = runner_build::wait_for_image_ready(
                &sys_state.runner_build,
                std::time::Duration::from_secs(600),
            ).await;
            if ready {
                tracing::info!("Runner image available — starting system agent container");
                let agent_start = std::time::Instant::now();
                match crate::server::system_agent::ensure_system_agent_container(&sys_state).await {
                    Ok(handle) => {
                        let startup_secs = agent_start.elapsed().as_secs_f64();
                        tracing::info!(container_id = %handle.container_id, startup_secs, "System agent container running");
                        sys_state.runner_build.write().await.agent_startup_secs = Some(startup_secs);
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to start system agent container on startup");
                    }
                }
            } else {
                tracing::warn!("System agent unavailable — runner image build failed or timed out");
            }
        });
    }

    let shutdown_state = state.clone();
    let app = routes::api_router(state.clone())
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_origin([
                    "http://localhost:8080".parse::<HeaderValue>().unwrap(),
                    "http://127.0.0.1:8080".parse::<HeaderValue>().unwrap(),
                ])
                .allow_methods([
                    http::Method::GET,
                    http::Method::POST,
                    http::Method::PUT,
                    http::Method::DELETE,
                ])
                .allow_headers([http::header::CONTENT_TYPE, http::header::AUTHORIZATION]),
        )
        .layer(TraceLayer::new_for_http())
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024)); // 50MB (doc/image uploads)

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;

    write_pid_file()?;
    tracing::info!(
        "McClawd daemon PID {} listening on 127.0.0.1:{port}",
        process::id()
    );

    let server = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal());
    let result = server.await;

    // Graceful shutdown: stop persistent containers
    {
        // System agent
        if let Some(handle) = shutdown_state.system_agent.write().await.take() {
            let _ = handle.shutdown().await;
            let store = shutdown_state.pg_store.clone();
            let cid = handle.container_id.clone();
            let _ = store.delete_persistent_container(&cid).await;
            if let Ok(orch) = SandboxOrchestrator::new() {
                let _ = orch.cleanup_container(&cid).await;
            }
            tracing::info!("System agent container stopped");
        }
        // Task containers
        let handles: Vec<_> = shutdown_state.task_containers.write().await.drain().collect();
        for (tid, handle) in handles {
            let _ = handle.shutdown().await;
            let store = shutdown_state.pg_store.clone();
            let cid = handle.container_id.clone();
            let _ = store.delete_persistent_container(&cid).await;
            if let Ok(orch) = SandboxOrchestrator::new() {
                let _ = orch.cleanup_container(&cid).await;
            }
            tracing::info!(task_id = %tid, "Task container stopped");
        }
    }

    remove_pid_file();
    tracing::info!("McClawd daemon shut down cleanly");
    result?;
    Ok(())
}

/// Reconnect to persistent containers that survived a server restart.
/// For each container in the DB, check if it's still running via Docker,
/// then attach stdin and start the output forwarder.
async fn reconnect_persistent_containers(
    state: AppState,
    containers: Vec<(String, String, String, String)>,
) {
    let docker = match bollard::Docker::connect_with_local_defaults() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(error = %e, "Cannot reconnect containers — Docker unavailable");
            return;
        }
    };

    for (container_id, task_id_str, agent_type, _workspace_dir) in containers {
        // Check if container is still running
        let inspect = match docker.inspect_container(&container_id, None).await {
            Ok(info) => info,
            Err(_) => {
                tracing::info!(container_id = %container_id, "Container gone — removing stale record");
                let _ = state.pg_store.delete_persistent_container(&container_id).await;
                continue;
            }
        };

        let running = inspect
            .state
            .as_ref()
            .and_then(|s| s.running)
            .unwrap_or(false);

        if !running {
            tracing::info!(container_id = %container_id, "Container not running — cleaning up");
            let _ = state.pg_store.delete_persistent_container(&container_id).await;
            if let Ok(orch) = SandboxOrchestrator::new() {
                let _ = orch.cleanup_container(&container_id).await;
            }
            continue;
        }

        // Container is running — reconnect stdin
        let task_id = TaskId(task_id_str.clone());
        let handle = match PersistentHandle::connect(&docker, container_id.clone(), task_id.clone())
            .await
        {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(
                    container_id = %container_id,
                    error = %e,
                    "Failed to reconnect to container stdin"
                );
                let _ = state.pg_store.delete_persistent_container(&container_id).await;
                continue;
            }
        };

        // Start background output forwarder
        let chunk_state = state.clone();
        let fwd_task_id = task_id.clone();
        let reader_cid = container_id.clone();

        // Ensure broadcast channel exists
        state.create_task_stream(&task_id).await;

        let (chunk_tx, mut chunk_rx) =
            tokio::sync::mpsc::channel::<mcclawd_channels::OutboundChunk>(256);

        tokio::spawn(async move {
            if let Ok(orch) = SandboxOrchestrator::new() {
                if let Err(e) = orch.stream_agent_output(&reader_cid, chunk_tx).await {
                    tracing::warn!(error = %e, "Reconnected output reader ended");
                }
            }
        });

        let fwd_state = chunk_state.clone();
        tokio::spawn(async move {
            use mcclawd_channels::OutboundChunk;
            while let Some(chunk) = chunk_rx.recv().await {
                let tx = {
                    let streams = fwd_state.task_streams.read().await;
                    streams.get(&fwd_task_id).cloned()
                };
                if let Some(tx) = tx {
                    match &chunk {
                        OutboundChunk::ChatHistory(json) => {
                            if let Ok(messages) =
                                serde_json::from_str::<Vec<rig::completion::message::Message>>(json)
                            {
                                fwd_state.set_chat_history(&fwd_task_id, messages).await;
                            }
                        }
                        _ => {
                            fwd_state
                                .send_and_persist(&fwd_task_id, &tx, chunk)
                                .await;
                        }
                    }
                }
            }
        });

        // Store handle in appropriate slot
        if agent_type == "system" {
            *state.system_agent.write().await = Some(handle.clone());
            tracing::info!(
                container_id = %container_id,
                "Reconnected to system agent container"
            );
        } else {
            state
                .task_containers
                .write()
                .await
                .insert(task_id.clone(), handle.clone());
            tracing::info!(
                container_id = %container_id,
                task_id = %task_id_str,
                "Reconnected to task container"
            );
        }

        // Update heartbeat
        let _ = state
            .pg_store
            .touch_persistent_container(&container_id)
            .await;
    }
}
