use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::Serialize;
use std::path::PathBuf;

use super::state::AppState;

/// Secret names in McClawd vault that map to worldmonitor env vars.
const WORLDMONITOR_KEYS: &[&str] = &[
    "ACLED_API_KEY",
    "ACLED_EMAIL",
    "EIA_API_KEY",
    "FRED_API_KEY",
    "FINNHUB_API_KEY",
    "AVIATIONSTACK_API_KEY",
    "OPENSKY_USERNAME",
    "OPENSKY_PASSWORD",
    "AISSTREAM_API_KEY",
    "NASA_FIRMS_MAP_KEY",
    "CLOUDFLARE_RADAR_TOKEN",
    "GROQ_API_KEY",
    "OPENROUTER_API_KEY",
    "TELEGRAM_API_ID",
    "TELEGRAM_API_HASH",
    "TELEGRAM_SESSION",
];

#[derive(Debug, Serialize)]
pub struct SyncEnvResponse {
    pub synced: usize,
    pub keys: Vec<String>,
}

/// POST /api/worldmonitor/sync-env — write vault secrets to docker/worldmonitor/.env and restart container
pub async fn sync_env(State(state): State<AppState>) -> Result<Json<SyncEnvResponse>, StatusCode> {
    let guard = state.secrets.read().await;
    let backend = guard.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // Collect matching secrets from vault
    let mut env_lines = vec![
        "# Auto-generated from McClawd vault — do not edit manually".to_string(),
        "LOCAL_API_PORT=3000".to_string(),
        "LOCAL_API_MODE=container".to_string(),
        "LOCAL_API_CLOUD_FALLBACK=true".to_string(),
    ];
    let mut synced_keys = Vec::new();

    for key in WORLDMONITOR_KEYS {
        match backend.get(key).await {
            Ok(Some(value)) if !value.is_empty() => {
                env_lines.push(format!("{key}={value}"));
                synced_keys.push(key.to_string());
            }
            _ => {} // key not in vault — skip
        }
    }

    // Write .env file
    let env_path = worldmonitor_env_path();
    let content = env_lines.join("\n") + "\n";
    tokio::fs::write(&env_path, content).await.map_err(|e| {
        tracing::error!("Failed to write worldmonitor .env: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let synced = synced_keys.len();
    tracing::info!("Synced {synced} secrets to worldmonitor .env: {synced_keys:?}");

    // Restart container to pick up new env
    restart_container().await;

    Ok(Json(SyncEnvResponse {
        synced,
        keys: synced_keys,
    }))
}

/// GET /api/worldmonitor/status — check worldmonitor container health
pub async fn status() -> Result<Json<serde_json::Value>, StatusCode> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match client.get("http://localhost:3001/api/health").send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Ok(Json(serde_json::json!({
                "running": true,
                "status": status,
                "body": body,
            })))
        }
        Err(_) => Ok(Json(serde_json::json!({
            "running": false,
        }))),
    }
}

fn worldmonitor_env_path() -> PathBuf {
    // Resolve relative to workspace root (where docker-compose.yml lives)
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    cwd.join("docker/worldmonitor/.env")
}

async fn restart_container() {
    match tokio::process::Command::new("docker")
        .args(["compose", "restart", "worldmonitor"])
        .output()
        .await
    {
        Ok(output) => {
            if output.status.success() {
                tracing::info!("Restarted worldmonitor container");
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!("Failed to restart worldmonitor: {stderr}");
            }
        }
        Err(e) => tracing::warn!("Failed to run docker compose restart: {e}"),
    }
}
