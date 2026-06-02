use async_trait::async_trait;
use reqwest::Client;
use tracing::warn;

use super::Provider;
use crate::config::ModelConfig;
use crate::error::AppError;
use crate::models::{ChatRequest, ProviderResponse};
use crate::transform::openai_anthropic;

pub struct AnthropicProvider;

#[async_trait]
impl Provider for AnthropicProvider {
    async fn send_message(
        &self,
        req: ChatRequest,
        config: &ModelConfig,
    ) -> Result<ProviderResponse, AppError> {
        let anthropic_req = openai_anthropic::to_anthropic_request(req, config.model.clone());
        let url = format!("{}/v1/messages", config.api_base.trim_end_matches('/'));

        let api_key = get_api_key(&config.api_key_env)?;

        let client = Client::new();
        let response = client
            .post(&url)
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&anthropic_req)
            .send()
            .await
            .map_err(|e| {
                warn!("Anthropic provider request failed: {}", e);
                AppError::Provider(format!("Request to Anthropic failed: {}", e))
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            warn!("Anthropic provider returned {}: {}", status, body);
            return Err(AppError::Provider(format!(
                "Anthropic returned status {}: {}",
                status, body
            )));
        }

        let body = response.text().await.map_err(|e| {
            warn!("Failed to read Anthropic response body: {}", e);
            AppError::Provider(format!("Failed to read response: {}", e))
        })?;

        let anthropic_resp: crate::models::AnthropicResponse = serde_json::from_str(&body)
            .map_err(|e| {
                warn!("Failed to parse Anthropic response: {}", e);
                AppError::Provider(format!("Failed to parse Anthropic response: {}", e))
            })?;

        let openai_resp = openai_anthropic::to_openai_response(&anthropic_resp, &config.model);
        let response_body = serde_json::to_string(&openai_resp).map_err(|e| {
            warn!("Failed to serialize OpenAI response: {}", e);
            AppError::Provider(format!("Failed to serialize response: {}", e))
        })?;

        Ok(ProviderResponse {
            body: Some(response_body),
        })
    }
}

fn get_api_key(env_var: &str) -> Result<String, AppError> {
    std::env::var(env_var)
        .map_err(|_| AppError::Config(format!("Environment variable {} is not set", env_var)))
}
