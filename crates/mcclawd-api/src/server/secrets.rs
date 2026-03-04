use axum::{
    extract::Path,
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct SecretEntry {
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CreateSecretRequest {
    pub name: String,
    pub value: String,
}

/// GET /api/secrets — stub, returns empty list (Phase 1 integrates SecretBackend)
pub async fn list_secrets() -> Json<Vec<SecretEntry>> {
    Json(vec![])
}

/// POST /api/secrets — stub, returns 201
pub async fn create_secret(
    Json(_body): Json<CreateSecretRequest>,
) -> StatusCode {
    StatusCode::CREATED
}

/// DELETE /api/secrets/{name} — stub, returns 204
pub async fn delete_secret(Path(_name): Path<String>) -> StatusCode {
    StatusCode::NO_CONTENT
}
