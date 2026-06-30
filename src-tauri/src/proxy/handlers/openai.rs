// OpenAI Handler
use axum::{
    extract::Json, extract::State, http::StatusCode, response::IntoResponse, response::Response,
};
use base64::Engine as _;
use bytes::Bytes;
use serde_json::{json, Value};
use tracing::{debug, error, info}; // Import Engine trait for encode method

use crate::proxy::mappers::openai::{
    transform_openai_request, transform_openai_response, OpenAIRequest,
};
// use crate::proxy::upstream::client::UpstreamClient; // 通过 state 获取
use crate::proxy::debug_logger;
use crate::proxy::server::AppState;
use crate::proxy::upstream::client::mask_email;

const MAX_RETRY_ATTEMPTS: usize = 3;
use super::common::{
    apply_retry_strategy, determine_retry_strategy, should_rotate_account, RetryStrategy,
};
use crate::modules::account;
use crate::proxy::common::client_adapter::CLIENT_ADAPTERS; // [NEW] Adapter Registry
use crate::proxy::session_manager::SessionManager;
use axum::http::HeaderMap;
use tokio::time::Duration;

pub async fn handle_chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap, // [CHANGED] Extract headers
    Json(mut body): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // [NEW] Check for Image Model Redirection
    let model_name = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    // [FIX] Only redirect non-native image aliases (dall-e / midjourney) to the
    // images-generations shim. Native Gemini image models (gemini-3-pro-image*) must
    // flow through the normal pipeline (transform_openai_request -> resolve_request_config),
    // which correctly sets requestType=image_gen, imageConfig (size/aspect ratio), sessionId,
    // structured requestId and per-account dynamic model resolution — matching the official
    // Antigravity client. The old shim dropped `size` and built a divergent upstream body,
    // which caused image generation to silently fail for gemini-3-pro-image.
    if (model_name.contains("image")
        || model_name.contains("dall-e")
        || model_name.contains("midjourney"))
        && !model_name.contains("gemini")
    {
        tracing::info!(
            "[ChatRedirection] Redirecting model {} to image generations",
            model_name
        );
        return intercept_chat_to_image(state, body, &model_name).await;
    }

    // [FIX] 保存原始请求体的完整副本，用于日志记录
    // 这确保了即使结构体定义遗漏字段，日志也能完整记录所有参数
    let original_body = body.clone();

    // [NEW] 自动检测并转换 Responses 格式
    // 如果请求包含 instructions 或 input 但没有 messages，则认为是 Responses 格式
    let is_responses_format = !body.get("messages").is_some()
        && (body.get("instructions").is_some() || body.get("input").is_some());

    if is_responses_format {
        debug!("Detected Responses API format, converting to Chat Completions format");

        // 转换 instructions 为 system message
        if let Some(instructions) = body.get("instructions").and_then(|v| v.as_str()) {
            if !instructions.is_empty() {
                let system_msg = json!({
                    "role": "system",
                    "content": instructions
                });

                // 初始化 messages 数组
                if !body.get("messages").is_some() {
                    body["messages"] = json!([]);
                }

                // 将 system message 插入到开头
                if let Some(messages) = body.get_mut("messages").and_then(|v| v.as_array_mut()) {
                    messages.insert(0, system_msg);
                }
            }
        }

        // 转换 input 为 user message（如果存在）
        if let Some(input) = body.get("input") {
            let user_msg = if input.is_string() {
                json!({
                    "role": "user",
                    "content": input.as_str().unwrap_or("")
                })
            } else {
                // input 是数组格式，暂时简化处理
                json!({
                    "role": "user",
                    "content": input.to_string()
                })
            };

            if let Some(messages) = body.get_mut("messages").and_then(|v| v.as_array_mut()) {
                messages.push(user_msg);
            }
        }
    }

    let mut openai_req: OpenAIRequest = serde_json::from_value(body)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid request: {}", e)))?;

    // Safety: Ensure messages is not empty
    if openai_req.messages.is_empty() {
        debug!("Received request with empty messages, injecting fallback...");
        openai_req
            .messages
            .push(crate::proxy::mappers::openai::OpenAIMessage {
                role: "user".to_string(),
                content: Some(crate::proxy::mappers::openai::OpenAIContent::String(
                    " ".to_string(),
                )),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
                refusal: None,
            });
    }

    let trace_id = format!("req_{}", chrono::Utc::now().timestamp_subsec_millis());
    info!(
        "[{}] OpenAI Chat Request: {} | {} messages | stream: {}",
        trace_id,
        openai_req.model,
        openai_req.messages.len(),
        openai_req.stream
    );
    let debug_cfg = state.debug_logging.read().await.clone();
    
    let mut force_rotate = false;

    if debug_logger::is_enabled(&debug_cfg) {
        // [FIX] 使用原始 body 副本记录日志，确保不丢失任何字段
        let original_payload = json!({
            "kind": "original_request",
            "protocol": "openai",
            "trace_id": trace_id,
            "original_model": openai_req.model,
            "request": original_body,  // 使用原始请求体，不是结构体序列化
        });
        debug_logger::write_debug_payload(
            &debug_cfg,
            Some(&trace_id),
            "original_request",
            &original_payload,
        )
        .await;
    }

    // [NEW] Detect Client Adapter
    let client_adapter = CLIENT_ADAPTERS
        .iter()
        .find(|a| a.matches(&headers))
        .cloned();
    if client_adapter.is_some() {
        debug!("[{}] Client Adapter detected", trace_id);
    }

    // 1. 获取 UpstreamClient (Clone handle)
    let upstream = state.upstream.clone();
    let token_manager = state.token_manager;
    let pool_size = token_manager.len();
    // [FIX] Ensure max_attempts is at least 2 to allow for internal retries
    let max_attempts = MAX_RETRY_ATTEMPTS.min(pool_size.saturating_add(1)).max(2);

    let mut last_error = String::new();
    let mut last_email: Option<String> = None;

    // 2. 模型路由解析 (移到循环外以支持在所有路径返回 X-Mapped-Model)
    let mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
        &openai_req.model,
        &*state.custom_mapping.read().await,
    );

    for attempt in 0..max_attempts {
        // 将 OpenAI 工具转为 Value 数组以便探测联网
        let tools_val: Option<Vec<Value>> = openai_req
            .tools
            .as_ref()
            .map(|list| list.iter().cloned().collect());
        let config = crate::proxy::mappers::common_utils::resolve_request_config(
            &openai_req.model,
            &mapped_model,
            &tools_val,
            None, // size (not used in handler, transform_openai_request handles it)
            None, // quality
            None, // image_size
            None, // body
        );

        // 3. 提取 SessionId (粘性指纹)
        let session_id = SessionManager::extract_openai_session_id(&openai_req);

        // 4. 获取 Token (使用准确的 request_type)
        // 关键：在重试尝试时根据 force_rotate 决定是否轮换账号
        let (access_token, project_id, email, account_id, _wait_ms) = match token_manager
            .get_token(
                &config.request_type,
                force_rotate,
                Some(&session_id),
                &mapped_model,
            )
            .await
        {
            Ok(t) => t,
            Err(e) => {
                // [FIX] Attach headers to error response for logging visibility
                let headers = [("X-Mapped-Model", mapped_model.as_str())];
                return Ok((
                    StatusCode::SERVICE_UNAVAILABLE,
                    headers,
                    format!("Token error: {}", e),
                )
                    .into_response());
            }
        };

        // [NEW v4.1.29] 获取完整 Token 对象用于动态规格查询
        let proxy_token = token_manager.get_token_by_id(&account_id);
        let mapped_model = token_manager
            .resolve_dynamic_model_for_account(&account_id, &mapped_model)
            .await;

        last_email = Some(email.clone());
        info!("✓ Using account: {} (type: {})", email, config.request_type);

        // 4. 转换请求 (返回内容包含 session_id 和 message_count)
        let (gemini_body, session_id, message_count) = transform_openai_request(
            &openai_req,
            &project_id,
            &mapped_model,
            proxy_token.as_ref(),
        );

        if debug_logger::is_enabled(&debug_cfg) {
            let payload = json!({
                "kind": "v1internal_request",
                "protocol": "openai",
                "trace_id": trace_id,
                "original_model": openai_req.model,
                "mapped_model": mapped_model,
                "request_type": config.request_type,
                "attempt": attempt,
                "v1internal_request": gemini_body.clone(),
            });
            debug_logger::write_debug_payload(
                &debug_cfg,
                Some(&trace_id),
                "v1internal_request",
                &payload,
            )
            .await;
        }

        // [New] 打印转换后的报文 (Gemini Body) 供调试
        if let Ok(body_json) = serde_json::to_string_pretty(&gemini_body) {
            debug!("[OpenAI-Request] Transformed Gemini Body:\n{}", body_json);
        }

        // 5. 发送请求
        let client_wants_stream = openai_req.stream;
        let force_stream_internally = !client_wants_stream;
        let actual_stream = client_wants_stream || force_stream_internally;

        if force_stream_internally {
            debug!(
                "[{}] 🔄 Auto-converting non-stream request to stream for better quota",
                trace_id
            );
        }

        let method = if actual_stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        let query_string = if actual_stream { Some("alt=sse") } else { None };

        // [FIX #1522] Inject Anthropic Beta Headers for Claude models (OpenAI path)
        let mut extra_headers = std::collections::HashMap::new();
        if mapped_model.to_lowercase().contains("claude") {
            extra_headers.insert(
                "anthropic-beta".to_string(),
                "claude-code-20250219".to_string(),
            );
            tracing::debug!(
                "[{}] Injected Anthropic beta headers for Claude model (via OpenAI)",
                trace_id
            );
        }

        let call_result = match upstream
            .call_v1_internal_with_headers(
                method,
                &access_token,
                gemini_body,
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
                    "OpenAI Request failed on attempt {}/{}: {}",
                    attempt + 1,
                    max_attempts,
                    e
                );
                continue;
            }
        };

        // [NEW] 记录端点降级日志到 debug 文件
        if !call_result.fallback_attempts.is_empty() && debug_logger::is_enabled(&debug_cfg) {
            let fallback_entries: Vec<Value> = call_result
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
                "protocol": "openai",
                "trace_id": trace_id,
                "original_model": openai_req.model,
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
            // 5. 处理流式 vs 非流式
            if actual_stream {
                use axum::body::Body;
                use axum::response::Response;
                use futures::StreamExt;

                let meta = json!({
                    "protocol": "openai",
                    "trace_id": trace_id,
                    "original_model": openai_req.model,
                    "mapped_model": mapped_model,
                    "request_type": config.request_type,
                    "attempt": attempt,
                    "status": status.as_u16(),
                    "upstream_url": upstream_url,
                });
                let gemini_stream = debug_logger::wrap_stream_with_debug(
                    Box::pin(response.bytes_stream()),
                    debug_cfg.clone(),
                    trace_id.clone(),
                    "upstream_response",
                    meta,
                );

                // [P1 FIX] Enhanced Peek logic to handle heartbeats and slow start
                // Pre-read until we find meaningful content, skip heartbeats
                use crate::proxy::mappers::openai::streaming::create_openai_sse_stream;
                let mut openai_stream = create_openai_sse_stream(
                    gemini_stream,
                    openai_req.model.clone(),
                    session_id,
                    message_count,
                );

                let mut first_data_chunk = None;
                let mut retry_this_account = false;

                // Loop to skip heartbeats during peek
                loop {
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(300),
                        openai_stream.next(),
                    )
                    .await
                    {
                        Ok(Some(Ok(bytes))) => {
                            if bytes.is_empty() {
                                continue;
                            }

                            let text = String::from_utf8_lossy(&bytes);
                            // Skip SSE comments/pings (heartbeats)
                            if text.trim().starts_with(":") || text.trim().starts_with("data: :") {
                                tracing::debug!("[OpenAI] Skipping peek heartbeat");
                                continue;
                            }

                            // Check for error events
                            if text.contains("\"error\"") {
                                tracing::warn!("[OpenAI] Error detected during peek, retrying...");
                                last_error = "Error event during peek".to_string();
                                retry_this_account = true;
                                break;
                            }

                            // We found real data!
                            first_data_chunk = Some(bytes);
                            break;
                        }
                        Ok(Some(Err(e))) => {
                            tracing::warn!("[OpenAI] Stream error during peek: {}, retrying...", e);
                            last_error = format!("Stream error during peek: {}", e);
                            retry_this_account = true;
                            break;
                        }
                        Ok(None) => {
                            tracing::warn!(
                                "[OpenAI] Stream ended during peek (Empty Response), retrying..."
                            );
                            last_error = "Empty response stream during peek".to_string();
                            retry_this_account = true;
                            break;
                        }
                        Err(_) => {
                            tracing::warn!("[OpenAI] First chunk timeout after 300s, retrying...");
                            last_error = "First chunk timeout".to_string();
                            retry_this_account = true;
                            break;
                        }
                    }
                }

                if retry_this_account {
                    continue; // Rotate to next account
                }

                // Combine first chunk with remaining stream
                let combined_stream =
                    futures::stream::once(
                        async move { Ok::<Bytes, String>(first_data_chunk.unwrap()) },
                    )
                    .chain(openai_stream);

                // [NEW] 针对 OpenAI 流增加 300 秒空闲超时保护
                let combined_stream = async_stream::stream! {
                    let mut s = Box::pin(combined_stream);

                    loop {
                        match tokio::time::timeout(std::time::Duration::from_secs(300), s.next()).await {
                            Ok(Some(item)) => yield item,
                            Ok(None) => break,
                            Err(_) => {
                                tracing::error!("[OpenAI-SSE] Idle timeout after 300s, terminating stream");
                                yield Ok::<Bytes, String>(Bytes::from("data: [DONE]\n\n"));
                                break;
                            }
                        }
                    }
                };

                if client_wants_stream {
                // [MULTI-TURN] 保存本次对话的 messages 到 session store（/v1/chat/completions）
                {
                    let save_msgs = openai_req.messages.iter().map(|m| {
                        let content_str = match &m.content {
                            Some(crate::proxy::mappers::openai::OpenAIContent::String(s)) => s.clone(),
                            _ => String::new(),
                        };
                        json!({"role": m.role, "content": content_str})
                    }).collect::<Vec<_>>();
                    let chat_response_id = format!("chatcmpl-{}", uuid::Uuid::new_v4().simple());
                    let entry = crate::proxy::http_session_store::HttpSessionEntry {
                        input_items: save_msgs,
                        instructions: String::new(),
                        model: openai_req.model.clone(),
                        last_accessed: std::time::Instant::now(),
                    };
                    let rid = chat_response_id.clone();
                    tokio::spawn(async move {
                        crate::proxy::http_session_store::save_session(rid, entry).await;
                    });
                }
                    // 客户端请求流式，返回 SSE
                    let body = Body::from_stream(combined_stream);
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
                    // 客户端请求非流式，但内部强制转为流式
                    // 收集流数据并聚合为 JSON
                    use crate::proxy::mappers::openai::collector::collect_stream_to_json;

                    match collect_stream_to_json(Box::pin(combined_stream)).await {
                        Ok(full_response) => {
                            info!("[{}] ✓ Stream collected and converted to JSON", trace_id);
                            return Ok((
                                StatusCode::OK,
                                [
                                    ("X-Account-Email", email.as_str()),
                                    ("X-Mapped-Model", mapped_model.as_str()),
                                ],
                                Json(full_response),
                            )
                                .into_response());
                        }
                        Err(e) => {
                            error!("[{}] Stream collection error: {}", trace_id, e);
                            return Ok((
                                StatusCode::INTERNAL_SERVER_ERROR,
                                format!("Stream collection error: {}", e),
                            )
                                .into_response());
                        }
                    }
                }
            }

            let gemini_resp: Value = response
                .json()
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Parse error: {}", e)))?;

            let openai_response =
                transform_openai_response(&gemini_resp, Some(&session_id), message_count);
            return Ok((
                StatusCode::OK,
                [
                    ("X-Account-Email", email.as_str()),
                    ("X-Mapped-Model", mapped_model.as_str()),
                ],
                Json(openai_response),
            )
                .into_response());
        }

        // 处理特定错误并重试
        let status_code = status.as_u16();
        let _retry_after = response
            .headers()
            .get("Retry-After")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| format!("HTTP {}", status_code));
        last_error = format!("HTTP {}: {}", status_code, error_text);

        // [New] 打印错误报文日志
        tracing::error!(
            "[OpenAI-Upstream] Error Response {}: {}",
            status_code,
            error_text
        );
        if debug_logger::is_enabled(&debug_cfg) {
            let payload = json!({
                "kind": "upstream_response_error",
                "protocol": "openai",
                "trace_id": trace_id,
                "original_model": openai_req.model,
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

        // 3. 标记限流状态(用于 UI 显示)
        if status_code == 429 || status_code == 529 || status_code == 503 || status_code == 500 {
            // [FIX] Use async version with model parameter for fine-grained rate limiting
            token_manager
                .mark_rate_limited_async(
                    &email,
                    status_code,
                    _retry_after.as_deref(),
                    &error_text,
                    Some(&mapped_model),
                )
                .await;
        }

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
            // [NEW] Apply Client Adapter "let_it_crash" strategy
            if let Some(adapter) = &client_adapter {
                if adapter.let_it_crash() && attempt > 0 {
                    // For let_it_crash clients (like opencode), allow maybe 1 retry but then fail fast
                    // to prevent long hangs on UI.
                    tracing::warn!(
                        "[OpenAI] let_it_crash active: Aborting retries after attempt {}",
                        attempt
                    );
                    // Breaking loop to return error immediately
                    // Reuse existing error return logic via loop exit behavior?
                    // Or construct error here?
                    // Let's just break for now, which will trigger the "All accounts exhausted" or last error logic.
                    break;
                }
            }

            // 判断是否需要轮换账号
            if !should_rotate_account(status_code, Some(&strategy)) {
                debug!(
                    "[{}] Keeping same account for status {} (Grace Retry or Server Issue)",
                    trace_id, status_code
                );
                force_rotate = false;
            } else {
                force_rotate = true;
            }

            // 2. [REMOVED] 不再特殊处理 QUOTA_EXHAUSTED，允许账号轮换
            // if error_text.contains("QUOTA_EXHAUSTED") { ... }
            /*
            if error_text.contains("QUOTA_EXHAUSTED") {
                error!(
                    "OpenAI Quota exhausted (429) on account {} attempt {}/{}, stopping to protect pool.",
                    email,
                    attempt + 1,
                    max_attempts
                );
                return Ok((status, [("X-Account-Email", email.as_str()), ("X-Mapped-Model", mapped_model.as_str())], error_text).into_response());
            }
            */

            // 3. 其他限流或服务器过载情况，轮换账号
            tracing::warn!(
                "OpenAI Upstream {} on {} attempt {}/{}, rotating account",
                status_code,
                email,
                attempt + 1,
                max_attempts
            );
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
                "[OpenAI] Signature error detected on account {}, retrying without thinking",
                email
            );

            // 追加修复提示词到最后一条用户消息
            if let Some(last_msg) = openai_req.messages.last_mut() {
                if last_msg.role == "user" {
                    let repair_prompt = "\n\n[System Recovery] Your previous output contained an invalid signature. Please regenerate the response without the corrupted signature block.";

                    if let Some(content) = &mut last_msg.content {
                        use crate::proxy::mappers::openai::{OpenAIContent, OpenAIContentBlock};
                        match content {
                            OpenAIContent::String(s) => {
                                s.push_str(repair_prompt);
                            }
                            OpenAIContent::Array(arr) => {
                                arr.push(OpenAIContentBlock::Text {
                                    text: repair_prompt.to_string(),
                                });
                            }
                        }
                        tracing::debug!("[OpenAI] Appended repair prompt to last user message");
                    }
                }
            }

            continue; // 重试
        }

        // 只有 403 (权限/地区限制) 和 401 (认证失效) 触发账号轮换
        if status_code == 403 || status_code == 401 {
            if apply_retry_strategy(
                RetryStrategy::FixedDelay(Duration::from_millis(200)),
                attempt,
                max_attempts,
                status_code,
                &trace_id,
            )
            .await
            {
                continue;
            }
        }

        // 只有 403 (权限/地区限制) 和 401 (认证失效) 触发账号轮换
        if status_code == 403 || status_code == 401 {
            // [NEW] 403 时设置 is_forbidden 状态，避免 Claude Code 会话退出
            if status_code == 403 {
                if let Some(acc_id) = token_manager.get_account_id_by_email(&email) {
                    // Check for VALIDATION_REQUIRED error - temporarily block account
                    if error_text.contains("VALIDATION_REQUIRED")
                        || error_text.contains("verify your account")
                        || error_text.contains("validation_url")
                    {
                        tracing::warn!(
                            "[OpenAI] VALIDATION_REQUIRED detected on account {}, temporarily blocking",
                            email
                        );
                        // Block for 10 minutes (default, configurable via config file)
                        let block_minutes = 10i64;
                        let block_until = chrono::Utc::now().timestamp() + (block_minutes * 60);

                        if let Err(e) = token_manager
                            .set_validation_block_public(&acc_id, block_until, &error_text)
                            .await
                        {
                            tracing::error!("Failed to set validation block: {}", e);
                        }
                    }

                    // 设置 is_forbidden 状态
                    if let Err(e) = token_manager.set_forbidden(&acc_id, &error_text).await {
                        tracing::error!("Failed to set forbidden status: {}", e);
                    }
                }
            }

            if apply_retry_strategy(
                RetryStrategy::FixedDelay(Duration::from_millis(200)),
                attempt,
                max_attempts,
                status_code,
                &trace_id,
            )
            .await
            {
                continue;
            }
        }

        // 404 等由于模型配置或路径错误的 HTTP 异常，直接报错，不进行无效轮换
        error!(
            "OpenAI Upstream non-retryable error {} on account {}: {}",
            status_code, email, error_text
        );
        return Ok((
            status,
            [
                ("X-Account-Email", email.as_str()),
                ("X-Mapped-Model", mapped_model.as_str()),
            ],
            // [FIX] Return JSON error for better client compatibility
            Json(json!({
                "error": {
                    "message": error_text,
                    "type": "upstream_error",
                    "code": status_code
                }
            })),
        )
            .into_response());
    }

    // 所有尝试均失败
    if let Some(email) = last_email {
        Ok((
            StatusCode::TOO_MANY_REQUESTS,
            [("X-Account-Email", email), ("X-Mapped-Model", mapped_model)],
            format!("All accounts exhausted. Last error: {}", last_error),
        )
            .into_response())
    } else {
        Ok((
            StatusCode::TOO_MANY_REQUESTS,
            [("X-Mapped-Model", mapped_model)],
            format!("All accounts exhausted. Last error: {}", last_error),
        )
            .into_response())
    }
}

