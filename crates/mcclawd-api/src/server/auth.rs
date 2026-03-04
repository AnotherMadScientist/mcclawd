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

/// POST /api/auth/login — validates password against vault passphrase, unlocks SecretBackend
pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, StatusCode> {
    // Phase 0: validate against hardcoded passphrase (same as CLI)
    // Phase 1+: derive from keychain or stored hash
    let passphrase = "mcclawd-local-dev";
    if body.password != passphrase {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let secrets_path = {
        let config = state.config.read().await;
        config.secrets_path()
    };
    match EncryptedFileBackend::new(&secrets_path, passphrase) {
        Ok(backend) => {
            let mut secrets = state.secrets.write().await;
            *secrets = Some(Arc::new(backend));
            tracing::info!("Secrets vault unlocked");
        }
        Err(e) => {
            tracing::warn!("Failed to unlock secrets vault: {e}");
            let backend = EncryptedFileBackend::new_empty(&secrets_path, passphrase)
                .map_err(|e| {
                    tracing::error!("Failed to create secrets vault: {e}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
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
