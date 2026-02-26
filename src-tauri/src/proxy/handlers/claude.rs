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
use tokio::time::Duration;
use tracing::{debug, error, info};

use crate::proxy::mappers::claude::{
    transform_claude_request_in, transform_response, create_claude_sse_stream, ClaudeRequest,
    filter_invalid_thinking_blocks_with_family, close_tool_loop_for_thinking,
    clean_cache_control_from_messages, merge_consecutive_messages,
    models::{Message, MessageContent},
};
use crate::proxy::server::AppState;
use crate::proxy::mappers::context_manager::ContextManager;
use crate::proxy::mappers::estimation_calibrator::get_calibrator;
use crate::proxy::debug_logger;
use crate::proxy::upstream::client::mask_email;
use crate::proxy::common::client_adapter::CLIENT_ADAPTERS; // [NEW] Import Adapter Registry
use axum::http::HeaderMap;
use std::sync::{atomic::Ordering, Arc};

// ===== Task #6: OpenCode variants thinking config mapping =====
// Helper structs for parsing thinking hints from raw JSON
#[derive(Debug, Clone)]
struct ThinkingHint {
    budget_tokens: Option<u32>,
    level: Option<String>,
}

/// Extract thinking hints from raw request JSON (OpenCode variants compatibility)
/// Checks multiple possible paths for budget and level configuration
fn extract_thinking_hint(body: &Value) -> ThinkingHint {
    let mut hint = ThinkingHint {
        budget_tokens: None,
        level: None,
    };

    // Try to extract budget_tokens from various paths
    // Priority: thinking.budget_tokens > thinking.budgetTokens > thinking.budget > thinkingConfig.thinkingBudget
    if let Some(budget) = body
        .get("thinking")
        .and_then(|t| t.get("budget_tokens"))
        .and_then(|b| b.as_u64())
    {
        hint.budget_tokens = Some(budget as u32);
    } else if let Some(budget) = body
        .get("thinking")
        .and_then(|t| t.get("budgetTokens"))
        .and_then(|b| b.as_u64())
    {
        hint.budget_tokens = Some(budget as u32);
    } else if let Some(budget) = body
        .get("thinking")
        .and_then(|t| t.get("budget"))
        .and_then(|b| b.as_u64())
    {
        hint.budget_tokens = Some(budget as u32);
    } else if let Some(budget) = body
        .get("thinkingConfig")
        .and_then(|t| t.get("thinkingBudget"))
        .and_then(|b| b.as_u64())
    {
        hint.budget_tokens = Some(budget as u32);
    }

    // Try to extract level from thinkingLevel
    if let Some(level) = body.get("thinkingLevel").and_then(|l| l.as_str()) {
        hint.level = Some(level.to_lowercase());
    }

    hint
}

/// Map thinking level to suggested budget tokens
fn level_to_budget(level: &str) -> u32 {
    match level {
        "minimal" => 1024,
        "low" => 8192,
        "medium" => 16384,
        "high" => 24576,
        _ => 8192, // default to low
    }
}

/// Map thinking level to effort level for output_config
fn level_to_effort(level: &str) -> String {
    match level {
        "minimal" | "low" => "low".to_string(),
        "medium" => "medium".to_string(),
        "high" => "high".to_string(),
        _ => "low".to_string(),
    }
}

/// Apply thinking hints to ClaudeRequest
fn apply_thinking_hints(
    request: &mut crate::proxy::mappers::claude::models::ClaudeRequest,
    hint: &ThinkingHint,
    trace_id: &str,
) {
    let mut applied = false;

    // If budget is provided, set/override thinking config
    if let Some(budget) = hint.budget_tokens {
        request.thinking = Some(crate::proxy::mappers::claude::models::ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(budget),
            effort: None,
        });
        tracing::debug!(
            "[{}] Applied thinking hint: budget_tokens={}",
            trace_id, budget
        );
        applied = true;
    }

    // If level is provided
    if let Some(ref level) = hint.level {
        // Map to output_config.effort if not already set
        if request.output_config.is_none() {
            request.output_config = Some(crate::proxy::mappers::claude::models::OutputConfig {
                effort: Some(level_to_effort(level)),
            });
            tracing::debug!("[{}] Applied thinking hint: effort={}", trace_id, level);
            applied = true;
        }

        // If no budget provided but level is, map level to budget
        if hint.budget_tokens.is_none() {
            let budget = level_to_budget(level);
            request.thinking = Some(crate::proxy::mappers::claude::models::ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(budget),
                effort: None,
            });
            tracing::debug!(
                "[{}] Applied thinking hint: level={} -> budget_tokens={}",
                trace_id, level, budget
            );
            applied = true;
        }
    }

    if applied {
        tracing::info!("[{}] Applied OpenCode thinking hints to request", trace_id);
    }
}

const MAX_RETRY_ATTEMPTS: usize = 3;

// ===== Model Constants for Background Tasks =====
// These can be adjusted for performance/cost optimization or overridden by custom_mapping
const INTERNAL_BACKGROUND_TASK: &str = "internal-background-task";  // Unified virtual ID for all background tasks

