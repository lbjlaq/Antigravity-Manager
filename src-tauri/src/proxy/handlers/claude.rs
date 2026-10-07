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

use crate::proxy::common::client_adapter::CLIENT_ADAPTERS; // [NEW] Import Adapter Registry
use crate::proxy::debug_logger;
use crate::proxy::mappers::claude::{
    clean_cache_control_from_messages, create_claude_sse_stream,
    filter_invalid_thinking_blocks_with_family, merge_consecutive_messages,
    models::{Message, MessageContent},
    transform_response, ClaudeRequest,
};
use crate::proxy::mappers::context_manager::ContextManager;
use crate::proxy::mappers::gemini::SUMMARY_REQUEST_TIMEOUT_SECS;
use crate::proxy::model_specs;
use crate::proxy::server::AppState;
use crate::proxy::upstream::client::mask_email;
use axum::http::HeaderMap;
use dashmap::DashMap;
use std::sync::{Arc, LazyLock};

/// 基于代际租约与滑动窗口的压缩免死状态机 (Compaction Immunity Lease)
/// 替代原无类型 DashSet 单次原子核销机制，天然支持并发请求、工具调用与网络重试；
/// 初始处于未激活保护态（支持长达 300s 电脑休眠唤醒），在首个业务请求接入后激活 30s 滑动窗口，
/// 支持窗口期内的所有并发请求与工具调用安全放行；滑动窗口超时后彻底销毁，
/// 根除“一次压缩终身免死无法再次自愈” (Fixes #3563)。
#[derive(Debug, Clone)]
struct CompactionImmunityLease {
    created_at: std::time::Instant,
    last_touched: std::time::Instant,
    consumed: bool,
}

static COMPACTION_IMMUNITY_LEASES: LazyLock<DashMap<String, CompactionImmunityLease>> =
    LazyLock::new(DashMap::new);

const COMPACTION_IMMUNITY_INITIAL_TTL_SECS: u64 = 300;
const COMPACTION_IMMUNITY_WINDOW_SECS: u64 = 30;

/// 压缩模式类型：区分手动 ./compact 指令驱动与自动超限自愈门禁驱动
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoworkCompactKind {
    Manual,
    Auto,
}

/// Claude Cowork 统一压缩执行状态池 (Session -> 状态信息)
/// 手动与自动压缩共享一套压缩标记与状态生命周期，防止冲突、竞争与双重 400 假报警
#[derive(Debug, Clone)]
pub struct CoworkCompactState {
    pub kind: CoworkCompactKind,
    pub before_tokens: u32,
    pub ts: std::time::Instant,
    pub summary_done: bool,
    pub target_limit: u32,
}

pub static COWORK_COMPACT_SESSIONS: LazyLock<DashMap<String, CoworkCompactState>> =
    LazyLock::new(DashMap::new);
pub const MAX_COMPACT_SESSIONS_CAPACITY: usize = 1000;

/// 近期触发压缩（手动或自动）的会话集合 (Session -> 触发时刻)，用于跨轮次对齐
pub static PENDING_COMPACT_SESSIONS: LazyLock<DashMap<String, std::time::Instant>> =
    LazyLock::new(DashMap::new);

/// 检查会话当前是否处于压缩流程中（无论是手动还是自动压缩，统一判断并在超时后主动清理）
pub fn is_session_in_compaction(session_key: &str) -> bool {
    if let Some(entry) = COWORK_COMPACT_SESSIONS.get(session_key) {
        if entry.ts.elapsed().as_secs() < 300 {
            true
        } else {
            drop(entry);
            COWORK_COMPACT_SESSIONS.remove(session_key);
            false
        }
    } else {
        false
    }
}

pub fn prune_compact_sessions_if_needed() {
    if COWORK_COMPACT_SESSIONS.len() >= MAX_COMPACT_SESSIONS_CAPACITY {
        let now = std::time::Instant::now();
        COWORK_COMPACT_SESSIONS.retain(|_, state| now.duration_since(state.ts).as_secs() < 300);
        if COWORK_COMPACT_SESSIONS.len() >= MAX_COMPACT_SESSIONS_CAPACITY {
            COWORK_COMPACT_SESSIONS.clear();
        }
    }
}

/// 刚刚完成 compact 的会话防重放缓存 (Session -> (完成时间戳, 回显文本, 剩余Token量))
pub static COWORK_JUST_COMPACTED_CACHE: LazyLock<
    DashMap<String, (std::time::Instant, String, u32)>,
> = LazyLock::new(DashMap::new);
pub const MAX_JUST_COMPACTED_CACHE_CAPACITY: usize = 1000;

pub fn prune_just_compacted_cache_if_needed() {
    if COWORK_JUST_COMPACTED_CACHE.len() >= MAX_JUST_COMPACTED_CACHE_CAPACITY {
        let purge_now = std::time::Instant::now();
        COWORK_JUST_COMPACTED_CACHE
            .retain(|_, (ts, _, _)| purge_now.duration_since(*ts).as_secs() < 60);
        if COWORK_JUST_COMPACTED_CACHE.len() >= MAX_JUST_COMPACTED_CACHE_CAPACITY {
            COWORK_JUST_COMPACTED_CACHE.clear();
        }
    }
}

