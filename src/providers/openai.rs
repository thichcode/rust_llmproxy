use async_trait::async_trait;
use reqwest::Client;
use tracing::warn;

use super::Provider;
use crate::config::ModelConfig;
use crate::error::AppError;
use crate::models::{ChatRequest, ProviderResponse};

pub struct OpenAIProvider;

#[async_trait]
impl Provider for OpenAIProvider {
    async fn send_message(
        &self,
        mut req: ChatRequest,
        config: &ModelConfig,
    ) -> Result<ProviderResponse, AppError> {
        req.model = config.model.clone();
        let url = format!("{}/chat/completions", config.api_base.trim_end_matches('/'));

        let api_key = get_api_key(&config.api_key_env)?;
        let is_stream = req.stream.unwrap_or(false);

        let client = Client::new();
        let mut request_builder = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&req);

        if is_stream {
            request_builder = request_builder.header("Accept", "text/event-stream");
        }

        let response = request_builder.send().await.map_err(|e| {
            warn!("OpenAI provider request failed: {}", e);
            AppError::Provider(format!("Request to OpenAI failed: {}", e))
        })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            warn!("OpenAI provider returned {}: {}", status, body);
            return Err(AppError::Provider(format!(
                "OpenAI returned status {}: {}",
                status, body
            )));
        }

        if is_stream {
            let body_bytes = response.bytes().await.map_err(|e| {
                warn!("Failed to read streaming response body: {}", e);
                AppError::Provider(format!("Failed to read streaming response: {}", e))
            })?;

            let body_str = String::from_utf8_lossy(&body_bytes).to_string();
            Ok(ProviderResponse {
                body: Some(body_str),
            })
        } else {
            let body = response.text().await.map_err(|e| {
                warn!("Failed to read response body: {}", e);
                AppError::Provider(format!("Failed to read response: {}", e))
            })?;

            Ok(ProviderResponse { body: Some(body) })
        }
    }
}

fn get_api_key(env_var: &str) -> Result<String, AppError> {
    std::env::var(env_var)
        .map_err(|_| AppError::Config(format!("Environment variable {} is not set", env_var)))
}
