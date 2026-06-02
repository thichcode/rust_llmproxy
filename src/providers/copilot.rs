use async_trait::async_trait;

use super::Provider;
use crate::config::ModelConfig;
use crate::error::AppError;
use crate::models::{ChatRequest, ProviderResponse};

pub struct CopilotProvider;

#[async_trait]
impl Provider for CopilotProvider {
    async fn send_message(
        &self,
        _req: ChatRequest,
        _config: &ModelConfig,
    ) -> Result<ProviderResponse, AppError> {
        Err(AppError::NotImplemented(
            "GitHub Copilot provider is not implemented yet".to_string(),
        ))
    }
}
