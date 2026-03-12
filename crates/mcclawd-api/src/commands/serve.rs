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
use mcclawd_core::hooks::{
    AuditHook, DlpHook, HookPipeline, PgAuditSink, SecretScannerHook, SecuritySidecarHook,
};
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
         Set database_url in ~/.mcclawd/mcclawd.json or run: docker compose up -d postgres",
        last_err.unwrap_or_default()
    ))
}

pub async fn execute(port: u16) -> anyhow::Result<()> {
    kill_stale_server(port);

    let config_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".mcclawd")
        .join("mcclawd.json");
    let config = McclawdConfig::load(&config_path)?;

    // PostgreSQL is a required dependency — fail loudly if unavailable.
    // Priority: DATABASE_URL env var > mcclawd.json > constructed from POSTGRES_* env vars.
    // In Docker Compose, set DATABASE_URL=postgresql://user:pass@postgres:5432/mcclawd
    // to use the service name on the internal network.
    let database_url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| config.database_url.clone())
        .unwrap_or_else(|| {
            let user = std::env::var("POSTGRES_USER").unwrap_or_else(|_| "mcclawd".into());
            let pass = std::env::var("POSTGRES_PASSWORD").unwrap_or_else(|_| "mcclawd".into());
            let host = std::env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".into());
            let port = std::env::var("POSTGRES_PORT").unwrap_or_else(|_| "5432".into());
            let db = std::env::var("POSTGRES_DB").unwrap_or_else(|_| "mcclawd".into());
            format!("postgresql://{user}:{pass}@{host}:{port}/{db}")
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

    // Extract vault paths BEFORE config is consumed by AppState::new (avoids RwLock deadlock)
    let vault_data_dir = config.data_dir.clone();
    let vault_secrets_path = config.secrets_path();

    let mut state = AppState::new(config, supervisor, pg_store.clone())?;

    // Build security hook pipeline (DLP → secret scanner → sidecar → audit)
    // All hooks share a single SecurityContext so findings from DLP/secret-scanner
    // are visible to AuditHook when it persists the security_event + dlp_findings rows.
    {
        let sidecar_url = std::env::var("SECURITY_SIDECAR_URL")
            .unwrap_or_else(|_| "http://localhost:8082".to_string());
        let pipeline = HookPipeline::new();
        // Clone the shared context reference before consuming `pipeline` via builder.
        let shared_ctx = pipeline.context.clone();
        let pipeline = pipeline
            .add(Arc::new(
                DlpHook::with_defaults().with_context(shared_ctx.clone()),
            ))
            .add(Arc::new(
                SecretScannerHook::with_defaults().with_context(shared_ctx.clone()),
            ))
            .add(Arc::new(SecuritySidecarHook::new(&sidecar_url)))
            .add(Arc::new(
                AuditHook::new(Arc::new(
                    PgAuditSink::new(pg_store.pool().clone())
                        .with_context(shared_ctx.clone()),
                ))
                .with_context(shared_ctx),
            ));
        let hook_count = pipeline.len();
        state.security_pipeline = Arc::new(pipeline);
        tracing::info!(hooks = hook_count, sidecar_url = %sidecar_url, "Security pipeline initialized");
    }

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
    // If vault.key is missing, create it. If secrets.enc is missing, create empty.
    // If secrets.enc is CORRUPT: log error, start without vault. Human must run
    // `mc secrets reset -y && mc secrets init` to recover. NEVER auto-delete secrets.
    // Seeds ALL keys from .env file (not just a hardcoded subset).
    {
        let (data_dir, secrets_path) = (vault_data_dir, vault_secrets_path);
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
                            Some(b)
                        }
                        Err(e) => {
                            // Vault is corrupted or key mismatch — DO NOT auto-delete.
                            // Require explicit human action: `mc secrets reset -y` to wipe.
                            tracing::error!(
                                "VAULT CORRUPTED: {e}. \
                                 secrets.enc cannot be decrypted with current vault.key. \
                                 Run `mc secrets reset -y && mc secrets init` to reset, \
                                 or restore secrets.enc from backup. \
                                 Server will start WITHOUT secrets."
                            );
                            None
                        }
                    }
                } else {
                    tracing::info!("Creating new secrets vault");
                    Some(
                        EncryptedFileBackend::new_empty(&secrets_path, &passphrase)
                            .map_err(|e| anyhow::anyhow!("Failed to create vault: {e}"))?,
                    )
                };

                if let Some(backend) = backend {
                    // No auto-seeding from .env on startup — secrets are only imported
                    // via explicit `mc secrets init` command. This prevents accidental
                    // overwrites and keeps the vault under human control.
                    let mut secrets = state.secrets.write().await;
                    *secrets = Some(Arc::new(backend));
                }
            }
            Err(e) => {
                tracing::error!("vault.key unreadable: {e}");
            }
        }
    }

    // Hydrate in-memory TaskManager from postgres on startup.
    // Running/Building tasks keep their status — the reconciliation loop will
    // attempt to reconnect or restart their containers using persisted config.
    // Tasks whose containers are truly gone get failed in reconcile_containers_and_tasks().
    match pg_store.list_tasks().await {
        Ok(rows) => {
            let mut mgr = state.tasks.write().await;
            for (id, prompt, status, error_message, tags, selected_skills, allowed_tools, tool_profile, skill_context) in rows {
                let task_status = row_to_status(&status, error_message.as_deref());
                mgr.hydrate_task(
                    mcclawd_core::types::TaskId(id),
                    prompt,
                    task_status,
                    tags,
                    selected_skills,
                    allowed_tools,
                    tool_profile,
                    skill_context,
                );
            }
            let count = mgr.all_tasks().len();
            if count > 0 {
                tracing::info!(count, "Hydrated tasks from database");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to hydrate tasks from DB");
        }
    }

    // Clean up orphaned security events (from previously deleted tasks)
    match pg_store.cleanup_orphaned_security_events().await {
        Ok(0) => {}
        Ok(n) => tracing::info!(removed = n, "Cleaned up orphaned security events"),
        Err(e) => tracing::warn!(error = %e, "Failed to clean up orphaned security events"),
    }

    // Clean up security events without findings (noise from allowed events)
    match pg_store.cleanup_events_without_findings().await {
        Ok(0) => {}
        Ok(n) => tracing::info!(deleted = n, "Cleaned up security events without findings"),
        Err(e) => tracing::warn!(error = %e, "Failed to cleanup events without findings"),
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

    // Hydrate config from Postgres (DB wins over file config).
    // The "main" key stores the full McclawdConfig as a JSON blob.
    match pg_store.get_config_key("admin", "main").await {
        Ok(Some(value)) => {
            match serde_json::from_value::<McclawdConfig>(value) {
                Ok(db_config) => {
                    let mut cfg = state.config.write().await;
                    cfg.agent.model = db_config.agent.model;
                    cfg.agent.max_turns = db_config.agent.max_turns;
                    cfg.agent.default_workspace = db_config.agent.default_workspace;
                    tracing::info!("Config hydrated from database (DB wins over file)");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to deserialize config from DB, using file config");
                }
            }
        }
        Ok(None) => {
            tracing::debug!("No config in DB yet, using file config");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load config from DB, using file config");
        }
    }

    state.config_path = Some(config_path);

    // Hydrate workspace: restore the active profile from DB on startup.
    // If an active profile was previously persisted, apply its files to disk
    // so the workspace is consistent with the last user selection.
    match pg_store.get_config_key("admin", "active_profile").await {
        Ok(Some(value)) => {
            let profile_name = value.as_str().unwrap_or("default").to_string();
            tracing::info!(profile = %profile_name, "Restoring active workspace profile from DB");

            // Load profile files (builtin or custom) and write to disk
            let config = state.config.read().await;
            let workspace_dir = config.data_dir.join(&config.agent.default_workspace);
            drop(config);
            let _ = tokio::fs::create_dir_all(&workspace_dir).await;

            // Try builtin profiles first
            let builtin = mcclawd_agent::workspace::builtin_profiles();
            if let Some(profile) = builtin.into_iter().find(|p| p.name == profile_name) {
                let files = [
                    ("SOUL.md", profile.soul),
                    ("AGENTS.md", profile.agents),
                    ("USER.md", profile.user),
                    ("IDENTITY.md", profile.identity),
                    ("TOOLS.md", profile.tools),
                    ("HEARTBEAT.md", profile.heartbeat),
                ];
                for (filename, content) in &files {
                    let _ = tokio::fs::write(workspace_dir.join(filename), content).await;
                }
                tracing::info!(profile = %profile_name, "Active workspace profile restored (builtin)");
            } else if let Ok(Some(files)) = pg_store.load_workspace_profile("admin", &profile_name).await {
                for (filename, content) in &files {
                    let _ = tokio::fs::write(workspace_dir.join(filename), content).await;
                }
                tracing::info!(profile = %profile_name, "Active workspace profile restored (custom)");
            } else {
                tracing::debug!(profile = %profile_name, "Active profile not found, using existing workspace files");
            }
        }
        Ok(None) => {
            // No active profile set yet — seed the default and persist it
            if let Err(e) = pg_store
                .save_config(
                    "admin",
                    "active_profile",
                    &serde_json::Value::String("default".to_string()),
                )
                .await
            {
                tracing::warn!(error = %e, "Failed to seed default active_profile in DB");
            } else {
                tracing::info!("Seeded active_profile = 'default' in DB");
            }
            // Also write default profile files to disk so workspace is immediately consistent
            let builtin = mcclawd_agent::workspace::builtin_profiles();
            if let Some(profile) = builtin.into_iter().find(|p| p.name == "default") {
                let config = state.config.read().await;
                let workspace_dir = config.data_dir.join(&config.agent.default_workspace);
                drop(config);
                let _ = tokio::fs::create_dir_all(&workspace_dir).await;
                let files = [
                    ("SOUL.md", profile.soul),
                    ("AGENTS.md", profile.agents),
                    ("USER.md", profile.user),
                    ("IDENTITY.md", profile.identity),
                    ("TOOLS.md", profile.tools),
                    ("HEARTBEAT.md", profile.heartbeat),
                ];
                for (filename, content) in &files {
                    let _ = tokio::fs::write(workspace_dir.join(filename), content).await;
                }
                tracing::info!("Default profile files written to disk (first-time setup)");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to read active_profile from DB");
        }
    }

    // ── Task↔Container 1:1 Enforcement ────────────────────────────────
    // Phase A: Reconcile Docker containers with postgres task records.
    //   - Containers WITHOUT matching active tasks → orphans → remove
    //   - DB container records for non-existent Docker containers → stale → remove
    //   - Running containers WITH matching tasks → reconnect handles
    // Phase B: After reconciliation, check tasks without containers → mark failed
    {
        let reconcile_state = state.clone();
        let reconcile_pg = pg_store.clone();
        tokio::spawn(async move {
            reconcile_containers_and_tasks(reconcile_state, reconcile_pg).await;
        });
        tracing::info!("Container↔task reconciliation started in background");
    }

    // Spawn periodic GC: every 30s, cross-reference Docker containers with tasks (DB-primary)
    // and clean up orphans. Safety net for the Docker event listener.
    {
        let gc_state = state.clone();
        tokio::spawn(async move {
            container_gc_loop(gc_state).await;
        });
        tracing::info!("Container GC loop started (30s interval)");
    }

    // Spawn Docker event listener for real-time container death detection.
    {
        let event_state = state.clone();
        tokio::spawn(async move {
            docker_event_listener(event_state).await;
        });
        tracing::info!("Docker event listener started");
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

            // Ensure the __system__ task exists in PG so FK constraints on
            // task_events / task_chat_history don't fail.
            if let Err(e) = sys_state
                .pg_store
                .save_task(SYSTEM_AGENT_TASK_ID, "System agent", "Running", None, "system", &[])
                .await
            {
                tracing::warn!(error = %e, "Failed to upsert __system__ task row");
            }

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

/// Full startup reconciliation: enforces 1:1 between tasks and containers.
///
/// Direction 1 (Container→Task): Find Docker containers with no matching active task → cleanup.
/// Direction 2 (DB→Docker): Find DB container records with no live Docker container → cleanup.
/// Direction 3 (Task→Container): Find active tasks without containers → mark failed.
/// Finally: Reconnect valid containers to their task handles.
async fn reconcile_containers_and_tasks(state: AppState, pg_store: PgTaskStore) {
    let docker = match bollard::Docker::connect_with_local_defaults() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(error = %e, "Cannot reconcile — Docker unavailable");
            return;
        }
    };

    // 1. List ALL mcclawd Docker containers (running + exited)
    let mut filters = std::collections::HashMap::new();
    filters.insert("name".to_string(), vec!["mcclawd-persistent-".to_string()]);
    let docker_containers = match docker
        .list_containers(Some(bollard::container::ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        }))
        .await
    {
        Ok(cs) => cs,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to list Docker containers for reconciliation");
            return;
        }
    };

    // 2. Load active tasks from postgres
    let active_task_ids: std::collections::HashSet<String> = match pg_store.list_tasks().await {
        Ok(rows) => rows.into_iter().map(|(id, _, _, _, _, _, _, _, _)| id).collect(),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load tasks for reconciliation");
            return;
        }
    };

    // 3. Load DB container records
    let db_containers = match pg_store.load_persistent_containers().await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load container records for reconciliation");
            return;
        }
    };

    let db_container_ids: std::collections::HashSet<String> =
        db_containers.iter().map(|(cid, _, _, _)| cid.clone()).collect();

    let mut orphans_removed = 0u32;
    let mut stale_db_removed = 0u32;
    let mut reconnected = 0u32;
    let mut tasks_failed = 0u32;

    // ── Direction 1: Docker containers without matching active tasks → orphans
    for container in &docker_containers {
        let cid = container.id.as_deref().unwrap_or_default();
        if cid.is_empty() {
            continue;
        }

        // Extract task_id from DB records first
        let mut container_task_id = db_containers
            .iter()
            .find(|(db_cid, _, _, _)| db_cid == cid || cid.starts_with(db_cid.as_str()))
            .map(|(_, tid, _, _)| tid.clone());

        // If not found in DB, try Docker labels (covers containers not in persistent_containers)
        if container_task_id.is_none() {
            container_task_id = docker
                .inspect_container(cid, None)
                .await
                .ok()
                .and_then(|info| {
                    info.config
                        .as_ref()
                        .and_then(|c| c.labels.as_ref())
                        .and_then(|l| l.get("mcclawd.task_id").cloned())
                });
        }

        // Skip system agent containers
        if container_task_id.as_deref() == Some("system-agent")
            || container_task_id.as_deref() == Some("__system__")
        {
            continue;
        }

        let has_active_task = container_task_id
            .as_ref()
            .map(|tid| active_task_ids.contains(tid))
            .unwrap_or(false);

        if !has_active_task {
            // Orphan container — no matching active task
            tracing::info!(
                container_id = %cid,
                task_id = ?container_task_id,
                "Orphan container found (no active task) — removing"
            );
            // Stop + remove
            let _ = docker
                .stop_container(cid, Some(bollard::container::StopContainerOptions { t: 5 }))
                .await;
            let _ = docker
                .remove_container(
                    cid,
                    Some(bollard::container::RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await;
            // Clean DB record
            let _ = pg_store.delete_persistent_container(cid).await;
            orphans_removed += 1;
        }
    }

    // ── Direction 2: DB records for containers not in Docker → stale
    let docker_ids: std::collections::HashSet<String> = docker_containers
        .iter()
        .filter_map(|c| c.id.clone())
        .collect();

    for (db_cid, _, _, _) in &db_containers {
        if !docker_ids.iter().any(|did| did == db_cid || did.starts_with(db_cid.as_str())) {
            tracing::info!(container_id = %db_cid, "Stale DB container record (Docker container gone) — removing");
            let _ = pg_store.delete_persistent_container(db_cid).await;
            stale_db_removed += 1;
        }
    }

    // ── Direction 3: Reconnect valid containers + fail orphan tasks
    // Reload DB containers after cleanup
    let valid_containers = match pg_store.load_persistent_containers().await {
        Ok(rows) => rows,
        Err(_) => Vec::new(),
    };

    if !valid_containers.is_empty() {
        reconnect_persistent_containers(state.clone(), valid_containers).await;
        reconnected = state.task_containers.read().await.len() as u32;
        if state.system_agent.read().await.is_some() {
            reconnected += 1;
        }
    }

    // ── Direction 4: Resilient restart for Running/Building/Pending tasks without containers.
    // Because we now persist allowed_tools, selected_skills, skill_context, and tool_profile
    // in the DB, we can reconstruct the full AgentEnvironment without re-resolving skills.
    {
        let tasks_to_restart: Vec<(TaskId, String)> = {
            let mgr = state.tasks.read().await;
            let containers = state.task_containers.read().await;
            mgr.all_tasks()
                .iter()
                .filter(|t| {
                    t.id.0 != "system-agent"
                        && t.id.0 != "__system__"
                        && (matches!(t.status, TaskStatus::Running)
                            || matches!(t.status, TaskStatus::Building)
                            || matches!(t.status, TaskStatus::Pending))
                        && !containers.contains_key(&t.id)
                })
                .map(|t| (t.id.clone(), t.prompt.clone()))
                .collect()
        };

        for (tid, _prompt) in &tasks_to_restart {
            // Try to restart by creating a new container with persisted config
            match pg_store.get_task_tools(&tid.0).await {
                Ok(Some((_skills, allowed_tools, tool_profile, skill_context))) => {
                    // We have persisted config — attempt container restart
                    let config = state.config.read().await;
                    let agent_env = crate::sandbox::container::AgentEnvironment {
                        image: "mcclawd-runner:latest".to_string(),
                        network: config.sandbox.network.clone(),
                        gateway_url: crate::sandbox::container::container_gateway_url(
                            &config.mcp.agentgateway_url,
                        ),
                        allowed_tools,
                        skill_context,
                        model: config.agent.model.clone(),
                    };

                    if let Ok(orch) = crate::sandbox::SandboxOrchestrator::new() {
                        let sandbox_cfg = SandboxConfig {
                            workspace_dir: config.workspaces_dir().to_string_lossy().to_string(),
                            agentgateway_url: config.mcp.agentgateway_url.clone(),
                            memory_limit: config.sandbox.memory_limit,
                            cpu_limit: config.sandbox.cpu_limit,
                            network: config.sandbox.network.clone(),
                            pids_limit: config.sandbox.pids_limit,
                            ..Default::default()
                        };
                        match orch
                            .create_persistent_runner_container(
                                tid,
                                &agent_env,
                                &sandbox_cfg.workspace_dir,
                                &sandbox_cfg,
                                &std::collections::HashMap::new(),
                                25, // default max_turns
                                None, // agent_type
                                None, // attachments_dir
                            )
                            .await
                        {
                            Ok(handle) => {
                                tracing::info!(
                                    task_id = %tid.0,
                                    container_id = %handle.container_id,
                                    tool_profile = ?tool_profile,
                                    "Restarted container for task after server restart"
                                );
                                // Save persistent container record
                                let _ = pg_store
                                    .save_persistent_container(
                                        &handle.container_id,
                                        &tid.0,
                                        "task",
                                        &sandbox_cfg.workspace_dir,
                                    )
                                    .await;
                                // Store handle
                                let mut containers = state.task_containers.write().await;
                                containers.insert(tid.clone(), handle);
                                reconnected += 1;
                                continue; // success — skip failure path
                            }
                            Err(e) => {
                                tracing::warn!(
                                    task_id = %tid.0,
                                    error = %e,
                                    "Failed to restart container for task — marking as failed"
                                );
                            }
                        }
                    }
                }
                Ok(None) => {
                    tracing::info!(
                        task_id = %tid.0,
                        "No persisted tool config for task — cannot restart, marking as failed"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        task_id = %tid.0,
                        error = %e,
                        "Failed to load task tools from DB — marking as failed"
                    );
                }
            }

            // If we reach here, restart failed — mark as failed
            tasks_failed += 1;
            let mut mgr = state.tasks.write().await;
            mgr.fail_task(tid, "Container lost after server restart".to_string());
            state
                .pg_update_status(tid, "Failed", Some("Container lost after server restart"))
                .await;
        }
    }

    tracing::info!(
        orphans_removed,
        stale_db_removed,
        reconnected,
        tasks_failed,
        "Container↔task reconciliation complete"
    );
}

/// Periodic garbage collection: every 30s, scan for orphan containers.
/// Acts as a safety net — Docker event listener handles real-time detection.
async fn container_gc_loop(state: AppState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
    interval.tick().await; // skip first immediate tick (startup reconciliation handles it)

    loop {
        interval.tick().await;

        let docker = match bollard::Docker::connect_with_local_defaults() {
            Ok(d) => d,
            Err(_) => continue,
        };

        // List mcclawd containers
        let mut filters = std::collections::HashMap::new();
        filters.insert("name".to_string(), vec!["mcclawd-persistent-".to_string()]);
        let containers = match docker
            .list_containers(Some(bollard::container::ListContainersOptions {
                all: true,
                filters,
                ..Default::default()
            }))
            .await
        {
            Ok(cs) => cs,
            Err(_) => continue,
        };

        if containers.is_empty() {
            continue;
        }

        // Phase A: Container → DB check (use DB as source of truth, not in-memory)
        let db_containers = state.pg_store.load_persistent_containers().await.unwrap_or_default();
        let known_container_ids: std::collections::HashSet<String> =
            db_containers.iter().map(|(cid, _, _, _)| cid.clone()).collect();
        let system_cid = state
            .system_agent
            .read()
            .await
            .as_ref()
            .map(|h| h.container_id.clone());

        let mut gc_count = 0u32;
        for container in &containers {
            let cid = container.id.as_deref().unwrap_or_default();
            if cid.is_empty() {
                continue;
            }

            // Skip if it's a known container in DB or system agent
            if known_container_ids.contains(cid)
                || system_cid.as_deref() == Some(cid)
            {
                continue;
            }

            // Check container state to decide cleanup strategy
            let state_str = container
                .state
                .as_deref()
                .unwrap_or("unknown")
                .to_lowercase();
            if state_str == "exited" || state_str == "dead" {
                // Exited/dead orphans: remove immediately
                tracing::info!(
                    container_id = %cid,
                    state = %state_str,
                    "GC: removing orphan container"
                );

                // Look up task_id from container labels before removing
                let orphan_task_id = docker
                    .inspect_container(cid, None)
                    .await
                    .ok()
                    .and_then(|info| {
                        info.config
                            .as_ref()
                            .and_then(|c| c.labels.as_ref())
                            .and_then(|l| l.get("mcclawd.task_id").cloned())
                    });

                let _ = docker
                    .remove_container(
                        cid,
                        Some(bollard::container::RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        }),
                    )
                    .await;
                let _ = state.pg_store.delete_persistent_container(cid).await;

                // Cascade: also delete the associated task from PG + memory
                if let Some(tid) = orphan_task_id {
                    if tid != "system-agent" && tid != "__system__" {
                        let task_id_typed = TaskId(tid.clone());
                        // pg_delete_task_sync handles full cascade (security_events + persistent_containers + task row)
                        state.pg_delete_task_sync(&task_id_typed).await;
                        state.task_containers.write().await.remove(&task_id_typed);
                        state.task_streams.write().await.remove(&task_id_typed);
                        state.task_chat_history.write().await.remove(&task_id_typed);
                        state.task_events.write().await.remove(&task_id_typed);
                        let mut mgr = state.tasks.write().await;
                        mgr.delete_task(&task_id_typed);
                    }
                }
                gc_count += 1;
            } else if state_str == "running" {
                // Running orphans: only remove if container has been running >120s
                // without a matching DB record (grace period avoids killing mid-startup containers)
                let created = container.created.unwrap_or(0);
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                let age_secs = now - created;

                if age_secs > 120 {
                    // Inspect BEFORE stopping to get task_id label
                    let orphan_task_id = docker
                        .inspect_container(cid, None)
                        .await
                        .ok()
                        .and_then(|info| {
                            info.config
                                .as_ref()
                                .and_then(|c| c.labels.as_ref())
                                .and_then(|l| l.get("mcclawd.task_id").cloned())
                        });

                    tracing::warn!(
                        container_id = %cid,
                        task_id = ?orphan_task_id,
                        age_secs = age_secs,
                        "GC: stopping orphaned running container (no DB record, age > 120s)"
                    );

                    let _ = docker
                        .stop_container(
                            cid,
                            Some(bollard::container::StopContainerOptions { t: 5 }),
                        )
                        .await;
                    let _ = docker
                        .remove_container(
                            cid,
                            Some(bollard::container::RemoveContainerOptions {
                                force: true,
                                ..Default::default()
                            }),
                        )
                        .await;
                    let _ = state.pg_store.delete_persistent_container(cid).await;

                    // Cascade: also delete the associated task from PG + memory
                    if let Some(tid) = orphan_task_id {
                        if tid != "system-agent" && tid != "__system__" {
                            let task_id_typed = TaskId(tid.clone());
                            state.pg_delete_task_sync(&task_id_typed).await;
                            let _ = state
                                .pg_store
                                .delete_persistent_containers_by_task(&tid)
                                .await;
                            state.task_containers.write().await.remove(&task_id_typed);
                            state.task_streams.write().await.remove(&task_id_typed);
                            state
                                .task_chat_history
                                .write()
                                .await
                                .remove(&task_id_typed);
                            state.task_events.write().await.remove(&task_id_typed);
                            let mut mgr = state.tasks.write().await;
                            mgr.delete_task(&task_id_typed);
                        }
                    }
                    gc_count += 1;
                }
            }
        }

        // Phase B: Task → Container check (reverse direction)
        // Find tasks marked Running/Building in DB that have no live Docker container.
        // IMPORTANT: Reload db_containers after Phase A cleanup to avoid stale data
        // that could cause tasks to survive a GC cycle when their containers were just removed.
        let fresh_db_containers = state.pg_store.load_persistent_containers().await.unwrap_or_default();
        let live_docker_ids: std::collections::HashSet<String> = containers
            .iter()
            .filter_map(|c| c.id.clone())
            .collect();

        if let Ok(rows) = state.pg_store.list_tasks().await {
            for (task_id, _, status, _, _, _, _, _, _) in &rows {
                // Skip system agent
                if task_id == "system-agent" || task_id == "__system__" {
                    continue;
                }

                // Check if this task has a live container (using fresh DB data after Phase A cleanup)
                let has_container = fresh_db_containers.iter().any(|(cid, tid, _, _)| {
                    tid == task_id && live_docker_ids.contains(cid)
                });
                // Check if this task has any persistent_container record at all
                let has_db_container = fresh_db_containers.iter().any(|(_, tid, _, _)| tid == task_id);

                if status == "Running" || status == "Building" {
                    // Running/Building tasks with no live container → fail + delete
                    if !has_container {
                        tracing::info!(task_id, "GC: task has no live container — deleting");
                        let tid = TaskId(task_id.clone());
                        state.pg_delete_task_sync(&tid).await;
                        let _ = state
                            .pg_store
                            .delete_persistent_containers_by_task(task_id)
                            .await;
                        state.task_containers.write().await.remove(&tid);
                        state.task_streams.write().await.remove(&tid);
                        state.task_chat_history.write().await.remove(&tid);
                        state.task_events.write().await.remove(&tid);
                        let mut mgr = state.tasks.write().await;
                        mgr.delete_task(&tid);
                        drop(mgr);
                        gc_count += 1;
                    }
                } else if status == "Failed" || status == "Completed" {
                    // Failed/Completed tasks with no container record at all → fully orphaned, delete
                    // Completed tasks get a 1-hour retention period so users can review results
                    if !has_db_container && !has_container {
                        if status == "Completed" {
                            let is_old = state.pg_store.is_task_older_than(task_id, 1).await.unwrap_or(false);
                            if !is_old {
                                continue; // Keep recent completed tasks for review
                            }
                        }
                        tracing::info!(task_id, status, "GC: orphan task with no container — deleting");
                        let tid = TaskId(task_id.clone());
                        state.pg_delete_task_sync(&tid).await;
                        let _ = state
                            .pg_store
                            .delete_persistent_containers_by_task(task_id)
                            .await;
                        state.task_streams.write().await.remove(&tid);
                        state.task_chat_history.write().await.remove(&tid);
                        state.task_events.write().await.remove(&tid);
                        let mut mgr = state.tasks.write().await;
                        mgr.delete_task(&tid);
                        drop(mgr);
                        gc_count += 1;
                    }
                }
            }
        }

        // Phase C: Clean up orphaned persistent_containers DB records
        // (container_ids in DB that don't exist in Docker anymore)
        if let Ok(db_rows) = state.pg_store.load_persistent_containers().await {
            for (db_cid, db_task_id, _, _) in &db_rows {
                if db_task_id == "system-agent" || db_task_id == "__system__" {
                    continue;
                }
                // Check if this container actually exists in Docker
                let exists_in_docker = containers.iter().any(|c| {
                    c.id.as_deref() == Some(db_cid.as_str())
                });
                if !exists_in_docker {
                    tracing::info!(
                        container_id = %db_cid,
                        task_id = %db_task_id,
                        "GC: removing orphaned persistent_containers DB record (container not in Docker)"
                    );
                    let _ = state.pg_store.delete_persistent_container(db_cid).await;
                    gc_count += 1;
                }
            }
        }

        if gc_count > 0 {
            tracing::info!(removed = gc_count, "Container GC cycle complete");
        }

        // Phase D: Periodic Docker prune (every 10th cycle ≈ 5 min)
        // Cleans up stopped mcclawd containers and dangling images from builds
        static GC_CYCLE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let cycle = GC_CYCLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if cycle % 10 == 9 {
            // Prune stopped containers with mcclawd label
            let mut prune_filters = std::collections::HashMap::new();
            prune_filters.insert("label".to_string(), vec!["mcclawd.task_id".to_string()]);
            match docker
                .prune_containers(Some(bollard::container::PruneContainersOptions {
                    filters: prune_filters,
                }))
                .await
            {
                Ok(result) => {
                    let pruned = result.containers_deleted.as_ref().map_or(0, |v| v.len());
                    if pruned > 0 {
                        tracing::info!(pruned, "Docker prune: removed stopped mcclawd containers");
                    }
                }
                Err(e) => tracing::debug!(error = %e, "Docker container prune failed"),
            }

            // Prune dangling images (from mcclawd builder)
            let mut img_filters = std::collections::HashMap::new();
            img_filters.insert("dangling".to_string(), vec!["true".to_string()]);
            match docker
                .prune_images(Some(bollard::image::PruneImagesOptions {
                    filters: img_filters,
                }))
                .await
            {
                Ok(result) => {
                    let pruned = result.images_deleted.as_ref().map_or(0, |v| v.len());
                    if pruned > 0 {
                        let reclaimed = result.space_reclaimed.unwrap_or(0);
                        tracing::info!(pruned, reclaimed_mb = reclaimed / 1_048_576,
                            "Docker prune: removed dangling images");
                    }
                }
                Err(e) => tracing::debug!(error = %e, "Docker image prune failed"),
            }
        }
    }
}

