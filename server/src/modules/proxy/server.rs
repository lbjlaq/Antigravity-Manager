use axum::{
    extract::State,
    response::{IntoResponse, Response, sse::{Event, Sse}},
    http::StatusCode,
    Json,
};
use std::sync::Arc;
use futures::stream::StreamExt;
use crate::modules::proxy::{converter, client::GeminiClient};
use crate::state::AppState;

// AppState definition removed (using crate::state::AppState)

// AxumServer removed as we integrate directly into main Axum app

// ===== API 处理器 =====

/// 请求处理结果
pub(crate) enum RequestResult {
    Success(Response),
    Retry(String), // 包含重试原因
    Error(Response),
}

/// 聊天补全处理器
pub async fn chat_completions_handler(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(request): Json<converter::OpenAIChatRequest>,
) -> Response {
    // 验证 API Key
    if let Err(e) = crate::routes::verify_api_key(&headers, &state) {
        return e.into_response();
    }
    
    if !state.proxy_enabled.load(std::sync::atomic::Ordering::Relaxed) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": { "message": "Proxy service is stopped", "type": "service_stopped" }
            }))
        ).into_response();
    }
    let max_retries = state.token_manager.len().max(1);
    let mut attempts = 0;
    
    // 克隆请求以支持重试
    let request = Arc::new(request);

    loop {
        attempts += 1;
        
        // 1. 获取 Token
        let token = match state.token_manager.get_token().await {
            Some(t) => t,
            None => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({
                        "error": {
                            "message": "没有可用账号",
                            "type": "no_accounts"
                        }
                    }))
                ).into_response();
            }
        };
        
        tracing::info!("尝试使用账号: {} (第 {}/{} 次尝试)", token.email, attempts, max_retries);

        // 2. 处理请求
        let result = process_request(state.clone(), request.clone(), token.clone()).await;
        
        match result {
            RequestResult::Success(response) => return response,
            RequestResult::Retry(reason) => {
                tracing::warn!("账号 {} 请求失败，准备重试: {}", token.email, reason);
                if attempts >= max_retries {
                    return (
                        StatusCode::TOO_MANY_REQUESTS,
                        Json(serde_json::json!({
                            "error": {
                                "message": format!("所有账号配额已耗尽或请求失败。最后错误: {}", reason),
                                "type": "all_accounts_exhausted"
                            }
                        }))
                    ).into_response();
                }
                // 继续下一次循环，token_manager.get_token() 会自动轮换
                continue;
            },
            RequestResult::Error(response) => return response,
        }
    }
}

/// 统一请求分发入口
pub(crate) async fn process_request(
    state: Arc<AppState>,
    request: Arc<converter::OpenAIChatRequest>,
    token: crate::modules::proxy::token_manager::ProxyToken,
) -> RequestResult {
    let is_stream = request.stream.unwrap_or(false);
    let is_image_model = request.model.contains("gemini-3-pro-image");
    
    // 与桌面版保持一致：只有在 stream=true 时才使用流式响应
    if is_stream {
        if is_image_model {
            handle_image_stream_request(state, request, token).await
        } else {
            handle_stream_request(state, request, token).await
        }
    } else {
        handle_non_stream_request(state, request, token).await
    }
}