// --- Codex GUIDANCE PROMPTS ---

const APPLY_PATCH_CHAT_PATH_SYSTEM_GUIDANCE_ZH: &str = concat!(
    "[apply_patch chat-path 指引 — 由 codex-app-transfer adapter 注入,因为上游 lark 语法约束在 chat function-call provider 上不可用]\n",
    "\n",
    "**务必使用 `apply_patch` tool 写文件内容** —— 新建文件、单行编辑、整文件重写都一样。**绝不使用 shell `cat <<EOF > file` / `printf '<content>' > file` / `echo '<content>' > file` / 任何 `>` 重定向来写实际文件内容** —— 这样做会绕过 Codex diff UI 和审计 trail。**同样,绝不使用 `sed -i` / `perl -i` / `ed`、或 shell 按行号删除(如 `sed -i 'N,Md' file`)来编辑或删除已有文件内容** —— 就地 shell 编辑器绕过 diff UI,且对多次编辑间的行号漂移很脆弱(按过期行号删会切错、损坏文件)。(新建或空文件用 `*** Add File: <path>` —— 不要用 shell 重定向。)**优先外科式针对性编辑**:要改/替换已有内容时,只发改动那几行的 `-`(旧)和 `+`(新),保持每个 hunk 最小;且**不要**把增删空行作为编辑的一部分,除非空行本身就是改动(空行 `+`/`-` 位置歧义、可能静默 apply 失败)。**删除内容 —— 即便是跨很多行的大段连续块 —— 也用 apply_patch hunk 里的 `-` 行表达,或用 `*** Delete File: <path>` 删整个文件;不要因为块大就改用 `sed`/`python` 按行范围删除。** 对同一文件的多处不相邻编辑可以放进**一次** apply_patch 调用、分成多个 hunk。**不要**整段重新生成再追加,**不要**因为改了一部分就整文件重写。整文件替换(同一 patch 内 `*** Delete File: <path>` + `*** Add File: <path>`、每行前缀 `+`)**仅限**真正需要时:新建全新内容,或几乎每行都不同。\n",
    "\n",
    "调用 `apply_patch` tool 时,遵循以下基于非 OpenAI chat provider 实战观察总结的规则:\n",
    "\n",
    "1. 推荐的 Update File 形式是**最简形态**:仅 `-line`(要删的行,byte-exact)和 `+line`(新行)直接跟在 `*** Update File: <path>` 之后 —— 无 `@@`、无 context 行。",
    "凡是 `-` 行在文件里**唯一**时(简单单行编辑、配置改动、function 签名等绝大多数场景皆是)就用这个形态。例:\n",
    "  *** Update File: src/config.py\n",
    "  -DEBUG = False\n",
    "  +DEBUG = True\n",
    "若 `-` 行单独**有歧义**(同一行文本在文件多处出现),在上方/下方加空格前缀的 context 行(` line`)钉住它。",
    "若 context 行也不足以消歧,再在独立行上加**单端** `@@ <header>` 标记(`@@ class Foo`、`@@ def bar():`、`@@ fn main() {`)。",
    "**绝不加尾随 `@@`**(`@@ <header> @@` 是错的)—— Codex Desktop 的 V4A applier 会把尾随 `@@` 当字面文本,报 `Failed to find context '... @@'`。",
    "深层嵌套消歧时用**多个** `@@` 行各占一行(例如 `@@ class Outer\\n@@ def inner():`),每条都是单端。\n",
    "\n",
    "2. Add File **不用** `@@` 标记、**不用** hunk。`*** Add File: <path>` 之后,新文件**每一行内容**(包括空行,写成单个 `+` 占一行)都前缀 `+`。没 `+` 前缀的原始源码(例如直接写 `def main():`)会触发 `'def main():' is not a valid hunk header` 错误。",
    "但结构标记 `*** Begin Patch` / `*** Add File:` / `*** End Patch` **不是内容,不加前缀**。尤其**绝不给终止符加前缀**(`+*** End Patch` 是错的):带 `+` 的终止符会被当成内容行,在新建文件末尾留下一行字面 `*** End Patch`。\n",
    "\n",
    "3. 每个 `-` 行和空格前缀的 context 行**必须**跟文件 byte-for-byte 一致(同样的前导 whitespace,不能 trim 尾随空格,字符完全相同)。不确定时先用 shell 跑 `cat <path>` 或 `sed -n '1,80p' <path>` 查一下,再用真实字节组 patch。靠猜会触发 `Failed to find context '<your guess>'` 错误。\n",
    "\n",
    "3a. 行前缀是**单字符**,前缀和内容之间**没有空格**:写 `-DEBUG = False`(不是 `- DEBUG = False`)、`+DEBUG = True`(不是 `+ DEBUG = True`),context 行 ` keepme`(单个前导空格)。Codex Desktop V4A applier 可能容忍多余空格,但其它 apply_patch 实现严格 —— 前缀写紧凑。\n",
    "\n",
    "4. **不要**在同一 patch 内对同一路径同时用 `*** Add File: <path>` 和 `*** Update File: <path>`。Update 步骤会在 Add 步骤落盘前读文件,看到空文件后失败。要么 (a) 让 `*** Add File:` 一次性写最终内容,要么 (b) 拆成两个独立的 `apply_patch` 调用。\n",
    "\n",
    "5. 新建或空文件用 `*** Add File: <path>`、每行前缀 `+`(不要用 `*** Update File:`,也不要用 shell 重定向)。\n",
    "\n",
    "6. 多行文件里,**没有**对应 `-` 行的孤立 `+` 行会**追加**在上文 context 之下 —— **不会**替换任何已有行。要修改已有行,**必须**同时包含 `-` 行(删旧内容)和 `+` 行(加新内容)。",
    "空格前缀的 context 行是拿来**跟文件匹配**的、绝不新增 —— 它必须已存在于文件中。要引入全新行,前缀 `+`;把文件里还没有的行写成 context(或不加前缀)会得到一个无实际改动、apply 失败或 `Failed to find context` 的 hunk。\n",
    "\n",
    "7. Update 报 `Failed to find context` 时,说明 `-`/context 行跟文件 byte 对不上 —— 重新 `cat <path>` / `sed -n` 读文件、把这些行改成完全一致,再重试**同一个**针对性 Update。**不要**升级成整文件重写/重新追加,把编辑保持在改动的那几行。",
    "在**一次**回合里对**同一文件**做多处编辑时,每个已应用的 hunk 都会改变文件内容 —— 把相关编辑放进**一个** patch 的多个 hunk,或在多次独立调用之间重新读文件。某个 `-` 行不再匹配,可能是它**已经被删掉**(被前一个 hunk、或本回合更早的编辑)—— 重发同一删除前先确认它还在,别盲目重试。\n",
    "\n",
    "8. `*** Begin Patch` **必须**是 `input` 字符串的字面第一行 —— 不能有前导空格,前面不能有其它内容,绝不能直接写 `*** Add File:` 或任何操作 header。漏了会触发 `invalid patch: The first line of the patch must be '*** Begin Patch'`。\n",
    "\n",
    "9. `*** Update File: <old>` + `*** Move to: <new>` **要求**至少一个 hunk(带 `-`/`+` 行或 `*** End of File` 标记)。空的 Update+Move 块会报 `Update file hunk for path '<old>' is empty`。**纯重命名不改内容**时,在同一 patch 内用 `*** Delete File: <old>` + `*** Add File: <new>`(把原内容每行前缀 `+` 复制过去)。**重命名同时改内容**时,保留 Update+Move 并写真实的 `-`/`+` hunk。\n",
    "\n",
    "10. 编辑 memory 文件(如 `~/.codex/memories/MEMORY.md`)要格外小心:并发进程可能在你上次读它、到你的 patch 落地之间重写该文件。打 patch **前立即** `cat` 该文件,让每个 `-`/context 行都是**当前**文件里存在的行,并用最小唯一锚点(如单个 `@@ <section header>` + 只写你实际改的那几行)。过期的 `-` 行 —— 内容已被并发固化(consolidation)改掉 —— 会报 `Failed to find context`;失败时重新读、按当前字节重建,而不是重试过期 patch。\n",
    "\n",
    "遵循这些规则可以避免 retry 风暴,提升首次尝试的成功率。"
);

