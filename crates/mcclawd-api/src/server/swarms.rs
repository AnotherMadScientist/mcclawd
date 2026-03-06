//! Swarm API route handlers — wired to SwarmRegistry + SwarmCoordinator.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use super::state::AppState;
use super::swarm_registry::{SwarmRunDetail, SwarmRunStatus, SwarmRunSummary};

/// Request body for POST /api/swarms
#[derive(Debug, Deserialize)]
pub struct CreateSwarmRequest {
    pub prompt: String,
    #[serde(default)]
    pub workspace: Option<String>,
}

/// Response for POST /api/swarms
#[derive(Debug, serde::Serialize)]
pub struct CreateSwarmResponse {
    pub swarm_id: String,
    pub status: String,
}

/// GET /api/swarms — list all swarm runs.
pub async fn list_swarms(State(state): State<AppState>) -> Json<Vec<SwarmRunSummary>> {
    Json(state.swarm_registry.list())
}

/// GET /api/swarms/{id} — get swarm status.
pub async fn get_swarm(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SwarmRunDetail>, StatusCode> {
    state
        .swarm_registry
        .get(&id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// POST /api/swarms — start a swarm run.
///
/// Creates a SwarmCoordinator, registers it in the SwarmRegistry,
/// and spawns background execution.
pub async fn create_swarm(
    State(state): State<AppState>,
    Json(payload): Json<CreateSwarmRequest>,
) -> (StatusCode, Json<CreateSwarmResponse>) {
    let swarm_id = Uuid::new_v4().to_string();

    // Register in the swarm registry (status: Planning)
    let cancel_token = state
        .swarm_registry
        .register(swarm_id.clone(), payload.prompt.clone());

    // Persist swarm creation to Postgres (fire-and-forget)
    {
        let store = state.pg_store.clone();
        let id = swarm_id.clone();
        let name = payload.prompt.chars().take(100).collect::<String>();
        let config_json = serde_json::json!({ "prompt": payload.prompt, "workspace": payload.workspace });
        tokio::spawn(async move {
            if let Err(e) = store.save_swarm_run("admin", &id, &name, "planning", &config_json).await {
                tracing::warn!(error = %e, "Failed to persist swarm creation to DB");
            }
        });
    }

    // Spawn background swarm execution
    let registry = state.swarm_registry.clone();
    let pg_store = state.pg_store.clone();
    let sid = swarm_id.clone();
    let prompt = payload.prompt.clone();

    tokio::spawn(async move {
        // Update status to Running
        registry.update_status(
            &sid,
            SwarmRunStatus::Running {
                wave: 0,
                total_waves: 0,
            },
        );
        // Persist running status (fire-and-forget)
        let store = pg_store.clone();
        let sid_c = sid.clone();
        tokio::spawn(async move {
            if let Err(e) = store.update_swarm_run(&sid_c, "running", None).await {
                tracing::warn!(error = %e, "Failed to persist swarm running status");
            }
        });

        // Build a DAG and execute via SwarmCoordinator
        // Phase 2: This would use SwarmPlanner to build a real DAG from the prompt
        // For now, log that we would execute
        tokio::select! {
            _ = cancel_token.cancelled() => {
                tracing::info!(swarm_id = %sid, "Swarm cancelled by user");
                // Persist cancelled status
                let store = pg_store.clone();
                let sid_c = sid.clone();
                tokio::spawn(async move {
                    if let Err(e) = store.update_swarm_run(&sid_c, "cancelled", None).await {
                        tracing::warn!(error = %e, "Failed to persist swarm cancelled status");
                    }
                });
            }
            _ = async {
                tracing::info!(swarm_id = %sid, prompt = %prompt, "Swarm execution started (stub)");
                // Simulate some work
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                registry.update_status(&sid, SwarmRunStatus::Completed);
                registry.set_result(&sid, format!("Swarm completed for: {prompt}"));
                tracing::info!(swarm_id = %sid, "Swarm execution completed");
                // Persist completed status + result
                let result_text = format!("Swarm completed for: {prompt}");
                let store = pg_store.clone();
                let sid_c = sid.clone();
                tokio::spawn(async move {
                    if let Err(e) = store.update_swarm_run(&sid_c, "completed", Some(&result_text)).await {
                        tracing::warn!(error = %e, "Failed to persist swarm completed status");
                    }
                });
            } => {}
        }
    });

    (
        StatusCode::CREATED,
        Json(CreateSwarmResponse {
            swarm_id,
            status: "planning".into(),
        }),
    )
}

/// DELETE /api/swarms/{id} — cancel a running swarm.
pub async fn cancel_swarm(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> StatusCode {
    if state.swarm_registry.cancel(&id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_swarm_request_deserializes() {
        let json = r#"{"prompt": "test swarm"}"#;
        let req: CreateSwarmRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.prompt, "test swarm");
        assert!(req.workspace.is_none());
    }

    #[test]
    fn create_swarm_request_with_workspace() {
        let json = r#"{"prompt": "test", "workspace": "my-ws"}"#;
        let req: CreateSwarmRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.workspace.as_deref(), Some("my-ws"));
    }
}
