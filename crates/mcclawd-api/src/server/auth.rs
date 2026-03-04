use axum::{
    extract::State,
    http::{header, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use mcclawd_core::secrets::EncryptedFileBackend;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: i64,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
}

/// POST /api/auth/login — fallback login using vault key (for non-WebAuthn environments).
///
/// Loads the vault key from data_dir/vault.key and uses it as the passphrase.
/// If vault.key does not exist, returns 400 (setup required via WebAuthn).
/// The `password` field is ignored — authentication is gate-kept by WebAuthn.
/// This endpoint exists for programmatic/CLI access after initial setup.
pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, StatusCode> {
    let (data_dir, secrets_path) = {
        let config = state.config.read().await;
        (config.data_dir.clone(), config.secrets_path())
    };

    // Load vault key from disk — if it doesn't exist, setup is required
    let vault_key_path = data_dir.join("vault.key");
    if !vault_key_path.exists() {
        tracing::warn!("Login attempted but vault.key not found — setup required");
        return Err(StatusCode::BAD_REQUEST);
    }

    let vault_key_bytes = tokio::fs::read(&vault_key_path).await.map_err(|e| {
        tracing::error!("Failed to read vault key: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let passphrase: String = vault_key_bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    // Validate: the provided password must match the hex-encoded vault key.
    // WebAuthn is the primary auth; this fallback is for programmatic/CLI access.
    if body.password != passphrase {
        tracing::warn!("Fallback login: incorrect vault key");
        return Err(StatusCode::UNAUTHORIZED);
    }

    match EncryptedFileBackend::new(&secrets_path, &passphrase) {
        Ok(backend) => {
            let mut secrets = state.secrets.write().await;
            *secrets = Some(Arc::new(backend));
            tracing::info!("Secrets vault unlocked via fallback login");
        }
        Err(e) => {
            tracing::warn!("Failed to unlock secrets vault: {e}");
            let backend = EncryptedFileBackend::new_empty(&secrets_path, &passphrase).map_err(
                |e| {
                    tracing::error!("Failed to create secrets vault: {e}");
                    StatusCode::INTERNAL_SERVER_ERROR
                },
            )?;
            let mut secrets = state.secrets.write().await;
            *secrets = Some(Arc::new(backend));
        }
    }

    let exp = Utc::now() + Duration::hours(24);
    let claims = Claims {
        sub: "mcclawd-user".to_string(),
        exp: exp.timestamp(),
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(LoginResponse { token }))
}

/// Auth middleware — validates Bearer token from Authorization header.
pub async fn auth_middleware(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(h) if h.starts_with("Bearer ") => &h[7..],
        _ => {
            return (StatusCode::UNAUTHORIZED, "Missing or invalid Authorization header")
                .into_response();
        }
    };

    let validation = Validation::default();
    match decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &validation,
    ) {
        Ok(_) => next.run(req).await,
        Err(_) => (StatusCode::UNAUTHORIZED, "Invalid token").into_response(),
    }
}
