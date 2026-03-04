//! WebAuthn (passkey/biometric) authentication endpoints.
//!
//! Flow:
//! 1. GET  /api/auth/status          - Check if setup is complete
//! 2. POST /api/auth/register/start  - Begin passkey registration (first-time setup)
//! 3. POST /api/auth/register/finish - Complete registration, generate vault key, issue JWT
//! 4. POST /api/auth/login/start     - Begin passkey authentication
//! 5. POST /api/auth/login/finish    - Complete authentication, unlock vault, issue JWT

use axum::{extract::State, http::StatusCode, Json};
use chrono::{Duration, Utc};
use jsonwebtoken::{encode, EncodingKey, Header};
use mcclawd_core::secrets::EncryptedFileBackend;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use webauthn_rs::prelude::*;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use super::auth::Claims;
use super::state::AppState;

// --- Response types ---

#[derive(Debug, Serialize)]
pub struct AuthStatusResponse {
    pub setup_complete: bool,
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub token: String,
}

// --- Credential persistence ---

/// Stored credential data (serialized to data_dir/webauthn_credentials.json).
#[derive(Debug, Serialize, Deserialize)]
struct StoredCredentials {
    user_id: Uuid,
    passkeys: Vec<Passkey>,
}

/// Get the path to the WebAuthn credentials file.
fn credentials_path(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join("webauthn_credentials.json")
}

/// Get the path to the vault key file.
fn vault_key_path(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join("vault.key")
}

