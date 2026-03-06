use axum::{
    extract::State,
    http::StatusCode,
    middleware,
    routing::{delete, get, post, put},
    Json, Router,
};

use super::auth;
use super::channel_state;
use super::channels;
use super::config_routes;
use super::mcp_routes;
use super::providers;
use super::schedule_routes;
use super::secrets;
use super::security;
use super::skills_routes;
use super::state::AppState;
use super::swarms;
use super::system_agent;
use super::tasks;
use super::webauthn_auth;
use super::workspace;
use super::ws;

pub fn api_router(state: AppState) -> Router<AppState> {
    // Public routes (no auth required)
    let public = Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/login", post(auth::login))
        // WebAuthn endpoints (public — they ARE the auth flow)
        .route("/api/auth/status", get(webauthn_auth::auth_status))
        .route(
            "/api/auth/register/start",
            post(webauthn_auth::register_start),
        )
        .route(
            "/api/auth/register/finish",
            post(webauthn_auth::register_finish),
        )
        .route("/api/auth/login/start", post(webauthn_auth::login_start))
        .route("/api/auth/credentials", delete(webauthn_auth::reset_credentials))
        .route(
            "/api/auth/login/finish",
            post(webauthn_auth::login_finish),
        );

    // WebSocket routes — auth handled via query param (browsers can't send headers on WS)
    let ws_routes = Router::new()
        .route("/api/tasks/{id}/stream", get(ws::task_stream));

    // Protected routes — all require valid JWT
    let protected = Router::new()
        // Tasks
        .route("/api/tasks", get(tasks::list_tasks).post(tasks::create_task).delete(tasks::delete_all_tasks))
        .route(
            "/api/tasks/{id}",
            get(tasks::get_task).delete(tasks::delete_task),
        )
        .route(
            "/api/tasks/{id}/message",
            post(tasks::send_message),
        )
        .route(
            "/api/tasks/{id}/attachments",
            get(tasks::list_attachments).post(tasks::upload_attachments),
        )
        .route(
            "/api/tasks/{id}/attachments/{filename}",
            get(tasks::download_attachment),
        )
        // Workspace
        .route("/api/workspace", get(workspace::list_files))
        .route(
            "/api/workspace/{file}",
            get(workspace::get_file).put(workspace::put_file),
        )
        // Secrets
        .route(
            "/api/secrets",
            get(secrets::list_secrets).post(secrets::create_secret),
        )
        .route(
            "/api/secrets/{name}",
            get(secrets::get_secret)
                .put(secrets::update_secret)
                .delete(secrets::delete_secret),
        )
        // Config
        .route(
            "/api/config",
            get(config_routes::get_config).put(config_routes::put_config),
        )
        // MCP
        .route(
            "/api/mcp/servers",
            get(mcp_routes::list_mcp_servers).post(mcp_routes::add_mcp_server),
        )
        .route(
            "/api/mcp/servers/{name}",
            delete(mcp_routes::remove_mcp_server),
        )
        .route(
            "/api/mcp/servers/{name}/restart",
            post(mcp_routes::restart_mcp_server),
        )
        .route(
            "/api/mcp/servers/{name}/status",
            get(mcp_routes::mcp_server_status),
        )
        // Swarms
        .route(
            "/api/swarms",
            get(swarms::list_swarms).post(swarms::create_swarm),
        )
        .route(
            "/api/swarms/{id}",
            get(swarms::get_swarm).delete(swarms::cancel_swarm),
        )
        // Schedules
        .route(
            "/api/schedules",
            get(schedule_routes::list_schedules).post(schedule_routes::create_schedule),
        )
        .route(
            "/api/schedules/{id}",
            get(schedule_routes::get_schedule).delete(schedule_routes::delete_schedule),
        )
        .route(
            "/api/schedules/{id}/toggle",
            put(schedule_routes::toggle_schedule),
        )
        // Channels
        .route("/api/channels", get(channels::list_channels))
        .route("/api/channels/{id}", get(channels::get_channel))
        .route(
            "/api/channels/{id}/test",
            post(channels::test_channel),
        )
        // Security
        .route("/api/security/hooks", get(security::list_hooks))
        .route("/api/security/backends", get(security::list_backends))
        // Channel state persistence
        .route(
            "/api/channels/state",
            get(channel_state::list_channel_states),
        )
        .route(
            "/api/channels/state/{kind}",
            delete(channel_state::delete_channel_state),
        )
        // Skills
        .route("/api/skills", get(skills_routes::list_installed))
        .route("/api/skills/search", get(skills_routes::search_clawhub))
        .route("/api/skills/install", post(skills_routes::install_skill))
        .route("/api/skills/create", post(skills_routes::create_skill))
        // Catalog cache endpoints (must be before /api/skills/{name} to avoid capture)
        .route("/api/skills/catalog", get(skills_routes::browse_catalog))
        .route(
            "/api/skills/catalog/{name}",
            get(skills_routes::skill_detail),
        )
        .route("/api/skills/refresh", post(skills_routes::refresh_catalog))
        .route("/api/skills/refresh-stream", get(skills_routes::refresh_catalog_stream))
        .route(
            "/api/skills/{name}/content",
            get(skills_routes::skill_content),
        )
        .route("/api/skills/{name}/scan", get(skills_routes::scan_skill))
        .route("/api/skills/updates", get(skills_routes::get_skill_updates))
        .route("/api/skills/{name}", delete(skills_routes::uninstall_skill))
        // Providers
        .route("/api/providers", get(providers::list_providers))
        .route("/api/providers/models", get(providers::list_models))
        .route("/api/providers/pricing", get(providers::model_pricing))
        .route("/api/providers/usage", get(providers::provider_usage))
        .route(
            "/api/providers/usage/detailed",
            get(providers::provider_usage_detailed),
        )
        .route("/api/providers/budget", put(providers::update_budget))
        .route(
            "/api/providers/budget/alerts",
            get(providers::budget_alerts),
        )
        .route(
            "/api/providers/budget/info",
            get(providers::budget_info),
        )
        .route(
            "/api/providers/credits",
            get(providers::provider_credits),
        )
        // Config reload
        .route("/api/config/reload", post(providers::reload_config))
        // LLM health check (tiny API call to verify key works)
        .route("/api/health/llm", get(llm_health))
        // System agent
        .route("/api/system-agent/chat", post(system_agent::chat))
        .route(
            "/api/system-agent/history",
            get(system_agent::history).delete(system_agent::clear_history),
        )
        // Apply JWT auth to all protected routes
        .route_layer(middleware::from_fn_with_state(state, auth::auth_middleware));

    public.merge(ws_routes).merge(protected)
}

