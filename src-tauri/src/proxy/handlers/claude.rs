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
use tracing::{debug, error};

use crate::proxy::mappers::claude::{
    derive_session_id_for_request, transform_claude_request_in, transform_response,
    create_claude_sse_stream, ClaudeRequest, Metadata,
};
use crate::proxy::server::AppState;
use axum::http::HeaderMap;
use std::sync::atomic::Ordering;

const MAX_RETRY_ATTEMPTS: usize = 3;

/// 处理 Claude messages 请求
/// 
/// 处理 Chat 消息请求流程
pub async fn handle_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    // Decide whether this request should be handled by z.ai (Anthropic passthrough) or the existing Google flow.
    let zai = state.zai.read().await.clone();
    let zai_enabled = zai.enabled && !matches!(zai.dispatch_mode, crate::proxy::ZaiDispatchMode::Off);
    let google_accounts = state.token_manager.len();

    let use_zai = if !zai_enabled {
        false
    } else {
        match zai.dispatch_mode {
            crate::proxy::ZaiDispatchMode::Off => false,
            crate::proxy::ZaiDispatchMode::Exclusive => true,
            crate::proxy::ZaiDispatchMode::Fallback => google_accounts == 0,
            crate::proxy::ZaiDispatchMode::Pooled => {
                // Treat z.ai as exactly one extra slot in the pool.
                // No strict guarantees: it may get 0 requests if selection never hits.
                let total = google_accounts.saturating_add(1).max(1);
                let slot = state.provider_rr.fetch_add(1, Ordering::Relaxed) % total;
                slot == 0
            }
        }
    };

    if use_zai {
        return crate::proxy::providers::zai_anthropic::forward_anthropic_json(
            &state,
            axum::http::Method::POST,
            "/v1/messages",
            &headers,
            body,
        )
        .await;
    }

    let request: ClaudeRequest = match serde_json::from_value(body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "type": "error",
                    "error": {
                        "type": "invalid_request_error",
                        "message": format!("Invalid request body: {}", e)
                    }
                })),
            )
                .into_response();
        }
    };

    // Derive a stable session id to keep the conversation on the same account.
    // Also inject it into metadata so the upstream request uses the same sessionId.
    let mut request = request;
    let derived_session_id = derive_session_id_for_request(&request);
    if let Some(sid) = derived_session_id.as_ref() {
        let has_user_id = request
            .metadata
            .as_ref()
            .and_then(|m| m.user_id.as_ref())
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if !has_user_id {
            let mut md = request.metadata.take().unwrap_or(Metadata { user_id: None });
            md.user_id = Some(sid.clone());
            request.metadata = Some(md);
        }
    }

    // 生成随机 Trace ID 用户追踪
    let trace_id: String = rand::Rng::sample_iter(rand::thread_rng(), &rand::distributions::Alphanumeric)
        .take(6)
        .map(char::from)
        .collect::<String>().to_lowercase();
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
    
    
    crate::modules::logger::log_info(&format!("[{}] Received Claude request for model: {}, content_preview: {:.100}...", trace_id, request.model, latest_msg));
    
    // ===== 调试日志（默认关闭，避免影响性能） =====
    if tracing::enabled!(tracing::Level::DEBUG) {
        tracing::debug!("========== [{}] CLAUDE REQUEST DEBUG START ==========", trace_id);
        tracing::debug!("[{}] Model: {}", trace_id, request.model);
        tracing::debug!("[{}] Stream: {}", trace_id, request.stream);
        tracing::debug!("[{}] Max Tokens: {:?}", trace_id, request.max_tokens);
        tracing::debug!("[{}] Temperature: {:?}", trace_id, request.temperature);
        tracing::debug!("[{}] Message Count: {}", trace_id, request.messages.len());
        tracing::debug!("[{}] Has Tools: {}", trace_id, request.tools.is_some());
        tracing::debug!("[{}] Has Thinking Config: {}", trace_id, request.thinking.is_some());

        // 输出每一条消息的简要信息（避免打印完整内容）
        for (idx, msg) in request.messages.iter().enumerate() {
            let content_preview = match &msg.content {
                crate::proxy::mappers::claude::models::MessageContent::String(s) => {
                    if s.len() > 200 {
                        format!("{}... (total {} chars)", &s[..200], s.len())
                    } else {
                        s.clone()
                    }
                },
                crate::proxy::mappers::claude::models::MessageContent::Array(arr) => {
                    format!("[Array with {} blocks]", arr.len())
                }
            };
            tracing::debug!(
                "[{}] Message[{}] - Role: {}, Content: {}",
                trace_id,
                idx,
                msg.role,
                content_preview
            );
        }

        if let Ok(full_json) = serde_json::to_string_pretty(&request) {
            tracing::debug!("[{}] Full Claude Request JSON: {}", trace_id, full_json);
        }
        tracing::debug!("========== [{}] CLAUDE REQUEST DEBUG END ==========", trace_id);
    }

    // 1. 获取 会话 ID (已废弃基于内容的哈希，改用 TokenManager 内部的时间窗口锁定)
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
        // 将 Claude 工具转为 Value 数组以便探测联网
        let tools_val: Option<Vec<Value>> = request_for_body.tools.as_ref().map(|list| {
            list.iter().map(|t| serde_json::to_value(t).unwrap_or(json!({}))).collect()
        });

        let config = crate::proxy::mappers::common_utils::resolve_request_config(&request_for_body.model, &mapped_model, &tools_val);

        // 4. 获取 Token (使用准确的 request_type)
        // 关键：在重试尝试 (attempt > 0) 时，必须根据错误类型决定是否强制轮换账号
        let force_rotate_token = attempt > 0; 
        
        let (access_token, project_id, email) = match token_manager
            .get_token(&config.request_type, force_rotate_token, derived_session_id.as_deref())
            .await
        {
            Ok(t) => t,
            Err(e) => {
                let safe_message = if e.contains("invalid_grant") {
                    "OAuth refresh failed (invalid_grant): refresh_token likely revoked/expired; reauthorize account(s) to restore service.".to_string()
                } else {
                    e
                };
                 return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({
                        "type": "error",
                        "error": {
                            "type": "overloaded_error",
                            "message": format!("No available accounts: {}", safe_message)
                        }
                    }))
                ).into_response();
            }
        };

        tracing::info!("Using account: {} for request (type: {})", email, config.request_type);
        
        
        // ===== 【优化】后台任务智能检测与降级 =====
        // 使用新的检测系统，支持 5 大类关键词和多 Flash 模型策略
        let background_task_type = detect_background_task_type(&request_for_body);
        
        // 传递映射后的模型名
        let mut request_with_mapped = request_for_body.clone();

        if let Some(task_type) = background_task_type {
            // 检测到后台任务，强制降级到 Flash 模型
            let downgrade_model = select_background_model(task_type);
            
            tracing::warn!(
                "[{}][AUTO] 检测到后台任务 (类型: {:?})，强制降级: {} -> {}",
                trace_id,
                task_type,
                mapped_model,
                downgrade_model
            );
            
            // 覆盖用户自定义映射
            mapped_model = downgrade_model.to_string();
            
            // 后台任务净化：
            // 1. 移除工具定义（后台任务不需要工具）
            request_with_mapped.tools = None;
            
            // 2. 移除 Thinking 配置（Flash 模型不支持）
            request_with_mapped.thinking = None;
            
            // 3. 清理历史消息中的 Thinking Block，防止 Invalid Argument
            for msg in request_with_mapped.messages.iter_mut() {
                if let crate::proxy::mappers::claude::models::MessageContent::Array(blocks) = &mut msg.content {
                    blocks.retain(|b| !matches!(b, 
                        crate::proxy::mappers::claude::models::ContentBlock::Thinking { .. } |
                        crate::proxy::mappers::claude::models::ContentBlock::RedactedThinking { .. }
                    ));
                }
            }
        } else {
            // 真实用户请求，保持原映射
            tracing::warn!(
                "[{}][USER] 用户交互请求，保持映射: {}",
                trace_id,
                mapped_model
            );
        }

        
        request_with_mapped.model = mapped_model;

        // 生成 Trace ID (简单用时间戳后缀)
        // let _trace_id = format!("req_{}", chrono::Utc::now().timestamp_subsec_millis());

        let gemini_body = match transform_claude_request_in(&request_with_mapped, &project_id) {
            Ok(b) => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    if let Ok(pretty) = serde_json::to_string_pretty(&b) {
                        tracing::debug!("[{}] Transformed Gemini Body: {}", trace_id, pretty);
                    }
                }
                b
            },
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
                let claude_stream = create_claude_sse_stream(gemini_stream, trace_id);

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

                // [Optimization] 记录闭环日志：消耗情况
                tracing::info!(
                    "[{}] Request finished. Model: {}, Tokens: In {}, Out {}", 
                    trace_id, 
                    request_with_mapped.model, 
                    claude_response.usage.input_tokens, 
                    claude_response.usage.output_tokens
                );

                return Json(claude_response).into_response();
            }
        }
        
        // 处理错误
        let error_text = response.text().await.unwrap_or_else(|_| format!("HTTP {}", status));
        last_error = format!("HTTP {}: {}", status, error_text);
        tracing::error!("[{}] Upstream Error Response: {}", trace_id, error_text);
        
        let status_code = status.as_u16();
        
        // 429 handling: mark the current account as temporarily unavailable and retry.
        // TokenManager will skip cooled-down accounts and will wait (up to a limit) when all accounts are limited.
        if status_code == 429 {
            if error_text.contains("QUOTA_EXHAUSTED") {
                error!(
                    "Claude Quota exhausted (429) on attempt {}/{}, stopping to protect pool.",
                    attempt + 1,
                    max_attempts
                );
                return (status, error_text).into_response();
            }
            if let Some(delay_ms) = crate::proxy::upstream::retry::parse_retry_delay(&error_text) {
                let actual_delay = delay_ms.saturating_add(200);
                token_manager.mark_rate_limited_by_email(&config.request_type, &email, actual_delay);
                tracing::warn!(
                    "Claude Upstream 429 on attempt {}/{}, cooling down account {} for {}ms",
                    attempt + 1,
                    max_attempts,
                    email,
                    actual_delay
                );
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
                || error_text.contains("thinking.thinking: Field required")
                || error_text.contains("thinking.signature")
                || error_text.contains("thinking.thinking"))
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
pub async fn handle_list_models(State(state): State<AppState>) -> impl IntoResponse {
    use crate::proxy::common::model_mapping::get_all_dynamic_models;

    let model_ids = get_all_dynamic_models(
        &state.openai_mapping,
        &state.custom_mapping,
        &state.anthropic_mapping,
    ).await;

    let data: Vec<_> = model_ids.into_iter().map(|id| {
        json!({
            "id": id,
            "object": "model",
            "created": 1706745600,
            "owned_by": "antigravity"
        })
    }).collect();

    Json(json!({
        "object": "list",
        "data": data
    }))
}

/// 计算 tokens (占位符)
pub async fn handle_count_tokens(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let zai = state.zai.read().await.clone();
    let zai_enabled = zai.enabled && !matches!(zai.dispatch_mode, crate::proxy::ZaiDispatchMode::Off);

    if zai_enabled {
        return crate::proxy::providers::zai_anthropic::forward_anthropic_json(
            &state,
            axum::http::Method::POST,
            "/v1/messages/count_tokens",
            &headers,
            body,
        )
        .await;
    }

    Json(json!({
        "input_tokens": 0,
        "output_tokens": 0
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_handle_list_models() {
        let token_manager = Arc::new(crate::proxy::TokenManager::new(std::env::temp_dir()));

        let state = AppState {
            token_manager,
            anthropic_mapping: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            openai_mapping: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            custom_mapping: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            request_timeout: 0,
            thought_signature_map: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            upstream_proxy: Arc::new(tokio::sync::RwLock::new(
                crate::proxy::config::UpstreamProxyConfig::default(),
            )),
            upstream: Arc::new(crate::proxy::upstream::client::UpstreamClient::new(None)),
            zai: Arc::new(tokio::sync::RwLock::new(crate::proxy::ZaiConfig::default())),
            provider_rr: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            zai_vision_mcp: Arc::new(crate::proxy::zai_vision_mcp::ZaiVisionMcpState::new()),
            monitor: Arc::new(crate::proxy::monitor::ProxyMonitor::new(10, None)),
        };

        let response = handle_list_models(State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }
}

// ===== 后台任务检测辅助函数 =====

/// 后台任务类型
#[derive(Debug, Clone, Copy, PartialEq)]
enum BackgroundTaskType {
    TitleGeneration,      // 标题生成
    SimpleSummary,        // 简单摘要
    ContextCompression,   // 上下文压缩
    PromptSuggestion,     // 提示建议
    SystemMessage,        // 系统消息
    EnvironmentProbe,     // 环境探测
}

/// 标题生成关键词
const TITLE_KEYWORDS: &[&str] = &[
    "write a 5-10 word title",
    "Please write a 5-10 word title",
    "Respond with the title",
    "Generate a title for",
    "Create a brief title",
    "title for the conversation",
    "conversation title",
    "生成标题",
    "为对话起个标题",
];

/// 摘要生成关键词
const SUMMARY_KEYWORDS: &[&str] = &[
    "Summarize this coding conversation",
    "Summarize the conversation",
    "Concise summary",
    "in under 50 characters",
    "compress the context",
    "Provide a concise summary",
    "condense the previous messages",
    "shorten the conversation history",
    "extract key points from",
];

/// 建议生成关键词
const SUGGESTION_KEYWORDS: &[&str] = &[
    "prompt suggestion generator",
    "suggest next prompts",
    "what should I ask next",
    "generate follow-up questions",
    "recommend next steps",
    "possible next actions",
];

/// 系统消息关键词
const SYSTEM_KEYWORDS: &[&str] = &[
    "Warmup",
    "<system-reminder>",
    "Caveat: The messages below were generated",
    "This is a system message",
];

/// 环境探测关键词
const PROBE_KEYWORDS: &[&str] = &[
    "check current directory",
    "list available tools",
    "verify environment",
    "test connection",
];

/// 检测后台任务并返回任务类型
fn detect_background_task_type(request: &ClaudeRequest) -> Option<BackgroundTaskType> {
    let last_user_msg = extract_last_user_message_for_detection(request)?;
    let preview = last_user_msg.chars().take(500).collect::<String>();
    
    // 长度过滤：后台任务通常不超过 800 字符
    if last_user_msg.len() > 800 {
        return None;
    }
    
    // 按优先级匹配
    if matches_keywords(&preview, SYSTEM_KEYWORDS) {
        return Some(BackgroundTaskType::SystemMessage);
    }
    
    if matches_keywords(&preview, TITLE_KEYWORDS) {
        return Some(BackgroundTaskType::TitleGeneration);
    }
    
    if matches_keywords(&preview, SUMMARY_KEYWORDS) {
        if preview.contains("in under 50 characters") {
            return Some(BackgroundTaskType::SimpleSummary);
        }
        return Some(BackgroundTaskType::ContextCompression);
    }
    
    if matches_keywords(&preview, SUGGESTION_KEYWORDS) {
        return Some(BackgroundTaskType::PromptSuggestion);
    }
    
    if matches_keywords(&preview, PROBE_KEYWORDS) {
        return Some(BackgroundTaskType::EnvironmentProbe);
    }
    
    None
}

/// 辅助函数：关键词匹配
fn matches_keywords(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|kw| text.contains(kw))
}

/// 辅助函数：提取最后一条用户消息（用于检测）
fn extract_last_user_message_for_detection(request: &ClaudeRequest) -> Option<String> {
    request.messages.iter().rev()
        .filter(|m| m.role == "user")
        .find_map(|m| {
            let content = match &m.content {
                crate::proxy::mappers::claude::models::MessageContent::String(s) => s.to_string(),
                crate::proxy::mappers::claude::models::MessageContent::Array(arr) => {
                    arr.iter()
                        .filter_map(|block| match block {
                            crate::proxy::mappers::claude::models::ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                }
            };
            
            if content.trim().is_empty() 
                || content.starts_with("Warmup") 
                || content.contains("<system-reminder>") 
            {
                None 
            } else {
                Some(content)
            }
        })
}

/// 根据后台任务类型选择合适的模型
fn select_background_model(task_type: BackgroundTaskType) -> &'static str {
    match task_type {
        BackgroundTaskType::TitleGeneration => "gemini-2.0-flash-exp",  // 极简任务
        BackgroundTaskType::SimpleSummary => "gemini-2.0-flash-exp",    // 简单摘要
        BackgroundTaskType::SystemMessage => "gemini-2.0-flash-exp",    // 系统消息
        BackgroundTaskType::PromptSuggestion => "gemini-2.0-flash-exp", // 建议生成
        BackgroundTaskType::EnvironmentProbe => "gemini-2.0-flash-exp", // 环境探测
        BackgroundTaskType::ContextCompression => "gemini-2.5-flash",   // 复杂压缩
    }
}
