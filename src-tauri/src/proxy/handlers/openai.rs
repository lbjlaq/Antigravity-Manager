// OpenAI Handler
use axum::{
    body::Body,
    extract::Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use bytes::Bytes;
use serde_json::{json, Value};
use tracing::{debug, error, info}; // Import Engine trait for encode method

use crate::proxy::mappers::openai::{
    transform_openai_request, transform_openai_response, OpenAIRequest,
};
// use crate::proxy::upstream::client::UpstreamClient; // 通过 state 获取
use crate::proxy::cache::{generate_cache_key, CachedImage};
use crate::proxy::errors::{categorize_error, format_error_message, hash_prompt};
use crate::proxy::server::AppState;

const MAX_RETRY_ATTEMPTS: usize = 3;
use crate::proxy::session_manager::SessionManager;

/// Helper function: Convert safety threshold level to Gemini API threshold value
///
/// # Arguments
/// * `config_threshold` - Optional safety threshold from config/request ("OFF"|"LOW"|"MEDIUM"|"HIGH")
///
/// # Returns
/// Gemini API threshold value:
/// - "OFF" → "OFF" (no filtering)
/// - "LOW" → "BLOCK_ONLY_HIGH" (block only high-risk content)
/// - "MEDIUM" → "BLOCK_MEDIUM_AND_ABOVE" (block medium and high-risk)
/// - "HIGH" → "BLOCK_LOW_AND_ABOVE" (block all except low-risk)
/// - None/Invalid → "OFF" (default, backward compatibility)
fn get_safety_threshold(config_threshold: Option<&str>) -> &'static str {
    match config_threshold {
        Some("OFF") => "OFF",
        Some("LOW") => "BLOCK_ONLY_HIGH",
        Some("MEDIUM") => "BLOCK_MEDIUM_AND_ABOVE",
        Some("HIGH") => "BLOCK_LOW_AND_ABOVE",
        _ => "OFF", // Default: backward compatibility
    }
}

