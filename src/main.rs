use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use tokio::net::TcpListener;

mod cleanup;
mod config;
mod diff;
mod error;
mod git;
mod inference;
mod language;
mod prompt;
mod rate_limit;
mod session;
mod state;
mod status;
mod upload;

use crate::config::Config;
use crate::rate_limit::RateLimiter;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::from_env();
    let bind_addr = config.bind_addr.clone();

    let state = AppState::new(config);
    let state_clone = state.clone();
    tokio::spawn(async move { cleanup::run_cleanup_task(state_clone).await });

    let rate_limiter = RateLimiter::new(10.0, 1.0);

    let app = Router::new()
        .route("/forge", post(upload::upload_handler))
        .route("/forge/:slug/status", get(status::status_handler))
        .route("/forge/:slug/stream", get(status::stream_handler))
        .route("/forge/:slug/diff", get(diff::diff_handler))
        .route("/git/:slug/info/refs", get(git::smart_http::info_refs))
        .route("/git/:slug/git-upload-pack", post(git::smart_http::upload_pack))
        .layer(middleware::from_fn(rate_limit::rate_limit_middleware))
        .layer(axum::Extension(rate_limiter))
        .with_state(state);

    let listener = TcpListener::bind(&bind_addr).await?;
    tracing::info!("Forge listening on {}", bind_addr);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}
