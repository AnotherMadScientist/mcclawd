use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use super::state::AppState;

#[derive(Debug, Serialize)]
pub struct SecretEntry {
    pub name: String,
    /// Optional human-readable descriptor (e.g. "prod billing account").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub descriptor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SecretValue {
    pub name: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub descriptor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSecretRequest {
    pub name: String,
    pub value: String,
    /// Optional human-readable descriptor for this secret.
    pub descriptor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSecretRequest {
    pub value: String,
    /// Optional descriptor update. If omitted, existing descriptor is preserved.
    pub descriptor: Option<String>,
}

/// GET /api/secrets — list secret names (with descriptors) from the encrypted vault
pub async fn list_secrets(State(state): State<AppState>) -> Result<Json<Vec<SecretEntry>>, StatusCode> {
    let guard = state.secrets.read().await;
    let backend = guard.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let secrets = backend.list_with_metadata().await.map_err(|e| {
        tracing::error!("Failed to list secrets: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let entries: Vec<SecretEntry> = secrets
        .into_iter()
        .map(|s| SecretEntry {
            name: s.key,
            descriptor: s.descriptor,
        })
        .collect();
    Ok(Json(entries))
}

/// POST /api/secrets — store a secret in the encrypted vault
pub async fn create_secret(
    State(state): State<AppState>,
    Json(body): Json<CreateSecretRequest>,
) -> Result<StatusCode, StatusCode> {
    let guard = state.secrets.read().await;
    let backend = guard.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    backend
        .set_with_descriptor(&body.name, &body.value, body.descriptor.as_deref())
        .await
        .map_err(|e| {
            tracing::error!("Failed to create secret: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::CREATED)
}

/// GET /api/secrets/{name} — reveal a secret's value from the encrypted vault
pub async fn get_secret(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<SecretValue>, StatusCode> {
    let guard = state.secrets.read().await;
    let backend = guard.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let value = backend.get(&name).await.map_err(|e| {
        tracing::error!("Failed to get secret: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    match value {
        Some(v) => {
            let descriptor = backend.get_descriptor(&name).await.unwrap_or(None);
            Ok(Json(SecretValue {
                name,
                value: v,
                descriptor,
            }))
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// PUT /api/secrets/{name} — update a secret's value in the encrypted vault
pub async fn update_secret(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<UpdateSecretRequest>,
) -> Result<StatusCode, StatusCode> {
    let guard = state.secrets.read().await;
    let backend = guard.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    // If descriptor is provided in the request, update it; otherwise preserve existing
    match &body.descriptor {
        Some(d) => {
            backend
                .set_with_descriptor(&name, &body.value, Some(d.as_str()))
                .await
        }
        None => backend.set(&name, &body.value).await,
    }
    .map_err(|e| {
        tracing::error!("Failed to update secret: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/secrets/{name} — remove a secret from the encrypted vault
pub async fn delete_secret(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let guard = state.secrets.read().await;
    let backend = guard.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    backend.delete(&name).await.map_err(|e| {
        tracing::error!("Failed to delete secret: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(StatusCode::NO_CONTENT)
}