const WEB_TOOLS_SYSTEM_GUIDANCE_ZH: &str = "联网获取信息时(实时事实 / 价格 / 文档 / 新闻 / 版本号 / 任何你不确定或可能已过时的内容),**优先用 `web_search` 和 `web_fetch` 工具,不要用 shell 的 curl / wget / python 去抓 URL 或搜索引擎**。本机对外网访问受限,shell 直连通常被防火墙 / 反爬拦截(返回空或 403),会白费多轮尝试、最后只能靠可能过时的记忆作答;而这两个工具经代理(浏览器 TLS 指纹 + headless 渲染)能真正抓到。用法:先 `web_search(query)` 找信息源,再用 `web_fetch(url)` 读该页**完整正文**(返回全文、自己读)。之前抓过的某 URL 若在对话历史里被折叠 / 压缩、需要回看完整原文, 用 `read_url_local(url)` 从本地缓存取回, 不必重新联网。";

const CHINESE_LANGUAGE_DIRECTIVE: &str = "**请始终使用简体中文回复用户**(代码、命令、标识符、文件路径等技术内容保持原文,不要翻译)。";

fn tools_register_apply_patch(body: &Value) -> bool {
    let Some(tools) = body.get("tools").and_then(Value::as_array) else {
        return false;
    };
    tools.iter().any(|t| {
        t.get("name").and_then(Value::as_str) == Some("apply_patch")
            && (t.get("type").and_then(Value::as_str) == Some("custom") || t.get("type").and_then(Value::as_str) == Some("function"))
    })
}

fn tools_register_web_fetch(body: &Value) -> bool {
    fn entry_is_web_tool(t: &Value) -> bool {
        matches!(
            t.get("name").and_then(Value::as_str),
            Some("web_fetch") | Some("web_search")
        )
    }
    body.get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools.iter().any(|t| {
                if t.get("type").and_then(Value::as_str) == Some("namespace") {
                    t.get("tools")
                        .and_then(Value::as_array)
                        .is_some_and(|inner| inner.iter().any(entry_is_web_tool))
                } else {
                    entry_is_web_tool(t)
                }
            })
        })
        .unwrap_or(false)
}

fn apply_patch_chat_guidance_message() -> Value {
    let content = format!("{CHINESE_LANGUAGE_DIRECTIVE}\n\n{APPLY_PATCH_CHAT_PATH_SYSTEM_GUIDANCE_ZH}");
    serde_json::json!({
        "role": "system",
        "content": content,
    })
}

fn web_tools_guidance_message() -> Value {
    serde_json::json!({
        "role": "system",
        "content": WEB_TOOLS_SYSTEM_GUIDANCE_ZH,
    })
}

// --- END Codex GUIDANCE PROMPTS ---