// ===== Layer 3: XML Summary Prompt Template =====
// Borrowed from Practical-Guide-to-Context-Engineering + Claude Code official practice
// This prompt generates a structured 8-section XML summary for context compression
const CONTEXT_SUMMARY_PROMPT: &str = r#"You are a context compression specialist. Your task is to create a structured XML snapshot of the conversation history.

This snapshot will become the Agent's ONLY memory of the past. All key details, plans, errors, and user instructions MUST be preserved.

First, think through the entire history in a private <scratchpad>. Review the user's overall goal, the agent's actions, tool outputs, file modifications, and any unresolved issues. Identify every piece of information critical for future actions.

After reasoning, generate the final <state_snapshot> XML object. Information must be extremely dense. Omit any irrelevant conversational filler.

The structure MUST be as follows:

<state_snapshot>
  <overall_goal>
    <!-- Describe the user's high-level goal in one concise sentence -->
  </overall_goal>
  
  <technical_context>
    <!-- Tech stack: frameworks, languages, toolchain, dependency versions -->
  </technical_context>
  
  <file_system_state>
    <!-- List files that were created, read, modified, or deleted. Note their status -->
  </file_system_state>
  
  <code_changes>
    <!-- Key code snippets (preserve function signatures and important logic) -->
  </code_changes>
  
  <debugging_history>
    <!-- List all errors encountered, with stack traces, and how they were fixed -->
  </debugging_history>
  
  <current_plan>
    <!-- Step-by-step plan. Mark completed steps -->
  </current_plan>
  
  <user_preferences>
    <!-- User's work preferences for this project (test commands, code style, etc.) -->
  </user_preferences>
  
  <key_decisions>
    <!-- Critical architectural decisions and design choices -->
  </key_decisions>
  
  <latest_thinking_signature>
    <!-- [CRITICAL] Preserve the last valid thinking signature -->
    <!-- Format: base64-encoded signature string -->
    <!-- This MUST be copied exactly as-is, no modifications -->
  </latest_thinking_signature>
</state_snapshot>

**IMPORTANT**:
1. Code snippets must be complete, including function signatures and key logic
2. Error messages must be preserved verbatim, including line numbers and stacks
3. File paths must use absolute paths
4. The thinking signature must be copied exactly, no modifications
"#;

// ===== Jitter Configuration (REMOVED) =====
// Jitter was causing connection instability, reverted to fixed delays
// const JITTER_FACTOR: f64 = 0.2;


// ===== 统一退避策略模块 =====

// [REMOVED] apply_jitter function
// Jitter logic removed to restore stability (v3.3.16 fix)

// ===== 统一退避策略模块 =====
// 移除本地重复定义，使用 common 中的统一实现
use super::common::{determine_retry_strategy, apply_retry_strategy, should_rotate_account, RetryStrategy};

// ===== 退避策略模块结束 =====

