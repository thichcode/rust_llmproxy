mod config;
mod error;
mod models;
mod providers;
mod router;
mod rtk;
mod server;
mod transform;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router as AxumRouter;
use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::router::Router;

#[derive(Parser, Debug)]
#[command(
    name = "mini-ai-router-rs",
    about = "A lightweight local AI API gateway"
)]
struct Cli {
    #[arg(short, long, default_value = "config.yaml")]
    config: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();
    let cfg = config::Config::from_file(&cli.config)?;
    let config = Arc::new(cfg);
    let router = Arc::new(Router::new(config.clone()));

    let app = AxumRouter::new()
        .route("/health", get(server::handlers::health))
        .route("/v1/models", get(server::handlers::list_models))
        .route(
            "/v1/chat/completions",
            post(server::handlers::chat_completions),
        )
        .route(
            "/anthropic/v1/messages",
            post(server::handlers::anthropic_messages),
        )
        .with_state(router.clone());

    let addr = format!("{}:{}", config.server.host, config.server.port);
    info!("mini-ai-router-rs listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