/// 处理 Legacy Completions API (/v1/completions)
/// 将 Prompt 转换为 Chat Message 格式，复用 handle_chat_completions
pub async fn handle_completions(
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    State(state): State<AppState>,
    Json(mut body): Json<Value>,
) -> Response {
    debug!(
        "Received /v1/completions or /v1/responses payload: {:?}",
        body
    );

    // [MULTI-TURN] 支持 previous_response_id 链式历史恢复
    // 当客户端通过 HTTP POST /v1/responses 传入 previous_response_id 时，
    // 从服务器端 session store 取出上一轮的历史，合并到本轮的 input 中
    let previous_response_id = body.get("previous_response_id").and_then(|v| v.as_str()).map(|s| s.to_string());
    let response_id_for_save = format!("resp-{}", uuid::Uuid::new_v4());
    let http_tool_call_cache: std::collections::HashMap<String, serde_json::Value> = std::collections::HashMap::new();
    if let Some(ref prev_id) = previous_response_id {
        if let Some(session) = crate::proxy::http_session_store::get_session(prev_id).await {
            // 把历史 input items 合并进来
            let existing_input = body.get("input").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            let merged = crate::proxy::http_session_store::merge_history_with_new_input(
                session.input_items,
                &[],
                &existing_input,
                &http_tool_call_cache,
            );
                let merged_len = merged.len();
            if let Some(obj) = body.as_object_mut() {
                obj.insert("input".to_string(), json!(merged));
                // 从历史 session 继承 instructions（如果本轮没带）
                if !obj.contains_key("instructions") && !session.instructions.is_empty() {
                    obj.insert("instructions".to_string(), json!(session.instructions));
                }
                // 继承 model（如果本轮没带）
                if !obj.contains_key("model") && !session.model.is_empty() {
                    obj.insert("model".to_string(), json!(session.model));
                }
            }
            tracing::debug!("[MultiTurn] Restored session from prev_id={}, {} items in history", prev_id, merged_len);
        }
    }

    let is_codex_style = body.get("input").is_some() || body.get("instructions").is_some();

    // 1. Convert Payload to Messages (Shared Chat Format)
    if is_codex_style {
        let instructions = body
            .get("instructions")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let input_items = body.get("input").and_then(|v| v.as_array());

        let mut messages = Vec::new();

        // System Instructions
        if !instructions.is_empty() {
            messages.push(json!({ "role": "system", "content": instructions }));
        }

        let mut call_id_to_name = std::collections::HashMap::new();

        // Pass 1: Build Call ID to Name Map
        if let Some(items) = input_items {
            for item in items {
                let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match item_type {
                    "function_call" | "local_shell_call" | "web_search_call" => {
                        let call_id = item
                            .get("call_id")
                            .and_then(|v| v.as_str())
                            .or_else(|| item.get("id").and_then(|v| v.as_str()))
                            .unwrap_or("unknown");

                        let name = if item_type == "local_shell_call" {
                            "shell"
                        } else if item_type == "web_search_call" {
                            "google_search"
                        } else {
                            item.get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                        };

                        call_id_to_name.insert(call_id.to_string(), name.to_string());
                        tracing::debug!("Mapped call_id {} to name {}", call_id, name);
                    }
                    _ => {}
                }
            }
        }

        // Pass 2: Map Input Items to Messages
        if let Some(items) = input_items {
            for item in items {
                let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match item_type {
                    "message" => {
                        let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                        let content = item.get("content").and_then(|v| v.as_array());
                        let mut text_parts = Vec::new();
                        let mut image_parts: Vec<Value> = Vec::new();

                        if let Some(parts) = content {
                            for part in parts {
                                // 处理文本块
                                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                    text_parts.push(text.to_string());
                                }
                                // [NEW] 处理图像块 (Codex input_image 格式)
                                else if part.get("type").and_then(|v| v.as_str())
                                    == Some("input_image")
                                {
                                    if let Some(image_url) =
                                        part.get("image_url").and_then(|v| v.as_str())
                                    {
                                        image_parts.push(json!({
                                            "type": "image_url",
                                            "image_url": { "url": image_url }
                                        }));
                                        debug!("[Codex] Found input_image: {}", image_url);
                                    }
                                }
                                // [NEW] 兼容标准 OpenAI image_url 格式
                                else if part.get("type").and_then(|v| v.as_str())
                                    == Some("image_url")
                                {
                                    if let Some(url_obj) = part.get("image_url") {
                                        image_parts.push(json!({
                                            "type": "image_url",
                                            "image_url": url_obj.clone()
                                        }));
                                    }
                                }
                            }
                        }

                        // 构造消息内容：如果有图像则使用数组格式
                        if image_parts.is_empty() {
                            messages.push(json!({
                                "role": role,
                                "content": text_parts.join("\n")
                            }));
                        } else {
                            let mut content_blocks: Vec<Value> = Vec::new();
                            if !text_parts.is_empty() {
                                content_blocks.push(json!({
                                    "type": "text",
                                    "text": text_parts.join("\n")
                                }));
                            }
                            content_blocks.extend(image_parts);
                            messages.push(json!({
                                "role": role,
                                "content": content_blocks
                            }));
                        }
                    }
                    "function_call" | "local_shell_call" | "web_search_call" => {
                        let mut name = item
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let mut args_str = item
                            .get("arguments")
                            .and_then(|v| v.as_str())
                            .unwrap_or("{}")
                            .to_string();
                        let call_id = item
                            .get("call_id")
                            .and_then(|v| v.as_str())
                            .or_else(|| item.get("id").and_then(|v| v.as_str()))
                            .unwrap_or("unknown");

                        // Handle native shell calls
                        if item_type == "local_shell_call" {
                            name = "shell";
                            if let Some(action) = item.get("action") {
                                if let Some(exec) = action.get("exec") {
                                    // Map to ShellCommandToolCallParams (string command) or ShellToolCallParams (array command)
                                    // Most LLMs prefer a single string for shell
                                    let mut args_obj = serde_json::Map::new();
                                    if let Some(cmd) = exec.get("command") {
                                        // CRITICAL FIX: The 'shell' tool schema defines 'command' as an ARRAY of strings.
                                        // We MUST pass it as an array, not a joined string, otherwise Gemini rejects with 400 INVALID_ARGUMENT.
                                        let cmd_val = if cmd.is_string() {
                                            json!([cmd]) // Wrap in array
                                        } else {
                                            cmd.clone() // Assume already array
                                        };
                                        args_obj.insert("command".to_string(), cmd_val);
                                    }
                                    if let Some(wd) =
                                        exec.get("working_directory").or(exec.get("workdir"))
                                    {
                                        args_obj.insert("workdir".to_string(), wd.clone());
                                    }
                                    args_str = serde_json::to_string(&args_obj)
                                        .unwrap_or("{}".to_string());
                                }
                            }
                        } else if item_type == "web_search_call" {
                            name = "google_search";
                            if let Some(action) = item.get("action") {
                                let mut args_obj = serde_json::Map::new();
                                if let Some(q) = action.get("query") {
                                    args_obj.insert("query".to_string(), q.clone());
                                }
                                args_str =
                                    serde_json::to_string(&args_obj).unwrap_or("{}".to_string());
                            }
                        }

                        messages.push(json!({
                            "role": "assistant",
                            "tool_calls": [
                                {
                                    "id": call_id,
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": args_str
                                    }
                                }
                            ]
                        }));
                    }
                    "function_call_output" | "custom_tool_call_output" => {
                        let call_id = item
                            .get("call_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let output = item.get("output");
                        let output_str = if let Some(o) = output {
                            if o.is_string() {
                                o.as_str().unwrap().to_string()
                            } else if let Some(content) = o.get("content").and_then(|v| v.as_str())
                            {
                                content.to_string()
                            } else {
                                o.to_string()
                            }
                        } else {
                            "".to_string()
                        };

                        let name = call_id_to_name.get(call_id).cloned().unwrap_or_else(|| {
                            // Fallback: if unknown and we see function_call_output, it's likely "shell" in this context
                            tracing::warn!(
                                "Unknown tool name for call_id {}, defaulting to 'shell'",
                                call_id
                            );
                            "shell".to_string()
                        });

                        messages.push(json!({
                            "role": "tool",
                            "tool_call_id": call_id,
                            "name": name,
                            "content": output_str
                        }));
                    }
                    _ => {}
                }
            }
        }

        if let Some(obj) = body.as_object_mut() {
            obj.insert("messages".to_string(), json!(messages));
        }
    } else if let Some(prompt_val) = body.get("prompt") {
        // Legacy OpenAI Style: prompt -> Chat
        let prompt_str = match prompt_val {
            Value::String(s) => s.clone(),
            Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            _ => prompt_val.to_string(),
        };
        let messages = json!([ { "role": "user", "content": prompt_str } ]);
        if let Some(obj) = body.as_object_mut() {
            obj.remove("prompt");
            obj.insert("messages".to_string(), messages);
        }
    }

    // 2. Reuse handle_chat_completions logic (wrapping with custom handler or direct call)
    // Actually, due to SSE handling differences (Codex uses different event format), we replicate the loop here or abstract it.
    // For now, let's replicate the core loop but with Codex specific SSE mapping.

    // [Fix Phase 2] Backport normalization logic from handle_chat_completions
    // Handle "instructions" + "input" (Codex style) -> system + user messages
    // This is critical because `transform_openai_request` expects `messages` to be populated.

    // [FIX] 检查是否已经有 messages (被第一次标准化处理过)
    let has_codex_fields = body.get("instructions").is_some() || body.get("input").is_some();
    let already_normalized = body
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|arr| !arr.is_empty())
        .unwrap_or(false);

    // 只有在未标准化时才进行简单转换
    if has_codex_fields && !already_normalized {
        tracing::debug!("[Codex] Performing simple normalization (messages not yet populated)");

        let mut messages = Vec::new();

        // instructions -> system message
        if let Some(inst) = body.get("instructions").and_then(|v| v.as_str()) {
            if !inst.is_empty() {
                messages.push(json!({
                    "role": "system",
                    "content": inst
                }));
            }
        }

        // input -> user message (支持对象数组形式的对话历史)
        if let Some(input) = body.get("input") {
            if let Some(s) = input.as_str() {
                messages.push(json!({
                    "role": "user",
                    "content": s
                }));
            } else if let Some(arr) = input.as_array() {
                // 判断是消息对象数组还是简单的内容块/字符串数组
                let is_message_array = arr
                    .first()
                    .and_then(|v| v.as_object())
                    .map(|obj| obj.contains_key("role") || obj.contains_key("type"))
                    .unwrap_or(false);

                if is_message_array {
                    // 深度识别：像处理 messages 一样处理 input 数组，并自动映射 Responses API 的工具流
                    for item in arr {
                        if let Some(obj) = item.as_object() {
                            if let Some(item_type) = obj.get("type").and_then(|v| v.as_str()) {
                                match item_type {
                                    "message" => {
                                        let role = obj.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                                        let content = obj.get("content").cloned().unwrap_or(json!(""));
                                        messages.push(json!({ "role": role, "content": content }));
                                    }
                                    "function_call" | "custom_tool_call" => {
                                        let call_id = obj.get("call_id").or_else(|| obj.get("id")).and_then(|v| v.as_str()).unwrap_or("");
                                        let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                        let arguments = obj.get("arguments").and_then(|v| v.as_str()).unwrap_or("");
                                        messages.push(json!({
                                            "role": "assistant",
                                            "content": "",
                                            "tool_calls": [{
                                                "id": if call_id.is_empty() { "call_unknown" } else { call_id },
                                                "type": "function",
                                                "function": { "name": name, "arguments": arguments },
                                            }],
                                        }));
                                    }
                                    "function_call_output" | "custom_tool_call_output" => {
                                        let call_id = obj.get("call_id").or_else(|| obj.get("tool_call_id")).or_else(|| obj.get("id")).and_then(|v| v.as_str()).unwrap_or("");
                                        let output_value = obj.get("output").cloned().unwrap_or(json!(""));
                                        let output_str = if let Some(s) = output_value.as_str() {
                                            s.to_string()
                                        } else {
                                            output_value.to_string()
                                        };
                                        messages.push(json!({
                                            "role": "tool",
                                            "tool_call_id": call_id,
                                            "content": output_str,
                                        }));
                                    }
                                    _ => {
                                        messages.push(item.clone());
                                    }
                                }
                                continue;
                            }
                        }
                        messages.push(item.clone());
                    }
                } else {
                    // 降级处理：传统的字符串或混合内容拼接
                    let content = arr
                        .iter()
                        .map(|v| {
                            if let Some(s) = v.as_str() {
                                s.to_string()
                            } else if v.is_object() {
                                v.to_string()
                            } else {
                                "".to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    if !content.is_empty() {
                        messages.push(json!({
                            "role": "user",
                            "content": content
                        }));
                    }
                }
            } else {
                let content = input.to_string();
                if !content.is_empty() {
                    messages.push(json!({
                        "role": "user",
                        "content": content
                    }));
                }
            };
        }

        if let Some(obj) = body.as_object_mut() {
            tracing::debug!(
                "[Codex] Injecting normalized messages: {} messages",
                messages.len()
            );
            obj.insert("messages".to_string(), json!(messages));
        }
    } else if already_normalized {
        tracing::debug!(
            "[Codex] Skipping normalization (messages already populated by first pass)"
        );
    }

    // [FIX] 在 openai_req 反序列化之前，从 body 中捕获原始 input 和 instructions
    // 用于后续 session 保存时，保留完整的工具调用历史（而非从 openai_req.messages 重建丢失信息）
    let session_save_input: Vec<serde_json::Value> = body
        .get("input")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let session_save_instructions: String = body
        .get("instructions")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut openai_req: OpenAIRequest = match serde_json::from_value(body.clone()) {
        Ok(req) => req,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("Invalid request: {}", e)).into_response();
        }
    };

    // Safety: Inject empty message if needed
    if openai_req.messages.is_empty() {
        openai_req
            .messages
            .push(crate::proxy::mappers::openai::OpenAIMessage {
                role: "user".to_string(),
                content: Some(crate::proxy::mappers::openai::OpenAIContent::String(
                    " ".to_string(),
                )),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
                refusal: None,
            });
    }

    let upstream = state.upstream.clone();
    let token_manager = state.token_manager;
    let pool_size = token_manager.len();
    // [FIX] Ensure max_attempts is at least 2 to allow for internal retries
    let max_attempts = MAX_RETRY_ATTEMPTS.min(pool_size.saturating_add(1)).max(2);

    let mut last_error = String::new();
    let mut last_email: Option<String> = None;

    // 2. 模型路由解析 (移到循环外以支持在所有路径返回 X-Mapped-Model)
    let mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
        &openai_req.model,
        &*state.custom_mapping.read().await,
    );
    let trace_id = format!("req_{}", chrono::Utc::now().timestamp_subsec_millis());

    let mut force_rotate = false;

    for attempt in 0..max_attempts {
        // 3. 模型配置解析
        // 将 OpenAI 工具转为 Value 数组以便探测联网
        let tools_val: Option<Vec<Value>> = openai_req
            .tools
            .as_ref()
            .map(|list| list.iter().cloned().collect());
        let config = crate::proxy::mappers::common_utils::resolve_request_config(
            &openai_req.model,
            &mapped_model,
            &tools_val,
            None, // size
            None, // quality
            None, // image_size
            None, // body
        );

        // 3. 提取 SessionId (复用)
        // [New] 使用 TokenManager 内部逻辑提取 session_id，支持粘性调度
        let session_id_str = SessionManager::extract_openai_session_id(&openai_req);
        let session_id = Some(session_id_str.as_str());

        let (access_token, project_id, email, account_id, _wait_ms) = match token_manager
            .get_token(
                &config.request_type,
                force_rotate,
                session_id,
                &mapped_model,
            )
            .await
        {
            Ok(t) => t,
            Err(e) => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    [("X-Mapped-Model", mapped_model)],
                    format!("Token error: {}", e),
                )
                    .into_response()
            }
        };

        let mapped_model = token_manager
            .resolve_dynamic_model_for_account(&account_id, &mapped_model)
            .await;

        last_email = Some(email.clone());

        info!("✓ Using account: {} (type: {})", email, config.request_type);

        let proxy_token = token_manager.get_token_by_id(&account_id);
        let (gemini_body, session_id, message_count) = transform_openai_request(
            &openai_req,
            &project_id,
            &mapped_model,
            proxy_token.as_ref(),
        );

        // [New] 打印转换后的报文 (Gemini Body) 供调试 (Codex 路径) ———— 缩减为 simple debug
        debug!(
            "[Codex-Request] Transformed Gemini Body ({} parts)",
            gemini_body
                .get("contents")
                .and_then(|c| c.as_array())
                .map(|a| a.len())
                .unwrap_or(0)
        );

        // [AUTO-CONVERSION] For Legacy/Codex as well
        let client_wants_stream = openai_req.stream;
        let force_stream_internally = !client_wants_stream;
        let list_response = client_wants_stream || force_stream_internally;
        let method = if list_response {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        let query_string = if list_response { Some("alt=sse") } else { None };

        let call_result = match upstream
            .call_v1_internal(
                method,
                &access_token,
                gemini_body,
                query_string,
                Some(account_id.as_str()),
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_error = e.clone();
                debug!(
                    "Codex Request failed on attempt {}/{}: {}",
                    attempt + 1,
                    max_attempts,
                    e
                );
                continue;
            }
        };

        let response = call_result.response;
        let status = response.status();
        if status.is_success() {
            // [智能限流] 请求成功，重置该账号的连续失败计数
            token_manager.mark_account_success(&email);

            if list_response {
                use axum::body::Body;
                use axum::response::Response;
                use futures::StreamExt;

                let gemini_stream = response.bytes_stream();

                // DECISION: Which stream to create?
                // If client wants stream: give them what they asked (Legacy/Codex SSE).
                // If forced stream: use Chat SSE + Collector, because our collector works on Chat format
                // and we already have logic to convert Chat JSON -> Legacy JSON.

                if client_wants_stream {
                    let mut openai_stream = if is_codex_style {
                        use crate::proxy::mappers::openai::streaming::create_codex_sse_stream;
                        create_codex_sse_stream(
                            Box::pin(gemini_stream),
                            openai_req.model.clone(),
                            session_id,
                            message_count,
                        )
                    } else {
                        use crate::proxy::mappers::openai::streaming::create_legacy_sse_stream;
                        create_legacy_sse_stream(
                            Box::pin(gemini_stream),
                            openai_req.model.clone(),
                            session_id,
                            message_count,
                        )
                    };

                    // [P1 FIX] Enhanced Peek logic (Reused from above/standard)
                    let mut first_data_chunk = None;
                    let mut retry_this_account = false;

                    loop {
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(60),
                            openai_stream.next(),
                        )
                        .await
                        {
                            Ok(Some(Ok(bytes))) => {
                                if bytes.is_empty() {
                                    continue;
                                }
                                let text = String::from_utf8_lossy(&bytes);
                                if text.trim().starts_with(":")
                                    || text.trim().starts_with("data: :")
                                {
                                    continue;
                                }
                                if text.contains("\"error\"") {
                                    last_error = "Error event during peek".to_string();
                                    retry_this_account = true;
                                    break;
                                }
                                first_data_chunk = Some(bytes);
                                break;
                            }
                            Ok(Some(Err(e))) => {
                                last_error = format!("Stream error during peek: {}", e);
                                retry_this_account = true;
                                break;
                            }
                            Ok(None) => {
                                last_error = "Empty response stream".to_string();
                                retry_this_account = true;
                                break;
                            }
                            Err(_) => {
                                last_error = "Timeout waiting for first data".to_string();
                                retry_this_account = true;
                                break;
                            }
                        }
                    }

                    if retry_this_account {
                        continue;
                    }

                    let combined_stream = futures::stream::once(async move {
                        Ok::<Bytes, String>(first_data_chunk.unwrap())
                    })
                    .chain(openai_stream);

                    // [MULTI-TURN][FIX] 保存本次完整 input_items 到 session store
                    // 使用从 body 中提取的原始 input（含文本/工具调用/工具结果全量历史），
                    // 而非从 openai_req.messages 重建（会丢失 tool_calls/tool 角色等信息）
                    {
                        let save_input = session_save_input.clone();
                        let save_instructions = session_save_instructions.clone();
                        let save_model = openai_req.model.clone();
                        let entry = crate::proxy::http_session_store::HttpSessionEntry {
                            input_items: save_input,
                            instructions: save_instructions,
                            model: save_model,
                            last_accessed: std::time::Instant::now(),
                        };
                        let rid = response_id_for_save.clone();
                        tokio::spawn(async move {
                            crate::proxy::http_session_store::save_session(rid, entry).await;
                        });
                    }
                    return Response::builder()
                        .header("Content-Type", "text/event-stream")
                        .header("Cache-Control", "no-cache")
                        .header("Connection", "keep-alive")
                        .header("X-Account-Email", &email)
                        .header("X-Mapped-Model", &mapped_model)
                        .body(Body::from_stream(combined_stream))
                        .unwrap()
                        .into_response();
                } else {
                    // Forced Stream Internal -> Convert to Legacy JSON
                    // Use CHAT SSE Stream (so Collector can parse it)
                    use crate::proxy::mappers::openai::streaming::create_openai_sse_stream;
                    // Note: We use create_openai_sse_stream regardless of is_codex_style here,
                    // because we just want the content aggregation which chat stream does well.
                    let mut openai_stream = create_openai_sse_stream(
                        Box::pin(gemini_stream),
                        openai_req.model.clone(),
                        session_id,
                        message_count,
                    );

                    // Peek Logic (Repeated for safety/correctness on this stream type)
                    let mut first_data_chunk = None;
                    let mut retry_this_account = false;
                    loop {
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(60),
                            openai_stream.next(),
                        )
                        .await
                        {
                            Ok(Some(Ok(bytes))) => {
                                if bytes.is_empty() {
                                    continue;
                                }
                                let text = String::from_utf8_lossy(&bytes);
                                if text.trim().starts_with(":")
                                    || text.trim().starts_with("data: :")
                                {
                                    continue;
                                }
                                if text.contains("\"error\"") {
                                    last_error = "Error event in internal stream".to_string();
                                    retry_this_account = true;
                                    break;
                                }
                                first_data_chunk = Some(bytes);
                                break;
                            }
                            Ok(Some(Err(e))) => {
                                last_error = format!("Internal stream error: {}", e);
                                retry_this_account = true;
                                break;
                            }
                            Ok(None) => {
                                last_error = "Empty internal stream".to_string();
                                retry_this_account = true;
                                break;
                            }
                            Err(_) => {
                                last_error = "Timeout peek internal".to_string();
                                retry_this_account = true;
                                break;
                            }
                        }
                    }
                    if retry_this_account {
                        continue;
                    }

                    let combined_stream = futures::stream::once(async move {
                        Ok::<Bytes, String>(first_data_chunk.unwrap())
                    })
                    .chain(openai_stream);

                    // Collect
                    use crate::proxy::mappers::openai::collector::collect_stream_to_json;
                    match collect_stream_to_json(Box::pin(combined_stream)).await {
                        Ok(chat_resp) => {
                            let is_responses_api = uri.path() == "/v1/responses";
                            
                            if is_responses_api {
                                let mut output = Vec::new();
                                for c in chat_resp.choices.iter() {
                                    let text = match &c.message.content {
                                        Some(crate::proxy::mappers::openai::OpenAIContent::String(s)) => s.clone(),
                                        _ => "".to_string()
                                    };
                                    
                                    let has_content = !text.is_empty();
                                    let has_tools = c.message.tool_calls.is_some() && !c.message.tool_calls.as_ref().unwrap().is_empty();
                                    
                                    if has_content || has_tools {
                                        let mut msg_obj = serde_json::Map::new();
                                        msg_obj.insert("type".to_string(), json!("message"));
                                        msg_obj.insert("role".to_string(), json!("assistant"));
                                        
                                        if has_content {
                                            msg_obj.insert("content".to_string(), json!(text));
                                        }
                                        if let Some(tool_calls) = &c.message.tool_calls {
                                            msg_obj.insert("tool_calls".to_string(), json!(tool_calls));
                                        }
                                        output.push(serde_json::Value::Object(msg_obj));
                                    }
                                }
                                
                                let resp = json!({
                                    "type": "response",
                                    "id": format!("resp_{}", uuid::Uuid::new_v4().simple()),
                                    "status": "completed",
                                    "output": output
                                });
                                
                                return (
                                    StatusCode::OK,
                                    [
                                        ("X-Account-Email", email.as_str()),
                                        ("X-Mapped-Model", mapped_model.as_str()),
                                    ],
                                    Json(resp),
                                )
                                    .into_response();
                            }

                            // NOW: Convert Chat Response -> Legacy Response (Same logic as below)
                            let choices = chat_resp.choices.iter().map(|c| {
                                let mut text = match &c.message.content {
                                    Some(crate::proxy::mappers::openai::OpenAIContent::String(s)) => s.clone(),
                                    _ => "".to_string()
                                };
                                if let Some(ref reasoning) = c.message.reasoning_content {
                                    if !reasoning.is_empty() {
                                        text = format!("{}\n\n{}", reasoning, text);
                                    }
                                }
                                json!({
                                    "text": text,
                                    "index": c.index,
                                    "logprobs": null,
                                    "finish_reason": c.finish_reason
                                })
                            }).collect::<Vec<_>>();

                            let legacy_resp = json!({
                                "id": chat_resp.id,
                                "object": "text_completion",
                                "created": chat_resp.created,
                                "model": chat_resp.model,
                                "choices": choices,
                                "usage": chat_resp.usage
                            });

                            return (
                                StatusCode::OK,
                                [
                                    ("X-Account-Email", email.as_str()),
                                    ("X-Mapped-Model", mapped_model.as_str()),
                                ],
                                Json(legacy_resp),
                            )
                                .into_response();
                        }
                        Err(e) => {
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                format!("Stream collection error: {}", e),
                            )
                                .into_response();
                        }
                    }
                }
            }

            let gemini_resp: Value = match response.json().await {
                Ok(json) => json,
                Err(e) => {
                    return (
                        StatusCode::BAD_GATEWAY,
                        [("X-Mapped-Model", mapped_model.as_str())],
                        format!("Parse error: {}", e),
                    )
                        .into_response();
                }
            };

            let chat_resp = transform_openai_response(&gemini_resp, Some("session-123"), 1);

            let is_responses_api = uri.path() == "/v1/responses";
            
            if is_responses_api {
                let mut output = Vec::new();
                for c in chat_resp.choices.iter() {
                    let text = match &c.message.content {
                        Some(crate::proxy::mappers::openai::OpenAIContent::String(s)) => s.clone(),
                        _ => "".to_string()
                    };
                    
                    let has_content = !text.is_empty();
                    let has_tools = c.message.tool_calls.is_some() && !c.message.tool_calls.as_ref().unwrap().is_empty();
                    
                    if has_content || has_tools {
                        let mut msg_obj = serde_json::Map::new();
                        msg_obj.insert("type".to_string(), json!("message"));
                        msg_obj.insert("role".to_string(), json!("assistant"));
                        
                        if has_content {
                            msg_obj.insert("content".to_string(), json!(text));
                        }
                        if let Some(tool_calls) = &c.message.tool_calls {
                            msg_obj.insert("tool_calls".to_string(), json!(tool_calls));
                        }
                        output.push(serde_json::Value::Object(msg_obj));
                    }
                }
                
                let resp = json!({
                    "type": "response",
                    "id": format!("resp_{}", uuid::Uuid::new_v4().simple()),
                    "status": "completed",
                    "output": output
                });
                
                return (
                    StatusCode::OK,
                    [
                        ("X-Account-Email", email.as_str()),
                        ("X-Mapped-Model", mapped_model.as_str()),
                    ],
                    Json(resp),
                )
                    .into_response();
            }

            // Map Chat Response -> Legacy Completions Response
            let choices = chat_resp.choices.iter().map(|c| {
                json!({
                    "text": match &c.message.content {
                        Some(crate::proxy::mappers::openai::OpenAIContent::String(s)) => s.clone(),
                        _ => "".to_string()
                    },
                    "index": c.index,
                    "logprobs": null,
                    "finish_reason": c.finish_reason
                })
            }).collect::<Vec<_>>();

            let legacy_resp = json!({
                "id": chat_resp.id,
                "object": "text_completion",
                "created": chat_resp.created,
                "model": chat_resp.model,
                "choices": choices,
                "usage": chat_resp.usage
            });

            return (
                StatusCode::OK,
                [
                    ("X-Account-Email", email.as_str()),
                    ("X-Mapped-Model", mapped_model.as_str()),
                ],
                Json(legacy_resp),
            )
                .into_response();
        }

        // Handle errors and retry
        let status_code = status.as_u16();
        let retry_after = response
            .headers()
            .get("Retry-After")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| format!("HTTP {}", status_code));
        last_error = format!("HTTP {}: {}", status_code, error_text);

        tracing::error!(
            "[Codex-Upstream] Error Response {}: {}",
            status_code,
            error_text
        );

        // 3. 标记限流状态(用于 UI 显示)
        if status_code == 429 || status_code == 529 || status_code == 503 || status_code == 500 {
            token_manager
                .mark_rate_limited_async(
                    &email,
                    status_code,
                    retry_after.as_deref(),
                    &error_text,
                    Some(&mapped_model),
                )
                .await;
        }

        // 确定重试策略
        // 确定重试策略 (对齐官方 1.5s Grace Window)
        let strategy = determine_retry_strategy(status_code, &error_text, false);

        // 执行退备
        if apply_retry_strategy(
            strategy.clone(),
            attempt,
            max_attempts,
            status_code,
            &trace_id,
        )
        .await
        {
            // 继续重试 (loop 会增加 attempt, 导致 force_rotate=true)
            continue;
        } else {
            // 不可重试
            return (
                status,
                [
                    ("X-Account-Email", email.as_str()),
                    ("X-Mapped-Model", mapped_model.as_str()),
                ],
                error_text,
            )
                .into_response();
        }
    }

    // 所有尝试均失败
    if let Some(email) = last_email {
        (
            StatusCode::TOO_MANY_REQUESTS,
            [("X-Account-Email", email), ("X-Mapped-Model", mapped_model)],
            format!("All accounts exhausted. Last error: {}", last_error),
        )
            .into_response()
    } else {
        (
            StatusCode::TOO_MANY_REQUESTS,
            [("X-Mapped-Model", mapped_model)],
            format!("All accounts exhausted. Last error: {}", last_error),
        )
            .into_response()
    }
}

