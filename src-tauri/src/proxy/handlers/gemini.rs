// Gemini Handler
use axum::{
    body::Body,
    extract::State,
    extract::{Json, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{json, Value};
use tracing::{debug, error, info};

use crate::proxy::common::client_adapter::CLIENT_ADAPTERS;
use crate::proxy::debug_logger;
use crate::proxy::handlers::common::{
    apply_retry_strategy, build_token_error_headers, next_rotation_attempt, should_rotate_account,
    FailureStatusTracker, RequestRetryState, RetryStrategy,
};
use crate::proxy::mappers::gemini::{unwrap_response, wrap_request, wrap_request_v2};
use crate::proxy::server::AppState;
use crate::proxy::session_manager::SessionManager;
use crate::proxy::upstream::client::mask_email;
use axum::http::HeaderMap;

const MAX_RETRY_ATTEMPTS: usize = 3;

fn response_has_inline_image_data(value: &Value) -> bool {
    let response = value.get("response").unwrap_or(value);
    response
        .get("candidates")
        .and_then(Value::as_array)
        .is_some_and(|candidates| {
            candidates.iter().any(|candidate| {
                candidate
                    .get("content")
                    .and_then(|content| content.get("parts"))
                    .and_then(Value::as_array)
                    .is_some_and(|parts| {
                        parts.iter().any(|part| {
                            part.get("inlineData")
                                .or_else(|| part.get("inline_data"))
                                .and_then(|image| image.get("data"))
                                .and_then(Value::as_str)
                                .is_some_and(|data| !data.is_empty())
                        })
                    })
            })
        })
}

#[cfg(test)]
mod image_success_tests {
    use super::response_has_inline_image_data;
    use serde_json::json;

    #[test]
    fn task_gemini_image_success_requires_nonempty_payload() {
        let empty = json!({
            "response": {"candidates": [{"content": {"parts": [{"inlineData": {"data": ""}}]}}]}
        });
        let image = json!({
            "response": {"candidates": [{"content": {"parts": [{"inlineData": {"data": "AQ=="}}]}}]}
        });
        assert!(!response_has_inline_image_data(&empty));
        assert!(response_has_inline_image_data(&image));
    }
}

/// 处理 generateContent 和 streamGenerateContent
/// 路径参数: model_name, method (e.g. "gemini-pro", "generateContent")
pub async fn handle_generate(
    State(state): State<AppState>,
    Path(model_action): Path<String>,
    headers: HeaderMap, // [NEW] Extract headers for adapter detection
    upstream_recorder: Option<
        axum::extract::Extension<crate::proxy::monitor::UpstreamRequestBodyHolder>,
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

    // [Stage Timing] 阶段耗时度量变量 (毫秒)
    let clean_micros = clean_start.elapsed().as_micros() as u64;
    let clean_ms: f64 = clean_micros as f64 / 1000.0;
    let mut norm_ms: f64 = 0.0;
    let mut think_fill_ms: f64 = 0.0;
    let mut ttft_ms: f64 = 0.0;

    // 1. 验证方法
    // [NEW] :countTokens 冒号语法，直接代理到上游 v1internal:countTokens
    if method == "countTokens" {
        return Ok(execute_count_tokens(state, model_name, body).await);
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

    // 2. 获取 UpstreamClient 和 TokenManager
    let upstream = state.upstream.clone();
    let image_scheduler = state.image_scheduler.clone();
    let request_timeout = state.request_timeout;
    let token_manager = state.token_manager;
    let pool_size = token_manager.len();
    let max_attempts = MAX_RETRY_ATTEMPTS.min(pool_size).max(1);

    let mut last_error = String::new();
    let mut last_email: Option<String> = None;
    let mut force_rotate = false;
    let mut retry_state = RequestRetryState::default();
    let mut retry_credentials: Option<(String, String, String, String, u64)> = None;
    let mut image_permit = None;
    let mut failure_statuses = FailureStatusTracker::default();
    let mut used_attempts = 0;

    let initial_mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
        &model_name,
        &*state.custom_mapping.read().await,
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
        // 提取 tools 列表以进行联网探测 (Gemini 风格可能是嵌套的)
        let tools_val: Option<Vec<Value>> =
            body.get("tools").and_then(|t| t.as_array()).map(|arr| {
                let mut flattened = Vec::new();
                for tool_entry in arr {
                    if let Some(decls) = tool_entry
                        .get("functionDeclarations")
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
            None,        // size (not applicable for Gemini native protocol)
            None,        // quality
            None,        // [NEW] image_size
            Some(&body), // [NEW] Pass request body for imageConfig parsing
        );

        // 4. 获取 Token (使用准确的 request_type)
        // 提取 SessionId (粘性指纹)
        let fallback_sid = SessionManager::extract_gemini_session_id(&body, &model_name);
        let session_scope = crate::proxy::thinking_store::SessionScope::from_headers_and_body(
            &headers,
            Some(&body),
            fallback_sid,
        );
        let session_id = session_scope.store_key.clone();
        let client_session_id = session_scope.client_id.clone();

        // 关键：根据 force_rotate 标志决定是否轮换账号（支持 Grace Retry 原地重试）
        let (access_token, project_id, email, account_id, _wait_ms) =
            if let Some(credentials) = retry_credentials.take() {
                credentials
            } else if config.request_type == "image_gen" {
                drop(image_permit.take());
                match token_manager
                    .get_image_token(
                        force_rotate,
                        Some(&session_id),
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
                match token_manager
                    .get_token(
                        &config.request_type,
                        force_rotate,
                        Some(&session_id),
                        &config.final_model,
                    )
                    .await
                {
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
        let mut wrapped_body = wrap_request_v2(
            &body,
            &project_id,
            &mapped_model,
            Some(account_id.as_str()),
            Some(&session_id),
            token_obj.as_ref(),
            Some(&token_manager),
        );
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
            .get("contents")
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
                wrapped_body,
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
                debug!(
                    "Gemini Request failed on attempt {}/{}: {}",
                    attempt + 1,
                    max_attempts,
                    e
                );
                continue;
            }
        };

        // [NEW] 记录端点降级日志到 debug 文件
        if !call_result.fallback_attempts.is_empty() && debug_logger::is_enabled(&debug_cfg) {
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
            // 6. 响应处理
            if is_stream {
                use axum::body::Body;
                use axum::response::Response;
                use bytes::{Bytes, BytesMut};
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
                let mut response_stream = debug_logger::wrap_stream_with_debug(
                    Box::pin(response.bytes_stream()),
                    debug_cfg.clone(),
                    trace_id.clone(),
                    "upstream_response",
                    meta,
                );
                let mut buffer = BytesMut::new();
                let s_id = session_id.clone(); // Clone for stream closure

                // [FIX #859] Implement peek logic for Gemini stream to prevent 0-token 200 OK
                let mut first_chunk = None;
                let mut retry_gemini = false;

                // [NEW] 实施双阶段超时：第一阶段为 FirstChunkTimeout (300s / 5min)
                // 这精准对齐了官方 Worker 在模型冷启动（Initialization）阶段的极度耐心
                match tokio::time::timeout(
                    std::time::Duration::from_secs(300),
                    response_stream.next(),
                )
                .await
                {
                    Ok(Some(Ok(bytes))) => {
                        if bytes.is_empty() {
                            tracing::warn!("[Gemini] Empty first chunk received, retrying...");
                            retry_gemini = true;
                        } else {
                            ttft_ms = upstream_req_start.elapsed().as_micros() as f64 / 1000.0;
                            first_chunk = Some(bytes);
                        }
                    }
                    Ok(Some(Err(e))) => {
                        tracing::warn!("[Gemini] Stream error during peek: {}, retrying...", e);
                        last_error = format!("Stream error: {}", e);
                        retry_gemini = true;
                    }
                    Ok(None) => {
                        tracing::warn!("[Gemini] Stream ended immediately, retrying...");
                        last_error = "Empty response".to_string();
                        retry_gemini = true;
                    }
                    Err(_) => {
                        tracing::warn!("[Gemini] First chunk timeout after 300s, retrying...");
                        last_error = "First chunk timeout".to_string();
                        retry_gemini = true;
                    }
                }

                if retry_gemini {
                    failure_statuses.record(StatusCode::BAD_GATEWAY);
                    continue;
                }
                let s_id_for_stream = s_id.clone();
                let model_name_for_stream = mapped_model.clone();
                let image_permit_for_stream = image_permit.take();
                let track_image_success = config.request_type == "image_gen";
                let image_success_manager = token_manager.clone();
                let image_success_account = account_id.clone();
                let image_success_model = mapped_model.clone();
                let causal_anchor_clone = causal_anchor.clone();
                let stream = async_stream::stream! {
                    let _image_permit = image_permit_for_stream;
                    let mut first_data = first_chunk;
                    let mut meta_sent = false;
                    let mut saw_image_data = false;
                    let mut stream_failed = false;
                    let mut thinking_acc =
                        crate::proxy::thinking_store::TurnAccumulator::with_anchor(&causal_anchor_clone);

                    loop {
                        // [NEW] 阶段 6.2: 补全 __cloudCodeMeta 响应元数据透传
                        // 官方 Worker 会将 TraceID 作为 SSE 流的第 0 个数据包下发
                        if !meta_sent {
                            if let Some(tid) = &cloud_code_trace_id {
                                let meta_pkg = serde_json::json!({
                                    "__cloudCodeMeta": {
                                        "traceId": tid
                                    }
                                });
                                yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&meta_pkg).unwrap())));
                            }
                            meta_sent = true;
                        }

                        let item = if let Some(fd) = first_data.take() {
                            Some(Ok(fd))
                        } else {
                            // [NEW] 第二阶段为 StreamIdleTimeout (300s / 5min)
                            match tokio::time::timeout(std::time::Duration::from_secs(300), response_stream.next()).await {
                                Ok(next_item) => next_item,
                                Err(_) => {
                                    error!("[Gemini-SSE] Idle timeout after 300s, terminating stream");
                                    stream_failed = true;
                                    None
                                }
                            }
                        };

                        let bytes = match item {
                            Some(Ok(b)) => b,
                            Some(Err(e)) => {
                                error!("[Gemini-SSE] Stream error: {}", e);
                                stream_failed = true;
                                let error_json = serde_json::json!({
                                    "id": &s_id_for_stream,
                                    "object": "chat.completion.chunk",
                                    "model": &model_name_for_stream,
                                    "choices": [
                                        {
                                            "index": 0,
                                            "delta": {
                                                "content": format!("\n[Stream Error] {}", e)
                                            },
                                            "finish_reason": "error"
                                        }
                                    ]
                                });
                                yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&error_json).unwrap_or_default())));
                                yield Ok::<Bytes, String>(Bytes::from("data: [DONE]\n\n"));
                                break;
                            }
                            None => break,
                        };

                        debug!("[Gemini-SSE] Received chunk: {} bytes", bytes.len());
                        buffer.extend_from_slice(&bytes);
                        while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                            let line_raw = buffer.split_to(pos + 1);
                            if let Ok(line_str) = std::str::from_utf8(&line_raw) {
                                let line = line_str.trim();
                                if line.is_empty() { continue; }

                                if line.starts_with("data: ") {
                                    let json_part = line.trim_start_matches("data: ").trim();
                                    if json_part == "[DONE]" {
                                        yield Ok::<Bytes, String>(Bytes::from("data: [DONE]\n\n"));
                                        continue;
                                    }

                                    match serde_json::from_str::<Value>(json_part) {
                                        Ok(mut json) => {
                                            if track_image_success && response_has_inline_image_data(&json) {
                                                saw_image_data = true;
                                            }
                                            // [FIX #765] Extract thoughtSignature from stream
                                            let inner_val = if json.get("response").is_some() {
                                                json.get("response")
                                            } else {
                                                Some(&json)
                                            };

                                            if let Some(resp) = inner_val {
                                                if let Some(candidates) = resp.get("candidates").and_then(|c| c.as_array()) {
                                                    for cand in candidates {
                                                        if let Some(parts) = cand.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                                                            for part in parts {
                                                                thinking_acc.ingest_part(part);
                                                                if let Some(sig) = part.get("thoughtSignature").and_then(|s| s.as_str()) {
                                                                    crate::proxy::SignatureCache::global()
                                                                        .cache_session_signature(&s_id_for_stream, sig.to_string(), 1);
                                                                    if let Some(call_id) = part.get("functionCall").and_then(|f| f.get("id")).and_then(|id| id.as_str()) {
                                                                        crate::proxy::SignatureCache::global().cache_tool_signature(call_id, sig.to_string());
                                                                    }
                                                                    debug!("[Gemini-SSE] Cached signature (len: {}) for session: {}", sig.len(), s_id_for_stream);
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }

                                            // [FIX #1522] Inject Tool ID into Stream Response
                                            crate::proxy::mappers::gemini::wrapper::inject_ids_to_response(&mut json, &model_name_for_stream);

                                            // Unwrap v1internal response wrapper
                                            if let Some(inner) = json.get_mut("response").map(|v| v.take()) {
                                                let new_line = format!("data: {}\n\n", serde_json::to_string(&inner).unwrap_or_default());
                                                yield Ok::<Bytes, String>(Bytes::from(new_line));
                                            } else {
                                                yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&json).unwrap_or_default())));
                                            }
                                        }
                                        Err(e) => {
                                            debug!("[Gemini-SSE] JSON parse error: {}, passing raw line", e);
                                            stream_failed = true;
                                            yield Ok::<Bytes, String>(Bytes::from(format!("{}\n\n", line)));
                                        }
                                    }
                                } else {
                                    // Non-data lines (comments, etc.)
                                    yield Ok::<Bytes, String>(Bytes::from(format!("{}\n\n", line)));
                                }
                            } else {
                                // Non-UTF8 data? Just pass it through or skip
                                debug!("[Gemini-SSE] Non-UTF8 line encountered");
                                yield Ok::<Bytes, String>(line_raw.freeze());
                            }
                        }
                    }

                    thinking_acc.commit(&s_id_for_stream);
                    if track_image_success && saw_image_data && !stream_failed {
                        image_success_manager.mark_account_success(&image_success_account);
                        image_success_manager
                            .clear_persisted_live_limit(
                                &image_success_account,
                                Some(&image_success_model),
                            );
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
                        .header("X-Session-Id", &client_session_id)
                        .header("X-Antigravity-Session-Id", &client_session_id)
                        .header("X-Timing-Clean-Ms", format!("{:.3}", clean_ms))
                        .header("X-Timing-Norm-Ms", format!("{:.3}", norm_ms))
                        .header("X-Timing-Thinking-Ms", format!("{:.3}", think_fill_ms))
                        .header("X-Timing-Ttft-Ms", format!("{:.3}", ttft_ms))
                        .body(body)
                        .unwrap()
                        .into_response());
                } else {
                    // Collect to JSON
                    use crate::proxy::mappers::gemini::collector::collect_stream_to_json_with_anchor;
                    match collect_stream_to_json_with_anchor(
                        Box::pin(stream),
                        &s_id,
                        Some(&causal_anchor),
                    )
                    .await
                    {
                        Ok(gemini_resp) => {
                            info!(
                                "[{}] ✓ Stream collected and converted to JSON (Gemini)",
                                session_id
                            );
                            let unwrapped = unwrap_response(&gemini_resp);
                            return Ok(Response::builder()
                                .status(StatusCode::OK)
                                .header("Content-Type", "application/json")
                                .header("X-Account-Email", &email)
                                .header("X-Mapped-Model", &mapped_model)
                                .header("X-Session-Id", &client_session_id)
                                .header("X-Antigravity-Session-Id", &client_session_id)
                                .header("X-Timing-Clean-Ms", format!("{:.3}", clean_ms))
                                .header("X-Timing-Norm-Ms", format!("{:.3}", norm_ms))
                                .header("X-Timing-Thinking-Ms", format!("{:.3}", think_fill_ms))
                                .header("X-Timing-Ttft-Ms", format!("{:.3}", ttft_ms))
                                .body(Body::from(serde_json::to_string(&unwrapped).unwrap()))
                                .unwrap()
                                .into_response());
                        }
                        Err(e) => {
                            error!("Stream collection error: {}", e);
                            return Ok((
                                StatusCode::INTERNAL_SERVER_ERROR,
                                format!("Stream collection error: {}", e),
                            )
                                .into_response());
                        }
                    }
                }
            }

            ttft_ms = upstream_req_start.elapsed().as_micros() as f64 / 1000.0;
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
                                        1,
                                    );
                                    if let Some(call_id) = part
                                        .get("functionCall")
                                        .and_then(|f| f.get("id"))
                                        .and_then(|id| id.as_str())
                                    {
                                        crate::proxy::SignatureCache::global()
                                            .cache_tool_signature(call_id, sig.to_string());
                                    }
                                    debug!("[Gemini-Response] Cached signature (len: {}) for session: {}", sig.len(), session_id);
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
                .header("X-Session-Id", &client_session_id)
                .header("X-Antigravity-Session-Id", &client_session_id)
                .header("X-Timing-Clean-Ms", format!("{:.3}", clean_ms))
                .header("X-Timing-Norm-Ms", format!("{:.3}", norm_ms))
                .header("X-Timing-Thinking-Ms", format!("{:.3}", think_fill_ms))
                .header("X-Timing-Ttft-Ms", format!("{:.3}", ttft_ms))
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
                }

                // 设置 is_forbidden 状态并持久化
                if let Err(e) = token_manager.set_forbidden(&acc_id, &error_text).await {
                    tracing::error!("Failed to set forbidden status: {}", e);
                }
            }
        }

        // [FIX session-1M] 上游按 sessionId 在服务端累计会话输入，长工具循环会把累计推过 1M，
        // 之后该 sessionId 的所有请求都 400 "input token count exceeds ... 1048576"。
        // 给 (账号, 对话) 的 sessionId 升代并立即重试:新 sessionId = 上游全新会话,对话无感恢复。
        if status_code == 400 && error_text.contains("exceeds the maximum number of tokens") {
            let fingerprint = session_id.as_str();
            let generation = crate::proxy::common::session::bump_session(&account_id, fingerprint);
            tracing::warn!(
                "[Gemini] Upstream session token accumulation exceeded 1M on account {}. sessionId bumped to generation {}, retrying with a fresh upstream session.",
                email, generation
            );
            continue; // 重试:下一轮读取新代数,派生全新 sessionId
        }

        if status_code == 429 || status_code == 529 {
            token_manager
                .unbind_session_and_clear_last_used(Some(&session_id))
                .await;
            tracing::debug!(
                "[Gemini] Unbound session {} from account {} due to status {}",
                session_id,
                email,
                status_code
            );
        }

        let scheduling_mode = token_manager.get_scheduling_mode().await;
        let allow_grace = match scheduling_mode {
            crate::proxy::sticky_config::SchedulingMode::Balance => {
                token_manager.tokens_count() <= 1
            }
            crate::proxy::sticky_config::SchedulingMode::CacheFirst => true,
            crate::proxy::sticky_config::SchedulingMode::PerformanceFirst => false,
        };

        // 确定重试策略
        let strategy = retry_state.determine_strategy_with_grace(
            &account_id,
            status_code,
            &error_text,
            retry_after.as_deref(),
            false,
            allow_grace,
        );
        let needs_quota_refresh = if config.request_type == "image_gen" && status_code == 429 {
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

        // [NEW] 处理 400 错误 (Thinking 签名失效)
        if status_code == 400
            && (error_text.contains("Invalid `signature`")
                || error_text.contains("thinking.signature")
                || error_text.contains("Invalid signature")
                || error_text.contains("Corrupted thought signature"))
        {
            tracing::warn!(
                "[Gemini] Signature error detected on account {}, retrying without thinking",
                email
            );

            // 追加修复提示词到请求体的最后一条内容
            if let Some(contents) = body.get_mut("contents").and_then(|v| v.as_array_mut()) {
                if let Some(last_content) = contents.last_mut() {
                    if let Some(parts) =
                        last_content.get_mut("parts").and_then(|v| v.as_array_mut())
                    {
                        parts.push(json!({
                            "text": "\n\n[System Recovery] Your previous output contained an invalid signature. Please regenerate the response without the corrupted signature block."
                        }));
                        tracing::debug!("[Gemini] Appended repair prompt to last content");
                    }
                }
            }

            continue; // 重试
        }

        // 404 等由于模型配置或路径错误的 HTTP 异常，直接报错，不进行无效轮换
        error!(
            "Gemini Upstream non-retryable error {}: {}",
            status_code, error_text
        );
        return Ok((
            status,
            [
                ("X-Account-Email", email.as_str()),
                ("X-Mapped-Model", mapped_model.as_str()),
            ],
            // [FIX] Return JSON error
            Json(json!({
                "error": {
                    "code": status_code,
                    "message": error_text,
                    "status": "UPSTREAM_ERROR"
                }
            })),
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

    Ok((
        final_status,
        headers,
        format!("All accounts exhausted. Last error: {}", last_error),
    )
        .into_response())
}

pub async fn handle_list_models(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    use crate::proxy::common::model_mapping::get_all_dynamic_models;

    // 获取所有动态模型列表（与 /v1/models 一致）
    let only_raw = *state.only_raw_quota_models.read().await;
    let model_ids =
        get_all_dynamic_models(&state.custom_mapping, Some(&state.token_manager), only_raw).await;

    // 转换为 Gemini API 格式
    let models: Vec<_> = model_ids
        .into_iter()
        .map(|id| {
            json!({
                "name": format!("models/{}", id),
                "version": "001",
                "displayName": id.clone(),
                "description": "",
                "inputTokenLimit": 128000,
                "outputTokenLimit": 8192,
                "supportedGenerationMethods": ["generateContent", "countTokens"],
                "temperature": 1.0,
                "topP": 0.95,
                "topK": 64
            })
        })
        .collect();

    Ok(Json(json!({ "models": models })))
}

pub async fn handle_get_model(Path(model_name): Path<String>) -> impl IntoResponse {
    Json(json!({
        "name": format!("models/{}", model_name),
        "displayName": model_name
    }))
}

pub async fn handle_count_tokens(
    State(state): State<AppState>,
    Path(model_name): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    execute_count_tokens(state, model_name, body).await
}

/// 核心 countTokens 实现：透明代理到上游 v1internal:countTokens
///
/// 获取有效 OAuth Token，将标准 Gemini 请求体包装为 v1internal 格式后转发，
/// 返回真实的 token 计数，而不是硬编码的 0
pub async fn execute_count_tokens(
    state: AppState,
    model_name: String,
    mut body: Value,
) -> Response {
    // [DEFENSE] 净化 Gemini 原生请求体中的所有 inlineData (过滤或降级空数据/损坏图片)
    crate::proxy::mappers::common_utils::sanitize_gemini_payload_inline_data(&mut body);

    // 1. 模型路由解析
    let mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
        &model_name,
        &*state.custom_mapping.read().await,
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

    let session_id = SessionManager::extract_gemini_session_id(&body, &model_name);

    let (access_token, _project_id, email, account_id, _wait_ms) = match state
        .token_manager
        .get_token(
            &config.request_type,
            false,
            Some(&session_id),
            &config.final_model,
        )
        .await
    {
        Ok(t) => t,
        Err(e) => {
            let headers = build_token_error_headers(Some(mapped_model.as_str()), None, &e);
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                headers,
                Json(json!({ "error": format!("Token error: {}", e) })),
            )
                .into_response();
        }
    };

    // 3. 包装为 v1internal 格式
    // [已验证] countTokens 与 generateContent 不同: 顶层只允许 "request" 键,
    // 携带 model/project 会被上游 400 拒绝 (Unknown name "model"/"project");
    // request 内的 safetySettings 同样不被接受 (对齐 CLIProxyAPI 的处理)
    let mut inner_body = body;
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
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("Upstream call error: {}", e) })),
            )
                .into_response();
        }
    };

    let response = call_result.response;
    let status = response.status();

    if !status.is_success() {
        let err_text = response.text().await.unwrap_or_default();
        return (
            status,
            Json(json!({ "error": format!("Upstream countTokens error: {}", err_text) })),
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
        ],
        Json(json!({ "totalTokens": total_tokens })),
    )
        .into_response()
}
