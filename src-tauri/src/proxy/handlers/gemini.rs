// Gemini Handler
use axum::{
    body::Body,
    extract::State,
    extract::{Json, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::proxy::common::client_adapter::CLIENT_ADAPTERS;
use crate::proxy::debug_logger;
use crate::proxy::handlers::common::{
    apply_retry_strategy, build_token_error_headers, is_transient_token_error,
    next_rotation_attempt, should_rotate_account, FailureStatusTracker, RequestRetryState,
    RetryStrategy,
};
use crate::proxy::mappers::gemini::{unwrap_response, wrap_request_v2};
use crate::proxy::pipeline::response_has_inline_image_data;
use crate::proxy::server::AppState;
use crate::proxy::session_manager::SessionManager;
use crate::proxy::upstream::client::mask_email;
use axum::http::HeaderMap;

const MAX_RETRY_ATTEMPTS: usize = 3;

/// 处理 generateContent 和 streamGenerateContent
/// 路径参数: model_name, method (e.g. "gemini-pro", "generateContent")
pub async fn handle_generate(
    State(state): State<AppState>,
    Path(model_action): Path<String>,
    headers: HeaderMap, // [NEW] Extract headers for adapter detection
    upstream_recorder: Option<
        axum::extract::Extension<crate::proxy::monitor::UpstreamRequestBodyHolder>,
    >,
    user_identity: Option<
        axum::extract::Extension<crate::proxy::middleware::auth::UserTokenIdentity>,
    >,
    Json(mut body): Json<Value>, // 改为 mut 以支持修复提示词注入
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let clean_start = std::time::Instant::now();

    // 解析 model:method
    let (model_name, method) = if let Some((m, action)) = model_action.rsplit_once(':') {
        (m.to_string(), action.to_string())
    } else {
        (model_action, "generateContent".to_string())
    };

    crate::modules::logger::log_info(&format!(
        "Received Gemini request: {}/{}",
        model_name, method
    ));
    let trace_id = format!("req_{}", chrono::Utc::now().timestamp_subsec_millis());
    let debug_cfg = state.debug_logging.read().await.clone();

    // [NEW] Detect Client Adapter
    let client_adapter = CLIENT_ADAPTERS
        .iter()
        .find(|a| a.matches(&headers))
        .cloned();
    if client_adapter.is_some() {
        debug!("[{}] Client Adapter detected", trace_id);
    }

    // [DEFENSE] 净化 Gemini 原生请求体中的所有 inlineData (过滤或降级空数据/损坏图片)
    crate::proxy::mappers::common_utils::sanitize_gemini_payload_inline_data(&mut body);

    let message_count = body
        .get("contents")
        .and_then(|c| c.as_array())
        .map(|a| a.len())
        .unwrap_or(1)
        .max(1);

    // [Stage Timing] 阶段耗时度量变量 (毫秒)
    let clean_micros = clean_start.elapsed().as_micros() as u64;
    let clean_ms: f64 = clean_micros as f64 / 1000.0;
    let mut norm_ms: f64 = 0.0;
    let mut think_fill_ms: f64 = 0.0;
    let mut ttft_ms: f64 = 0.0;
    let mut fallback_overhead_ms: f64 = 0.0;

    // 1. 验证方法
    // [NEW] :countTokens 冒号语法，直接代理到上游 v1internal:countTokens
    if method == "countTokens" {
        // [MODEL GATE] 探测端点同样接入门禁（POST 也需要，不能只靠 GET 拦截）
        if let Err(rejection) = crate::proxy::handlers::common::enforce_model_gate(
            &state,
            Some(model_name.as_str()),
            "gemini",
            upstream_recorder.as_ref().map(|recorder| &recorder.0),
        )
        .await
        {
            return Ok(rejection);
        }
        return Ok(execute_count_tokens(
            state,
            headers,
            user_identity
                .as_ref()
                .map(|identity| identity.token_id.clone()),
            model_name,
            body,
        )
        .await);
    }

    if method != "generateContent" && method != "streamGenerateContent" {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Unsupported method: {}", method),
        ));
    }
    if debug_logger::is_enabled(&debug_cfg) {
        let original_payload = json!({
            "kind": "original_request",
            "protocol": "gemini",
            "trace_id": trace_id,
            "original_model": model_name,
            "method": method,
            "request": body.clone(),
        });
        debug_logger::write_debug_payload(
            &debug_cfg,
            Some(&trace_id),
            "original_request",
            &original_payload,
        )
        .await;
    }
    let client_wants_stream = method == "streamGenerateContent";
    // [AUTO-CONVERSION] 强制内部流式化
    let force_stream_internally = !client_wants_stream;
    let is_stream = client_wants_stream || force_stream_internally;

    if force_stream_internally {
        // debug!("[AutoConverter] Converting non-stream request to stream");
    }

    // [MODEL GATE] 入站模型闸门：缺失/不存在 → 404 model not exist；全池无配额 → 502 no quota accounts
    // 必须放在任何 `state.*` 字段被 move 之前，避免 borrow-after-partial-move。
    if let Err(rejection) = crate::proxy::handlers::common::enforce_model_gate(
        &state,
        Some(model_name.as_str()),
        "gemini",
        upstream_recorder.as_ref().map(|recorder| &recorder.0),
    )
    .await
    {
        return Ok(rejection);
    }

    // 2. 获取 UpstreamClient 和 TokenManager
    let upstream = state.upstream.clone();
    let image_scheduler = state.image_scheduler.clone();
    let request_timeout = state.request_timeout;
    let token_manager = state.token_manager;
    let pool_size = token_manager.len();
    // [FIX #3485] 自适应多账号池与单账号退避最大重试次数 (单账号3次，多账号整池两轮)
    let max_attempts = crate::proxy::handlers::common::calculate_max_retry_attempts(pool_size);

    let mut last_error = String::new();
    let mut last_email: Option<String> = None;
    let mut force_rotate = false;
    let mut retry_state = RequestRetryState::default();
    let mut retry_credentials: Option<(String, String, String, String, u64)> = None;
    let mut image_permit = None;
    let mut failure_statuses = FailureStatusTracker::default();
    let mut used_attempts = 0;
    let mut retried_without_thinking = false;
    let mut retried_tool_choice_conflict = false;
    let mut retried_with_budget_degraded = false;
    let mut retried_with_image_omitted = false;
    let mut degraded_wrapped_body: Option<Value> = None;

    let client_effort = body
        .get("request")
        .unwrap_or(&body)
        .get("generationConfig")
        .and_then(|gc| gc.get("thinkingConfig"))
        .and_then(|tc| tc.get("thinkingLevel").or_else(|| tc.get("effort")))
        .and_then(|v| v.as_str());

    let initial_mapped_model = crate::proxy::common::model_mapping::resolve_model_route_with_effort(
        &model_name,
        &*state.custom_mapping.read().await,
        client_effort,
    );

    while let Some(attempt) = next_rotation_attempt(
        &mut used_attempts,
        max_attempts,
        retry_credentials.is_some(),
    ) {
        // [Stage Timing] 中转归一计时起点
        let norm_start = std::time::Instant::now();

        // 3. 模型路由解析
        let mapped_model = initial_mapped_model.clone();
        // 提取 tools 列表以进行联网探测 (Gemini 风格可能是嵌套的，支持已带 request 包装的客户端)
        let effective_req = body.get("request").unwrap_or(&body);
        let tools_val: Option<Vec<Value>> = effective_req
            .get("tools")
            .and_then(|t| t.as_array())
            .map(|arr| {
                let mut flattened = Vec::new();
                for tool_entry in arr {
                    if let Some(decls) = tool_entry
                        .get("functionDeclarations")
                        .or_else(|| tool_entry.get("function_declarations"))
                        .and_then(|v| v.as_array())
                    {
                        flattened.extend(decls.iter().cloned());
                    } else {
                        flattened.push(tool_entry.clone());
                    }
                }
                flattened
            });

        let config = crate::proxy::mappers::common_utils::resolve_request_config(
            &model_name,
            &mapped_model,
            &tools_val,
            None,                // size (not applicable for Gemini native protocol)
            None,                // quality
            None,                // [NEW] image_size
            Some(effective_req), // [NEW] Pass request body for imageConfig parsing
        );

        // 内容锚点只进思维库。账号粘性与上游 sessionId 用 affinity_key。
        let anchor = SessionManager::gemini_content_anchor(&body);
        let session_scope = crate::proxy::thinking_store::SessionScope::resolve(
            &headers,
            Some(&body),
            None,
            anchor,
            user_identity
                .as_ref()
                .map(|identity| identity.token_id.as_str()),
        );
        let session_id = session_scope.store_key.clone();
        let affinity_key = session_scope.affinity_key.clone();
        let client_session_id = session_scope.client_id.clone();

        // 关键：根据 force_rotate 标志决定是否轮换账号（支持 Grace Retry 原地重试）
        let (access_token, project_id, email, account_id, _wait_ms) = if let Some(credentials) =
            retry_credentials.take()
        {
            credentials
        } else if config.request_type == "image_gen" {
            drop(image_permit.take());
            match token_manager
                .get_image_token(
                    force_rotate,
                    Some(&affinity_key),
                    &config.final_model,
                    &image_scheduler,
                    request_timeout,
                )
                .await
            {
                Ok((access_token, project_id, email, account_id, wait_ms, permit)) => {
                    image_permit = Some(permit);
                    (access_token, project_id, email, account_id, wait_ms)
                }
                Err((status, message)) => {
                    failure_statuses.record(status);
                    last_error = message;
                    break;
                }
            }
        } else {
            let mut token_result = token_manager
                .get_token(
                    &config.request_type,
                    force_rotate,
                    Some(&affinity_key),
                    &config.final_model,
                )
                .await;

            if let Err(ref e) = token_result {
                if is_transient_token_error(e) {
                    tracing::warn!(
                            "Token acquisition transient error ({}), retrying once with force_rotate...",
                            e
                        );
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    token_result = token_manager
                        .get_token(
                            &config.request_type,
                            true,
                            Some(&affinity_key),
                            &config.final_model,
                        )
                        .await;
                }
            }

            match token_result {
                Ok(t) => t,
                Err(e) => {
                    let headers = build_token_error_headers(
                        Some(mapped_model.as_str()),
                        last_email.as_deref(),
                        &e,
                    );
                    return Ok((
                        StatusCode::SERVICE_UNAVAILABLE,
                        headers,
                        format!("Token error: {}", e),
                    )
                        .into_response());
                }
            }
        };

        let in_flight_guard = Arc::new(token_manager.acquire_in_flight(&account_id));

        let mapped_model = token_manager
            .resolve_dynamic_model_for_account(&account_id, &mapped_model)
            .await;

        last_email = Some(email.clone());
        info!("✓ Using account: {} (type: {})", email, config.request_type);

        // 5. 包装请求 (project injection)
        // [FIX #765] Pass session_id to wrap_request for signature injection
        // [NEW] 获取完整 Token 对象以注入动态规格 (dynamic > static default > 65535)
        let token_obj = token_manager.get_token_by_id(&account_id);
        let tf_start = std::time::Instant::now();
        let mut wrapped_body = if let Some(body) = degraded_wrapped_body.take() {
            body
        } else {
            let mut wb = wrap_request_v2(
                &body,
                &project_id,
                &mapped_model,
                Some(account_id.as_str()),
                Some(&session_id),
                token_obj.as_ref(),
                Some(&token_manager),
                Some(&state.upstream),
                Some(&affinity_key),
                retried_without_thinking,
            );
            if retried_with_budget_degraded {
                crate::proxy::pipeline::InboundThinkingPipeline::try_degrade_thinking_budget(
                    &mut wb,
                );
            }
            wb
        };
        // [Signature-Retry] 400 签名错误后的重试：由流水线统一清除签名（包含客户端自带的 thoughtSignature）
        if retried_without_thinking {
            crate::proxy::pipeline::InboundThinkingPipeline::strip_signatures_for_retry(
                &mut wrapped_body,
                Some(session_id.as_str()),
                &mapped_model,
            );
        }
        let tf_micros = tf_start.elapsed().as_micros() as u64;
        let norm_total_micros = norm_start.elapsed().as_micros() as u64;
        norm_ms = norm_total_micros.saturating_sub(tf_micros) as f64 / 1000.0;
        think_fill_ms = tf_micros as f64 / 1000.0;

        let _ =
            crate::proxy::mappers::context_manager::ContextManager::apply_post_transit_context_mgmt(
                &mut wrapped_body,
                &mapped_model,
            );
        crate::proxy::mappers::prompt_sanitizer::PromptSanitizer::sanitize_gemini_payload(
            &mut wrapped_body,
        );
        crate::proxy::mappers::common_utils::ensure_gemini_payload_ends_with_user(
            &mut wrapped_body,
        );
        let estimated_input_tokens = crate::proxy::pipeline::estimate_tokens(&wrapped_body);

        if let Some(ref recorder) = upstream_recorder {
            recorder.set_value(&wrapped_body);
        }

        if debug_logger::is_enabled(&debug_cfg) {
            let payload = json!({
                "kind": "v1internal_request",
                "protocol": "gemini",
                "trace_id": trace_id,
                "original_model": model_name,
                "mapped_model": mapped_model,
                "request_type": config.request_type,
                "attempt": attempt,
                "v1internal_request": wrapped_body.clone(),
            });
            debug_logger::write_debug_payload(
                &debug_cfg,
                Some(&trace_id),
                "v1internal_request",
                &payload,
            )
            .await;
        }

        // 5. 上游调用
        let query_string = if is_stream { Some("alt=sse") } else { None };
        let upstream_method = if is_stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        };

        // [FIX #1522] Inject Anthropic Beta Headers for Claude models
        let mut extra_headers = std::collections::HashMap::new();
        extra_headers.insert("x-session-id".to_string(), client_session_id.clone());
        if mapped_model.to_lowercase().contains("claude") {
            extra_headers.insert("anthropic-beta".to_string(), "claude-code-20250219,interleaved-thinking-2025-05-14,fine-grained-tool-streaming-2025-05-14".to_string());
            tracing::debug!(
                "[Gemini] Injected Anthropic beta headers for Claude model: {}",
                mapped_model
            );
        }

        let preceding_turn_anchor = wrapped_body
            .get("request")
            .and_then(|r| r.get("contents"))
            .or_else(|| wrapped_body.get("contents"))
            .and_then(|c| c.as_array())
            .and_then(|a| a.last())
            .cloned();
        let causal_anchor =
            crate::proxy::thinking_store::compute_causal_anchor(preceding_turn_anchor.as_ref());

        let upstream_req_start = std::time::Instant::now();
        let call_result = match upstream
            .call_v1_internal_with_headers(
                upstream_method,
                &access_token,
                wrapped_body.clone(),
                query_string,
                extra_headers.clone(),
                Some(account_id.as_str()),
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_error = e.clone();
                failure_statuses.record(StatusCode::BAD_GATEWAY);
                drop(image_permit.take());
                // 首字节预算耗尽不是账号故障：不计入账号冷却，也不跨账号重放同一 body
                let first_byte_terminal =
                    crate::proxy::upstream::client::is_first_byte_stage_failure(&e);
                if !first_byte_terminal {
                    token_manager.mark_account_transient_failure(&account_id, 30, &e);
                }
                debug!(
                    "Gemini Request failed on attempt {}/{}: {}",
                    attempt + 1,
                    max_attempts,
                    e
                );
                if crate::proxy::upstream::client::is_terminal_replay_suppressed(&e) {
                    tracing::warn!(
                        first_byte = first_byte_terminal,
                        "Stopping account rotation to avoid re-upload/prefill amplification"
                    );
                    break;
                }
                continue;
            }
        };

        let effective_attempt_start = call_result.effective_attempt_start;
        let mut fallback_attempts = call_result.fallback_attempts.clone();
        fallback_overhead_ms = call_result.fallback_overhead_s() * 1000.0;
        let attempts_b64 =
            crate::proxy::handlers::common::encode_attempts_header(&fallback_attempts);

        // [NEW] 记录端点降级日志到 debug 文件
        if call_result.has_fallback() && debug_logger::is_enabled(&debug_cfg) {
            let fallback_entries: Vec<serde_json::Value> = call_result
                .fallback_attempts
                .iter()
                .map(|a| {
                    json!({
                        "endpoint_url": a.endpoint_url,
                        "status": a.status,
                        "error": a.error,
                    })
                })
                .collect();
            let payload = json!({
                "kind": "endpoint_fallback",
                "protocol": "gemini",
                "trace_id": trace_id,
                "original_model": model_name,
                "mapped_model": mapped_model,
                "attempt": attempt,
                "account": mask_email(&email),
                "fallback_attempts": fallback_entries,
            });
            debug_logger::write_debug_payload(
                &debug_cfg,
                Some(&trace_id),
                "endpoint_fallback",
                &payload,
            )
            .await;
        }

        let response = call_result.response;
        // [NEW] 提取实际请求的上游端点 URL，用于日志记录和排查
        let upstream_url = response.url().to_string();
        let status = response.status();

        // [NEW] 提取官方 TraceID
        let cloud_code_trace_id = response
            .headers()
            .get("x-cloudaicompanion-trace-id")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        if status.is_success() {
            let track_image_success = config.request_type == "image_gen";
            if !track_image_success {
                token_manager.commit_session(&affinity_key, &account_id);
                // [智能限流] 请求成功，重置该账号的连续失败计数
                token_manager.mark_account_success(&account_id);
            }
            // 6. 响应处理
            if is_stream {
                use axum::body::Body;
                use axum::response::Response;
                use futures::StreamExt;

                let meta = json!({
                    "protocol": "gemini",
                    "trace_id": trace_id,
                    "original_model": model_name,
                    "mapped_model": mapped_model,
                    "request_type": config.request_type,
                    "attempt": attempt,
                    "status": status.as_u16(),
                    "upstream_url": upstream_url,
                });
                let response_stream = debug_logger::wrap_stream_with_debug(
                    Box::pin(response.bytes_stream()),
                    debug_cfg.clone(),
                    trace_id.clone(),
                    "upstream_response",
                    meta,
                );

                // [Auto-Heal] 纯思考空回复流式自愈门禁 (Pipeline First)
                let auto_heal_ctx = crate::proxy::pipeline::auto_heal::ThinkingAutoHealContext {
                    upstream: upstream.clone(),
                    method: upstream_method,
                    access_token: access_token.clone(),
                    original_body: wrapped_body.clone(),
                    query_string,
                    extra_headers: extra_headers.clone(),
                    account_id: Some(account_id.clone()),
                    trace_id: trace_id.clone(),
                };
                let response_stream =
                    crate::proxy::pipeline::auto_heal::wrap_stream_with_empty_thinking_auto_heal(
                        Box::pin(response_stream),
                        auto_heal_ctx,
                    );
                let s_id = session_id.clone(); // Clone for stream closure

                // [对齐其他协议] 上游已返回 200：立即向客户端返回响应头，
                // 由流生命周期的 InitialTtft 负责首字宽限；不再阻塞式 peek 首包，
                // 避免响应头被推迟、peek 期间无心跳。
                ttft_ms = effective_attempt_start.elapsed().as_micros() as f64 / 1000.0;
                let is_thinking = body
                    .get("generationConfig")
                    .and_then(|g| g.get("thinkingConfig"))
                    .is_some()
                    || mapped_model.to_lowercase().contains("thinking");
                crate::proxy::handlers::common::report_qos_ttft(
                    &token_manager,
                    &account_id,
                    Some(&affinity_key),
                    ttft_ms,
                    is_thinking,
                    Some(estimated_input_tokens),
                );
                if let Some(effective_item) = fallback_attempts.iter_mut().find(|a| a.is_effective)
                {
                    effective_item.duration_s = ttft_ms / 1000.0;
                }
                let _ = crate::proxy::monitor::CURRENT_UPSTREAM_CAPTURE.try_with(|holder| {
                    holder.set_attempts(fallback_attempts.clone());
                });
                let image_permit_for_stream = image_permit.take();
                let track_image_success = config.request_type == "image_gen";
                let in_flight_guard_for_stream = Arc::clone(&in_flight_guard);
                let lifecycle_cfg = crate::proxy::pipeline::StreamLifecycleConfig::new(
                    "gemini",
                    "handle_generate",
                    format!(
                        "model={} session={} messages={}",
                        mapped_model, session_id, message_count
                    ),
                );
                let handler = crate::proxy::mappers::gemini::streaming::GeminiStreamHandler::new(
                    cloud_code_trace_id.clone(),
                    s_id.clone(),
                    mapped_model.clone(),
                    message_count,
                    causal_anchor.clone(),
                    track_image_success,
                    Some(token_manager.clone()),
                    account_id.clone(),
                    mapped_model.clone(),
                );
                let upstream_stream = response_stream.map(|r| r.map_err(|e| e.to_string()));
                let lifecycle_stream = crate::proxy::pipeline::run_stream_lifecycle(
                    Box::pin(upstream_stream),
                    handler,
                    lifecycle_cfg,
                );
                // 仅保留请求级守卫（in-flight / 图片并发配额），流式策略全部交给流水线
                let stream = async_stream::stream! {
                    let _in_flight = in_flight_guard_for_stream;
                    let _image_permit = image_permit_for_stream;
                    let mut inner = lifecycle_stream;
                    while let Some(item) = inner.next().await {
                        yield item;
                    }
                };

                if client_wants_stream {
                    let body = Body::from_stream(stream);
                    return Ok(Response::builder()
                        .header("Content-Type", "text/event-stream")
                        .header("Cache-Control", "no-cache")
                        .header("Connection", "keep-alive")
                        .header("X-Accel-Buffering", "no")
                        .header("X-Account-Email", &email)
                        .header("X-Mapped-Model", &mapped_model)
                        .header("X-Upstream-Model", config.final_model.as_str())
                        .header("X-Session-Id", &client_session_id)
                        .header("X-Antigravity-Session-Id", &client_session_id)
                        .header("X-Timing-Clean-Ms", format!("{:.3}", clean_ms))
                        .header("X-Timing-Norm-Ms", format!("{:.3}", norm_ms))
                        .header("X-Timing-Thinking-Ms", format!("{:.3}", think_fill_ms))
                        .header("X-Timing-Ttft-Ms", format!("{:.3}", ttft_ms))
                        .header(
                            "X-Timing-Ttft-Overhead-Ms",
                            format!("{:.3}", fallback_overhead_ms),
                        )
                        .header(
                            "X-Timing-Ttft-Attempts",
                            attempts_b64.as_deref().unwrap_or(""),
                        )
                        .body(body)
                        .unwrap()
                        .into_response());
                } else {
                    // Collect to JSON
                    use crate::proxy::mappers::gemini::collector::collect_stream_to_json_with_anchor_and_count;
                    match collect_stream_to_json_with_anchor_and_count(
                        Box::pin(stream),
                        &s_id,
                        Some(&causal_anchor),
                        message_count,
                    )
                    .await
                    {
                        Ok(gemini_resp) => {
                            let unwrapped = unwrap_response(&gemini_resp);
                            if track_image_success && response_has_inline_image_data(&unwrapped) {
                                token_manager.commit_session(&affinity_key, &account_id);
                                token_manager.mark_account_success(&account_id);
                                token_manager
                                    .clear_persisted_live_limit(&account_id, Some(&mapped_model));
                            }
                            if let Some(error_obj) = unwrapped.get("error") {
                                let code = error_obj
                                    .get("code")
                                    .and_then(|c| c.as_u64())
                                    .unwrap_or(500)
                                    as u16;
                                let status_code = StatusCode::from_u16(code)
                                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                                warn!(
                                    "[{}] Stream collection received error in stream (status {}): {:?}",
                                    session_id, status_code, error_obj
                                );
                                return Ok(Response::builder()
                                    .status(status_code)
                                    .header("Content-Type", "application/json")
                                    .header("X-Account-Email", &email)
                                    .header("X-Mapped-Model", &mapped_model)
                                    .header("X-Upstream-Model", config.final_model.as_str())
                                    .header("X-Session-Id", &client_session_id)
                                    .header("X-Antigravity-Session-Id", &client_session_id)
                                    .header("X-Timing-Clean-Ms", format!("{:.3}", clean_ms))
                                    .header("X-Timing-Norm-Ms", format!("{:.3}", norm_ms))
                                    .header("X-Timing-Thinking-Ms", format!("{:.3}", think_fill_ms))
                                    .header("X-Timing-Ttft-Ms", format!("{:.3}", ttft_ms))
                                    .header(
                                        "X-Timing-Ttft-Overhead-Ms",
                                        format!("{:.3}", fallback_overhead_ms),
                                    )
                                    .body(Body::from(serde_json::to_string(&unwrapped).unwrap()))
                                    .unwrap()
                                    .into_response());
                            }

                            info!(
                                "[{}] ✓ Stream collected and converted to JSON (Gemini)",
                                session_id
                            );
                            return Ok(Response::builder()
                                .status(StatusCode::OK)
                                .header("Content-Type", "application/json")
                                .header("X-Account-Email", &email)
                                .header("X-Mapped-Model", &mapped_model)
                                .header("X-Upstream-Model", config.final_model.as_str())
                                .header("X-Session-Id", &client_session_id)
                                .header("X-Antigravity-Session-Id", &client_session_id)
                                .header("X-Timing-Clean-Ms", format!("{:.3}", clean_ms))
                                .header("X-Timing-Norm-Ms", format!("{:.3}", norm_ms))
                                .header("X-Timing-Thinking-Ms", format!("{:.3}", think_fill_ms))
                                .header("X-Timing-Ttft-Ms", format!("{:.3}", ttft_ms))
                                .header(
                                    "X-Timing-Ttft-Overhead-Ms",
                                    format!("{:.3}", fallback_overhead_ms),
                                )
                                .body(Body::from(serde_json::to_string(&unwrapped).unwrap()))
                                .unwrap()
                                .into_response());
                        }
                        Err(e) => {
                            error!("Stream collection error: {}", e);
                            let error_json = serde_json::json!({
                                "error": {
                                    "code": 500,
                                    "message": format!("Stream collection error: {}", e),
                                    "status": "INTERNAL"
                                }
                            });
                            return Ok(Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .header("Content-Type", "application/json")
                                .header("X-Account-Email", &email)
                                .header("X-Mapped-Model", &mapped_model)
                                .header("X-Upstream-Model", config.final_model.as_str())
                                .header("X-Session-Id", &client_session_id)
                                .header("X-Antigravity-Session-Id", &client_session_id)
                                .body(Body::from(serde_json::to_string(&error_json).unwrap()))
                                .unwrap()
                                .into_response());
                        }
                    }
                }
            }

            ttft_ms = effective_attempt_start.elapsed().as_micros() as f64 / 1000.0;
            let is_thinking = body
                .get("generationConfig")
                .and_then(|g| g.get("thinkingConfig"))
                .is_some()
                || mapped_model.to_lowercase().contains("thinking");
            crate::proxy::handlers::common::report_qos_ttft(
                &token_manager,
                &account_id,
                Some(&affinity_key),
                ttft_ms,
                is_thinking,
                Some(estimated_input_tokens),
            );
            if let Some(effective_item) = fallback_attempts.iter_mut().find(|a| a.is_effective) {
                effective_item.duration_s = ttft_ms / 1000.0;
            }
            let _ = crate::proxy::monitor::CURRENT_UPSTREAM_CAPTURE.try_with(|holder| {
                holder.set_attempts(fallback_attempts.clone());
            });
            let mut gemini_resp: Value = response
                .json()
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Parse error: {}", e)))?;

            // [FIX #1522] Inject Tool ID into Non-streaming Response
            crate::proxy::mappers::gemini::wrapper::inject_ids_to_response(
                &mut gemini_resp,
                &mapped_model,
            );

            // [FIX #765] Extract thoughtSignature from non-streaming response
            let inner_val = if gemini_resp.get("response").is_some() {
                gemini_resp.get("response")
            } else {
                Some(&gemini_resp)
            };

            if let Some(resp) = inner_val {
                if let Some(candidates) = resp.get("candidates").and_then(|c| c.as_array()) {
                    for cand in candidates {
                        if let Some(parts) = cand
                            .get("content")
                            .and_then(|c| c.get("parts"))
                            .and_then(|p| p.as_array())
                        {
                            for part in parts {
                                if let Some(sig) =
                                    part.get("thoughtSignature").and_then(|s| s.as_str())
                                {
                                    crate::proxy::SignatureCache::global().cache_session_signature(
                                        &session_id,
                                        sig.to_string(),
                                        message_count,
                                    );
                                    debug!("[Gemini-Response] Cached session signature (len: {}) for session: {} at count {}", sig.len(), session_id, message_count);
                                }
                            }
                        }
                    }
                }
            }

            let preceding_turn = preceding_turn_anchor.as_ref();
            crate::proxy::thinking_store::capture_gemini_response_with_preceding(
                &session_id,
                &gemini_resp,
                preceding_turn,
            );
            let unwrapped = unwrap_response(&gemini_resp);
            return Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .header("X-Account-Email", &email)
                .header("X-Mapped-Model", &mapped_model)
                .header("X-Upstream-Model", config.final_model.as_str())
                .header("X-Session-Id", &client_session_id)
                .header("X-Antigravity-Session-Id", &client_session_id)
                .header("X-Timing-Clean-Ms", format!("{:.3}", clean_ms))
                .header("X-Timing-Norm-Ms", format!("{:.3}", norm_ms))
                .header("X-Timing-Thinking-Ms", format!("{:.3}", think_fill_ms))
                .header("X-Timing-Ttft-Ms", format!("{:.3}", ttft_ms))
                .header(
                    "X-Timing-Ttft-Overhead-Ms",
                    format!("{:.3}", fallback_overhead_ms),
                )
                .header(
                    "X-Timing-Ttft-Attempts",
                    attempts_b64.as_deref().unwrap_or(""),
                )
                .body(Body::from(serde_json::to_string(&unwrapped).unwrap()))
                .unwrap()
                .into_response());
        }

        // 处理错误并重试
        failure_statuses.record(status);
        let status_code = status.as_u16();
        let retry_after = response
            .headers()
            .get("Retry-After")
            .and_then(|header| header.to_str().ok())
            .map(str::to_string);
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| format!("HTTP {}", status_code));
        last_error = format!("HTTP {}: {}", status_code, error_text);
        if debug_logger::is_enabled(&debug_cfg) {
            let payload = json!({
                "kind": "upstream_response_error",
                "protocol": "gemini",
                "trace_id": trace_id,
                "original_model": model_name,
                "mapped_model": mapped_model,
                "request_type": config.request_type,
                "attempt": attempt,
                "status": status_code,
                "upstream_url": upstream_url,
                "account": mask_email(&email),
                "error_text": error_text,
            });
            debug_logger::write_debug_payload(
                &debug_cfg,
                Some(&trace_id),
                "upstream_response_error",
                &payload,
            )
            .await;
        }

        // [FIX] 403 时优先检测 VALIDATION_REQUIRED 并设置 is_forbidden / validation_block 状态，确保及时提取 URL 与更新 UI
        if status_code == 403 {
            if let Some(acc_id) = token_manager.get_account_id_by_email(&email) {
                if error_text.contains("VALIDATION_REQUIRED")
                    || error_text.contains("verify your account")
                    || error_text.contains("Verify your account")
                    || error_text.contains("validation_url")
                {
                    tracing::warn!(
                        "[Gemini] VALIDATION_REQUIRED detected on account {}, temporarily blocking",
                        email
                    );
                    let block_minutes = 10i64;
                    let block_until = chrono::Utc::now().timestamp() + (block_minutes * 60);

                    if let Err(e) = token_manager
                        .set_validation_block_public(&acc_id, block_until, &error_text)
                        .await
                    {
                        tracing::error!("Failed to set validation block: {}", e);
                    }
                } else {
                    // 设置 is_forbidden 状态并持久化（仅当真正非人机验证类 403 时才进入永久禁用）
                    if let Err(e) = token_manager.set_forbidden(&acc_id, &error_text).await {
                        tracing::error!("Failed to set forbidden status: {}", e);
                    }
                }
            }
        }

        // [FIX session-1M] 上游按 sessionId 在服务端累计会话输入，长工具循环会把累计推过 1M，
        // 之后该 sessionId 的所有请求都 400 "input token count exceeds ... 1048576"。
        // 给 (账号, 对话) 的 sessionId 升代并立即重试:新 sessionId = 上游全新会话,对话无感恢复。
        if status_code == 400 && error_text.contains("exceeds the maximum number of tokens") {
            let generation = crate::proxy::common::session::bump_session(&account_id, &session_id);
            tracing::warn!(
                "[Gemini] Upstream session token accumulation exceeded 1M on account {}. sessionId bumped to generation {}, retrying with a fresh upstream session.",
                email, generation
            );
            continue; // 重试:下一轮读取新代数,派生全新 sessionId
        }

        let scheduling_mode = token_manager.get_scheduling_mode().await;
        let _allow_grace = match scheduling_mode {
            crate::proxy::sticky_config::SchedulingMode::Balance => {
                token_manager.tokens_count() <= 1
            }
            crate::proxy::sticky_config::SchedulingMode::CacheFirst => true,
            crate::proxy::sticky_config::SchedulingMode::PerformanceFirst => false,
        };

        // 确定重试策略：传入当前 attempt 与 pool_size，执行智能自适应裁决
        let strategy = retry_state.determine_strategy_adaptive(
            &account_id,
            status_code,
            &error_text,
            retry_after.as_deref(),
            retried_without_thinking,
            attempt,
            pool_size,
        );
        // 统一流水线决策判定：协议无关的限流与错误判定
        let classification = crate::proxy::pipeline::UpstreamClassification::classify(
            status_code,
            &error_text,
            retry_after.as_deref(),
        );

        // [FIX #3713] 503 / 529 服务端过载：思考预算原地降级自愈 (Pipeline First)
        // 当模型遭遇过载 (503/529 / overloaded)，若当前请求带有 -1 或超大思考预算，优先在当前账号上降级预算原地重试一次，
        // 杜绝盲目全池轮换导致跨账号缓存丢失 (Prefix Cache Protection)。
        if crate::proxy::pipeline::UpstreamClassification::is_server_overloaded(
            status_code,
            &error_text,
        ) && !retried_with_budget_degraded
            && crate::proxy::pipeline::InboundThinkingPipeline::try_degrade_thinking_budget(
                &mut wrapped_body,
            )
        {
            retried_with_budget_degraded = true;
            degraded_wrapped_body = Some(wrapped_body);
            force_rotate = false;
            tracing::warn!(
                "[Gemini] Upstream {}/overloaded detected. In-place self-healing: degraded thinking budget to 4000 on account {} (preserving prefix cache)",
                status_code, email
            );
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            continue;
        }

        // [FIX #3721] 400 强制工具调用与思考冲突：自愈剔除 thinkingConfig 原地重试 (Pipeline First)
        if crate::proxy::pipeline::UpstreamClassification::is_thinking_tool_choice_conflict(
            status_code,
            &error_text,
        ) && !retried_tool_choice_conflict
        {
            retried_tool_choice_conflict = true;
            if let Some(req) = wrapped_body
                .get_mut("request")
                .and_then(Value::as_object_mut)
            {
                if let Some(gc) = req
                    .get_mut("generationConfig")
                    .and_then(Value::as_object_mut)
                {
                    gc.remove("thinkingConfig");
                }
            } else if let Some(gc) = wrapped_body
                .get_mut("generationConfig")
                .and_then(Value::as_object_mut)
            {
                gc.remove("thinkingConfig");
            }
            degraded_wrapped_body = Some(wrapped_body);
            force_rotate = false;
            tracing::warn!(
                "[Gemini] Upstream 400 tool_choice/thinking conflict detected. In-place self-healing: stripped thinkingConfig on account {}, retrying.",
                email
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            continue;
        }

        // [FIX Corrupted Image] 400 多模态图像解码/处理失败：自愈剔除损坏图片并在同账号原地重试
        if classification.is_image_processing_error() && !retried_with_image_omitted {
            let purged_count =
                crate::proxy::mappers::common_utils::purge_corrupted_images_from_gemini_payload(
                    &mut wrapped_body,
                );
            if purged_count == 0 {
                tracing::warn!(
                    "[Gemini] Upstream 400 Unable to process input image detected, but no inline image was purged. Skipping retry to prevent duplicate 400."
                );
            } else {
                retried_with_image_omitted = true;
                degraded_wrapped_body = Some(wrapped_body);
                force_rotate = false;
                tracing::warn!(
                    "[Gemini] Upstream 400 Unable to process input image detected. In-place self-healing: purged {} image(s) to text placeholders on account {}, retrying.",
                    purged_count, email
                );
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            }
        }

        if classification.abandons_sticky_account() {
            token_manager.abandon_session(&affinity_key, &account_id);
            tracing::debug!(
                "[Gemini] Unbound session {} from account {} due to status {}",
                affinity_key,
                email,
                status_code
            );
        }

        if classification.is_model_not_found() {
            // [NEW] 针对特定账号记录单模型临时熔断（例如该 PRO 账号未开通 Claude 5.5），绝不连坐其他模型
            token_manager.mark_model_unsupported(&account_id, &mapped_model, Some(900));

            // 如果账号池中还有其他未尝试的候选账号，则顺畅换号重试，而不是直接放弃报错
            if attempt < pool_size {
                tracing::warn!(
                    "[Gemini] 上游报错模型不可用 (HTTP {})，已标记账号 {} 对模型 [{}] 临时熔断，继续换号重试 ({}/{})...",
                    status_code, email, mapped_model, attempt, pool_size
                );
                force_rotate = true;
                continue;
            }

            tracing::warn!(
                "[Gemini] Target model [{}] not found on upstream (HTTP {}). Pool exhausted without account-level lockout.",
                mapped_model, status_code
            );
            let dual_err = crate::proxy::handlers::common::build_dual_track_error(
                "gemini",
                status_code,
                &mapped_model,
                &error_text,
            );
            return Ok((
                StatusCode::from_u16(status_code).unwrap_or(StatusCode::NOT_FOUND),
                [
                    ("X-Account-Email", email.as_str()),
                    ("X-Mapped-Model", mapped_model.as_str()),
                    ("X-Upstream-Model", config.final_model.as_str()),
                ],
                Json(dual_err),
            )
                .into_response());
        }

        if classification.is_thought_signature_error() {
            if !retried_without_thinking {
                retried_without_thinking = true;
                tracing::warn!(
                    "[Gemini] Pipeline: Thinking signature error detected on upstream (HTTP {}). Surgically purging corrupted signatures and retrying on same account.",
                    status_code
                );
                // 1. 精准定向净化 ThinkingStore 中的异构污染签名
                session_scope.purge_signatures(&mapped_model);
                // 3. 保持同一账号原地重试
                force_rotate = false;
                continue;
            } else {
                tracing::warn!(
                    "[Gemini] Pipeline: Thinking signature error persisted after retry without thinking. Terminating retry loop."
                );
            }
        }

        let should_mark_limited = classification.should_lock_account();
        let needs_quota_refresh = if should_mark_limited {
            token_manager
                .mark_rate_limited_fast(
                    &email,
                    status_code,
                    retry_after.as_deref(),
                    &error_text,
                    Some(&mapped_model),
                )
                .await
        } else {
            false
        };
        if !matches!(&strategy, RetryStrategy::GraceRetry(_)) {
            drop(image_permit.take());
        }
        if needs_quota_refresh {
            token_manager
                .refresh_quota_lock_after_fast_mark(&email, Some(&mapped_model))
                .await;
        }
        let trace_id = format!("gemini_{}", session_id);

        // 执行退避
        if apply_retry_strategy(
            strategy.clone(),
            attempt,
            max_attempts,
            status_code,
            &trace_id,
        )
        .await
        {
            if matches!(strategy, RetryStrategy::GraceRetry(_)) {
                retry_credentials = Some((
                    access_token.clone(),
                    project_id.clone(),
                    email.clone(),
                    account_id.clone(),
                    0,
                ));
            }
            // [NEW] Apply Client Adapter "let_it_crash" strategy
            if let Some(adapter) = &client_adapter {
                if adapter.let_it_crash() && attempt > 0 {
                    tracing::warn!(
                        "[Gemini] let_it_crash active: Aborting retries after attempt {}",
                        attempt
                    );
                    break;
                }
            }

            // 判断是否需要轮换账号
            if !should_rotate_account(status_code, Some(&strategy)) {
                debug!(
                "[{}] Keeping same account for status {} (Gemini server-side issue or Grace Retry)",
                trace_id, status_code
            );
                force_rotate = false;
            } else {
                force_rotate = true;
            }

            continue;
        }

        // 404 等由于模型配置或路径错误的 HTTP 异常，直接报错，不进行无效轮换
        error!(
            "Gemini Upstream non-retryable error {}: {}",
            status_code, error_text
        );
        let dual_err = crate::proxy::handlers::common::build_dual_track_error(
            "gemini",
            status_code,
            &mapped_model,
            &error_text,
        );
        return Ok((
            status,
            [
                ("X-Account-Email", email.as_str()),
                ("X-Mapped-Model", mapped_model.as_str()),
                ("X-Upstream-Model", config.final_model.as_str()),
            ],
            Json(dual_err),
        )
            .into_response());
    }

    // 所有尝试均失败：仅当全部结构化失败状态均为 429 时返回 429
    let final_status = failure_statuses.final_status();
    let headers = build_token_error_headers(
        Some(initial_mapped_model.as_str()),
        last_email.as_deref(),
        &last_error,
    );

    let dual_err = crate::proxy::handlers::common::build_dual_track_error(
        "gemini",
        final_status.as_u16(),
        initial_mapped_model.as_str(),
        &last_error,
    );

    Ok((final_status, headers, Json(dual_err)).into_response())
}