/// Load stored credentials from disk.
async fn load_credentials(
    data_dir: &std::path::Path,
) -> Result<Option<StoredCredentials>, StatusCode> {
    let path = credentials_path(data_dir);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = tokio::fs::read(&path).await.map_err(|e| {
        tracing::error!("Failed to read credentials file: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let creds: StoredCredentials = serde_json::from_slice(&bytes).map_err(|e| {
        tracing::error!("Failed to parse credentials file: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Some(creds))
}

/// Save credentials to disk (restricted permissions).
async fn save_credentials(
    data_dir: &std::path::Path,
    creds: &StoredCredentials,
) -> Result<(), StatusCode> {
    let path = credentials_path(data_dir);
    // Ensure data dir exists
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            tracing::error!("Failed to create data dir: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }
    let json = serde_json::to_vec_pretty(&creds).map_err(|e| {
        tracing::error!("Failed to serialize credentials: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    write_sensitive_file(&path, &json).await
}

/// Issue a JWT token.
fn issue_jwt(jwt_secret: &str) -> Result<String, StatusCode> {
    let exp = Utc::now() + Duration::hours(24);
    let claims = Claims {
        sub: "mcclawd-user".to_string(),
        exp: exp.timestamp(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_bytes()),
    )
    .map_err(|e| {
        tracing::error!("Failed to encode JWT: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

/// Unlock (or create) the vault using the stored vault key.
async fn unlock_vault_with_key(state: &AppState) -> Result<(), StatusCode> {
    let (data_dir, secrets_path) = {
        let config = state.config.read().await;
        (config.data_dir.clone(), config.secrets_path())
    };
    let key_path = vault_key_path(&data_dir);
    let vault_key_bytes = tokio::fs::read(&key_path).await.map_err(|e| {
        tracing::error!("Failed to read vault key: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let passphrase = hex_encode(&vault_key_bytes);

    match EncryptedFileBackend::new(&secrets_path, &passphrase) {
        Ok(backend) => {
            let mut secrets = state.secrets.write().await;
            *secrets = Some(Arc::new(backend));
            tracing::info!("Secrets vault unlocked via WebAuthn");
        }
        Err(_) => {
            // Vault file may not exist yet — create empty
            let backend = EncryptedFileBackend::new_empty(&secrets_path, &passphrase).map_err(
                |e| {
                    tracing::error!("Failed to create secrets vault: {e}");
                    StatusCode::INTERNAL_SERVER_ERROR
                },
            )?;
            let mut secrets = state.secrets.write().await;
            *secrets = Some(Arc::new(backend));
            tracing::info!("New secrets vault created via WebAuthn");
        }
    }
    Ok(())
}

/// Simple hex encoding (avoids adding hex crate dependency).
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Write sensitive file with restricted permissions (0600 on Unix).
async fn write_sensitive_file(
    path: &std::path::Path,
    contents: &[u8],
) -> Result<(), StatusCode> {
    tokio::fs::write(path, contents).await.map_err(|e| {
        tracing::error!("Failed to write {}: {e}", path.display());
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    #[cfg(unix)]
    {
        let perms = std::fs::Permissions::from_mode(0o600);
        tokio::fs::set_permissions(path, perms).await.map_err(|e| {
            tracing::error!("Failed to set permissions on {}: {e}", path.display());
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }
    Ok(())
}

// --- Endpoints ---

/// GET /api/auth/status — check if WebAuthn setup is complete.
pub async fn auth_status(
    State(state): State<AppState>,
) -> Result<Json<AuthStatusResponse>, StatusCode> {
    let data_dir = {
        let config = state.config.read().await;
        config.data_dir.clone()
    };
    let setup_complete = credentials_path(&data_dir).exists();
    Ok(Json(AuthStatusResponse { setup_complete }))
}

/// POST /api/auth/register/start — begin passkey registration.
///
/// Returns PublicKeyCredentialCreationOptions for the browser.
pub async fn register_start(
    State(state): State<AppState>,
) -> Result<Json<CreationChallengeResponse>, StatusCode> {
    let data_dir = {
        let config = state.config.read().await;
        config.data_dir.clone()
    };

    // Prevent re-registration if credentials already exist
    if credentials_path(&data_dir).exists() {
        tracing::warn!("Attempted re-registration when credentials already exist");
        return Err(StatusCode::CONFLICT);
    }

    let user_id = Uuid::new_v4();
    let (ccr, reg_state) = state
        .webauthn
        .start_passkey_registration(user_id, "mcclawd-admin", "McClawd Admin", None)
        .map_err(|e| {
            tracing::error!("Failed to start passkey registration: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Store registration state + user_id for finish step
    {
        let mut reg = state.webauthn_reg_state.write().await;
        *reg = Some((user_id, reg_state));
    }

    Ok(Json(ccr))
}

/// POST /api/auth/register/finish — complete passkey registration.
///
/// Expects RegisterPublicKeyCredential from the browser.
/// On success: saves credential, generates vault key, issues JWT.
pub async fn register_finish(
    State(state): State<AppState>,
    Json(credential): Json<RegisterPublicKeyCredential>,
) -> Result<Json<TokenResponse>, StatusCode> {
    // Take the registration state (one-time use)
    let (reg_state_user_id, reg_state) = {
        let mut reg = state.webauthn_reg_state.write().await;
        reg.take().ok_or_else(|| {
            tracing::error!("No registration state found — call register/start first");
            StatusCode::BAD_REQUEST
        })?
    };

    // Verify the registration response
    let passkey = state
        .webauthn
        .finish_passkey_registration(&credential, &reg_state)
        .map_err(|e| {
            tracing::error!("Failed to finish passkey registration: {e}");
            StatusCode::BAD_REQUEST
        })?;

    let data_dir = {
        let config = state.config.read().await;
        config.data_dir.clone()
    };

    // Ensure data dir exists
    tokio::fs::create_dir_all(&data_dir).await.map_err(|e| {
        tracing::error!("Failed to create data dir: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Save the credential — reuse the user_id from the registration ceremony
    let creds = StoredCredentials {
        user_id: reg_state_user_id,
        passkeys: vec![passkey],
    };
    save_credentials(&data_dir, &creds).await?;

    // Generate random 32-byte vault key (restricted permissions)
    let mut vault_key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut vault_key);
    let key_path = vault_key_path(&data_dir);
    write_sensitive_file(&key_path, &vault_key).await?;

    // Unlock vault with the new key
    unlock_vault_with_key(&state).await?;

    // Issue JWT
    let token = issue_jwt(&state.jwt_secret)?;
    tracing::info!("WebAuthn registration complete, vault created");

    Ok(Json(TokenResponse { token }))
}

/// POST /api/auth/login/start — begin passkey authentication.
///
/// Returns PublicKeyCredentialRequestOptions for the browser.
pub async fn login_start(
    State(state): State<AppState>,
) -> Result<Json<RequestChallengeResponse>, StatusCode> {
    let data_dir = {
        let config = state.config.read().await;
        config.data_dir.clone()
    };

    // Load stored credentials
    let creds = load_credentials(&data_dir)
        .await?
        .ok_or_else(|| {
            tracing::warn!("No credentials found — setup required");
            StatusCode::BAD_REQUEST
        })?;

    if creds.passkeys.is_empty() {
        tracing::error!("Credentials file exists but contains no passkeys");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let (rcr, auth_state) = state
        .webauthn
        .start_passkey_authentication(&creds.passkeys)
        .map_err(|e| {
            tracing::error!("Failed to start passkey authentication: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Store authentication state for finish step
    {
        let mut auth = state.webauthn_auth_state.write().await;
        *auth = Some(auth_state);
    }

    Ok(Json(rcr))
}

/// POST /api/auth/login/finish — complete passkey authentication.
///
/// Expects PublicKeyCredential from the browser.
/// On success: unlocks vault, issues JWT.
pub async fn login_finish(
    State(state): State<AppState>,
    Json(credential): Json<PublicKeyCredential>,
) -> Result<Json<TokenResponse>, StatusCode> {
    // Take the authentication state (one-time use)
    let auth_state = {
        let mut auth = state.webauthn_auth_state.write().await;
        auth.take().ok_or_else(|| {
            tracing::error!("No authentication state found — call login/start first");
            StatusCode::BAD_REQUEST
        })?
    };

    // Verify the authentication response
    let auth_result = state
        .webauthn
        .finish_passkey_authentication(&credential, &auth_state)
        .map_err(|e| {
            tracing::error!("Failed to finish passkey authentication: {e}");
            StatusCode::UNAUTHORIZED
        })?;

    // Update credential counter to detect authenticator cloning
    let data_dir = {
        let config = state.config.read().await;
        config.data_dir.clone()
    };
    if let Ok(Some(mut creds)) = load_credentials(&data_dir).await {
        for passkey in &mut creds.passkeys {
            passkey.update_credential(&auth_result);
        }
        if let Err(e) = save_credentials(&data_dir, &creds).await {
            tracing::warn!("Failed to update credential counter: {e:?}");
        }
    }

    // Unlock vault
    unlock_vault_with_key(&state).await?;

    // Issue JWT
    let token = issue_jwt(&state.jwt_secret)?;
    tracing::info!("WebAuthn authentication successful, vault unlocked");

    Ok(Json(TokenResponse { token }))
}
