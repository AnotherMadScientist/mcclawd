use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use mcclawd_agent::workspace::WorkspaceLoader;
use serde::{Deserialize, Serialize};

use super::state::AppState;

#[derive(Debug, Serialize)]
pub struct WorkspaceFile {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WriteFileRequest {
    pub content: String,
}

const WORKSPACE_FILES: &[&str] = &[
    "SOUL.md",
    "AGENTS.md",
    "USER.md",
    "IDENTITY.md",
    "TOOLS.md",
    "HEARTBEAT.md",
];

/// GET /api/workspace — list workspace files
pub async fn list_files() -> Json<Vec<WorkspaceFile>> {
    let files: Vec<WorkspaceFile> = WORKSPACE_FILES
        .iter()
        .map(|name| WorkspaceFile {
            name: name.to_string(),
            content: None,
        })
        .collect();
    Json(files)
}

/// GET /api/workspace/{file} — read a workspace file
///
/// If the workspace directory or file doesn't exist, auto-scaffolds with rich
/// OpenClaw-compatible defaults from WorkspaceLoader::scaffold().
pub async fn get_file(
    State(state): State<AppState>,
    Path(file): Path<String>,
) -> Result<Json<WorkspaceFile>, StatusCode> {
    // Reject path traversal attempts
    if file.contains("..") || file.contains('/') || file.contains('\\') {
        return Err(StatusCode::BAD_REQUEST);
    }

    let config = state.config.read().await;
    let workspace_dir = config.data_dir.join(&config.agent.default_workspace);
    let file_path = workspace_dir.join(&file);

    // Try reading the file; if it doesn't exist or is empty, scaffold the workspace
    let content = match tokio::fs::read_to_string(&file_path).await {
        Ok(content) if !content.trim().is_empty() => content,
        _ => {
            // Auto-scaffold: create workspace with rich defaults
            let ws_name = config.agent.default_workspace.clone();
            let loader = WorkspaceLoader::new(config.data_dir.clone());
            if let Err(e) = loader.scaffold(&ws_name) {
                tracing::warn!("Failed to scaffold workspace '{}': {e}", ws_name);
            }
            // Re-read after scaffold
            tokio::fs::read_to_string(&file_path)
                .await
                .unwrap_or_default()
        }
    };

    Ok(Json(WorkspaceFile {
        name: file,
        content: Some(content),
    }))
}

/// PUT /api/workspace/{file} — write a workspace file
pub async fn put_file(
    State(state): State<AppState>,
    Path(file): Path<String>,
    Json(body): Json<WriteFileRequest>,
) -> StatusCode {
    // Reject path traversal attempts
    if file.contains("..") || file.contains('/') || file.contains('\\') {
        return StatusCode::BAD_REQUEST;
    }

    let config = state.config.read().await;
    let workspace_dir = config.data_dir.join(&config.agent.default_workspace);

    if let Err(e) = tokio::fs::create_dir_all(&workspace_dir).await {
        tracing::error!("Failed to create workspace dir: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }

    let file_path = workspace_dir.join(&file);
    match tokio::fs::write(&file_path, body.content.as_bytes()).await {
        Ok(_) => {
            // Fire-and-forget: persist to Postgres
            let store = state.pg_store.clone();
            let filename = file.clone();
            let content = body.content.clone();
            tokio::spawn(async move {
                if let Err(e) = store.save_workspace_file("admin", "default", &filename, &content).await {
                    tracing::warn!("Failed to persist workspace file to DB: {e}");
                }
            });
            StatusCode::OK
        }
        Err(e) => {
            tracing::error!("Failed to write workspace file: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}