pub async fn handle_chat_completions(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
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
            });
    }

    debug!("Received OpenAI request for model: {}", openai_req.model);

    // Story-013-05: Check response cache for non-streaming thinking requests
    let cache_enabled = state.response_cache.is_some();
    let can_use_cache = cache_enabled && !openai_req.stream && openai_req.reasoning_effort.is_some();

    if can_use_cache {
        if let Some(ref cache) = state.response_cache {
            // Generate cache key based on request parameters
            let messages_val = serde_json::to_value(&openai_req.messages).unwrap_or_default();
            let thinking_level = openai_req.reasoning_effort.as_deref().unwrap_or("NONE");
            let temperature = openai_req.temperature.unwrap_or(0.7);
            let top_p = openai_req.top_p.unwrap_or(0.95);
            let max_tokens = openai_req.max_tokens.unwrap_or(8192) as i32;

            let cache_key = crate::proxy::response_cache::generate_cache_key(
                &openai_req.model,
                &messages_val,
                thinking_level,
                temperature,
                top_p,
                max_tokens,
            );

            // Try cache lookup
            if let Some(cached_response) = cache.get(&cache_key) {
                info!(
                    category = "cache",
                    cache_hit = true,
                    model = %openai_req.model,
                    thinking_level = %thinking_level,
                    "Response cache hit - returning cached response"
                );
                return Ok((StatusCode::OK, Json(cached_response)).into_response());
            }
        }
    }

    // 1. 获取 UpstreamClient (Clone handle)
    let upstream = state.upstream.clone();
    let token_manager = state.token_manager;
    let pool_size = token_manager.len();
    let max_attempts = MAX_RETRY_ATTEMPTS.min(pool_size).max(1);

    let mut last_error = String::new();

    for attempt in 0..max_attempts {
        // 2. 模型路由解析
        let mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
            &openai_req.model,
            &*state.custom_mapping.read().await,
        );
        // 将 OpenAI 工具转为 Value 数组以便探测联网
        let tools_val: Option<Vec<Value>> = openai_req.tools.as_ref().map(|list| list.to_vec());
        let config = crate::proxy::mappers::common_utils::resolve_request_config(
            &openai_req.model,
            &mapped_model,
            &tools_val,
        );

        // 3. 提取 SessionId (粘性指纹)
        let session_id = SessionManager::extract_openai_session_id(&openai_req);

        // 4. 获取 Token (使用准确的 request_type)
        // 关键：在重试尝试 (attempt > 0) 时强制轮换账号
        // 🆕 传递模型参数实现 model-aware rate limiting
        let (access_token, project_id, email) = match token_manager
            .get_token(
                &config.request_type,
                attempt > 0,
                Some(&session_id),
                Some(&mapped_model),
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

        info!("✓ Using account: {} (type: {})", email, config.request_type);

        // 4. 转换请求
        let gemini_body = match transform_openai_request(&openai_req, &project_id, &mapped_model) {
            Ok(body) => body,
            Err(e) => {
                error!("[OpenAI] Request transformation failed: {}", e);
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        json!({
                            "error": {
                                "message": e,
                                "type": "invalid_request_error",
                                "code": "validation_error"
                            }
                        })
                        .to_string(),
                    ))
                    .unwrap());
            }
        };

        // [New] 打印转换后的报文 (Gemini Body) 供调试
        if let Ok(body_json) = serde_json::to_string_pretty(&gemini_body) {
            debug!("[OpenAI-Request] Transformed Gemini Body:\n{}", body_json);
        }

        // 5. 发送请求 - 自动转换逻辑
        let client_wants_stream = openai_req.stream;
        // [AUTO-CONVERSION] 非 Stream 请求自动转换为 Stream 以享受更宽松的配额
        let force_stream_internally = !client_wants_stream;
        let actual_stream = client_wants_stream || force_stream_internally;

        if force_stream_internally {
            info!("[OpenAI] 🔄 Auto-converting non-stream request to stream for better quota");
        }

        let method = if actual_stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        let query_string = if actual_stream { Some("alt=sse") } else { None };

        let response = match upstream
            .call_v1_internal(method, &access_token, gemini_body, query_string)
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

        let status = response.status();
        if status.is_success() {
            // 5. 处理流式 vs 非流式
            if actual_stream {
                use crate::proxy::mappers::openai::streaming::create_openai_sse_stream;
                use axum::body::Body;
                use axum::response::Response;

                let gemini_stream = response.bytes_stream();
                let openai_stream =
                    create_openai_sse_stream(Box::pin(gemini_stream), openai_req.model.clone());

                // 判断客户端期望的格式
                if client_wants_stream {
                    // 客户端本就要 Stream，直接返回 SSE
                    let body = Body::from_stream(openai_stream);
                    return Ok(Response::builder()
                        .header("Content-Type", "text/event-stream")
                        .header("Cache-Control", "no-cache")
                        .header("Connection", "keep-alive")
                        .header("X-Account-Email", &email)
                        .header("X-Mapped-Model", &mapped_model)
                        .body(body)
                        .unwrap()
                        .into_response());
                } else {
                    // 客户端要非 Stream，需要收集完整响应并转换为 JSON
                    use crate::proxy::mappers::openai::collect_openai_stream_to_json;
                    use futures::StreamExt;

                    // 转换为 io::Error stream
                    let sse_stream = openai_stream.map(|result| -> Result<Bytes, std::io::Error> {
                        match result {
                            Ok(bytes) => Ok(bytes),
                            Err(e) => Err(std::io::Error::other(e)),
                        }
                    });

                    match collect_openai_stream_to_json(sse_stream).await {
                        Ok(full_response) => {
                            info!("[OpenAI] ✓ Stream collected and converted to JSON");
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
                            return Err((
                                StatusCode::INTERNAL_SERVER_ERROR,
                                format!("Stream collection error: {}", e),
                            ));
                        }
                    }
                }
            }

            let gemini_resp: Value = response
                .json()
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Parse error: {}", e)))?;

            let openai_response = transform_openai_response(&gemini_resp);

            // Epic-008 Story-012-02: Record feedback for budget optimization (async, non-blocking)
            {
                let optimizer = state.budget_optimizer.clone();
                let request_clone = openai_req.clone();
                let response_clone = serde_json::to_value(&openai_response).unwrap_or_default();

                tokio::spawn(async move {
                    use crate::proxy::handlers::feedback_utils;

                    let prompt = feedback_utils::extract_openai_prompt(&request_clone);
                    if !prompt.is_empty() {
                        let budget_used = feedback_utils::extract_openai_budget(&response_clone);
                        let quality_score =
                            feedback_utils::calculate_openai_quality(&response_clone, budget_used);

                        optimizer.record_feedback(&prompt, budget_used, quality_score);
                        tracing::debug!(
                            "[Epic-008] Budget feedback recorded: budget={}, quality={:.2}",
                            budget_used,
                            quality_score
                        );
                    }
                });
            }

            // Story-013-05: Cache successful non-streaming thinking responses
            if can_use_cache {
                if let Some(ref cache) = state.response_cache {
                    let messages_val = serde_json::to_value(&openai_req.messages).unwrap_or_default();
                    let thinking_level = openai_req.reasoning_effort.as_deref().unwrap_or("NONE");
                    let temperature = openai_req.temperature.unwrap_or(0.7);
                    let top_p = openai_req.top_p.unwrap_or(0.95);
                    let max_tokens = openai_req.max_tokens.unwrap_or(8192) as i32;

                    let cache_key = crate::proxy::response_cache::generate_cache_key(
                        &openai_req.model,
                        &messages_val,
                        thinking_level,
                        temperature,
                        top_p,
                        max_tokens,
                    );

                    // Cache the response
                    let response_val = serde_json::to_value(&openai_response).unwrap_or_default();
                    cache.put(cache_key, response_val);
                }
            }

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

        // [New] 打印错误报文日志
        tracing::error!(
            "[OpenAI-Upstream] Error Response {}: {}",
            status_code,
            error_text
        );

        // 429/529/503 智能处理
        if status_code == 429 || status_code == 529 || status_code == 503 || status_code == 500 {
            // 记录限流信息 (全局同步) - 🆕 传递模型实现 model-level rate limiting
            token_manager.mark_rate_limited(
                &email,
                status_code,
                retry_after.as_deref(),
                &error_text,
                Some(&mapped_model),
            );

            // 1. 优先尝试解析 RetryInfo (由 Google Cloud 直接下发)
            if let Some(delay_ms) = crate::proxy::upstream::retry::parse_retry_delay(&error_text) {
                let actual_delay = delay_ms.saturating_add(200).min(10_000);
                tracing::warn!(
                    "OpenAI Upstream {} on {} attempt {}/{}, waiting {}ms then retrying",
                    status_code,
                    email,
                    attempt + 1,
                    max_attempts,
                    actual_delay
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(actual_delay)).await;
                continue;
            }

            // 2. 只有明确包含 "QUOTA_EXHAUSTED" 才停止 -> 【Fix PR#493】改为继续尝试下一个账号
            if error_text.contains("QUOTA_EXHAUSTED") {
                error!(
                    "OpenAI Quota exhausted (429) on account {} attempt {}/{}, will rotate to next account.",
                    email,
                    attempt + 1,
                    max_attempts
                );
                // 标记该账号受限 (已经在上面的 mark_rate_limited 完成)
                // 继续循环以尝试下一个账号
                continue;
            }

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

        // 只有 403 (权限/地区限制) 和 401 (认证失效) 触发账号轮换
        if status_code == 403 || status_code == 401 {
            tracing::warn!(
                "OpenAI Upstream {} on account {} attempt {}/{}, rotating account",
                status_code,
                email,
                attempt + 1,
                max_attempts
            );
            continue;
        }

        // 404 等由于模型配置或路径错误的 HTTP 异常，直接报错，不进行无效轮换
        error!(
            "OpenAI Upstream non-retryable error {} on account {}: {}",
            status_code, email, error_text
        );
        return Err((status, error_text));
    }

    // 所有尝试均失败
    Err((
        StatusCode::TOO_MANY_REQUESTS,
        format!("All accounts exhausted. Last error: {}", last_error),
    ))
}

