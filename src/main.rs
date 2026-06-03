mod auth;
mod config;
mod error;
mod models;
mod providers;
mod router;
mod rtk;
mod server;
mod transform;
mod web;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router as AxumRouter;
use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::auth::github_device_oauth::{
    poll_for_token, request_device_code, GITHUB_COPILOT_CLIENT_ID,
};
use crate::auth::token_store::{mask_token, GithubTokenData, TokenStore};
use crate::router::Router;

#[derive(Parser, Debug)]
#[command(
    name = "mini-ai-router-rs",
    about = "A lightweight local AI API gateway"
)]
struct Cli {
    #[arg(short, long, default_value = "config.yaml", global = true)]
    config: String,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the API server
    Serve {
        #[arg(short, long, default_value = "config.yaml")]
        config: String,
    },
    /// Authenticate with GitHub Copilot
    Copilot {
        #[command(subcommand)]
        action: CopilotAction,
    },
}

#[derive(Subcommand, Debug)]
enum CopilotAction {
    /// Login to GitHub Copilot via OAuth device flow
    Login,
    /// Logout and remove stored token
    Logout,
    /// Show authentication status
    Status,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();

    match cli.command {
        None => run_server(cli.config).await,
        Some(Commands::Serve { config }) => run_server(config).await,
        Some(Commands::Copilot { action }) => match action {
            CopilotAction::Login => cmd_copilot_login().await,
            CopilotAction::Logout => cmd_copilot_logout(),
            CopilotAction::Status => cmd_copilot_status(),
        },
    }
}

async fn run_server(config_path: String) -> anyhow::Result<()> {
    let cfg = config::Config::from_file(&config_path)?;
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
        .route("/", get(web::dashboard))
        .route("/api/web/models", get(web::web_models))
        .route("/api/web/copilot-status", get(web::copilot_status))
        .route("/api/web/copilot-login", post(web::copilot_login))
        .route("/api/web/copilot-poll", post(web::copilot_poll))
        .route("/api/web/copilot-logout", post(web::copilot_logout))
        .with_state(router.clone());

    let addr = format!("{}:{}", config.server.host, config.server.port);
    info!("mini-ai-router-rs listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn cmd_copilot_login() -> anyhow::Result<()> {
    let store = TokenStore::new()?;

    if store.load()?.is_some() {
        println!("Already authenticated. Run 'copilot logout' first to re-authenticate.");
        return Ok(());
    }

    println!("Starting GitHub Copilot OAuth device flow...");
    println!("Using client_id: {}", GITHUB_COPILOT_CLIENT_ID);

    let device_resp = request_device_code(GITHUB_COPILOT_CLIENT_ID).await?;

    println!();
    println!("1. Open: {}", device_resp.verification_uri);
    println!("2. Enter code: {}", device_resp.user_code);
    println!();
    println!("Code expires in {} seconds", device_resp.expires_in);
    println!("Waiting for authorization...");

    let token_resp = poll_for_token(
        GITHUB_COPILOT_CLIENT_ID,
        &device_resp.device_code,
        device_resp.interval,
        device_resp.expires_in,
    )
    .await?;

    let access_token = token_resp
        .access_token
        .ok_or_else(|| anyhow::anyhow!("No access token received"))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let token_data = GithubTokenData {
        github_access_token: access_token,
        token_type: token_resp
            .token_type
            .unwrap_or_else(|| "bearer".to_string()),
        scope: token_resp.scope,
        created_at: now,
    };

    store.save(&token_data)?;

    println!();
    println!("✓ Authentication successful!");
    println!("Token stored at: {}", store.path().display());

    Ok(())
}

fn cmd_copilot_logout() -> anyhow::Result<()> {
    let store = TokenStore::new()?;
    let was_present = store.load()?.is_some();
    store.delete()?;
    if was_present {
        println!("✓ Logged out. Token removed.");
    } else {
        println!("No stored token found.");
    }
    Ok(())
}

fn cmd_copilot_status() -> anyhow::Result<()> {
    let store = TokenStore::new()?;
    let token_data = store.load()?;

    match token_data {
        Some(data) => {
            println!("authenticated: true");
            println!("token_present: true");
            println!("created_at: {}", data.created_at);
            println!("token_prefix: {}", mask_token(&data.github_access_token));
        }
        None => {
            println!("authenticated: false");
            println!("token_present: false");
        }
    }

    Ok(())
}
