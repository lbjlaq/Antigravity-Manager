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
    /// 可选：直接提供 Account ID
    pub account_id: Option<String>,
    /// 诊断/定向测活意图（为 true 时测活成功允许解除单模型冻结与验证锁）
    #[serde(default)]
    pub probe_intent: bool,
}

/// 预热响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarmupResponse {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_retired: Option<bool>,
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

    let mapped_model = {
        let custom_mapping = state.custom_mapping.read().await;
        crate::proxy::common::model_mapping::resolve_model_route_with_effort(
            &req.model,
            &custom_mapping,
            None,
        )
    };

    info!(
        "[Warmup-API] ========== START: email={}, model={} (mapped={}) ==========",
        req.email, req.model, mapped_model
    );

    // ===== 步骤 1: 获取 Token =====
    let (access_token, project_id, account_id) =
        if let (Some(at), Some(pid)) = (&req.access_token, &req.project_id) {
            let acc_id = req.account_id.clone().unwrap_or_else(|| {
                crate::modules::account::find_account_id_by_email(&req.email).unwrap_or_default()
            });
            (at.clone(), pid.clone(), acc_id)
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
                    let log_id = uuid::Uuid::new_v4().to_string();
                    let log = ProxyRequestLog {
                        id: log_id.clone(),
                        timestamp: chrono::Utc::now().timestamp_millis(),
                        method: "POST".to_string(),
                        url: format!("/internal/warmup -> {}", req.model),
                        status: 400,
                        duration,
                        model: Some(req.model.clone()),
                        mapped_model: Some(mapped_model.clone()),
                        upstream_model: Some(mapped_model.clone()),
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
                        reasoning_tokens: None,
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
                            log_id: Some(log_id),
                            duration_ms: Some(duration),
                            status: Some(400),
                            account_email: Some(req.email.clone()),
                            account_id: req.account_id.clone(),
                            is_retired: None,
                            error: Some(e),
                            is_server_error: Some(false),
                            error_type: Some("token_error".to_string()),
                        }),
                    )
                        .into_response();
                }
            }
        };

    // [测试专用] 模拟上游未响应 / 无请求回报（网络连接超时或分阶段传输中断）
    if req.model == "simulate_no_response" || req.model == "simulate-no-response" {
        let duration = 15000;
        let log_id = uuid::Uuid::new_v4().to_string();
        let diag = "请求上游接口时网络连接失败，未收到上游请求回报（非网关服务端故障，请排查网络或代理）：Upstream send failed at https://daily-cloudcode-pa.googleapis.com/v1internal (method=streamGenerateContent, stage=upload, upload_budget=15s): Upload budget exhausted after 15s of transfer (sent 9175040/10168215 bytes, no response received)。".to_string();
        let sug = "请检查系统代理、上游节点连通性或本地网络设置，确保能正常连接上游服务，并可在「超时设置」中调整分阶段传输预算。".to_string();

        let log = ProxyRequestLog {
            id: log_id.clone(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            method: "POST".to_string(),
            url: format!("/internal/warmup -> {}", req.model),
            status: 502,
            duration,
            model: Some(req.model.clone()),
            mapped_model: Some(mapped_model.clone()),
            upstream_model: Some(mapped_model.clone()),
            account_email: Some(req.email.clone()),
            client_ip: Some("127.0.0.1".to_string()),
            error: Some("Network Error: Upstream send failed (未收到请求回报)".to_string()),
            request_body: Some(format!(
                "{{\"type\": \"warmup\", \"model\": \"{}\"}}",
                req.model
            )),
            upstream_request_body: None,
            response_body: Some(diag.clone()),
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
            reasoning_tokens: None,
            protocol: Some("warmup".to_string()),
            username: None,
            request_headers: None,
            upstream_request_headers: None,
            response_headers: None,
            session_id: None,
        };
        state.monitor.log_request(log).await;

        return (
            StatusCode::BAD_GATEWAY,
            Json(WarmupResponse {
                success: false,
                message: format!("【网络请求异常（非服务端故障）】 {}", diag),
                log_id: Some(log_id),
                duration_ms: Some(duration),
                status: Some(502),
                account_email: Some(req.email.clone()),
                account_id: if account_id.is_empty() {
                    None
                } else {
                    Some(account_id.clone())
                },
                is_retired: None,
                error: Some(format!(
                    "【网络请求异常（非服务端故障）】\n诊断: {}\n建议: {}",
                    diag, sug
                )),
                is_server_error: Some(false),
                error_type: Some("upstream_network_error".to_string()),
            }),
        )
            .into_response();
    }

    // ===== 步骤 2: 根据模型类型构建请求体 =====
    let is_claude = mapped_model.to_lowercase().contains("claude")
        || req.model.to_lowercase().contains("claude");
    let is_image =
        mapped_model.to_lowercase().contains("image") || req.model.to_lowercase().contains("image");

    let body: Value = if is_claude {
        // Claude 模型：使用 transform_claude_request_in 转换
        let session_id = format!(
            "warmup_{}_{}",
            chrono::Utc::now().timestamp_millis(),
            &uuid::Uuid::new_v4().to_string()[..8]
        );
        let claude_request = crate::proxy::mappers::claude::models::ClaudeRequest {
            model: mapped_model.clone(),
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
                let log_id = uuid::Uuid::new_v4().to_string();
                let log = ProxyRequestLog {
                    id: log_id.clone(),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    method: "POST".to_string(),
                    url: format!("/internal/warmup -> {}", req.model),
                    status: 400,
                    duration,
                    model: Some(req.model.clone()),
                    mapped_model: Some(mapped_model.clone()),
                    upstream_model: Some(mapped_model.clone()),
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
                    reasoning_tokens: None,
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
                        log_id: Some(log_id),
                        duration_ms: Some(duration),
                        status: Some(400),
                        account_email: Some(req.email.clone()),
                        account_id: if account_id.is_empty() {
                            None
                        } else {
                            Some(account_id.clone())
                        },
                        is_retired: None,
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
                "model": mapped_model,
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
                "model": mapped_model,
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
            &mapped_model,
            None,
            Some(&session_id),
            None,
        ) // [FIX] Added None for token param
    };

    // ===== 步骤 3: 调用 UpstreamClient =====
    let upstream_req_body = serde_json::to_string_pretty(&body)
        .or_else(|_| serde_json::to_string(&body))
        .ok();

    let model_lower = mapped_model.to_lowercase();
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
                let is_retired = response_text.contains("no longer available")
                    || response_text.contains("is not available")
                    || response_text.contains("is retired")
                    || response_text.contains("Please switch to");

                let (final_success, message) = if is_retired {
                    (false, format!("模型已退役下线: {}", response_text.trim()))
                } else {
                    (true, format!("Warmup triggered for {}", req.model))
                };

                // 探活成功 = 上游已实际可用：立即清除该账号在这个模型上的
                // 900s 临时熔断与限流锁定，避免“探活成功但调度仍等 15 分钟”。
                if final_success && !account_id.is_empty() {
                    state
                        .token_manager
                        .clear_model_unsupported(&account_id, &mapped_model);
                    state
                        .token_manager
                        .clear_model_rate_limit(&account_id, &mapped_model);
                    tracing::info!(
                        "[Warmup-API] Cleared model breaker for {} / {} after successful probe",
                        req.email,
                        mapped_model
                    );

                    // 关键守卫（审查项 5）：仅当显式带有定向诊断/测活意图（probe_intent == true）时，
                    // 成功后才允许触发自愈解限；后台定时周预热/自动轮询严禁越权清除处于冷静期的验证锁！
                    if req.probe_intent && !account_id.is_empty() {
                        let _ = crate::modules::account::clear_account_forbidden_and_validation(
                            &account_id,
                            Some(&mapped_model),
                        );
                    }
                }

                info!(
                    "[Warmup-API] ========== SUCCESS: {} / {} -> {} ({}ms, retired={}) ==========",
                    req.email, req.model, mapped_model, duration, is_retired
                );

                // 记录成功预热请求到流量日志
                let log_id = uuid::Uuid::new_v4().to_string();
                let log = ProxyRequestLog {
                    id: log_id.clone(),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    method: "POST".to_string(),
                    url: format!("/internal/warmup -> {}", req.model),
                    status: status_code,
                    duration,
                    model: Some(req.model.clone()),
                    mapped_model: Some(mapped_model.clone()),
                    upstream_model: Some(mapped_model.clone()),
                    account_email: Some(req.email.clone()),
                    client_ip: Some("127.0.0.1".to_string()),
                    error: if is_retired {
                        Some("Model Retired (已退役下线)".to_string())
                    } else {
                        None
                    },
                    request_body: Some(format!(
                        "{{\"type\": \"warmup\", \"model\": \"{}\"}}",
                        req.model
                    )),
                    upstream_request_body: upstream_req_body.clone(),
                    response_body: Some(if response_text.is_empty() {
                        "OK".to_string()
                    } else {
                        response_text.clone()
                    }),
                    input_tokens: Some(0),
                    output_tokens: Some(0),
                    cached_tokens: None,
                    reasoning_tokens: None,
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
                        success: final_success,
                        message,
                        log_id: Some(log_id),
                        duration_ms: Some(duration),
                        status: Some(status_code),
                        account_email: Some(req.email.clone()),
                        account_id: if account_id.is_empty() {
                            None
                        } else {
                            Some(account_id.clone())
                        },
                        is_retired: if is_retired { Some(true) } else { None },
                        error: if is_retired {
                            Some(response_text)
                        } else {
                            None
                        },
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
                                "[Warmup-API] 403 Forbidden detected for {}, marking forbidden status",
                                req.email
                            );
                            // 关键防护（审查项 4）：统一使用 mapped_model 记录冻结 scope，确保与解冻模型完全闭环
                            let _ = crate::modules::account::mark_account_forbidden_with_model(
                                &resolved_account_id,
                                &error_text,
                                Some(&mapped_model),
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
                let log_id = uuid::Uuid::new_v4().to_string();
                let log = ProxyRequestLog {
                    id: log_id.clone(),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    method: "POST".to_string(),
                    url: format!("/internal/warmup -> {}", req.model),
                    status: status_code,
                    duration,
                    model: Some(req.model.clone()),
                    mapped_model: Some(mapped_model.clone()),
                    upstream_model: Some(mapped_model.clone()),
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
                    reasoning_tokens: None,
                    protocol: Some("warmup".to_string()),
                    username: None,
                    request_headers: None,
                    upstream_request_headers: None,
                    response_headers: None,
                    session_id: None,
                };
                state.monitor.log_request(log).await;

                // 检查是否命中 User-Agent 门禁或全池临时熔断
                let custom_ua = state.upstream.get_user_agent_override().await;
                let lang = crate::modules::config::load_app_config()
                    .ok()
                    .map(|c| c.language)
                    .unwrap_or_else(|| "zh".to_string());
                let i18n_err = crate::modules::i18n::get_gateway_error_strings(&lang);

                let (final_message, error_detail, error_type) =
                    if let Some((title, diag, sug, _category, err_code)) =
                        crate::proxy::handlers::common::check_outdated_user_agent_issue_with_lang(
                            &req.model,
                            status_code,
                            &error_text,
                            custom_ua.as_deref(),
                            Some(&lang),
                        )
                    {
                        (
                            format!("{} {}", title, diag),
                            Some(format!(
                                "{}\n{} {}\n{} {}",
                                title,
                                i18n_err.label_diagnosis,
                                diag,
                                i18n_err.label_suggestion,
                                sug
                            )),
                            Some(err_code.to_string()),
                        )
                    } else {
                        (
                            format!(
                                "Warmup failed: HTTP {} (上游拒绝，非服务端故障)",
                                status_code
                            ),
                            Some(error_text),
                            Some("upstream_error".to_string()),
                        )
                    };

                (
                    StatusCode::from_u16(status_code).unwrap_or(StatusCode::BAD_GATEWAY),
                    Json(WarmupResponse {
                        success: false,
                        message: final_message,
                        log_id: Some(log_id),
                        duration_ms: Some(duration),
                        status: Some(status_code),
                        account_email: Some(req.email.clone()),
                        account_id: if account_id.is_empty() {
                            None
                        } else {
                            Some(account_id.clone())
                        },
                        is_retired: None,
                        error: error_detail,
                        is_server_error: Some(status_code >= 500),
                        error_type,
                    }),
                )
                    .into_response()
            };

            // 添加响应头，让监控中间件捕获账号信息
            if let Ok(email_val) = axum::http::HeaderValue::from_str(&req.email) {
                response.headers_mut().insert("X-Account-Email", email_val);
            }
            if let Ok(model_val) = axum::http::HeaderValue::from_str(&mapped_model) {
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
            let log_id = uuid::Uuid::new_v4().to_string();
            let log = ProxyRequestLog {
                id: log_id.clone(),
                timestamp: chrono::Utc::now().timestamp_millis(),
                method: "POST".to_string(),
                url: format!("/internal/warmup -> {}", req.model),
                status: 502,
                duration,
                model: Some(req.model.clone()),
                mapped_model: Some(mapped_model.clone()),
                upstream_model: Some(mapped_model.clone()),
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
                reasoning_tokens: None,
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
                    log_id: Some(log_id),
                    duration_ms: Some(duration),
                    status: Some(502),
                    account_email: Some(req.email.clone()),
                    account_id: if account_id.is_empty() {
                        None
                    } else {
                        Some(account_id.clone())
                    },
                    is_retired: None,
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
            if let Ok(model_val) = axum::http::HeaderValue::from_str(&mapped_model) {
                response.headers_mut().insert("X-Mapped-Model", model_val);
            }

            response
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_warmup_model_route_resolution() {
        let mut custom_mapping = HashMap::new();

        // 裸模型 gemini-3.8-flash 必须被映射为具备官方实体的具名档位（默认对齐 high 旗舰算力通道）
        let mapped_flash = crate::proxy::common::model_mapping::resolve_model_route_with_effort(
            "gemini-3.8-flash",
            &custom_mapping,
            None,
        );
        assert_eq!(mapped_flash, "gemini-3.8-flash-high");

        // 已带档位的具名模型保持不变
        let mapped_high = crate::proxy::common::model_mapping::resolve_model_route_with_effort(
            "gemini-3.8-flash-high",
            &custom_mapping,
            None,
        );
        assert_eq!(mapped_high, "gemini-3.8-flash-high");

        // 自定义映射优先起效
        custom_mapping.insert(
            "gpt-oss-120b".to_string(),
            "gpt-oss-120b-medium".to_string(),
        );
        let mapped_gpt = crate::proxy::common::model_mapping::resolve_model_route_with_effort(
            "gpt-oss-120b",
            &custom_mapping,
            None,
        );
        assert_eq!(mapped_gpt, "gpt-oss-120b-medium");
    }
}
