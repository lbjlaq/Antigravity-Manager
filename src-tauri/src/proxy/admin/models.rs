use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

/// 统一响应格式包装器
#[derive(Debug, Clone, Serialize)]
pub struct Envelope<T> {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
    // 向后兼容：旧版前端期望 {success, message}
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

impl<T> Envelope<T> {
    pub fn ok(data: T) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
            success: None,
            message: None,
        }
    }

    pub fn err(code: impl Into<String>, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            ok: false,
            data: None,
            error: Some(ErrorBody {
                code: code.into(),
                message: message.clone(),
            }),
            success: Some(false),
            message: Some(message),
        }
    }
}

/// 账号 DTO
#[derive(Debug, Clone, Serialize)]
pub struct AccountDto {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub is_active: bool,
    pub quota: Option<QuotaDto>,
    pub created_at: i64,
    pub last_used: i64,
}

/// 配额 DTO
#[derive(Debug, Clone, Serialize)]
pub struct QuotaDto {
    pub models: Vec<ModelQuotaDto>,
    pub is_forbidden: bool,
    pub last_updated: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelQuotaDto {
    pub name: String,
    pub percentage: i32,
    pub reset_time: String,
}

/// 代理配置 DTO（不包含敏感的 api_key）
#[derive(Debug, Clone, Serialize)]
pub struct ProxyConfigDto {
    pub enabled: bool,
    pub port: u16,
    pub allow_lan_access: bool,
    pub request_timeout: u64,
    pub upstream_proxy: UpstreamProxyDto,
    pub anthropic_mapping: std::collections::HashMap<String, String>,
    pub openai_mapping: std::collections::HashMap<String, String>,
    pub custom_mapping: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpstreamProxyDto {
    pub enabled: bool,
    pub url: String,
}

/// 服务状态 DTO
#[derive(Debug, Clone, Serialize)]
pub struct StatusDto {
    pub running: bool,
    pub version: String,
    pub uptime_seconds: u64,
    pub active_accounts: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned_account_id: Option<String>,
}

/// Admin API 错误类型
#[derive(Debug, thiserror::Error)]
pub enum AdminError {
    #[error("Unauthorized")]
    Unauthorized,
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Internal(String),
}

impl AdminError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    fn code(&self) -> &'static str {
        match self {
            AdminError::Unauthorized => "unauthorized",
            AdminError::BadRequest(_) => "bad_request",
            AdminError::NotFound(_) => "not_found",
            AdminError::Internal(_) => "internal",
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            AdminError::Unauthorized => StatusCode::UNAUTHORIZED,
            AdminError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AdminError::NotFound(_) => StatusCode::NOT_FOUND,
            AdminError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for AdminError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = Envelope::<serde_json::Value>::err(self.code(), self.to_string());
        (status, Json(body)).into_response()
    }
}