pub async fn handle_list_models(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    use crate::proxy::common::model_mapping::{
        get_all_dynamic_models, get_all_dynamic_models_metadata,
    };

    // 获取所有动态模型列表（与 /v1/models 一致）
    let only_raw = *state.only_raw_quota_models.read().await;
    let model_ids =
        get_all_dynamic_models(&state.custom_mapping, Some(&state.token_manager), only_raw).await;
    let metadata_map = get_all_dynamic_models_metadata(
        &state.custom_mapping,
        Some(&state.token_manager),
        only_raw,
    )
    .await;

    // 转换为 Gemini API 格式
    let models: Vec<_> = model_ids
        .into_iter()
        .map(|id| {
            let (fallback_in, fallback_out) =
                crate::proxy::handlers::openai::resolve_model_token_limits(&id);
            let meta = metadata_map
                .get(&id)
                .or_else(|| metadata_map.get(&id.to_lowercase()));
            let display_name = meta
                .and_then(|m| m.display_name.clone())
                .unwrap_or_else(|| id.clone());
            let input_limit = meta
                .and_then(|m| m.max_tokens)
                .map(|v| v as i64)
                .unwrap_or(fallback_in);
            let output_limit = meta
                .and_then(|m| m.max_output_tokens)
                .map(|v| v as i64)
                .unwrap_or(fallback_out);

            json!({
                "name": format!("models/{}", id),
                "version": "001",
                "displayName": display_name,
                "description": "",
                "inputTokenLimit": input_limit,
                "outputTokenLimit": output_limit,
                "maxTokens": input_limit,
                "maxOutputTokens": output_limit,
                "supportedGenerationMethods": ["generateContent", "countTokens"],
                "temperature": 1.0,
                "topP": 0.95,
                "topK": 64
            })
        })
        .collect();

    Ok(Json(json!({ "models": models })))
}