/// 处理画图模型的流式请求（模拟流式）
pub(crate) async fn handle_image_stream_request(
    state: Arc<AppState>,
    request: Arc<converter::OpenAIChatRequest>,
    token: crate::modules::proxy::token_manager::ProxyToken,
) -> RequestResult {
    let proxy_config = state.upstream_proxy.read().await.clone();
    let client = GeminiClient::new(state.request_timeout, Some(proxy_config));
    let model = request.model.clone();
    
    let project_id = match get_project_id(&token) {
        Ok(id) => id,
        Err(e) => return RequestResult::Error(e),
    };
    
    tracing::info!("(Image) 开始请求图片生成，模型: {}, Project: {}", request.model, project_id);
    let response_result = client.generate(
        &request,
        &token.access_token,
        project_id,
        &token.session_id,
    ).await;
    
    match response_result {
        Ok(response) => {
            tracing::info!("(Image) 收到响应，开始处理 inline_data...");
            // 2. 处理图片转 Markdown
            let processed_json = process_inline_data(response);
            tracing::info!("(Image) inline_data 处理完成");
            
            // 3. 提取 Markdown 文本
            let content = processed_json["response"]["candidates"][0]["content"]["parts"][0]["text"]
                .as_str()
                .or_else(|| {
                    // 尝试备用路径：有时候 structure 可能略有不同
                    tracing::warn!("(Image) Standard path for image content failed. Checking response structure...");
                    processed_json["candidates"][0]["content"]["parts"][0]["text"].as_str()
                })
                .unwrap_or("生成图片失败或格式错误")
                .to_string();
            
            tracing::info!("(Image) 提取内容长度: {} 字符", content.len());
                
            // 4. 构造 SSE 流
            let stream = async_stream::stream! {
                let chunk = serde_json::json!({
                    "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model,
                    "choices": [
                        {
                            "index": 0,
                            "delta": { "content": content },
                            "finish_reason": null
                        }
                    ]
                });
                yield Ok::<_, axum::Error>(Event::default().data(chunk.to_string()));
                
                let end_chunk = serde_json::json!({
                    "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model,
                    "choices": [
                        {
                            "index": 0,
                            "delta": {},
                            "finish_reason": "stop"
                        }
                    ]
                });
                yield Ok(Event::default().data(end_chunk.to_string()));
                yield Ok(Event::default().data("[DONE]"));
            };
            
            RequestResult::Success(Sse::new(stream).into_response())
        },
        Err(e) => check_retry_error(&e),
    }
}

/// 处理流式请求
pub(crate) async fn handle_stream_request(
    state: Arc<AppState>,
    request: Arc<converter::OpenAIChatRequest>,
    token: crate::modules::proxy::token_manager::ProxyToken,
) -> RequestResult {
    let proxy_config = state.upstream_proxy.read().await.clone();
    let client = GeminiClient::new(state.request_timeout, Some(proxy_config));
    
    let project_id = match get_project_id(&token) {
        Ok(id) => id,
        Err(e) => return RequestResult::Error(e),
    };
    
    let stream_result = client.stream_generate(
        &request,
        &token.access_token,
        project_id,
        &token.session_id,
    ).await;
    
    match stream_result {
        Ok(stream) => {
            let sse_stream = stream.map(move |chunk| {
                match chunk {
                    Ok(data) => Ok(Event::default().data(data)),
                    Err(e) => {
                        tracing::error!("Stream error: {}", e);
                        Err(axum::Error::new(e))
                    }
                }
            });
            RequestResult::Success(Sse::new(sse_stream).into_response())
        },
        Err(e) => check_retry_error(&e),
    }
}

/// 处理非流式请求
pub(crate) async fn handle_non_stream_request(
    state: Arc<AppState>,
    request: Arc<converter::OpenAIChatRequest>,
    token: crate::modules::proxy::token_manager::ProxyToken,
) -> RequestResult {
    let proxy_config = state.upstream_proxy.read().await.clone();
    let client = GeminiClient::new(state.request_timeout, Some(proxy_config));
    
    let project_id = match get_project_id(&token) {
        Ok(id) => id,
        Err(e) => return RequestResult::Error(e),
    };
    
    let response_result = client.generate(
        &request,
        &token.access_token,
        project_id,
        &token.session_id,
    ).await;
    
    match response_result {
        Ok(response) => {
            let processed_response = process_inline_data(response);
            // 转换为 OpenAI 格式（兼容 OpenAI API 客户端）
            let openai_response = convert_gemini_to_openai_format(processed_response, &request.model);
            RequestResult::Success(Json(openai_response).into_response())
        },
        Err(e) => check_retry_error(&e),
    }
}

/// 辅助函数：获取 Project ID
fn get_project_id(token: &crate::modules::proxy::token_manager::ProxyToken) -> Result<&str, Response> {
    token.project_id.as_ref()
        .map(|s| s.as_str())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "message": "没有 project_id",
                        "type": "config_error"
                    }
                }))
            ).into_response()
        })
}

