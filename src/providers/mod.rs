pub mod anthropic;
pub mod copilot;
pub mod openai;
pub mod phi4;

use async_trait::async_trait;

use crate::config::ModelConfig;
use crate::error::AppError;
use crate::models::{ChatRequest, ProviderResponse};

#[async_trait]
pub trait Provider: Send + Sync {
    async fn send_message(
        &self,
        req: ChatRequest,
        config: &ModelConfig,
    ) -> Result<ProviderResponse, AppError>;
}

pub fn get_provider(provider_type: &str) -> Result<Box<dyn Provider>, AppError> {
    match provider_type {
        "openai" => Ok(Box::new(openai::OpenAIProvider)),
        "anthropic" => Ok(Box::new(anthropic::AnthropicProvider)),
        "copilot" => Ok(Box::new(copilot::CopilotProvider::new()?)),
        "phi4" => Ok(Box::new(phi4::Phi4Provider)),
        other => Err(AppError::Config(format!(
            "Unknown provider type: {}",
            other
        ))),
    }
}