/// 深度检测请求前几条消息是否包含 post-compaction continuation 接续标记
/// 遍历前 3 条消息中所有的 Text 内容块，彻底解决 <system-reminder> 块遮挡接续标记导致漏检的问题
pub fn detect_post_compaction_continuation(request: &ClaudeRequest) -> bool {
    for m in request.messages.iter().take(3) {
        match &m.content {
            crate::proxy::mappers::claude::models::MessageContent::String(s) => {
                if crate::proxy::mappers::common_utils::is_post_compaction_continuation_text(s) {
                    return true;
                }
            }
            crate::proxy::mappers::claude::models::MessageContent::Array(blocks) => {
                for b in blocks {
                    if let crate::proxy::mappers::claude::models::ContentBlock::Text { text } = b {
                        if crate::proxy::mappers::common_utils::is_post_compaction_continuation_text(
                            text,
                        ) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// 计算自动压缩的有效安全阈值：
/// 必须至少保留 target_limit + 25,000 的安全净空，防止压缩后上下文紧贴阈值瞬间再触发死循环 (autocompact_thrashing)
pub fn calculate_effective_auto_compact_threshold(user_threshold: u32, target_limit: u32) -> u32 {
    let min_safe_threshold = target_limit.saturating_add(25_000);
    user_threshold.max(50_000).max(min_safe_threshold)
}

/// 判定请求是否为用户在 Cowork 客户端输入的手动 ./compact 指令
fn is_manual_compact_command(request: &ClaudeRequest) -> bool {
    let mut text_opt = None;
    for m in request.messages.iter().rev() {
        if m.role == "user" {
            let t = match &m.content {
                MessageContent::String(s) => s.clone(),
                MessageContent::Array(blocks) => {
                    let mut s = String::new();
                    for b in blocks {
                        if let crate::proxy::mappers::claude::models::ContentBlock::Text { text } =
                            b
                        {
                            s.push_str(text);
                            s.push(' ');
                        }
                    }
                    s
                }
            };
            text_opt = Some(t);
            break;
        }
    }
    if let Some(text) = text_opt {
        static SYSTEM_REMINDER_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(r"(?s)<system-reminder>.*?</system-reminder>").unwrap()
        });
        let cleaned = SYSTEM_REMINDER_RE.replace_all(&text, "");
        for line in cleaned.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let lower = trimmed.to_lowercase();
            if lower == "./compact"
                || lower == ".\\compact"
                || lower == "./compact."
                || lower == "/compact"
                || lower.starts_with("./compact ")
            {
                return true;
            }
            break;
        }
    }
    false
}

/// 计算 Claude 请求中非对话历史的固定开销 (Fixed Overhead: System Prompt + Tool 声明)
/// 用于动态计算可解的目标上限 (target_limit = max(fixed_overhead + 15000, 35000))
fn calculate_claude_fixed_overhead(request: &ClaudeRequest) -> u32 {
    let mut overhead = 0u32;
    if let Some(sys) = &request.system {
        match sys {
            crate::proxy::mappers::claude::models::SystemPrompt::String(s) => {
                overhead += crate::proxy::pipeline::estimator::estimate_tokens_from_str(s);
            }
            crate::proxy::mappers::claude::models::SystemPrompt::Array(blocks) => {
                for block in blocks {
                    overhead +=
                        crate::proxy::pipeline::estimator::estimate_tokens_from_str(&block.text);
                }
            }
        }
    }
    if let Some(tools) = &request.tools {
        for tool in tools {
            let name_len =
                crate::proxy::pipeline::estimator::estimate_tokens_from_str(&tool.get_name());
            let desc_len = tool
                .description
                .as_deref()
                .map(crate::proxy::pipeline::estimator::estimate_tokens_from_str)
                .unwrap_or(0);
            let schema_len = tool
                .input_schema
                .as_ref()
                .map(crate::proxy::pipeline::estimator::estimate_tokens)
                .unwrap_or(0);
            overhead += name_len + desc_len + schema_len + 30;
        }
    }
    overhead
}

/// 构造 Anthropic 原生 SSE 流式响应体，回显真实 Compacted 结果
fn make_compact_sse_response(text: &str, model: &str, input_tokens: u32) -> String {
    let msg_id = format!(
        "msg_compact_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let mut out = String::new();

    let start_event = json!({
        "type": "message_start",
        "message": {
            "id": msg_id,
            "type": "message",
            "role": "assistant",
            "content": [],
            "model": model,
            "stop_reason": serde_json::Value::Null,
            "stop_sequence": serde_json::Value::Null,
            "usage": {
                "input_tokens": input_tokens.max(1),
                "output_tokens": 1
            }
        }
    });
    out.push_str(&format!(
        "event: message_start\ndata: {}\n\n",
        serde_json::to_string(&start_event).unwrap_or_default()
    ));

    let cbs_event = json!({
        "type": "content_block_start",
        "index": 0,
        "content_block": {"type": "text", "text": ""}
    });
    out.push_str(&format!(
        "event: content_block_start\ndata: {}\n\n",
        serde_json::to_string(&cbs_event).unwrap_or_default()
    ));

    let cbd_event = json!({
        "type": "content_block_delta",
        "index": 0,
        "delta": {"type": "text_delta", "text": text}
    });
    out.push_str(&format!(
        "event: content_block_delta\ndata: {}\n\n",
        serde_json::to_string(&cbd_event).unwrap_or_default()
    ));

    let cbst_event = json!({
        "type": "content_block_stop",
        "index": 0
    });
    out.push_str(&format!(
        "event: content_block_stop\ndata: {}\n\n",
        serde_json::to_string(&cbst_event).unwrap_or_default()
    ));

    let md_event = json!({
        "type": "message_delta",
        "delta": {"stop_reason": "end_turn", "stop_sequence": serde_json::Value::Null},
        "usage": {"output_tokens": 15}
    });
    out.push_str(&format!(
        "event: message_delta\ndata: {}\n\n",
        serde_json::to_string(&md_event).unwrap_or_default()
    ));

    out.push_str("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n");
    out
}

/// 构造 Anthropic 原生非流式 JSON 响应体
fn make_compact_json_response(text: &str, model: &str, input_tokens: u32) -> Value {
    let msg_id = format!(
        "msg_compact_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    json!({
        "id": msg_id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [{
            "type": "text",
            "text": text
        }],
        "stop_reason": "end_turn",
        "stop_sequence": serde_json::Value::Null,
        "usage": {
            "input_tokens": input_tokens.max(1),
            "output_tokens": 15
        }
    })
}

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
    // Priority: thinking.budget_tokens > thinking.budgetTokens > thinking.max_tokens > thinking.budget > thinkingConfig.thinkingBudget > reasoning.max_tokens
    if let Some(budget) = body
        .get("thinking")
        .and_then(|t| {
            t.get("budget_tokens")
                .or_else(|| t.get("budgetTokens"))
                .or_else(|| t.get("max_tokens"))
                .or_else(|| t.get("maxTokens"))
                .or_else(|| t.get("budget"))
        })
        .and_then(|b| b.as_u64())
    {
        hint.budget_tokens = Some(budget as u32);
    } else if let Some(budget) = body
        .get("thinkingConfig")
        .and_then(|t| {
            t.get("thinkingBudget")
                .or_else(|| t.get("thinking_budget"))
                .or_else(|| t.get("budget_tokens"))
                .or_else(|| t.get("budgetTokens"))
        })
        .and_then(|b| b.as_u64())
    {
        hint.budget_tokens = Some(budget as u32);
    } else if let Some(budget) = body
        .get("reasoning")
        .and_then(|r| {
            r.get("max_tokens")
                .or_else(|| r.get("maxTokens"))
                .or_else(|| r.get("budget_tokens"))
                .or_else(|| r.get("budgetTokens"))
        })
        .and_then(|b| b.as_u64())
    {
        hint.budget_tokens = Some(budget as u32);
    }

    // Try to extract level from thinkingLevel / reasoning_effort / output_config.effort / thinking.effort
    if let Some(level) = body
        .get("thinkingLevel")
        .or_else(|| body.get("thinking_level"))
        .or_else(|| body.get("reasoning_effort"))
        .or_else(|| body.get("reasoningEffort"))
        .or_else(|| body.get("output_config").and_then(|o| o.get("effort")))
        .or_else(|| body.get("thinking").and_then(|t| t.get("effort")))
        .and_then(|l| l.as_str())
    {
        hint.level = Some(level.to_lowercase());
    }

    hint
}

/// Map thinking level to suggested budget tokens
fn level_to_budget(level: &str, cap: u64) -> u32 {
    let base = match level {
        "minimal" => 1024,
        "low" => 8192,
        "medium" => 16384,
        "high" => 24576,
        _ => 8192, // default to low
    };
    base.min(cap as u32)
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
    budget_cap: u64, // [NEW]
) {
    let mut applied = false;

    // If budget is provided, set/override thinking config
    if let Some(budget) = hint.budget_tokens {
        let existing_effort = request
            .thinking
            .as_ref()
            .and_then(|t| t.effort.clone())
            .or_else(|| hint.level.clone());
        request.thinking = Some(crate::proxy::mappers::claude::models::ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(budget),
            effort: existing_effort,
        });
        tracing::debug!(
            "[{}] Applied thinking hint: budget_tokens={}",
            trace_id,
            budget
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
            let budget = level_to_budget(level, budget_cap);
            request.thinking = Some(crate::proxy::mappers::claude::models::ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(budget),
                effort: None,
            });
            tracing::debug!(
                "[{}] Applied thinking hint: level={} -> budget_tokens={}",
                trace_id,
                level,
                budget
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
const INTERNAL_BACKGROUND_TASK: &str = "internal-background-task"; // Unified virtual ID for all background tasks

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
use super::common::{apply_retry_strategy, should_rotate_account, RetryStrategy};

// ===== 退避策略模块结束 =====

#[cfg(test)]
mod variant_tests {
    use super::*;

    fn request_with_effort(model: &str, effort: &str, budget_tokens: u32) -> ClaudeRequest {
        serde_json::from_value(json!({
            "model": model,
            "messages": [{"role": "user", "content": "test"}],
            "thinking": {"type": "enabled", "budget_tokens": budget_tokens},
            "output_config": {"effort": effort}
        }))
        .expect("test request must deserialize")
    }

    #[test]
    fn applies_flash_low_effort_and_removes_output_config_before_serialization() {
        let mut request = request_with_effort("gemini-3.5-flash", "low", 10_000);
        let effort = crate::proxy::common::variant_mapping::tier_from_effort(
            request
                .output_config
                .as_ref()
                .and_then(|config| config.effort.as_deref()),
        );

        apply_variant(&mut request, effort, Some(10_000)).expect("Gemini 3.5 Flash must resolve");

        assert_eq!(request.model, "gemini-3.5-flash-extra-low");
        assert!(request.output_config.is_none());
        assert!(serde_json::to_value(request)
            .expect("resolved request must serialize")
            .get("output_config")
            .is_none());
    }

    #[test]
    fn applies_pro_high_effort_over_low_budget() {
        let mut request = request_with_effort("gemini-3.1-pro", "high", 1_000);
        let effort = crate::proxy::common::variant_mapping::tier_from_effort(
            request
                .output_config
                .as_ref()
                .and_then(|config| config.effort.as_deref()),
        );

        apply_variant(&mut request, effort, Some(1_000)).expect("Gemini 3.1 Pro must resolve");

        assert_eq!(request.model, "gemini-pro-agent");
    }

    #[test]
    fn invalid_effort_falls_back_to_budget_tokens_for_gemini_3_model() {
        // Given a Gemini 3 model ("gemini-3-flash") with an unrecognized
        // effort value ("unrecognized"), tier_from_effort returns None, so
        // apply_variant falls back to budget-based tier inference.
        let mut request = request_with_effort("gemini-3-flash", "unrecognized", 4_000);
        let effort = crate::proxy::common::variant_mapping::tier_from_effort(
            request
                .output_config
                .as_ref()
                .and_then(|config| config.effort.as_deref()),
        );

        // tier_from_effort(Some("unrecognized")) → None (invalid value)
        assert_eq!(effort, None);

        // With effort=None and budget=4_000, infer_tier → Medium →
        // resolve_with_tier("gemini-3-flash", None, Some(4_000)) →
        // SPEC_35_FLASH_LOW → physical id "gemini-3.5-flash-low"
        apply_variant(&mut request, effort, Some(4_000))
            .expect("gemini-3-flash must resolve even without valid effort");

        assert_eq!(request.model, "gemini-3.5-flash-low");
        assert!(request.output_config.is_none());
        // SPEC_35_FLASH_LOW has thinking_budget=4_000, preserve_client_budget=false
        assert_eq!(
            request.thinking.as_ref().and_then(|t| t.budget_tokens),
            Some(4_000)
        );
    }

    #[test]
    fn max_effort_maps_to_high_tier_for_gemini_3_flash() {
        let mut request = request_with_effort("gemini-3-flash", "max", 1_000);
        let effort = crate::proxy::common::variant_mapping::tier_from_effort(
            request
                .output_config
                .as_ref()
                .and_then(|config| config.effort.as_deref()),
        );
        assert_eq!(
            effort,
            Some(crate::proxy::common::variant_mapping::VariantTier::High)
        );

        apply_variant(&mut request, effort, Some(1_000))
            .expect("gemini-3-flash must resolve with max effort");

        assert_eq!(request.model, "gemini-3-flash-agent");
        assert_eq!(
            request.thinking.as_ref().and_then(|t| t.budget_tokens),
            Some(10_000)
        );
    }

    #[test]
    fn claude_model_without_variant_mapping_preserves_output_config_on_none() {
        // claude-sonnet-4-5 is NOT in GEMINI_FAMILIES and NOT in
        // resolve_non_variant_model, so resolve_with_tier returns None,
        // and apply_variant returns None without mutating the request.
        let mut request = request_with_effort("claude-sonnet-4-5", "high", 10_000);
        let effort = crate::proxy::common::variant_mapping::tier_from_effort(
            request
                .output_config
                .as_ref()
                .and_then(|config| config.effort.as_deref()),
        );

        let result = apply_variant(&mut request, effort, Some(10_000));
        assert!(
            result.is_none(),
            "unregistered Claude model must return None"
        );

        // Model and output_config must remain untouched.
        assert_eq!(request.model, "claude-sonnet-4-5");
        assert_eq!(
            request
                .output_config
                .as_ref()
                .and_then(|c| c.effort.as_deref()),
            Some("high")
        );
    }
}

fn apply_variant(
    request: &mut ClaudeRequest,
    effort_tier: Option<crate::proxy::common::variant_mapping::VariantTier>,
    client_budget: Option<u32>,
) -> Option<crate::proxy::common::variant_mapping::RealModelSpec> {
    let spec = crate::proxy::common::variant_mapping::resolve_with_tier(
        &request.model,
        effort_tier,
        client_budget,
    )?;

    request.model = spec.id.to_string();
    if spec.thinking_budget == 0 {
        request.thinking = None;
        request.tools = None;
    } else {
        request.thinking = Some(crate::proxy::mappers::claude::models::ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(spec.effective_thinking_budget(client_budget)),
            effort: None,
        });
    }
    request.output_config = None;
    request.max_tokens = Some(spec.max_output_tokens);

    Some(spec)
}

/// [FIX #3593] 检查 Claude 流数据块是否包含错误事件
/// 在 Peek 预读阶段识别流错误（如上游断连、错误帧），触发账号轮换重试，避免将错误误判为合法首包导致 500
fn claude_stream_chunk_has_error_event(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    let mut saw_error_event = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "event: error" {
            saw_error_event = true;
        } else if let Some(data) = trimmed.strip_prefix("data:") {
            if saw_error_event {
                return true;
            }
            if let Ok(payload) = serde_json::from_str::<Value>(data.trim()) {
                if payload.get("type").and_then(Value::as_str) == Some("error")
                    || payload.get("error").is_some_and(|e| !e.is_null())
                {
                    return true;
                }
            }
        } else if trimmed.is_empty() {
            saw_error_event = false;
        }
    }
    false
}

/// 处理 Claude messages 请求
///
/// 处理 Chat 消息请求流程
pub async fn handle_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    upstream_recorder: Option<
        axum::extract::Extension<crate::proxy::monitor::UpstreamRequestBodyHolder>,
    >,
    user_identity: Option<
        axum::extract::Extension<crate::proxy::middleware::auth::UserTokenIdentity>,
    >,
    Json(body): Json<Value>,
) -> Response {
    // [FIX] 保存原始请求体的完整副本，用于日志记录
    // 这确保了即使结构体定义遗漏字段，日志也能完整记录所有参数
    let original_body = body.clone();

    tracing::debug!(
        "handle_messages called. Body JSON len: {}",
        body.to_string().len()
    );

    // 生成随机 Trace ID 用户追踪
    let trace_id: String =
        rand::Rng::sample_iter(rand::thread_rng(), &rand::distributions::Alphanumeric)
            .take(6)
            .map(char::from)
            .collect::<String>()
            .to_lowercase();
    let debug_cfg = state.debug_logging.read().await.clone();

    // [NEW] Detect Client Adapter
    // 检查是否有匹配的客户端适配器（如 opencode）
    let client_adapter = CLIENT_ADAPTERS
        .iter()
        .find(|a| a.matches(&headers))
        .cloned();
    if let Some(_adapter) = &client_adapter {
        tracing::debug!(
            "[{}] Client Adapter detected: Applying custom strategies",
            trace_id
        );
    }

    // [CRITICAL REFACTOR] 优先解析请求以获取模型信息(用于智能兜底判断)
    let mut request: crate::proxy::mappers::claude::models::ClaudeRequest =
        match serde_json::from_value(body.clone()) {
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

    // 0. 自定义映射优先拦截 (用户自定义路由与热更新规则拥有最高优先级，避免被后续变体推断抹平原模型意图)
    let custom_target = {
        let custom_mapping = state.custom_mapping.read().await;
        crate::proxy::common::model_mapping::resolve_custom_model_route(
            &request.model,
            &*custom_mapping,
        )
    };
    if let Some(target) = custom_target {
        tracing::info!(
            "[{}] [CustomMapping] 命中用户自定义映射规则: {} -> {}",
            trace_id,
            request.model,
            target
        );
        request.model = target;
    }

    // [Variant] Resolve canonical model + variant → real model + real params.
    let model_lower = request.model.to_lowercase();
    let is_v3_or_above = model_specs::is_gemini_v3_or_above(&request.model);
    let is_explicit_tier_model = model_lower.ends_with("-high")
        || model_lower.ends_with("-medium")
        || model_lower.ends_with("-low")
        || model_lower.ends_with("-extra-low");

    let thinking_hint = extract_thinking_hint(&original_body);
    let tb_config = crate::proxy::config::get_thinking_budget_config();
    let is_client_control =
        tb_config.control_source == crate::proxy::config::ThinkingControlSource::Client;

    let client_switch = crate::proxy::pipeline::extract_client_thinking_switch(
        request.thinking.as_ref().map(|t| t.type_.as_str()),
        request
            .thinking
            .as_ref()
            .and_then(|t| t.budget_tokens.map(|b| b as u64)),
        request
            .output_config
            .as_ref()
            .and_then(|c| c.effort.as_deref())
            .or_else(|| request.thinking.as_ref().and_then(|t| t.effort.as_deref())),
    );
    let client_disabled = client_switch.is_disabled();

    let raw_client_budget = request.thinking.as_ref().and_then(|t| t.budget_tokens);

    // [USER RULE] 对于 Gemini >= 3 或显式指定档位的模型，进站阶段彻底忽略客户端思考与预算参数，绝不被客户端 1024 或 low 污染
    if is_client_control {
        if client_disabled {
            request.thinking = Some(crate::proxy::mappers::claude::models::ThinkingConfig {
                type_: "disabled".to_string(),
                budget_tokens: Some(0),
                effort: None,
            });
        }
    } else if is_v3_or_above || is_explicit_tier_model {
        // 无论客户端未提供 thinking，或者传了 disabled，只要是 3+ 或显式模型，强制矫正为 enabled，清理客户端 budget_tokens
        let effort_in_thinking = request.thinking.as_ref().and_then(|t| t.effort.clone());
        request.thinking = Some(crate::proxy::mappers::claude::models::ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: None,
            effort: effort_in_thinking,
        });
    } else {
        // 由于此时还没拿到账号，先用模型默认限额兜底
        let temp_cap = model_specs::get_thinking_budget(&request.model, None);
        apply_thinking_hints(&mut request, &thinking_hint, &trace_id, temp_cap);
    }

    // [USER RULE] 对显式指定档位或 Gemini >= 3 的思考模型，进站阶段彻底忽略客户端思考预算，绝不参与档位推断
    let effective_budget_hint = if !is_client_control && (is_explicit_tier_model || is_v3_or_above)
    {
        None
    } else {
        original_body
            .get("thinking")
            .and_then(|t| t.get("budget_tokens"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
    };

    // 当客户端显式关闭思考或预算为 0 时（如 Claude Desktop 自动模式安全门禁），意图对齐至 low 档位，确保命中低开销模型
    let effort_hint = if client_disabled {
        Some("low".to_string())
    } else {
        request
            .output_config
            .as_ref()
            .and_then(|config| config.effort.clone())
            .or_else(|| request.thinking.as_ref().and_then(|t| t.effort.clone()))
            .or_else(|| thinking_hint.level.clone())
    };
    let effort_tier =
        crate::proxy::common::variant_mapping::tier_from_effort(effort_hint.as_deref());
    let canonical_model = request.model.clone();
    if let Some(spec) = apply_variant(&mut request, effort_tier, effective_budget_hint) {
        if is_client_control && client_disabled {
            request.thinking = Some(crate::proxy::mappers::claude::models::ThinkingConfig {
                type_: "disabled".to_string(),
                budget_tokens: Some(0),
                effort: None,
            });
        } else if is_client_control && raw_client_budget.is_some() {
            request.thinking = Some(crate::proxy::mappers::claude::models::ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: raw_client_budget,
                effort: effort_hint.clone(),
            });
        } else if is_client_control {
            // [CRITICAL FIX] 客户端控制模式下，客户端未传数字预算（全缺省或仅传等级）
            // 严禁保留 apply_variant 内部赋予的 spec.thinking_budget (4000)！保持真实客户端状态
            if let Some(ref mut t) = request.thinking {
                t.budget_tokens = None;
            }
        }
        tracing::info!(
            "[{}] [Variant] canonical='{}' effort_hint={:?} budget_hint={:?} -> real_model='{}' budget={} maxOut={}",
            trace_id, canonical_model, effort_hint, effective_budget_hint, spec.id, spec.thinking_budget, spec.max_output_tokens
        );
    }

    if debug_logger::is_enabled(&debug_cfg) {
        // [FIX] 使用原始 body 副本记录日志，确保不丢失任何字段
        let original_payload = json!({
            "kind": "original_request",
            "protocol": "anthropic",
            "trace_id": trace_id,
            "original_model": request.model,
            "request": crate::proxy::payload_audit::reorder_payload_fields(&original_body),  // 原始请求体（字段按关注度重排），不是结构体序列化
        });
        debug_logger::write_debug_payload(
            &debug_cfg,
            Some(&trace_id),
            "original_request",
            &original_payload,
        )
        .await;
    }

    // [Stage 1 Timing] 初始会话清洗计时
    let clean_start = std::time::Instant::now();

    // [CRITICAL FIX] 预先清理所有消息中的 cache_control 字段 (Issue #744)
    clean_cache_control_from_messages(&mut request.messages);

    // [FIX #813] 合并连续的同角色消息 (Consecutive User Messages)
    merge_consecutive_messages(&mut request.messages);

    // Get model family for signature validation
    let mapped_model =
        crate::proxy::common::model_mapping::map_claude_model_to_gemini(&request.model);
    let target_family = if mapped_model.contains("gemini") {
        Some("gemini")
    } else {
        Some("claude")
    };

    // [CRITICAL FIX] 过滤并修复 Thinking 块签名 (Enhanced with family check)
    filter_invalid_thinking_blocks_with_family(&mut request.messages, target_family);

    // [FIX Prompt-Cache] 严禁在正常请求路径中注入合成消息 (close_tool_loop_for_thinking)！
    // Claude Code 客户端按规范不会在后续轮次中回传历史 thinking 块。
    // InboundThinkingPipeline 与 ThinkingStore 会在转译为 Google contents 时自动恢复真实思考块和加密签名，
    // finalize_gemini_contents_thinking 亦具备完整的首位思考块与哨兵兜底。
    // 若在此处注入 "[System: Tool execution completed...]" 等合成消息，会导致对话历史前缀在轮次间突变，
    // 进而彻底破坏 Google Gemini 上游的 Prompt Caching（缓存崩塌）。

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

    // [NEW] 获取上下文控制配置
    let experimental = state.experimental.read().await;
    let scaling_enabled = experimental.enable_usage_scaling;

    // [全链路会话生命周期与自愈分流体系 (Pipeline First)]
    // 提取会话唯一标识（优先提取产品专属会话头或内容锚点）
    let session_key =
        crate::proxy::thinking_store::stable_session_winner(&headers, Some(&original_body), None)
            .unwrap_or_else(|| {
                let anchor =
                    crate::proxy::session_manager::SessionManager::extract_session_id(&request);
                crate::proxy::thinking_store::derive_winner_session_id("anon", None, &anchor)
            });

    // 维持状态机容量，执行条目级细粒度 TTL 清理，彻底避免粗暴全量 clear() 导致的 herd invalidation
    if COMPACTION_IMMUNITY_LEASES.len() > 1000 {
        let now = std::time::Instant::now();
        COMPACTION_IMMUNITY_LEASES.retain(|_, lease| {
            if !lease.consumed {
                now.duration_since(lease.created_at).as_secs()
                    < COMPACTION_IMMUNITY_INITIAL_TTL_SECS
            } else {
                now.duration_since(lease.last_touched).as_secs() < COMPACTION_IMMUNITY_WINDOW_SECS
            }
        });
    }

    // 分流 A: 客户端原生发起的压缩总结请求 (Compaction Summary Request) -> 生命线直通放行，绝对不误杀
    let is_compaction_header = headers
        .get("x-stainless-helper")
        .and_then(|h| h.to_str().ok())
        .map_or(false, |v| v.contains("compaction"));

    // 检查是否有任何消息或 System Prompt 命中摘要特征
    let has_compaction_message = request.messages.iter().rev().take(5).any(|m| {
        let text = match &m.content {
            crate::proxy::mappers::claude::models::MessageContent::String(s) => s.as_str(),
            crate::proxy::mappers::claude::models::MessageContent::Array(blocks) => blocks
                .iter()
                .rev()
                .find_map(|b| match b {
                    crate::proxy::mappers::claude::models::ContentBlock::Text { text } => {
                        Some(text.as_str())
                    }
                    _ => None,
                })
                .unwrap_or(""),
        };
        crate::proxy::mappers::common_utils::is_compaction_request_text(text)
    });

    let has_compaction_system = request.system.as_ref().map_or(false, |sys| {
        let sys_text = match sys {
            crate::proxy::mappers::claude::models::SystemPrompt::String(s) => s.as_str(),
            crate::proxy::mappers::claude::models::SystemPrompt::Array(arr) => {
                arr.first().map(|b| b.text.as_str()).unwrap_or("")
            }
        };
        crate::proxy::mappers::common_utils::is_compaction_request_text(sys_text)
    });

    let is_compaction_request =
        is_compaction_header || has_compaction_message || has_compaction_system;

    if is_compaction_request {
        // [Compaction Immunity Lease] 为该会话发放免死租约，支持并发接续与重试
        let now = std::time::Instant::now();
        COMPACTION_IMMUNITY_LEASES.insert(
            session_key.clone(),
            CompactionImmunityLease {
                created_at: now,
                last_touched: now,
                consumed: false,
            },
        );

        // 跨轮次关联对齐：遍历近期处于 compacting 状态的会话，同步发放租约并确认为已完成摘要
        // 解决客户端摘要请求因 tools/system 变动导致 extract_session_id 发生哈希偏移的问题
        PENDING_COMPACT_SESSIONS.retain(|_, ts| now.duration_since(*ts).as_secs() < 120);
        for entry in PENDING_COMPACT_SESSIONS.iter() {
            let pending_sid = entry.key();
            COMPACTION_IMMUNITY_LEASES.insert(
                pending_sid.clone(),
                CompactionImmunityLease {
                    created_at: now,
                    last_touched: now,
                    consumed: false,
                },
            );
            if let Some(mut state) = COWORK_COMPACT_SESSIONS.get_mut(pending_sid) {
                state.summary_done = true;
            }
        }

        if let Some(mut state) = COWORK_COMPACT_SESSIONS.get_mut(&session_key) {
            state.summary_done = true;
        }
        tracing::info!(
            "[{}] [Lifecycle] Compaction summary request detected for session {}, issued immunity lease",
            trace_id, session_key
        );
    }

    // 检查是否包含接续标记 (Post-Compaction Continuation)
    let is_continuation_detected = detect_post_compaction_continuation(&request);

    // 分流 B: 已完成压缩提纯的会话接续 (Post-Compaction Continuation)
    // 采用代际租约状态机 (CompactionImmunityLease):
    // 1. 严格守卫 !is_compaction_request：压缩摘要请求自身绝对不自毁刚刚发放的免死租约；
    // 2. 首个接续请求接入后激活 30 秒滑动窗口，窗口内放行该会话的所有并发请求、工具调用与网络重试；
    // 3. 空闲超过 30 秒后即刻核销失效，后续若再次超限将正常触发下一轮自愈，彻底杜绝终身免死 (Fixes #3563)。
    let is_post_compaction = if !is_compaction_request {
        if let Some(mut lease) = COMPACTION_IMMUNITY_LEASES.get_mut(&session_key) {
            let now = std::time::Instant::now();
            let valid = if !lease.consumed {
                now.duration_since(lease.created_at).as_secs()
                    < COMPACTION_IMMUNITY_INITIAL_TTL_SECS
            } else {
                now.duration_since(lease.last_touched).as_secs() < COMPACTION_IMMUNITY_WINDOW_SECS
            };
            if valid {
                lease.consumed = true;
                lease.last_touched = now;
                tracing::info!(
                    "[{}] [Lifecycle] Active immunity lease for session {}, granted 1M passthrough (sliding window active)",
                    trace_id, session_key
                );
                true
            } else {
                drop(lease);
                COMPACTION_IMMUNITY_LEASES.remove(&session_key);
                false
            }
        } else if is_continuation_detected {
            // 接续标记保底：若检测到接续标记但无活跃租约，发放滑动窗口租约
            let now = std::time::Instant::now();
            COMPACTION_IMMUNITY_LEASES.insert(
                session_key.clone(),
                CompactionImmunityLease {
                    created_at: now,
                    last_touched: now,
                    consumed: true,
                },
            );
            tracing::info!(
                "[{}] [Lifecycle] Continuation detected for session {}, minted sliding window lease",
                trace_id, session_key
            );
            true
        } else {
            false
        }
    } else {
        false
    };

    // 若检测到已进入接续阶段且并非摘要请求自身，从统一压缩池中回收结算并归入防重放缓存
    if (is_post_compaction || is_continuation_detected) && !is_compaction_request {
        if let Some((_, state)) = COWORK_COMPACT_SESSIONS.remove(&session_key) {
            let est_tokens = crate::proxy::pipeline::estimate_tokens(&original_body);
            let saved_tok = state.before_tokens.saturating_sub(est_tokens);
            let saved_k = (saved_tok as f64 / 1000.0).round() as u32;
            let reply_text = if saved_k > 0 {
                format!("Compacted conversation · saved {}k tokens", saved_k)
            } else {
                "Compacted conversation".to_string()
            };
            PENDING_COMPACT_SESSIONS.remove(&session_key);
            prune_just_compacted_cache_if_needed();
            COWORK_JUST_COMPACTED_CACHE.insert(
                session_key.clone(),
                (std::time::Instant::now(), reply_text, est_tokens),
            );
            tracing::info!(
                "[{}] [Lifecycle] Harvested completed {:?} compact for session {} ({} -> {} tokens, saved {}k)",
                trace_id, state.kind, session_key, state.before_tokens, est_tokens, saved_k
            );
        }
    }

    // 检查是否为手动 ./compact 穿透指令
    let is_manual_compact = is_manual_compact_command(&request);

    // 检查是否正处于压缩流程中（手动或自动），若是则自动豁免 auto_compact 门禁，彻底解耦双重 400 撞车！
    let is_in_compaction = is_session_in_compaction(&session_key);

    // 分流 C (优先分流): 手动 ./compact 指令拦截与闭环响应 (具有最高调度优先级，彻底短路自动门禁抢跑)
    if experimental.enable_cowork_manual_compact && is_manual_compact {
        let now = std::time::Instant::now();
        let req_model = request.model.clone();
        let est_tokens = crate::proxy::pipeline::estimate_tokens(&original_body);

        // 情况 A: 60秒内刚完成过 compact，命中防重放缓存
        if let Some(entry) = COWORK_JUST_COMPACTED_CACHE.get(&session_key) {
            let (cached_ts, ref cached_text, cached_tokens) = *entry;
            if now.duration_since(cached_ts).as_secs() < 60 {
                tracing::info!(
                    "[{}] [Manual-Compact] Cache hit for session {}, returning: {}",
                    trace_id,
                    session_key,
                    cached_text
                );
                if request.stream {
                    let sse = make_compact_sse_response(cached_text, &req_model, cached_tokens);
                    return (
                        StatusCode::OK,
                        [
                            (header::CONTENT_TYPE, "text/event-stream; charset=utf-8"),
                            (header::CACHE_CONTROL, "no-cache"),
                            (header::CONNECTION, "keep-alive"),
                        ],
                        sse,
                    )
                        .into_response();
                } else {
                    let json_val =
                        make_compact_json_response(cached_text, &req_model, cached_tokens);
                    return (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/json")],
                        Json(json_val),
                    )
                        .into_response();
                }
            } else {
                drop(entry);
                COWORK_JUST_COMPACTED_CACHE.remove(&session_key);
            }
        }

        // 情况 B: 判断是否真正完成了压缩并处于重试阶段
        let mut is_truly_compacted = false;
        let mut before_tok = 0u32;

        if let Some(entry) = COWORK_COMPACT_SESSIONS.get(&session_key) {
            // 防死锁：超过 300 秒过期自动重置
            if now.duration_since(entry.ts).as_secs() >= 300 {
                drop(entry);
                COWORK_COMPACT_SESSIONS.remove(&session_key);
            } else {
                before_tok = entry.before_tokens;
                let summary_already_done = entry.summary_done;

                // 条件: 摘要请求已完成，或带接续标记/豁免，或上下文 tokens 明显回落（回落至 85% 以下）
                if summary_already_done
                    || is_post_compaction
                    || is_continuation_detected
                    || (before_tok > 0 && est_tokens < (before_tok * 85 / 100))
                {
                    is_truly_compacted = true;
                }
            }
        }

        if is_truly_compacted {
            let saved_tok = before_tok.saturating_sub(est_tokens);
            let saved_k = (saved_tok as f64 / 1000.0).round() as u32;
            let reply_text = if saved_k > 0 {
                format!("Compacted conversation · saved {}k tokens", saved_k)
            } else {
                "Compacted conversation".to_string()
            };

            COWORK_COMPACT_SESSIONS.remove(&session_key);
            PENDING_COMPACT_SESSIONS.remove(&session_key);
            prune_just_compacted_cache_if_needed();
            COWORK_JUST_COMPACTED_CACHE
                .insert(session_key.clone(), (now, reply_text.clone(), est_tokens));
            COMPACTION_IMMUNITY_LEASES.remove(&session_key);

            tracing::info!(
                "[{}] [Manual-Compact] Successfully compacted for session {} ({} -> {} tokens, saved {}k)! Returning 200 OK",
                trace_id, session_key, before_tok, est_tokens, saved_k
            );

            if request.stream {
                let sse = make_compact_sse_response(&reply_text, &req_model, est_tokens);
                return (
                    StatusCode::OK,
                    [
                        (header::CONTENT_TYPE, "text/event-stream; charset=utf-8"),
                        (header::CACHE_CONTROL, "no-cache"),
                        (header::CONNECTION, "keep-alive"),
                    ],
                    sse,
                )
                    .into_response();
            } else {
                let json_val = make_compact_json_response(&reply_text, &req_model, est_tokens);
                return (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/json")],
                    Json(json_val),
                )
                    .into_response();
            }
        }

        // 情况 C: 初次捕获 ./compact 指令，或处于客户端网络级即时重试阶段
        // 持续响应 400 假报警，直到驱动客户端彻底触发 Reactive Compact
        prune_compact_sessions_if_needed();
        let fixed_overhead = calculate_claude_fixed_overhead(&request);
        let target_limit = (fixed_overhead + 15_000).max(35_000);

        if !COWORK_COMPACT_SESSIONS.contains_key(&session_key) {
            COWORK_COMPACT_SESSIONS.insert(
                session_key.clone(),
                CoworkCompactState {
                    kind: CoworkCompactKind::Manual,
                    before_tokens: est_tokens,
                    ts: now,
                    summary_done: false,
                    target_limit,
                },
            );
        } else if let Some(mut entry) = COWORK_COMPACT_SESSIONS.get_mut(&session_key) {
            entry.ts = now;
            if entry.before_tokens == 0 {
                entry.before_tokens = est_tokens;
            }
            entry.target_limit = target_limit;
        }
        PENDING_COMPACT_SESSIONS.insert(session_key.clone(), now);

        // 防御性校验：若当前 tokens 已经处于 target_limit 之内，直接返回 200 成功响应，
        // 绝不发射 400 假报警，彻底杜绝客户端 Fst / fIt 算出负/零 initialTokenGap 触发 compactionImpossible
        if est_tokens <= target_limit {
            let reply_text = "Compacted conversation · already at minimal context".to_string();
            tracing::info!(
                "[{}] [Manual-Compact] Session {} context ({} tokens) is already <= target_limit ({}), returning 200 OK directly",
                trace_id, session_key, est_tokens, target_limit
            );
            if request.stream {
                let sse = make_compact_sse_response(&reply_text, &req_model, est_tokens);
                return (
                    StatusCode::OK,
                    [
                        (header::CONTENT_TYPE, "text/event-stream; charset=utf-8"),
                        (header::CACHE_CONTROL, "no-cache"),
                        (header::CONNECTION, "keep-alive"),
                    ],
                    sse,
                )
                    .into_response();
            } else {
                let json_val = make_compact_json_response(&reply_text, &req_model, est_tokens);
                return (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/json")],
                    Json(json_val),
                )
                    .into_response();
            }
        }

        let report_tokens = est_tokens.max(target_limit + 10_000);
        let err_msg = format!(
            "prompt is too long: {} tokens > {} maximum",
            report_tokens, target_limit
        );
        tracing::warn!(
            "[{}] [Manual-Compact] Intercepted ./compact for session {} (tokens={}, fixed_overhead={}), responding with 400 fake alarm (gap target: {} tokens) to trigger deep client-side compact",
            trace_id, session_key, est_tokens, fixed_overhead, target_limit
        );

        return (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "application/json")],
            Json(json!({
                "type": "error",
                "error": {
                    "type": "invalid_request_error",
                    "message": err_msg
                }
            })),
        )
            .into_response();
    }

    // 分流 D: 超限自愈假报警触发门禁 (必须自定义开启 + 双重确权 + 非手动指令 + 非正在压缩中)
    // 铁律：普通 Agent 与未开启配置时，绝对不拦截，100% 享受 Gemini 百万超长上下文！
    if experimental.enable_cowork_auto_compact
        && !is_compaction_request
        && !is_post_compaction
        && !is_continuation_detected
        && !is_manual_compact
        && !is_in_compaction
    {
        let is_cowork = request.tools.as_ref().map_or(false, |tools| {
            tools.iter().any(|t| {
                let n = t.get_name();
                n.starts_with("mcp__cowork") || n.starts_with("mcp__workspace")
            })
        });

        if is_cowork {
            let fixed_overhead = calculate_claude_fixed_overhead(&request);
            let target_limit = (fixed_overhead + 15_000).max(35_000);
            let effective_threshold = calculate_effective_auto_compact_threshold(
                experimental.cowork_compact_threshold,
                target_limit,
            );
            let est_tokens = crate::proxy::pipeline::estimate_tokens(&original_body);

            // 负 Gap 与净空防御门禁：必须满足 est_tokens >= effective_threshold 且 est_tokens > target_limit
            if est_tokens >= effective_threshold && est_tokens > target_limit {
                tracing::warn!(
                    "[{}] [Cowork-Gatekeeper] Cowork session reached {} tokens >= effective threshold {} (target_limit: {}), triggering native reactive compact",
                    trace_id,
                    est_tokens,
                    effective_threshold,
                    target_limit
                );
                prune_compact_sessions_if_needed();
                let now = std::time::Instant::now();
                COWORK_COMPACT_SESSIONS.insert(
                    session_key.clone(),
                    CoworkCompactState {
                        kind: CoworkCompactKind::Auto,
                        before_tokens: est_tokens,
                        ts: now,
                        summary_done: false,
                        target_limit,
                    },
                );
                PENDING_COMPACT_SESSIONS.insert(session_key.clone(), now);
                let report_tokens = est_tokens.max(target_limit + 10_000);
                let err_msg = format!(
                    "prompt is too long: {} tokens > {} maximum",
                    report_tokens, target_limit
                );
                return (
                    StatusCode::BAD_REQUEST,
                    [("content-type", "application/json")],
                    Json(json!({
                        "type": "error",
                        "error": {
                            "type": "invalid_request_error",
                            "message": err_msg
                        }
                    })),
                )
                    .into_response();
            }
        }
    }

    if is_compaction_request {
        tracing::info!(
            "[{}] [Lifecycle] Compaction summary request detected, passing through to upstream",
            trace_id
        );
    } else if is_post_compaction {
        tracing::debug!(
            "[{}] [Lifecycle] Post-compaction continuation session detected, granted upstream 1M immunity",
            trace_id
        );
    }

    // 获取最新一条“有意义”的消息内容（用于日志记录和后台任务检测）
    // 策略：反向遍历，首先筛选出所有角色为 "user" 的消息，然后从中找到第一条非 "Warmup" 且非空的文本消息
    // 获取最新一条“有意义”的消息内容（用于日志记录和后台任务检测）
    // 策略：反向遍历，首先筛选出所有和用户相关的消息 (role="user")
    // 然后提取其文本内容，跳过 "Warmup" 或系统预设的 reminder
    let meaningful_msg = request
        .messages
        .iter()
        .rev()
        .filter(|m| m.role == "user")
        .find_map(|m| {
            let content = match &m.content {
                crate::proxy::mappers::claude::models::MessageContent::String(s) => s.to_string(),
                crate::proxy::mappers::claude::models::MessageContent::Array(arr) => {
                    // 对于数组，提取所有 Text 块并拼接，忽略 ToolResult
                    arr.iter()
                        .filter_map(|block| match block {
                            crate::proxy::mappers::claude::models::ContentBlock::Text { text } => {
                                Some(text.as_str())
                            }
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
        request
            .messages
            .last()
            .map(|m| match &m.content {
                crate::proxy::mappers::claude::models::MessageContent::String(s) => s.clone(),
                crate::proxy::mappers::claude::models::MessageContent::Array(_) => {
                    "[Complex/Tool Message]".to_string()
                }
            })
            .unwrap_or_else(|| "[No Messages]".to_string())
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
    debug!(
        "========== [{}] CLAUDE REQUEST DEBUG START ==========",
        trace_id
    );
    debug!("[{}] Model: {}", trace_id, request.model);
    debug!("[{}] Stream: {}", trace_id, request.stream);
    debug!("[{}] Max Tokens: {:?}", trace_id, request.max_tokens);
    debug!("[{}] Temperature: {:?}", trace_id, request.temperature);
    debug!("[{}] Message Count: {}", trace_id, request.messages.len());
    debug!("[{}] Has Tools: {}", trace_id, request.tools.is_some());
    debug!(
        "[{}] Has Thinking Config: {}",
        trace_id,
        request.thinking.is_some()
    );
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
            }
            crate::proxy::mappers::claude::models::MessageContent::Array(arr) => {
                format!("[Array with {} blocks]", arr.len())
            }
        };
        debug!(
            "[{}] Message[{}] - Role: {}, Content: {}",
            trace_id, idx, msg.role, content_preview
        );
    }

    debug!(
        "[{}] Full Claude Request JSON: {}",
        trace_id,
        serde_json::to_string_pretty(&request).unwrap_or_default()
    );
    debug!(
        "========== [{}] CLAUDE REQUEST DEBUG END ==========",
        trace_id
    );

    // 1. 获取 会话 ID (已废弃基于内容的哈希，改用 TokenManager 内部的时间窗口锁定)
    let _session_id: Option<&str> = None;

    // 2. 获取 UpstreamClient
    let upstream = state.upstream.clone();

    // 3. 准备闭包
    let mut request_for_body = request.clone();
    let token_manager = state.token_manager;

    let pool_size = token_manager.len();
    // [FIX #3485] 自适应多账号池与单账号退避最大重试次数 (单账号3次，多账号整池两轮)
    let max_attempts = super::common::calculate_max_retry_attempts(pool_size);

    let mut last_error = String::new();
    let mut retried_without_thinking = false;
    let mut last_email: Option<String> = None;
    let mut last_mapped_model: Option<String> = None;
    let mut last_status = StatusCode::SERVICE_UNAVAILABLE; // Default to 503 if no response reached
    let mut force_rotate = false;

    // [Stage Timing] 阶段耗时度量变量 (毫秒，保留微秒级浮点精度)
    let clean_micros = clean_start.elapsed().as_micros() as u64;
    let clean_ms: f64 = clean_micros as f64 / 1000.0;
    let mut norm_ms: f64 = 0.0;
    let mut think_fill_ms: f64 = 0.0;
    let mut ttft_ms: f64 = 0.0;

    for attempt in 0..max_attempts {
        // [Stage 2 Timing] 中转归一计时起点
        let norm_start = std::time::Instant::now();

        // 2. 模型路由解析
        let mapped_model = crate::proxy::common::model_mapping::resolve_model_route_with_effort(
            &request_for_body.model,
            &*state.custom_mapping.read().await,
            effort_hint.as_deref(),
        );
        last_mapped_model = Some(mapped_model.clone());

        // 将 Claude 工具转为 Value 数组以便探测联网
        let tools_val: Option<Vec<Value>> = request_for_body.tools.as_ref().map(|list| {
            list.iter()
                .map(|t| serde_json::to_value(t).unwrap_or(json!({})))
                .collect()
        });

        let config = crate::proxy::mappers::common_utils::resolve_request_config(
            &request_for_body.model,
            &mapped_model,
            &tools_val,
            request.size.as_deref(),    // [NEW] Pass size parameter
            request.quality.as_deref(), // [NEW] Pass quality parameter
            None,                       // image_size
            None,                       // body
        );

        // 内容锚点只进思维库。账号粘性与上游 sessionId 用 affinity_key。
        let anchor =
            crate::proxy::session_manager::SessionManager::extract_session_id(&request_for_body);
        let session_scope = crate::proxy::thinking_store::SessionScope::resolve(
            &headers,
            Some(&original_body),
            None,
            anchor,
            user_identity
                .as_ref()
                .map(|identity| identity.token_id.as_str()),
        );
        let store_key = session_scope.store_key.clone();
        let affinity_key = session_scope.affinity_key.clone();
        let client_session_id = session_scope.client_id.clone();
        let session_id = Some(affinity_key.as_str());

        let mut token_result = token_manager
            .get_token(
                &config.request_type,
                force_rotate,
                session_id,
                &config.final_model,
            )
            .await;

        if let Err(ref e) = token_result {
            if crate::proxy::handlers::common::is_transient_token_error(e) {
                tracing::warn!(
                    "Token acquisition transient error ({}), retrying once with force_rotate...",
                    e
                );
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                token_result = token_manager
                    .get_token(&config.request_type, true, session_id, &config.final_model)
                    .await;
            }
        }

        let (access_token, project_id, email, account_id, _wait_ms) = match token_result {
            Ok(t) => t,
            Err(e) => {
                let safe_message = if e.contains("invalid_grant") {
                    "OAuth refresh failed (invalid_grant): refresh_token likely revoked/expired; reauthorize account(s) to restore service.".to_string()
                } else {
                    e
                };
                let headers = crate::proxy::handlers::common::build_token_error_headers(
                    Some(mapped_model.as_str()),
                    None,
                    &safe_message,
                );
                let dual_err = crate::proxy::handlers::common::build_dual_track_error(
                    "claude",
                    StatusCode::SERVICE_UNAVAILABLE.as_u16(),
                    mapped_model.as_str(),
                    &safe_message,
                );
                return (StatusCode::SERVICE_UNAVAILABLE, headers, Json(dual_err)).into_response();
            }
        };

        last_email = Some(email.clone());
        info!("✓ Using account: {} (type: {})", email, config.request_type);

        // 方案 A：移除后台任务静默降级策略，请求直通客户端指定的模型，与 OpenAI 协议保持一致
        let mut request_with_mapped = request_for_body.clone();

        // [FIX] Estimate AFTER purification to get accurate token count for calibrator learning
        let raw_estimated = ContextManager::estimate_token_usage(&request_with_mapped);

        request_with_mapped.model = mapped_model.clone();

        // 生成 Trace ID (简单用时间戳后缀)
        // let _trace_id = format!("req_{}", chrono::Utc::now().timestamp_subsec_millis());

        let token_obj = token_manager.get_token_by_id(&account_id);
        let (mut gemini_body, transform_timing) =
            match crate::proxy::mappers::claude::transform_claude_request_in_timed(
                &request_with_mapped,
                &project_id,
                retried_without_thinking,
                Some(account_id.as_str()),
                &store_key,
                &affinity_key,
                token_obj.as_ref(),
            ) {
                Ok((b, timing)) => {
                    debug!(
                        "[{}] Transformed Gemini Body: {}",
                        trace_id,
                        serde_json::to_string_pretty(&b).unwrap_or_default()
                    );
                    (b, timing)
                }
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
                        })),
                    )
                        .into_response();
                }
            };

        let _ =
            crate::proxy::mappers::context_manager::ContextManager::apply_post_transit_context_mgmt(
                &mut gemini_body,
                &mapped_model,
            );
        crate::proxy::mappers::prompt_sanitizer::PromptSanitizer::sanitize_gemini_payload(
            &mut gemini_body,
        );
        crate::proxy::mappers::common_utils::ensure_gemini_payload_ends_with_user(&mut gemini_body);

        let norm_total_micros = norm_start.elapsed().as_micros() as u64;
        let tf_micros = transform_timing.think_fill_micros;
        norm_ms = norm_total_micros.saturating_sub(tf_micros) as f64 / 1000.0;
        think_fill_ms = tf_micros as f64 / 1000.0;

        if let Some(ref recorder) = upstream_recorder {
            recorder.set_value(&gemini_body);
        }

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
            debug_logger::write_debug_payload(
                &debug_cfg,
                Some(&trace_id),
                "v1internal_request",
                &payload,
            )
            .await;
        }

        // 4. 上游调用 - 自动转换逻辑
        let client_wants_stream = request.stream;
        // [AUTO-CONVERSION] 非 Stream 请求自动转换为 Stream 以享受更宽松的配额
        let force_stream_internally = !client_wants_stream;
        let actual_stream = client_wants_stream || force_stream_internally;

        if force_stream_internally {
            info!(
                "[{}] 🔄 Auto-converting non-stream request to stream for better quota",
                trace_id
            );
        }

        let method = if actual_stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        let query = if actual_stream { Some("alt=sse") } else { None };
        // [FIX #765/1522] Prepare Robust Beta Headers for Claude models
        let mut extra_headers = std::collections::HashMap::new();
        extra_headers.insert("x-session-id".to_string(), client_session_id.clone());
        if mapped_model.to_lowercase().contains("claude") {
            extra_headers.insert(
                "anthropic-beta".to_string(),
                "claude-code-20250219".to_string(),
            );
            tracing::debug!(
                "[{}] Added Comprehensive Beta Headers for Claude model",
                trace_id
            );
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

        // [Stage 4 Timing] 等待谷歌上游首包计时起点
        let upstream_req_start = std::time::Instant::now();

        let call_result = match upstream
            .call_v1_internal_with_headers(
                method,
                &access_token,
                gemini_body.clone(),
                query,
                extra_headers.clone(),
                Some(account_id.as_str()),
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_error = e.clone();
                debug!(
                    "Request failed on attempt {}/{}: {}",
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
                "protocol": "anthropic",
                "trace_id": trace_id,
                "original_model": request.model,
                "mapped_model": request_with_mapped.model,
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
        last_status = status;

        // 成功
        if status.is_success() {
            token_manager.commit_session(&affinity_key, &account_id);
            // [智能限流] 请求成功，重置该账号的连续失败计数
            token_manager.mark_account_success(&account_id);

            // Determine context limit based on model
            let context_limit = crate::proxy::mappers::claude::utils::get_context_limit_for_model(
                &request_with_mapped.model,
            );

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

                // [Auto-Heal] 纯思考空回复流式自愈门禁 (Pipeline First)
                let auto_heal_ctx = crate::proxy::pipeline::auto_heal::ThinkingAutoHealContext {
                    upstream: upstream.clone(),
                    method,
                    access_token: access_token.clone(),
                    original_body: gemini_body.clone(),
                    query_string: query,
                    extra_headers: extra_headers.clone(),
                    account_id: Some(account_id.clone()),
                    trace_id: trace_id.clone(),
                };
                let gemini_stream =
                    crate::proxy::pipeline::auto_heal::wrap_stream_with_empty_thinking_auto_heal(
                        Box::pin(gemini_stream),
                        auto_heal_ctx,
                    );

                let current_message_count = request_with_mapped.messages.len();

                // [FIX #MCP] Extract registered tool names for MCP fuzzy matching
                let registered_tool_names: Vec<String> = request_with_mapped
                    .tools
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
                    Some(store_key.clone()),
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
                    match tokio::time::timeout(
                        // [FIX #Bug1] Reduced from 300s to 30s.
                        // Gemini sends first chunk within 5s normally; 30s allows for retries
                        // without causing the 5-minute hang users observed.
                        std::time::Duration::from_secs(30),
                        claude_stream.next(),
                    )
                    .await
                    {
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

                            // [FIX #3593] 识别 Peek 阶段的错误事件，触发账号轮换重试而非误判为首包
                            if claude_stream_chunk_has_error_event(&bytes) {
                                tracing::warn!(
                                    "[{}] Error event detected during peek: {}, retrying...",
                                    trace_id,
                                    text.trim()
                                );
                                last_error = format!("Error event during peek: {}", text.trim());
                                retry_this_account = true;
                                break;
                            }

                            // We found real data!
                            ttft_ms = upstream_req_start.elapsed().as_micros() as f64 / 1000.0;
                            first_data_chunk = Some(bytes);
                            break;
                        }
                        Ok(Some(Err(e))) => {
                            tracing::warn!(
                                "[{}] Stream error during peek: {}, retrying...",
                                trace_id,
                                e
                            );
                            last_error = format!("Stream error during peek: {}", e);
                            retry_this_account = true;
                            break;
                        }
                        Ok(None) => {
                            tracing::warn!(
                                "[{}] Stream ended during peek (Empty Response), retrying...",
                                trace_id
                            );
                            last_error = "Empty response stream during peek".to_string();
                            retry_this_account = true;
                            break;
                        }
                        Err(_) => {
                            tracing::warn!(
                                "[{}] Timeout waiting for first data (30s), retrying...",
                                trace_id
                            );
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
                        let combined_stream = futures::stream::once(async move { Ok(bytes) })
                            .chain(stream_rest.map(|result| -> Result<Bytes, std::io::Error> {
                                match result {
                                    Ok(b) => Ok(b),
                                    Err(e) => Ok(Bytes::from(format!(
                                        "data: {{\"error\":\"{}\"}}\n\n",
                                        e
                                    ))),
                                }
                            }));

                        // [FIX #Bug1] 针对 Claude 流增加空闲超时保护，从 300s 降至 120s
                        // 300s 会导致客户端等待长达 5 分钟；120s 仍有足够容错余量
                        let combined_stream = async_stream::stream! {
                            let mut s = Box::pin(combined_stream);
                            loop {
                                match tokio::time::timeout(std::time::Duration::from_secs(120), s.next()).await {
                                    Ok(Some(item)) => yield item,
                                    Ok(None) => break,
                                    Err(_) => {
                                        tracing::error!("[Claude-SSE] Idle timeout after 120s, terminating stream");
                                        yield Ok::<Bytes, std::io::Error>(Bytes::from("data: {\"type\": \"message_stop\"}\n\ndata: [DONE]\n\n"));
                                        break;
                                    }
                                }
                            }
                        };

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
                                .header("X-Session-Id", &client_session_id)
                                .header("X-Antigravity-Session-Id", &client_session_id)
                                .header("X-Context-Purified", "false")
                                .header("X-Timing-Clean-Ms", format!("{:.3}", clean_ms))
                                .header("X-Timing-Norm-Ms", format!("{:.3}", norm_ms))
                                .header("X-Timing-Thinking-Ms", format!("{:.3}", think_fill_ms))
                                .header("X-Timing-Ttft-Ms", format!("{:.3}", ttft_ms))
                                .body(Body::from_stream(combined_stream))
                                .unwrap();
                        } else {
                            // 客户端要非 Stream，需要收集完整响应并转换为 JSON
                            use crate::proxy::mappers::claude::collect_stream_to_json;

                            match collect_stream_to_json(Box::pin(combined_stream)).await {
                                Ok(full_response) => {
                                    info!(
                                        "[{}] ✓ Stream collected and converted to JSON",
                                        trace_id
                                    );
                                    return Response::builder()
                                        .status(StatusCode::OK)
                                        .header(header::CONTENT_TYPE, "application/json")
                                        .header("X-Account-Email", &email)
                                        .header("X-Mapped-Model", &request_with_mapped.model)
                                        .header("X-Session-Id", &client_session_id)
                                        .header("X-Antigravity-Session-Id", &client_session_id)
                                        .header("X-Context-Purified", "false")
                                        .header("X-Timing-Clean-Ms", format!("{:.3}", clean_ms))
                                        .header("X-Timing-Norm-Ms", format!("{:.3}", norm_ms))
                                        .header(
                                            "X-Timing-Thinking-Ms",
                                            format!("{:.3}", think_fill_ms),
                                        )
                                        .header("X-Timing-Ttft-Ms", format!("{:.3}", ttft_ms))
                                        .body(Body::from(
                                            serde_json::to_string(&full_response).unwrap(),
                                        ))
                                        .unwrap();
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "[{}] Stream collection error (possibly upstream interrupted): {}, retrying with another account...",
                                        trace_id,
                                        e
                                    );
                                    last_error = format!("Stream collection error: {}", e);
                                    force_rotate = true;
                                    continue;
                                }
                            }
                        }
                    }

                    None => {
                        tracing::warn!(
                            "[{}] Stream ended immediately (Empty Response), retrying...",
                            trace_id
                        );
                        last_error = "Empty response stream (None)".to_string();
                        continue;
                    }
                }
            } else {
                // 处理非流式响应
                let bytes = match response.bytes().await {
                    Ok(b) => b,
                    Err(e) => {
                        return (
                            StatusCode::BAD_GATEWAY,
                            format!("Failed to read body: {}", e),
                        )
                            .into_response()
                    }
                };

                // Debug print
                if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                    debug!("Upstream Response for Claude request: {}", text);
                }

                let gemini_resp: Value = match serde_json::from_slice(&bytes) {
                    Ok(v) => v,
                    Err(e) => {
                        return (StatusCode::BAD_GATEWAY, format!("Parse error: {}", e))
                            .into_response()
                    }
                };

                // 解包 response 字段（v1internal 格式）
                let raw = gemini_resp.get("response").unwrap_or(&gemini_resp);

                // 转换为 Gemini Response 结构
                let gemini_response: crate::proxy::mappers::claude::models::GeminiResponse =
                    match serde_json::from_value(raw.clone()) {
                        Ok(r) => r,
                        Err(e) => {
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                format!("Convert error: {}", e),
                            )
                                .into_response()
                        }
                    };

                // Determine context limit based on model
                let context_limit =
                    crate::proxy::mappers::claude::utils::get_context_limit_for_model(
                        &request_with_mapped.model,
                    );

                // 转换
                // [FIX #765] Pass session_id and model_name for signature caching
                let s_id_owned = Some(store_key.clone());
                // [FIX #3379] Extract registered tool names for non-streaming leakage recovery
                let ns_registered_tool_names: Vec<String> = request_with_mapped
                    .tools
                    .as_ref()
                    .map(|tools| tools.iter().filter_map(|t| t.name.clone()).collect())
                    .unwrap_or_default();
                // 转换
                let claude_response = match transform_response(
                    &gemini_response,
                    scaling_enabled,
                    context_limit,
                    s_id_owned,
                    request_with_mapped.model.clone(),
                    request_with_mapped.messages.len(), // [NEW v4.0.0] Pass message count for rewind detection
                    ns_registered_tool_names, // [FIX #3379] For call:default_api leakage recovery
                ) {
                    Ok(r) => r,
                    Err(e) => {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("Transform error: {}", e),
                        )
                            .into_response()
                    }
                };

                // [Optimization] 记录闭环日志：消耗情况
                let cache_info = if let Some(cached) = claude_response.usage.cache_read_input_tokens
                {
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

                return (
                    StatusCode::OK,
                    [
                        ("X-Account-Email", email.as_str()),
                        ("X-Mapped-Model", request_with_mapped.model.as_str()),
                    ],
                    Json(claude_response),
                )
                    .into_response();
            }
        }

        // 1. 立即提取状态码和 headers（防止 response 被 move）
        let status_code = status.as_u16();
        last_status = status;
        let retry_after = response
            .headers()
            .get("Retry-After")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        // 2. 获取错误文本并转移 Response 所有权
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| format!("HTTP {}", status));
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
            debug_logger::write_debug_payload(
                &debug_cfg,
                Some(&trace_id),
                "upstream_response_error",
                &payload,
            )
            .await;
        }

        // 3. 统一流水线决策判定（协议无关的唯一真理）
        let classification = crate::proxy::pipeline::UpstreamClassification::classify(
            status_code,
            &error_text,
            retry_after.as_deref(),
        );

        if classification.is_model_not_found() {
            // [NEW] 针对特定账号记录单模型临时熔断（例如该 PRO 账号未开通 Claude 5.5），绝不连坐其他模型
            token_manager.mark_model_unsupported(
                &account_id,
                &request_with_mapped.model,
                Some(900),
            );

            // 如果账号池中还有其他未尝试的候选账号，则顺畅换号重试，而不是直接放弃报错
            if attempt < pool_size {
                tracing::warn!(
                    "[{}] 上游报错模型不可用 (HTTP {})，已标记账号 {} 对模型 [{}] 临时熔断，继续换号重试 ({}/{})...",
                    trace_id, status_code, email, request_with_mapped.model, attempt, pool_size
                );
                force_rotate = true;
                continue;
            }

            tracing::warn!(
                "[{}] Pipeline: Target model [{}] not found on upstream (HTTP {}). Pool exhausted without account-level lockout.",
                trace_id, request_with_mapped.model, status_code
            );
            let dual_err = crate::proxy::handlers::common::build_dual_track_error(
                "claude",
                status_code,
                &request_with_mapped.model,
                &error_text,
            );
            return (
                StatusCode::from_u16(status_code).unwrap_or(StatusCode::NOT_FOUND),
                [
                    ("X-Account-Email", email.as_str()),
                    ("X-Mapped-Model", request_with_mapped.model.as_str()),
                ],
                Json(dual_err),
            )
                .into_response();
        }

        if classification.should_lock_account() {
            token_manager
                .mark_rate_limited_async_baseline(
                    &email,
                    status_code,
                    retry_after.as_deref(),
                    &error_text,
                    Some(&request_with_mapped.model),
                )
                .await;
        }
        if classification.abandons_sticky_account() {
            token_manager.abandon_session(&affinity_key, &account_id);
            debug!(
                "[{}] Unbound session {} from account {} due to status {}",
                trace_id, affinity_key, email, status_code
            );
        }

        // 4. 处理 400 错误 (Thinking 签名失效 或 块顺序错误)
        // [FIX 2026-08-28] Use case-insensitive matching and cover Google's exact phrasing:
        // "Invalid thought signature." / "thoughtSignature" / "thought_signature"
        let lower_err = error_text.to_lowercase();
        if status_code == 400
            && !retried_without_thinking
            && (lower_err.contains("invalid thought signature")
                || lower_err.contains("invalid `signature`")
                || lower_err.contains("invalid signature")
                || lower_err.contains("thought_signature")
                || lower_err.contains("thoughtsignature")
                || lower_err.contains("thinking.signature: field required")
                || lower_err.contains("thinking.thinking: field required")
                || lower_err.contains("thinking.signature")
                || lower_err.contains("thinking.thinking")
                || lower_err.contains("corrupted thought signature")
                || lower_err.contains("failed to deserialise")
                || lower_err.contains("thinking block")
                || lower_err.contains("found `text`")
                || lower_err.contains("found 'text'")
                || lower_err.contains("must be `thinking`")
                || lower_err.contains("must be 'thinking'"))
        {
            // Existing logic for thinking signature.
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
                if let crate::proxy::mappers::claude::models::MessageContent::Array(blocks) =
                    &mut msg.content
                {
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

            // 精准定向净化 ThinkingStore 中当前 session 的异构污染签名，保留思考文本与健康历史签名，
            // 彻底防止重试阶段再次把坏签名还原回 contents
            session_scope.purge_signatures(&mapped_model);

            // [FIX Prompt-Cache] 严禁在重试路径中注入合成消息 (close_tool_loop_for_thinking)！
            // 保持历史消息真实纯净，由 InboundThinkingPipeline 与 finalize_gemini_contents_thinking 统一兜底签名与占位。

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
                &trace_id,
            )
            .await
            {
                continue;
            }
        }

        // 5. 统一处理所有可重试错误
        // [REMOVED] 不再特殊处理 QUOTA_EXHAUSTED,允许账号轮换
        // 原逻辑会在第一个账号配额耗尽时直接返回,导致"平衡"模式无法切换账号

        // [FIX] 403 时设置 is_forbidden 状态，避免账号被重复选中
        if status_code == 403 {
            // Check for VALIDATION_REQUIRED error - temporarily block account
            if error_text.contains("VALIDATION_REQUIRED")
                || error_text.contains("verify your account")
                || error_text.contains("validation_url")
            {
                tracing::warn!(
                    "[Claude] VALIDATION_REQUIRED detected on account {}, temporarily blocking",
                    email
                );
                let block_minutes = 10i64;
                let block_until = chrono::Utc::now().timestamp() + (block_minutes * 60);
                if let Err(e) = token_manager
                    .set_validation_block_public(&account_id, block_until, &error_text)
                    .await
                {
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

        // [FIX session-1M] 上游按 sessionId 在服务端累计会话输入，长工具循环会把累计推过 1M，
        // 之后该 sessionId 的所有请求都 400 "input token count exceeds ... 1048576"。
        // 给 (账号, 对话) 的 sessionId 升代并立即重试:新 sessionId = 上游全新会话,对话无感恢复。
        if status_code == 400 && error_text.contains("exceeds the maximum number of tokens") {
            let generation = crate::proxy::common::session::bump_session(&account_id, &store_key);
            tracing::warn!(
                "[Claude] Upstream session token accumulation exceeded 1M on account {}. sessionId bumped to generation {}, retrying with a fresh upstream session.",
                email, generation
            );
            continue; // 重试:下一轮 transform 时读取新代数,派生全新 sessionId
        }

        let scheduling_mode = token_manager.get_scheduling_mode().await;
        let allow_grace = match scheduling_mode {
            crate::proxy::sticky_config::SchedulingMode::Balance => {
                token_manager.tokens_count() <= 1
            }
            crate::proxy::sticky_config::SchedulingMode::CacheFirst => true,
            crate::proxy::sticky_config::SchedulingMode::PerformanceFirst => false,
        };

        // 确定重试策略：传入当前 attempt 与 pool_size，执行智能自适应裁决
        let retry_strategy = super::common::determine_retry_strategy_adaptive(
            status_code,
            &error_text,
            retry_after.as_deref(),
            retried_without_thinking,
            allow_grace,
            attempt,
            pool_size,
        );

        // 执行退避
        if apply_retry_strategy(
            retry_strategy.clone(),
            attempt,
            max_attempts,
            status_code,
            &trace_id,
        )
        .await
        {
            // 判断是否需要轮换账号
            if !should_rotate_account(status_code, Some(&retry_strategy)) {
                debug!(
                    "[{}] Keeping same account for status {} (Grace Retry or Server Issue)",
                    trace_id, status_code
                );
                force_rotate = false;
            } else {
                force_rotate = true;
            }
            continue;
        } else {
            // 不可重试的错误，直接返回双轨制友好报文
            error!(
                "[{}] Non-retryable error {}: {}",
                trace_id, status_code, error_text
            );
            let dual_err = crate::proxy::handlers::common::build_dual_track_error(
                "claude",
                status_code,
                &request_with_mapped.model,
                &error_text,
            );
            return (
                status,
                [
                    ("X-Account-Email", email.as_str()),
                    ("X-Mapped-Model", request_with_mapped.model.as_str()),
                ],
                Json(dual_err),
            )
                .into_response();
        }
    }

    if let Some(email) = last_email {
        // [FIX] Include X-Mapped-Model in exhaustion error
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Account-Email",
            header::HeaderValue::from_str(&email).unwrap(),
        );
        if let Some(ref model) = last_mapped_model {
            if let Ok(v) = header::HeaderValue::from_str(model) {
                headers.insert("X-Mapped-Model", v);
            }
        }

        let _error_type = match last_status.as_u16() {
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

        if let Some(sec) = crate::proxy::handlers::common::extract_retry_after_seconds(&last_error)
        {
            if let Ok(val) = header::HeaderValue::from_str(&sec.to_string()) {
                headers.insert(axum::http::header::RETRY_AFTER, val);
            }
        }

        let model_str = last_mapped_model.as_deref().unwrap_or("unknown");
        let dual_err = crate::proxy::handlers::common::build_dual_track_error(
            "claude",
            response_status.as_u16(),
            model_str,
            &last_error,
        );

        (response_status, headers, Json(dual_err)).into_response()
    } else {
        // Fallback if no email (e.g. mapping error before token)
        let mut headers = HeaderMap::new();
        if let Some(ref model) = last_mapped_model {
            if let Ok(v) = header::HeaderValue::from_str(model) {
                headers.insert("X-Mapped-Model", v);
            }
        }
        if let Some(sec) = crate::proxy::handlers::common::extract_retry_after_seconds(&last_error)
        {
            if let Ok(val) = header::HeaderValue::from_str(&sec.to_string()) {
                headers.insert(axum::http::header::RETRY_AFTER, val);
            }
        }

        let _error_type = match last_status.as_u16() {
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

        let model_str = last_mapped_model.as_deref().unwrap_or("unknown");
        let dual_err = crate::proxy::handlers::common::build_dual_track_error(
            "claude",
            response_status.as_u16(),
            model_str,
            &last_error,
        );

        (response_status, headers, Json(dual_err)).into_response()
    }
}

/// 列出可用模型
pub async fn handle_list_models(State(state): State<AppState>) -> impl IntoResponse {
    use crate::proxy::common::model_mapping::get_all_dynamic_models;

    let only_raw = *state.only_raw_quota_models.read().await;
    let model_ids =
        get_all_dynamic_models(&state.custom_mapping, Some(&state.token_manager), only_raw).await;

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

/// Claude Models API: GET /v1/models/claude/{model}
/// 检索指定模型的详细元数据
pub async fn handle_retrieve_model(
    State(state): State<AppState>,
    axum::extract::Path(model): axum::extract::Path<String>,
) -> impl IntoResponse {
    use crate::proxy::common::model_mapping::find_dynamic_model;

    let only_raw = *state.only_raw_quota_models.read().await;
    if let Some(matched_id) = find_dynamic_model(
        &state.custom_mapping,
        Some(&state.token_manager),
        only_raw,
        &model,
    )
    .await
    {
        (
            StatusCode::OK,
            Json(json!({
                "type": "model",
                "id": matched_id.clone(),
                "display_name": matched_id,
                "created_at": "2024-10-22T00:00:00Z"
            })),
        )
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "type": "error",
                "error": {
                    "type": "not_found_error",
                    "message": format!("model: {}", model)
                }
            })),
        )
            .into_response()
    }
}

