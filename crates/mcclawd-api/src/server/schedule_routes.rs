//! Schedule API route handlers — CRUD for scheduled/cron tasks.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use mcclawd_tasks::scheduler::{CreateScheduleRequest, ScheduledTask};

use super::state::AppState;

/// GET /api/schedules — list all scheduled tasks.
pub async fn list_schedules(State(state): State<AppState>) -> Json<Vec<ScheduledTask>> {
    Json(state.scheduler.list_schedules().await)
}

/// POST /api/schedules — create a new scheduled task.
pub async fn create_schedule(
    State(state): State<AppState>,
    Json(req): Json<CreateScheduleRequest>,
) -> Result<(StatusCode, Json<ScheduledTask>), (StatusCode, String)> {
    match state.scheduler.add_schedule(req).await {
        Ok(task) => {
            // Persist to Postgres (fire-and-forget)
            let store = state.pg_store.clone();
            let t = task.clone();
            tokio::spawn(async move {
                if let Err(e) = store
                    .save_scheduled_task(
                        "admin",
                        &t.id,
                        &t.name,
                        &t.cron_expression,
                        &t.prompt,
                        t.workspace.as_deref(),
                        t.enabled,
                    )
                    .await
                {
                    tracing::warn!(error = %e, "Failed to persist scheduled task to DB");
                }
            });
            Ok((StatusCode::CREATED, Json(task)))
        }
        Err(e) => Err((StatusCode::UNPROCESSABLE_ENTITY, e)),
    }
}

/// GET /api/schedules/:id — get a single schedule.
pub async fn get_schedule(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ScheduledTask>, StatusCode> {
    state
        .scheduler
        .get_schedule(&id)
        .await
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// DELETE /api/schedules/:id — delete a schedule.
pub async fn delete_schedule(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> StatusCode {
    if state.scheduler.remove_schedule(&id).await {
        // Remove from Postgres (fire-and-forget)
        let store = state.pg_store.clone();
        let id_c = id.clone();
        tokio::spawn(async move {
            if let Err(e) = store.delete_scheduled_task(&id_c).await {
                tracing::warn!(error = %e, "Failed to delete scheduled task from DB");
            }
        });
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

/// PUT /api/schedules/:id/toggle — toggle schedule enabled/disabled.
pub async fn toggle_schedule(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ScheduledTask>, StatusCode> {
    match state.scheduler.toggle_schedule(&id).await {
        Some(task) => {
            // Persist updated enabled state to Postgres (fire-and-forget)
            let store = state.pg_store.clone();
            let t = task.clone();
            tokio::spawn(async move {
                if let Err(e) = store
                    .save_scheduled_task(
                        "admin",
                        &t.id,
                        &t.name,
                        &t.cron_expression,
                        &t.prompt,
                        t.workspace.as_deref(),
                        t.enabled,
                    )
                    .await
                {
                    tracing::warn!(error = %e, "Failed to persist schedule toggle to DB");
                }
            });
            Ok(Json(task))
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}
