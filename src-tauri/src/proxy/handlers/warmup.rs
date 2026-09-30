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
use tracing::{error, info, warn};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_server_error: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
}

/// 处理预热请求
pub async fn handle_warmup(
    State(state): State<AppState>,
    Json(req): Json<WarmupRequest>,
) -> Response {
    let start_time = std::time::Instant::now();

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
                    error!(
                        "[Warmup-API] Step 1 FAILED (non-server fault): Token error for {}: {}",
                        req.email, e
                    );
                    let duration = start_time.elapsed().as_millis() as u64;
                    let err_msg = format!("获取账号 Token 失败（非服务端故障）: {}", e);
                    let log = ProxyRequestLog {
                        id: uuid::Uuid::new_v4().to_string(),
                        timestamp: chrono::Utc::now().timestamp_millis(),
                        method: "POST".to_string(),
                        url: format!("/internal/warmup -> {}", req.model),
                        status: 400,
                        duration,
                        model: Some(req.model.clone()),
                        mapped_model: Some(req.model.clone()),
                        account_email: Some(req.email.clone()),
                        client_ip: Some("127.0.0.1".to_string()),
                        error: Some(format!("Token Error (非服务端故障): {}", e)),
                        request_body: Some(format!(
                            "{{\"type\": \"warmup\", \"model\": \"{}\"}}",
                            req.model
                        )),
                        upstream_request_body: None,
                        response_body: Some(err_msg.clone()),
                        input_tokens: Some(0),
                        output_tokens: Some(0),
                        cached_tokens: None,
                        protocol: Some("warmup".to_string()),
                        username: None,
                        request_headers: None,
                        upstream_request_headers: None,
                        response_headers: None,
                        session_id: None,
                    };
                    state.monitor.log_request(log).await;

                    return (
                        StatusCode::BAD_REQUEST,
                        Json(WarmupResponse {
                            success: false,
                            message: err_msg,
                            error: Some(e),
                            is_server_error: Some(false),
                            error_type: Some("token_error".to_string()),
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
            tool_choice: None,
        };

        match crate::proxy::mappers::claude::transform_claude_request_in(
            &claude_request,
            &project_id,
            false,
            None,
            "warmup",
            None, // [NEW] No token for warmup
        ) {
            Ok(transformed) => transformed,
            Err(e) => {
                error!(
                    "[Warmup-API] Step 2 FAILED (non-server fault): Claude transform error: {}",
                    e
                );
                let duration = start_time.elapsed().as_millis() as u64;
                let err_msg = format!("请求报文转换失败（非服务端故障）: {}", e);
                let log = ProxyRequestLog {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    method: "POST".to_string(),
                    url: format!("/internal/warmup -> {}", req.model),
                    status: 400,
                    duration,
                    model: Some(req.model.clone()),
                    mapped_model: Some(req.model.clone()),
                    account_email: Some(req.email.clone()),
                    client_ip: Some("127.0.0.1".to_string()),
                    error: Some(format!("Transform Error (非服务端故障): {}", e)),
                    request_body: Some(format!(
                        "{{\"type\": \"warmup\", \"model\": \"{}\"}}",
                        req.model
                    )),
                    upstream_request_body: None,
                    response_body: Some(err_msg.clone()),
                    input_tokens: Some(0),
                    output_tokens: Some(0),
                    cached_tokens: None,
                    protocol: Some("warmup".to_string()),
                    username: None,
                    request_headers: None,
                    upstream_request_headers: None,
                    response_headers: None,
                    session_id: None,
                };
                state.monitor.log_request(log).await;

                return (
                    StatusCode::BAD_REQUEST,
                    Json(WarmupResponse {
                        success: false,
                        message: err_msg,
                        error: Some(e),
                        is_server_error: Some(false),
                        error_type: Some("transform_error".to_string()),
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

        wrap_request(
            &base_request,
            &project_id,
            &req.model,
            None,
            Some(&session_id),
            None,
        ) // [FIX] Added None for token param
    };

    // ===== 步骤 3: 调用 UpstreamClient =====
    let upstream_req_body = serde_json::to_string_pretty(&body)
        .or_else(|_| serde_json::to_string(&body))
        .ok();

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

            let mut response = if status.is_success() {
                let response_text = response.text().await.unwrap_or_default();
                info!(
                    "[Warmup-API] ========== SUCCESS: {} / {} ({}ms) ==========",
                    req.email, req.model, duration
                );

                // 记录成功预热请求到流量日志
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
                    error: None,
                    request_body: Some(format!(
                        "{{\"type\": \"warmup\", \"model\": \"{}\"}}",
                        req.model
                    )),
                    upstream_request_body: upstream_req_body.clone(),
                    response_body: Some(if response_text.is_empty() {
                        "OK".to_string()
                    } else {
                        response_text
                    }),
                    input_tokens: Some(0),
                    output_tokens: Some(0),
                    cached_tokens: None,
                    protocol: Some("warmup".to_string()),
                    username: None,
                    request_headers: None,
                    upstream_request_headers: None,
                    response_headers: None,
                    session_id: None,
                };
                state.monitor.log_request(log).await;

                (
                    StatusCode::OK,
                    Json(WarmupResponse {
                        success: true,
                        message: format!("Warmup triggered for {}", req.model),
                        error: None,
                        is_server_error: None,
                        error_type: None,
                    }),
                )
                    .into_response()
            } else {
                let error_text = response.text().await.unwrap_or_default();

                // 记录内部错误日志，让 error.log 抓到该问题
                error!(
                    "[Warmup-API] Upstream returned error (non-server fault): email={}, model={}, status={}, error={}",
                    req.email, req.model, status_code, error_text
                );

                // [FIX] 预热阶段检测到 403 时，排查是否属于真正的账号封禁
                if status_code == 403 {
                    let is_user_location_error =
                        error_text.contains("User location is not supported");
                    let is_waf_rate_limit = error_text.contains("Cloud Armor")
                        || error_text.contains("Resource has been exhausted")
                        || error_text.contains("rate limit")
                        || error_text.contains("RESOURCE_EXHAUSTED");

                    if !is_user_location_error && !is_waf_rate_limit {
                        let resolved_account_id = if !account_id.is_empty() {
                            account_id.clone()
                        } else {
                            crate::modules::account::find_account_id_by_email(&req.email)
                                .unwrap_or_default()
                        };

                        if !resolved_account_id.is_empty() {
                            warn!(
                                "[Warmup-API] 403 Forbidden detected for {}, marking account as forbidden",
                                req.email
                            );
                            let _ = crate::modules::account::mark_account_forbidden(
                                &resolved_account_id,
                                &error_text,
                            );
                        } else {
                            warn!(
                                "[Warmup-API] 403 Forbidden detected for {} but could not resolve account_id, skipping mark",
                                req.email
                            );
                        }
                    } else {
                        warn!(
                            "[Warmup-API] 403 Forbidden detected for {} but identified as location or WAF burst rate-limit, skipping mark_account_forbidden",
                            req.email
                        );
                    }
                }

                // 记录失败的预热请求到流量日志（包含完整转发报文与响应报文）
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
                    error: Some(format!("HTTP {} (上游拒绝，非服务端故障)", status_code)),
                    request_body: Some(format!(
                        "{{\"type\": \"warmup\", \"model\": \"{}\"}}",
                        req.model
                    )),
                    upstream_request_body: upstream_req_body.clone(),
                    response_body: Some(error_text.clone()),
                    input_tokens: Some(0),
                    output_tokens: Some(0),
                    cached_tokens: None,
                    protocol: Some("warmup".to_string()),
                    username: None,
                    request_headers: None,
                    upstream_request_headers: None,
                    response_headers: None,
                    session_id: None,
                };
                state.monitor.log_request(log).await;

                (
                    StatusCode::from_u16(status_code).unwrap_or(StatusCode::BAD_GATEWAY),
                    Json(WarmupResponse {
                        success: false,
                        message: format!(
                            "Warmup failed: HTTP {} (上游拒绝，非服务端故障)",
                            status_code
                        ),
                        error: Some(error_text),
                        is_server_error: Some(false),
                        error_type: Some("upstream_error".to_string()),
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
            // 关键：升级为 ERROR 级别，写入 error.log
            error!(
                "[Warmup-API] ========== ERROR (non-server fault): {} / {} - {} ({}ms) ==========",
                req.email, req.model, e, duration
            );

            // 记录失败的预热请求到流量日志
            let log = ProxyRequestLog {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp: chrono::Utc::now().timestamp_millis(),
                method: "POST".to_string(),
                url: format!("/internal/warmup -> {}", req.model),
                status: 502,
                duration,
                model: Some(req.model.clone()),
                mapped_model: Some(req.model.clone()),
                account_email: Some(req.email.clone()),
                client_ip: Some("127.0.0.1".to_string()),
                error: Some(format!("Network Error (网络请求异常，非服务端故障): {}", e)),
                request_body: Some(format!(
                    "{{\"type\": \"warmup\", \"model\": \"{}\"}}",
                    req.model
                )),
                upstream_request_body: upstream_req_body,
                response_body: Some(format!("网络请求异常（非服务端故障）: {}", e)),
                input_tokens: None,
                output_tokens: None,
                cached_tokens: None,
                protocol: Some("warmup".to_string()),
                username: None,
                request_headers: None,
                upstream_request_headers: None,
                response_headers: None,
                session_id: None,
            };
            state.monitor.log_request(log).await;

            let mut response = (
                StatusCode::BAD_GATEWAY,
                Json(WarmupResponse {
                    success: false,
                    message: format!("网络请求失败（非服务端故障）: {}", e),
                    error: Some(e),
                    is_server_error: Some(false),
                    error_type: Some("network_error".to_string()),
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
