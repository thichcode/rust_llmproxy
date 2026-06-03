mod auth;
mod config;
mod error;
mod models;
mod providers;
mod router;
mod rtk;
mod server;
mod transform;
mod updater;
mod web;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router as AxumRouter;
use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::auth::anthropic_oauth::AnthropicOAuth;
use crate::auth::github_device_oauth::{
    poll_for_token, request_device_code, GITHUB_COPILOT_CLIENT_ID,
};
use crate::auth::token_store::{mask_token, ClaudeTokenStore, GithubTokenData, TokenStore};
use crate::server::AppState;
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
    /// Check for updates and apply
    Update {
        #[command(subcommand)]
        action: UpdateAction,
    },
    /// Authenticate with GitHub Copilot
    Copilot {
        #[command(subcommand)]
        action: CopilotAction,
    },
    /// Authenticate with Claude (Anthropic OAuth)
    Claude {
        #[command(subcommand)]
        action: ClaudeAction,
    },
}

#[derive(Subcommand, Debug)]
enum UpdateAction {
    /// Check if a new version is available
    Check,
    /// Download and apply the latest version
    Apply,
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

#[derive(Subcommand, Debug)]
enum ClaudeAction {
    /// Login to Claude via OAuth (opens browser)
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
        Some(Commands::Update { action }) => match action {
            UpdateAction::Check => cmd_update_check().await,
            UpdateAction::Apply => cmd_update_apply().await,
        },
        Some(Commands::Copilot { action }) => match action {
            CopilotAction::Login => cmd_copilot_login().await,
            CopilotAction::Logout => cmd_copilot_logout(),
            CopilotAction::Status => cmd_copilot_status(),
        },
        Some(Commands::Claude { action }) => match action {
            ClaudeAction::Login => cmd_claude_login().await,
            ClaudeAction::Logout => cmd_claude_logout(),
            ClaudeAction::Status => cmd_claude_status(),
        },
    }
}

async fn run_server(config_path: String) -> anyhow::Result<()> {
    let cfg = config::Config::from_file(&config_path)?;
    let config = Arc::new(cfg);
    let router = Arc::new(Router::new(config.clone()));

    let state = Arc::new(AppState {
        router: router.clone(),
        anthropic_oauth: Arc::new(AnthropicOAuth::new()),
    });

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
        .route("/api/web/update-check", get(web::check_update))
        .route("/api/web/update-apply", post(web::apply_update))
        .route("/api/web/claude-status", get(web::claude_status))
        .route("/api/web/claude-login", post(web::claude_login))
        .route("/api/web/claude-logout", post(web::claude_logout))
        .route("/anthropic/oauth/callback", get(web::claude_callback))
        .with_state(state);

    let addr = format!("{}:{}", config.server.host, config.server.port);
    info!("mini-ai-router-rs listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn cmd_update_check() -> anyhow::Result<()> {
    let current = updater::current_version();
    println!("Current version: v{}", current);

    match updater::fetch_latest_release().await {
        Ok(release) => {
            println!("Latest version:  {}", release.tag_name);
            if updater::compare_versions(current, &release.tag_name)? == Some(true) {
                println!("Update available! Run 'update apply' to download.");
                if let Some(body) = &release.body {
                    println!("\nChangelog:\n{}", body);
                }
            } else {
                println!("You are up to date.");
            }
        }
        Err(e) => {
            println!("Failed to check for updates: {}", e);
        }
    }
    Ok(())
}

async fn cmd_update_apply() -> anyhow::Result<()> {
    let current = updater::current_version();
    println!("Current version: v{}", current);

    let release = updater::fetch_latest_release().await?;
    println!("Latest version:  {}", release.tag_name);

    match updater::compare_versions(current, &release.tag_name)? {
        Some(true) => {
            println!("Downloading {}...", release.tag_name);
            let bytes = updater::download_exe(&release).await?;
            println!("Downloaded {} bytes. Applying update...", bytes.len());
            updater::apply_update(&bytes)?;
            println!("Update applied! Please restart mini-ai-router-rs.");
            println!("Backup saved as: mini-ai-router-rs.exe.bak");
        }
        _ => {
            println!("You are already up to date (v{}).", current);
        }
    }
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

async fn cmd_claude_login() -> anyhow::Result<()> {
    let store = ClaudeTokenStore::new()?;
    if store.load()?.is_some() {
        println!("Already authenticated. Run 'claude logout' first to re-authenticate.");
        return Ok(());
    }

    let oauth = AnthropicOAuth::new();
    let port: u16 = 20228;
    let redirect_uri = format!("http://127.0.0.1:{}/anthropic/oauth/callback", port);

    let (authorize_url, _state) = oauth.build_authorize_url(&redirect_uri)?;

    println!("1. Opening browser for Claude authorization...");
    if let Err(_e) = open::that(&authorize_url) {
        println!("Could not open browser automatically.");
        println!("   Please open this URL manually:");
        println!("   {}", authorize_url);
    } else {
        println!("   URL: {}", authorize_url);
    }

    println!();
    println!("Waiting for authorization callback on http://127.0.0.1:{}/anthropic/oauth/callback", port);
    println!("Make sure the server is running to receive the callback.");
    println!("After authorizing, re-run 'claude status' to verify.");

    Ok(())
}

fn cmd_claude_logout() -> anyhow::Result<()> {
    let store = ClaudeTokenStore::new()?;
    let was_present = store.load()?.is_some();
    store.delete()?;
    if was_present {
        println!("✓ Logged out. Token removed.");
    } else {
        println!("No stored token found.");
    }
    Ok(())
}

fn cmd_claude_status() -> anyhow::Result<()> {
    let store = ClaudeTokenStore::new()?;
    let token_data = store.load()?;

    match token_data {
        Some(data) => {
            println!("authenticated: true");
            println!("token_present: true");
            println!("created_at: {}", data.created_at);
            println!("token_prefix: {}", mask_token(&data.access_token));
            println!("expires_in: {}s", data.expires_in);
        }
        None => {
            println!("authenticated: false");
            println!("token_present: false");
        }
    }

    Ok(())
}
