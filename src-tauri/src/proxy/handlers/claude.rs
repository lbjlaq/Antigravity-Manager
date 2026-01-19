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
use tracing::{debug, error, info};

use crate::proxy::mappers::claude::{
    transform_claude_request_in, transform_response, create_claude_sse_stream, ClaudeRequest,
    filter_invalid_thinking_blocks_with_family, close_tool_loop_for_thinking,
    clean_cache_control_from_messages, merge_consecutive_messages,
};
use crate::proxy::server::AppState;
use crate::proxy::mappers::context_manager::{ContextManager, PurificationStrategy};
use axum::http::HeaderMap;
use std::sync::atomic::Ordering;

const MAX_RETRY_ATTEMPTS: usize = 3;
const MIN_SIGNATURE_LENGTH: usize = 10;  // 最小有效签名长度

// ===== Model Constants for Background Tasks =====
// These can be adjusted for performance/cost optimization
const BACKGROUND_MODEL_LITE: &str = "gemini-2.5-flash-lite";  // For simple/lightweight tasks
const BACKGROUND_MODEL_STANDARD: &str = "gemini-2.5-flash";   // For complex background tasks

// ===== Jitter Configuration (REMOVED) =====
// Jitter was causing connection instability, reverted to fixed delays
// const JITTER_FACTOR: f64 = 0.2;


// ===== 统一退避策略模块 =====

// [REMOVED] apply_jitter function
// Jitter logic removed to restore stability (v3.3.16 fix)

/// 重试策略枚举
#[derive(Debug, Clone)]
enum RetryStrategy {
    /// 不重试，直接返回错误
    NoRetry,
    /// 固定延迟
    FixedDelay(Duration),
    /// 线性退避：base_ms * (attempt + 1)
    LinearBackoff { base_ms: u64 },
    /// 指数退避：base_ms * 2^attempt，上限 max_ms
    ExponentialBackoff { base_ms: u64, max_ms: u64 },
}

/// 根据错误状态码和错误信息确定重试策略
fn determine_retry_strategy(
    status_code: u16,
    error_text: &str,
    retried_without_thinking: bool,
) -> RetryStrategy {
    match status_code {
        // 400 错误：Thinking 签名失败
        400 if !retried_without_thinking
            && (error_text.contains("Invalid `signature`")
                || error_text.contains("thinking.signature")
                || error_text.contains("thinking.thinking")) =>
        {
            // 固定 200ms 延迟后重试
            RetryStrategy::FixedDelay(Duration::from_millis(200))
        }

        // 429 限流错误
        429 => {
            // 优先使用服务端返回的 Retry-After
            if let Some(delay_ms) = crate::proxy::upstream::retry::parse_retry_delay(error_text) {
                let actual_delay = delay_ms.saturating_add(200).min(10_000);
                RetryStrategy::FixedDelay(Duration::from_millis(actual_delay))
            } else {
                // 否则使用线性退避：1s, 2s, 3s
                RetryStrategy::LinearBackoff { base_ms: 1000 }
            }
        }

        // 503 服务不可用 / 529 服务器过载
        503 | 529 => {
            // 指数退避：1s, 2s, 4s, 8s
            RetryStrategy::ExponentialBackoff {
                base_ms: 1000,
                max_ms: 8000,
            }
        }

        // 500 服务器内部错误
        500 => {
            // 线性退避：500ms, 1s, 1.5s
            RetryStrategy::LinearBackoff { base_ms: 500 }
        }

        // 401/403 认证/权限错误：可重试（轮换账号）
        401 | 403 => RetryStrategy::FixedDelay(Duration::from_millis(100)),

        // 其他错误：不重试
        _ => RetryStrategy::NoRetry,
    }
}

/// 执行退避策略并返回是否应该继续重试
async fn apply_retry_strategy(
    strategy: RetryStrategy,
    attempt: usize,
    status_code: u16,
    trace_id: &str,
) -> bool {
    match strategy {
        RetryStrategy::NoRetry => {
            debug!("[{}] Non-retryable error {}, stopping", trace_id, status_code);
            false
        }

        RetryStrategy::FixedDelay(duration) => {
            let base_ms = duration.as_millis() as u64;
            info!(
                "[{}] ⏱️  Retry with fixed delay: status={}, attempt={}/{}, base={}ms",
                trace_id,
                status_code,
                attempt + 1,
                MAX_RETRY_ATTEMPTS,
                base_ms
            );
            sleep(duration).await;
            true
        }

        RetryStrategy::LinearBackoff { base_ms } => {
            let calculated_ms = base_ms * (attempt as u64 + 1);
            info!(
                "[{}] ⏱️  Retry with linear backoff: status={}, attempt={}/{}, base={}ms",
                trace_id,
                status_code,
                attempt + 1,
                MAX_RETRY_ATTEMPTS,
                calculated_ms
            );
            sleep(Duration::from_millis(calculated_ms)).await;
            true
        }

        RetryStrategy::ExponentialBackoff { base_ms, max_ms } => {
            let calculated_ms = (base_ms * 2_u64.pow(attempt as u32)).min(max_ms);
            info!(
                "[{}] ⏱️  Retry with exponential backoff: status={}, attempt={}/{}, base={}ms",
                trace_id,
                status_code,
                attempt + 1,
                MAX_RETRY_ATTEMPTS,
                calculated_ms
            );
            sleep(Duration::from_millis(calculated_ms)).await;
            true
        }
    }
}

