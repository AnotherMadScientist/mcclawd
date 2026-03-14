//! Swarm API route handlers — wired to SwarmRegistry + SwarmCoordinator.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use mcclawd_swarm::{SwarmConfig, SwarmCoordinator, SwarmPlanner};
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
    let secrets = state.secrets.clone();
    let sid = swarm_id.clone();
    let prompt = payload.prompt.clone();

    tokio::spawn(async move {
        // Resolve API key: try secrets vault first, then env var
        let api_key = {
            let secrets_guard = secrets.read().await;
            match secrets_guard.as_ref() {
                Some(backend) => backend
                    .get("ANTHROPIC_API_KEY")
                    .await
                    .ok()
                    .flatten(),
                None => None,
            }
        };
        let api_key = api_key
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok());

        let api_key = match api_key {
            Some(key) if !key.is_empty() => key,
            _ => {
                tracing::error!(swarm_id = %sid, "No API key available for swarm execution");
                registry.update_status(&sid, SwarmRunStatus::Failed {
                    error: "No API key available".into(),
                });
                let store = pg_store.clone();
                let sid_c = sid.clone();
                tokio::spawn(async move {
                    if let Err(e) = store.update_swarm_run(&sid_c, "failed", Some("No API key available")).await {
                        tracing::warn!(error = %e, "Failed to persist swarm failed status");
                    }
                });
                return;
            }
        };

        // Plan: decompose the prompt into a TaskDag
        let planner = SwarmPlanner::new(None, api_key);
        let dag = match planner.decompose(&prompt, &[]).await {
            Ok(dag) => dag,
            Err(e) => {
                let error_msg = format!("Planning failed: {e}");
                tracing::error!(swarm_id = %sid, error = %e, "Swarm planning failed");
                registry.update_status(&sid, SwarmRunStatus::Failed {
                    error: error_msg.clone(),
                });
                let store = pg_store.clone();
                let sid_c = sid.clone();
                tokio::spawn(async move {
                    if let Err(e) = store.update_swarm_run(&sid_c, "failed", Some(&error_msg)).await {
                        tracing::warn!(error = %e, "Failed to persist swarm failed status");
                    }
                });
                return;
            }
        };

        // Update status to Running with wave count from the DAG
        let total_waves = dag.topological_waves().map(|w| w.len()).unwrap_or(0);
        registry.update_status(
            &sid,
            SwarmRunStatus::Running {
                wave: 0,
                total_waves,
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

        // Execute the DAG via SwarmCoordinator
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
            result = async {
                let coordinator = SwarmCoordinator::new(SwarmConfig::default());
                coordinator.execute(&prompt, &dag).await
            } => {
                match result {
                    Ok(swarm_result) => {
                        registry.update_status(&sid, SwarmRunStatus::Completed);
                        registry.set_result(&sid, swarm_result.final_output.clone());
                        tracing::info!(swarm_id = %sid, "Swarm execution completed");
                        // Persist completed status + result
                        let result_text = swarm_result.final_output;
                        let store = pg_store.clone();
                        let sid_c = sid.clone();
                        tokio::spawn(async move {
                            if let Err(e) = store.update_swarm_run(&sid_c, "completed", Some(&result_text)).await {
                                tracing::warn!(error = %e, "Failed to persist swarm completed status");
                            }
                        });
                    }
                    Err(e) => {
                        let error_msg = format!("Swarm execution failed: {e}");
                        tracing::error!(swarm_id = %sid, error = %e, "Swarm execution failed");
                        registry.update_status(&sid, SwarmRunStatus::Failed {
                            error: error_msg.clone(),
                        });
                        let store = pg_store.clone();
                        let sid_c = sid.clone();
                        tokio::spawn(async move {
                            if let Err(e) = store.update_swarm_run(&sid_c, "failed", Some(&error_msg)).await {
                                tracing::warn!(error = %e, "Failed to persist swarm failed status");
                            }
                        });
                    }
                }
            }
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