async fn health() -> &'static str {
    "ok"
}

/// Check if the LLM is reachable by doing a tiny Anthropic API call (max_tokens=1).
/// Returns { "ok": true/false, "error": "..." }
async fn llm_health(
    State(state): State<AppState>,
) -> (StatusCode, Json<serde_json::Value>) {
    // 1. Check if vault is unlocked and ANTHROPIC_API_KEY exists
    let api_key = {
        let secrets = state.secrets.read().await;
        match secrets.as_ref() {
            Some(backend) => match backend.get("ANTHROPIC_API_KEY").await {
                Ok(Some(key)) if !key.is_empty() => key,
                _ => {
                    return (
                        StatusCode::OK,
                        Json(serde_json::json!({ "ok": false, "error": "ANTHROPIC_API_KEY not set" })),
                    );
                }
            },
            None => {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({ "ok": false, "error": "Vault locked" })),
                );
            }
        }
    };

    // 2. Tiny API call — 1 token, cheapest model
    let client = reqwest::Client::new();
    let res = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "model": "claude-haiku-4-5-20251001",
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await;

    match res {
        Ok(r) if r.status().is_success() => (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true })),
        ),
        Ok(r) => {
            let status = r.status().as_u16();
            let body = r.text().await.unwrap_or_default();
            let msg = if status == 401 { "Invalid API key" } else { &body };
            (
                StatusCode::OK,
                Json(serde_json::json!({ "ok": false, "error": msg })),
            )
        }
        Err(e) => (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": false, "error": format!("Network error: {e}") })),
        ),
    }
}