pub async fn handle_list_models(State(state): State<AppState>) -> impl IntoResponse {
    use crate::proxy::common::model_mapping::get_all_dynamic_models;

    let model_ids = get_all_dynamic_models(&state.custom_mapping, Some(&state.token_manager)).await;

    let data: Vec<_> = model_ids
        .into_iter()
        .map(|id| {
            json!({
                "id": id,
                "object": "model",
                "created": 1706745600,
                "owned_by": "antigravity"
            })
        })
        .collect();

    Json(json!({
        "object": "list",
        "data": data
    }))
}

/// OpenAI Images API: POST /v1/images/generations
/// 处理图像生成请求，转换为 Gemini API 格式
pub async fn handle_chat_redirection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    handle_chat_completions(State(state), headers, Json(body)).await
}

async fn intercept_chat_to_image(
    state: AppState,
    body: Value,
    model_name: &str,
) -> Result<Response, (StatusCode, String)> {
    // 1. Extract prompt from messages
    let mut prompt = String::new();
    if let Some(messages) = body.get("messages").and_then(|v| v.as_array()) {
        for msg in messages {
            if msg.get("role").and_then(|v| v.as_str()) == Some("user") {
                if let Some(content) = msg.get("content") {
                    if let Some(s) = content.as_str() {
                        prompt = s.to_string();
                    } else if let Some(arr) = content.as_array() {
                        for part in arr {
                            if part.get("type").and_then(|v| v.as_str()) == Some("text") {
                                prompt.push_str(
                                    part.get("text").and_then(|v| v.as_str()).unwrap_or(""),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    if prompt.is_empty() {
        prompt = "A beautiful painting".to_string(); // fallback
    }

    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // 2. Call internal image generator
    let img_req = json!({
        "prompt": prompt,
        "model": model_name,
        "n": 1,
        "response_format": "url"
    });

    match handle_images_generations_internal(state, img_req).await {
        Ok((email, img_res)) => {
            // Extract URL
            let mut img_markdown = String::new();
            if let Some(data) = img_res.get("data").and_then(|v| v.as_array()) {
                for item in data {
                    if let Some(url) = item.get("url").and_then(|v| v.as_str()) {
                        img_markdown.push_str(&format!("![Generated Image]({})\n\n", url));
                    }
                }
            }

            if img_markdown.is_empty() {
                img_markdown = "Failed to extract image URL from generation result.".to_string();
            }

            // 3. Construct Chat Completion Response
            if is_stream {
                use axum::body::Body;

                let chunk = json!({
                    "id": format!("chatcmpl-img-{}", uuid::Uuid::new_v4()),
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model_name,
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "role": "assistant",
                            "content": img_markdown
                        },
                        "finish_reason": null
                    }]
                });

                let done_chunk = json!({
                    "id": format!("chatcmpl-img-{}", uuid::Uuid::new_v4()),
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model_name,
                    "choices": [{
                        "index": 0,
                        "delta": {},
                        "finish_reason": "stop"
                    }]
                });

                let sse_data = format!(
                    "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                    chunk.to_string(),
                    done_chunk.to_string()
                );

                let body = Body::from(sse_data);
                Ok(Response::builder()
                    .header("Content-Type", "text/event-stream")
                    .header("Cache-Control", "no-cache")
                    .header("X-Account-Email", email)
                    .body(body)
                    .unwrap())
            } else {
                let resp = json!({
                    "id": format!("chatcmpl-img-{}", uuid::Uuid::new_v4()),
                    "object": "chat.completion",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model_name,
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": img_markdown
                        },
                        "finish_reason": "stop"
                    }],
                    "usage": { "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0 }
                });

                Ok((
                    StatusCode::OK,
                    [("X-Account-Email", email.as_str())],
                    Json(resp),
                )
                    .into_response())
            }
        }
        Err((status, msg, _email)) => Err((status, msg)),
    }
}

pub async fn handle_images_generations(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match handle_images_generations_internal(state, body).await {
        Ok((email_header, openai_response)) => Ok((
            StatusCode::OK,
            [
                ("X-Mapped-Model", "dall-e-3"),
                ("X-Account-Email", email_header.as_str()),
            ],
            Json(openai_response),
        )
            .into_response()),
        // Attach the attempted account to error responses too, so the traffic log shows
        // which account the failed (e.g. 502/503) image request used.
        Err((status, msg, email_opt)) => {
            let email = email_opt.unwrap_or_default();
            Ok((status, [("X-Account-Email", email)], msg).into_response())
        }
    }
}

pub async fn handle_images_generations_internal(
    state: AppState,
    body: Value,
) -> Result<(String, Value), (StatusCode, String, Option<String>)> {
    // 1. 解析请求参数
    let prompt = body.get("prompt").and_then(|v| v.as_str()).ok_or((
        StatusCode::BAD_REQUEST,
        "Missing 'prompt' field".to_string(),
        None,
    ))?;

    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini-3-pro-image");

    let n = body.get("n").and_then(|v| v.as_u64()).unwrap_or(1) as usize;

    let size = body.get("size").and_then(|v| v.as_str());

    let response_format = body
        .get("response_format")
        .and_then(|v| v.as_str())
        .unwrap_or("b64_json");

    let quality = body.get("quality").and_then(|v| v.as_str());

    let image_size = body
        .get("image_size")
        .or(body.get("imageSize"))
        .and_then(|v| v.as_str());

    let style = body
        .get("style")
        .and_then(|v| v.as_str())
        .unwrap_or("vivid");

    info!(
        "[Images] Received request: model={}, prompt={:.50}..., n={}, size={}, quality={}, style={}",
        model,
        prompt,
        n,
        size.unwrap_or("auto"),
        quality.unwrap_or("auto"),
        style
    );

    // 2. 使用 common_utils 解析图片配置（统一逻辑，支持动态计算宽高比和 quality 映射）
    let (image_config, clean_model_name) =
        crate::proxy::mappers::common_utils::parse_image_config_with_params(
            model, size, quality, image_size,
        );

    // 3. Prompt Enhancement（保留原有逻辑）
    let mut final_prompt = prompt.to_string();
    if quality == Some("hd") {
        final_prompt.push_str(", (high quality, highly detailed, 4k resolution, hdr)");
    }
    match style {
        "vivid" => final_prompt.push_str(", (vivid colors, dramatic lighting, rich details)"),
        "natural" => final_prompt.push_str(", (natural lighting, realistic, photorealistic)"),
        _ => {}
    }

    // 4. 并发发送请求
    // 注意：不再在外部获取 Token，而是移入 Task 内部并在重试时获取
    let upstream = state.upstream.clone();
    let token_manager = state.token_manager.clone();
    let max_pool_size = token_manager.len();
    let max_attempts = MAX_RETRY_ATTEMPTS
        .min(max_pool_size.saturating_add(1))
        .max(2);

    let mut tasks = Vec::new();

    // Track the last account actually attempted, so error responses (502/503) can be
    // attributed to an account in the traffic log instead of showing "(none)".
    let attempted_account = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));

    for _ in 0..n {
        let upstream = upstream.clone();
        let token_manager = token_manager.clone();
        let final_prompt = final_prompt.clone();
        let image_config = image_config.clone(); // 使用解析后的完整配置
        let _response_format = response_format.to_string();

        let model_to_use = clean_model_name.clone();
        let attempted_account = attempted_account.clone();

        tasks.push(tokio::spawn(async move {
            let mut last_error = String::new();
            let mut force_rotate = false;

            for attempt in 0..max_attempts {
                let (access_token, project_id, email, account_id, _wait_ms) = match token_manager
                    .get_token("image_gen", force_rotate, None, &model_to_use)
                    .await
                {
                    Ok(t) => t,
                    Err(e) => {
                        last_error = format!("Token error: {}", e);
                        if attempt < max_attempts - 1 {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            continue;
                        }
                        break;
                    }
                };
                if let Ok(mut g) = attempted_account.lock() {
                    *g = Some(email.clone());
                }

                // [FIX] Resolve to the account-specific dynamic image model, exactly like the
                // chat (openai.rs:232) and gemini (gemini.rs:155) handlers do. Sending the static
                // alias (e.g. "gemini-3-pro-image") made upstream return 404 "Requested entity was
                // not found" because each account exposes its own concrete image model id.
                let resolved_model = token_manager
                    .resolve_dynamic_model_for_account(&account_id, &model_to_use)
                    .await;

                let gemini_body = json!({
                    "project": project_id,
                    "requestId": format!("agent-{}", uuid::Uuid::new_v4()),
                    "model": resolved_model,
                    "userAgent": "antigravity",
                    "requestType": "image_gen",
                    "request": {
                        "contents": [{
                            "role": "user",
                            "parts": [{"text": final_prompt}]
                        }],
                        "generationConfig": {
                            "candidateCount": 1, // 强制单张
                            "imageConfig": image_config // ✅ 使用完整配置（包含 aspectRatio 和 imageSize）
                        },
                        "safetySettings": [
                            { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "OFF" },
                            { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "OFF" },
                            { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "OFF" },
                            { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "OFF" },
                        ]
                    }
                });

                match upstream
                    .call_v1_internal(
                        "generateContent",
                        &access_token,
                        gemini_body,
                        None,
                        Some(account_id.as_str()),
                    )
                    .await
                {
                    Ok(call_result) => {
                        let response = call_result.response;
                        let status = response.status();
                        if !status.is_success() {
                            let err_text = response.text().await.unwrap_or_default();
                            let status_code = status.as_u16();
                            last_error = format!("Upstream error {}: {}", status, err_text);

                            // 429/500/503: mark limited and rotate to another account
                            if status_code == 429 || status_code == 503 || status_code == 500 {
                                tracing::warn!(
                                    "[Images] Account {} rate limited/error ({}), rotating...",
                                    email,
                                    status_code
                                );
                                token_manager
                                    .mark_rate_limited_async(
                                        &email,
                                        status_code,
                                        None,
                                        &err_text,
                                        Some(model_to_use.as_str()),
                                    )
                                    .await;
                                force_rotate = true;
                                continue; // Retry loop
                            }

                            // [FIX] 403/404 usually mean THIS account lacks the image model or
                            // project access. Rotate to another account instead of failing the
                            // whole request, so an image-capable account can serve it.
                            if (status_code == 403 || status_code == 404)
                                && attempt < max_attempts - 1
                            {
                                tracing::warn!(
                                    "[Images] Account {} returned {} for image gen, rotating to another account",
                                    email,
                                    status_code
                                );
                                force_rotate = true;
                                continue;
                            }

                            // Other errors: return
                            return Err(last_error);
                        }
                        match response.json::<Value>().await {
                            Ok(json) => return Ok((json, email)),
                            Err(e) => return Err(format!("Parse error: {}", e)),
                        }
                    }
                    Err(e) => {
                        last_error = format!("Network error: {}", e);
                        continue;
                    }
                }
            }

            // All attempts failed
            Err(format!("Max retries exhausted. Last error: {}", last_error))
        }));
    }

    // 5. 收集结果
    let mut images: Vec<Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut used_email: Option<String> = None;

    for (idx, task) in tasks.into_iter().enumerate() {
        match task.await {
            Ok(result) => match result {
                Ok((gemini_resp, email_used)) => {
                    // Capture the email from the first successful task for logging
                    if used_email.is_none() {
                        used_email = Some(email_used);
                    }
                    let raw = gemini_resp.get("response").unwrap_or(&gemini_resp);
                    if let Some(parts) = raw
                        .get("candidates")
                        .and_then(|c| c.get(0))
                        .and_then(|cand| cand.get("content"))
                        .and_then(|content| content.get("parts"))
                        .and_then(|p| p.as_array())
                    {
                        for part in parts {
                            if let Some(img) = part.get("inlineData") {
                                let data = img.get("data").and_then(|v| v.as_str()).unwrap_or("");
                                if !data.is_empty() {
                                    if response_format == "url" {
                                        let mime_type = img
                                            .get("mimeType")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("image/png");
                                        images.push(json!({
                                            "url": format!("data:{};base64,{}", mime_type, data)
                                        }));
                                    } else {
                                        images.push(json!({
                                            "b64_json": data
                                        }));
                                    }
                                    tracing::debug!("[Images] Task {} succeeded", idx);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("[Images] Task {} failed: {}", idx, e);
                    errors.push(e);
                }
            },
            Err(e) => {
                let err_msg = format!("Task join error: {}", e);
                tracing::error!("[Images] Task {} join error: {}", idx, e);
                errors.push(err_msg);
            }
        }
    }

    if images.is_empty() {
        let error_msg = if !errors.is_empty() {
            errors.join("; ")
        } else {
            "No images generated".to_string()
        };
        tracing::error!("[Images] All {} requests failed. Errors: {}", n, error_msg);

        // [FIX] Map upstream status codes correctly instead of forcing 502
        let status = if error_msg.contains("429") || error_msg.contains("Quota exhausted") {
            StatusCode::TOO_MANY_REQUESTS
        } else if error_msg.contains("503") || error_msg.contains("Service Unavailable") {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::BAD_GATEWAY
        };

        let attempted = used_email
            .clone()
            .or_else(|| attempted_account.lock().ok().and_then(|g| g.clone()));
        return Err((status, error_msg, attempted));
    }

    // 部分成功时记录警告
    if !errors.is_empty() {
        tracing::warn!(
            "[Images] Partial success: {} out of {} requests succeeded. Errors: {}",
            images.len(),
            n,
            errors.join("; ")
        );
    }

    tracing::info!(
        "[Images] Successfully generated {} out of {} requested image(s)",
        images.len(),
        n
    );

    // 6. 构建 OpenAI 格式响应
    let openai_response = json!({
        "created": chrono::Utc::now().timestamp(),
        "data": images
    });

    // [FIX] 图像生成成功后触发配额刷新 (Issue #1995)
    tokio::spawn(async move {
        let _ = account::refresh_all_quotas_logic().await;
    });

    let email_header = used_email.unwrap_or_default();
    Ok((email_header, openai_response))
}

pub async fn handle_images_edits(
    State(state): State<AppState>,
    mut multipart: axum::extract::Multipart,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    tracing::info!("[Images] Received edit request");

    let mut image_data = None;
    let mut mask_data = None;
    let mut reference_images: Vec<String> = Vec::new(); // Store base64 data of reference images
    let mut prompt = String::new();
    let mut n = 1;
    let mut size = "1024x1024".to_string();
    let mut response_format = "b64_json".to_string();
    let mut model = "gemini-3-pro-image".to_string();
    let mut aspect_ratio: Option<String> = None;
    let mut image_size_param: Option<String> = None;
    let mut style: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Multipart error: {}", e)))?
    {
        let name = field.name().unwrap_or("").to_string();

        if name == "image" {
            let data = field
                .bytes()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("Image read error: {}", e)))?;
            image_data = Some(base64::engine::general_purpose::STANDARD.encode(data));
        } else if name == "mask" {
            let data = field
                .bytes()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("Mask read error: {}", e)))?;
            mask_data = Some(base64::engine::general_purpose::STANDARD.encode(data));
        } else if name.starts_with("image") && name != "image_size" {
            // Support image1, image2, etc.
            let data = field.bytes().await.map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("Reference image read error: {}", e),
                )
            })?;
            reference_images.push(base64::engine::general_purpose::STANDARD.encode(data));
        } else if name == "prompt" {
            prompt = field
                .text()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("Prompt read error: {}", e)))?;
        } else if name == "n" {
            if let Ok(val) = field.text().await {
                n = val.parse().unwrap_or(1);
            }
        } else if name == "size" {
            if let Ok(val) = field.text().await {
                size = val;
            }
        } else if name == "image_size" {
            if let Ok(val) = field.text().await {
                image_size_param = Some(val);
            }
        } else if name == "aspect_ratio" {
            if let Ok(val) = field.text().await {
                aspect_ratio = Some(val);
            }
        } else if name == "style" {
            if let Ok(val) = field.text().await {
                style = Some(val);
            }
        } else if name == "response_format" {
            if let Ok(val) = field.text().await {
                response_format = val;
            }
        } else if name == "model" {
            if let Ok(val) = field.text().await {
                if !val.is_empty() {
                    model = val;
                }
            }
        }
    }

    // Validation: Require either 'image' (standard edit) OR 'prompt' (generation)
    // If reference images are present, we treat it as generation with image context
    if prompt.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Missing prompt".to_string()));
    }

    tracing::info!(
        "[Images] Edit/Ref Request: model={}, prompt={}, n={}, size={}, aspect_ratio={:?}, image_size={:?}, style={:?}, refs={}, has_main_image={}",
        model,
        prompt,
        n,
        size,
        aspect_ratio,
        image_size_param,
        style,
        reference_images.len(),
        image_data.is_some()
    );

    // 2. Prepare Config (Aspect Ratio / Size)
    // Priority: aspect_ratio param > size param
    // Priority: image_size param > quality param (derived from model suffix or default)

    // We reuse parse_image_config_with_params but need to adapt the inputs
    let size_input = aspect_ratio.as_deref().or(Some(&size)); // If aspect_ratio is "16:9", it works. If it's just "1:1", it also works.

    // Map 'image_size' (2K) to 'quality' semantics if needed, or pass directly if logic supports
    // common_utils logic: 'hd' -> 4K, 'medium' -> 2K.
    let quality_input = match image_size_param.as_deref() {
        Some("4K") => Some("hd"),
        Some("2K") => Some("medium"),
        _ => None, // Fallback to standard
    };

    let (image_config, _) = crate::proxy::mappers::common_utils::parse_image_config_with_params(
        &model,
        size_input,
        quality_input,
        image_size_param.as_deref(), // [NEW] Pass direct image_size param
    );

    // 3. Construct Contents
    let mut contents_parts = Vec::new();

    // Add Prompt
    let mut final_prompt = prompt.clone();
    if let Some(s) = style {
        final_prompt.push_str(&format!(", style: {}", s));
    }
    contents_parts.push(json!({
        "text": final_prompt
    }));

    // Add Main Image (if standard edit)
    if let Some(data) = image_data {
        contents_parts.push(json!({
            "inlineData": {
                "mimeType": "image/png",
                "data": data
            }
        }));
    }

    // Add Mask (if standard edit)
    if let Some(data) = mask_data {
        contents_parts.push(json!({
            "inlineData": {
                "mimeType": "image/png",
                "data": data
            }
        }));
    }

    // Add Reference Images (Image-to-Image)
    for ref_data in reference_images {
        contents_parts.push(json!({
            "inlineData": {
                "mimeType": "image/jpeg", // Assume JPEG for refs as per spec suggestion, or auto-detect
                "data": ref_data
            }
        }));
    }

    // 4. 并发发送请求
    // 注意：不再在外部获取 Token，而是移入 Task 内部
    let upstream = state.upstream.clone();
    let token_manager = state.token_manager.clone();
    let max_pool_size = token_manager.len();
    let max_attempts = MAX_RETRY_ATTEMPTS
        .min(max_pool_size.saturating_add(1))
        .max(2);

    let mut tasks = Vec::new();
    for _ in 0..n {
        let upstream = upstream.clone();
        let token_manager = token_manager.clone();
        let contents_parts = contents_parts.clone();
        let image_config = image_config.clone();
        let response_format = response_format.clone();
        let model = model.clone();

        tasks.push(tokio::spawn(async move {
            let mut last_error = String::new();

            let mut force_rotate = false;
            
            for attempt in 0..max_attempts {
                // 4.1 获取 Token
                let (access_token, project_id, email, account_id, _wait_ms) = match token_manager
                    .get_token("image_gen", force_rotate, None, "gemini-3-pro-image")
                    .await
                {
                    Ok(t) => t,
                    Err(e) => {
                        last_error = format!("Token error: {}", e);
                        if attempt < max_attempts - 1 {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            continue;
                        }
                        break;
                    }
                };

                // 4.2 Construct Request Body (Need project_id)
                let gemini_body = json!({
                    "project": project_id,
                    "requestId": format!("img-edit-{}", uuid::Uuid::new_v4()),
                    "model": model,
                    "userAgent": "antigravity",
                    "requestType": "image_gen",
                    "request": {
                        "contents": [{
                            "role": "user",
                            "parts": contents_parts
                        }],
                        "generationConfig": {
                            "candidateCount": 1,
                            "imageConfig": image_config,
                            "maxOutputTokens": 8192,
                            "stopSequences": [],
                            "temperature": 1.0,
                            "topP": 0.95,
                            "topK": 40
                        },
                        "safetySettings": [
                            { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "OFF" },
                            { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "OFF" },
                            { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "OFF" },
                            { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "OFF" },
                        ]
                    }
                });

                match upstream
                    .call_v1_internal(
                        "generateContent",
                        &access_token,
                        gemini_body,
                        None,
                        Some(account_id.as_str()),
                    )
                    .await
                {
                    Ok(call_result) => {
                        let response = call_result.response;
                        let status = response.status();
                        if !status.is_success() {
                            let err_text = response.text().await.unwrap_or_default();
                            let status_code = status.as_u16();
                            last_error = format!("Upstream error {}: {}", status, err_text);

                            // 429/500/503 等错误进行标记和重试
                            if status_code == 429 || status_code == 503 || status_code == 500 {
                                tracing::warn!(
                                    "[Images] Account {} rate limited/error ({}), rotating...",
                                    email,
                                    status_code
                                );
                                token_manager
                                    .mark_rate_limited_async(
                                        &email,
                                        status_code,
                                        None,
                                        &err_text,
                                        Some("dall-e-3"),
                                    )
                                    .await;
                                continue; // Retry loop
                            }
                            return Err(last_error);
                        }
                        match response.json::<Value>().await {
                            Ok(json) => return Ok((json, response_format.clone(), email)),
                            Err(e) => return Err(format!("Parse error: {}", e)),
                        }
                    }
                    Err(e) => {
                        last_error = format!("Network error: {}", e);
                        continue;
                    }
                }
            }
            Err(format!("Max retries exhausted. Last error: {}", last_error))
        }));
    }

    // 5. Collect Results
    let mut images: Vec<Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut used_email: Option<String> = None;

    for (idx, task) in tasks.into_iter().enumerate() {
        match task.await {
            Ok(result) => match result {
                Ok((gemini_resp, response_format, email_used)) => {
                    if used_email.is_none() {
                        used_email = Some(email_used);
                    }
                    let raw = gemini_resp.get("response").unwrap_or(&gemini_resp);
                    if let Some(parts) = raw
                        .get("candidates")
                        .and_then(|c| c.get(0))
                        .and_then(|cand| cand.get("content"))
                        .and_then(|content| content.get("parts"))
                        .and_then(|p| p.as_array())
                    {
                        for part in parts {
                            if let Some(img) = part.get("inlineData") {
                                let data = img.get("data").and_then(|v| v.as_str()).unwrap_or("");
                                if !data.is_empty() {
                                    if response_format == "url" {
                                        let mime_type = img
                                            .get("mimeType")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("image/png");
                                        images.push(json!({
                                            "url": format!("data:{};base64,{}", mime_type, data)
                                        }));
                                    } else {
                                        images.push(json!({
                                            "b64_json": data
                                        }));
                                    }
                                    tracing::debug!("[Images] Task {} succeeded", idx);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("[Images] Task {} failed: {}", idx, e);
                    errors.push(e);
                }
            },
            Err(e) => {
                let err_msg = format!("Task join error: {}", e);
                tracing::error!("[Images] Task {} join error: {}", idx, e);
                errors.push(err_msg);
            }
        }
    }

    if images.is_empty() {
        let error_msg = if !errors.is_empty() {
            errors.join("; ")
        } else {
            "No images generated".to_string()
        };
        tracing::error!(
            "[Images] All {} edit requests failed. Errors: {}",
            n,
            error_msg
        );
        return Err((StatusCode::BAD_GATEWAY, error_msg));
    }

    if !errors.is_empty() {
        tracing::warn!(
            "[Images] Partial success: {} out of {} requests succeeded. Errors: {}",
            images.len(),
            n,
            errors.join("; ")
        );
    }

    tracing::info!(
        "[Images] Successfully generated {} out of {} requested edited image(s)",
        images.len(),
        n
    );

    let openai_response = json!({
        "created": chrono::Utc::now().timestamp(),
        "data": images
    });

    let email_header = used_email.unwrap_or_default();
    Ok((
        StatusCode::OK,
        [
            ("X-Mapped-Model", "dall-e-3"),
            ("X-Account-Email", email_header.as_str()),
        ],
        Json(openai_response),
    )
        .into_response())
}


// ==========================================
// CODE INTEGRATION: Codex WebSocket Handler
// ==========================================

use axum::extract::ws::{WebSocketUpgrade, WebSocket, Message};
use futures::{StreamExt, SinkExt};
use uuid::Uuid;

// ==========================================

// CODE INTEGRATION: Global Tool Call Cache

// ==========================================

use std::sync::OnceLock;

use tokio::sync::RwLock as TokioRwLock;

use std::collections::HashMap;



static WEBSOCKET_TOOL_CALL_CACHE: OnceLock<TokioRwLock<HashMap<String, Value>>> = OnceLock::new();



pub fn get_cached_tool_call(call_id: &str) -> Option<Value> {

    if let Some(cache) = WEBSOCKET_TOOL_CALL_CACHE.get() {

        if let Ok(guard) = cache.try_read() {

            return guard.get(call_id).cloned();

        }

    }

    None

}



pub fn insert_cached_tool_call(call_id: String, item: Value) {

    if call_id.is_empty() {

        return;

    }

    let cache = WEBSOCKET_TOOL_CALL_CACHE.get_or_init(|| TokioRwLock::new(HashMap::new()));

    if let Ok(mut guard) = cache.try_write() {

        guard.insert(call_id, item);

    }

}



#[derive(Debug, Clone)]

struct WebsocketSessionState {
    last_request: Option<Value>,
    last_response_output: Value,
    last_response_id: String,
    last_response_pending_tool_call_ids: Vec<String>,
    tool_call_cache: std::collections::HashMap<String, Value>,
}

pub async fn handle_responses_websocket(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_websocket_session(socket, headers, state))
}

async fn handle_websocket_session(
    mut socket: WebSocket,
    headers: HeaderMap,
    state: AppState,
) {
    tracing::info!("Codex responses websocket: client connected");
    let mut session_state = WebsocketSessionState {
        last_request: None,
        last_response_output: json!([]),
        last_response_id: String::new(),
        last_response_pending_tool_call_ids: Vec::new(),
        tool_call_cache: std::collections::HashMap::new(),
    };

    while let Some(msg_result) = socket.recv().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("responses websocket: read message failed: {:?}", e);
                break;
            }
        };

        let text = match msg {
            Message::Text(t) => t,
            Message::Binary(b) => {
                match String::from_utf8(b) {
                    Ok(s) => s,
                    Err(_) => continue,
                }
            }
            Message::Close(_) => {
                tracing::info!("responses websocket: client disconnected");
                break;
            }
            _ => continue,
        };

        let payload: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                let error_ev = json!({
                    "type": "error",
                    "error": {
                        "message": format!("Invalid JSON: {}", e),
                        "type": "invalid_request_error"
                    }
                });
                let _ = socket.send(Message::Text(error_ev.to_string())).await;
                continue;
            }
        };

        if should_handle_prewarm_locally(&payload, &session_state) {
            let (created, completed) = handle_prewarm_locally(&payload, &mut session_state);
            let _ = socket.send(Message::Text(created.to_string())).await;
            let _ = socket.send(Message::Text(completed.to_string())).await;
            continue;
        }

        let normalized = match normalize_responses_websocket_request(&payload, &mut session_state) {
            Ok(n) => n,
            Err(e) => {
                let error_ev = json!({
                    "type": "error",
                    "error": {
                        "message": e,
                        "type": "invalid_request_error"
                    }
                });
                let _ = socket.send(Message::Text(error_ev.to_string())).await;
                continue;
            }
        };

        let openai_body = convert_codex_to_openai_request(normalized);
        let response_result = handle_chat_completions(State(state.clone()), headers.clone(), Json(openai_body)).await;
        
        let response = match response_result {
            Ok(res) => res.into_response(),
            Err((status, err_msg)) => {
                let error_ev = json!({
                    "type": "error",
                    "error": {
                        "message": err_msg,
                        "type": "server_error",
                        "code": status.as_u16().to_string()
                    }
                });
                let _ = socket.send(Message::Text(error_ev.to_string())).await;
                continue;
            }
        };

        if !response.status().is_success() {
            let error_ev = json!({
                "type": "error",
                "error": {
                    "message": format!("Upstream returned status {}", response.status()),
                    "type": "server_error"
                }
            });
            let _ = socket.send(Message::Text(error_ev.to_string())).await;
            continue;
        }

        let body = response.into_body();
        let mut stream = body.into_data_stream();

        let mut translation_state = TranslationState {
            response_id: format!("resp-{}", &Uuid::new_v4().to_string()[..24]),
            item_id: format!("item-{}", &Uuid::new_v4().to_string()[..16]),
            message_item_added: false,
            content_part_added: false,
            accumulated_text: String::new(),
            tool_calls: std::collections::HashMap::new(),
            tool_calls_added: std::collections::HashSet::new(),
        };

        let created_ev = json!({
            "type": "response.created",
            "response": {
                "id": &translation_state.response_id,
                "object": "response",
                "status": "in_progress",
                "output": []
            }
        });
        let _ = socket.send(Message::Text(created_ev.to_string())).await;

        let mut buffer = bytes::BytesMut::new();
        while let Some(chunk_res) = stream.next().await {
            let chunk = match chunk_res {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("Stream chunk error: {:?}", e);
                    break;
                }
            };
            buffer.extend_from_slice(&chunk);
            while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                let line_raw = buffer.split_to(pos + 1);
                if let Ok(line_str) = std::str::from_utf8(&line_raw) {
                    let line = line_str.trim();
                    if line.is_empty() || !line.starts_with("data: ") {
                        continue;
                    }
                    let json_part = line.trim_start_matches("data: ").trim();
                    if json_part == "[DONE]" {
                        break;
                    }
                    if let Ok(chunk_json) = serde_json::from_str::<Value>(json_part) {
                        translate_openai_chunk_to_ws(&chunk_json, &mut translation_state, &mut socket).await;
                    }
                }
            }
        }

        if !buffer.is_empty() {
            if let Ok(line_str) = std::str::from_utf8(&buffer) {
                let line = line_str.trim();
                if line.starts_with("data: ") {
                    let json_part = line.trim_start_matches("data: ").trim();
                    if json_part != "[DONE]" {
                        if let Ok(chunk_json) = serde_json::from_str::<Value>(json_part) {
                            translate_openai_chunk_to_ws(&chunk_json, &mut translation_state, &mut socket).await;
                        }
                    }
                }
            }
        }

        let completed_output = finalize_ws_events(&mut translation_state, &mut socket, &mut session_state).await;
        
        session_state.last_response_output = completed_output;
        session_state.last_response_id = translation_state.response_id.clone();
        session_state.last_response_pending_tool_call_ids = translation_state.tool_calls.values()
            .map(|(_, call_id, _, _)| call_id.clone()).collect();
    }
}