/// 辅助函数：检查错误是否需要重试
fn check_retry_error(error_msg: &str) -> RequestResult {
    // 检查 404/403 - 跳过当前账号，尝试下一个
    // 参考 CLIProxyAPI 的做法：遇到 404/403 时继续尝试其他账号
    if error_msg.contains("404") || error_msg.contains("NOT_FOUND") ||
       error_msg.contains("403") || error_msg.contains("PERMISSION_DENIED") {
        return RequestResult::Retry(format!("账号不支持此模型或无权限，跳过: {}", error_msg));
    }
    
    // 检查 429 或者 配额耗尽 关键字
    if error_msg.contains("429") || 
       error_msg.contains("RESOURCE_EXHAUSTED") || 
       error_msg.contains("QUOTA_EXHAUSTED") ||
       error_msg.contains("The request has been rate limited") ||
       error_msg.contains("closed connection") ||
       error_msg.contains("error sending request") ||
       error_msg.contains("operation timed out") ||
       error_msg.contains("RATE_LIMIT_EXCEEDED") {
        return RequestResult::Retry(error_msg.to_string());
    }
    
    // 其他错误直接返回
    RequestResult::Error((
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "error": {
                "message": format!("Antigravity API 错误: {}", error_msg),
                "type": "api_error"
            }
        }))
    ).into_response())
}