pub async fn handle_get_model(
    State(state): State<AppState>,
    Path(model_name): Path<String>,
) -> impl IntoResponse {
    use crate::proxy::common::model_mapping::{
        find_dynamic_model, get_all_dynamic_models_metadata,
    };

    // [MODEL GATE] GET /v1beta/models/:model 探测端点同样接入门禁（含已下线模型族 404）
    if let Err(rejection) = crate::proxy::handlers::common::enforce_model_gate(
        &state,
        Some(model_name.as_str()),
        "gemini",
        None,
    )
    .await
    {
        return rejection;
    }

    let only_raw = *state.only_raw_quota_models.read().await;
    if let Some(matched_id) = find_dynamic_model(
        &state.custom_mapping,
        Some(&state.token_manager),
        only_raw,
        &model_name,
    )
    .await
    {
        let metadata_map = get_all_dynamic_models_metadata(
            &state.custom_mapping,
            Some(&state.token_manager),
            only_raw,
        )
        .await;

        let (fallback_in, fallback_out) =
            crate::proxy::handlers::openai::resolve_model_token_limits(&matched_id);
        let meta = metadata_map
            .get(&matched_id)
            .or_else(|| metadata_map.get(&matched_id.to_lowercase()));
        let display_name = meta
            .and_then(|m| m.display_name.clone())
            .unwrap_or_else(|| matched_id.clone());
        let input_limit = meta
            .and_then(|m| m.max_tokens)
            .map(|v| v as i64)
            .unwrap_or(fallback_in);
        let output_limit = meta
            .and_then(|m| m.max_output_tokens)
            .map(|v| v as i64)
            .unwrap_or(fallback_out);

        (
            StatusCode::OK,
            Json(json!({
                "name": format!("models/{}", matched_id),
                "version": "001",
                "displayName": display_name,
                "description": "",
                "inputTokenLimit": input_limit,
                "outputTokenLimit": output_limit,
                "maxTokens": input_limit,
                "maxOutputTokens": output_limit,
                "supportedGenerationMethods": ["generateContent", "countTokens"],
                "temperature": 1.0,
                "topP": 0.95,
                "topK": 64
            })),
        )
            .into_response()
    } else {
        let clean_name = model_name.strip_prefix("models/").unwrap_or(&model_name);
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": {
                    "code": 404,
                    "message": format!(
                        "models/{} is not found for API version v1beta, or is not supported for generateContent. Call ListModels to see the list of available models and their supported methods.",
                        clean_name
                    ),
                    "status": "NOT_FOUND"
                }
            })),
        )
            .into_response()
    }
}