fn should_handle_prewarm_locally(payload: &Value, state: &WebsocketSessionState) -> bool {
    if state.last_request.is_some() {
        return false;
    }
    let event_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if event_type != "response.create" {
        return false;
    }
    if let Some(generate) = payload.get("generate").and_then(|v| v.as_bool()) {
        if !generate {
            return true;
        }
    }
    false
}

fn handle_prewarm_locally(
    payload: &Value,
    state: &mut WebsocketSessionState,
) -> (Value, Value) {
    let response_id = format!("resp_prewarm_{}", Uuid::new_v4());
    let created_at = chrono::Utc::now().timestamp();
    let model = payload.get("model").and_then(|v| v.as_str()).unwrap_or("unknown");

    let created_ev = json!({
        "type": "response.created",
        "sequence_number": 0,
        "response": {
            "id": &response_id,
            "object": "response",
            "created_at": created_at,
            "status": "in_progress",
            "background": false,
            "error": null,
            "output": [],
            "model": model,
        }
    });

    let completed_ev = json!({
        "type": "response.completed",
        "sequence_number": 1,
        "response": {
            "id": &response_id,
            "object": "response",
            "created_at": created_at,
            "status": "completed",
            "background": false,
            "error": null,
            "output": [],
            "usage": {
                "input_tokens": 0,
                "output_tokens": 0,
                "total_tokens": 0
            },
            "model": model,
        }
    });

    let mut normalized = payload.clone();
    if let Some(obj) = normalized.as_object_mut() {
        obj.remove("type");
        obj.remove("generate");
    }
    state.last_request = Some(normalized);
    state.last_response_output = json!([]);
    state.last_response_id = response_id;
    state.last_response_pending_tool_call_ids = Vec::new();

    (created_ev, completed_ev)
}

