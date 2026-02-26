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
use crate::proxy::monitor::ProxyRequestLog;
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
    let start_time = std::time::Instant::now();

    // ===== 前置检查：跳过 gemini-2.5-* 家族模型 =====
    let model_lower = req.model.to_lowercase();
    if model_lower.contains("2.5-") || model_lower.contains("2-5-") {
        info!(
            "[Warmup-API] SKIP: gemini-2.5-* model not supported for warmup: {} / {}",
            req.email, req.model
        );
        return (
            StatusCode::OK,
            Json(WarmupResponse {
                success: true,
                message: format!(
                    "Skipped warmup for {} (2.5 models not supported)",
                    req.model
                ),
                error: None,
            }),
        )
            .into_response();
    }

    info!(
        "[Warmup-API] ========== START: email={}, model={} ==========",
        req.email, req.model
    );

    // ===== 步骤 1: 获取 Token =====
    let (access_token, project_id, account_id) =
        if let (Some(at), Some(pid)) = (&req.access_token, &req.project_id) {
            (at.clone(), pid.clone(), String::new())
        } else {
            match state.token_manager.get_token_by_email(&req.email).await {
                Ok((at, pid, _, acc_id, _wait_ms)) => (at, pid, acc_id),
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
        let session_id = format!(
            "warmup_{}_{}",
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
            size: None,
            quality: None,
        };

        match crate::proxy::mappers::claude::transform_claude_request_in(
            &claude_request,
            &project_id,
            false,
            None,
            "warmup",
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
        let session_id = format!(
            "warmup_{}_{}",
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

        wrap_request(&base_request, &project_id, &req.model, None, Some(&session_id), None)
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
        .call_v1_internal(
            method,
            &access_token,
            body.clone(),
            query,
            Some(account_id.as_str()),
        )
        .await;

    // 如果流式请求失败，尝试非流式请求
    if result.is_err() && !prefer_non_stream {
        result = state
            .upstream
            .call_v1_internal(
                "generateContent",
                &access_token,
                body,
                None,
                Some(account_id.as_str()),
            )
            .await;
    }

    let duration = start_time.elapsed().as_millis() as u64;

    // ===== 步骤 4: 处理响应并记录流量日志 =====
    match result {
        Ok(call_result) => {
            let response = call_result.response;
            let status = response.status();
            let status_code = status.as_u16();

            // 记录预热请求到流量日志
            let log = ProxyRequestLog {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp: chrono::Utc::now().timestamp_millis(),
                method: "POST".to_string(),
                url: format!("/internal/warmup -> {}", req.model),
                status: status_code,
                duration,
                model: Some(req.model.clone()),
                mapped_model: Some(req.model.clone()),
                account_email: Some(req.email.clone()),
                client_ip: Some("127.0.0.1".to_string()),
                error: if status.is_success() {
                    None
                } else {
                    Some(format!("HTTP {}", status_code))
                },
                request_body: Some(format!(
                    "{{\"type\": \"warmup\", \"model\": \"{}\"}}",
                    req.model
                )),
                response_body: None,
                input_tokens: Some(0),
                output_tokens: Some(0),
                protocol: Some("warmup".to_string()),
                username: None,
            };
            state.monitor.log_request(log).await;

            let mut response = if status.is_success() {
                info!(
                    "[Warmup-API] ========== SUCCESS: {} / {} ({}ms) ==========",
                    req.email, req.model, duration
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
                let error_text = response.text().await.unwrap_or_default();

                // [FIX] 预热阶段检测到 403 时，标记账号为 forbidden，避免无效账号继续参与轮询
                // 如果 account_id 为空（直接传入 access_token 的场景），通过 email 从索引中找到 ID
                if status_code == 403 {
                    let resolved_account_id = if !account_id.is_empty() {
                        account_id.clone()
                    } else {
                        // 尝试通过 email 查找账号 ID
                        crate::modules::account::find_account_id_by_email(&req.email)
                            .unwrap_or_default()
                    };

                    if !resolved_account_id.is_empty() {
                        warn!(
                            "[Warmup-API] 403 Forbidden detected for {}, marking account as forbidden",
                            req.email
                        );
                        let _ = crate::modules::account::mark_account_forbidden(&resolved_account_id, &error_text);
                    } else {
                        warn!(
                            "[Warmup-API] 403 Forbidden detected for {} but could not resolve account_id, skipping mark",
                            req.email
                        );
                    }
                }

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
                "[Warmup-API] ========== ERROR: {} / {} - {} ({}ms) ==========",
                req.email, req.model, e, duration
            );

            // 记录失败的预热请求到流量日志
            let log = ProxyRequestLog {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp: chrono::Utc::now().timestamp_millis(),
                method: "POST".to_string(),
                url: format!("/internal/warmup -> {}", req.model),
                status: 500,
                duration,
                model: Some(req.model.clone()),
                mapped_model: Some(req.model.clone()),
                account_email: Some(req.email.clone()),
                client_ip: Some("127.0.0.1".to_string()),
                error: Some(e.clone()),
                request_body: Some(format!(
                    "{{\"type\": \"warmup\", \"model\": \"{}\"}}",
                    req.model
                )),
                response_body: None,
                input_tokens: None,
                output_tokens: None,
                protocol: Some("warmup".to_string()),
                username: None,
            };
            state.monitor.log_request(log).await;

            let mut response = (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WarmupResponse {
                    success: false,
                    message: "Warmup request failed".to_string(),
                    error: Some(e),
                }),
            )
                .into_response();

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
