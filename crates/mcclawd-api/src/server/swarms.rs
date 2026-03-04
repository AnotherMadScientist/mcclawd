//! Swarm API route handlers — Phase 2 placeholders.

use axum::{
    extract::Path,
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Request body for POST /api/swarms
#[derive(Debug, Deserialize)]
pub struct CreateSwarmRequest {
    pub prompt: String,
}

/// Response for POST /api/swarms
#[derive(Debug, Serialize)]
pub struct CreateSwarmResponse {
    pub swarm_id: String,
    pub status: String,
}

/// Summary of a swarm run (for list endpoint).
#[derive(Debug, Serialize)]
pub struct SwarmSummary {
    pub swarm_id: String,
    pub status: String,
    pub prompt: String,
}

/// Detailed swarm status.
#[derive(Debug, Serialize)]
pub struct SwarmStatus {
    pub swarm_id: String,
    pub status: String,
    pub wave: usize,
    pub total_waves: usize,
    pub subtasks: Vec<String>,
}

/// GET /api/swarms — list all swarm runs (placeholder: returns empty array).
pub async fn list_swarms() -> Json<Vec<SwarmSummary>> {
    // Phase 2 placeholder — will query SwarmRegistry when wired up
    Json(vec![])
}

/// GET /api/swarms/{id} — get swarm status (placeholder: returns 404).
pub async fn get_swarm(Path(id): Path<String>) -> Result<Json<SwarmStatus>, StatusCode> {
    // Phase 2 placeholder — will look up swarm by ID when registry is wired
    tracing::debug!(swarm_id = %id, "Swarm lookup (placeholder — not found)");
    Err(StatusCode::NOT_FOUND)
}

/// POST /api/swarms — start a swarm run (placeholder: accepts prompt, returns swarm_id).
pub async fn create_swarm(
    Json(payload): Json<CreateSwarmRequest>,
) -> (StatusCode, Json<CreateSwarmResponse>) {
    let swarm_id = Uuid::new_v4().to_string();
    tracing::info!(swarm_id = %swarm_id, prompt = %payload.prompt, "Swarm created (placeholder)");

    (
        StatusCode::CREATED,
        Json(CreateSwarmResponse {
            swarm_id,
            status: "pending".into(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_swarms_returns_empty() {
        let result = list_swarms().await;
        assert!(result.0.is_empty());
    }

    #[tokio::test]
    async fn get_swarm_returns_not_found() {
        let result = get_swarm(Path("nonexistent".into())).await;
        assert_eq!(result.unwrap_err(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn create_swarm_returns_id() {
        let req = CreateSwarmRequest {
            prompt: "test swarm".into(),
        };
        let (status, json) = create_swarm(Json(req)).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(json.status, "pending");
        assert!(!json.swarm_id.is_empty());
    }
}
