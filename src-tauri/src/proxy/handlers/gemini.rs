// Gemini Handler
use axum::{
    extract::State,
    extract::{Json, Path},
    http::StatusCode,
    response::IntoResponse,
};
use serde_json::{json, Value};
use tracing::{debug, error, info};

use crate::proxy::common::client_adapter::CLIENT_ADAPTERS;
use crate::proxy::debug_logger;
use crate::proxy::handlers::common::{
    apply_retry_strategy, determine_retry_strategy, should_rotate_account,
};
use crate::proxy::mappers::gemini::{unwrap_response, wrap_request};
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
    headers: HeaderMap,          // [NEW] Extract headers for adapter detection
    Json(mut body): Json<Value>, // 改为 mut 以支持修复提示词注入
) -> Result<impl IntoResponse, (StatusCode, String)> {
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

    // 1. 验证方法
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
    let token_manager = state.token_manager;
    let pool_size = token_manager.len();
    let max_attempts = MAX_RETRY_ATTEMPTS.min(pool_size).max(1);

    let mut last_error = String::new();
    let mut last_email: Option<String> = None;

    for attempt in 0..max_attempts {
        // 3. 模型路由解析
        let mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
            &model_name,
            &*state.custom_mapping.read().await,
        );
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
        let session_id = SessionManager::extract_gemini_session_id(&body, &model_name);

        // 关键：在重试尝试 (attempt > 0) 时强制轮换账号
        let (access_token, project_id, email, account_id, _wait_ms) = match token_manager
            .get_token(
                &config.request_type,
                attempt > 0,
                Some(&session_id),
                &config.final_model,
            )
            .await
        {
            Ok(t) => t,
            Err(e) => {
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!("Token error: {}", e),
                ));
            }
        };

        last_email = Some(email.clone());
        info!("✓ Using account: {} (type: {})", email, config.request_type);

        // 5. 包装请求 (project injection)
        // [FIX #765] Pass session_id to wrap_request for signature injection
        // [NEW] 查询动态限额 (dynamic > static default > 131072)
        let dynamic_cap = token_manager.get_model_output_limit_for_account(&account_id, &mapped_model);
        let wrapped_body = wrap_request(&body, &project_id, &mapped_model, Some(account_id.as_str()), Some(&session_id), dynamic_cap);

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
        if mapped_model.to_lowercase().contains("claude") {
            extra_headers.insert("anthropic-beta".to_string(), "claude-code-20250219,interleaved-thinking-2025-05-14,fine-grained-tool-streaming-2025-05-14".to_string());
            tracing::debug!(
                "[Gemini] Injected Anthropic beta headers for Claude model: {}",
                mapped_model
            );
        }

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

                match tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    response_stream.next(),
                )
                .await
                {
                    Ok(Some(Ok(bytes))) => {
                        if bytes.is_empty() {
                            tracing::warn!("[Gemini] Empty first chunk received, retrying...");
                            retry_gemini = true;
                        } else {
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
                        tracing::warn!("[Gemini] Timeout waiting for first chunk, retrying...");
                        last_error = "Timeout".to_string();
                        retry_gemini = true;
                    }
                }

                if retry_gemini {
                    continue;
                }

                let s_id_for_stream = s_id.clone();
                let model_name_for_stream = mapped_model.clone();
                let stream = async_stream::stream! {
                    let mut first_data = first_chunk;
                    loop {
                        let item = if let Some(fd) = first_data.take() {
                            Some(Ok(fd))
                        } else {
                            response_stream.next().await
                        };

                        let bytes = match item {
                            Some(Ok(b)) => b,
                            Some(Err(e)) => {
                                error!("[Gemini-SSE] Connection error: {}", e);
                                yield Err(format!("Stream error: {}", e));
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
                                                                if let Some(sig) = part.get("thoughtSignature").and_then(|s| s.as_str()) {
                                                                    crate::proxy::SignatureCache::global()
                                                                        .cache_session_signature(&s_id_for_stream, sig.to_string(), 1);
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
                        .body(body)
                        .unwrap()
                        .into_response());
                } else {
                    // Collect to JSON
                    use crate::proxy::mappers::gemini::collector::collect_stream_to_json;
                    match collect_stream_to_json(Box::pin(stream), &s_id).await {
                        Ok(gemini_resp) => {
                            info!(
                                "[{}] ✓ Stream collected and converted to JSON (Gemini)",
                                session_id
                            );
                            let unwrapped = unwrap_response(&gemini_resp);
                            return Ok((
                                StatusCode::OK,
                                [
                                    ("X-Account-Email", email.as_str()),
                                    ("X-Mapped-Model", mapped_model.as_str()),
                                ],
                                Json(unwrapped),
                            )
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
                                    debug!("[Gemini-Response] Cached signature (len: {}) for session: {}", sig.len(), session_id);
                                }
                            }
                        }
                    }
                }
            }

            let unwrapped = unwrap_response(&gemini_resp);
            return Ok((
                StatusCode::OK,
                [
                    ("X-Account-Email", email.as_str()),
                    ("X-Mapped-Model", mapped_model.as_str()),
                ],
                Json(unwrapped),
            )
                .into_response());
        }

        // 处理错误并重试
        let status_code = status.as_u16();
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

        // 确定重试策略
        let strategy = determine_retry_strategy(status_code, &error_text, false);
        let trace_id = format!("gemini_{}", session_id);

        // 执行退避
        if apply_retry_strategy(strategy, attempt, max_attempts, status_code, &trace_id).await {
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
            if !should_rotate_account(status_code) {
                debug!(
                    "[{}] Keeping same account for status {} (Gemini server-side issue)",
                    trace_id, status_code
                );
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

    if let Some(email) = last_email {
        Ok((
            StatusCode::TOO_MANY_REQUESTS,
            [("X-Account-Email", email)],
            format!("All accounts exhausted. Last error: {}", last_error),
        )
            .into_response())
    } else {
        Ok((
            StatusCode::TOO_MANY_REQUESTS,
            format!("All accounts exhausted. Last error: {}", last_error),
        )
            .into_response())
    }
}

pub async fn handle_list_models(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    use crate::proxy::common::model_mapping::get_all_dynamic_models;

    // 获取所有动态模型列表（与 /v1/models 一致）
    let model_ids = get_all_dynamic_models(&state.custom_mapping, Some(&state.token_manager)).await;

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
    Path(_model_name): Path<String>,
    Json(_body): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let model_group = "gemini";
    let (_access_token, _project_id, _, _, _wait_ms) = state
        .token_manager
        .get_token(model_group, false, None, "gemini")
        .await
        .map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Token error: {}", e),
            )
        })?;

    Ok(Json(json!({"totalTokens": 0})))
}
