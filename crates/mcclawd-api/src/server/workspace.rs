use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use mcclawd_agent::workspace::{builtin_profiles, WorkspaceLoader};
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
/// If the workspace directory or the *specific requested file* doesn't exist,
/// auto-scaffolds with rich OpenClaw-compatible defaults from
/// WorkspaceLoader::scaffold().  Existing non-empty files are **never**
/// overwritten — scaffold only fills in missing/empty files.
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

    // 1. If the file exists on disk and has content, return it immediately.
    //    Do NOT scaffold — the user's saved content must be preserved.
    if let Ok(content) = tokio::fs::read_to_string(&file_path).await {
        if !content.trim().is_empty() {
            return Ok(Json(WorkspaceFile {
                name: file,
                content: Some(content),
            }));
        }
    }

    // 2. File is missing or empty — scaffold defaults for any missing files,
    //    then re-read the requested file.
    let ws_name = config.agent.default_workspace.clone();
    let loader = WorkspaceLoader::new(config.data_dir.clone());
    if let Err(e) = loader.scaffold(&ws_name) {
        tracing::warn!("Failed to scaffold workspace '{}': {e}", ws_name);
    }

    let content = tokio::fs::read_to_string(&file_path)
        .await
        .unwrap_or_default();

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

// ---------------------------------------------------------------------------
// Profile endpoints
// ---------------------------------------------------------------------------

/// GET /api/workspace/profiles — list available profiles (built-in + custom)
pub async fn list_profiles(State(state): State<AppState>) -> Json<Vec<serde_json::Value>> {
    let mut profiles: Vec<serde_json::Value> = builtin_profiles()
        .iter()
        .map(|p| {
            serde_json::json!({
                "name": p.name,
                "description": p.description,
                "builtin": true,
            })
        })
        .collect();

    // Load custom profiles from DB
    if let Ok(custom) = state.pg_store.list_workspace_profiles("admin").await {
        for (name, desc) in custom {
            profiles.push(serde_json::json!({
                "name": name,
                "description": desc,
                "builtin": false,
            }));
        }
    }

    Json(profiles)
}

/// POST /api/workspace/profiles/{name}/apply — overwrite workspace with profile content
pub async fn apply_profile(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    // Check built-in profiles first
    if let Some(profile) = builtin_profiles().into_iter().find(|p| p.name == name) {
        let files = [
            ("SOUL.md", profile.soul),
            ("AGENTS.md", profile.agents),
            ("USER.md", profile.user),
            ("IDENTITY.md", profile.identity),
            ("TOOLS.md", profile.tools),
            ("HEARTBEAT.md", profile.heartbeat),
        ];
        for (filename, content) in &files {
            if let Err(e) = state
                .pg_store
                .save_workspace_file("admin", "default", filename, content)
                .await
            {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("{e}")})),
                )
                    .into_response();
            }
        }
        // Also write to disk
        let config = state.config.read().await;
        let workspace_dir = config.data_dir.join(&config.agent.default_workspace);
        drop(config);
        let _ = tokio::fs::create_dir_all(&workspace_dir).await;
        for (filename, content) in &files {
            let _ = tokio::fs::write(workspace_dir.join(filename), content).await;
        }
        return (
            StatusCode::OK,
            Json(serde_json::json!({"applied": name})),
        )
            .into_response();
    }

    // Check custom profiles in DB
    if let Ok(Some(files)) = state.pg_store.load_workspace_profile("admin", &name).await {
        for (filename, content) in &files {
            if let Err(e) = state
                .pg_store
                .save_workspace_file("admin", "default", filename, content)
                .await
            {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("{e}")})),
                )
                    .into_response();
            }
        }
        // Also write to disk
        let config = state.config.read().await;
        let workspace_dir = config.data_dir.join(&config.agent.default_workspace);
        drop(config);
        let _ = tokio::fs::create_dir_all(&workspace_dir).await;
        for (filename, content) in &files {
            let _ = tokio::fs::write(workspace_dir.join(filename), content).await;
        }
        return (
            StatusCode::OK,
            Json(serde_json::json!({"applied": name})),
        )
            .into_response();
    }

    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "Profile not found"})),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
pub struct SaveProfileRequest {
    #[serde(default)]
    pub description: String,
}

/// POST /api/workspace/profiles/{name}/save — save current workspace as custom profile
pub async fn save_profile(
    State(state): State<AppState>,
    Path(name): Path<String>,
    body: Option<Json<SaveProfileRequest>>,
) -> impl IntoResponse {
    // Prevent overwriting built-in profiles
    if builtin_profiles().iter().any(|p| p.name == name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Cannot overwrite built-in profile"})),
        )
            .into_response();
    }

    let description = body.map(|b| b.description.clone()).unwrap_or_default();

    // Read current workspace files from DB
    let ws_files = [
        "SOUL.md",
        "AGENTS.md",
        "USER.md",
        "IDENTITY.md",
        "TOOLS.md",
        "HEARTBEAT.md",
    ];
    let mut profile_data = Vec::new();
    for filename in &ws_files {
        let content = state
            .pg_store
            .get_workspace_file("admin", "default", filename)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        profile_data.push((filename.to_string(), content));
    }

    if let Err(e) = state
        .pg_store
        .save_workspace_profile("admin", &name, &description, &profile_data)
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        )
            .into_response();
    }

    (
        StatusCode::CREATED,
        Json(serde_json::json!({"saved": name})),
    )
        .into_response()
}

/// DELETE /api/workspace/profiles/{name} — delete a custom profile
pub async fn delete_profile(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if builtin_profiles().iter().any(|p| p.name == name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Cannot delete built-in profile"})),
        )
            .into_response();
    }

    match state
        .pg_store
        .delete_workspace_profile("admin", &name)
        .await
    {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({"deleted": name})),
        )
            .into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Profile not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        )
            .into_response(),
    }
}
