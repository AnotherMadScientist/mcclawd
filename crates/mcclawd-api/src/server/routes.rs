use axum::{
    middleware,
    routing::{delete, get, post, put},
    Router,
};

use super::auth;
use super::channels;
use super::config_routes;
use super::mcp_routes;
use super::secrets;
use super::state::AppState;
use super::swarms;
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
        .route("/api/tasks", get(tasks::list_tasks).post(tasks::create_task))
        .route(
            "/api/tasks/{id}",
            get(tasks::get_task).delete(tasks::delete_task),
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
        .route("/api/mcp/servers", get(mcp_routes::list_mcp_servers))
        // Swarms
        .route(
            "/api/swarms",
            get(swarms::list_swarms).post(swarms::create_swarm),
        )
        .route("/api/swarms/{id}", get(swarms::get_swarm))
        // Channels
        .route("/api/channels", get(channels::list_channels))
        .route("/api/channels/{id}", get(channels::get_channel))
        .route(
            "/api/channels/{id}/test",
            post(channels::test_channel),
        )
        // Apply JWT auth to all protected routes
        .route_layer(middleware::from_fn_with_state(state, auth::auth_middleware));

    public.merge(ws_routes).merge(protected)
}

async fn health() -> &'static str {
    "ok"
}