/// 模型列表处理器
pub async fn list_models_handler(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Response {
    // 验证 API Key
    if let Err(e) = crate::routes::verify_api_key(&headers, &state) {
        return e.into_response();
    }
    // 返回 Antigravity 实际可用的模型列表
    let models = serde_json::json!({
        "object": "list",
        "data": [
            // Gemini Native (from Log)
            { "id": "gemini-2.5-flash-thinking", "object": "model", "created": 1734336000, "owned_by": "google", "permission": [] },
            { "id": "gemini-2.5-flash", "object": "model", "created": 1734336000, "owned_by": "google", "permission": [] },
            { "id": "gemini-2.5-flash-lite", "object": "model", "created": 1734336000, "owned_by": "google", "permission": [] },
            { "id": "gemini-2.5-pro", "object": "model", "created": 1734336000, "owned_by": "google", "permission": [] },
            { "id": "gemini-3-pro-low", "object": "model", "created": 1734336000, "owned_by": "google", "permission": [] },
            { "id": "gemini-3-pro-high", "object": "model", "created": 1734336000, "owned_by": "google", "permission": [] },
            { "id": "gemini-3-flash", "object": "model", "created": 1734336000, "owned_by": "google", "permission": [] },

            // Claude Native (from Log)
            { "id": "claude-sonnet-4-5", "object": "model", "created": 1734336000, "owned_by": "anthropic", "permission": [] },
            { "id": "claude-sonnet-4-5-thinking", "object": "model", "created": 1734336000, "owned_by": "anthropic", "permission": [] },
            { "id": "claude-opus-4-5-thinking", "object": "model", "created": 1734336000, "owned_by": "anthropic", "permission": [] },

            // Internal Image Models
            { "id": "gemini-3-pro-image", "object": "model", "created": 1734336000, "owned_by": "google", "permission": [] },
            { "id": "gemini-3-pro-image-16x9", "object": "model", "created": 1734336000, "owned_by": "google", "permission": [] },
            { "id": "gemini-3-pro-image-9x16", "object": "model", "created": 1734336000, "owned_by": "google", "permission": [] },
            { "id": "gemini-3-pro-image-4k", "object": "model", "created": 1734336000, "owned_by": "google", "permission": [] },
            { "id": "gemini-2.5-flash-image", "object": "model", "created": 1759363200, "owned_by": "google", "permission": [] },
            { "id": "gemini-2.5-flash-image-preview", "object": "model", "created": 1756166400, "owned_by": "google", "permission": [] },
            { "id": "gemini-3-pro-image-preview", "object": "model", "created": 1737158400, "owned_by": "google", "permission": [] }
        ]
    });
    
    Json(models).into_response()
}

/// 健康检查处理器
pub async fn health_check_handler() -> Response {
    Json(serde_json::json!({
        "status": "ok"
    })).into_response()
}

/// 处理 Antigravity 响应中的 inlineData(生成的图片)
/// 将 base64 图片转换为 Markdown 格式
/// 处理 Inline Data (base64 图片) 转 Markdown
fn process_inline_data(mut response: serde_json::Value) -> serde_json::Value {
    // 1. 定位 candidates 节点
    // Antigravity 响应可能是 { "candidates": ... } 或 { "response": { "candidates": ... } }
    tracing::debug!("(process_inline_data) 响应顶层键: {:?}", response.as_object().map(|o| o.keys().collect::<Vec<_>>()));
    
    let candidates_node = if response.get("candidates").is_some() {
        tracing::debug!("(process_inline_data) 找到 candidates 在顶层");
        response.get_mut("candidates")
    } else if let Some(r) = response.get_mut("response") {
        tracing::debug!("(process_inline_data) 找到 response 节点，查找 candidates");
        r.get_mut("candidates")
    } else {
        tracing::warn!("(process_inline_data) 未找到 candidates 或 response 节点");
        None
    };

    if let Some(candidates_val) = candidates_node {
        if let Some(candidates) = candidates_val.as_array_mut() {
            tracing::debug!("(process_inline_data) 找到 {} 个 candidates", candidates.len());
            for (idx, candidate) in candidates.iter_mut().enumerate() {
                tracing::debug!("(process_inline_data) 处理 candidate[{}], 键: {:?}", idx, candidate.as_object().map(|o| o.keys().collect::<Vec<_>>()));
                if let Some(content) = candidate.get_mut("content").and_then(|c| c.as_object_mut()) {
                    tracing::debug!("(process_inline_data) candidate[{}] 有 content，查找 parts", idx);
                    if let Some(parts) = content.get_mut("parts").and_then(|p| p.as_array_mut()) {
                        tracing::debug!("(process_inline_data) candidate[{}] 有 {} 个 parts", idx, parts.len());
                        let mut new_parts = Vec::new();
                        
                        for part in parts.iter() {
                            // 检查是否有 inlineData
                            if let Some(inline_data) = part.get("inlineData").and_then(|d| d.as_object()) {
                                let mime_type = inline_data.get("mimeType")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("image/jpeg");
                                let data = inline_data.get("data")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                
                                // 构造 Markdown 图片语法
                                let image_markdown = format!(
                                    "\n\n![Generated Image](data:{};base64,{})\n\n",
                                    mime_type, data
                                );
                                
                                // 替换为文本 part
                                new_parts.push(serde_json::json!({
                                    "text": image_markdown
                                }));
                            } else {
                                // 保留原始 part
                                new_parts.push(part.clone());
                            }
                        }
                        
                        // 更新 parts
                        *parts = new_parts;
                    }
                }
            }
        }
    }
    
    // 直接返回修改后的对象，不再包裹 "response"
    response
}

/// 将 Gemini 格式响应转换为 OpenAI 格式
/// 注意：当前未使用，与桌面版保持一致直接返回 Gemini 格式
/// 将 Gemini 格式响应转换为 OpenAI 格式（用于非流式响应）
fn convert_gemini_to_openai_format(gemini_response: serde_json::Value, model: &str) -> serde_json::Value {
    // 提取 candidates（支持两种格式：{ "candidates": ... } 或 { "response": { "candidates": ... } }）
    let candidates = gemini_response.get("candidates")
        .or_else(|| gemini_response.get("response").and_then(|r| r.get("candidates")))
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();
    
    if candidates.is_empty() {
        return serde_json::json!({
            "error": {
                "message": "No candidates in response",
                "type": "invalid_response"
            }
        });
    }
    
    // 提取第一个 candidate 的内容
    let candidate = &candidates[0];
    let content = candidate.get("content")
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    
    // 提取 finishReason 并转换为 OpenAI 格式
    let gemini_finish_reason = candidate.get("finishReason")
        .and_then(|f| f.as_str());
    
    let finish_reason = match gemini_finish_reason {
        Some("STOP") => Some("stop"),
        Some("MAX_TOKENS") => Some("length"),
        Some("SAFETY") => Some("content_filter"),
        Some("RECITATION") => Some("content_filter"),
        _ => None
    };
    
    // 提取 usage metadata
    let usage_metadata = gemini_response.get("response")
        .and_then(|r| r.get("usageMetadata"))
        .or_else(|| gemini_response.get("usageMetadata"));
    
    let prompt_tokens = usage_metadata
        .and_then(|u| u.get("promptTokenCount"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0);
    
    let completion_tokens = usage_metadata
        .and_then(|u| u.get("candidatesTokenCount"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0);
    
    let total_tokens = usage_metadata
        .and_then(|u| u.get("totalTokenCount"))
        .and_then(|t| t.as_u64())
        .unwrap_or(prompt_tokens + completion_tokens);
    
    // 构造 OpenAI 格式响应
    serde_json::json!({
        "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content
            },
            "finish_reason": finish_reason
        }],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": total_tokens
        }
    })
}

