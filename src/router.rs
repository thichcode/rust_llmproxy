use std::sync::Arc;

use tracing::{info, warn};

use crate::config::{Config, RtkConfig};
use crate::error::AppError;
use crate::models::ChatRequest;
use crate::providers;
use crate::rtk;

pub struct Router {
    config: Arc<Config>,
}

pub struct RouteResult {
    pub body: String,
    pub rtk_applied: bool,
}

impl Router {
    pub fn new(config: Arc<Config>) -> Self {
        Router { config }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub async fn route(&self, mut req: ChatRequest) -> Result<RouteResult, AppError> {
        let model_name = if self.config.models.contains_key(&req.model) {
            req.model.clone()
        } else {
            info!(
                "Model '{}' not found in config, using default '{}'",
                req.model, self.config.default_model
            );
            self.config.default_model.clone()
        };

        let model_config = self
            .config
            .models
            .get(&model_name)
            .ok_or_else(|| AppError::ModelNotFound(model_name.clone()))?;

        let rtk_enabled = self.config.rtk.enabled;
        self.apply_rtk(&mut req);

        info!(
            "Routing request to model '{}' via provider '{}'",
            model_name, model_config.provider
        );
        let provider = providers::get_provider(&model_config.provider)?;

        let response = provider.send_message(req, model_config).await?;

        Ok(RouteResult {
            body: response.body.unwrap_or_default(),
            rtk_applied: rtk_enabled,
        })
    }

    fn apply_rtk(&self, req: &mut ChatRequest) {
        let rtk_config = RtkConfig {
            enabled: self.config.rtk.enabled,
            max_message_chars: self.config.rtk.max_message_chars,
            preserve_head_chars: self.config.rtk.preserve_head_chars,
            preserve_tail_chars: self.config.rtk.preserve_tail_chars,
        };

        if rtk_config.enabled {
            warn!("RTK compression is enabled, this may truncate long messages");
        }

        rtk::compress::compress_chat_request(req, &rtk_config);
    }
}
