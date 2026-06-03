use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::warn;

use crate::auth::token_store::AnthropicTokenData;
use crate::error::AppError;

pub const ANTHROPIC_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
#[allow(dead_code)]
pub const ANTHROPIC_API_BASE: &str = "https://api.anthropic.com";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: u64,
    #[serde(default)]
    token_type: String,
}

pub struct AnthropicOAuth {
    sessions: Mutex<HashMap<String, PkceSession>>,
}

pub(crate) struct PkceSession {
    code_verifier: String,
    redirect_uri: String,
    created_at: u64,
}

impl AnthropicOAuth {
    pub fn new() -> Self {
        AnthropicOAuth {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn build_authorize_url(&self, redirect_uri: &str) -> Result<(String, String), AppError> {
        let verifier: String = (0..64)
            .map(|_| {
                let idx = rand::random::<usize>() % 16;
                std::char::from_digit(idx as u32, 16).unwrap()
            })
            .collect();

        let challenge = {
            let mut hasher = Sha256::new();
            hasher.update(verifier.as_bytes());
            base64_url_encode(&hasher.finalize())
        };

        let state: String = (0..32)
            .map(|_| {
                let idx = rand::random::<usize>() % 16;
                std::char::from_digit(idx as u32, 16).unwrap()
            })
            .collect();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        {
            let mut sessions = self.sessions.lock().unwrap();
            sessions.retain(|_, s| now - s.created_at < 300);
            sessions.insert(
                state.clone(),
                PkceSession {
                    code_verifier: verifier,
                    redirect_uri: redirect_uri.to_string(),
                    created_at: now,
                },
            );
        }

        let params = format!(
            "response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state={}&scope={}",
            ANTHROPIC_CLIENT_ID,
            urlencoding::encode(redirect_uri),
            challenge,
            urlencoding::encode(&state),
            urlencoding::encode("user:profile user:sessions:claude_code"),
        );

        Ok((format!("{}?{}", AUTHORIZE_URL, params), state))
    }

    pub fn take_session(&self, state: &str) -> Option<PkceSession> {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.remove(state)
    }

    pub async fn exchange_code(
        &self,
        code: &str,
        state: &str,
    ) -> Result<AnthropicTokenData, AppError> {
        let session = self
            .take_session(state)
            .ok_or_else(|| AppError::Provider("OAuth state not found or expired".to_string()))?;

        let client = reqwest::Client::new();
        let params = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &session.redirect_uri),
            ("client_id", ANTHROPIC_CLIENT_ID),
            ("code_verifier", &session.code_verifier),
        ];

        let resp = client
            .post(TOKEN_URL)
            .form(&params)
            .send()
            .await
            .map_err(|e| {
                warn!("Token exchange request failed: {}", e);
                AppError::Provider(format!("Token exchange failed: {}", e))
            })?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            warn!("Token exchange returned {}: {}", status, body);
            return Err(AppError::Provider(format!(
                "Token exchange returned {}: {}",
                status, body
            )));
        }

        let token: TokenResponse = resp.json().await.map_err(|e| {
            warn!("Failed to parse token response: {}", e);
            AppError::Provider(format!("Failed to parse token response: {}", e))
        })?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(AnthropicTokenData {
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_in: token.expires_in,
            created_at: now,
        })
    }

    #[allow(dead_code)]
    pub async fn refresh_access_token(
        refresh_token: &str,
    ) -> Result<AnthropicTokenData, AppError> {
        let client = reqwest::Client::new();
        let params = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", ANTHROPIC_CLIENT_ID),
        ];

        let resp = client
            .post(TOKEN_URL)
            .form(&params)
            .send()
            .await
            .map_err(|e| {
                warn!("Token refresh request failed: {}", e);
                AppError::Provider(format!("Token refresh failed: {}", e))
            })?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            warn!("Token refresh returned {}: {}", status, body);
            return Err(AppError::Provider(format!(
                "Token refresh returned {}: {}",
                status, body
            )));
        }

        let token: TokenResponse = resp.json().await.map_err(|e| {
            warn!("Failed to parse refresh response: {}", e);
            AppError::Provider(format!("Failed to parse refresh response: {}", e))
        })?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(AnthropicTokenData {
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_in: token.expires_in,
            created_at: now,
        })
    }
}

fn base64_url_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}
