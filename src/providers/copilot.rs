use std::sync::Mutex;

use async_trait::async_trait;
use reqwest::Client;
use tracing::{info, warn};

use super::Provider;
use crate::auth::github_device_oauth::{
    exchange_copilot_session_token, is_session_token_expired, AuthMode, CopilotSessionToken,
};
use crate::auth::token_store::{GithubTokenData, TokenStore};
use crate::error::AppError;
use crate::models::{ChatRequest, ProviderResponse};

pub struct CopilotProvider {
    token_store: TokenStore,
    session_cache: Mutex<Option<CopilotSessionCache>>,
}

struct CopilotSessionCache {
    token: CopilotSessionToken,
    mode: AuthMode,
}

#[allow(dead_code)]
impl CopilotProvider {
    pub fn new() -> Result<Self, AppError> {
        Ok(CopilotProvider {
            token_store: TokenStore::new()?,
            session_cache: Mutex::new(None),
        })
    }

    pub fn from_store(token_store: TokenStore) -> Self {
        CopilotProvider {
            token_store,
            session_cache: Mutex::new(None),
        }
    }

    fn get_github_token(&self) -> Result<GithubTokenData, AppError> {
        self.token_store.load()?.ok_or_else(|| {
            AppError::Provider(
                "GitHub Copilot not authenticated. Run 'mini-ai-router-rs copilot login' first."
                    .to_string(),
            )
        })
    }

    fn resolve_session_token(
        &self,
        github_token: &str,
    ) -> Result<(String, String, AuthMode), AppError> {
        let mut cache = self
            .session_cache
            .lock()
            .map_err(|e| AppError::Provider(format!("Session cache lock error: {}", e)))?;

        if let Some(ref cached) = *cache {
            if !is_session_token_expired(&cached.token) {
                let base_url = cached
                    .token
                    .endpoints
                    .as_ref()
                    .and_then(|e| e.api.clone())
                    .unwrap_or_else(|| "https://api.githubcopilot.com".to_string());
                return Ok((cached.token.token.clone(), base_url, cached.mode.clone()));
            }
        }

        let rt = tokio::runtime::Handle::current();

        match rt.block_on(exchange_copilot_session_token(github_token)) {
            Ok(session) => {
                let base_url = session
                    .endpoints
                    .as_ref()
                    .and_then(|e| e.api.clone())
                    .unwrap_or_else(|| "https://api.githubcopilot.com".to_string());
                info!("Copilot auth mode: session_token");
                let token = session.token.clone();
                *cache = Some(CopilotSessionCache {
                    token: session,
                    mode: AuthMode::SessionToken,
                });
                Ok((token, base_url, AuthMode::SessionToken))
            }
            Err(e) => {
                warn!(
                    "Session token exchange failed, falling back to raw OAuth: {}",
                    e
                );
                info!("Copilot auth mode: raw_oauth (fallback)");
                let token = github_token.to_string();
                let base_url = "https://api.individual.githubcopilot.com".to_string();
                *cache = Some(CopilotSessionCache {
                    token: CopilotSessionToken {
                        token: token.clone(),
                        expires_at: None,
                        refresh_in: None,
                        endpoints: None,
                    },
                    mode: AuthMode::RawOAuth,
                });
                Ok((token, base_url, AuthMode::RawOAuth))
            }
        }
    }
}

#[async_trait]
impl Provider for CopilotProvider {
    async fn send_message(
        &self,
        mut req: ChatRequest,
        config: &crate::config::ModelConfig,
    ) -> Result<ProviderResponse, AppError> {
        req.model = config.model.clone();

        let github_data = self.get_github_token()?;
        let (token, base_url, _mode) =
            self.resolve_session_token(&github_data.github_access_token)?;

        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let is_stream = req.stream.unwrap_or(false);

        let client = Client::new();
        let mut req_builder = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .header("Copilot-Integration-Id", "mini-ai-router-rs")
            .header("Editor-Version", "mini-ai-router-rs/0.1.0")
            .header("Editor-Plugin-Version", "mini-ai-router-rs/0.1.0")
            .header("User-Agent", "mini-ai-router-rs/0.1.0");

        if is_stream {
            req_builder = req_builder
                .header("Accept", "text/event-stream")
                .header("Accept", "application/json");
        } else {
            req_builder = req_builder.header("Accept", "application/json");
        }

        let response = req_builder.json(&req).send().await.map_err(|e| {
            warn!("Copilot provider request failed: {}", e);
            AppError::Provider(format!("Request to Copilot failed: {}", e))
        })?;

        let status = response.status();

        if status.as_u16() == 401 || status.as_u16() == 403 {
            let mut cache = self
                .session_cache
                .lock()
                .map_err(|e| AppError::Provider(format!("Session cache lock error: {}", e)))?;
            *cache = None;
            warn!(
                "Copilot returned {}, clearing session cache for retry",
                status
            );
            return Err(AppError::Provider(format!(
                "Copilot authentication failed ({}). Run 'copilot login' again.",
                status
            )));
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            warn!("Copilot provider returned {}: {}", status, body);
            return Err(AppError::Provider(format!(
                "Copilot returned status {}: {}",
                status, body
            )));
        }

        if is_stream {
            let body_bytes = response.bytes().await.map_err(|e| {
                warn!("Failed to read Copilot streaming body: {}", e);
                AppError::Provider(format!("Failed to read streaming response: {}", e))
            })?;
            let body_str = String::from_utf8_lossy(&body_bytes).to_string();
            Ok(ProviderResponse {
                body: Some(body_str),
            })
        } else {
            let body = response.text().await.map_err(|e| {
                warn!("Failed to read Copilot response body: {}", e);
                AppError::Provider(format!("Failed to read response: {}", e))
            })?;
            Ok(ProviderResponse { body: Some(body) })
        }
    }
}