/// 处理 Claude messages 请求
/// 
/// 处理 Chat 消息请求流程
pub async fn handle_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    // [FIX] 保存原始请求体的完整副本，用于日志记录
    // 这确保了即使结构体定义遗漏字段，日志也能完整记录所有参数
    let original_body = body.clone();
    
    tracing::debug!("handle_messages called. Body JSON len: {}", body.to_string().len());
    
    // 生成随机 Trace ID 用户追踪
    let trace_id: String = rand::Rng::sample_iter(rand::thread_rng(), &rand::distributions::Alphanumeric)
        .take(6)
        .map(char::from)
        .collect::<String>().to_lowercase();
    let debug_cfg = state.debug_logging.read().await.clone();
    
    // [NEW] Detect Client Adapter
    // 检查是否有匹配的客户端适配器（如 opencode）
    let client_adapter = CLIENT_ADAPTERS.iter().find(|a| a.matches(&headers)).cloned();
    if let Some(_adapter) = &client_adapter {
        tracing::debug!("[{}] Client Adapter detected: Applying custom strategies", trace_id);
    }
        
    // Decide whether this request should be handled by z.ai (Anthropic passthrough) or the existing Google flow.
    let zai = state.zai.read().await.clone();
    let zai_enabled = zai.enabled && !matches!(zai.dispatch_mode, crate::proxy::ZaiDispatchMode::Off);
    let google_accounts = state.token_manager.len();

    // [CRITICAL REFACTOR] 优先解析请求以获取模型信息(用于智能兜底判断)
    let mut request: crate::proxy::mappers::claude::models::ClaudeRequest = match serde_json::from_value(body.clone()) {
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

    // [Task #6] Apply OpenCode variants thinking hints from raw JSON
    let thinking_hint = extract_thinking_hint(&original_body);
    apply_thinking_hints(&mut request, &thinking_hint, &trace_id);

    if debug_logger::is_enabled(&debug_cfg) {
        // [FIX] 使用原始 body 副本记录日志，确保不丢失任何字段
        let original_payload = json!({
            "kind": "original_request",
            "protocol": "anthropic",
            "trace_id": trace_id,
            "original_model": request.model,
            "request": original_body,  // 使用原始请求体，不是结构体序列化
        });
        debug_logger::write_debug_payload(&debug_cfg, Some(&trace_id), "original_request", &original_payload).await;
    }

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
            request.messages.len(), // [NEW v4.0.0] Pass message count
        )
        .await;
    }
    
    // Google Flow 继续使用 request 对象
    // (后续代码不需要再次 filter_invalid_thinking_blocks)
    
    // [NEW] 获取上下文控制配置
    let experimental = state.experimental.read().await;
    let scaling_enabled = experimental.enable_usage_scaling;
    let threshold_l1 = experimental.context_compression_threshold_l1;
    let threshold_l2 = experimental.context_compression_threshold_l2;
    let threshold_l3 = experimental.context_compression_threshold_l3;

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
    let retried_without_thinking = false;
    let mut last_email: Option<String> = None;
    let mut last_mapped_model: Option<String> = None;
    let mut last_status = StatusCode::SERVICE_UNAVAILABLE; // Default to 503 if no response reached
    
    for attempt in 0..max_attempts {
        // 2. 模型路由解析
        let mut mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
            &request_for_body.model,
            &*state.custom_mapping.read().await,
        );
        last_mapped_model = Some(mapped_model.clone());
        
        // 将 Claude 工具转为 Value 数组以便探测联网
        let tools_val: Option<Vec<Value>> = request_for_body.tools.as_ref().map(|list| {
            list.iter().map(|t| serde_json::to_value(t).unwrap_or(json!({}))).collect()
        });

        let config = crate::proxy::mappers::common_utils::resolve_request_config(
            &request_for_body.model,
            &mapped_model,
            &tools_val,
            request.size.as_deref(),      // [NEW] Pass size parameter
            request.quality.as_deref(),   // [NEW] Pass quality parameter
            None,  // image_size
            None,  // body
        );

        // 0. 尝试提取 session_id 用于粘性调度 (Phase 2/3)
        // 使用 SessionManager 生成稳定的会话指纹
        let session_id_str = crate::proxy::session_manager::SessionManager::extract_session_id(&request_for_body);
        let session_id = Some(session_id_str.as_str());

        let force_rotate_token = attempt > 0;
        let (access_token, project_id, email, account_id, _wait_ms) = match token_manager.get_token(&config.request_type, force_rotate_token, session_id, &config.final_model).await {
            Ok(t) => t,
            Err(e) => {
                let safe_message = if e.contains("invalid_grant") {
                    "OAuth refresh failed (invalid_grant): refresh_token likely revoked/expired; reauthorize account(s) to restore service.".to_string()
                } else {
                    e
                };
                let headers = [
                    ("X-Mapped-Model", mapped_model.as_str()),
                ];
                 return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    headers,
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
            let virtual_model_id = select_background_model(task_type);
            
            // [FIX] 必须根据虚拟 ID Re-resolve 路由，以支持用户自定义映射 (如 internal-task -> gemini-3)
            // 否则会直接使用 generic ID 导致下游无法识别或只能使用静态默认值
            let resolved_model = crate::proxy::common::model_mapping::resolve_model_route(
                virtual_model_id, 
                &*state.custom_mapping.read().await
            );

            info!(
                "[{}][AUTO] 检测到后台任务 (类型: {:?}), 路由重定向: {} -> {} (最终物理模型: {})",
                trace_id,
                task_type,
                mapped_model,
                virtual_model_id,
                resolved_model
            );
            
            // 覆盖用户自定义映射 (同时更新变量和 Request 对象)
            mapped_model = resolved_model.clone();
            request_with_mapped.model = resolved_model;
            
            // 后台任务净化：
            // 1. 移除工具定义（后台任务不需要工具）
            request_with_mapped.tools = None;
            
            // 2. 移除 Thinking 配置（Flash 模型不支持）
            request_with_mapped.thinking = None;
            
            // 3. 清理历史消息中的 Thinking Block，防止 Invalid Argument
            // 使用 ContextManager 的统一策略 (Aggressive)
            crate::proxy::mappers::context_manager::ContextManager::purify_history(
                &mut request_with_mapped.messages, 
                crate::proxy::mappers::context_manager::PurificationStrategy::Aggressive
            );
        }

        // ===== [3-Layer Progressive Compression + Calibrated Estimation] Context Management =====
        // [ENHANCED] 整合 3.3.47 的三层压缩框架 + PR #925 的动态校准机制
        // [NEW] 只有当 scaling_enabled 为 true 时才执行压缩逻辑 (联动机制)
        // Layer 1 (60%): Tool message trimming - Does NOT break cache
        // Layer 2 (75%): Thinking purification - Breaks cache but preserves signatures
        // Layer 3 (90%): Fork conversation + XML summary - Ultimate optimization
        let mut is_purified = false;
        let mut compression_applied = false;
        
        if !retried_without_thinking && scaling_enabled {  // 新增 scaling_enabled 联动判断
            // 1. Determine context limit (Flash: ~1M, Pro: ~2M)
            let context_limit = if mapped_model.contains("flash") {
                1_000_000
            } else {
                2_000_000
            };

            // 2. [ENHANCED] 使用校准器提高估算准确度 (PR #925)
            let raw_estimated = ContextManager::estimate_token_usage(&request_with_mapped);
            let calibrator = get_calibrator();
            let mut estimated_usage = calibrator.calibrate(raw_estimated);
            let mut usage_ratio = estimated_usage as f32 / context_limit as f32;
            
            info!(
                "[{}] [ContextManager] Context pressure: {:.1}% (raw: {}, calibrated: {} / {}), Calibration factor: {:.2}",
                trace_id, usage_ratio * 100.0, raw_estimated, estimated_usage, context_limit, calibrator.get_factor()
            );

            // ===== Layer 1: Tool Message Trimming (L1 threshold) =====
            // Borrowed from Practical-Guide-to-Context-Engineering
            // Advantage: Completely cache-friendly (only removes messages, doesn't modify content)
            if usage_ratio > threshold_l1 && !compression_applied {
                if ContextManager::trim_tool_messages(&mut request_with_mapped.messages, 5) {
                    info!(
                        "[{}] [Layer-1] Tool trimming triggered (usage: {:.1}%, threshold: {:.1}%)",
                        trace_id, usage_ratio * 100.0, threshold_l1 * 100.0
                    );
                    compression_applied = true;
                    
                    // Re-estimate after trimming (with calibration)
                    let new_raw = ContextManager::estimate_token_usage(&request_with_mapped);
                    let new_usage = calibrator.calibrate(new_raw);
                    let new_ratio = new_usage as f32 / context_limit as f32;
                    
                    info!(
                        "[{}] [Layer-1] Compression result: {:.1}% → {:.1}% (saved {} tokens)",
                        trace_id, usage_ratio * 100.0, new_ratio * 100.0, estimated_usage - new_usage
                    );
                    
                    // If compression is sufficient, skip further layers
                    if new_ratio < 0.7 {
                        estimated_usage = new_usage;
                        usage_ratio = new_ratio;
                        // Success, no need for Layer 2
                    } else {
                        // Still high pressure, update for Layer 2
                        usage_ratio = new_ratio;
                        compression_applied = false; // Allow Layer 2 to run
                    }
                }
            }

            // ===== Layer 2: Thinking Content Compression (L2 threshold) =====
            // NEW: Preserve signatures while compressing thinking text
            // This prevents signature chain breakage (Issue #902)
            if usage_ratio > threshold_l2 && !compression_applied {
                info!(
                    "[{}] [Layer-2] Thinking compression triggered (usage: {:.1}%, threshold: {:.1}%)",
                    trace_id, usage_ratio * 100.0, threshold_l2 * 100.0
                );
                
                // Use new signature-preserving compression
                if ContextManager::compress_thinking_preserve_signature(
                    &mut request_with_mapped.messages, 
                    4 // Protect last 4 messages (~2 turns)
                ) {
                    is_purified = true; // Still breaks cache, but preserves signatures
                    compression_applied = true;
                    
                    let new_raw = ContextManager::estimate_token_usage(&request_with_mapped);
                    let new_usage = calibrator.calibrate(new_raw);
                    let new_ratio = new_usage as f32 / context_limit as f32;
                    
                    info!(
                        "[{}] [Layer-2] Compression result: {:.1}% → {:.1}% (saved {} tokens)",
                        trace_id, usage_ratio * 100.0, new_ratio * 100.0, estimated_usage - new_usage
                    );
                    
                    usage_ratio = new_ratio;
                }
            }

            // ===== Layer 3: Fork Conversation + XML Summary (L3 threshold) =====
            // Ultimate optimization: Generate structured summary and start fresh conversation
            // Advantage: Completely cache-friendly (append-only), extreme compression ratio
            if usage_ratio > threshold_l3 && !compression_applied {
                info!(
                    "[{}] [Layer-3] Context pressure ({:.1}%) exceeded threshold ({:.1}%), attempting Fork+Summary",
                    trace_id, usage_ratio * 100.0, threshold_l3 * 100.0
                );
                
                // Clone token_manager Arc to avoid borrow issues
                let token_manager_clone = token_manager.clone();
                
                match try_compress_with_summary(&request_with_mapped, &trace_id, &token_manager_clone).await {
                    Ok(forked_request) => {
                        info!(
                            "[{}] [Layer-3] Fork successful: {} → {} messages",
                            trace_id,
                            request_with_mapped.messages.len(),
                            forked_request.messages.len()
                        );
                        
                        request_with_mapped = forked_request;
                        is_purified = false; // Fork doesn't break cache!
                        
                        // Re-estimate after fork (with calibration)
                        let new_raw = ContextManager::estimate_token_usage(&request_with_mapped);
                        let new_usage = calibrator.calibrate(new_raw);
                        let new_ratio = new_usage as f32 / context_limit as f32;
                        
                        info!(
                            "[{}] [Layer-3] Compression result: {:.1}% → {:.1}% (saved {} tokens)",
                            trace_id, usage_ratio * 100.0, new_ratio * 100.0, estimated_usage - new_usage
                        );
                    }
                    Err(e) => {
                        error!(
                            "[{}] [Layer-3] Fork+Summary failed: {}, falling back to error response",
                            trace_id, e
                        );
                        
                        // Return friendly error to user
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({
                                "type": "error",
                                "error": {
                                    "type": "invalid_request_error",
                                    "message": format!("Context too long and automatic compression failed: {}", e),
                                    "suggestion": "Please use /compact or /clear command in Claude Code, or switch to a model with larger context window."
                                }
                            }))
                        ).into_response();
                    }
                }
            }
        }

        // [FIX] Estimate AFTER purification to get accurate token count for calibrator learning
        // Only estimate for calibrator when content was not purified, to avoid skewed learning
        let raw_estimated = if !is_purified {
            ContextManager::estimate_token_usage(&request_with_mapped)
        } else {
            0 // Don't record calibration data when content was purified
        };

        request_with_mapped.model = mapped_model.clone();

        // 生成 Trace ID (简单用时间戳后缀)
        // let _trace_id = format!("req_{}", chrono::Utc::now().timestamp_subsec_millis());

        let gemini_body = match transform_claude_request_in(&request_with_mapped, &project_id, retried_without_thinking, Some(account_id.as_str()), &session_id_str) {
            Ok(b) => {
                debug!("[{}] Transformed Gemini Body: {}", trace_id, serde_json::to_string_pretty(&b).unwrap_or_default());
                b
            },
            Err(e) => {
                 let headers = [
                    ("X-Mapped-Model", request_with_mapped.model.as_str()),
                    ("X-Account-Email", email.as_str()),
                ];
                 return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    headers,
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

        if debug_logger::is_enabled(&debug_cfg) {
            let payload = json!({
                "kind": "v1internal_request",
                "protocol": "anthropic",
                "trace_id": trace_id,
                "original_model": request.model,
                "mapped_model": request_with_mapped.model,
                "request_type": config.request_type,
                "attempt": attempt,
                "v1internal_request": gemini_body.clone(),
            });
            debug_logger::write_debug_payload(&debug_cfg, Some(&trace_id), "v1internal_request", &payload).await;
        }
        
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
        // [FIX #765/1522] Prepare Robust Beta Headers for Claude models
        let mut extra_headers = std::collections::HashMap::new();
        if mapped_model.to_lowercase().contains("claude") {
            extra_headers.insert("anthropic-beta".to_string(), "claude-code-20250219".to_string());
            tracing::debug!("[{}] Added Comprehensive Beta Headers for Claude model", trace_id);
        }
        
        // [NEW] Inject Beta Headers from Client Adapter
        if let Some(adapter) = &client_adapter {
            let mut temp_headers = HeaderMap::new();
            adapter.inject_beta_headers(&mut temp_headers);
            for (k, v) in temp_headers {
                if let Some(name) = k {
                    if let Ok(v_str) = v.to_str() {
                        extra_headers.insert(name.to_string(), v_str.to_string());
                        tracing::debug!("[{}] Added Adapter Header: {}: {}", trace_id, name, v_str);
                    }
                }
            }
        }

        // Upstream call configuration continued...

        let call_result = match upstream
            .call_v1_internal_with_headers(method, &access_token, gemini_body, query, extra_headers.clone(), Some(account_id.as_str()))
            .await {
            Ok(r) => r,
            Err(e) => {
                last_error = e.clone();
                debug!("Request failed on attempt {}/{}: {}", attempt + 1, max_attempts, e);
                continue;
            }
        };

        // [NEW] 记录端点降级日志到 debug 文件
        if !call_result.fallback_attempts.is_empty() && debug_logger::is_enabled(&debug_cfg) {
            let fallback_entries: Vec<Value> = call_result.fallback_attempts.iter().map(|a| {
                json!({
                    "endpoint_url": a.endpoint_url,
                    "status": a.status,
                    "error": a.error,
                })
            }).collect();
            let payload = json!({
                "kind": "endpoint_fallback",
                "protocol": "anthropic",
                "trace_id": trace_id,
                "original_model": request.model,
                "mapped_model": request_with_mapped.model,
                "attempt": attempt,
                "account": mask_email(&email),
                "fallback_attempts": fallback_entries,
            });
            debug_logger::write_debug_payload(&debug_cfg, Some(&trace_id), "endpoint_fallback", &payload).await;
        }

        let response = call_result.response;
        // [NEW] 提取实际请求的上游端点 URL，用于日志记录和排查
        let upstream_url = response.url().to_string();
        let status = response.status();
        last_status = status;
        
        // 成功
        if status.is_success() {
            // [智能限流] 请求成功，重置该账号的连续失败计数
            token_manager.mark_account_success(&email);
            
                // Determine context limit based on model
                let context_limit = crate::proxy::mappers::claude::utils::get_context_limit_for_model(&request_with_mapped.model);

            // 处理流式响应
            if actual_stream {
                let meta = json!({
                    "protocol": "anthropic",
                    "trace_id": trace_id,
                    "original_model": request.model,
                    "mapped_model": request_with_mapped.model,
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

                let current_message_count = request_with_mapped.messages.len();

                // [FIX #MCP] Extract registered tool names for MCP fuzzy matching
                let registered_tool_names: Vec<String> = request_with_mapped.tools
                    .as_ref()
                    .map(|tools| tools.iter().filter_map(|t| t.name.clone()).collect())
                    .unwrap_or_default();

                // [FIX #530/#529/#859] Enhanced Peek logic to handle heartbeats and slow start
                // We must pre-read until we find a MEANINGFUL content block (like message_start).
                // If we only get heartbeats (ping) and then the stream dies, we should rotate account.
                let mut claude_stream = create_claude_sse_stream(
                    gemini_stream,
                    trace_id.clone(),
                    email.clone(),
                    Some(session_id_str.clone()),
                    scaling_enabled,
                    context_limit,
                    Some(raw_estimated), // [FIX] Pass estimated tokens for calibrator learning
                    current_message_count, // [NEW v4.0.0] Pass message count for rewind detection
                    client_adapter.clone(), // [NEW] Pass client adapter
                    registered_tool_names, // [FIX #MCP] Pass tool names for fuzzy matching
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
                                .header("X-Accel-Buffering", "no")
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
                // 转换
                let claude_response = match transform_response(
                    &gemini_response,
                    scaling_enabled,
                    context_limit,
                    s_id_owned,
                    request_with_mapped.model.clone(),
                    request_with_mapped.messages.len(), // [NEW v4.0.0] Pass message count for rewind detection
                ) {
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
        last_status = status;
        let retry_after = response.headers().get("Retry-After").and_then(|h| h.to_str().ok()).map(|s| s.to_string());
        
        // 2. 获取错误文本并转移 Response 所有权
        let error_text = response.text().await.unwrap_or_else(|_| format!("HTTP {}", status));
        last_error = format!("HTTP {}: {}", status_code, error_text);
        debug!("[{}] Upstream Error Response: {}", trace_id, error_text);
        if debug_logger::is_enabled(&debug_cfg) {
            let payload = json!({
                "kind": "upstream_response_error",
                "protocol": "anthropic",
                "trace_id": trace_id,
                "original_model": request.model,
                "mapped_model": request_with_mapped.model,
                "request_type": config.request_type,
                "attempt": attempt,
                "status": status_code,
                "upstream_url": upstream_url,
                "account": mask_email(&email),
                "error_text": error_text,
            });
            debug_logger::write_debug_payload(&debug_cfg, Some(&trace_id), "upstream_response_error", &payload).await;
        }
        
        // 3. 标记限流状态(用于 UI 显示) - 使用异步版本以支持实时配额刷新
        // 🆕 传入实际使用的模型,实现模型级别限流,避免不同模型配额互相影响
        if status_code == 429 || status_code == 529 || status_code == 503 || status_code == 500 || status_code == 404 {
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
            // Existing logic for thinking signature...\n            retried_without_thinking = true;
            
            // 使用 WARN 级别,因为这不应该经常发生(已经主动过滤过)
            tracing::warn!(
                "[{}] Unexpected thinking signature error (should have been filtered). \
                 Retrying with all thinking blocks removed.",
                trace_id
            );

            // [NEW] 追加修复提示词到最后一条用户消息
            if let Some(last_msg) = request_for_body.messages.last_mut() {
                if last_msg.role == "user" {
                    let repair_prompt = "\n\n[System Recovery] Your previous output contained an invalid signature. Please regenerate the response without the corrupted signature block.";
                    
                    match &mut last_msg.content {
                        crate::proxy::mappers::claude::models::MessageContent::String(s) => {
                            s.push_str(repair_prompt);
                        }
                        crate::proxy::mappers::claude::models::MessageContent::Array(blocks) => {
                            blocks.push(crate::proxy::mappers::claude::models::ContentBlock::Text {
                                text: repair_prompt.to_string(),
                            });
                        }
                    }
                    tracing::debug!("[{}] Appended repair prompt to last user message", trace_id);
                }
            }

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
                    *blocks = new_blocks;
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
                if m.contains("claude-sonnet-4-6-") {
                    m = "claude-sonnet-4-6".to_string();
                } else if m.contains("claude-sonnet-4-5-") {
                    m = "claude-sonnet-4-6".to_string();
                } else if m.contains("claude-opus-4-6-") {
                    m = "claude-opus-4-6".to_string();
                } else if m.contains("claude-opus-4-5-") || m.contains("claude-opus-4-") {
                    m = "claude-opus-4-5".to_string();
                }
                request_for_body.model = m;
            }
            
            // [FIX] 强制重试：因为我们已经清理了 thinking block，所以这是一个新的、可以重试的请求
            // 不要使用 determine_retry_strategy，因为它会因为 retried_without_thinking=true 而返回 NoRetry
            if apply_retry_strategy(
                RetryStrategy::FixedDelay(Duration::from_millis(200)), 
                attempt, 
                max_attempts,
                status_code, 
                &trace_id
            ).await {
                continue;
            }
        }

        // 5. 统一处理所有可重试错误
        // [REMOVED] 不再特殊处理 QUOTA_EXHAUSTED,允许账号轮换
        // 原逻辑会在第一个账号配额耗尽时直接返回,导致"平衡"模式无法切换账号

        // [FIX] 403 时设置 is_forbidden 状态，避免账号被重复选中
        if status_code == 403 {
            // Check for VALIDATION_REQUIRED error - temporarily block account
            if error_text.contains("VALIDATION_REQUIRED") ||
               error_text.contains("verify your account") ||
               error_text.contains("validation_url")
            {
                tracing::warn!(
                    "[Claude] VALIDATION_REQUIRED detected on account {}, temporarily blocking",
                    email
                );
                let block_minutes = 10i64;
                let block_until = chrono::Utc::now().timestamp() + (block_minutes * 60);
                if let Err(e) = token_manager.set_validation_block_public(&account_id, block_until, &error_text).await {
                    tracing::error!("Failed to set validation block: {}", e);
                }
            }

            // 设置 is_forbidden 状态
            if let Err(e) = token_manager.set_forbidden(&account_id, &error_text).await {
                tracing::error!("Failed to set forbidden status for {}: {}", email, e);
            } else {
                tracing::warn!("[Claude] Account {} marked as forbidden due to 403", email);
            }
        }

        // 确定重试策略
        let strategy = determine_retry_strategy(status_code, &error_text, retried_without_thinking);
        
        // 执行退避
        if apply_retry_strategy(strategy, attempt, max_attempts, status_code, &trace_id).await {
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
            return (status, [
                ("X-Account-Email", email.as_str()),
                ("X-Mapped-Model", request_with_mapped.model.as_str())
            ], error_text).into_response();
        }
    }
    
    
    if let Some(email) = last_email {
        // [FIX] Include X-Mapped-Model in exhaustion error
        let mut headers = HeaderMap::new();
        headers.insert("X-Account-Email", header::HeaderValue::from_str(&email).unwrap());
        if let Some(model) = last_mapped_model {
             if let Ok(v) = header::HeaderValue::from_str(&model) {
                headers.insert("X-Mapped-Model", v);
             }
        }

        let error_type = match last_status.as_u16() {
            400 => "invalid_request_error",
            401 => "authentication_error",
            403 => "permission_error",
            429 => "rate_limit_error",
            529 => "overloaded_error",
            _ => "api_error",
        };

        // [FIX] 403 时返回 503，避免 Claude Code 客户端退出到登录页
        let response_status = if last_status.as_u16() == 403 {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            last_status
        };

        (response_status, headers, Json(json!({
            "type": "error",
            "error": {
                "id": "err_retry_exhausted",
                "type": error_type,
                "message": format!("All {} attempts failed. Last status: {}. Error: {}", max_attempts, last_status, last_error)
            }
        }))).into_response()
    } else {
        // Fallback if no email (e.g. mapping error before token)
        let mut headers = HeaderMap::new();
        if let Some(model) = last_mapped_model {
             if let Ok(v) = header::HeaderValue::from_str(&model) {
                headers.insert("X-Mapped-Model", v);
             }
        }

        let error_type = match last_status.as_u16() {
            400 => "invalid_request_error",
            401 => "authentication_error",
            403 => "permission_error",
            429 => "rate_limit_error",
            529 => "overloaded_error",
            _ => "api_error",
        };

        // [FIX] 403 时返回 503，避免 Claude Code 客户端退出到登录页
        let response_status = if last_status.as_u16() == 403 {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            last_status
        };

        (response_status, headers, Json(json!({
            "type": "error",
            "error": {
                "id": "err_retry_exhausted",
                "type": error_type,
                "message": format!("All {} attempts failed. Last status: {}. Error: {}", max_attempts, last_status, last_error)
            }
        }))).into_response()
    }
}

/// 列出可用模型
pub async fn handle_list_models(State(state): State<AppState>) -> impl IntoResponse {
    use crate::proxy::common::model_mapping::get_all_dynamic_models;

    let model_ids = get_all_dynamic_models(
        &state.custom_mapping,
        Some(&state.token_manager)
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
            0, // [NEW v4.0.0] Tokens count doesn't need rewind detection
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
        BackgroundTaskType::TitleGeneration => INTERNAL_BACKGROUND_TASK,
        BackgroundTaskType::SimpleSummary => INTERNAL_BACKGROUND_TASK,
        BackgroundTaskType::SystemMessage => INTERNAL_BACKGROUND_TASK,
        BackgroundTaskType::PromptSuggestion => INTERNAL_BACKGROUND_TASK,
        BackgroundTaskType::EnvironmentProbe => INTERNAL_BACKGROUND_TASK,
        BackgroundTaskType::ContextCompression => INTERNAL_BACKGROUND_TASK,
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

// ===== [Helper] Synchronous Upstream Call =====
// Reusable function for making non-streaming calls to Gemini API
// Used by Layer 3 and potentially other internal operations

/// Call Gemini API synchronously and return the response text
/// 
/// This is used for internal operations that need to wait for a complete response,
/// such as generating summaries or other background tasks.
async fn call_gemini_sync(
    model: &str,
    request: &ClaudeRequest,
    token_manager: &Arc<crate::proxy::TokenManager>,
    trace_id: &str,
) -> Result<String, String> {
    // Get token and transform request
    let (access_token, project_id, _, account_id, _wait_ms) = token_manager
        .get_token("gemini", false, None, model)
        .await
        .map_err(|e| format!("Failed to get account: {}", e))?;
    
    let gemini_body = crate::proxy::mappers::claude::transform_claude_request_in(request, &project_id, false, Some(account_id.as_str()), trace_id)
        .map_err(|e| format!("Failed to transform request: {}", e))?;
    
    // Call Gemini API
    let upstream_url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
        model
    );
    
    debug!("[{}] Calling Gemini API: {}", trace_id, model);
    
    let response = reqwest::Client::new()
        .post(&upstream_url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .json(&gemini_body)
        .send()
        .await
        .map_err(|e| format!("API call failed: {}", e))?;
    
    if !response.status().is_success() {
        return Err(format!(
            "API returned {}: {}", 
            response.status(), 
            response.text().await.unwrap_or_default()
        ));
    }
    
    let gemini_response: Value = response.json().await
        .map_err(|e| format!("Failed to parse response: {}", e))?;
    
    // Extract text from response
    gemini_response
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Failed to extract text from response".to_string())
}

// ===== [Layer 3] Fork Conversation + XML Summary =====
// This is the ultimate context compression strategy
// Borrowed from Practical-Guide-to-Context-Engineering + Claude Code official practice

/// Try to compress context by generating an XML summary and forking the conversation
/// 
/// This function:
/// 1. Extracts the last valid thinking signature
/// 2. Calls a cheap model (gemini-2.5-flash-lite) to generate XML summary
/// 3. Creates a new message sequence with summary as prefix
/// 4. Preserves the signature in the summary
/// 5. Returns the forked request
/// 
/// Returns Ok(forked_request) on success, Err(error_message) on failure
async fn try_compress_with_summary(
    original_request: &ClaudeRequest,
    trace_id: &str,
    token_manager: &Arc<crate::proxy::TokenManager>,
) -> Result<ClaudeRequest, String> {
    info!("[{}] [Layer-3] Starting context compression with XML summary", trace_id);
    
    // 1. Extract last valid signature
    let last_signature = ContextManager::extract_last_valid_signature(&original_request.messages);
    
    if let Some(ref sig) = last_signature {
        debug!("[{}] [Layer-3] Extracted signature (len: {})", trace_id, sig.len());
    }
    
    // 2. Build summary request
    let mut summary_messages = original_request.messages.clone();
    
    // Add instruction to include signature in summary
    let signature_instruction = if let Some(ref sig) = last_signature {
        format!("\n\n**CRITICAL**: The last thinking signature is:\n```\n{}\n```\nYou MUST include this EXACTLY in the <latest_thinking_signature> section.", sig)
    } else {
        "\n\n**Note**: No thinking signature found in history. Leave <latest_thinking_signature> empty.".to_string()
    };
    
    // Append summary request as the last user message
    summary_messages.push(Message {
        role: "user".to_string(),
        content: MessageContent::String(format!(
            "{}{}",
            CONTEXT_SUMMARY_PROMPT,
            signature_instruction
        )),
    });
    
    let summary_request = ClaudeRequest {
        model: INTERNAL_BACKGROUND_TASK.to_string(),
        messages: summary_messages,
        system: None,
        stream: false,
        max_tokens: Some(8000),
        temperature: Some(0.3),
        tools: None,
        thinking: None,
        metadata: None,
        top_p: None,
        top_k: None,
        output_config: None,
        size: None,
        quality: None,
    };
    
    debug!("[{}] [Layer-3] Calling {} for summary generation", trace_id, INTERNAL_BACKGROUND_TASK);
    
    // 3. Call upstream using helper function (reuse existing infrastructure)
    let xml_summary = call_gemini_sync(
        INTERNAL_BACKGROUND_TASK,
        &summary_request,
        token_manager,
        trace_id,
    ).await?;
    
    info!("[{}] [Layer-3] Generated XML summary (len: {} chars)", trace_id, xml_summary.len());
    
    // 4. Create forked conversation with summary as prefix
    let mut forked_messages = vec![
        Message {
            role: "user".to_string(),
            content: MessageContent::String(format!(
                "Context has been compressed. Here is the structured summary of our conversation history:\n\n{}",
                xml_summary
            )),
        },
        Message {
            role: "assistant".to_string(),
            content: MessageContent::String(
                "I have reviewed the compressed context summary. I understand the current state and will continue from here.".to_string()
            ),
        },
    ];
    
    // 5. Append the user's latest message (if exists and is not the summary request)
    if let Some(last_msg) = original_request.messages.last() {
        if last_msg.role == "user" {
            // Check if it's not the summary instruction we just added
            if !matches!(&last_msg.content, MessageContent::String(s) if s.contains(CONTEXT_SUMMARY_PROMPT)) {
                forked_messages.push(last_msg.clone());
            }
        }
    }
    
    info!(
        "[{}] [Layer-3] Fork successful: {} messages → {} messages",
        trace_id,
        original_request.messages.len(),
        forked_messages.len()
    );
    
    // 6. Return forked request
    Ok(ClaudeRequest {
        model: original_request.model.clone(),
        messages: forked_messages,
        system: original_request.system.clone(),
        stream: original_request.stream,
        max_tokens: original_request.max_tokens,
        temperature: original_request.temperature,
        tools: original_request.tools.clone(),
        thinking: original_request.thinking.clone(),
        metadata: original_request.metadata.clone(),
        top_p: original_request.top_p,
        top_k: original_request.top_k,
        output_config: original_request.output_config.clone(),
        size: original_request.size.clone(),
        quality: original_request.quality.clone(),
    })
}
