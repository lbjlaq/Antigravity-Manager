// Claude 协议处理器

use axum::{
    body::Body,
    extract::{Json, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};
use tokio::time::{sleep, Duration};
use tracing::{debug, error};

use crate::proxy::mappers::claude::{
    transform_claude_request_in, transform_response, create_claude_sse_stream, ClaudeRequest,
};
use crate::proxy::server::AppState;

const MAX_RETRY_ATTEMPTS: usize = 3;

/// 处理 Claude messages 请求
/// 
/// 处理 Chat 消息请求流程
pub async fn handle_messages(
    State(state): State<AppState>,
    Json(request): Json<ClaudeRequest>,
) -> Response {
    // 获取最新一条“有意义”的消息内容（用于日志记录和后台任务检测）
    // 策略：反向遍历，首先筛选出所有角色为 "user" 的消息，然后从中找到第一条非 "Warmup" 且非空的文本消息
    // 获取最新一条“有意义”的消息内容（用于日志记录和后台任务检测）
    // 策略：反向遍历，首先筛选出所有和用户相关的消息 (role="user")
    // 然后提取其文本内容，跳过 "Warmup" 或系统预设的 reminder
    let meaningful_msg = request.messages.iter().rev()
        .filter(|m| m.role == "user")
        .find_map(|m| {
            let content = match &m.content {
                crate::proxy::mappers::claude::models::MessageContent::String(s) => s.to_string(),
                crate::proxy::mappers::claude::models::MessageContent::Array(arr) => {
                    // 对于数组，提取所有 Text 块并拼接，忽略 ToolResult
                    arr.iter()
                        .filter_map(|block| match block {
                            crate::proxy::mappers::claude::models::ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                }
            };
            
            // 过滤规则：
            // 1. 忽略空消息
            // 2. 忽略 "Warmup" 消息
            // 3. 忽略 <system-reminder> 标签的消息
            if content.trim().is_empty() 
                || content.starts_with("Warmup") 
                || content.contains("<system-reminder>") 
            {
                None 
            } else {
                Some(content)
            }
        });

    // 如果经过过滤还是找不到（例如纯工具调用），则回退到最后一条消息的原始展示
    let latest_msg = meaningful_msg.unwrap_or_else(|| {
        request.messages.last().map(|m| {
            match &m.content {
                crate::proxy::mappers::claude::models::MessageContent::String(s) => s.clone(),
                crate::proxy::mappers::claude::models::MessageContent::Array(_) => "[Complex/Tool Message]".to_string()
            }
        }).unwrap_or_else(|| "[No Messages]".to_string())
    });
    
    crate::modules::logger::log_info(&format!("Received Claude request for model: {}, content_preview: {:.100}...", request.model, latest_msg));

    // 1. 获取 会话 ID (已废弃基于内容的哈希,改用 TokenManager 内部的时间窗口锁定)
    let _session_id: Option<&str> = None;

    // 2. 获取 UpstreamClient
    let upstream = state.upstream.clone();
    
    // 3. 准备闭包
    let mut request_for_body = request.clone();
    let token_manager = state.token_manager;
    
    let pool_size = token_manager.len();
    let max_attempts = MAX_RETRY_ATTEMPTS.min(pool_size).max(1);

    let mut last_error = String::new();
    let mut retried_without_thinking = false;
    
    for attempt in 0..max_attempts {
        // 3. 模型路由与配置解析 (提前解析以确定请求类型)
        let mut mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
            &request_for_body.model,
            &*state.custom_mapping.read().await,
            &*state.openai_mapping.read().await,
            &*state.anthropic_mapping.read().await,
        );
        let config = crate::proxy::mappers::common_utils::resolve_request_config(&request_for_body.model, &mapped_model);

        // 4. 获取 Token (使用准确的 request_type)
        let (access_token, project_id, email) = match token_manager.get_token(&config.request_type, false).await {
            Ok(t) => t,
            Err(e) => {
                 return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({
                        "type": "error",
                        "error": {
                            "type": "overloaded_error",
                            "message": format!("No available accounts: {}", e)
                        }
                    }))
                ).into_response();
            }
        };

        tracing::info!("Using account: {} for request (type: {})", email, config.request_type);
        
        // --- 核心优化：智能识别与拦截后台自动请求 ---
        // 关键词识别：标题生成、摘要提取、下一步提示建议等
        // [Optimization] 使用更长的预览窗口 (500 chars) 以捕获更具体的意图
        let preview_msg = latest_msg.chars().take(500).collect::<String>();
        let is_background_task = preview_msg.contains("write a 5-10 word title") 
            || preview_msg.contains("Respond with the title")
            || preview_msg.contains("Concise summary")
            || preview_msg.contains("prompt suggestion generator");

        // 传递映射后的模型名
        let mut request_with_mapped = request_for_body.clone();

        if is_background_task {
             mapped_model = "gemini-2.5-flash".to_string();
             tracing::info!("[AUTO] 检测到后台自动任务 ({}...)，已智能重定向到廉价节点: {}", 
                preview_msg,
                mapped_model
             );
             // [Optimization] **后台任务净化**: 
             // 此类任务纯粹为文本处理，绝不需要执行工具。
             // 强制清空 tools 字段，彻底根除 "Multiple tools" (400) 冲突风险。
             request_with_mapped.tools = None;
        } else {
             // [USER] 标记真实用户请求
             // [Optimization] 使用 WARN 级别高亮显示用户消息，防止被后台任务日志淹没
             tracing::warn!("[USER] 检测到用户交互请求 ({}...)，保持原模型: {}", 
                preview_msg,
                mapped_model
             );
        }
        
        request_with_mapped.model = mapped_model;

        // 生成 Trace ID (简单用时间戳后缀)
        // let _trace_id = format!("req_{}", chrono::Utc::now().timestamp_subsec_millis());

        let gemini_body = match transform_claude_request_in(&request_with_mapped, &project_id) {
            Ok(b) => b,
            Err(e) => {
                 return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "type": "error",
                        "error": {
                            "type": "api_error",
                            "message": format!("Transform error: {}", e)
                        }
                    }))
                ).into_response();
            }
        };
        
    // 4. 上游调用
    let is_stream = request.stream;
    let method = if is_stream { "streamGenerateContent" } else { "generateContent" };
    let query = if is_stream { Some("alt=sse") } else { None };

    let response = match upstream.call_v1_internal(
        method,
        &access_token,
        gemini_body,
        query
    ).await {
            Ok(r) => r,
            Err(e) => {
                last_error = e.clone();
                tracing::warn!("Request failed on attempt {}/{}: {}", attempt + 1, max_attempts, e);
                continue;
            }
        };
        
        let status = response.status();
        
        // 成功
        if status.is_success() {
            // 处理流式响应
            if request.stream {
                let stream = response.bytes_stream();
                let gemini_stream = Box::pin(stream);
                let claude_stream = create_claude_sse_stream(gemini_stream);

                // 转换为 Bytes stream
                let sse_stream = claude_stream.map(|result| -> Result<Bytes, std::io::Error> {
                    match result {
                        Ok(bytes) => Ok(bytes),
                        Err(e) => Ok(Bytes::from(format!("data: {{\"error\":\"{}\"}}\n\n", e))),
                    }
                });

                return Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/event-stream")
                    .header(header::CACHE_CONTROL, "no-cache")
                    .header(header::CONNECTION, "keep-alive")
                    .body(Body::from_stream(sse_stream))
                    .unwrap();
            } else {
                // 处理非流式响应
                let bytes = match response.bytes().await {
                    Ok(b) => b,
                    Err(e) => return (StatusCode::BAD_GATEWAY, format!("Failed to read body: {}", e)).into_response(),
                };
                
                // Debug print
                if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                    debug!("Upstream Response for Claude request: {}", text);
                }

                let gemini_resp: Value = match serde_json::from_slice(&bytes) {
                    Ok(v) => v,
                    Err(e) => return (StatusCode::BAD_GATEWAY, format!("Parse error: {}", e)).into_response(),
                };

                // 解包 response 字段（v1internal 格式）
                let raw = gemini_resp.get("response").unwrap_or(&gemini_resp);

                // 转换为 Gemini Response 结构
                let gemini_response: crate::proxy::mappers::claude::models::GeminiResponse = match serde_json::from_value(raw.clone()) {
                    Ok(r) => r,
                    Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("Convert error: {}", e)).into_response(),
                };
                
                // 转换
                let claude_response = match transform_response(&gemini_response) {
                    Ok(r) => r,
                    Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("Transform error: {}", e)).into_response(),
                };

                return Json(claude_response).into_response();
            }
        }
        
        // 处理错误
        let error_text = response.text().await.unwrap_or_else(|_| format!("HTTP {}", status));
        last_error = format!("HTTP {}: {}", status, error_text);
        
        let status_code = status.as_u16();
        
        // Handle transient 429s using upstream-provided retry delay (avoid surfacing errors to clients).
        if status_code == 429 {
            if let Some(delay_ms) = crate::proxy::upstream::retry::parse_retry_delay(&error_text) {
                let actual_delay = delay_ms.saturating_add(200).min(10_000);
                tracing::warn!(
                    "Claude Upstream 429 on attempt {}/{}, waiting {}ms then retrying",
                    attempt + 1,
                    max_attempts,
                    actual_delay
                );
                sleep(Duration::from_millis(actual_delay)).await;
                continue;
            }
        }

        // Special-case 400 errors caused by invalid/foreign thinking signatures (common after /resume).
        // Retry once by stripping thinking blocks & thinking config from the request, and by disabling
        // the "-thinking" model variant if present.
        if status_code == 400
            && !retried_without_thinking
            && (error_text.contains("Invalid `signature`")
                || error_text.contains("thinking.signature: Field required")
                || error_text.contains("thinking.signature")
                // Handle "Expected `thinking` or `redacted_thinking`, but found `tool_use`" errors
                || error_text.contains("Expected `thinking`")
                || error_text.contains("Expected 'thinking'")
                || error_text.contains("must start with a thinking block")
                // Gemini 3 specific: thought_signature required for function calls with thinking mode
                || error_text.contains("thought_signature")
                || error_text.contains("thoughtSignature"))
        {
            retried_without_thinking = true;
            tracing::warn!("Upstream rejected thinking signature; retrying once with thinking stripped");

            // 1) Remove thinking config
            request_for_body.thinking = None;

            // 2) Remove thinking blocks from message history
            for msg in request_for_body.messages.iter_mut() {
                if let crate::proxy::mappers::claude::models::MessageContent::Array(blocks) = &mut msg.content {
                    blocks.retain(|b| !matches!(b, crate::proxy::mappers::claude::models::ContentBlock::Thinking { .. }));
                }
            }

            // 3) Prefer non-thinking Claude model variant on retry (best-effort)
            if request_for_body.model.contains("claude-") {
                let mut m = request_for_body.model.clone();
                m = m.replace("-thinking", "");
                // If it's a dated alias, fall back to a stable non-thinking id
                if m.contains("claude-sonnet-4-5-") {
                    m = "claude-sonnet-4-5".to_string();
                } else if m.contains("claude-opus-4-5-") || m.contains("claude-opus-4-") {
                    m = "claude-opus-4-5".to_string();
                }
                request_for_body.model = m;
            }

            continue;
        }

        // 只有 429 (限流), 403 (权限/地区限制) 和 401 (认证失效) 触发账号轮换
        if status_code == 429 || status_code == 403 || status_code == 401 {
            // 如果是 429 且标记为配额耗尽（明确），直接报错，避免穿透整个账号池
            if status_code == 429 && error_text.contains("QUOTA_EXHAUSTED") {
                error!("Claude Quota exhausted (429) on attempt {}/{}, stopping to protect pool.", attempt + 1, max_attempts);
                return (status, error_text).into_response();
            }

            tracing::warn!("Claude Upstream {} on attempt {}/{}, rotating account", status, attempt + 1, max_attempts);
            continue;
        }
        
        // 404 等由于模型配置或路径错误的 HTTP 异常，直接报错，不进行无效轮换
        error!("Claude Upstream non-retryable error {}: {}", status_code, error_text);
        return (status, error_text).into_response();
    }
    
    (StatusCode::TOO_MANY_REQUESTS, Json(json!({
        "type": "error",
        "error": {
            "type": "overloaded_error",
            "message": format!("All {} attempts failed. Last error: {}", max_attempts, last_error)
        }
    }))).into_response()
}

/// 列出可用模型
pub async fn handle_list_models() -> impl IntoResponse {
    Json(json!({
        "object": "list",
        "data": [
            {
                "id": "claude-sonnet-4-5",
                "object": "model",
                "created": 1706745600,
                "owned_by": "anthropic"
            },
            {
                "id": "claude-opus-4-5-thinking",
                "object": "model",
                "created": 1706745600,
                "owned_by": "anthropic"
            },
            {
                "id": "claude-3-5-sonnet-20241022",
                "object": "model",
                "created": 1706745600,
                "owned_by": "anthropic"
            }
        ]
    }))
}

/// 计算 tokens (占位符)
pub async fn handle_count_tokens(Json(_body): Json<Value>) -> impl IntoResponse {
    Json(json!({
        "input_tokens": 0,
        "output_tokens": 0
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_handle_list_models() {
        let response = handle_list_models().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
