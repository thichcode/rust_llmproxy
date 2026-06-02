use serde::{Deserialize, Serialize};
use tokio::time::{sleep, Duration};
use tracing::warn;

use crate::error::AppError;

pub const GITHUB_COPILOT_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPollResponse {
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotSessionToken {
    pub token: String,
    #[serde(default)]
    pub expires_at: Option<u64>,
    #[serde(default)]
    pub refresh_in: Option<u64>,
    #[serde(default)]
    pub endpoints: Option<CopilotEndpoints>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotEndpoints {
    #[serde(default)]
    pub api: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum AuthMode {
    Auto,
    RawOAuth,
    SessionToken,
}

#[allow(dead_code)]
impl AuthMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "raw_oauth" => AuthMode::RawOAuth,
            "session_token" => AuthMode::SessionToken,
            _ => AuthMode::Auto,
        }
    }
}

pub async fn request_device_code(client_id: &str) -> Result<DeviceCodeResponse, AppError> {
    let client = reqwest::Client::new();
    let params = [("client_id", client_id), ("scope", "read:user")];

    let response = client
        .post("https://github.com/login/device/code")
        .form(&params)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| AppError::Provider(format!("Device code request failed: {}", e)))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Provider(format!(
            "Device code request returned {}: {}",
            status, body
        )));
    }

    let device_resp: DeviceCodeResponse = response
        .json()
        .await
        .map_err(|e| AppError::Provider(format!("Failed to parse device code response: {}", e)))?;

    Ok(device_resp)
}

pub async fn poll_for_token(
    client_id: &str,
    device_code: &str,
    interval: u64,
    expires_in: u64,
) -> Result<TokenPollResponse, AppError> {
    let client = reqwest::Client::new();
    let mut current_interval = Duration::from_secs(interval);
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(expires_in);

    loop {
        if start.elapsed() >= timeout {
            return Err(AppError::Provider(
                "Device code expired. Please try again.".to_string(),
            ));
        }

        sleep(current_interval).await;

        let params = [
            ("client_id", client_id),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ];

        let resp = client
            .post("https://github.com/login/oauth/access_token")
            .form(&params)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| AppError::Provider(format!("Token poll request failed: {}", e)))?;

        let poll_resp: TokenPollResponse = resp
            .json()
            .await
            .map_err(|e| AppError::Provider(format!("Failed to parse poll response: {}", e)))?;

        if poll_resp.access_token.is_some() {
            return Ok(poll_resp);
        }

        if let Some(ref error) = poll_resp.error {
            match error.as_str() {
                "authorization_pending" => {
                    continue;
                }
                "slow_down" => {
                    current_interval += Duration::from_secs(5);
                    continue;
                }
                "expired_token" => {
                    return Err(AppError::Provider(
                        "Device code expired. Please run 'copilot login' again.".to_string(),
                    ));
                }
                "access_denied" => {
                    return Err(AppError::Provider("Access denied by user.".to_string()));
                }
                "incorrect_device_code" => {
                    return Err(AppError::Provider(
                        "Incorrect device code. Please run 'copilot login' again.".to_string(),
                    ));
                }
                _ => {
                    return Err(AppError::Provider(format!(
                        "Unknown polling error: {}",
                        error
                    )));
                }
            }
        }
    }
}

pub async fn exchange_copilot_session_token(
    github_access_token: &str,
) -> Result<CopilotSessionToken, AppError> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.github.com/copilot_internal/v2/token")
        .header("Authorization", format!("token {}", github_access_token))
        .header("Accept", "application/json")
        .header("Editor-Version", "mini-ai-router-rs/0.1.0")
        .header("Editor-Plugin-Version", "mini-ai-router-rs/0.1.0")
        .header("User-Agent", "mini-ai-router-rs/0.1.0")
        .send()
        .await
        .map_err(|e| AppError::Provider(format!("Copilot session token request failed: {}", e)))?;

    let status = resp.status();
    if status.is_success() {
        let session: CopilotSessionToken = resp.json().await.map_err(|e| {
            AppError::Provider(format!("Failed to parse session token response: {}", e))
        })?;
        Ok(session)
    } else if status.as_u16() == 403 || status.as_u16() == 404 || status.as_u16() == 401 {
        let body = resp.text().await.unwrap_or_default();
        warn!(
            "Copilot session token exchange returned {} (may fallback to raw OAuth): {}",
            status, body
        );
        Err(AppError::Provider(format!(
            "Session token exchange not supported ({}): {}",
            status, body
        )))
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(AppError::Provider(format!(
            "Session token exchange failed ({}): {}",
            status, body
        )))
    }
}

pub fn is_session_token_expired(session: &CopilotSessionToken) -> bool {
    match session.expires_at {
        Some(exp) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            exp <= now + 60
        }
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_token_not_expired() {
        let future = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + 3600;
        let session = CopilotSessionToken {
            token: "test".to_string(),
            expires_at: Some(future),
            refresh_in: None,
            endpoints: None,
        };
        assert!(!is_session_token_expired(&session));
    }

    #[test]
    fn test_session_token_expired() {
        let past = 1000000;
        let session = CopilotSessionToken {
            token: "test".to_string(),
            expires_at: Some(past),
            refresh_in: None,
            endpoints: None,
        };
        assert!(is_session_token_expired(&session));
    }

    #[test]
    fn test_session_token_no_expiry() {
        let session = CopilotSessionToken {
            token: "test".to_string(),
            expires_at: None,
            refresh_in: None,
            endpoints: None,
        };
        assert!(is_session_token_expired(&session));
    }

    #[test]
    fn test_auth_mode_from_str() {
        assert_eq!(AuthMode::from_str("auto"), AuthMode::Auto);
        assert_eq!(AuthMode::from_str("raw_oauth"), AuthMode::RawOAuth);
        assert_eq!(AuthMode::from_str("session_token"), AuthMode::SessionToken);
        assert_eq!(AuthMode::from_str("unknown"), AuthMode::Auto);
    }

    #[test]
    fn test_polling_error_parsing() {
        let resp = TokenPollResponse {
            access_token: None,
            token_type: None,
            scope: None,
            error: Some("authorization_pending".to_string()),
            error_description: None,
        };
        assert_eq!(resp.error.as_deref(), Some("authorization_pending"));

        let resp2 = TokenPollResponse {
            access_token: None,
            token_type: None,
            scope: None,
            error: Some("slow_down".to_string()),
            error_description: None,
        };
        assert_eq!(resp2.error.as_deref(), Some("slow_down"));

        let resp3 = TokenPollResponse {
            access_token: None,
            token_type: None,
            scope: None,
            error: Some("expired_token".to_string()),
            error_description: None,
        };
        assert_eq!(resp3.error.as_deref(), Some("expired_token"));
    }
}
