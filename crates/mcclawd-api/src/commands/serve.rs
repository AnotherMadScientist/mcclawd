use axum::extract::DefaultBodyLimit;
use axum::http::{self, HeaderValue};
use std::fs;
use std::process;
use std::sync::Arc;

use crate::sandbox::{ImageBuilder, SandboxOrchestrator};
use crate::server::pg_store::PgTaskStore;
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
fn row_to_status(status: &str, error_message: Option<&str>) -> TaskStatus {
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
        .layer(DefaultBodyLimit::max(1024 * 1024)); // 1MB

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;

    write_pid_file()?;
    tracing::info!(
        "McClawd daemon PID {} listening on 127.0.0.1:{port}",
        process::id()
    );

    let server = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal());
    let result = server.await;

    remove_pid_file();
    tracing::info!("McClawd daemon shut down cleanly");
    result?;
    Ok(())
}
