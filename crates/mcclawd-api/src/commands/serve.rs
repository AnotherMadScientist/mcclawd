use axum::extract::DefaultBodyLimit;
use axum::http::{self, HeaderValue};
use crate::server::{routes, state::AppState};
use mcclawd_core::McclawdConfig;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

pub async fn execute(port: u16) -> anyhow::Result<()> {
    let config_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".mcclawd")
        .join("config.toml");
    let config = McclawdConfig::load(&config_path)?;
    let state = AppState::new(config);

    let app = routes::api_router(state.clone())
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_origin([
                    "http://localhost:8080".parse::<HeaderValue>().unwrap(),
                    "http://127.0.0.1:8080".parse::<HeaderValue>().unwrap(),
                ])
                .allow_methods([
                    http::Method::GET,
                    http::Method::POST,
                    http::Method::PUT,
                    http::Method::DELETE,
                ])
                .allow_headers([http::header::CONTENT_TYPE, http::header::AUTHORIZATION]),
        )
        .layer(TraceLayer::new_for_http())
        .layer(DefaultBodyLimit::max(1024 * 1024)); // 1MB

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    tracing::info!("McClawd API server listening on 127.0.0.1:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}
