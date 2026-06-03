use std::sync::Arc;
use std::time::SystemTime;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::models::ChatRequest;
use crate::server::AppState;

pub async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

pub async fn list_models(State(state): State<Arc<AppState>>) -> Result<Json<Value>, AppError> {
    let config = state.router.config();
    let mut data = Vec::new();
    for (name, model_cfg) in &config.models {
        data.push(json!({
            "id": name,
            "object": "model",
            "created": SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            "owned_by": model_cfg.provider,
        }));
    }
    Ok(Json(json!({
        "object": "list",
        "data": data
    })))
}

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> Result<axum::response::Response, AppError> {
    let is_stream = req.stream.unwrap_or(false);
    let result = state.router.route(req).await?;

    if is_stream {
        let response = axum::response::Response::builder()
            .header("Content-Type", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .header("X-RTK-Applied", if result.rtk_applied { "true" } else { "false" })
            .body(axum::body::Body::from(result.body))
            .map_err(|e| {
                AppError::Provider(format!("Failed to build streaming response: {}", e))
            })?;
        Ok(response)
    } else {
        let value: Value = serde_json::from_str(&result.body)
            .map_err(|e| AppError::Provider(format!("Failed to parse provider response: {}", e)))?;

        let mut resp = json!({
            "rtk_applied": result.rtk_applied,
        });

        if let Value::Object(obj) = &mut resp {
            if let Value::Object(data) = &value {
                obj.extend(data.clone());
            }
        }

        Ok(Json(resp).into_response())
    }
}

pub async fn anthropic_messages(
    State(state): State<Arc<AppState>>,
    Json(anthropic_req): Json<crate::models::AnthropicRequest>,
) -> Result<axum::response::Response, AppError> {
    let chat_req = ChatRequest {
        model: anthropic_req.model.clone(),
        messages: vec![crate::models::ChatMessage {
            role: "user".to_string(),
            content: anthropic_req.messages.first().map(|m| m.content.clone()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        temperature: anthropic_req.temperature,
        max_tokens: anthropic_req.max_tokens,
        stream: None,
        extra: anthropic_req.extra,
    };

    let result = state.router.route(chat_req).await?;
    let value: Value = serde_json::from_str(&result.body)
        .map_err(|e| AppError::Provider(format!("Failed to parse provider response: {}", e)))?;
    Ok(Json(value).into_response())
}
