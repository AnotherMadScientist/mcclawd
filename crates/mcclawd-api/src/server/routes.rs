use axum::{
    routing::{delete, get, post, put},
    Router,
};

use super::auth;
use super::config_routes;
use super::mcp_routes;
use super::secrets;
use super::state::AppState;
use super::tasks;
use super::workspace;
use super::ws;

pub fn api_router() -> Router<AppState> {
    // Public routes (no auth required)
    let public = Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/login", post(auth::login));

    // Protected routes — all require valid JWT
    let protected = Router::new()
        // Tasks
        .route("/api/tasks", get(tasks::list_tasks).post(tasks::create_task))
        .route(
            "/api/tasks/{id}",
            get(tasks::get_task).delete(tasks::delete_task),
        )
        // WebSocket streaming
        .route("/api/tasks/{id}/stream", get(ws::task_stream))
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
        .route("/api/mcp/servers", get(mcp_routes::list_mcp_servers));

    public.merge(protected)
}

async fn health() -> &'static str {
    "ok"
}