pub async fn handle_count_tokens(
    State(state): State<AppState>,
    Path(model_name): Path<String>,
    headers: HeaderMap,
    user_identity: Option<
        axum::extract::Extension<crate::proxy::middleware::auth::UserTokenIdentity>,
    >,
    Json(body): Json<Value>,
) -> Response {
    // [MODEL GATE] POST /v1beta/models/:model/countTokens 探测端点同样接入门禁
    if let Err(rejection) = crate::proxy::handlers::common::enforce_model_gate(
        &state,
        Some(model_name.as_str()),
        "gemini",
        None,
    )
    .await
    {
        return rejection;
    }

    execute_count_tokens(
        state,
        headers,
        user_identity.map(|identity| identity.token_id.clone()),
        model_name,
        body,
    )
    .await
}

/// 核心 countTokens 实现：透明代理到上游 v1internal:countTokens
///
/// 获取有效 OAuth Token，将标准 Gemini 请求体包装为 v1internal 格式后转发，
/// 返回真实的 token 计数，而不是硬编码的 0
pub async fn execute_count_tokens(
    state: AppState,
    headers: HeaderMap,
    tenant_id: Option<String>,
    model_name: String,
    mut body: Value,
) -> Response {
    // [DEFENSE] 净化 Gemini 原生请求体中的所有 inlineData (过滤或降级空数据/损坏图片)
    crate::proxy::mappers::common_utils::sanitize_gemini_payload_inline_data(&mut body);

    // 1. 模型路由解析
    let client_effort = body
        .get("request")
        .unwrap_or(&body)
        .get("generationConfig")
        .and_then(|gc| gc.get("thinkingConfig"))
        .and_then(|tc| tc.get("thinkingLevel").or_else(|| tc.get("effort")))
        .and_then(|v| v.as_str());

    let mapped_model = crate::proxy::common::model_mapping::resolve_model_route_with_effort(
        &model_name,
        &*state.custom_mapping.read().await,
        client_effort,
    );

    // 2. 解析请求配置并获取 Token
    let config = crate::proxy::mappers::common_utils::resolve_request_config(
        &model_name,
        &mapped_model,
        &None,
        None,
        None,
        None,
        Some(&body),
    );

    let anchor = SessionManager::gemini_content_anchor(&body);
    let session_scope = crate::proxy::thinking_store::SessionScope::resolve(
        &headers,
        Some(&body),
        None,
        anchor,
        tenant_id.as_deref(),
    );

    let (access_token, _project_id, email, account_id, _wait_ms) = match state
        .token_manager
        .get_token(
            &config.request_type,
            false,
            Some(&session_scope.affinity_key),
            &config.final_model,
        )
        .await
    {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(
                "[Gemini] Token acquisition failed for countTokens (error: {}), falling back to PipelineTokenEstimator",
                e
            );
            let local_tokens = crate::proxy::pipeline::estimate_tokens(&body);
            return (
                StatusCode::OK,
                [
                    ("X-Account-Email", "offline-estimate"),
                    ("X-Mapped-Model", mapped_model.as_str()),
                    ("X-Upstream-Model", config.final_model.as_str()),
                ],
                Json(json!({ "totalTokens": local_tokens })),
            )
                .into_response();
        }
    };

    // 3. 包装为 v1internal 格式
    // [已验证] countTokens 与 generateContent 不同: 顶层只允许 "request" 键,
    // 携带 model/project 会被上游 400 拒绝 (Unknown name "model"/"project");
    // request 内的 safetySettings 同样不被接受 (对齐 CLIProxyAPI 的处理)
    let mut inner_body = body.clone();
    if let Some(obj) = inner_body.as_object_mut() {
        obj.remove("safetySettings");
    }
    let wrapped_body = json!({
        "request": inner_body,
    });

    // 4. 调用上游 v1internal:countTokens
    let call_result = match state
        .upstream
        .call_v1_internal_with_headers(
            "countTokens",
            &access_token,
            wrapped_body,
            None,
            std::collections::HashMap::new(),
            Some(account_id.as_str()),
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                "[Gemini] Upstream call error for countTokens ({}), falling back to PipelineTokenEstimator",
                e
            );
            let local_tokens = crate::proxy::pipeline::estimate_tokens(&body);
            return (
                StatusCode::OK,
                [
                    ("X-Account-Email", email.as_str()),
                    ("X-Mapped-Model", mapped_model.as_str()),
                    ("X-Upstream-Model", config.final_model.as_str()),
                ],
                Json(json!({ "totalTokens": local_tokens })),
            )
                .into_response();
        }
    };

    let response = call_result.response;
    let status = response.status();

    if !status.is_success() {
        let err_text = response.text().await.unwrap_or_default();
        tracing::warn!(
            "[Gemini] Upstream countTokens error (status: {}, body: {}), falling back to PipelineTokenEstimator",
            status,
            err_text
        );
        let local_tokens = crate::proxy::pipeline::estimate_tokens(&body);
        return (
            StatusCode::OK,
            [
                ("X-Account-Email", email.as_str()),
                ("X-Mapped-Model", mapped_model.as_str()),
                ("X-Upstream-Model", config.final_model.as_str()),
            ],
            Json(json!({ "totalTokens": local_tokens })),
        )
            .into_response();
    }

    let gemini_resp: Value = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("Parse error: {}", e) })),
            )
                .into_response();
        }
    };

    // 5. 提取 totalTokens (兼容 wrapped / unwrapped 两种响应格式)
    let total_tokens = gemini_resp
        .get("response")
        .and_then(|r| r.get("totalTokens"))
        .or_else(|| gemini_resp.get("totalTokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    // 6. 返回标准 Gemini REST 响应
    (
        StatusCode::OK,
        [
            ("X-Account-Email", email.as_str()),
            ("X-Mapped-Model", mapped_model.as_str()),
            ("X-Upstream-Model", config.final_model.as_str()),
        ],
        Json(json!({ "totalTokens": total_tokens })),
    )
        .into_response()
}
