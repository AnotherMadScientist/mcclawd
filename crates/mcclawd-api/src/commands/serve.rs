use axum::extract::DefaultBodyLimit;
use axum::http::{self, HeaderValue};
use std::fs;
use std::process;
use std::sync::Arc;

use crate::sandbox::{ImageBuilder, SandboxOrchestrator};
use crate::server::{routes, state::AppState};
use crate::supervisor::AgentSupervisor;
use mcclawd_core::skills::SandboxConfig;
use mcclawd_core::McclawdConfig;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

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

pub async fn execute(port: u16) -> anyhow::Result<()> {
    let config_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".mcclawd")
        .join("config.toml");
    let config = McclawdConfig::load(&config_path)?;

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

    let state = AppState::new(config, supervisor);

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

    let result = axum::serve(listener, app).await;

    remove_pid_file();
    result?;
    Ok(())
}