/// 处理 Legacy Completions API (/v1/completions)
/// 将 Prompt 转换为 Chat Message 格式，复用 handle_chat_completions
pub async fn handle_completions(
    State(state): State<AppState>,
    Json(mut body): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    info!(
        "Received /v1/completions or /v1/responses payload: {:?}",
        body
    );

    let is_codex_style = body.get("input").is_some() && body.get("instructions").is_some();

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

    let mut openai_req: OpenAIRequest = serde_json::from_value(body.clone())
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid request: {}", e)))?;

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
            });
    }

    let upstream = state.upstream.clone();
    let token_manager = state.token_manager;
    let pool_size = token_manager.len();
    let max_attempts = MAX_RETRY_ATTEMPTS.min(pool_size).max(1);

    let mut last_error = String::new();

    for _attempt in 0..max_attempts {
        // 1. 模型路由解析
        let mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
            &openai_req.model,
            &*state.custom_mapping.read().await,
        );
        // 将 OpenAI 工具转为 Value 数组以便探测联网
        let tools_val: Option<Vec<Value>> = openai_req.tools.as_ref().map(|list| list.to_vec());
        let config = crate::proxy::mappers::common_utils::resolve_request_config(
            &openai_req.model,
            &mapped_model,
            &tools_val,
        );

        // 🆕 传递模型参数实现 model-aware rate limiting
        let (access_token, project_id, email) = match token_manager
            .get_token(&config.request_type, false, None, Some(&mapped_model))
            .await
        {
            Ok(t) => t,
            Err(e) => {
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!("Token error: {}", e),
                ))
            }
        };

        info!("✓ Using account: {} (type: {})", email, config.request_type);

        let gemini_body = match transform_openai_request(&openai_req, &project_id, &mapped_model) {
            Ok(body) => body,
            Err(e) => {
                error!("[Codex] Request transformation failed: {}", e);
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("Gemini API validation error: {}", e),
                ));
            }
        };

        // [New] 打印转换后的报文 (Gemini Body) 供调试 (Codex 路径)
        if let Ok(body_json) = serde_json::to_string_pretty(&gemini_body) {
            debug!("[Codex-Request] Transformed Gemini Body:\n{}", body_json);
        }

        let list_response = openai_req.stream;
        let method = if list_response {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        let query_string = if list_response { Some("alt=sse") } else { None };

        let response = match upstream
            .call_v1_internal(method, &access_token, gemini_body, query_string)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_error = e.clone();
                continue;
            }
        };

        let status = response.status();
        if status.is_success() {
            if list_response {
                use axum::body::Body;
                use axum::response::Response;

                let gemini_stream = response.bytes_stream();
                let body = if is_codex_style {
                    use crate::proxy::mappers::openai::streaming::create_codex_sse_stream;
                    let s =
                        create_codex_sse_stream(Box::pin(gemini_stream), openai_req.model.clone());
                    Body::from_stream(s)
                } else {
                    use crate::proxy::mappers::openai::streaming::create_legacy_sse_stream;
                    let s =
                        create_legacy_sse_stream(Box::pin(gemini_stream), openai_req.model.clone());
                    Body::from_stream(s)
                };

                return Ok(Response::builder()
                    .header("Content-Type", "text/event-stream")
                    .header("Cache-Control", "no-cache")
                    .header("Connection", "keep-alive")
                    .header("X-Account-Email", &email)
                    .header("X-Mapped-Model", &mapped_model)
                    .body(body)
                    .unwrap()
                    .into_response());
            }

            let gemini_resp: Value = response
                .json()
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Parse error: {}", e)))?;

            let chat_resp = transform_openai_response(&gemini_resp);

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
                "choices": choices
            });

            return Ok(axum::Json(legacy_resp).into_response());
        }

        // Handle errors and retry
        let status_code = status.as_u16();
        let error_text = response.text().await.unwrap_or_default();
        last_error = format!("HTTP {}: {}", status_code, error_text);

        if status_code == 429 || status_code == 403 || status_code == 401 {
            continue;
        }
        return Err((status, error_text));
    }

    Err((
        StatusCode::TOO_MANY_REQUESTS,
        format!("All attempts failed. Last error: {}", last_error),
    ))
}