/// 计算 tokens (Anthropic 官方 Messages Count Tokens API)
/// 接入 Pipeline 协议无关通用估算引擎与全局高并发内容哈希缓存，
/// 严格遵循官方 Schema 仅返回 input_tokens，彻底移除非标冗余 output_tokens 字段。
pub async fn handle_count_tokens(_headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let input_tokens = crate::proxy::pipeline::estimate_tokens(&body);

    Json(json!({
        "input_tokens": input_tokens
    }))
    .into_response()
}

#[cfg(test)]
mod opus_variant_tests {
    use crate::proxy::common::variant_mapping;
    use crate::proxy::mappers::claude::models::ThinkingConfig;

    #[test]
    fn claude_opus_preserves_client_budget_when_present() {
        let client_budget = Some(32_768);
        let spec = variant_mapping::resolve("claude-opus-4-6-thinking", client_budget)
            .expect("Claude Opus 4.6 thinking must resolve");
        let request_thinking = ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(spec.effective_thinking_budget(client_budget)),
            effort: None,
        };

        assert_eq!(request_thinking.budget_tokens, client_budget);
    }

    #[test]
    fn claude_opus_falls_back_to_spec_budget_when_client_budget_is_absent() {
        let client_budget = None;
        let spec = variant_mapping::resolve("claude-opus-4-6-thinking", client_budget)
            .expect("Claude Opus 4.6 thinking must resolve");
        let request_thinking = ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(spec.effective_thinking_budget(client_budget)),
            effort: None,
        };

        assert_eq!(request_thinking.budget_tokens, Some(1_024));
    }
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
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
enum BackgroundTaskType {
    TitleGeneration,    // 标题生成
    SimpleSummary,      // 简单摘要
    ContextCompression, // 上下文压缩
    PromptSuggestion,   // 提示建议
    SystemMessage,      // 系统消息
    EnvironmentProbe,   // 环境探测
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
const SUGGESTION_KEYWORDS: &[&str] = &[
    "prompt suggestion generator",
    "suggest next prompts",
    "what should I ask next",
    "generate follow-up questions",
    "recommend next steps",
    "possible next actions",
];

#[allow(dead_code)]
const SYSTEM_KEYWORDS: &[&str] = &[
    "Warmup",
    "<system-reminder>",
    // Removed: "Caveat: The messages below were generated" - this is a normal Claude Desktop system prompt
    "This is a system message",
];

#[allow(dead_code)]
const PROBE_KEYWORDS: &[&str] = &[
    "check current directory",
    "list available tools",
    "verify environment",
    "test connection",
];

#[allow(dead_code)]
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

#[allow(dead_code)]
fn matches_keywords(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|kw| text.contains(kw))
}

