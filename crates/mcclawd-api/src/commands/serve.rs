use crate::server::{routes, state::AppState};
use mcclawd_core::McclawdConfig;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

pub async fn execute(port: u16) -> anyhow::Result<()> {
    let config_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".mcclawd")
        .join("config.toml");
    let config = McclawdConfig::load(&config_path)?;
    let state = AppState::new(config);

    let app = routes::api_router()
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!("McClawd API server listening on :{port}");
    axum::serve(listener, app).await?;
    Ok(())
}
