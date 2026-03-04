use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use mcclawd_core::types::TaskId;
use mcclawd_tasks::manager::{TaskRecord, TaskStatus};
use serde::{Deserialize, Serialize};

use super::state::AppState;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CreateTaskRequest {
    pub prompt: String,
    pub workspace: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TaskResponse {
    pub id: String,
    pub prompt: String,
    pub status: TaskStatus,
}

impl From<&TaskRecord> for TaskResponse {
    fn from(r: &TaskRecord) -> Self {
        Self {
            id: r.id.0.clone(),
            prompt: r.prompt.clone(),
            status: r.status.clone(),
        }
    }
}

/// GET /api/tasks — list all tasks
pub async fn list_tasks(State(state): State<AppState>) -> Json<Vec<TaskResponse>> {
    let mgr = state.tasks.read().await;
    let tasks: Vec<TaskResponse> = mgr.all_tasks().iter().map(|t| TaskResponse::from(*t)).collect();
    Json(tasks)
}

/// POST /api/tasks — create a new task
pub async fn create_task(
    State(state): State<AppState>,
    Json(body): Json<CreateTaskRequest>,
) -> (StatusCode, Json<TaskResponse>) {
    let mut mgr = state.tasks.write().await;
    let id = mgr.start_task(body.prompt.clone());
    let task = mgr.get_task(&id).unwrap();
    let resp = TaskResponse::from(task);
    (StatusCode::CREATED, Json(resp))
}

/// GET /api/tasks/{id} — get single task
pub async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TaskResponse>, StatusCode> {
    let mgr = state.tasks.read().await;
    let task_id = TaskId(id);
    match mgr.get_task(&task_id) {
        Some(task) => Ok(Json(TaskResponse::from(task))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// DELETE /api/tasks/{id} — cancel task
pub async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> StatusCode {
    let mut mgr = state.tasks.write().await;
    let task_id = TaskId(id);
    mgr.fail_task(&task_id, "Cancelled by user".to_string());
    StatusCode::NO_CONTENT
}