/// Anthropic Messages 处理器
pub async fn anthropic_messages_handler(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(request): Json<converter::AnthropicChatRequest>,
) -> Response {
    // 验证 API Key（Anthropic 格式使用 x-api-key 头）
    if let Err(e) = crate::routes::verify_api_key(&headers, &state) {
        return e.into_response();
    }
    
    // 检查代理服务是否启用
    if !state.proxy_enabled.load(std::sync::atomic::Ordering::Relaxed) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "type": "error",
                "error": {
                    "type": "service_stopped",
                    "message": "Proxy service is stopped"
                }
            }))
        ).into_response();
    }
    
    // 记录请求信息
    let stream_mode = request.stream.unwrap_or(true);
    let msg_count = request.messages.len();
    let first_msg_preview = if let Some(first_msg) = request.messages.first() {
        // content 是 Vec<AnthropicContent>
        if let Some(first_content) = first_msg.content.first() {
            match first_content {
                converter::AnthropicContent::Text { text } => {
                    if text.len() > 50 {
                        format!("{}...", &text[..50])
                    } else {
                        text.clone()
                    }
                },
                converter::AnthropicContent::Image { .. } => {
                    "[图片]".to_string()
                },
                converter::AnthropicContent::Thinking { .. } => {
                    "[Thinking]".to_string()
                }
            }
        } else {
            "无内容".to_string()
        }
    } else {
        "无消息".to_string()
    };
    
    // 预处理：解析映射后的模型名（仅用于日志显示，实际逻辑在 client 中也会再次处理，或者我们可以这里处理完传进去）
    // 为了保持一致性，我们复用简单的查找逻辑用于日志
    let mapped_model = {
        let mapping_guard = state.anthropic_mapping.read().await;
            // 鲁棒模糊模型映射 (参考 CLIProxyAPI 经验，与 client.rs 同步)
            let initial_m = {
                let mut tmp = request.model.clone();
                for (k, v) in mapping_guard.iter() {
                    if request.model.contains(k) {
                        tmp = v.clone();
                        break;
                    }
                }
                tmp
            };
            
            let lower_name = initial_m.to_lowercase();
            // 最终 API 型号转换：将内部型号转换为 Antigravity Daily API 实际支持的名称
            if lower_name.contains("sonnet") || lower_name.contains("thinking") {
                "gemini-3-pro-preview".to_string()
            } else if lower_name.contains("haiku") {
                "gemini-2.0-flash-exp".to_string()
            } else if lower_name.contains("opus") {
                "gemini-3-pro-preview".to_string()
            } else if lower_name.contains("claude") {
                "gemini-2.5-flash-thinking".to_string()
            } else if lower_name == "gemini-3-pro-high" || lower_name == "gemini-3-pro-low" {
                "gemini-3-pro-preview".to_string()
            } else if lower_name == "gemini-3-flash" {
                "gemini-3-flash-preview".to_string()
            } else {
                initial_m
            }
    };

    // 截断过长的消息预览
    let truncated_preview = if first_msg_preview.len() > 50 {
        format!("{}...", &first_msg_preview[..50])
    } else {
        first_msg_preview.clone()
    };
    
    tracing::info!(
        "(Anthropic) 请求 {} → {} | 消息数:{} | 流式:{} | 预览:{}",
        request.model,
        mapped_model,
        msg_count,
        if stream_mode { "是" } else { "否" },
        truncated_preview
    );
    let max_retries = state.token_manager.len().max(1);
    let mut attempts = 0;
    
    // Check if stream is requested. Default to false? Anthropic usually true for interactive.
    let is_stream = request.stream.unwrap_or(false);
    
    // Clone request for retries
    let request = Arc::new(request);

    loop {
        attempts += 1;
        
        // 1. 获取 Token
        let token = match state.token_manager.get_token().await {
            Some(t) => t,
            None => {
                 return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({
                        "type": "error",
                        "error": {
                            "type": "overloaded_error",
                            "message": "No available accounts"
                        }
                    }))
                ).into_response();
            }
        };
        
        tracing::info!("(Anthropic) 尝试使用账号: {} (第 {}/{} 次尝试)", token.email, attempts, max_retries);

        // 2. 发起请求
        // Helper logic inline to support retries
        let proxy_config = state.upstream_proxy.read().await.clone();
        let client = GeminiClient::new(state.request_timeout, Some(proxy_config));
        let project_id_result = get_project_id(&token);
        
        if let Err(e) = project_id_result {
             // If config error, don't retry, just fail
             return e; // e is Response
        }
        let project_id = project_id_result.unwrap();

        let mapping_guard = state.anthropic_mapping.read().await;
        
        if is_stream {
             let stream_result = client.stream_generate_anthropic(
                &request,
                &token.access_token,
                project_id,
                &token.session_id,
                &mapping_guard,
                state.thought_signature_map.clone()
            ).await;
            
            match stream_result {
                Ok(stream) => {
                    let mut stream = stream;
                    
                    // ⚠️ 预检：如果第一个分片就是错误（例如我们刚加的空响应错误），则触发重试
                    let first_chunk = match futures::StreamExt::next(&mut stream).await {
                        Some(Ok(chunk)) => chunk,
                        Some(Err(e)) => {
                            let check = check_retry_error(&e);
                            match check {
                                RequestResult::Retry(reason) => {
                                    tracing::warn!("(Anthropic) 账号 {} 请求失败，重试: {}", token.email, reason);
                                    if attempts >= max_retries {
                                        return (
                                            StatusCode::TOO_MANY_REQUESTS,
                                            Json(serde_json::json!({
                                                "type": "error",
                                                "error": { "type": "rate_limit_error", "message": format!("Max retries exceeded. Last error: {}", reason) }
                                            }))
                                        ).into_response();
                                    }
                                    continue;
                                },
                                RequestResult::Error(resp) => return resp,
                                RequestResult::Success(resp) => return resp,
                            }
                        },
                        None => continue,
                    };

                    // Success! Convert stream to Anthropic SSE
                    let msg_id = format!("msg_{}", uuid::Uuid::new_v4());
                    let token_clone = token.clone();
                    let _request_clone = Arc::clone(&request);
                    let mut _total_content_length = 0;
                    let mut total_content = String::new(); 
                    let model_name = request.model.clone();

                    // 将拿出的第一个分片重新包装回流中
                    let combined_stream = futures::stream::once(futures::future::ready(Ok(first_chunk))).chain(stream);
                    
                     let sse_stream = async_stream::stream! {
                        // 1. send message_start
                        let start_event = crate::modules::proxy::claude_converter::ClaudeStreamConverter::create_message_start(&msg_id, &model_name);
                        yield Ok::<_, axum::Error>(Event::default().event(start_event.event).data(start_event.data));

                        // 状态机
                        let mut converter = crate::modules::proxy::claude_converter::ClaudeStreamConverter::new();
                         
                        // 2. Loop over combined stream
                        for await chunk_result in combined_stream {
                            match chunk_result {
                                Ok(chunk_str) => {
                                    if chunk_str == "[DONE]" { continue; }
                                    
                                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&chunk_str) {
                                        // 记录请求详情以便调试 promptFeedback
                                        if json.get("candidates").is_none() && json.get("choices").is_none() {
                                             if let Some(feedback) = json.get("promptFeedback") {
                                                tracing::warn!("(Anthropic) 收到 promptFeedback (可能被拦截): {}", feedback);
                                             }
                                        }

                                        let events = converter.process_chunk(&json);
                                        for event in events {
                                            if event.event == "content_block_delta" {
                                                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&event.data) {
                                                    if let Some(delta) = data.get("delta") {
                                                        if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                                                             _total_content_length += text.len();
                                                             total_content.push_str(text);
                                                        } else if let Some(thinking) = delta.get("thinking").and_then(|t| t.as_str()) {
                                                             // 同时也记录 thinking 内容
                                                             _total_content_length += thinking.len();
                                                             total_content.push_str(thinking);
                                                        }
                                                    }
                                                }
                                            } else if event.event == "message_stop" {
                                                if total_content.is_empty() {
                                                    tracing::warn!(
                                                        "(Anthropic) ✓ {} | 回答为空 (可能是 Gemini 返回了非文本数据)",
                                                        token_clone.email
                                                    );
                                                } else {
                                                    let response_preview: String = total_content.chars().take(100).collect();
                                                    let suffix = if total_content.chars().count() > 100 { "..." } else { "" };
                                                    
                                                    tracing::info!(
                                                        "(Anthropic) ✓ {} | 回答: {}{}",
                                                        token_clone.email,
                                                        response_preview,
                                                        suffix
                                                    );
                                                }
                                            }
                                            yield Ok(Event::default().event(event.event).data(event.data));
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Stream error: {}", e);
                                    let err_check = check_retry_error(&e);
                                    if let RequestResult::Retry(reason) = err_check {
                                         tracing::warn!("Stream interrupted (retryable): {}", reason);
                                    }
                                }
                            }
                        }
                    };
                    
                    return Sse::new(sse_stream).into_response();
                },
                Err(e_msg) => {
                    let check = check_retry_error(&e_msg);
                    match check {
                        RequestResult::Retry(reason) => {
                            tracing::warn!("(Anthropic) 账号 {} 请求失败，重试: {}", token.email, reason);
                            if attempts >= max_retries {
                                return (
                                    StatusCode::TOO_MANY_REQUESTS,
                                    Json(serde_json::json!({
                                        "type": "error",
                                        "error": { "type": "rate_limit_error", "message": format!("Max retries exceeded. Last error: {}", reason) }
                                    }))
                                ).into_response();
                            }
                            continue;
                        },
                        RequestResult::Error(resp) => return resp,
                        RequestResult::Success(resp) => return resp,
                    }
                }
            }

        } else {
            // Non-stream: collect streaming response and convert to non-streaming format
            let mapping_guard = state.anthropic_mapping.read().await;
            
            let stream_result = client.stream_generate_anthropic(
                &request,
                &token.access_token,
                project_id,
                &token.session_id,
                &mapping_guard,
                state.thought_signature_map.clone()
            ).await;
            
            match stream_result {
                Ok(mut stream) => {
                    let mut full_text = String::new();
                    let mut stop_reason = "end_turn";
                    
                    // Collect all chunks
                    while let Some(chunk_result) = stream.next().await {
                        match chunk_result {
                            Ok(chunk_str) => {
                                if chunk_str == "[DONE]" { continue; }
                                
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&chunk_str) {
                                    if let Some(content) = json["choices"][0]["delta"]["content"].as_str() {
                                        full_text.push_str(content);
                                    }
                                    if let Some(reason) = json["choices"][0]["finish_reason"].as_str() {
                                        stop_reason = match reason {
                                            "stop" => "end_turn",
                                            "length" => "max_tokens",
                                            _ => "end_turn"
                                        };
                                    }
                                }
                            }
                            Err(_) => {}
                        }
                    }
                    
                    // 收集完后检查是否为空且为 MAX_TOKENS
                    if full_text.is_empty() && stop_reason == "max_tokens" {
                        tracing::warn!("(Anthropic) 非流式：检测到空响应且原因为 MAX_TOKENS，触发重试...");
                        if attempts >= max_retries {
                            // 同上错误返回
                             return (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({ "error": { "message": "Max retries exceeded due to empty MAX_TOKENS responses" } }))).into_response();
                        }
                        continue;
                    }
                    
                    // Build Anthropic non-streaming response
                    let response = serde_json::json!({
                        "id": format!("msg_{}", uuid::Uuid::new_v4()),
                        "type": "message",
                        "role": "assistant",
                        "model": request.model,
                        "content": [{
                            "type": "text",
                            "text": full_text
                        }],
                        "stop_reason": stop_reason,
                        "stop_sequence": null,
                        "usage": {
                            "input_tokens": 0,
                            "output_tokens": 0
                        }
                    });
                    
                    // 记录响应(截取前60字符)
                    let answer_text = response["content"].as_array()
                        .and_then(|arr| arr.first())
                        .and_then(|c| c["text"].as_str())
                        .unwrap_or("");
                    let response_preview: String = answer_text.chars().take(60).collect();
                    let suffix = if answer_text.chars().count() > 60 { "..." } else { "" };
                    
                    tracing::info!(
                        "(Anthropic) ✓ {} | 回答: {}{}",
                        token.email, response_preview, suffix
                    );
                    
                    return (StatusCode::OK, Json(response)).into_response();
                },
                Err(e_msg) => {
                    let check = check_retry_error(&e_msg);
                    match check {
                        RequestResult::Retry(reason) => {
                            tracing::warn!("(Anthropic) 账号 {} 请求失败，重试: {}", token.email, reason);
                            if attempts >= max_retries {
                                return (
                                    StatusCode::TOO_MANY_REQUESTS,
                                    Json(serde_json::json!({
                                        "type": "error",
                                        "error": {
                                            "type": "rate_limit_error",
                                            "message": format!("Max retries exceeded. Last error: {}", reason)
                                        }
                                    }))
                                ).into_response();
                            }
                            continue;
                        },
                        RequestResult::Error(resp) => return resp,
                        RequestResult::Success(resp) => return resp,
                    }
                }
            }
        }
    }
}