/// Listen to Docker events for real-time container death detection.
/// When a container with `mcclawd.task_id` label dies/stops/is destroyed,
/// cascade-fail the associated task immediately instead of waiting for the GC loop.
async fn docker_event_listener(state: AppState) {
    use bollard::system::EventsOptions;
    use futures::StreamExt;

    let docker = match bollard::Docker::connect_with_local_defaults() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(error = %e, "Docker event listener: cannot connect — disabled");
            return;
        }
    };

    let mut filters = std::collections::HashMap::new();
    filters.insert("type".to_string(), vec!["container".to_string()]);
    filters.insert(
        "event".to_string(),
        vec!["die".to_string(), "stop".to_string(), "destroy".to_string()],
    );
    filters.insert(
        "label".to_string(),
        vec!["mcclawd.task_id".to_string()],
    );

    let mut stream = docker.events(Some(EventsOptions {
        filters,
        ..Default::default()
    }));

    while let Some(Ok(event)) = stream.next().await {
        let actor = match &event.actor {
            Some(a) => a,
            None => continue,
        };
        let task_id_str = actor
            .attributes
            .as_ref()
            .and_then(|a| a.get("mcclawd.task_id").cloned());
        let container_id = actor.id.clone().unwrap_or_default();

        if let Some(tid) = task_id_str {
            // Skip system agent — it has its own lifecycle
            if tid == "system-agent" || tid == "__system__" {
                continue;
            }

            tracing::info!(
                task_id = %tid,
                container_id = %container_id,
                event = ?event.action,
                "Docker event: container stopped/died"
            );

            let task_id_typed = TaskId(tid.clone());

            // Check current task status BEFORE cascading — only fail tasks that
            // are still Running/Building. Completed/Failed tasks should not be
            // overwritten when their container exits naturally.
            let should_fail = {
                let mgr = state.tasks.read().await;
                mgr.get_task(&task_id_typed)
                    .map(|t| {
                        matches!(
                            t.status,
                            TaskStatus::Running | TaskStatus::Building | TaskStatus::Pending
                        )
                    })
                    .unwrap_or(false)
            };

            // Always clean up the container handle and DB record
            state.task_containers.write().await.remove(&task_id_typed);
            let _ = state
                .pg_store
                .delete_persistent_containers_by_task(&tid)
                .await;

            if should_fail {
                // Task was still active — mark as failed in both DB and memory
                state
                    .pg_update_status_sync(&task_id_typed, "Failed", Some("Container stopped"))
                    .await;
                {
                    let mut mgr = state.tasks.write().await;
                    mgr.fail_task(&task_id_typed, "Container stopped".to_string());
                }
                // Clean in-memory caches for failed tasks
                state.task_streams.write().await.remove(&task_id_typed);
                state.task_chat_history.write().await.remove(&task_id_typed);
                state.task_events.write().await.remove(&task_id_typed);
            } else {
                tracing::debug!(
                    task_id = %tid,
                    "Container stopped but task already completed/failed — skipping status update"
                );
            }
        }
    }

    tracing::warn!("Docker event listener stream ended — no more real-time detection");
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
