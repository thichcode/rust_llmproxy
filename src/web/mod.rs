use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::github_device_oauth::{
    poll_for_token, request_device_code, GITHUB_COPILOT_CLIENT_ID,
};
use crate::auth::token_store::{mask_token, ClaudeTokenStore, GithubTokenData, TokenStore};
use crate::error::AppError;
use crate::server::AppState;

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
    State(state): State<Arc<AppState>>,
) -> Result<Json<WebModelsResponse>, AppError> {
    let config = state.router.config();
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

pub async fn check_update() -> Result<Json<Value>, AppError> {
    let current = crate::updater::current_version();
    match crate::updater::fetch_latest_release().await {
        Ok(release) => {
            let has_update = crate::updater::compare_versions(current, &release.tag_name)?
                .unwrap_or(false);
            Ok(Json(json!({
                "current_version": current,
                "latest_version": release.tag_name,
                "has_update": has_update,
                "changelog": release.body,
            })))
        }
        Err(e) => Ok(Json(json!({
            "current_version": current,
            "has_update": false,
            "error": e.to_string(),
        }))),
    }
}

pub async fn apply_update() -> Result<Json<Value>, AppError> {
    let current = crate::updater::current_version();
    let release = crate::updater::fetch_latest_release().await?;

    if crate::updater::compare_versions(current, &release.tag_name)? != Some(true) {
        return Ok(Json(json!({
            "success": false,
            "message": "Already up to date."
        })));
    }

    let bytes = crate::updater::download_exe(&release).await?;
    crate::updater::apply_update(&bytes)?;

    Ok(Json(json!({
        "success": true,
        "message": format!("Update to {} applied. Please restart the server.", release.tag_name),
        "current_version": current,
        "new_version": release.tag_name,
    })))
}

pub async fn copilot_logout() -> Result<Json<Value>, AppError> {
    let store = TokenStore::new()?;
    store.delete()?;
    Ok(Json(json!({"success": true})))
}

// -- Claude / Anthropic OAuth endpoints --

#[derive(Serialize)]
pub struct ClaudeStatusResponse {
    pub authenticated: bool,
    pub token_prefix: Option<String>,
    pub created_at: Option<u64>,
}

pub async fn claude_status() -> Json<ClaudeStatusResponse> {
    let store = match ClaudeTokenStore::new() {
        Ok(s) => s,
        Err(_) => {
            return Json(ClaudeStatusResponse {
                authenticated: false,
                token_prefix: None,
                created_at: None,
            })
        }
    };

    match store.load().unwrap_or(None) {
        Some(data) => Json(ClaudeStatusResponse {
            authenticated: true,
            token_prefix: Some(mask_token(&data.access_token)),
            created_at: Some(data.created_at),
        }),
        None => Json(ClaudeStatusResponse {
            authenticated: false,
            token_prefix: None,
            created_at: None,
        }),
    }
}

#[derive(Deserialize)]
pub struct ClaudeLoginRequest {
    pub redirect_uri: String,
}

pub async fn claude_login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ClaudeLoginRequest>,
) -> Result<Json<Value>, AppError> {
    let store = ClaudeTokenStore::new()?;
    if store.load()?.is_some() {
        return Err(AppError::Provider(
            "Already authenticated. Logout first.".to_string(),
        ));
    }

    let (authorize_url, _state) = state
        .anthropic_oauth
        .build_authorize_url(&req.redirect_uri)?;

    Ok(Json(json!({
        "authorize_url": authorize_url,
    })))
}

pub async fn claude_logout() -> Result<Json<Value>, AppError> {
    let store = ClaudeTokenStore::new()?;
    store.delete()?;
    Ok(Json(json!({"success": true})))
}

#[derive(Deserialize)]
pub struct ClaudeCallbackParams {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

pub async fn claude_callback(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ClaudeCallbackParams>,
) -> Result<axum::response::Response, AppError> {
    if let Some(err) = &params.error {
        return Ok(Html(format!(
            r#"<html><body><h2>OAuth Error</h2><p>{}</p><script>window.close()</script></body></html>"#,
            err
        ))
        .into_response());
    }

    let code = params
        .code
        .ok_or_else(|| AppError::Provider("Missing authorization code".to_string()))?;
    let oauth_state = params
        .state
        .ok_or_else(|| AppError::Provider("Missing state parameter".to_string()))?;

    let token_data = state
        .anthropic_oauth
        .exchange_code(&code, &oauth_state)
        .await?;

    let store = ClaudeTokenStore::new()?;
    store.save(&token_data)?;

    Ok(Html(
        r#"<html><body><h2>Authentication successful!</h2><p>You can close this tab.</p><script>
            if (window.opener) {
                window.opener.postMessage({type: 'claude-oauth-complete'}, '*');
            }
            window.close();
        </script></body></html>"#,
    )
    .into_response())
}