fn normalize_responses_websocket_request(
    payload: &Value,
    state: &mut WebsocketSessionState,
) -> Result<Value, String> {
    let event_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match event_type {
        "response.create" => {
            if state.last_request.is_none() {
                let mut normalized = payload.clone();
                if let Some(obj) = normalized.as_object_mut() {
                    obj.remove("type");
                    obj.insert("stream".to_string(), Value::Bool(true));
                    if !obj.contains_key("input") {
                        obj.insert("input".to_string(), json!([]));
                    }
                }
                let model_name = normalized.get("model").and_then(|v| v.as_str()).unwrap_or("");
                if model_name.is_empty() {
                    return Err("missing model in response.create request".to_string());
                }
                state.last_request = Some(normalized.clone());
                Ok(normalized)
            } else {
                normalize_response_subsequent_request(payload, state)
            }
        }
        "response.append" => {
            normalize_response_subsequent_request(payload, state)
        }
        _ => Err(format!("unsupported websocket request type: {}", event_type)),
    }
}

fn normalize_response_subsequent_request(
    payload: &Value,
    state: &mut WebsocketSessionState,
) -> Result<Value, String> {
    if state.last_request.is_none() {
        return Err("websocket request received before response.create".to_string());
    }

    // [FIX] 拦截 compaction 和完整历史替换事件
    if should_replace_websocket_transcript(payload) {
        let mut normalized = payload.clone();
        if let Some(obj) = normalized.as_object_mut() {
            obj.remove("type");
            obj.remove("previous_response_id");
            obj.insert("stream".to_string(), Value::Bool(true));
        }
        state.last_request = Some(normalized.clone());
        return Ok(normalized);
    }

    // [FIX] 始终走完整的 merge 逻辑，废弃 transcript replacement 分支
    // 旧逻辑在检测到 function_call/assistant 时直接替换整个历史，导致多轮对话历史丢失
    // 正确做法：last_request.input + last_response_output + new payload.input 全部合并
    let mut merged_input = Vec::new();

    // 1. 上一轮请求的 input（已含此前所有历史）
    if let Some(last_req) = &state.last_request {
        if let Some(arr) = last_req.get("input").and_then(|v| v.as_array()) {
            merged_input.extend(arr.clone());
        }
    }

    // 2. 上一轮 response 的 output items（assistant 回复、工具调用等）
    if let Some(arr) = state.last_response_output.as_array() {
        merged_input.extend(arr.clone());
    }

    // 3. 本轮新的 input items（用户消息、工具调用结果等）
    if let Some(arr) = payload.get("input").and_then(|v| v.as_array()) {
        for item in arr {
            let t = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if t == "compaction" || t == "compaction_summary" {
                continue;
            }
            if t == "function_call_output" || t == "custom_tool_call_output" {
                if let Some(call_id) = item.get("call_id").and_then(|v| v.as_str()) {
                    state.last_response_pending_tool_call_ids.retain(|x| x != call_id);
                }
            }
            merged_input.push(item.clone());
        }
    }

    repair_tool_calls(&mut merged_input, &state.tool_call_cache);

    let deduped = dedupe_function_calls_by_call_id(dedupe_input_items_by_id(merged_input));

    let mut normalized = payload.clone();
    if let Some(obj) = normalized.as_object_mut() {
        obj.remove("type");
        obj.remove("previous_response_id");
        obj.insert("input".to_string(), json!(deduped));
        if !obj.contains_key("model") {
            if let Some(last_req) = &state.last_request {
                if let Some(model) = last_req.get("model") {
                    obj.insert("model".to_string(), model.clone());
                }
            }
        }
        if !obj.contains_key("instructions") {
            if let Some(last_req) = &state.last_request {
                if let Some(instructions) = last_req.get("instructions") {
                    obj.insert("instructions".to_string(), instructions.clone());
                }
            }
        }
        if !obj.contains_key("tools") {
            if let Some(last_req) = &state.last_request {
                if let Some(tools) = last_req.get("tools") {
                    obj.insert("tools".to_string(), tools.clone());
                }
            }
        }
        if !obj.contains_key("tool_choice") {
            if let Some(last_req) = &state.last_request {
                if let Some(tool_choice) = last_req.get("tool_choice") {
                    obj.insert("tool_choice".to_string(), tool_choice.clone());
                }
            }
        }
        obj.insert("stream".to_string(), Value::Bool(true));
    }
    state.last_request = Some(normalized.clone());
    Ok(normalized)
}
#[allow(dead_code)]
fn should_replace_websocket_transcript(payload: &Value) -> bool {
    let previous_response_id = payload.get("previous_response_id").and_then(|v| v.as_str()).unwrap_or("");
    if !previous_response_id.is_empty() {
        return false;
    }
    if let Some(input_array) = payload.get("input").and_then(|v| v.as_array()) {
        for item in input_array {
            let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if item_type == "function_call" || item_type == "custom_tool_call" {
                return true;
            }
            if item_type == "message" {
                let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("");
                if role == "assistant" {
                    return true;
                }
            }
        }
    }
    false
}

#[allow(dead_code)]
fn normalize_response_transcript_replacement(payload: &Value, last_request: &Value) -> Value {
    let mut normalized = payload.clone();
    if let Some(obj) = normalized.as_object_mut() {
        obj.remove("type");
        obj.remove("previous_response_id");
        obj.insert("stream".to_string(), Value::Bool(true));
        if !obj.contains_key("model") {
            if let Some(model) = last_request.get("model") {
                obj.insert("model".to_string(), model.clone());
            }
        }
        if !obj.contains_key("instructions") {
            if let Some(instructions) = last_request.get("instructions") {
                obj.insert("instructions".to_string(), instructions.clone());
            }
        }
    }
    normalized
}

fn dedupe_input_items_by_id(items: Vec<Value>) -> Vec<Value> {
    use std::collections::{HashSet, HashMap};
    let mut referenced_call_ids = HashSet::new();
    for item in &items {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if item_type == "function_call_output" || item_type == "custom_tool_call_output" {
            if let Some(call_id) = item.get("call_id").and_then(|v| v.as_str()) {
                if !call_id.is_empty() {
                    referenced_call_ids.insert(call_id.to_string());
                }
            }
        }
    }

    let mut keep_map: HashMap<String, (usize, bool)> = HashMap::new();
    for (idx, item) in items.iter().enumerate() {
        let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if item_id.is_empty() {
            continue;
        }
        let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
        let is_referenced = !call_id.is_empty() && referenced_call_ids.contains(call_id);
        if let Some(&(existing_idx, existing_referenced)) = keep_map.get(item_id) {
            if is_referenced || !existing_referenced {
                keep_map.insert(item_id.to_string(), (idx, is_referenced));
            }
        } else {
            keep_map.insert(item_id.to_string(), (idx, is_referenced));
        }
    }

    let mut keep_indices = HashSet::new();
    for (_, (idx, _)) in keep_map {
        keep_indices.insert(idx);
    }

    let mut filtered = Vec::new();
    for (idx, item) in items.into_iter().enumerate() {
        let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if !item_id.is_empty() {
            if !keep_indices.contains(&idx) {
                continue;
            }
        }
        filtered.push(item);
    }
    filtered
}

fn dedupe_function_calls_by_call_id(items: Vec<Value>) -> Vec<Value> {
    use std::collections::HashSet;
    let mut seen_call_ids = HashSet::new();
    let mut filtered = Vec::new();
    for item in items {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if item_type == "function_call" || item_type == "custom_tool_call" {
            if let Some(call_id) = item.get("call_id").and_then(|v| v.as_str()) {
                if !call_id.is_empty() {
                    if seen_call_ids.contains(call_id) {
                        continue;
                    }
                    seen_call_ids.insert(call_id.to_string());
                }
            }
        }
        filtered.push(item);
    }
    filtered
}