/// 判断是否应该轮换账号
fn should_rotate_account(status_code: u16) -> bool {
    match status_code {
        // 这些错误是账号级别的，需要轮换
        429 | 401 | 403 | 500 => true,
        // 这些错误是服务端级别的，轮换账号无意义
        400 | 503 | 529 => false,
        // 其他错误默认不轮换
        _ => false,
    }
}

// ===== 退避策略模块结束 =====

/// 处理 Claude messages 请求
/// 
/// 处理 Chat 消息请求流程
pub async fn handle_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    tracing::debug!("handle_messages called. Body JSON len: {}", body.to_string().len());
    
    // 生成随机 Trace ID 用户追踪
    let trace_id: String = rand::Rng::sample_iter(rand::thread_rng(), &rand::distributions::Alphanumeric)
        .take(6)
        .map(char::from)
        .collect::<String>().to_lowercase();
        
    // Decide whether this request should be handled by z.ai (Anthropic passthrough) or the existing Google flow.
    let zai = state.zai.read().await.clone();
    let zai_enabled = zai.enabled && !matches!(zai.dispatch_mode, crate::proxy::ZaiDispatchMode::Off);
    let google_accounts = state.token_manager.len();

    // [CRITICAL REFACTOR] 优先解析请求以获取模型信息(用于智能兜底判断)
    let mut request: crate::proxy::mappers::claude::models::ClaudeRequest = match serde_json::from_value(body) {
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
                }))
            ).into_response();
        }
    };

    // [Issue #703 Fix] 智能兜底判断:需要归一化模型名用于配额保护检查
    let normalized_model = crate::proxy::common::model_mapping::normalize_to_standard_id(&request.model)
        .unwrap_or_else(|| request.model.clone());

    let use_zai = if !zai_enabled {
        false
    } else {
        match zai.dispatch_mode {
            crate::proxy::ZaiDispatchMode::Off => false,
            crate::proxy::ZaiDispatchMode::Exclusive => true,
            crate::proxy::ZaiDispatchMode::Fallback => {
                if google_accounts == 0 {
                    // 没有 Google 账号,使用兜底
                    tracing::info!("[{}] No Google accounts available, using fallback provider", trace_id);
                    true
                } else {
                    // [Issue #703 Fix] 智能判断:检查是否有可用的 Google 账号
                    let has_available = state.token_manager.has_available_account("claude", &normalized_model).await;
                    if !has_available {
                        tracing::info!(
                            "[{}] All Google accounts unavailable (rate-limited or quota-protected for {}), using fallback provider",
                            trace_id,
                            request.model
                        );
                    }
                    !has_available
                }
            }
            crate::proxy::ZaiDispatchMode::Pooled => {
                // Treat z.ai as exactly one extra slot in the pool.
                // No strict guarantees: it may get 0 requests if selection never hits.
                let total = google_accounts.saturating_add(1).max(1);
                let slot = state.provider_rr.fetch_add(1, Ordering::Relaxed) % total;
                slot == 0
            }
        }
    };

    // [CRITICAL FIX] 预先清理所有消息中的 cache_control 字段 (Issue #744)
    // 必须在序列化之前处理，以确保 z.ai 和 Google Flow 都不受历史消息缓存标记干扰
    clean_cache_control_from_messages(&mut request.messages);

    // [FIX #813] 合并连续的同角色消息 (Consecutive User Messages)
    // 这对于 z.ai (Anthropic 直接转发) 路径至关重要，因为原始结构必须符合协议
    merge_consecutive_messages(&mut request.messages);

    // Get model family for signature validation
    let target_family = if use_zai {
        Some("claude")
    } else {
        let mapped_model = crate::proxy::common::model_mapping::map_claude_model_to_gemini(&request.model);
        if mapped_model.contains("gemini") {
            Some("gemini")
        } else {
            Some("claude")
        }
    };

    // [CRITICAL FIX] 过滤并修复 Thinking 块签名 (Enhanced with family check)
    filter_invalid_thinking_blocks_with_family(&mut request.messages, target_family);

    // [New] Recover from broken tool loops (where signatures were stripped)
    // This prevents "Assistant message must start with thinking" errors by closing the loop with synthetic messages
    if state.experimental.read().await.enable_tool_loop_recovery {
        close_tool_loop_for_thinking(&mut request.messages);
    }

    // ===== [Issue #467 Fix] 拦截 Claude Code Warmup 请求 =====
    // Claude Code 会每 10 秒发送一次 warmup 请求来保持连接热身，
    // 这些请求会消耗大量配额。检测到 warmup 请求后直接返回模拟响应。
    if is_warmup_request(&request) {
        tracing::info!(
            "[{}] 🔥 拦截 Warmup 请求，返回模拟响应（节省配额）",
            trace_id
        );
        return create_warmup_response(&request, request.stream);
    }

    if use_zai {
        // 重新序列化修复后的请求体
        let new_body = match serde_json::to_value(&request) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("Failed to serialize fixed request for z.ai: {}", e);
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };

        return crate::proxy::providers::zai_anthropic::forward_anthropic_json(
            &state,
            axum::http::Method::POST,
            "/v1/messages",
            &headers,
            new_body,
        )
        .await;
    }
    
    // Google Flow 继续使用 request 对象
    // (后续代码不需要再次 filter_invalid_thinking_blocks)
    
    // [NEW] 获取上下文缩放配置
    let scaling_enabled = state.experimental.read().await.enable_usage_scaling;

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
    
    
    // INFO 级别: 简洁的一行摘要
    info!(
        "[{}] Claude Request | Model: {} | Stream: {} | Messages: {} | Tools: {}",
        trace_id,
        request.model,
        request.stream,
        request.messages.len(),
        request.tools.is_some()
    );
    
    // DEBUG 级别: 详细的调试信息
    debug!("========== [{}] CLAUDE REQUEST DEBUG START ==========", trace_id);
    debug!("[{}] Model: {}", trace_id, request.model);
    debug!("[{}] Stream: {}", trace_id, request.stream);
    debug!("[{}] Max Tokens: {:?}", trace_id, request.max_tokens);
    debug!("[{}] Temperature: {:?}", trace_id, request.temperature);
    debug!("[{}] Message Count: {}", trace_id, request.messages.len());
    debug!("[{}] Has Tools: {}", trace_id, request.tools.is_some());
    debug!("[{}] Has Thinking Config: {}", trace_id, request.thinking.is_some());
    debug!("[{}] Content Preview: {:.100}...", trace_id, latest_msg);
    
    // 输出每一条消息的详细信息
    for (idx, msg) in request.messages.iter().enumerate() {
        let content_preview = match &msg.content {
            crate::proxy::mappers::claude::models::MessageContent::String(s) => {
                let char_count = s.chars().count();
                if char_count > 200 {
                    // 【修复】使用 chars().take() 安全截取，避免 UTF-8 字符边界 panic
                    let preview: String = s.chars().take(200).collect();
                    format!("{}... (total {} chars)", preview, char_count)
                } else {
                    s.clone()
                }
            },
            crate::proxy::mappers::claude::models::MessageContent::Array(arr) => {
                format!("[Array with {} blocks]", arr.len())
            }
        };
        debug!("[{}] Message[{}] - Role: {}, Content: {}", 
            trace_id, idx, msg.role, content_preview);
    }
    
    debug!("[{}] Full Claude Request JSON: {}", trace_id, serde_json::to_string_pretty(&request).unwrap_or_default());
    debug!("========== [{}] CLAUDE REQUEST DEBUG END ==========", trace_id);

    // 1. 获取 会话 ID (已废弃基于内容的哈希，改用 TokenManager 内部的时间窗口锁定)
    let _session_id: Option<&str> = None;

    // 2. 获取 UpstreamClient
    let upstream = state.upstream.clone();
    
    // 3. 准备闭包
    let mut request_for_body = request.clone();
    let token_manager = state.token_manager;
    
    let pool_size = token_manager.len();
    // [FIX] Ensure max_attempts is at least 2 to allow for internal retries (e.g. stripping signatures)
    // even if the user has only 1 account.
    let max_attempts = MAX_RETRY_ATTEMPTS.min(pool_size.saturating_add(1)).max(2);

    let mut last_error = String::new();
    let mut retried_without_thinking = false;
    let mut last_email: Option<String> = None;
    
    for attempt in 0..max_attempts {
        // 2. 模型路由解析
        let mut mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
            &request_for_body.model,
            &*state.custom_mapping.read().await,
        );
        
        // 将 Claude 工具转为 Value 数组以便探测联网
        let tools_val: Option<Vec<Value>> = request_for_body.tools.as_ref().map(|list| {
            list.iter().map(|t| serde_json::to_value(t).unwrap_or(json!({}))).collect()
        });

        let config = crate::proxy::mappers::common_utils::resolve_request_config(&request_for_body.model, &mapped_model, &tools_val);

        // 0. 尝试提取 session_id 用于粘性调度 (Phase 2/3)
        // 使用 SessionManager 生成稳定的会话指纹
        let session_id_str = crate::proxy::session_manager::SessionManager::extract_session_id(&request_for_body);
        let session_id = Some(session_id_str.as_str());

        let force_rotate_token = attempt > 0;
        let (access_token, project_id, email) = match token_manager.get_token(&config.request_type, force_rotate_token, session_id, &config.final_model).await {
            Ok(t) => t,
            Err(e) => {
                // ===== [模型轮询] 当主模型账号全部不可用时，尝试备选模型 =====
                let polling_enabled = state.experimental.read().await.enable_model_polling;
                
                if polling_enabled {
                    // 触发主模型熔断（5分钟）
                    token_manager.trip_model_circuit_breaker(&request_for_body.model, 300);
                    
                    // 获取备选模型列表
                    let fallback_models = crate::proxy::common::model_fallback::get_fallback_models(&request_for_body.model);
                    
                    tracing::info!(
                        "[{}] 🔄 主模型 {} 不可用，启动模型轮询，尝试备选模型: {:?}",
                        trace_id, request_for_body.model, fallback_models
                    );
                    
                    // 尝试每个备选模型
                    for fallback_model in fallback_models {
                        // 检查备选模型是否也被熔断
                        if token_manager.is_model_circuit_broken(fallback_model) {
                            tracing::debug!("[{}] 备选模型 {} 处于熔断状态，跳过", trace_id, fallback_model);
                            continue;
                        }
                        
                        // 创建一个使用备选模型的请求副本
                        let mut fallback_request = request_for_body.clone();
                        fallback_request.model = fallback_model.to_string();
                        
                        // 备选模型（特别是 Gemini）可能不支持 Thinking，需要清理
                        if fallback_model.contains("flash") || fallback_model.contains("gemini") {
                            fallback_request.thinking = None;
                            // 清理历史 Thinking 块
                            for msg in fallback_request.messages.iter_mut() {
                                if let crate::proxy::mappers::claude::models::MessageContent::Array(blocks) = &mut msg.content {
                                    blocks.retain(|b| !matches!(b, 
                                        crate::proxy::mappers::claude::models::ContentBlock::Thinking { .. } |
                                        crate::proxy::mappers::claude::models::ContentBlock::RedactedThinking { .. }
                                    ));
                                }
                            }
                        }
                        
                        // 重新获取配置和尝试获取 token
                        let fallback_config = crate::proxy::mappers::common_utils::resolve_request_config(
                            &fallback_request.model, 
                            &fallback_request.model, 
                            &None
                        );
                        
                        match token_manager.get_token(&fallback_config.request_type, false, session_id, &fallback_config.final_model).await {
                            Ok((fb_token, fb_project, fb_email)) => {
                                tracing::info!(
                                    "[{}] ✅ 轮询成功！使用备选模型: {} (账号: {})",
                                    trace_id, fallback_model, fb_email
                                );
                                
                                // 直接处理备选模型请求
                                let fallback_mapped_model = fallback_model.to_string();
                                let gemini_body = match transform_claude_request_in(&fallback_request, &fb_project, false) {
                                    Ok(b) => b,
                                    Err(transform_err) => {
                                        tracing::error!("[{}] 备选模型转换失败: {}", trace_id, transform_err);
                                        continue;
                                    }
                                };
                                
                                let client_wants_stream = request.stream;
                                let method = "streamGenerateContent";
                                let query = Some("alt=sse");
                                let extra_headers = std::collections::HashMap::new();
                                
                                // 调用上游
                                match upstream.call_v1_internal_with_headers(method, &fb_token, gemini_body, query, extra_headers).await {
                                    Ok(response) => {
                                        if response.status().is_success() {
                                            token_manager.mark_account_success(&fb_email);
                                            
                                            let context_limit = crate::proxy::mappers::claude::utils::get_context_limit_for_model(&fallback_mapped_model);
                                            let stream = response.bytes_stream();
                                            let gemini_stream = Box::pin(stream);
                                            let mut claude_stream = create_claude_sse_stream(
                                                gemini_stream, 
                                                trace_id.clone(), 
                                                fb_email.clone(),
                                                Some(session_id_str.clone()),
                                                scaling_enabled,
                                                context_limit
                                            );
                                            
                                            let first_chunk = claude_stream.next().await;
                                            
                                            match first_chunk {
                                                Some(Ok(bytes)) if !bytes.is_empty() => {
                                                    let stream_rest = claude_stream;
                                                    let combined_stream = Box::pin(
                                                        futures::stream::once(async move { Ok(bytes) })
                                                            .chain(stream_rest.map(|result| -> Result<Bytes, std::io::Error> {
                                                                match result {
                                                                    Ok(b) => Ok(b),
                                                                    Err(e) => Ok(Bytes::from(format!("data: {{\"error\":\"{}\"}}\n\n", e))),
                                                                }
                                                            }))
                                                    );
                                                    
                                                    if client_wants_stream {
                                                        return Response::builder()
                                                            .status(StatusCode::OK)
                                                            .header(header::CONTENT_TYPE, "text/event-stream")
                                                            .header(header::CACHE_CONTROL, "no-cache")
                                                            .header(header::CONNECTION, "keep-alive")
                                                            .header("X-Account-Email", &fb_email)
                                                            .header("X-Mapped-Model", &fallback_mapped_model)
                                                            .header("X-Fallback-Model", "true")
                                                            .body(Body::from_stream(combined_stream))
                                                            .unwrap();
                                                    } else {
                                                        use crate::proxy::mappers::claude::collect_stream_to_json;
                                                        match collect_stream_to_json(combined_stream).await {
                                                            Ok(full_response) => {
                                                                return Response::builder()
                                                                    .status(StatusCode::OK)
                                                                    .header(header::CONTENT_TYPE, "application/json")
                                                                    .header("X-Account-Email", &fb_email)
                                                                    .header("X-Mapped-Model", &fallback_mapped_model)
                                                                    .header("X-Fallback-Model", "true")
                                                                    .body(Body::from(serde_json::to_string(&full_response).unwrap()))
                                                                    .unwrap();
                                                            }
                                                            Err(collect_err) => {
                                                                tracing::warn!("[{}] 备选模型响应收集失败: {}", trace_id, collect_err);
                                                                continue;
                                                            }
                                                        }
                                                    }
                                                }
                                                _ => {
                                                    tracing::warn!("[{}] 备选模型 {} 返回空响应", trace_id, fallback_model);
                                                    continue;
                                                }
                                            }
                                        } else {
                                            let status_code = response.status().as_u16();
                                            tracing::warn!("[{}] 备选模型 {} 请求失败: HTTP {}", trace_id, fallback_model, status_code);
                                            if status_code == 429 || status_code == 503 {
                                                token_manager.trip_model_circuit_breaker(fallback_model, 300);
                                            }
                                            continue;
                                        }
                                    }
                                    Err(upstream_err) => {
                                        tracing::warn!("[{}] 备选模型 {} 上游调用失败: {}", trace_id, fallback_model, upstream_err);
                                        continue;
                                    }
                                }
                            }
                            Err(_) => {
                                tracing::debug!("[{}] 备选模型 {} 也无可用账号，继续尝试下一个", trace_id, fallback_model);
                                token_manager.trip_model_circuit_breaker(fallback_model, 300);
                                continue;
                            }
                        }
                    }
                    
                    // 所有备选模型都失败，尝试智谱兜底（需要 z.ai 配置）
                    let zhipu_models = crate::proxy::common::model_fallback::get_zhipu_fallback_models();
                    tracing::info!("[{}] 所有备选模型不可用，智谱兜底: {:?}（需 z.ai 配置）", trace_id, zhipu_models);
                }
                
                // 原有错误处理
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

        last_email = Some(email.clone());
        info!("✓ Using account: {} (type: {})", email, config.request_type);
        
        
        // ===== 【优化】后台任务智能检测与降级 =====
        // 使用新的检测系统，支持 5 大类关键词和多 Flash 模型策略
        let background_task_type = detect_background_task_type(&request_for_body);
        
        // 传递映射后的模型名
        let mut request_with_mapped = request_for_body.clone();

        if let Some(task_type) = background_task_type {
            // 检测到后台任务,强制降级到 Flash 模型
            let downgrade_model = select_background_model(task_type);
            
            info!(
                "[{}][AUTO] 检测到后台任务 (类型: {:?}),强制降级: {} -> {}",
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
        }

        // ===== [Context Purification] Dynamic Thinking Stripping (Issue #PromptTooLong) =====
        // 对 Pro/Flash 模型进行差异化的上下文管理
        let mut is_purified = false;
        if !retried_without_thinking {
            // 1. 确定上下文限制 (Flash: ~1M, Pro: ~2M)
            // Conservatively use 900k for Flash and 1.8M for Pro to check pressure
            let context_limit = if mapped_model.contains("flash") {
                1_000_000
            } else {
                2_000_000
            };

            // 2. 估算当前用量
            let estimated_usage = ContextManager::estimate_token_usage(&request_with_mapped);
            let usage_ratio = estimated_usage as f32 / context_limit as f32;

            // 3. 确定清洗策略
            // > 90%: 激进剥离 (Aggressive) - 移除所有历史 Thinking
            // > 60%: 柔性剥离 (Soft) - 仅保留最近 2 轮 Thinking
            // < 60%: 不处理
            // 3. 确定清洗策略
            // > 90%: 激进剥离 (Aggressive) - 移除所有历史 Thinking
            // > 60%: 柔性剥离 (Soft) - 仅保留最近 2 轮 Thinking
            // < 60%: 不处理
            let strategy = if usage_ratio > 0.9 {
                PurificationStrategy::Aggressive
            } else if usage_ratio > 0.6 {
                PurificationStrategy::Soft
            } else {
                PurificationStrategy::None
            };
            
            // 4. 执行清洗
            if strategy != PurificationStrategy::None {
                info!(
                    "[{}] [ContextManager] Context pressure: {:.1}% ({} / {}), Strategy: {:?} => Purifying history", 
                    trace_id, usage_ratio * 100.0, estimated_usage, context_limit, strategy
                );
                
                if ContextManager::purify_history(&mut request_with_mapped.messages, strategy) {
                    is_purified = true;
                    debug!("[{}] History purified successfully", trace_id);
                }
            }
        }

        request_with_mapped.model = mapped_model;

        // 生成 Trace ID (简单用时间戳后缀)
        // let _trace_id = format!("req_{}", chrono::Utc::now().timestamp_subsec_millis());

        let gemini_body = match transform_claude_request_in(&request_with_mapped, &project_id, retried_without_thinking) {
            Ok(b) => {
                debug!("[{}] Transformed Gemini Body: {}", trace_id, serde_json::to_string_pretty(&b).unwrap_or_default());
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
        
    // 4. 上游调用 - 自动转换逻辑
    let client_wants_stream = request.stream;
    // [AUTO-CONVERSION] 非 Stream 请求自动转换为 Stream 以享受更宽松的配额
    let force_stream_internally = !client_wants_stream;
    let actual_stream = client_wants_stream || force_stream_internally;
    
    if force_stream_internally {
        info!("[{}] 🔄 Auto-converting non-stream request to stream for better quota", trace_id);
    }
    
    let method = if actual_stream { "streamGenerateContent" } else { "generateContent" };
    let query = if actual_stream { Some("alt=sse") } else { None };
        // [FIX #765] Prepare Beta Headers for Thinking + Tools
        let mut extra_headers = std::collections::HashMap::new();
        if request_with_mapped.thinking.is_some() && request_with_mapped.tools.is_some() {
            extra_headers.insert("anthropic-beta".to_string(), "interleaved-thinking-2025-05-14".to_string());
            tracing::debug!("[{}] Added Beta Header: interleaved-thinking-2025-05-14", trace_id);
        }

        // 5. 上游调用
        let response = match upstream
            .call_v1_internal_with_headers(method, &access_token, gemini_body, query, extra_headers.clone())
            .await {
            Ok(r) => r,
            Err(e) => {
                last_error = e.clone();
                debug!("Request failed on attempt {}/{}: {}", attempt + 1, max_attempts, e);
                continue;
            }
        };
        
        let status = response.status();
        
        // 成功
        if status.is_success() {
            // [智能限流] 请求成功，重置该账号的连续失败计数
            token_manager.mark_account_success(&email);
            
                // Determine context limit based on model
                let context_limit = crate::proxy::mappers::claude::utils::get_context_limit_for_model(&request_with_mapped.model);

            // 处理流式响应
            if actual_stream {
                let stream = response.bytes_stream();
                let gemini_stream = Box::pin(stream);


                // [FIX #530/#529/#859] Enhanced Peek logic to handle heartbeats and slow start
                // We must pre-read until we find a MEANINGFUL content block (like message_start).
                // If we only get heartbeats (ping) and then the stream dies, we should rotate account.
                let mut claude_stream = create_claude_sse_stream(
                    gemini_stream, 
                    trace_id.clone(), 
                    email.clone(),
                    Some(session_id_str.clone()),
                    scaling_enabled,
                    context_limit
                );

                let mut first_data_chunk = None;
                let mut retry_this_account = false;

                // Loop to skip heartbeats during peek
                loop {
                    match tokio::time::timeout(std::time::Duration::from_secs(60), claude_stream.next()).await {
                        Ok(Some(Ok(bytes))) => {
                            if bytes.is_empty() {
                                continue;
                            }
                            
                            let text = String::from_utf8_lossy(&bytes);
                            // Skip SSE comments/pings
                            if text.trim().starts_with(":") {
                                debug!("[{}] Skipping peek heartbeat: {}", trace_id, text.trim());
                                continue;
                            }

                            // We found real data!
                            first_data_chunk = Some(bytes);
                            break;
                        }
                        Ok(Some(Err(e))) => {
                            tracing::warn!("[{}] Stream error during peek: {}, retrying...", trace_id, e);
                            last_error = format!("Stream error during peek: {}", e);
                            retry_this_account = true;
                            break;
                        }
                        Ok(None) => {
                            tracing::warn!("[{}] Stream ended during peek (Empty Response), retrying...", trace_id);
                            last_error = "Empty response stream during peek".to_string();
                            retry_this_account = true;
                            break;
                        }
                        Err(_) => {
                            tracing::warn!("[{}] Timeout waiting for first data (60s), retrying...", trace_id);
                            last_error = "Timeout waiting for first data".to_string();
                            retry_this_account = true;
                            break;
                        }
                    }
                }

                if retry_this_account {
                    continue;
                }

                match first_data_chunk {
                    Some(bytes) => {
                        // We have data! Construct the combined stream
                        let stream_rest = claude_stream;
                        let combined_stream = Box::pin(futures::stream::once(async move { Ok(bytes) })
                            .chain(stream_rest.map(|result| -> Result<Bytes, std::io::Error> {
                                match result {
                                    Ok(b) => Ok(b),
                                    Err(e) => Ok(Bytes::from(format!("data: {{\"error\":\"{}\"}}\n\n", e))),
                                }
                            })));

                        // 判断客户端期望的格式
                        if client_wants_stream {
                            // 客户端本就要 Stream，直接返回 SSE
                            return Response::builder()
                                .status(StatusCode::OK)
                                .header(header::CONTENT_TYPE, "text/event-stream")
                                .header(header::CACHE_CONTROL, "no-cache")
                                .header(header::CONNECTION, "keep-alive")
                                .header("X-Account-Email", &email)
                                .header("X-Mapped-Model", &request_with_mapped.model)
                                .header("X-Context-Purified", if is_purified { "true" } else { "false" })
                                .body(Body::from_stream(combined_stream))
                                .unwrap();
                        } else {
                            // 客户端要非 Stream，需要收集完整响应并转换为 JSON
                            use crate::proxy::mappers::claude::collect_stream_to_json;
                            
                            match collect_stream_to_json(combined_stream).await {
                                Ok(full_response) => {
                                    info!("[{}] ✓ Stream collected and converted to JSON", trace_id);
                                    return Response::builder()
                                        .status(StatusCode::OK)
                                        .header(header::CONTENT_TYPE, "application/json")
                                        .header("X-Account-Email", &email)
                                        .header("X-Mapped-Model", &request_with_mapped.model)
                                        .header("X-Context-Purified", if is_purified { "true" } else { "false" })
                                        .body(Body::from(serde_json::to_string(&full_response).unwrap()))
                                        .unwrap();
                                }
                                Err(e) => {
                                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("Stream collection error: {}", e)).into_response();
                                }
                            }
                        }
                    },

                    None => {
                        tracing::warn!("[{}] Stream ended immediately (Empty Response), retrying...", trace_id);
                        last_error = "Empty response stream (None)".to_string();
                        continue;
                    }
                }
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
                
                // Determine context limit based on model
                let context_limit = crate::proxy::mappers::claude::utils::get_context_limit_for_model(&request_with_mapped.model);

                // 转换
                // [FIX #765] Pass session_id and model_name for signature caching
                let s_id_owned = session_id.map(|s| s.to_string());
                let claude_response = match transform_response(&gemini_response, scaling_enabled, context_limit, s_id_owned, request_with_mapped.model.clone()) {
                    Ok(r) => r,
                    Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("Transform error: {}", e)).into_response(),
                };

                // [Optimization] 记录闭环日志：消耗情况
                let cache_info = if let Some(cached) = claude_response.usage.cache_read_input_tokens {
                    format!(", Cached: {}", cached)
                } else {
                    String::new()
                };
                
                tracing::info!(
                    "[{}] Request finished. Model: {}, Tokens: In {}, Out {}{}", 
                    trace_id, 
                    request_with_mapped.model, 
                    claude_response.usage.input_tokens, 
                    claude_response.usage.output_tokens,
                    cache_info
                );

                return (StatusCode::OK, [("X-Account-Email", email.as_str()), ("X-Mapped-Model", request_with_mapped.model.as_str())], Json(claude_response)).into_response();
            }
        }
        
        // 1. 立即提取状态码和 headers（防止 response 被 move）
        let status_code = status.as_u16();
        let retry_after = response.headers().get("Retry-After").and_then(|h| h.to_str().ok()).map(|s| s.to_string());
        
        // 2. 获取错误文本并转移 Response 所有权
        let error_text = response.text().await.unwrap_or_else(|_| format!("HTTP {}", status));
        last_error = format!("HTTP {}: {}", status_code, error_text);
        debug!("[{}] Upstream Error Response: {}", trace_id, error_text);
        
        // 3. 标记限流状态(用于 UI 显示) - 使用异步版本以支持实时配额刷新
        // 🆕 传入实际使用的模型,实现模型级别限流,避免不同模型配额互相影响
        if status_code == 429 || status_code == 529 || status_code == 503 || status_code == 500 {
            token_manager.mark_rate_limited_async(&email, status_code, retry_after.as_deref(), &error_text, Some(&request_with_mapped.model)).await;
        }

        // 4. 处理 400 错误 (Thinking 签名失效 或 块顺序错误)
        if status_code == 400
            && !retried_without_thinking
            && (error_text.contains("Invalid `signature`")
                || error_text.contains("thinking.signature: Field required")
                || error_text.contains("thinking.thinking: Field required")
                || error_text.contains("thinking.signature")
                || error_text.contains("thinking.thinking")
                || error_text.contains("INVALID_ARGUMENT")
                || error_text.contains("Corrupted thought signature")
                || error_text.contains("failed to deserialise")
                || error_text.contains("Invalid signature")
                || error_text.contains("thinking block")
                || error_text.contains("Found `text`")
                || error_text.contains("Found 'text'")
                || error_text.contains("must be `thinking`")
                || error_text.contains("must be 'thinking'")
                )
        {
            // Existing logic for thinking signature...
            retried_without_thinking = true;
            
            // 使用 WARN 级别,因为这不应该经常发生(已经主动过滤过)
            tracing::warn!(
                "[{}] Unexpected thinking signature error (should have been filtered). \
                 Retrying with all thinking blocks removed.",
                trace_id
            );

            // [IMPROVED] 不再禁用 Thinking 模式！
            // 既然我们已经将历史 Thinking Block 转换为 Text，那么当前请求可以视为一个新的 Thinking 会话
            // 保持 thinking 配置开启，让模型重新生成思维，避免退化为简单的 "OK" 回复
            // request_for_body.thinking = None;
            
            // 清理历史消息中的所有 Thinking Block，将其转换为 Text 以保留上下文
            for msg in request_for_body.messages.iter_mut() {
                if let crate::proxy::mappers::claude::models::MessageContent::Array(blocks) = &mut msg.content {
                    let mut new_blocks = Vec::with_capacity(blocks.len());
                    for block in blocks.drain(..) {
                        match block {
                            crate::proxy::mappers::claude::models::ContentBlock::Thinking { thinking, .. } => {
                                // 降级为 text
                                if !thinking.is_empty() {
                                    tracing::debug!("[Fallback] Converting thinking block to text (len={})", thinking.len());
                                    new_blocks.push(crate::proxy::mappers::claude::models::ContentBlock::Text { 
                                        text: thinking 
                                    });
                                }
                            },
                            crate::proxy::mappers::claude::models::ContentBlock::RedactedThinking { .. } => {
                                // Redacted thinking 没什么用，直接丢弃
                            },
                            _ => new_blocks.push(block),
                        }
                    }
                }
            }
            
            // [NEW] Heal session after stripping thinking blocks to prevent "naked ToolResult" rejection
            // This ensures that any ToolResult in history is properly "closed" with synthetic messages
            // if its preceding Thinking block was just converted to Text.
            crate::proxy::mappers::claude::thinking_utils::close_tool_loop_for_thinking(&mut request_for_body.messages);
            
            // 清理模型名中的 -thinking 后缀
            if request_for_body.model.contains("claude-") {
                let mut m = request_for_body.model.clone();
                m = m.replace("-thinking", "");
                if m.contains("claude-sonnet-4-5-") {
                    m = "claude-sonnet-4-5".to_string();
                } else if m.contains("claude-opus-4-5-") || m.contains("claude-opus-4-") {
                    m = "claude-opus-4-5".to_string();
                }
                request_for_body.model = m;
            }
            
            // [FIX] 强制重试：因为我们已经清理了 thinking block，所以这是一个新的、可以重试的请求
            // 不要使用 determine_retry_strategy，因为它会因为 retried_without_thinking=true 而返回 NoRetry
            if apply_retry_strategy(
                RetryStrategy::FixedDelay(Duration::from_millis(100)), 
                attempt, 
                status_code, 
                &trace_id
            ).await {
                continue;
            }
        }

        // 5. 统一处理所有可重试错误
        // [REMOVED] 不再特殊处理 QUOTA_EXHAUSTED,允许账号轮换
        // 原逻辑会在第一个账号配额耗尽时直接返回,导致"平衡"模式无法切换账号
        
        
        // 确定重试策略
        let strategy = determine_retry_strategy(status_code, &error_text, retried_without_thinking);
        
        // 执行退避
        if apply_retry_strategy(strategy, attempt, status_code, &trace_id).await {
            // 判断是否需要轮换账号
            if !should_rotate_account(status_code) {
                debug!("[{}] Keeping same account for status {} (server-side issue)", trace_id, status_code);
            }
            continue;
        } else {
            // 5. 增强的 400 错误处理: Prompt Too Long 友好提示
            if status_code == 400 && (error_text.contains("too long") || error_text.contains("exceeds") || error_text.contains("limit")) {
                 return (
                    StatusCode::BAD_REQUEST,
                    [("X-Account-Email", email.as_str())],
                    Json(json!({
                        "id": "err_prompt_too_long",
                        "type": "error",
                        "error": {
                            "type": "invalid_request_error",
                            "message": "Prompt is too long (server-side context limit reached).",
                            "suggestion": "Please: 1) Executive '/compact' in Claude Code 2) Reduce conversation history 3) Switch to gemini-1.5-pro (2M context limit)"
                        }
                    }))
                ).into_response();
            }

            // 不可重试的错误，直接返回
            error!("[{}] Non-retryable error {}: {}", trace_id, status_code, error_text);
            return (status, [("X-Account-Email", email.as_str())], error_text).into_response();
        }
    }
    
    if let Some(email) = last_email {
        (StatusCode::TOO_MANY_REQUESTS, [("X-Account-Email", email)], Json(json!({
            "type": "error",
            "error": {
                "type": "overloaded_error",
                "message": format!("All {} attempts failed. Last error: {}", max_attempts, last_error)
            }
        }))).into_response()
    } else {
        (StatusCode::TOO_MANY_REQUESTS, Json(json!({
            "type": "error",
            "error": {
                "type": "overloaded_error",
                "message": format!("All {} attempts failed. Last error: {}", max_attempts, last_error)
            }
        }))).into_response()
    }
}

/// 列出可用模型
pub async fn handle_list_models(State(state): State<AppState>) -> impl IntoResponse {
    use crate::proxy::common::model_mapping::get_all_dynamic_models;

    let model_ids = get_all_dynamic_models(
        &state.custom_mapping,
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

// 移除已失效的简单单元测试，后续将补全完整的集成测试
/*
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_handle_list_models() {
        // handle_list_models 现在需要 AppState，此处跳过旧的单元测试
    }
}
*/

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
    // Removed: "Caveat: The messages below were generated" - this is a normal Claude Desktop system prompt
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
        BackgroundTaskType::TitleGeneration => BACKGROUND_MODEL_LITE,     // 极简任务
        BackgroundTaskType::SimpleSummary => BACKGROUND_MODEL_LITE,       // 简单摘要
        BackgroundTaskType::SystemMessage => BACKGROUND_MODEL_LITE,       // 系统消息
        BackgroundTaskType::PromptSuggestion => BACKGROUND_MODEL_LITE,    // 建议生成
        BackgroundTaskType::EnvironmentProbe => BACKGROUND_MODEL_LITE,    // 环境探测
        BackgroundTaskType::ContextCompression => BACKGROUND_MODEL_STANDARD, // 复杂压缩
    }
}

// ===== [Issue #467 Fix] Warmup 请求拦截 =====

/// 检测是否为 Warmup 请求
/// 
/// Claude Code 每 10 秒发送一次 warmup 请求，特征包括：
/// 1. 用户消息内容以 "Warmup" 开头或包含 "Warmup"
/// 2. tool_result 内容为 "Warmup" 错误
/// 3. 消息循环模式：助手发送工具调用，用户返回 Warmup 错误
fn is_warmup_request(request: &ClaudeRequest) -> bool {
    // [FIX] Only check the LATEST message for Warmup characteristics.
    // Scanning history (take(10)) caused a "poisoned session" bug where one historical Warmup
    // message would cause all subsequent user inputs (e.g. "Continue") to be intercepted 
    // and replied with "OK".
    
    if let Some(msg) = request.messages.last() {
        // We only care if the *current* trigger is a Warmup
        match &msg.content {
            crate::proxy::mappers::claude::models::MessageContent::String(s) => {
                // Check if simple text starts with Warmup (and is short)
                if s.trim().starts_with("Warmup") && s.len() < 100 {
                    return true;
                }
            },
            crate::proxy::mappers::claude::models::MessageContent::Array(arr) => {
                for block in arr {
                    match block {
                        crate::proxy::mappers::claude::models::ContentBlock::Text { text } => {
                            let trimmed = text.trim();
                            if trimmed == "Warmup" || trimmed.starts_with("Warmup\n") {
                                return true;
                            }
                        },
                        crate::proxy::mappers::claude::models::ContentBlock::ToolResult { 
                            content, is_error, .. 
                        } => {
                            // Check tool result errors
                            let content_str = if let Some(s) = content.as_str() {
                                s.to_string()
                            } else {
                                content.to_string()
                            };
                            
                            // If it's an error and starts with Warmup, it's a warmup signal
                            if *is_error == Some(true) && content_str.trim().starts_with("Warmup") {
                                return true;
                            }
                        },
                        _ => {}
                    }
                }
            }
        }
    }
    
    false
}

/// 创建 Warmup 请求的模拟响应
/// 
/// 返回一个简单的响应，不消耗上游配额
fn create_warmup_response(request: &ClaudeRequest, is_stream: bool) -> Response {
    let model = &request.model;
    let message_id = format!("msg_warmup_{}", chrono::Utc::now().timestamp_millis());
    
    if is_stream {
        // 流式响应：发送标准的 SSE 事件序列
        let events = vec![
            // message_start
            format!(
                "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"id\":\"{}\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"{}\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{{\"input_tokens\":1,\"output_tokens\":0}}}}}}\n\n",
                message_id, model
            ),
            // content_block_start
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n".to_string(),
            // content_block_delta
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"OK\"}}\n\n".to_string(),
            // content_block_stop
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n".to_string(),
            // message_delta
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":1}}\n\n".to_string(),
            // message_stop
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string(),
        ];
        
        let body = events.join("");
        
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .header(header::CONNECTION, "keep-alive")
            .header("X-Warmup-Intercepted", "true")
            .body(Body::from(body))
            .unwrap()
    } else {
        // 非流式响应
        let response = json!({
            "id": message_id,
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "text",
                "text": "OK"
            }],
            "model": model,
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {
                "input_tokens": 1,
                "output_tokens": 1
            }
        });
        
        (
            StatusCode::OK,
            [("X-Warmup-Intercepted", "true")],
            Json(response)
        ).into_response()
    }
}
