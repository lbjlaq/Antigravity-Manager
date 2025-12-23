//! 错误处理模块

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// 应用错误类型
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("未找到: {0}")]
    NotFound(String),

    #[error("参数错误: {0}")]
    BadRequest(String),

    #[error("未授权: {0}")]
    Unauthorized(String),

    #[error("服务器内部错误: {0}")]
    Internal(String),

    #[error("上游服务错误: {0}")]
    Upstream(String),

    #[error("配置错误: {0}")]
    Config(String),

    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            AppError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            AppError::Upstream(msg) => (StatusCode::BAD_GATEWAY, msg.clone()),
            AppError::Config(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            AppError::Anyhow(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };

        let body = Json(json!({
            "error": {
                "message": message,
                "type": error_type(&self),
            }
        }));

        (status, body).into_response()
    }
}

fn error_type(err: &AppError) -> &'static str {
    match err {
        AppError::NotFound(_) => "not_found",
        AppError::BadRequest(_) => "bad_request",
        AppError::Unauthorized(_) => "unauthorized",
        AppError::Internal(_) => "internal_error",
        AppError::Upstream(_) => "upstream_error",
        AppError::Config(_) => "config_error",
        AppError::Anyhow(_) => "internal_error",
    }
}

/// 结果类型别名
pub type AppResult<T> = Result<T, AppError>;
