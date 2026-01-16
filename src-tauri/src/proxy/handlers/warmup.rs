// 预热处理器 - 内部预热 API
//
// 提供 /internal/warmup 端点，支持：
// - 指定账号（通过 email）
// - 指定模型（不做映射，直接使用原始模型名称）
// - 复用代理的所有基础设施（UpstreamClient、TokenManager）

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::proxy::mappers::gemini::wrapper::wrap_request;
use crate::proxy::server::AppState;

/// 预热请求体
#[derive(Debug, Deserialize)]
pub struct WarmupRequest {
    /// 账号邮箱
    pub email: String,
    /// 模型名称（原始名称，不做映射）
    pub model: String,
    /// 可选：直接提供 Access Token（用于不在 TokenManager 中的账号）
    pub access_token: Option<String>,
    /// 可选：直接提供 Project ID
    pub project_id: Option<String>,
}

/// 预热响应
#[derive(Debug, Serialize)]
pub struct WarmupResponse {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 处理预热请求
pub async fn handle_warmup(
    State(state): State<AppState>,
    Json(req): Json<WarmupRequest>,
) -> Response {
    info!(
        "[Warmup-API] ========== START: email={}, model={} ==========",
        req.email, req.model
    );

    // ===== 步骤 1: 获取 Token =====
    let (access_token, project_id) = if let (Some(at), Some(pid)) = (&req.access_token, &req.project_id) {
        (at.clone(), pid.clone())
    } else {
        match state.token_manager.get_token_by_email(&req.email).await {
            Ok((at, pid, _)) => (at, pid),
            Err(e) => {
                warn!(
                    "[Warmup-API] Step 1 FAILED: Token error for {}: {}",
                    req.email, e
                );
                return (
                    StatusCode::BAD_REQUEST,
                    Json(WarmupResponse {
                        success: false,
                        message: format!("Failed to get token for {}", req.email),
                        error: Some(e),
                    }),
                )
                    .into_response();
            }
        }
    };

    // ===== 步骤 2: 根据模型类型构建请求体 =====
    let is_claude = req.model.to_lowercase().contains("claude");
    let is_image = req.model.to_lowercase().contains("image");

    let body: Value = if is_claude {
        // Claude 模型：使用 transform_claude_request_in 转换
        let session_id = format!("warmup_{}_{}", 
            chrono::Utc::now().timestamp_millis(),
            &uuid::Uuid::new_v4().to_string()[..8]
        );
        let claude_request = crate::proxy::mappers::claude::models::ClaudeRequest {
            model: req.model.clone(),
            messages: vec![crate::proxy::mappers::claude::models::Message {
                role: "user".to_string(),
                content: crate::proxy::mappers::claude::models::MessageContent::String(
                    "ping".to_string(),
                ),
            }],
            max_tokens: Some(1),
            stream: false,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            tools: None,
            metadata: Some(crate::proxy::mappers::claude::models::Metadata {
                user_id: Some(session_id),
            }),
            thinking: None,
            output_config: None,
        };

        match crate::proxy::mappers::claude::transform_claude_request_in(
            &claude_request,
            &project_id,
        ) {
            Ok(transformed) => transformed,
            Err(e) => {
                warn!("[Warmup-API] Step 2 FAILED: Claude transform error: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(WarmupResponse {
                        success: false,
                        message: format!("Transform error: {}", e),
                        error: Some(e),
                    }),
                )
                    .into_response();
            }
        }
    } else {
        // Gemini 模型：使用 wrap_request
        let session_id = format!("warmup_{}_{}", 
            chrono::Utc::now().timestamp_millis(),
            &uuid::Uuid::new_v4().to_string()[..8]
        );

        let base_request = if is_image {
            json!({
                "model": req.model,
                "contents": [{"role": "user", "parts": [{"text": "Say hi"}]}],
                "generationConfig": {
                    "maxOutputTokens": 10,
                    "temperature": 0,
                    "responseModalities": ["TEXT"]
                },
                "session_id": session_id
            })
        } else {
            json!({
                "model": req.model,
                "contents": [{"role": "user", "parts": [{"text": "Say hi"}]}],
                "generationConfig": {
                    "temperature": 0
                },
                "session_id": session_id
            })
        };

        wrap_request(&base_request, &project_id, &req.model)
    };

    // ===== 步骤 3: 调用 UpstreamClient =====
    let model_lower = req.model.to_lowercase();
    let prefer_non_stream = model_lower.contains("flash-lite") || model_lower.contains("2.5-pro");

    let (method, query) = if prefer_non_stream {
        ("generateContent", None)
    } else {
        ("streamGenerateContent", Some("alt=sse"))
    };

    let mut result = state
        .upstream
        .call_v1_internal(method, &access_token, body.clone(), query)
        .await;

    // 如果流式请求失败，尝试非流式请求
    if result.is_err() && !prefer_non_stream {
        result = state
            .upstream
            .call_v1_internal("generateContent", &access_token, body, None)
            .await;
    }

    // ===== 步骤 4: 处理响应 =====
    match result {
        Ok(response) => {
            let status = response.status();
            let mut response = if status.is_success() {
                info!(
                    "[Warmup-API] ========== SUCCESS: {} / {} ==========",
                    req.email, req.model
                );
                (
                    StatusCode::OK,
                    Json(WarmupResponse {
                        success: true,
                        message: format!("Warmup triggered for {}", req.model),
                        error: None,
                    }),
                )
                    .into_response()
            } else {
                let status_code = status.as_u16();
                let error_text = response.text().await.unwrap_or_default();
                (
                    StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                    Json(WarmupResponse {
                        success: false,
                        message: format!("Warmup failed: HTTP {}", status_code),
                        error: Some(error_text),
                    }),
                )
                    .into_response()
            };

            // 添加响应头，让监控中间件捕获账号信息
            if let Ok(email_val) = axum::http::HeaderValue::from_str(&req.email) {
                response.headers_mut().insert("X-Account-Email", email_val);
            }
            if let Ok(model_val) = axum::http::HeaderValue::from_str(&req.model) {
                response.headers_mut().insert("X-Mapped-Model", model_val);
            }
            
            response
        }
        Err(e) => {
            warn!(
                "[Warmup-API] ========== ERROR: {} / {} - {} ==========",
                req.email, req.model, e
            );
            
            let mut response = (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WarmupResponse {
                    success: false,
                    message: "Warmup request failed".to_string(),
                    error: Some(e),
                }),
            ).into_response();

            // 即使失败也添加响应头，以便监控
            if let Ok(email_val) = axum::http::HeaderValue::from_str(&req.email) {
                response.headers_mut().insert("X-Account-Email", email_val);
            }
            if let Ok(model_val) = axum::http::HeaderValue::from_str(&req.model) {
                response.headers_mut().insert("X-Mapped-Model", model_val);
            }
            
            response
        }
    }
}