fn repair_tool_calls(
    input_items: &mut Vec<Value>,
    tool_call_cache: &std::collections::HashMap<String, Value>,
) {
    let mut call_present = std::collections::HashSet::new();
    for item in input_items.iter() {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if item_type == "function_call" || item_type == "custom_tool_call" {
            if let Some(call_id) = item.get("call_id").and_then(|v| v.as_str()) {
                call_present.insert(call_id.to_string());
            }
        }
    }

    let mut new_items = Vec::new();
    let mut inserted = std::collections::HashSet::new();
    for item in input_items.drain(..) {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if item_type == "function_call_output" || item_type == "custom_tool_call_output" {
            if let Some(call_id) = item.get("call_id").and_then(|v| v.as_str()) {
                if !call_id.is_empty() && !call_present.contains(call_id) && !inserted.contains(call_id) {
                    if let Some(cached_call) = tool_call_cache.get(call_id) {
                        new_items.push(cached_call.clone());
                        inserted.insert(call_id.to_string());
                    }
                }
            }
        }
        new_items.push(item);
    }
    *input_items = new_items;
}

fn convert_codex_to_openai_request(mut body: Value) -> Value {
    let instructions = body.get("instructions").and_then(|v| v.as_str()).unwrap_or_default();
    let input_items = body.get("input").and_then(|v| v.as_array());

    let mut messages = Vec::new();
    if !instructions.is_empty() {
        messages.push(json!({ "role": "system", "content": instructions }));
    }

    let mut call_id_to_name = std::collections::HashMap::new();

    if let Some(items) = input_items {
        for item in items {
            let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match item_type {
                "function_call" | "local_shell_call" | "web_search_call" => {
                    let call_id = item.get("call_id").and_then(|v| v.as_str())
                        .or_else(|| item.get("id").and_then(|v| v.as_str())).unwrap_or("unknown");
                    let mut name = item.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                    if item_type == "local_shell_call" || name == "local_shell_call" {
                        name = "shell".to_string();
                    } else if item_type == "web_search_call" || name == "web_search_call" {
                        name = "google_search".to_string();
                    }
                    call_id_to_name.insert(call_id.to_string(), name);
                }
                _ => {}
            }
        }
    }

    if let Some(items) = input_items {
        for item in items {
            let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match item_type {
                "message" => {
                    let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                    let content = item.get("content").and_then(|v| v.as_array());
                    let mut text_parts = Vec::new();
                    let mut image_parts = Vec::new();

                    if let Some(parts) = content {
                        for part in parts {
                            if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                text_parts.push(text.to_string());
                            } else if part.get("type").and_then(|v| v.as_str()) == Some("input_image") {
                                if let Some(image_url) = part.get("image_url").and_then(|v| v.as_str()) {
                                    image_parts.push(json!({ "type": "image_url", "image_url": { "url": image_url } }));
                                }
                            } else if part.get("type").and_then(|v| v.as_str()) == Some("image_url") {
                                if let Some(url_obj) = part.get("image_url") {
                                    image_parts.push(json!({ "type": "image_url", "image_url": url_obj.clone() }));
                                }
                            }
                        }
                    }

                    if image_parts.is_empty() {
                        messages.push(json!({ "role": role, "content": text_parts.join("
") }));
                    } else {
                        let mut content_blocks = Vec::new();
                        if !text_parts.is_empty() {
                            content_blocks.push(json!({ "type": "text", "text": text_parts.join("
") }));
                        }
                        content_blocks.extend(image_parts);
                        messages.push(json!({ "role": role, "content": content_blocks }));
                    }
                }
                "function_call" | "local_shell_call" | "web_search_call" => {
                    let mut name = item.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let mut args_str = item.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}").to_string();
                    let call_id = item.get("call_id").and_then(|v| v.as_str())
                        .or_else(|| item.get("id").and_then(|v| v.as_str())).unwrap_or("unknown");

                    if item_type == "local_shell_call" || name == "local_shell_call" {
                        name = "shell";
                        if let Some(action) = item.get("action") {
                            if let Some(exec) = action.get("exec") {
                                let mut args_obj = serde_json::Map::new();
                                if let Some(cmd) = exec.get("command") {
                                    let cmd_val = if cmd.is_string() { json!([cmd]) } else { cmd.clone() };
                                    args_obj.insert("command".to_string(), cmd_val);
                                }
                                if let Some(wd) = exec.get("working_directory").or(exec.get("workdir")) {
                                    args_obj.insert("workdir".to_string(), wd.clone());
                                }
                                args_str = serde_json::to_string(&args_obj).unwrap_or_else(|_| "{}".to_string());
                            }
                        }
                    } else if item_type == "web_search_call" || name == "web_search_call" {
                        name = "google_search";
                        if let Some(action) = item.get("action") {
                            let mut args_obj = serde_json::Map::new();
                            if let Some(q) = action.get("query") {
                                args_obj.insert("query".to_string(), q.clone());
                            }
                            args_str = serde_json::to_string(&args_obj).unwrap_or_else(|_| "{}".to_string());
                        }
                    }

                    messages.push(json!({
                        "role": "assistant",
                        "tool_calls": [{
                            "id": call_id,
                            "type": "function",
                            "function": { "name": name, "arguments": args_str }
                        }]
                    }));
                }
                "function_call_output" | "custom_tool_call_output" => {
                    let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let output = item.get("output");
                    let output_str = if let Some(o) = output {
                        if o.is_string() { o.as_str().unwrap().to_string() }
                        else if let Some(content) = o.get("content").and_then(|v| v.as_str()) { content.to_string() }
                        else { o.to_string() }
                    } else { "".to_string() };

                    let name = call_id_to_name.get(call_id).cloned()
                        .or_else(|| get_cached_tool_call(call_id).and_then(|v| v.get("name").and_then(|n| n.as_str()).map(|s| s.to_string())))
                        .unwrap_or_else(|| "shell".to_string());

                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": call_id,
                        "name": name,
                        "content": output_str
                    }));
                }
                _ => {}
            }
        }
    }

    if let Some(obj) = body.as_object_mut() {
        obj.insert("messages".to_string(), json!(messages));
    }
    body
}

struct TranslationState {
    response_id: String,
    item_id: String,
    message_item_added: bool,
    content_part_added: bool,
    accumulated_text: String,
    tool_calls: std::collections::HashMap<u32, (String, String, String, String)>,
    tool_calls_added: std::collections::HashSet<u32>,
}

async fn translate_openai_chunk_to_ws(
    chunk: &Value,
    state: &mut TranslationState,
    socket: &mut WebSocket,
) {
    if let Some(choices) = chunk.get("choices").and_then(|c| c.as_array()) {
        for choice in choices {
            if let Some(delta) = choice.get("delta") {
                if let Some(reasoning) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
                    if !reasoning.is_empty() {
                        let reasoning_ev = json!({
                            "type": "response.reasoning_summary_text.delta",
                            "sequence_number": 0,
                            "item_id": &state.item_id,
                            "output_index": 0,
                            "summary_index": 0,
                            "delta": reasoning
                        });
                        let _ = socket.send(Message::Text(reasoning_ev.to_string())).await;

                        if !state.message_item_added {
                            let item_added = json!({
                                "type": "response.output_item.added",
                                "output_index": 0,
                                "item": {
                                    "id": &state.item_id,
                                    "type": "message",
                                    "role": "assistant",
                                    "status": "in_progress",
                                    "content": []
                                }
                            });
                            let _ = socket.send(Message::Text(item_added.to_string())).await;

                            let part_added = json!({
                                "type": "response.content_part.added",
                                "item_id": &state.item_id,
                                "output_index": 0,
                                "content_index": 0,
                                "part": {
                                    "type": "output_text",
                                    "text": ""
                                }
                            });
                            let _ = socket.send(Message::Text(part_added.to_string())).await;
                            state.message_item_added = true;
                            state.content_part_added = true;
                        }

                        let delta_ev = json!({
                            "type": "response.output_text.delta",
                            "item_id": &state.item_id,
                            "output_index": 0,
                            "content_index": 0,
                            "delta": reasoning
                        });
                        let _ = socket.send(Message::Text(delta_ev.to_string())).await;
                        state.accumulated_text.push_str(reasoning);
                    }
                }

                if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                    if !content.is_empty() {
                        if !state.message_item_added {
                            let item_added = json!({
                                "type": "response.output_item.added",
                                "output_index": 0,
                                "item": {
                                    "id": &state.item_id,
                                    "type": "message",
                                    "role": "assistant",
                                    "status": "in_progress",
                                    "content": []
                                }
                            });
                            let _ = socket.send(Message::Text(item_added.to_string())).await;

                            let part_added = json!({
                                "type": "response.content_part.added",
                                "item_id": &state.item_id,
                                "output_index": 0,
                                "content_index": 0,
                                "part": {
                                    "type": "output_text",
                                    "text": ""
                                }
                            });
                            let _ = socket.send(Message::Text(part_added.to_string())).await;
                            state.message_item_added = true;
                            state.content_part_added = true;
                        }

                        let delta_ev = json!({
                            "type": "response.output_text.delta",
                            "item_id": &state.item_id,
                            "output_index": 0,
                            "content_index": 0,
                            "delta": content
                        });
                        let _ = socket.send(Message::Text(delta_ev.to_string())).await;
                        state.accumulated_text.push_str(content);
                    }
                }

                if let Some(tool_calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc in tool_calls {
                        let tc_idx = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        let tc_id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let tc_name = tc.get("function").and_then(|f| f.get("name")).and_then(|v| v.as_str()).unwrap_or("");
                        let tc_args = tc.get("function").and_then(|f| f.get("arguments")).and_then(|v| v.as_str()).unwrap_or("");

                        if !tc_id.is_empty() || !tc_name.is_empty() {
                            let tool_item_id = format!("item-{}", &Uuid::new_v4().to_string()[..16]);
                            let call_id = if tc_id.is_empty() {
                                format!("call_{}", &Uuid::new_v4().to_string()[..16])
                            } else {
                                tc_id.to_string()
                            };
                            state.tool_calls.insert(tc_idx, (tool_item_id, call_id.clone(), tc_name.to_string(), String::new()));
                            if !tc_name.is_empty() {
                                // 临时插入一个包含 name 的 Value，最终会被 finalize_ws_events 里的完整 Value 覆盖
                                insert_cached_tool_call(call_id, json!({ "name": tc_name }));
                            }
                        }

                        if let Some((tool_item_id, call_id, name, args)) = state.tool_calls.get_mut(&tc_idx) {
                            args.push_str(tc_args);

                            if !state.tool_calls_added.contains(&tc_idx) {
                                let (actual_name, namespace) = split_namespace_tool_name(name);
                                let mut item_obj = serde_json::json!({
                                    "id": tool_item_id,
                                    "type": "function_call",
                                    "status": "in_progress",
                                    "name": actual_name,
                                    "call_id": call_id,
                                    "arguments": ""
                                });
                                if let Some(ns) = namespace {
                                    item_obj["namespace"] = json!(ns);
                                }
                                let tool_added = json!({
                                    "type": "response.output_item.added",
                                    "output_index": 0,
                                    "item": item_obj
                                });
                                let _ = socket.send(Message::Text(tool_added.to_string())).await;
                                state.tool_calls_added.insert(tc_idx);
                            }

                            if !tc_args.is_empty() {
                                let args_delta = json!({
                                    "type": "response.function_call_arguments.delta",
                                    "item_id": tool_item_id,
                                    "output_index": 0,
                                    "delta": tc_args
                                });
                                let _ = socket.send(Message::Text(args_delta.to_string())).await;
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn finalize_ws_events(
    state: &mut TranslationState,
    socket: &mut WebSocket,
    session_state: &mut WebsocketSessionState,
) -> Value {
    let mut output_items = Vec::new();
    let mut tool_keys: Vec<u32> = state.tool_calls.keys().cloned().collect();
    tool_keys.sort();

    for tc_idx in tool_keys {
        if let Some((tool_item_id, call_id, name, args)) = state.tool_calls.get(&tc_idx) {
            let args_done = json!({
                "type": "response.function_call_arguments.done",
                "item_id": tool_item_id,
                "output_index": 0,
                "arguments": args
            });
            let _ = socket.send(Message::Text(args_done.to_string())).await;

            let (actual_name, namespace) = split_namespace_tool_name(name);
            let mut item_obj = serde_json::json!({
                "id": tool_item_id,
                "type": "function_call",
                "status": "completed",
                "name": actual_name,
                "call_id": call_id,
                "arguments": args
            });
            if let Some(ns) = namespace {
                item_obj["namespace"] = json!(ns);
            }

            let tool_done = json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": item_obj
            });
            let _ = socket.send(Message::Text(tool_done.to_string())).await;

            let tc_val = item_obj.clone();
            
            session_state.tool_call_cache.insert(call_id.clone(), tc_val.clone());
            insert_cached_tool_call(call_id.clone(), tc_val.clone());
            output_items.push(tc_val);
        }
    }

    if state.message_item_added {
        let text_done = json!({
            "type": "response.output_text.done",
            "item_id": &state.item_id,
            "output_index": 0,
            "content_index": 0,
            "text": &state.accumulated_text
        });
        let _ = socket.send(Message::Text(text_done.to_string())).await;

        let part_done = json!({
            "type": "response.content_part.done",
            "item_id": &state.item_id,
            "output_index": 0,
            "content_index": 0,
            "part": {
                "type": "output_text",
                "text": &state.accumulated_text
            }
        });
        let _ = socket.send(Message::Text(part_done.to_string())).await;

        let message_done = json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": {
                "id": &state.item_id,
                "type": "message",
                "role": "assistant",
                "status": "completed",
                "content": [{
                    "type": "output_text",
                    "text": &state.accumulated_text
                }]
            }
        });
        let _ = socket.send(Message::Text(message_done.to_string())).await;

        output_items.push(json!({
            "id": &state.item_id,
            "type": "message",
            "role": "assistant",
            "status": "completed",
            "content": [{
                "type": "output_text",
                "text": &state.accumulated_text
            }]
        }));
    }

    let completed_ev = json!({
        "type": "response.completed",
        "response": {
            "id": &state.response_id,
            "object": "response",
            "status": "completed",
            "output": output_items
        }
    });
    let _ = socket.send(Message::Text(completed_ev.to_string())).await;

    json!(output_items)
}

fn split_namespace_tool_name(qualified_name: &str) -> (String, Option<String>) {
    let name = qualified_name.trim();
    if name.starts_with("mcp__") {
        return (name.to_string(), None);
    }
    if let Some(pos) = name.find("__") {
        if pos > 0 {
            let namespace = name[..pos].to_string();
            let actual_name = name[pos + 2..].to_string();
            return (actual_name, Some(namespace));
        }
    }
    (name.to_string(), None)
}

