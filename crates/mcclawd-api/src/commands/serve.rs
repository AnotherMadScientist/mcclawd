use axum::extract::DefaultBodyLimit;
use axum::http::{self, HeaderValue};
use std::fs;
use std::process;
use std::sync::Arc;

use crate::sandbox::{ImageBuilder, SandboxOrchestrator};
use crate::server::pg_store::PgTaskStore;
use crate::server::{routes, state::AppState};
use crate::supervisor::AgentSupervisor;
use mcclawd_core::secrets::EncryptedFileBackend;
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

pub async fn execute(port: u16) -> anyhow::Result<()> {
    kill_stale_server(port);

    let config_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".mcclawd")
        .join("config.toml");
    let config = McclawdConfig::load(&config_path)?;

    // Initialize postgres if database_url is configured (5s timeout — never hang)
    let pg_store = if let Some(ref database_url) = config.database_url {
        tracing::info!("Connecting to PostgreSQL...");
        let connect_result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(10)
                .connect(database_url),
        )
        .await;
        match connect_result {
            Ok(Ok(pool)) => {
                match sqlx::migrate!("../mcclawd-core/migrations").run(&pool).await {
                    Ok(_) => {
                        tracing::info!("PostgreSQL connected, migrations applied");
                        Some(PgTaskStore::new(pool))
                    }
                    Err(e) => {
                        tracing::warn!("PostgreSQL migration failed, running in-memory only: {e}");
                        None
                    }
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("PostgreSQL connection failed, running in-memory only: {e}");
                None
            }
            Err(_) => {
                tracing::warn!("PostgreSQL connection timed out after 5s, running in-memory only");
                None
            }
        }
    } else {
        tracing::info!("No database_url configured — running in-memory only");
        None
    };

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

    let mut state = AppState::new(config, supervisor)?;
    state.pg_store = pg_store.clone();

    // Auto-unlock vault if vault.key exists — server must never run with locked vault
    {
        let (data_dir, secrets_path) = {
            let c = state.config.read().await;
            (c.data_dir.clone(), c.secrets_path())
        };
        let vault_key_path = data_dir.join("vault.key");
        if vault_key_path.exists() {
            match tokio::fs::read(&vault_key_path).await {
                Ok(vault_key_bytes) => {
                    let passphrase: String =
                        vault_key_bytes.iter().map(|b| format!("{b:02x}")).collect();
                    let backend = match EncryptedFileBackend::new(&secrets_path, &passphrase) {
                        Ok(b) => {
                            tracing::info!("Vault auto-unlocked on startup");
                            b
                        }
                        Err(_) => {
                            // secrets.enc corrupted or mismatched — create fresh empty vault
                            match EncryptedFileBackend::new_empty(&secrets_path, &passphrase) {
                                Ok(b) => {
                                    tracing::warn!(
                                        "Created fresh vault on startup (old secrets.enc was unreadable)"
                                    );
                                    b
                                }
                                Err(e) => {
                                    tracing::error!("Failed to create vault on startup: {e}");
                                    return Err(e.into());
                                }
                            }
                        }
                    };
                    let mut secrets = state.secrets.write().await;
                    *secrets = Some(Arc::new(backend));
                }
                Err(e) => {
                    tracing::warn!("vault.key exists but could not be read: {e}");
                }
            }
        } else {
            tracing::info!("No vault.key found — vault locked until first WebAuthn registration");
        }
    }

    // Hydrate in-memory TaskManager from postgres on startup
    if let Some(ref store) = pg_store {
        match store.list_tasks().await {
            Ok(rows) => {
                let mut mgr = state.tasks.write().await;
                for (id, prompt, status, error_message) in &rows {
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
    }

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
