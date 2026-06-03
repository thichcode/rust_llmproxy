use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use axum::response::Html;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::github_device_oauth::{
    poll_for_token, request_device_code, GITHUB_COPILOT_CLIENT_ID,
};
use crate::auth::token_store::{mask_token, GithubTokenData, TokenStore};
use crate::error::AppError;
use crate::router::Router;

pub async fn dashboard() -> Html<&'static str> {
    Html(include_str!("dashboard.html"))
}

#[derive(Serialize)]
pub struct WebModelsResponse {
    pub models: Vec<WebModelEntry>,
}

#[derive(Serialize)]
pub struct WebModelEntry {
    pub id: String,
    pub provider: String,
    pub model_id: String,
    pub api_base: String,
    pub auth_mode: String,
}

pub async fn web_models(
    State(router): State<Arc<Router>>,
) -> Result<Json<WebModelsResponse>, AppError> {
    let config = router.config();
    let mut models = Vec::new();
    for (name, cfg) in &config.models {
        models.push(WebModelEntry {
            id: name.clone(),
            provider: cfg.provider.clone(),
            model_id: cfg.model.clone(),
            api_base: cfg.api_base.clone(),
            auth_mode: cfg.copilot_auth_mode.clone(),
        });
    }
    Ok(Json(WebModelsResponse { models }))
}

#[derive(Serialize)]
pub struct CopilotStatusResponse {
    pub authenticated: bool,
    pub token_prefix: Option<String>,
    pub created_at: Option<u64>,
}

pub async fn copilot_status() -> Json<CopilotStatusResponse> {
    let store = match TokenStore::new() {
        Ok(s) => s,
        Err(_) => {
            return Json(CopilotStatusResponse {
                authenticated: false,
                token_prefix: None,
                created_at: None,
            })
        }
    };

    match store.load().unwrap_or(None) {
        Some(data) => Json(CopilotStatusResponse {
            authenticated: true,
            token_prefix: Some(mask_token(&data.github_access_token)),
            created_at: Some(data.created_at),
        }),
        None => Json(CopilotStatusResponse {
            authenticated: false,
            token_prefix: None,
            created_at: None,
        }),
    }
}

pub async fn copilot_login() -> Result<Json<Value>, AppError> {
    let store = TokenStore::new()?;
    if store.load()?.is_some() {
        return Err(AppError::Provider(
            "Already authenticated. Logout first.".to_string(),
        ));
    }

    let device = request_device_code(GITHUB_COPILOT_CLIENT_ID).await?;
    Ok(Json(json!({
        "verification_uri": device.verification_uri,
        "user_code": device.user_code,
        "device_code": device.device_code,
        "interval": device.interval,
        "expires_in": device.expires_in,
    })))
}

#[derive(Deserialize)]
pub struct CopilotPollRequest {
    pub device_code: String,
    pub interval: u64,
    pub expires_in: u64,
}

pub async fn copilot_poll(
    Json(req): Json<CopilotPollRequest>,
) -> Result<Json<Value>, AppError> {
    let token_resp = poll_for_token(
        GITHUB_COPILOT_CLIENT_ID,
        &req.device_code,
        req.interval,
        req.expires_in,
    )
    .await?;

    let access_token = token_resp
        .access_token
        .ok_or_else(|| AppError::Provider("No access token received".to_string()))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let token_data = GithubTokenData {
        github_access_token: access_token,
        token_type: token_resp.token_type.unwrap_or_else(|| "bearer".to_string()),
        scope: token_resp.scope,
        created_at: now,
    };

    let store = TokenStore::new()?;
    store.save(&token_data)?;

    Ok(Json(json!({"success": true})))
}

pub async fn copilot_logout() -> Result<Json<Value>, AppError> {
    let store = TokenStore::new()?;
    store.delete()?;
    Ok(Json(json!({"success": true})))
}