pub async fn handle_list_models(State(state): State<AppState>) -> impl IntoResponse {
    use crate::proxy::common::model_mapping::get_all_dynamic_models;

    let model_ids = get_all_dynamic_models(&state.custom_mapping).await;

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
pub async fn handle_images_generations(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Track generation time for error logging
    let start_time = std::time::Instant::now();

    // 1. 解析请求参数
    let prompt = body.get("prompt").and_then(|v| v.as_str()).ok_or((
        StatusCode::BAD_REQUEST,
        "Missing 'prompt' field".to_string(),
    ))?;

    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini-3-pro-image");

    let n = body.get("n").and_then(|v| v.as_u64()).unwrap_or(1) as usize;

    let size = body
        .get("size")
        .and_then(|v| v.as_str())
        .unwrap_or("1024x1024");

    let response_format = body
        .get("response_format")
        .and_then(|v| v.as_str())
        .unwrap_or("b64_json");

    let quality = body
        .get("quality")
        .and_then(|v| v.as_str())
        .unwrap_or("standard");
    let style = body
        .get("style")
        .and_then(|v| v.as_str())
        .unwrap_or("vivid");

    // AC-3: Extract safety_threshold from request (per-request override)
    let request_safety = body.get("safety_threshold").and_then(|v| v.as_str());

    // AC-1: Get config-level safety threshold (environment variable)
    let config_safety = state
        .safety_threshold
        .read()
        .await
        .as_deref()
        .map(String::from);

    // AC-3: Request-level override has priority over config-level
    let final_safety_threshold = request_safety.map(String::from).or(config_safety);

    info!(
        "[Images] Received request: model={}, prompt={:.50}..., n={}, size={}, quality={}, style={}",
        model,
        prompt,
        n,
        size,
        quality,
        style
    );

    // [AC-1] Generate cache key from request parameters
    let cache_key = generate_cache_key(model, prompt, Some(quality), Some(style));

    // [AC-2] Try cache lookup (single image only - n=1)
    if n == 1 {
        if let Some(ref cache) = state.image_cache {
            match cache.get(&cache_key).await {
                Ok(Some(cached)) => {
                    info!(
                        "[Images] 🎯 Cache hit for model={}, prompt_hash={}",
                        model, cached.prompt_hash
                    );

                    // Return cached response in OpenAI format
                    let openai_response = json!({
                        "created": cached.created_at,
                        "data": [{
                            "b64_json": cached.b64_json
                        }]
                    });

                    return Ok(Json(openai_response));
                }
                Ok(None) => {
                    debug!(
                        "[Images] Cache miss for model={}, prompt_hash={}",
                        model,
                        hash_prompt(prompt)
                    );
                }
                Err(e) => {
                    // Cache error - log and continue with generation
                    tracing::warn!(
                        "[Images] Cache lookup error (continuing with generation): {}",
                        e
                    );
                }
            }
        }
    }

    // 2. 解析尺寸为宽高比
    let aspect_ratio = match size {
        "1792x768" | "2560x1080" => "21:9", // Ultra-wide
        "1792x1024" | "1920x1080" => "16:9",
        "1024x1792" | "1080x1920" => "9:16",
        "1024x768" | "1280x960" => "4:3",
        "768x1024" | "960x1280" => "3:4",
        _ => "1:1", // 默认 1024x1024
    };

    // Prompt Enhancement
    let mut final_prompt = prompt.to_string();
    if quality == "hd" {
        final_prompt.push_str(", (high quality, highly detailed, 4k resolution, hdr)");
    }
    match style {
        "vivid" => final_prompt.push_str(", (vivid colors, dramatic lighting, rich details)"),
        "natural" => final_prompt.push_str(", (natural lighting, realistic, photorealistic)"),
        _ => {}
    }

    // 3. 获取 Token
    let upstream = state.upstream.clone();
    let token_manager = state.token_manager;

    // 🆕 传递模型参数实现 model-aware rate limiting (image generation)
    let (access_token, project_id, email) = match token_manager
        .get_token("image_gen", false, None, Some(model))
        .await
    {
        Ok(t) => t,
        Err(e) => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Token error: {}", e),
            ))
        }
    };

    info!("✓ Using account: {} for image generation", email);

    // AC-2: Convert safety threshold to Gemini API format
    let safety_threshold = get_safety_threshold(final_safety_threshold.as_deref());

    // 4. 并发发送请求 (解决 candidateCount > 1 不支持的问题)
    let mut tasks = Vec::new();

    for _ in 0..n {
        let upstream = upstream.clone();
        let access_token = access_token.clone();
        let project_id = project_id.clone();
        let final_prompt = final_prompt.clone();
        let aspect_ratio = aspect_ratio.to_string();
        let _response_format = response_format.to_string();

        tasks.push(tokio::spawn(async move {
            let gemini_body = json!({
                "project": project_id,
                "requestId": format!("img-{}", uuid::Uuid::new_v4()),
                "model": "gemini-3-pro-image",
                "userAgent": "antigravity",
                "requestType": "image_gen",
                "request": {
                    "contents": [{
                        "role": "user",
                        "parts": [{"text": final_prompt}]
                    }],
                    "generationConfig": {
                        "candidateCount": 1, // 强制单张
                        "imageConfig": {
                            "aspectRatio": aspect_ratio
                        }
                    },
                    // AC-2 & AC-3: Dynamic safety settings (config/request override)
                    "safetySettings": [
                        { "category": "HARM_CATEGORY_HARASSMENT", "threshold": safety_threshold },
                        { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": safety_threshold },
                        { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": safety_threshold },
                        { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": safety_threshold },
                        { "category": "HARM_CATEGORY_CIVIC_INTEGRITY", "threshold": safety_threshold },
                    ]
                }
            });

            match upstream
                .call_v1_internal("generateContent", &access_token, gemini_body, None)
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    if !status.is_success() {
                        let err_text = response.text().await.unwrap_or_default();
                        return Err(format!("Upstream error {}: {}", status, err_text));
                    }
                    match response.json::<Value>().await {
                        Ok(json) => Ok(json),
                        Err(e) => Err(format!("Parse error: {}", e)),
                    }
                }
                Err(e) => Err(format!("Network error: {}", e)),
            }
        }));
    }

    // 5. 收集结果
    let mut images: Vec<Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for (idx, task) in tasks.into_iter().enumerate() {
        match task.await {
            Ok(result) => match result {
                Ok(gemini_resp) => {
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
                    // Enhanced structured error logging
                    let generation_time_ms = start_time.elapsed().as_millis() as u64;

                    // Parse status code and error text from error message
                    let (status_code, error_text) = if e.contains("Upstream error") {
                        // Format: "Upstream error 429: error details"
                        let parts: Vec<&str> = e.splitn(3, ' ').collect();
                        let code = if parts.len() > 2 {
                            parts[2].trim_end_matches(':').parse::<u16>().unwrap_or(500)
                        } else {
                            500
                        };
                        let text = if let Some(pos) = e.find(": ") {
                            &e[pos + 2..]
                        } else {
                            &e
                        };
                        (code, text)
                    } else {
                        (500, e.as_str())
                    };

                    let category = categorize_error(status_code, error_text);

                    error!(
                        error_type = category.as_str(),
                        account_email = %email,
                        model = %model,
                        prompt_hash = %hash_prompt(&final_prompt),
                        generation_time_ms = generation_time_ms,
                        aspect_ratio = %aspect_ratio,
                        quality = %quality,
                        style = %style,
                        n = n,
                        safety_threshold = %safety_threshold,
                        status_code = status_code,
                        task_index = idx,
                        "{}",
                        format_error_message(category, status_code, error_text)
                    );

                    errors.push(e);
                }
            },
            Err(e) => {
                // Enhanced structured error logging for task join errors
                let generation_time_ms = start_time.elapsed().as_millis() as u64;
                let error_text = format!("Task join error: {}", e);
                let category = categorize_error(500, &error_text);

                error!(
                    error_type = category.as_str(),
                    account_email = %email,
                    model = %model,
                    prompt_hash = %hash_prompt(&final_prompt),
                    generation_time_ms = generation_time_ms,
                    aspect_ratio = %aspect_ratio,
                    quality = %quality,
                    style = %style,
                    n = n,
                    safety_threshold = %safety_threshold,
                    status_code = 500,
                    task_index = idx,
                    "{}",
                    format_error_message(category, 500, &error_text)
                );

                errors.push(error_text);
            }
        }
    }

    if images.is_empty() {
        let error_msg = if !errors.is_empty() {
            errors.join("; ")
        } else {
            "No images generated".to_string()
        };

        // Enhanced structured error logging for complete failure
        let generation_time_ms = start_time.elapsed().as_millis() as u64;
        let category = categorize_error(502, &error_msg);

        error!(
            error_type = category.as_str(),
            account_email = %email,
            model = %model,
            prompt_hash = %hash_prompt(&final_prompt),
            generation_time_ms = generation_time_ms,
            aspect_ratio = %aspect_ratio,
            quality = %quality,
            style = %style,
            n = n,
            safety_threshold = %safety_threshold,
            status_code = 502,
            "{}",
            format_error_message(category, 502, &error_msg)
        );

        return Err((StatusCode::BAD_GATEWAY, error_msg));
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

    // [AC-3] Store in cache (single successful image only - n=1)
    if n == 1 && images.len() == 1 {
        if let Some(ref cache) = state.image_cache {
            // Extract b64_json from first image
            if let Some(b64_data) = images[0].get("b64_json").and_then(|v| v.as_str()) {
                let cached_image = CachedImage {
                    b64_json: b64_data.to_string(),
                    model: model.to_string(),
                    created_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    prompt_hash: hash_prompt(&final_prompt),
                    quality: quality.to_string(),
                    style: style.to_string(),
                };

                match cache
                    .set(
                        &cache_key,
                        cached_image,
                        std::time::Duration::from_secs(3600),
                    )
                    .await
                {
                    Ok(_) => {
                        debug!(
                            "[Images] ✓ Cached image for model={}, prompt_hash={}",
                            model,
                            hash_prompt(&final_prompt)
                        );
                    }
                    Err(e) => {
                        tracing::warn!("[Images] Failed to cache image (non-critical): {}", e);
                    }
                }
            }
        }
    }

    // 6. 构建 OpenAI 格式响应
    let openai_response = json!({
        "created": chrono::Utc::now().timestamp(),
        "data": images
    });

    Ok(Json(openai_response))
}

pub async fn handle_images_edits(
    State(state): State<AppState>,
    mut multipart: axum::extract::Multipart,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Track generation time for error logging
    let start_time = std::time::Instant::now();

    tracing::info!("[Images] Received edit request");

    let mut image_data = None;
    let mut mask_data = None;
    let mut prompt = String::new();
    let mut n = 1;
    let mut size = "1024x1024".to_string();
    let mut response_format = "b64_json".to_string(); // Default to b64_json for better compatibility with tools handling edits
    let mut model = "gemini-3-pro-image".to_string();
    let mut request_safety: Option<String> = None; // AC-3: Per-request safety override

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
        } else if name == "safety_threshold" {
            // AC-3: Per-request safety override for image editing
            if let Ok(val) = field.text().await {
                if !val.is_empty() {
                    request_safety = Some(val);
                }
            }
        }
    }

    if image_data.is_none() {
        return Err((StatusCode::BAD_REQUEST, "Missing image".to_string()));
    }
    if prompt.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Missing prompt".to_string()));
    }

    tracing::info!(
        "[Images] Edit Request: model={}, prompt={}, n={}, size={}, mask={}, response_format={}",
        model,
        prompt,
        n,
        size,
        mask_data.is_some(),
        response_format
    );

    // [AC-4] Generate cache key for edit operations
    // Note: Image editing cache keys include hash of image data for uniqueness
    let image_hash = if let Some(ref img_data) = image_data {
        // Use first 16 chars of image data hash
        let image_portion = if img_data.len() > 64 {
            &img_data[0..64]
        } else {
            img_data.as_str()
        };
        hash_prompt(image_portion)
    } else {
        "no_image".to_string()
    };

    let cache_key = format!(
        "img-edit:{}:{}:{}:{}",
        model,
        image_hash,
        hash_prompt(&prompt),
        mask_data.is_some()
    );

    // [AC-4] Try cache lookup for edit operations (single image only - n=1)
    if n == 1 {
        if let Some(ref cache) = state.image_cache {
            match cache.get(&cache_key).await {
                Ok(Some(cached)) => {
                    info!(
                        "[Images] 🎯 Cache hit for edit operation, model={}, prompt_hash={}",
                        model, cached.prompt_hash
                    );

                    // Return cached response in OpenAI format
                    let data_field = if response_format == "url" {
                        json!([{
                            "url": format!("data:image/png;base64,{}", cached.b64_json)
                        }])
                    } else {
                        json!([{
                            "b64_json": cached.b64_json
                        }])
                    };

                    let openai_response = json!({
                        "created": cached.created_at,
                        "data": data_field
                    });

                    return Ok(Json(openai_response));
                }
                Ok(None) => {
                    debug!(
                        "[Images] Cache miss for edit operation, prompt_hash={}",
                        hash_prompt(&prompt)
                    );
                }
                Err(e) => {
                    tracing::warn!("[Images] Cache lookup error for edit (continuing): {}", e);
                }
            }
        }
    }

    // FIX: Client Display Issue
    // Cherry Studio (and potentially others) might accept Data URI for generations but display raw text for edits
    // if 'url' format is used with a data-uri.
    // If request asks for 'url' but we are a local proxy, returning b64_json is often safer for correct rendering if the client supports it.
    // However, strictly following spec means 'url' should be 'url'.
    // Let's rely on client requesting the right thing, BUT allow a server-side heuristic:
    // If we simply return b64_json structure even if url was requested? No, that breaks spec.
    // Instead, let's assume successful clients request b64_json.
    // But if users see raw text, it means client defaulted to 'url' or we defaulted to 'url'.
    // Let's keep the log to confirm.

    // AC-1 & AC-3: Determine safety threshold (request override > config)
    let config_safety = state
        .safety_threshold
        .read()
        .await
        .as_deref()
        .map(String::from);
    let final_safety_threshold = request_safety.or(config_safety);
    let safety_threshold = get_safety_threshold(final_safety_threshold.as_deref());

    // 1. 获取 Upstream
    let upstream = state.upstream.clone();
    let token_manager = state.token_manager;
    // Fix: Proper get_token call with correct signature and unwrap (using image_gen quota)
    // 🆕 传递模型参数实现 model-aware rate limiting (image edit)
    let (access_token, project_id, email) = match token_manager
        .get_token("image_gen", false, None, Some(&model))
        .await
    {
        Ok(t) => t,
        Err(e) => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Token error: {}", e),
            ))
        }
    };

    // 2. 映射配置
    let mut contents_parts = Vec::new();

    // Capture mask presence before moving mask_data
    let has_mask = mask_data.is_some();

    contents_parts.push(json!({
        "text": format!("Edit this image: {}", prompt)
    }));

    if let Some(data) = image_data {
        contents_parts.push(json!({
            "inlineData": {
                "mimeType": "image/png",
                "data": data
            }
        }));
    }

    if let Some(data) = mask_data {
        contents_parts.push(json!({
            "inlineData": {
                "mimeType": "image/png",
                "data": data
            }
        }));
    }

    // 构造 Gemini 内网 API Body (Envelope Structure)
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
                "maxOutputTokens": 8192,
                "stopSequences": [],
                "temperature": 1.0,
                "topP": 0.95,
                "topK": 40
            },
            // AC-2 & AC-3: Dynamic safety settings (config/request override)
            "safetySettings": [
                { "category": "HARM_CATEGORY_HARASSMENT", "threshold": safety_threshold },
                { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": safety_threshold },
                { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": safety_threshold },
                { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": safety_threshold },
                { "category": "HARM_CATEGORY_CIVIC_INTEGRITY", "threshold": safety_threshold },
            ]
        }
    });

    let mut tasks = Vec::new();
    for _ in 0..n {
        let upstream = upstream.clone();
        let access_token = access_token.clone();
        let body = gemini_body.clone();

        tasks.push(tokio::spawn(async move {
            match upstream
                .call_v1_internal("generateContent", &access_token, body, None)
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    if !status.is_success() {
                        let err_text = response.text().await.unwrap_or_default();
                        return Err(format!("Upstream error {}: {}", status, err_text));
                    }
                    match response.json::<Value>().await {
                        Ok(json) => Ok(json),
                        Err(e) => Err(format!("Parse error: {}", e)),
                    }
                }
                Err(e) => Err(format!("Network error: {}", e)),
            }
        }));
    }

    let mut images: Vec<Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for (idx, task) in tasks.into_iter().enumerate() {
        match task.await {
            Ok(result) => match result {
                Ok(gemini_resp) => {
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
                    // Enhanced structured error logging
                    let generation_time_ms = start_time.elapsed().as_millis() as u64;

                    // Parse status code and error text from error message
                    let (status_code, error_text) = if e.contains("Upstream error") {
                        // Format: "Upstream error 429: error details"
                        let parts: Vec<&str> = e.splitn(3, ' ').collect();
                        let code = if parts.len() > 2 {
                            parts[2].trim_end_matches(':').parse::<u16>().unwrap_or(500)
                        } else {
                            500
                        };
                        let text = if let Some(pos) = e.find(": ") {
                            &e[pos + 2..]
                        } else {
                            &e
                        };
                        (code, text)
                    } else {
                        (500, e.as_str())
                    };

                    let category = categorize_error(status_code, error_text);

                    error!(
                        error_type = category.as_str(),
                        account_email = %email,
                        model = %model,
                        prompt_hash = %hash_prompt(&prompt),
                        generation_time_ms = generation_time_ms,
                        operation = "image_edit",
                        size = %size,
                        n = n,
                        has_mask = has_mask,
                        response_format = %response_format,
                        safety_threshold = %safety_threshold,
                        status_code = status_code,
                        task_index = idx,
                        "{}",
                        format_error_message(category, status_code, error_text)
                    );

                    errors.push(e);
                }
            },
            Err(e) => {
                // Enhanced structured error logging for task join errors
                let generation_time_ms = start_time.elapsed().as_millis() as u64;
                let error_text = format!("Task join error: {}", e);
                let category = categorize_error(500, &error_text);

                error!(
                    error_type = category.as_str(),
                    account_email = %email,
                    model = %model,
                    prompt_hash = %hash_prompt(&prompt),
                    generation_time_ms = generation_time_ms,
                    operation = "image_edit",
                    size = %size,
                    n = n,
                    has_mask = has_mask,
                    response_format = %response_format,
                    safety_threshold = %safety_threshold,
                    status_code = 500,
                    task_index = idx,
                    "{}",
                    format_error_message(category, 500, &error_text)
                );

                errors.push(error_text);
            }
        }
    }

    if images.is_empty() {
        let error_msg = if !errors.is_empty() {
            errors.join("; ")
        } else {
            "No images generated".to_string()
        };

        // Enhanced structured error logging for complete failure
        let generation_time_ms = start_time.elapsed().as_millis() as u64;
        let category = categorize_error(502, &error_msg);

        error!(
            error_type = category.as_str(),
            account_email = %email,
            model = %model,
            prompt_hash = %hash_prompt(&prompt),
            generation_time_ms = generation_time_ms,
            operation = "image_edit",
            size = %size,
            n = n,
            has_mask = has_mask,
            response_format = %response_format,
            safety_threshold = %safety_threshold,
            status_code = 502,
            "{}",
            format_error_message(category, 502, &error_msg)
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

    // [AC-4] Store edited image in cache (single successful image only - n=1)
    if n == 1 && images.len() == 1 {
        if let Some(ref cache) = state.image_cache {
            // Extract b64_json from first image (handle both b64_json and url formats)
            let b64_data = if let Some(b64) = images[0].get("b64_json").and_then(|v| v.as_str()) {
                Some(b64.to_string())
            } else if let Some(url) = images[0].get("url").and_then(|v| v.as_str()) {
                // Extract base64 from data URI format: data:image/png;base64,{data}
                url.find("base64,").map(|pos| url[pos + 7..].to_string())
            } else {
                None
            };

            if let Some(b64_data) = b64_data {
                let cached_image = CachedImage {
                    b64_json: b64_data,
                    model: model.to_string(),
                    created_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    prompt_hash: hash_prompt(&prompt),
                    quality: "standard".to_string(), // Edits don't have quality parameter
                    style: "natural".to_string(),    // Edits don't have style parameter
                };

                match cache
                    .set(
                        &cache_key,
                        cached_image,
                        std::time::Duration::from_secs(3600),
                    )
                    .await
                {
                    Ok(_) => {
                        debug!(
                            "[Images] ✓ Cached edited image for model={}, prompt_hash={}",
                            model,
                            hash_prompt(&prompt)
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            "[Images] Failed to cache edited image (non-critical): {}",
                            e
                        );
                    }
                }
            }
        }
    }

    let openai_response = json!({
        "created": chrono::Utc::now().timestamp(),
        "data": images
    });

    Ok(Json(openai_response))
}

#[cfg(test)]
mod tests {
    use super::*;

    // AC-4: Unit Tests for Safety Settings Configuration

    #[test]
    fn test_safety_threshold_off() {
        // Default OFF behavior (backward compatibility)
        let result = get_safety_threshold(None);
        assert_eq!(result, "OFF", "Default threshold should be OFF");

        let result = get_safety_threshold(Some("OFF"));
        assert_eq!(result, "OFF", "Explicit OFF should return OFF");
    }

    #[test]
    fn test_safety_threshold_low() {
        // LOW maps to BLOCK_ONLY_HIGH
        let result = get_safety_threshold(Some("LOW"));
        assert_eq!(
            result, "BLOCK_ONLY_HIGH",
            "LOW should map to BLOCK_ONLY_HIGH"
        );
    }

    #[test]
    fn test_safety_threshold_medium() {
        // MEDIUM maps to BLOCK_MEDIUM_AND_ABOVE
        let result = get_safety_threshold(Some("MEDIUM"));
        assert_eq!(
            result, "BLOCK_MEDIUM_AND_ABOVE",
            "MEDIUM should map to BLOCK_MEDIUM_AND_ABOVE"
        );
    }

    #[test]
    fn test_safety_threshold_high() {
        // HIGH maps to BLOCK_LOW_AND_ABOVE
        let result = get_safety_threshold(Some("HIGH"));
        assert_eq!(
            result, "BLOCK_LOW_AND_ABOVE",
            "HIGH should map to BLOCK_LOW_AND_ABOVE"
        );
    }

    #[test]
    fn test_safety_threshold_invalid() {
        // Invalid values default to OFF
        let result = get_safety_threshold(Some("INVALID"));
        assert_eq!(result, "OFF", "Invalid threshold should default to OFF");

        let result = get_safety_threshold(Some(""));
        assert_eq!(
            result, "OFF",
            "Empty string threshold should default to OFF"
        );

        let result = get_safety_threshold(Some("low")); // lowercase
        assert_eq!(
            result, "OFF",
            "Lowercase 'low' should default to OFF (case-sensitive)"
        );
    }

    #[test]
    fn test_safety_threshold_request_override() {
        // This test validates the override logic conceptually
        // In actual implementation, request-level "HIGH" should override config-level "MEDIUM"

        let config_threshold = Some("MEDIUM");
        let request_threshold = Some("HIGH");

        // Simulate: request_threshold.or(config_threshold)
        let final_threshold = request_threshold.or(config_threshold);
        assert_eq!(
            final_threshold,
            Some("HIGH"),
            "Request-level override should take priority over config"
        );

        let result = get_safety_threshold(final_threshold);
        assert_eq!(
            result, "BLOCK_LOW_AND_ABOVE",
            "Final threshold should be HIGH (BLOCK_LOW_AND_ABOVE)"
        );

        // Test when request is None, config should be used
        let request_threshold: Option<&str> = None;
        let config_threshold = Some("MEDIUM");

        let final_threshold = request_threshold.or(config_threshold);
        assert_eq!(
            final_threshold,
            Some("MEDIUM"),
            "Config-level should be used when request is None"
        );

        let result = get_safety_threshold(final_threshold);
        assert_eq!(
            result, "BLOCK_MEDIUM_AND_ABOVE",
            "Final threshold should be MEDIUM (BLOCK_MEDIUM_AND_ABOVE)"
        );

        // Test when both are None
        let request_threshold: Option<&str> = None;
        let config_threshold: Option<&str> = None;

        let final_threshold = request_threshold.or(config_threshold);
        assert_eq!(final_threshold, None, "Both None should result in None");

        let result = get_safety_threshold(final_threshold);
        assert_eq!(
            result, "OFF",
            "When both are None, default to OFF (backward compatibility)"
        );
    }
}
