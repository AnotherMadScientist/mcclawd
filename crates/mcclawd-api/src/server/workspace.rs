use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
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

const WORKSPACE_FILES: &[&str] = &["SOUL.md", "AGENTS.md", "USER.md"];

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
pub async fn get_file(
    State(state): State<AppState>,
    Path(file): Path<String>,
) -> Result<Json<WorkspaceFile>, StatusCode> {
    let config = state.config.read().await;
    let workspace_dir = config.data_dir.join(&config.agent.default_workspace);
    let file_path = workspace_dir.join(&file);

    match tokio::fs::read_to_string(&file_path).await {
        Ok(content) => Ok(Json(WorkspaceFile {
            name: file,
            content: Some(content),
        })),
        Err(_) => Ok(Json(WorkspaceFile {
            name: file,
            content: Some(String::new()),
        })),
    }
}

/// PUT /api/workspace/{file} — write a workspace file
pub async fn put_file(
    State(state): State<AppState>,
    Path(file): Path<String>,
    Json(body): Json<WriteFileRequest>,
) -> StatusCode {
    let config = state.config.read().await;
    let workspace_dir = config.data_dir.join(&config.agent.default_workspace);

    if let Err(e) = tokio::fs::create_dir_all(&workspace_dir).await {
        tracing::error!("Failed to create workspace dir: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }

    let file_path = workspace_dir.join(&file);
    match tokio::fs::write(&file_path, body.content.as_bytes()).await {
        Ok(_) => StatusCode::OK,
        Err(e) => {
            tracing::error!("Failed to write workspace file: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}
