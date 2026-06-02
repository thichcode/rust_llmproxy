use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("Config error: {0}")]
    Config(String),

    #[error("Provider error: {0}")]
    Provider(String),

    #[error("HTTP client error: {0}")]
    HttpClient(#[from] reqwest::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Model '{0}' not found")]
    ModelNotFound(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::Config(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            AppError::Provider(msg) => (StatusCode::BAD_GATEWAY, msg.clone()),
            AppError::HttpClient(e) => (StatusCode::BAD_GATEWAY, e.to_string()),
            AppError::Serde(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            AppError::ModelNotFound(m) => {
                (StatusCode::NOT_FOUND, format!("Model '{}' not found", m))
            }
        };

        let body = Json(json!({
            "error": {
                "message": message,
                "type": format!("{:?}", self)
            }
        }));

        (status, body).into_response()
    }
}
