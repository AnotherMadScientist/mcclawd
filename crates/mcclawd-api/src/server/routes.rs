use axum::{routing::get, Router};

use super::state::AppState;

pub fn api_router() -> Router<AppState> {
    Router::new().route("/api/health", get(health))
}

async fn health() -> &'static str {
    "ok"
}