#[allow(dead_code)]
fn extract_last_user_message_for_detection(request: &ClaudeRequest) -> Option<String> {
    request
        .messages
        .iter()
        .rev()
        .filter(|m| m.role == "user")
        .find_map(|m| {
            let content = match &m.content {
                crate::proxy::mappers::claude::models::MessageContent::String(s) => s.to_string(),
                crate::proxy::mappers::claude::models::MessageContent::Array(arr) => arr
                    .iter()
                    .filter_map(|block| match block {
                        crate::proxy::mappers::claude::models::ContentBlock::Text { text } => {
                            Some(text.as_str())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
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

#[allow(dead_code)]
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

/// 检测是否为真正的 Claude Code 保活心跳请求（极度收窄规则，杜绝误杀）
///
/// 只有当最后一条消息为用户角色且内容严格全等于 "Warmup" 单词本身，且不包含任何工具调用或多余内容时，
/// 才认定为客户端心跳。绝不使用 starts_with 匹配，绝不拦截 ToolResult。
fn is_warmup_request(request: &ClaudeRequest) -> bool {
    if let Some(msg) = request.messages.last() {
        if msg.role != "user" {
            return false;
        }
        match &msg.content {
            crate::proxy::mappers::claude::models::MessageContent::String(s) => {
                s.trim().eq_ignore_ascii_case("warmup")
            }
            crate::proxy::mappers::claude::models::MessageContent::Array(arr) => {
                if arr.len() == 1 {
                    if let crate::proxy::mappers::claude::models::ContentBlock::Text { text } =
                        &arr[0]
                    {
                        text.trim().eq_ignore_ascii_case("warmup")
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
        }
    } else {
        false
    }
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
            Json(response),
        )
            .into_response()
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
    upstream: &Arc<crate::proxy::upstream::client::UpstreamClient>,
    trace_id: &str,
) -> Result<String, String> {
    // Get token and transform request
    let (access_token, project_id, _, account_id, _wait_ms) = token_manager
        .get_token("gemini", false, None, model)
        .await
        .map_err(|e| format!("Failed to get account: {}", e))?;

    let token_obj = token_manager.get_token_by_id(&account_id);
    let gemini_body = crate::proxy::mappers::claude::transform_claude_request_in(
        request,
        &project_id,
        false,
        Some(account_id.as_str()),
        trace_id,
        token_obj.as_ref(),
    )
    .map_err(|e| format!("Failed to transform request: {}", e))?;

    // 走共享的辅助调用通道：复用主请求路径的客户端（含按账号代理池）、
    // 端点顺序（Daily → Sandbox → Prod）、URL 形状与回退判定。
    //
    // 历史实现把 `transform_claude_request_in` 产出的**已包装 cloudcode 信封**
    // （内含 project / request / model / userAgent / requestId / requestType）
    // POST 到 `https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent`
    // —— host 与形状双双不符。实测该 host 对 Antigravity 账号恒返回
    // 403 `ACCESS_TOKEN_SCOPE_INSUFFICIENT`（token 不具备公共 Generative Language API 权限），
    // 因此该后台摘要从未成功过。
    debug!(
        "[{}] Calling {} via cloudcode v1internal for summary",
        trace_id, model
    );

    let gemini_response = upstream
        .call_v1_internal_auxiliary(
            "generateContent",
            &access_token,
            gemini_body,
            Some(account_id.as_str()),
            SUMMARY_REQUEST_TIMEOUT_SECS,
        )
        .await?;

    // 上游返回 `{"response": {...}}` 包装体，必须先解包。
    let unwrapped = crate::proxy::mappers::gemini::unwrap_response(&gemini_response);

    // 只拼接「非思考 part」的文本：前面可能存在 `{"thought": true, "text": ""}` 的空块，
    // 直接取 `parts[0].text` 会拿到空串并把空摘要当成功。
    unwrapped
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|parts| parts.as_array())
        .map(|parts| {
            parts
                .iter()
                .filter(|part| !crate::proxy::thinking_store::is_thought_part(part))
                .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                .collect::<String>()
        })
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| "Failed to extract text from response".to_string())
}

// ===== [Layer 3] Fork Conversation + XML Summary =====
// This is the ultimate context compression strategy
// Borrowed from Practical-Guide-to-Context-Engineering + Claude Code official practice

/// Try to compress context by generating an XML summary and forking the conversation
///
/// This function:
/// 1. Extracts the last valid thinking signature
/// 2. Calls a cheap model (gemini-3.1-flash-lite via internal-background-task) to generate XML summary
/// 3. Creates a new message sequence with summary as prefix
/// 4. Preserves the signature in the summary
/// 5. Returns the forked request
///
/// Returns Ok(forked_request) on success, Err(error_message) on failure
async fn try_compress_with_summary(
    original_request: &ClaudeRequest,
    trace_id: &str,
    token_manager: &Arc<crate::proxy::TokenManager>,
    upstream: &Arc<crate::proxy::upstream::client::UpstreamClient>,
) -> Result<ClaudeRequest, String> {
    info!(
        "[{}] [Layer-3] Starting context compression with XML summary",
        trace_id
    );

    // 1. Extract last valid signature
    let last_signature = ContextManager::extract_last_valid_signature(&original_request.messages);

    if let Some(ref sig) = last_signature {
        debug!(
            "[{}] [Layer-3] Extracted signature (len: {})",
            trace_id,
            sig.len()
        );
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
            CONTEXT_SUMMARY_PROMPT, signature_instruction
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
        tool_choice: None,
    };

    debug!(
        "[{}] [Layer-3] Calling {} for summary generation",
        trace_id, INTERNAL_BACKGROUND_TASK
    );

    // 3. Call upstream using helper function (reuse existing infrastructure)
    let xml_summary = call_gemini_sync(
        INTERNAL_BACKGROUND_TASK,
        &summary_request,
        token_manager,
        upstream,
        trace_id,
    )
    .await?;

    info!(
        "[{}] [Layer-3] Generated XML summary (len: {} chars)",
        trace_id,
        xml_summary.len()
    );

    // 4. Create forked conversation with summary as prefix
    // Wrap text inside a ContentBlock::Text and attach cache_control to freeze it in upstream's Prompt Cache
    let mut forked_messages = vec![
        Message {
            role: "user".to_string(),
            content: MessageContent::Array(vec![
                crate::proxy::mappers::claude::models::ContentBlock::Text {
                    text: format!(
                        "Context has been compressed. Here is the structured summary of our conversation history:\n\n{}",
                        xml_summary
                    ),
                }
            ]),
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
            if !matches!(&last_msg.content, MessageContent::String(s) if s.contains(CONTEXT_SUMMARY_PROMPT))
            {
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
        tool_choice: original_request.tool_choice.clone(),
    })
}

/// Injects cache_control ephemeral trigger to first message's content block if it's the XML summary
fn inject_cache_control_to_forked_summary(body: &mut serde_json::Value) {
    if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        if !messages.is_empty() {
            let first_msg = &mut messages[0];
            if let Some(content) = first_msg.get_mut("content") {
                if let Some(content_arr) = content.as_array_mut() {
                    if !content_arr.is_empty() {
                        let is_summary = content_arr[0]
                            .get("text")
                            .and_then(|t| t.as_str())
                            .map(|s| s.contains("Context has been compressed"))
                            .unwrap_or(false);

                        if is_summary {
                            if let Some(obj) = content_arr[0].as_object_mut() {
                                obj.insert(
                                    "cache_control".to_string(),
                                    serde_json::json!({
                                        "type": "ephemeral"
                                    }),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod warmup_tests {
    use super::*;
    use crate::proxy::mappers::claude::models::{ContentBlock, Message, MessageContent};

    #[test]
    fn test_is_warmup_request_strictly_exact() {
        // 1. 严格全等为 Warmup 的请求
        let exact_req = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Warmup".to_string()),
            }],
            system: None,
            max_tokens: Some(100),
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            tools: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };
        assert!(is_warmup_request(&exact_req));

        // 2. 带后续句子的真实用户问题，绝不误杀！
        let real_question_req = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Warmup function in PyTorch 怎么写？".to_string()),
            }],
            system: None,
            max_tokens: Some(100),
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            tools: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };
        assert!(!is_warmup_request(&real_question_req));

        // 3. 包含 ToolResult 的消息，绝不误杀！
        let tool_error_req = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Array(vec![ContentBlock::ToolResult {
                    tool_use_id: "tool_1".to_string(),
                    content: serde_json::json!("Warmup failed: connection refused"),
                    is_error: Some(true),
                }]),
            }],
            system: None,
            max_tokens: Some(100),
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            tools: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };
        assert!(!is_warmup_request(&tool_error_req));
    }

    #[test]
    fn test_is_manual_compact_command() {
        use crate::proxy::mappers::claude::models::{ContentBlock, Message, MessageContent};

        // 1. 标准 ./compact 字符串
        let req1 = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("./compact".to_string()),
            }],
            system: None,
            max_tokens: Some(100),
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            tools: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };
        assert!(is_manual_compact_command(&req1));

        // 2. 带 system-reminder 干扰的 ./compact
        let req2 = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String(
                    "<system-reminder>some reminder</system-reminder>\n\n./compact".to_string(),
                ),
            }],
            system: None,
            max_tokens: Some(100),
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            tools: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };
        assert!(is_manual_compact_command(&req2));

        // 3. Array 结构的 ContentBlock 文本
        let req3 = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Array(vec![ContentBlock::Text {
                    text: "./compact please summarize".to_string(),
                }]),
            }],
            system: None,
            max_tokens: Some(100),
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            tools: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };
        assert!(is_manual_compact_command(&req3));

        // 4. 普通用户提问，不应误判
        let req4 = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("How to use compact method in Ruby?".to_string()),
            }],
            system: None,
            max_tokens: Some(100),
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            tools: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };
        assert!(!is_manual_compact_command(&req4));
    }

    #[test]
    fn test_make_compact_responses() {
        let text = "Compacted conversation · saved 289k tokens";
        let sse = make_compact_sse_response(text, "claude-3-7-sonnet", 45100);
        assert!(sse.contains("event: message_start"));
        assert!(sse.contains("Compacted conversation · saved 289k tokens"));
        assert!(sse.contains("\"input_tokens\":45100"));

        let json = make_compact_json_response(text, "claude-3-7-sonnet", 45100);
        assert_eq!(json["role"], "assistant");
        assert_eq!(json["content"][0]["text"], text);
        assert_eq!(json["usage"]["input_tokens"], 45100);
    }

    #[test]
    fn test_calculate_claude_fixed_overhead_and_solvable_target_limit() {
        use crate::proxy::mappers::claude::models::{SystemBlock, SystemPrompt, Tool};

        let req = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![],
            system: Some(SystemPrompt::Array(vec![SystemBlock {
                block_type: "text".to_string(),
                text: "You are Claude Code, an AI assistant.".to_string(),
            }])),
            tools: Some(vec![
                Tool {
                    type_: None,
                    name: Some("Bash".to_string()),
                    description: Some("Execute bash command".to_string()),
                    input_schema: None,
                },
                Tool {
                    type_: None,
                    name: Some("Read".to_string()),
                    description: Some("Read file".to_string()),
                    input_schema: None,
                },
            ]),
            ..Default::default()
        };

        let overhead = calculate_claude_fixed_overhead(&req);
        assert!(overhead > 0);
        let target_limit = (overhead + 15_000).max(35_000);
        assert!(target_limit >= 35_000);
        assert!(target_limit >= overhead + 15_000);
    }

    #[test]
    fn test_compact_session_orphan_timeout_eviction() {
        let session_key = "test_orphan_session_eviction".to_string();
        COWORK_COMPACT_SESSIONS.insert(
            session_key.clone(),
            CoworkCompactState {
                kind: CoworkCompactKind::Manual,
                before_tokens: 100_000,
                ts: std::time::Instant::now() - std::time::Duration::from_secs(301),
                summary_done: false,
                target_limit: 35_000,
            },
        );

        let is_in_compaction_active = is_session_in_compaction(&session_key);

        assert!(
            !is_in_compaction_active,
            "Expired orphan session must not be considered in compaction"
        );
        assert!(
            !COWORK_COMPACT_SESSIONS.contains_key(&session_key),
            "Expired orphan session must be evicted from map"
        );
    }

    #[test]
    fn test_just_compacted_cache_expiration_and_capacity_cap() {
        let session_key = "test_expired_compacted_session".to_string();
        COWORK_JUST_COMPACTED_CACHE.insert(
            session_key.clone(),
            (
                std::time::Instant::now() - std::time::Duration::from_secs(65),
                "Compacted conversation".to_string(),
                20_000,
            ),
        );

        let now = std::time::Instant::now();
        if let Some(entry) = COWORK_JUST_COMPACTED_CACHE.get(&session_key) {
            let (cached_ts, _, _) = *entry;
            if now.duration_since(cached_ts).as_secs() < 60 {
                panic!("Should not hit cache for expired entry");
            } else {
                drop(entry);
                COWORK_JUST_COMPACTED_CACHE.remove(&session_key);
            }
        }

        assert!(
            !COWORK_JUST_COMPACTED_CACHE.contains_key(&session_key),
            "Expired entry must be actively removed from COWORK_JUST_COMPACTED_CACHE"
        );

        // Test capacity cap behavior
        for i in 0..1005 {
            let k = format!("cap_test_session_{}", i);
            COWORK_JUST_COMPACTED_CACHE.insert(
                k,
                (
                    std::time::Instant::now() - std::time::Duration::from_secs(70),
                    "Compacted conversation".to_string(),
                    10_000,
                ),
            );
        }

        if COWORK_JUST_COMPACTED_CACHE.len() >= MAX_JUST_COMPACTED_CACHE_CAPACITY {
            let purge_now = std::time::Instant::now();
            COWORK_JUST_COMPACTED_CACHE
                .retain(|_, (ts, _, _)| purge_now.duration_since(*ts).as_secs() < 60);
            if COWORK_JUST_COMPACTED_CACHE.len() >= MAX_JUST_COMPACTED_CACHE_CAPACITY {
                COWORK_JUST_COMPACTED_CACHE.clear();
            }
        }

        assert!(
            COWORK_JUST_COMPACTED_CACHE.len() <= MAX_JUST_COMPACTED_CACHE_CAPACITY,
            "Cache length must not exceed maximum capacity"
        );
    }

    #[test]
    fn test_compaction_immunity_lease_lifecycle() {
        let session_key = "test_lease_lifecycle".to_string();
        let now = std::time::Instant::now();
        COMPACTION_IMMUNITY_LEASES.insert(
            session_key.clone(),
            CompactionImmunityLease {
                created_at: now,
                last_touched: now,
                consumed: false,
            },
        );

        // Turn 1: Initial passthrough activates sliding window
        let is_compaction_req = false;
        let mut passed = false;
        if !is_compaction_req {
            if let Some(mut lease) = COMPACTION_IMMUNITY_LEASES.get_mut(&session_key) {
                let check_now = std::time::Instant::now();
                let valid = if !lease.consumed {
                    check_now.duration_since(lease.created_at).as_secs()
                        < COMPACTION_IMMUNITY_INITIAL_TTL_SECS
                } else {
                    check_now.duration_since(lease.last_touched).as_secs()
                        < COMPACTION_IMMUNITY_WINDOW_SECS
                };
                if valid {
                    lease.consumed = true;
                    lease.last_touched = check_now;
                    passed = true;
                }
            }
        }
        assert!(passed, "Turn 1 must pass through and activate lease");
        assert!(
            COMPACTION_IMMUNITY_LEASES
                .get(&session_key)
                .unwrap()
                .consumed,
            "Lease must be marked as consumed"
        );

        // Turn 2: Concurrent request / tool call within 30s sliding window passes through
        passed = false;
        if !is_compaction_req {
            if let Some(mut lease) = COMPACTION_IMMUNITY_LEASES.get_mut(&session_key) {
                let check_now = std::time::Instant::now();
                let valid = if !lease.consumed {
                    check_now.duration_since(lease.created_at).as_secs()
                        < COMPACTION_IMMUNITY_INITIAL_TTL_SECS
                } else {
                    check_now.duration_since(lease.last_touched).as_secs()
                        < COMPACTION_IMMUNITY_WINDOW_SECS
                };
                if valid {
                    lease.last_touched = check_now;
                    passed = true;
                }
            }
        }
        assert!(
            passed,
            "Concurrent turn within sliding window must pass through"
        );

        // Expired lease after window is evicted
        if let Some(mut lease) = COMPACTION_IMMUNITY_LEASES.get_mut(&session_key) {
            lease.last_touched = std::time::Instant::now() - std::time::Duration::from_secs(35);
        }
        passed = false;
        if !is_compaction_req {
            if let Some(lease) = COMPACTION_IMMUNITY_LEASES.get_mut(&session_key) {
                let check_now = std::time::Instant::now();
                let valid = if !lease.consumed {
                    check_now.duration_since(lease.created_at).as_secs()
                        < COMPACTION_IMMUNITY_INITIAL_TTL_SECS
                } else {
                    check_now.duration_since(lease.last_touched).as_secs()
                        < COMPACTION_IMMUNITY_WINDOW_SECS
                };
                if valid {
                    passed = true;
                } else {
                    drop(lease);
                    COMPACTION_IMMUNITY_LEASES.remove(&session_key);
                }
            }
        }
        assert!(!passed, "Expired lease must not pass through");
        assert!(
            !COMPACTION_IMMUNITY_LEASES.contains_key(&session_key),
            "Expired lease must be removed"
        );
    }

    #[test]
    fn test_claude_stream_chunk_has_error_event() {
        // Normal text chunk
        let normal = b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n";
        assert!(!claude_stream_chunk_has_error_event(normal));

        // Normal text delta containing word 'error' in user text
        let delta = b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"there is an error in code\"}}\n\n";
        assert!(!claude_stream_chunk_has_error_event(delta));

        // Error event from create_claude_sse_stream
        let error_chunk = b"event: error\ndata: {\"error\":{\"call_site\":\"src/proxy/mappers/claude/mod.rs:98\",\"function\":\"create_claude_sse_stream\",\"message\":\"Stream interrupted\",\"params\":\"trace=xxx\",\"type\":\"stream_error\"},\"type\":\"error\"}\n\n";
        assert!(claude_stream_chunk_has_error_event(error_chunk));

        // Bare error event
        let bare_error = b"data: {\"type\":\"error\",\"error\":{\"message\":\"fail\"}}\n\n";
        assert!(claude_stream_chunk_has_error_event(bare_error));

        // Heartbeat ping
        let heartbeat = b": ping\n\n";
        assert!(!claude_stream_chunk_has_error_event(heartbeat));
    }

    #[test]
    fn test_calculate_claude_fixed_overhead_includes_schema_and_description() {
        use crate::proxy::mappers::claude::models::Tool;

        let req_without_schema = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![],
            system: None,
            tools: Some(vec![Tool {
                type_: None,
                name: Some("test_tool".to_string()),
                description: None,
                input_schema: None,
            }]),
            ..Default::default()
        };
        let overhead_small = calculate_claude_fixed_overhead(&req_without_schema);

        let large_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "param1": {"type": "string", "description": "A very detailed description of parameter one that takes up lots of tokens"},
                "param2": {"type": "array", "items": {"type": "string"}, "description": "Another parameter with deep nested definitions"},
                "param3": {"type": "object", "properties": {"nested": {"type": "boolean"}}}
            },
            "required": ["param1"]
        });

        let req_with_schema = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![],
            system: None,
            tools: Some(vec![Tool {
                type_: None,
                name: Some("test_tool".to_string()),
                description: Some("This is a comprehensive description of the tool that explains its purpose and behavior in great detail.".to_string()),
                input_schema: Some(large_schema),
            }]),
            ..Default::default()
        };
        let overhead_large = calculate_claude_fixed_overhead(&req_with_schema);

        assert!(
            overhead_large > overhead_small + 50,
            "Overhead must properly include tool descriptions and input_schemas"
        );
    }

    #[test]
    fn test_compact_sessions_capacity_and_prune() {
        for i in 0..1010 {
            COWORK_COMPACT_SESSIONS.insert(
                format!("test_cap_session_{}", i),
                CoworkCompactState {
                    kind: CoworkCompactKind::Auto,
                    before_tokens: 50_000,
                    ts: std::time::Instant::now() - std::time::Duration::from_secs(350),
                    summary_done: false,
                    target_limit: 35_000,
                },
            );
        }

        prune_compact_sessions_if_needed();
        assert!(
            COWORK_COMPACT_SESSIONS.len() <= MAX_COMPACT_SESSIONS_CAPACITY,
            "Pruning must enforce capacity limit and evict timed-out entries"
        );
    }

    #[test]
    fn test_compaction_lease_expiration_does_not_fall_back_to_weak_continuation() {
        let session_key = "test_lease_no_weak_fallback".to_string();
        COMPACTION_IMMUNITY_LEASES.insert(
            session_key.clone(),
            CompactionImmunityLease {
                created_at: std::time::Instant::now() - std::time::Duration::from_secs(100),
                last_touched: std::time::Instant::now() - std::time::Duration::from_secs(50),
                consumed: true,
            },
        );

        let is_compaction_request = false;
        let is_post_compaction = if !is_compaction_request {
            if let Some(mut lease) = COMPACTION_IMMUNITY_LEASES.get_mut(&session_key) {
                let now = std::time::Instant::now();
                let valid = if !lease.consumed {
                    now.duration_since(lease.created_at).as_secs()
                        < COMPACTION_IMMUNITY_INITIAL_TTL_SECS
                } else {
                    now.duration_since(lease.last_touched).as_secs()
                        < COMPACTION_IMMUNITY_WINDOW_SECS
                };
                if valid {
                    lease.consumed = true;
                    lease.last_touched = now;
                    true
                } else {
                    drop(lease);
                    COMPACTION_IMMUNITY_LEASES.remove(&session_key);
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        assert!(
            !is_post_compaction,
            "Expired lease must strictly return false without granting permanent immunity"
        );
        assert!(!COMPACTION_IMMUNITY_LEASES.contains_key(&session_key));
    }

    #[test]
    fn test_detect_post_compaction_continuation_multi_block() {
        use crate::proxy::mappers::claude::models::{ContentBlock, Message, MessageContent};

        // Case 1: First block is <system-reminder>, second block is continuation text
        let req_multi_block = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Array(vec![
                    ContentBlock::Text {
                        text: "<system-reminder>Some system reminder</system-reminder>".to_string(),
                    },
                    ContentBlock::Text {
                        text: "This session is being continued from a previous conversation that ran out of context.".to_string(),
                    },
                ]),
            }],
            ..Default::default()
        };

        assert!(
            detect_post_compaction_continuation(&req_multi_block),
            "Multi-block continuation must be detected even when preceded by system reminder"
        );

        // Case 2: Negative case
        let req_normal = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Hello, how are you?".to_string()),
            }],
            ..Default::default()
        };
        assert!(!detect_post_compaction_continuation(&req_normal));
    }

    #[test]
    fn test_calculate_effective_auto_compact_threshold_guarantees_headroom() {
        // User sets threshold to 50k, but large tool overhead makes target_limit 53k
        let user_threshold = 50_000;
        let target_limit = 53_000;
        let effective = calculate_effective_auto_compact_threshold(user_threshold, target_limit);

        // Effective threshold must be at least target_limit + 25,000 = 78,000
        assert_eq!(effective, 78_000);
        assert!(effective >= target_limit + 25_000);

        // If user threshold is generous (e.g. 120k), user threshold is respected
        let generous_threshold = 120_000;
        assert_eq!(
            calculate_effective_auto_compact_threshold(generous_threshold, target_limit),
            120_000
        );
    }

    #[test]
    fn test_unified_compaction_state_machine_prevents_collision() {
        let session_key = "test_unified_compaction_collision".to_string();

        // 1. Auto-compaction triggers, registers in unified state
        let target_limit = 45_000;
        COWORK_COMPACT_SESSIONS.insert(
            session_key.clone(),
            CoworkCompactState {
                kind: CoworkCompactKind::Auto,
                before_tokens: 90_000,
                ts: std::time::Instant::now(),
                summary_done: false,
                target_limit,
            },
        );

        // 2. Both manual and auto compact recognize the session is in compaction
        assert!(is_session_in_compaction(&session_key));

        // 3. Summary request arrives, marks summary_done
        if let Some(mut state) = COWORK_COMPACT_SESSIONS.get_mut(&session_key) {
            state.summary_done = true;
        }

        // 4. Continuation arrives with reduced context (e.g. 42_000 tokens)
        let est_tokens = 42_000;
        if let Some((_, state)) = COWORK_COMPACT_SESSIONS.remove(&session_key) {
            let saved = state.before_tokens.saturating_sub(est_tokens);
            assert_eq!(saved, 48_000);
            COWORK_JUST_COMPACTED_CACHE.insert(
                session_key.clone(),
                (
                    std::time::Instant::now(),
                    format!("Compacted conversation · saved {}k tokens", saved / 1000),
                    est_tokens,
                ),
            );
        }

        // 5. Active compaction state is cleared, cache is populated
        assert!(!is_session_in_compaction(&session_key));
        assert!(COWORK_JUST_COMPACTED_CACHE.contains_key(&session_key));

        // Clean up
        COWORK_JUST_COMPACTED_CACHE.remove(&session_key);
    }
}
