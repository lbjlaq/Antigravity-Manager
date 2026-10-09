// Claude 请求转换 (Claude → Gemini v1internal)
// 对应 transformClaudeRequestIn

use super::models::*;
use crate::proxy::mappers::signature_store::get_thought_signature; // Deprecated, kept for fallback
use crate::proxy::session_manager::SessionManager;
use serde_json::{json, Value};
use std::collections::HashMap;

#[cfg(test)]
const CLAUDE_AGENT_SDK_IDENTITY: &str =
    "You are a Claude agent, built on Anthropic's Claude Agent SDK.";
#[cfg(test)]
const CLAUDE_CODE_CLI_IDENTITY: &str = "You are Claude Code, Anthropic's official CLI for Claude.";

/// Normalize the standalone identity block injected by Claude Agent SDK clients.
///
/// Antigravity's upstream currently classifies this SDK identity differently from
/// Claude Code's CLI identity and can reject an otherwise identical request with
/// RESOURCE_EXHAUSTED. Keep the match exact so user-authored text that merely
/// mentions the SDK identity is not rewritten.
fn normalize_claude_client_identity(text: &str) -> &str {
    crate::proxy::mappers::prompt_sanitizer::PromptSanitizer::normalize_client_identity(text)
}

// ===== Safety Settings Configuration =====

/// Safety threshold levels for Gemini API
/// Can be configured via GEMINI_SAFETY_THRESHOLD environment variable
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SafetyThreshold {
    /// Disable all safety filters (default for proxy compatibility)
    Off,
    /// Block low probability and above
    BlockLowAndAbove,
    /// Block medium probability and above
    BlockMediumAndAbove,
    /// Only block high probability content
    BlockOnlyHigh,
    /// Don't block anything (BLOCK_NONE)
    BlockNone,
}

impl SafetyThreshold {
    /// Get threshold from environment variable or default to Off
    pub fn from_env() -> Self {
        match std::env::var("GEMINI_SAFETY_THRESHOLD").as_deref() {
            Ok("OFF") | Ok("off") => SafetyThreshold::Off,
            Ok("LOW") | Ok("low") => SafetyThreshold::BlockLowAndAbove,
            Ok("MEDIUM") | Ok("medium") => SafetyThreshold::BlockMediumAndAbove,
            Ok("HIGH") | Ok("high") => SafetyThreshold::BlockOnlyHigh,
            Ok("NONE") | Ok("none") => SafetyThreshold::BlockNone,
            _ => SafetyThreshold::Off, // Default: maintain current behavior
        }
    }

    /// Convert to Gemini API threshold string
    pub fn to_gemini_threshold(&self) -> &'static str {
        match self {
            SafetyThreshold::Off => "OFF",
            SafetyThreshold::BlockLowAndAbove => "BLOCK_LOW_AND_ABOVE",
            SafetyThreshold::BlockMediumAndAbove => "BLOCK_MEDIUM_AND_ABOVE",
            SafetyThreshold::BlockOnlyHigh => "BLOCK_ONLY_HIGH",
            SafetyThreshold::BlockNone => "BLOCK_NONE",
        }
    }
}

/// Build safety settings based on configuration
fn build_safety_settings() -> Value {
    let threshold = SafetyThreshold::from_env();
    let threshold_str = threshold.to_gemini_threshold();

    json!([
        { "category": "HARM_CATEGORY_HARASSMENT", "threshold": threshold_str },
        { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": threshold_str },
        { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": threshold_str },
        { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": threshold_str },
    ])
}

/// 清理消息中的 cache_control 字段
///
/// 这个函数会深度遍历所有消息内容块,移除 cache_control 字段。
/// 这是必要的,因为:
/// 1. VS Code 等客户端会将历史消息(包含 cache_control)原封不动发回
/// 2. Anthropic API 不接受请求中包含 cache_control 字段
/// 3. 即使是转发到 Gemini,也应该清理以保持协议纯净性
///
/// [FIX #593] 增强版本:添加详细日志用于调试 MCP 工具兼容性问题
pub fn clean_cache_control_from_messages(messages: &mut [Message]) {
    tracing::info!(
        "[DEBUG-593] Starting cache_control cleanup for {} messages",
        messages.len()
    );

    let mut total_cleaned = 0;

    for (idx, msg) in messages.iter_mut().enumerate() {
        if let MessageContent::Array(blocks) = &mut msg.content {
            for (block_idx, block) in blocks.iter_mut().enumerate() {
                match block {
                    ContentBlock::Thinking { cache_control, .. } => {
                        if cache_control.is_some() {
                            tracing::info!(
                                "[ISSUE-744] Found cache_control in Thinking block at message[{}].content[{}]: {:?}",
                                idx,
                                block_idx,
                                cache_control
                            );
                            *cache_control = None;
                            total_cleaned += 1;
                        }
                    }
                    ContentBlock::Image { cache_control, .. } => {
                        if cache_control.is_some() {
                            tracing::debug!(
                                "[Cache-Control-Cleaner] Removed cache_control from Image block at message[{}].content[{}]",
                                idx,
                                block_idx
                            );
                            *cache_control = None;
                            total_cleaned += 1;
                        }
                    }
                    ContentBlock::Document { cache_control, .. } => {
                        if cache_control.is_some() {
                            tracing::debug!(
                                "[Cache-Control-Cleaner] Removed cache_control from Document block at message[{}].content[{}]",
                                idx,
                                block_idx
                            );
                            *cache_control = None;
                            total_cleaned += 1;
                        }
                    }
                    ContentBlock::ToolUse { cache_control, .. } => {
                        if cache_control.is_some() {
                            tracing::debug!(
                                "[Cache-Control-Cleaner] Removed cache_control from ToolUse block at message[{}].content[{}]",
                                idx,
                                block_idx
                            );
                            *cache_control = None;
                            total_cleaned += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if total_cleaned > 0 {
        tracing::info!(
            "[DEBUG-593] Cache control cleanup complete: removed {} cache_control fields",
            total_cleaned
        );
    } else {
        tracing::debug!("[DEBUG-593] No cache_control fields found");
    }
}

/// [FIX #593] 递归深度清理 JSON 中的 cache_control 字段
///
/// 用于处理嵌套结构和非标准位置的 cache_control。
/// 这是最后一道防线,确保发送给 Antigravity 的请求中不包含任何 cache_control。
fn deep_clean_cache_control(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.remove("cache_control").is_some() {
                tracing::debug!("[DEBUG-593] Removed cache_control from nested JSON object");
            }
            for (_, v) in map.iter_mut() {
                deep_clean_cache_control(v);
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                deep_clean_cache_control(item);
            }
        }
        _ => {}
    }
}

/// [FIX #564] Sort blocks in assistant messages to ensure thinking blocks are first
///
/// When context compression (kilo) reorders message blocks, thinking blocks may appear
/// after text blocks. Claude/Anthropic API requires thinking blocks to be first if
/// any thinking blocks exist in the message. This function pre-sorts blocks to ensure
/// thinking/redacted_thinking blocks always come before other block types.
fn sort_thinking_blocks_first(messages: &mut [Message]) {
    for msg in messages.iter_mut() {
        if msg.role == "assistant" {
            if let MessageContent::Array(blocks) = &mut msg.content {
                // [FIX #709] Triple-stage partition: [Thinking, Text, ToolUse]
                // This ensures protocol compliance while maintaining logical order.

                let mut thinking_blocks: Vec<ContentBlock> = Vec::new();
                let mut text_blocks: Vec<ContentBlock> = Vec::new();
                let mut tool_blocks: Vec<ContentBlock> = Vec::new();
                let mut other_blocks: Vec<ContentBlock> = Vec::new();

                let original_len = blocks.len();
                let mut needs_reorder = false;
                let mut saw_non_thinking = false;

                for (_i, block) in blocks.iter().enumerate() {
                    match block {
                        ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. } => {
                            if saw_non_thinking {
                                needs_reorder = true;
                            }
                        }
                        ContentBlock::Text { .. } => {
                            saw_non_thinking = true;
                        }
                        ContentBlock::ToolUse { .. } => {
                            saw_non_thinking = true;
                            // Check if tool is after text (this is normal, but we want a strict group order)
                        }
                        _ => saw_non_thinking = true,
                    }
                }

                if needs_reorder || original_len > 1 {
                    // For safety, we always perform the triple partition if there's more than one block.
                    // This also handles empty text block filtering.
                    for block in blocks.drain(..) {
                        match &block {
                            ContentBlock::Thinking { .. }
                            | ContentBlock::RedactedThinking { .. } => {
                                thinking_blocks.push(block);
                            }
                            ContentBlock::Text { text } => {
                                // Filter out purely empty or structural text like "(no content)"
                                if !text.trim().is_empty() && text != "(no content)" {
                                    text_blocks.push(block);
                                }
                            }
                            ContentBlock::ToolUse { .. } => {
                                tool_blocks.push(block);
                            }
                            _ => {
                                other_blocks.push(block);
                            }
                        }
                    }

                    // Reconstruct in strict order: Thinking -> Text/Other -> Tool
                    blocks.extend(thinking_blocks);
                    blocks.extend(text_blocks);
                    blocks.extend(other_blocks);
                    blocks.extend(tool_blocks);

                    if needs_reorder {
                        tracing::warn!(
                            "[FIX #709] Reordered assistant messages to [Thinking, Text, Tool] structure."
                        );
                    }
                }
            }
        }
    }
}

/// 合并 ClaudeRequest 中连续的同角色消息
///
/// 场景: 当从 Spec/Plan 模式切换回编码模式时，可能出现连续两条 "user" 消息
/// (一条是 ToolResult，一条是 <system-reminder>)。
/// 这会违反角色交替规则，导致 400 报错。
pub fn merge_consecutive_messages(messages: &mut Vec<Message>) {
    if messages.len() <= 1 {
        return;
    }

    let mut merged: Vec<Message> = Vec::with_capacity(messages.len());
    let old_messages = std::mem::take(messages);
    let mut messages_iter = old_messages.into_iter();

    if let Some(mut current) = messages_iter.next() {
        for next in messages_iter {
            if current.role == next.role {
                // 合并内容
                match (&mut current.content, next.content) {
                    (MessageContent::Array(current_blocks), MessageContent::Array(next_blocks)) => {
                        current_blocks.extend(next_blocks);
                    }
                    (MessageContent::Array(current_blocks), MessageContent::String(next_text)) => {
                        current_blocks.push(ContentBlock::Text { text: next_text });
                    }
                    (MessageContent::String(current_text), MessageContent::String(next_text)) => {
                        *current_text = format!("{}\n\n{}", current_text, next_text);
                    }
                    (MessageContent::String(current_text), MessageContent::Array(next_blocks)) => {
                        let mut new_blocks = vec![ContentBlock::Text {
                            text: current_text.clone(),
                        }];
                        new_blocks.extend(next_blocks);
                        current.content = MessageContent::Array(new_blocks);
                    }
                }
            } else {
                merged.push(current);
                current = next;
            }
        }
        merged.push(current);
    }

    *messages = merged;
}

/// 转换 Claude 请求为 Gemini v1internal 格式

/// [FIX #709] Reorder serialized Gemini parts to ensure thinking blocks are first,
/// deduplicate multiple thinking blocks to at most ONE per model turn, and clean out raw dot placeholders.
fn reorder_gemini_parts(parts: &mut Vec<Value>) {
    if parts.len() <= 1 {
        return;
    }

    let mut thinking_parts = Vec::new();
    // 非思考部件**保持原有相对顺序**。
    //
    // 历史实现按 `[text] → [other] → [tool]` 重排，把 `functionCall` 挤到最后：
    // 同一段对话经 Claude 入口与经 OpenAI / Gemini 入口会产出不同的 `contents`
    // （前缀缓存哈希必然不命中），且在 text / tool 混排轮上移动签名锚点。
    // parts 顺序即锚点语义，除「思考块置于首位」外不得改写。
    let mut rest_parts = Vec::new();

    for part in parts.drain(..) {
        if part.get("thought").and_then(|t| t.as_bool()) == Some(true) {
            thinking_parts.push(part);
        } else if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
            let t = text.trim();
            // Filter empty / dummy text parts that might have been created during merging or from client dot echo
            if !t.is_empty() && t != "(no content)" && t != "·" {
                rest_parts.push(part);
            }
        } else {
            rest_parts.push(part);
        }
    }

    // Keep at most ONE thinking block per model turn (the one with longest text or real signature)
    if thinking_parts.len() > 1 {
        let best_thought = thinking_parts.into_iter().max_by_key(|p| {
            let sig_len = p
                .get("thoughtSignature")
                .or_else(|| p.get("thought_signature"))
                .and_then(|s| s.as_str())
                .map(|s| s.len())
                .unwrap_or(0);
            let text_len = p
                .get("text")
                .and_then(|t| t.as_str())
                .map(|t| t.len())
                .unwrap_or(0);
            (sig_len, text_len)
        });
        if let Some(t) = best_thought {
            parts.push(t);
        }
    } else if let Some(t) = thinking_parts.pop() {
        parts.push(t);
    }

    parts.extend(rest_parts);
}

#[derive(Debug, Default, Clone, Copy)]
pub struct TransformTiming {
    pub think_fill_micros: u64,
}

pub fn transform_claude_request_in(
    claude_req: &ClaudeRequest,
    project_id: &str,
    is_retry: bool,
    account_id: Option<&str>,
    session_id: &str,
    token: Option<&crate::proxy::token_manager::ProxyToken>,
) -> Result<Value, String> {
    transform_claude_request_in_timed(
        claude_req, project_id, is_retry, account_id, session_id, session_id, token,
    )
    .map(|(body, _)| body)
}

pub fn transform_claude_request_in_timed(
    claude_req: &ClaudeRequest,
    project_id: &str,
    is_retry: bool,
    account_id: Option<&str>,
    thinking_session_id: &str,
    _upstream_session_id: &str,
    token: Option<&crate::proxy::token_manager::ProxyToken>, // [NEW] 支持动态规格
) -> Result<(Value, TransformTiming), String> {
    let mut timing = TransformTiming::default();
    let message_count = claude_req.messages.len();

    // [CRITICAL FIX] 预先清理所有消息中的 cache_control 字段
    // 这解决了 VS Code 插件等客户端在多轮对话中将历史消息的 cache_control 字段
    // 原封不动发回导致的 "Extra inputs are not permitted" 错误
    let mut cleaned_req = claude_req.clone();

    // [JEIKCODE FROZEN SYSTEM PRINCIPLE]
    // 仅收集最开头的连续 role == "system" 消息进入 extra_system_messages。
    // 一旦对话开始（遇到首个非 system 轮次），后续中途出现的任何 system 消息绝对严禁提升至 systemInstruction，
    // 否则会导致发往 Google 上游的顶层系统前缀发生字节级突变并引发 KV Cache 崩溃！
    // 中途 system 消息就地转为 synthetic user 消息留在原时间线中，并在后续由 merge_consecutive_messages 自然融合。
    let mut extra_system_messages = Vec::new();
    let mut filtered_messages = Vec::new();
    let mut in_leading_system = true;

    for msg in cleaned_req.messages {
        if msg.role == "system" {
            if in_leading_system {
                match &msg.content {
                    MessageContent::String(text) => {
                        extra_system_messages.push(text.clone());
                    }
                    MessageContent::Array(blocks) => {
                        for block in blocks {
                            if let ContentBlock::Text { text } = block {
                                extra_system_messages.push(text.clone());
                            }
                        }
                    }
                }
            } else {
                let text = match &msg.content {
                    MessageContent::String(t) => t.clone(),
                    MessageContent::Array(blocks) => {
                        let mut joined = String::new();
                        for b in blocks {
                            if let ContentBlock::Text { text } = b {
                                if !joined.is_empty() {
                                    joined.push('\n');
                                }
                                joined.push_str(text);
                            }
                        }
                        joined
                    }
                };
                let wrapped_reminder =
                    crate::proxy::mappers::common_utils::wrap_in_system_reminder(&text);
                if !wrapped_reminder.is_empty() {
                    filtered_messages.push(Message {
                        role: "user".to_string(),
                        content: MessageContent::String(wrapped_reminder),
                    });
                }
            }
        } else {
            in_leading_system = false;
            filtered_messages.push(msg);
        }
    }
    cleaned_req.messages = filtered_messages;

    // [FIX #813] Anthropic / z.ai 要求角色交替。这里只做源协议的相邻同角色归一。
    // 转成 Gemini contents 之后，相邻 user / model / functionResponse 的合并由进站流水线负责。
    merge_consecutive_messages(&mut cleaned_req.messages);

    clean_cache_control_from_messages(&mut cleaned_req.messages);

    // [FIX #564] Pre-sort thinking blocks to be first in assistant messages
    // This handles cases where context compression (kilo) incorrectly reorders blocks
    sort_thinking_blocks_first(&mut cleaned_req.messages);

    // [FIX #1747] If thinking is auto-enabled by model default (e.g. Opus) but no
    // ThinkingConfig was provided by the client, inject a default config with a budget
    // to prevent 'thinking requires a budget' errors from upstream APIs.
    if cleaned_req.thinking.is_none() && should_enable_thinking_by_default(&cleaned_req.model) {
        let default_budget =
            crate::proxy::model_specs::get_thinking_budget(&cleaned_req.model, token);
        tracing::info!(
            "[Thinking-Mode] Injecting default ThinkingConfig (budget={}) for model: {}",
            default_budget,
            cleaned_req.model
        );
        cleaned_req.thinking = Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(default_budget as u32),
            effort: None,
        });
    }

    let claude_req = &cleaned_req; // 后续使用清理后的请求

    // 思维库和上游 sessionId 都用带锚点的 store key。账号粘性不经过这里。
    let session_id = if !thinking_session_id.is_empty() {
        thinking_session_id.to_string()
    } else {
        SessionManager::extract_session_id(claude_req)
    };
    tracing::debug!("[Claude-Request] Session ID: {}", session_id);

    // 检测是否有联网工具 (server tool or built-in tool)
    let has_web_search_tool = claude_req
        .tools
        .as_ref()
        .map(|tools| {
            tools.iter().any(|t| {
                t.is_web_search()
                    || t.name.as_deref() == Some("google_search")
                    || t.name.as_deref() == Some("builtin_web_search")
                    || t.type_.as_deref() == Some("web_search_20250305")
                    || t.type_.as_deref() == Some("builtin_web_search")
            })
        })
        .unwrap_or(false);

    // 用于存储 tool_use id -> name 映射
    let mut tool_id_to_name: HashMap<String, String> = HashMap::new();

    // [New] 预先构建工具名称到原始 Schema 的映射，用于后续参数类型修正
    let mut tool_name_to_schema = HashMap::new();
    if let Some(tools) = &claude_req.tools {
        for tool in tools {
            if let (Some(name), Some(schema)) = (&tool.name, &tool.input_schema) {
                tool_name_to_schema.insert(name.clone(), schema.clone());
            }
        }
    }

    //  Map model name (Use standard mapping)
    // [IMPROVED] 提取 web search 模型为常量，便于维护
    const WEB_SEARCH_FALLBACK_MODEL: &str = "gemini-2.5-flash";

    let mapped_model =
        crate::proxy::common::model_mapping::map_claude_model_to_gemini(&claude_req.model);

    // 1. System Instruction (透传系统提示词分块，若目标为 Gemini 则自动过滤无用计费元数据 #3452)
    let system_instruction =
        build_system_instruction(&claude_req.system, &mapped_model, &extra_system_messages);

    // 将 Claude 工具转为 Value 数组以便探测联网
    let tools_val: Option<Vec<Value>> = claude_req.tools.as_ref().map(|list| {
        list.iter()
            .map(|t| serde_json::to_value(t).unwrap_or(json!({})))
            .collect()
    });

    // Resolve grounding config
    let config = crate::proxy::mappers::common_utils::resolve_request_config(
        &claude_req.model,
        &mapped_model,
        &tools_val,
        claude_req.size.as_deref(),    // [NEW] Pass size parameter
        claude_req.quality.as_deref(), // [NEW] Pass quality parameter
        None,                          // [NEW] image_size
        None,                          // body
    );

    // [CRITICAL FIX] Disable dummy thought injection for Vertex AI
    // [CRITICAL FIX] Disable dummy thought injection for Vertex AI
    // Vertex AI rejects thinking blocks without valid signatures
    // Even if thinking is enabled, we should NOT inject dummy blocks for historical messages
    let allow_dummy_thought = false;

    // Check if thinking is enabled in the request
    let client_switch = crate::proxy::pipeline::extract_client_thinking_switch(
        claude_req.thinking.as_ref().map(|t| t.type_.as_str()),
        claude_req
            .thinking
            .as_ref()
            .and_then(|t| t.budget_tokens.map(|b| b as u64)),
        claude_req
            .output_config
            .as_ref()
            .and_then(|c| c.effort.as_deref())
            .or_else(|| {
                claude_req
                    .thinking
                    .as_ref()
                    .and_then(|t| t.effort.as_deref())
            }),
    );

    let tb_config = crate::proxy::config::get_thinking_budget_config();
    let is_client_control =
        tb_config.control_source == crate::proxy::config::ThinkingControlSource::Client;
    let is_client_disabled = is_client_control && client_switch.is_disabled();

    let thinking_type = claude_req.thinking.as_ref().map(|t| t.type_.as_str());
    let force_server_thinking = crate::proxy::thinking_store::any_model_forces_server_thinking(&[
        claude_req.model.as_str(),
        mapped_model.as_str(),
    ]);
    let target_model_supports_thinking = model_supports_thinking(&mapped_model);
    let is_under_v3 = crate::proxy::model_specs::is_gemini_under_v3(&mapped_model)
        || crate::proxy::model_specs::is_gemini_under_v3(&claude_req.model);
    let mut is_thinking_enabled = !is_client_disabled
        && !is_under_v3
        && (target_model_supports_thinking
            || force_server_thinking
            || thinking_type == Some("enabled")
            || thinking_type == Some("adaptive")
            || (thinking_type.is_none() && should_enable_thinking_by_default(&claude_req.model)));

    if is_thinking_enabled && !target_model_supports_thinking && !force_server_thinking {
        tracing::warn!(
            "[Thinking-Mode] Target model '{}' does not support thinking. Force disabling thinking mode.",
            mapped_model
        );
        is_thinking_enabled = false;
    }

    // Check signature requirements for function calls
    if is_thinking_enabled {
        let _global_sig = get_thought_signature();

        // Check if there are function calls in the request
        let has_function_calls = claude_req.messages.iter().any(|m| {
            if let MessageContent::Array(blocks) = &m.content {
                blocks
                    .iter()
                    .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
            } else {
                false
            }
        });

        if has_function_calls {
            // [FIX #2167] Rely on ThinkingStore / sentinel injection rather than disabling thinking
            tracing::info!(
                "[Thinking-Mode] Function calls present. Relying on InboundThinkingPipeline restoration and sentinel injection."
            );
        }
    }

    // 4. Generation Config & Thinking (Pass final is_thinking_enabled)
    let generation_config = build_generation_config(
        claude_req,
        &mapped_model,
        has_web_search_tool,
        is_thinking_enabled,
        token, // [NEW] 传递 token 用于动态限额
    );

    // 2. Contents (Messages)
    let contents = build_google_contents(
        &claude_req.messages,
        claude_req,
        &mut tool_id_to_name,
        &tool_name_to_schema,
        is_thinking_enabled,
        allow_dummy_thought,
        &mapped_model,
        &session_id,
        is_retry,
        &mut timing,
    )?;

    // 3. Tools
    let tools = build_tools(&claude_req.tools, has_web_search_tool, &mapped_model)?;

    // 5. Safety Settings (configurable via GEMINI_SAFETY_THRESHOLD env var)
    let safety_settings = build_safety_settings();

    // Build inner request
    let mut inner_request = json!({
        "contents": contents,
        "safetySettings": safety_settings,
    });

    if let Some(sys_inst) = system_instruction {
        inner_request["systemInstruction"] = sys_inst;
    }

    if !generation_config.is_null() {
        inner_request["generationConfig"] = generation_config;
    }

    if let Some(tools_val) = tools {
        inner_request["tools"] = tools_val;
    }

    // [tool_choice] 客户端有就传，没有就不传：
    // 若客户端显式指定了 tool_choice，将其规范化映射为标准的 Gemini toolConfig
    if let Some(tool_choice) = &claude_req.tool_choice {
        if let Some(gemini_tc) =
            crate::proxy::mappers::common_utils::map_claude_tool_choice_to_gemini(tool_choice)
        {
            inner_request["toolConfig"] = gemini_tc;
        }
    }

    // [PIPELINE] 统一清洗提示词与风控伪 Header（含 [undefined] 深度清理，见 PromptSanitizer）
    crate::proxy::mappers::prompt_sanitizer::PromptSanitizer::sanitize_gemini_payload(
        &mut inner_request,
    );

    if config.inject_google_search && !has_web_search_tool {
        crate::proxy::mappers::common_utils::inject_google_search_tool(
            &mut inner_request,
            Some(&mapped_model),
        );
    }

    // Inject imageConfig if present (for image generation models)
    if let Some(image_config) = config.image_config {
        if let Some(obj) = inner_request.as_object_mut() {
            // 1. Remove tools (image generation does not support tools)
            obj.remove("tools");

            // 2. Remove systemInstruction (image generation does not support system prompts)
            obj.remove("systemInstruction");

            // 3. Clean generationConfig (remove responseMimeType, responseModalities etc.)
            let gen_config = obj.entry("generationConfig").or_insert_with(|| json!({}));
            if let Some(gen_obj) = gen_config.as_object_mut() {
                // [RESOLVE #1694] Check image thinking mode
                let image_thinking_mode = crate::proxy::config::get_image_thinking_mode();
                if image_thinking_mode == "disabled" {
                    tracing::debug!(
                        "[Claude-Request] Image thinking mode disabled: enforcing includeThoughts=false for {}",
                        mapped_model
                    );
                    gen_obj.insert(
                        "thinkingConfig".to_string(),
                        json!({
                            "includeThoughts": false
                        }),
                    );
                }

                gen_obj.remove("responseMimeType");
                gen_obj.remove("responseModalities");
                gen_obj.insert("imageConfig".to_string(), image_config);
            }
        }
    }

    // [ADDED v4.1.24] 注入稳定 sessionId 对齐官方规范
    // [FIX session-1M] 混入对话指纹与代数,不同对话隔离服务端会话,1M 累计报错后 bump 自愈
    if let Some(account_id) = account_id {
        crate::proxy::common::session::apply_upstream_session(
            &mut inner_request,
            account_id,
            &session_id,
        );
    }

    // 生成 requestId —— 官方 5 段形态，三适配器共用。
    // 必须含 unixMs：历史实现为 `agent/antigravity/{session[:8]}/{count}`，**不含时间戳**，
    // 同一会话同一轮次重试会拿到完全相同的 ID，从而 pin 到上一次的 429 / 旧缓存。
    let request_id =
        super::super::common_utils::build_official_request_id(&session_id, message_count as u64);

    // 官方客户端指纹（企业 / GCP 账号为 jetski）—— 三适配器共用，避免指纹漂移
    let (official_user_agent, _official_ide_type) =
        super::super::common_utils::resolve_official_fingerprint(token);

    // [CACHE] 统一委托进站流水线进行前缀拓扑规范化与对齐（Pipeline First 核心归一）
    crate::proxy::pipeline::InboundThinkingPipeline::align_google_request_prefix_topology_with_model(
        &mut inner_request,
        &config.final_model,
        Some(&request_id),
    );
    let reordered_inner = inner_request;

    // [NEW] 动态检测是否需要标记为 agent 请求
    let has_tools = reordered_inner
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|arr| !arr.is_empty())
        .unwrap_or(false);
    let has_tool_interactions = reordered_inner
        .get("contents")
        .map(super::super::common_utils::contents_has_tool_interactions)
        .unwrap_or(false);
    let is_agent_request =
        config.request_type != "image_gen" && (has_tools || has_tool_interactions);

    // 构建最终请求体 (顶层键序对齐官方: project -> requestId -> request -> model -> userAgent -> requestType)
    let mut body = json!({
        "project": project_id,
        "requestId": request_id,
        "request": reordered_inner,
        "model": config.final_model,
        "userAgent": official_user_agent,
    });

    if config.request_type == "image_gen" {
        body["requestType"] = json!("image_gen");
    } else if is_agent_request {
        body["requestType"] = json!("agent");
    }

    crate::proxy::pipeline::InboundThinkingPipeline::align_official_envelope(&mut body);

    // [FIX #593] 最后一道防线: 递归深度清理所有 cache_control 字段
    // 确保发送给 Antigravity 的请求中不包含任何 cache_control
    deep_clean_cache_control(&mut body);
    tracing::debug!("[DEBUG-593] Final deep clean complete, request ready to send");

    // [DEFENSE] 净化所有 contents 中的 inlineData，过滤或降级空数据/损坏数据
    if let Some(inner) = body.get_mut("request") {
        crate::proxy::mappers::common_utils::sanitize_gemini_payload_inline_data(inner);
    }

    Ok((body, timing))
}

/// Check if thinking mode should be enabled by default for a given model
///
/// Claude Code v2.0.67+ enables thinking by default for Opus 4.5 models.
/// This function determines if the model should have thinking enabled
/// when no explicit thinking configuration is provided.
fn should_enable_thinking_by_default(model: &str) -> bool {
    // [保底防御] Gemini < 3 的模型（例如 gemini-2.5-flash, gemini-2.5-pro, gemini-2.0 等），直接不注入思考参数，保留思考为关
    if crate::proxy::model_specs::is_gemini_under_v3(model) {
        return false;
    }
    if crate::proxy::thinking_store::model_forces_server_thinking(model) {
        return true;
    }
    let model_lower = model.to_lowercase();

    // Enable thinking by default for Opus 4.5 and 4.6 variants
    if model_lower.contains("opus-4-5")
        || model_lower.contains("opus-4.5")
        || model_lower.contains("opus-4-6")
        || model_lower.contains("opus-4.6")
    {
        tracing::debug!(
            "[Thinking-Mode] Auto-enabling thinking for Opus model: {}",
            model
        );
        return true;
    }

    // Also enable for explicit thinking model variants
    if model_lower.contains("-thinking") {
        return true;
    }

    // Gemini 3 及以上模型（如 gemini-3.7-flash, gemini-3-pro, gemini-3.1-pro, gemini-3.8-flash 等）强行开启思考
    if crate::proxy::model_specs::is_gemini_v3_or_above(model) {
        tracing::debug!(
            "[Thinking-Mode] Auto-enabling thinking for Gemini 3+ model: {}",
            model
        );
        return true;
    }

    false
}

/// Whether the mapped target model supports Gemini `thinkingConfig`.
///
/// `gemini-pro-agent` is the mapped target of `gemini-3.1-pro-high` /
/// `gemini-3-pro-high` (`model_mapping.rs`) and is a forced-thinking Pro agent
/// model. It is kept in sync with the OpenAI protocol path
/// (`openai/request.rs` `is_gemini_3_thinking` includes `-pro-agent`) and the
/// model spec `SPEC_PRO_AGENT { include_thoughts: true }` (`variant_mapping.rs`).
fn model_supports_thinking(mapped_model: &str) -> bool {
    if crate::proxy::thinking_store::model_forces_server_thinking(mapped_model) {
        return true;
    }
    if crate::proxy::model_specs::is_gemini_v3_or_above(mapped_model) {
        return true;
    }
    // [保底防御] Gemini < 3 的普通非思考模型（如 gemini-2.5-flash）不支持 thinkingConfig，严禁注入
    if crate::proxy::model_specs::is_gemini_under_v3(mapped_model) {
        return false;
    }
    mapped_model.contains("-thinking") || mapped_model.starts_with("claude-")
}

/// Minimum length for a valid thought_signature
const MIN_SIGNATURE_LENGTH: usize = 32;

/// Sentinel signature for models that support skipping signature validation
const SENTINEL_SIGNATURE: &str = "skip_thought_signature_validator";

fn clean_system_prompt_text(text: &str) -> String {
    crate::proxy::mappers::prompt_sanitizer::PromptSanitizer::strip_pipeline_markers(text)
}

fn is_gemini_client_billing_metadata(_model: &str, text: &str) -> bool {
    crate::proxy::mappers::prompt_sanitizer::PromptSanitizer::is_billing_metadata(text)
}

/// 构建 System Instruction (支持动态身份映射与 Prompt 隔离)
fn build_system_instruction(
    system: &Option<SystemPrompt>,
    model_name: &str,
    extra_system_messages: &[String],
) -> Option<Value> {
    let mut parts = Vec::new();

    // 不注入官方 Antigravity 身份：客户端自带 system prompt 时原样透传。
    // 全局提示词仍做清洗 + 换行隔离，避免 Markdown 粘连；身份文本若混入则由 clean_system_prompt_text 剥掉。
    let global_prompt_config = crate::proxy::config::get_global_system_prompt();
    if global_prompt_config.enabled && !global_prompt_config.content.trim().is_empty() {
        let cleaned = clean_system_prompt_text(&global_prompt_config.content);
        if !cleaned.is_empty() {
            parts.push(json!({"text": format!("{}\n\n", cleaned)}));
        }
    }

    // 添加用户的系统提示词
    if let Some(sys) = system {
        match sys {
            SystemPrompt::String(text) => {
                // [Issue #3452] 过滤 Claude Desktop 注入的单行计费元数据，防止与大量工具组合时触发 Google 上游 429 RESOURCE_EXHAUSTED
                if !is_gemini_client_billing_metadata(model_name, text) {
                    let norm = normalize_claude_client_identity(text);
                    let cleaned = clean_system_prompt_text(norm);
                    if !cleaned.is_empty() {
                        parts.push(json!({"text": cleaned}));
                    }
                }
            }
            SystemPrompt::Array(blocks) => {
                for block in blocks {
                    if block.block_type == "text" {
                        // [Issue #3452] 过滤 Claude Desktop 注入的单行计费元数据
                        if is_gemini_client_billing_metadata(model_name, &block.text) {
                            continue;
                        }
                        let norm = normalize_claude_client_identity(&block.text);
                        let cleaned = clean_system_prompt_text(norm);
                        if !cleaned.is_empty() {
                            parts.push(json!({
                                "text": cleaned
                            }));
                        }
                    }
                }
            }
        }
    }

    // 3. 添加提取出来的 role == "system" 消息
    for extra_text in extra_system_messages {
        if is_gemini_client_billing_metadata(model_name, extra_text) {
            continue;
        }
        let cleaned = clean_system_prompt_text(extra_text);
        if !cleaned.is_empty() {
            parts.push(json!({"text": format!("\n{}", cleaned)}));
        }
    }

    if parts.is_empty() {
        return None;
    }

    Some(json!({
        "role": "user",
        "parts": parts
    }))
}

/// 构建 Contents (Messages)
fn build_contents(
    content: &MessageContent,
    is_assistant: bool,
    _claude_req: &ClaudeRequest,
    is_thinking_enabled: bool,
    _session_id: &str,
    _msg_index: usize,
    _allow_dummy_thought: bool,
    is_retry: bool,
    tool_id_to_name: &mut HashMap<String, String>,
    tool_name_to_schema: &HashMap<String, Value>,
    mapped_model: &str,
    last_thought_signature: &mut Option<String>,
    pending_tool_use_ids: &mut Vec<String>,
    last_user_task_text_normalized: &mut Option<String>,
    previous_was_tool_result: &mut bool,
    _existing_tool_result_ids: &std::collections::HashSet<String>,
) -> Result<Vec<Value>, String> {
    let mut parts = Vec::new();
    // Track tool results in the current turn to identify missing ones
    let mut current_turn_tool_result_ids = std::collections::HashSet::new();

    // Pre-scan for this assistant turn's signature to keep each turn strictly isolated
    let mut turn_signature: Option<String> = None;
    if is_assistant {
        if let MessageContent::Array(blocks) = content {
            for b in blocks {
                match b {
                    ContentBlock::Thinking {
                        signature: Some(s), ..
                    } => {
                        if (s == SENTINEL_SIGNATURE || s.len() >= MIN_SIGNATURE_LENGTH)
                            && (!mapped_model.to_lowercase().contains("gemini")
                                || crate::proxy::thinking_store::is_likely_gemini_signature(s))
                        {
                            turn_signature = Some(s.clone());
                            break;
                        }
                    }
                    ContentBlock::ToolUse { signature, .. } => {
                        if let Some(s) = signature {
                            if (s == SENTINEL_SIGNATURE || s.len() >= MIN_SIGNATURE_LENGTH)
                                && (!mapped_model.to_lowercase().contains("gemini")
                                    || crate::proxy::thinking_store::is_likely_gemini_signature(s))
                            {
                                turn_signature = Some(s.clone());
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Track if we have already seen non-thinking content in this message.
    // Anthropic/Gemini protocol: Thinking blocks MUST come first.
    let mut saw_non_thinking = false;

    match content {
        MessageContent::String(text) => {
            if text != "(no content)" {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    parts.extend(
                        crate::proxy::mappers::common_utils::parse_markdown_images_to_parts(
                            trimmed,
                        ),
                    );
                }
            }
        }
        MessageContent::Array(blocks) => {
            let mut turn_tool_media_parts = Vec::new();
            for item in blocks {
                match item {
                    ContentBlock::Text { text } => {
                        if text != "(no content)" && !text.trim().is_empty() {
                            // [2026-09-27] 如果 assistant 消息同时包含 tool_use，且该文本仅为占位符（如 "..."、"·" 等），
                            // 丢弃无实质意义的占位文本，避免在 Gemini 格式转换中被误选为签名锚点，
                            // 导致上游 400 'Function call is missing a thought_signature in functionCall parts'
                            if is_assistant
                                && crate::proxy::thinking_store::is_placeholder_thought(text)
                                && blocks
                                    .iter()
                                    .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
                            {
                                tracing::debug!(
                                    "[Claude-Request] Dropping placeholder text accompanying tool_use (text={:?})",
                                    text
                                );
                                continue;
                            }

                            // [NEW] 任务去重逻辑: 如果当前是 User 消息，且紧跟在 ToolResult 之后，
                            // 检查该文本是否与上一轮任务描述完全一致。
                            if !is_assistant && *previous_was_tool_result {
                                if let Some(last_task) = last_user_task_text_normalized {
                                    let current_normalized =
                                        text.replace(|c: char| c.is_whitespace(), "");
                                    if !current_normalized.is_empty()
                                        && current_normalized == *last_task
                                    {
                                        tracing::info!("[Claude-Request] Dropping duplicated task text echo (len: {})", text.len());
                                        continue;
                                    }
                                }
                            }

                            parts.extend(
                                crate::proxy::mappers::common_utils::parse_markdown_images_to_parts(
                                    text,
                                ),
                            );
                            saw_non_thinking = true;

                            // 记录最近一次 User 任务文本用于后续比对
                            if !is_assistant {
                                *last_user_task_text_normalized =
                                    Some(text.replace(|c: char| c.is_whitespace(), ""));
                            }
                            *previous_was_tool_result = false;
                        }
                    }
                    ContentBlock::Thinking {
                        thinking,
                        signature,
                        ..
                    } => {
                        tracing::debug!(
                            "[DEBUG-TRANSFORM] Processing thinking block. Sig: {:?}",
                            signature
                        );

                        // [2026-09-27] 占位/空思考块直接丢弃（不写思考块、不降级为文本）。
                        // 官方样本（baogao.txt）9/24 轮是「无思考块 + 锚点带签名」，
                        // 空 / "." / "..." / 空格 / "·" 等占位思考无信息量；签名归位
                        // 由流水线终审 place_turn_signature 按锚点规则处理（ inbound 已
                        // 把占位块的签名转移给首个非思考 part）。
                        let is_placeholder =
                            crate::proxy::thinking_store::is_placeholder_thought(thinking);
                        if is_placeholder || thinking.trim().is_empty() {
                            tracing::debug!(
                                "[Claude-Request] Placeholder thinking block dropped (text={:?}).",
                                &thinking[..thinking.len().min(20)],
                            );
                            continue;
                        }
                        let final_thought_text = thinking.as_str();

                        // [HOTFIX] Gemini Protocol Enforcement: Thinking block MUST be the first block.
                        // If we already have content (like Text), we must downgrade this thinking block to Text.
                        if saw_non_thinking || !parts.is_empty() {
                            tracing::warn!("[Claude-Request] Thinking block found at non-zero index (prev parts: {}). Downgrading to Text.", parts.len());
                            if !final_thought_text.is_empty() {
                                parts.push(json!({
                                    "text": final_thought_text
                                }));
                                saw_non_thinking = true;
                            }
                            continue;
                        }

                        // [FIX] If thinking is disabled (smart downgrade), convert ALL thinking blocks to text
                        // to avoid "thinking is disabled but message contains thinking" error
                        if !is_thinking_enabled {
                            tracing::warn!("[Claude-Request] Thinking disabled. Downgrading thinking block to text.");
                            if !final_thought_text.is_empty() {
                                parts.push(json!({
                                    "text": final_thought_text
                                }));
                                saw_non_thinking = true;
                            }
                            continue;
                        }

                        let is_claude_model = mapped_model.to_lowercase().contains("claude");

                        let mut effective_sig = None;

                        // 1. Check incoming signature if long enough or sentinel
                        if let Some(sig) = signature {
                            if sig == SENTINEL_SIGNATURE || sig.len() >= MIN_SIGNATURE_LENGTH {
                                let cached_family = crate::proxy::SignatureCache::global()
                                    .get_signature_family(sig);

                                match cached_family {
                                    Some(family) => {
                                        let compatible =
                                            !is_retry && is_model_compatible(&family, mapped_model);
                                        if compatible
                                            || (!is_retry
                                                && is_claude_model
                                                && family.to_lowercase().contains("claude"))
                                        {
                                            effective_sig = Some(sig.clone());
                                        } else {
                                            tracing::warn!(
                                                "[Thinking-Signature] {} signature (Family: {}, Target: {}).",
                                                if is_retry { "Stripping historical" } else { "Incompatible" },
                                                family, mapped_model
                                            );
                                        }
                                    }
                                    None => {
                                        if !is_retry {
                                            if mapped_model.to_lowercase().contains("gemini") {
                                                if crate::proxy::thinking_store::is_likely_gemini_signature(sig) {
                                                    effective_sig = Some(sig.clone());
                                                } else {
                                                    tracing::warn!(
                                                        "[Thinking-Signature] Dropping unknown/foreign signature for Gemini target (len: {}), fallback to sentinel",
                                                        sig.len()
                                                    );
                                                }
                                            } else {
                                                effective_sig = Some(sig.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // 2. Try turn_signature (strictly local to this assistant turn)
                        if effective_sig.is_none() {
                            effective_sig = turn_signature.clone();
                        }

                        // 3. 无合法签名时**绝不发明哨兵**。
                        //    依据（3 份官方报文 / 23 处签名）：哨兵在官方流量里出现 0/23 次，
                        //    它不属于 Antigravity 协议；且「跳过校验」不等于恢复推理连续性。
                        //    签名缺失是被上游容忍的（官方"在飞轮"即缺席），
                        //    归位统一交给流水线终审 `place_turn_signature`。
                        if let Some(sig) = effective_sig {
                            *last_thought_signature = Some(sig.clone());
                            let part = json!({
                                "text": final_thought_text,
                                "thought": true,
                                "thoughtSignature": sig
                            });
                            parts.push(part);
                        } else {
                            // 无签名：保留思考块结构（`thought: true`），但**不携带签名字段**。
                            // Gemini 目标的思考块本就不应带签名（铁律 I4：
                            // 官方报文里 `thought:true` 的 part 没有 thoughtSignature）。
                            tracing::debug!(
                                "[Thinking-Signature] No signature for thought block (model: {}). Keeping thought block without signature.",
                                mapped_model
                            );
                            parts.push(json!({
                                "text": final_thought_text,
                                "thought": true,
                            }));
                        }
                    }
                    ContentBlock::RedactedThinking { data } => {
                        // [FIX] 将 RedactedThinking 作为普通文本处理，保留上下文
                        tracing::debug!("[Claude-Request] Degrade RedactedThinking to text");
                        parts.push(json!({
                            "text": format!("[Redacted Thinking: {}]", data)
                        }));
                        saw_non_thinking = true;
                        continue;
                    }
                    ContentBlock::Image { source, .. } => {
                        if source.source_type == "base64" {
                            let part =
                                crate::proxy::mappers::common_utils::create_gemini_inline_part(
                                    source.media_type.as_deref(),
                                    source.data.as_deref().unwrap_or_default(),
                                    "Image",
                                );
                            parts.push(part);
                            saw_non_thinking = true;
                        }
                    }
                    ContentBlock::Document { source, .. } => {
                        if source.source_type == "base64" {
                            let part =
                                crate::proxy::mappers::common_utils::create_gemini_inline_part(
                                    source.media_type.as_deref(),
                                    source.data.as_deref().unwrap_or_default(),
                                    "Document",
                                );
                            parts.push(part);
                            saw_non_thinking = true;
                        }
                    }
                    ContentBlock::ToolUse {
                        id,
                        name,
                        input,
                        signature,
                        ..
                    } => {
                        let mut final_input = input.clone();

                        // [New] 利用通用引擎修正参数类型 (替代以前硬编码的 shell 工具修复逻辑)
                        if let Some(original_schema) = tool_name_to_schema.get(name) {
                            crate::proxy::common::json_schema::fix_tool_call_args(
                                &mut final_input,
                                original_schema,
                            );
                        }

                        let mut part = json!({
                            "functionCall": {
                                "name": name,
                                "args": final_input,
                                "id": id
                            }
                        });
                        saw_non_thinking = true;

                        // Track pending tool use
                        if is_assistant {
                            pending_tool_use_ids.push(id.clone());
                        }

                        // 记录 id -> name 映射与签名上下文
                        tool_id_to_name.insert(id.clone(), name.clone());
                        let final_sig = signature
                            .as_ref()
                            .filter(|s| {
                                (s.as_str() == SENTINEL_SIGNATURE
                                    || s.len() >= MIN_SIGNATURE_LENGTH)
                                    && (!mapped_model.to_lowercase().contains("gemini")
                                        || crate::proxy::thinking_store::is_likely_gemini_signature(
                                            s,
                                        ))
                            })
                            .cloned();

                        if let Some(ref s) = final_sig {
                            *last_thought_signature = Some(s.clone());
                        }

                        let is_claude_model = mapped_model.to_lowercase().contains("claude");
                        if is_claude_model {
                            // Claude 模型：Anthropic 官方验签引擎要求签名必须在思考块上，工具调用绝不携带签名，更不塞假哨兵
                            if let Some(obj) = part.as_object_mut() {
                                obj.remove("thoughtSignature");
                                obj.remove("thought_signature");
                            }
                        } else {
                            // 纯净线缆透传：客户端若自带签名则保持，未带则留空，全权委托进站流水线统一对齐与回填
                            if let Some(ref sig) = final_sig {
                                part["thoughtSignature"] = json!(sig);
                            }
                        }
                        parts.push(part);
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                        ..
                    } => {
                        // Mark this tool ID as resolved in this turn
                        current_turn_tool_result_ids.insert(tool_use_id.clone());
                        // 优先使用之前记录的 name，否则用 tool_use_id
                        let func_name = tool_id_to_name
                            .get(tool_use_id)
                            .cloned()
                            .unwrap_or_else(|| tool_use_id.clone());

                        // Tool results should pass transparency. If images are present, map them to inlineData.
                        let mut extra_parts = Vec::new();

                        let mut merged_content = match content {
                            serde_json::Value::String(s) => {
                                crate::proxy::mappers::common_utils::extract_multimodal_from_tool_text(
                                    s,
                                    &mut extra_parts,
                                )
                            }
                            serde_json::Value::Array(arr) => {
                                let mut texts = Vec::new();
                                for block in arr {
                                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                                        texts.push(text.to_string());
                                    } else if block.get("source").is_some() {
                                        if block.get("type").and_then(|v| v.as_str())
                                            == Some("image")
                                        {
                                            let source = block.get("source").unwrap();
                                            let media_type =
                                                source.get("media_type").and_then(|v| v.as_str());
                                            let data = source
                                                .get("data")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or_default();
                                            extra_parts.push(crate::proxy::mappers::common_utils::create_gemini_inline_part(
                                                media_type,
                                                data,
                                                "Tool Result Image",
                                            ));
                                        }
                                    }
                                }
                                texts.join("\n")
                            }
                            serde_json::Value::Null => String::new(),
                            _ => content.to_string(),
                        };

                        // [优化] 如果结果为空，注入显式确认信号，防止模型幻觉
                        if merged_content.trim().is_empty() {
                            if is_error.unwrap_or(false) {
                                merged_content =
                                    "Tool execution failed with no output.".to_string();
                            } else {
                                merged_content = "Command executed successfully.".to_string();
                            }
                        }

                        let part = json!({
                            "functionResponse": {
                                "name": func_name,
                                "response": {"output": merged_content},
                                "id": tool_use_id
                            }
                        });

                        // 危险测试分支法则：ToolResult (functionResponse) 绝不携带签名
                        parts.push(part);

                        // 暂存随行媒体 parts，确保同一轮中所有 functionResponse 置顶连续，多模态媒体统一延后追加
                        turn_tool_media_parts.extend(extra_parts);

                        // 标记状态，用于下一条 User 消息的去重判断
                        *previous_was_tool_result = true;
                    }
                    // ContentBlock::RedactedThinking handled above at line 583
                    ContentBlock::ServerToolUse { .. }
                    | ContentBlock::WebSearchToolResult { .. } => {
                        // 搜索结果 block 不应由客户端发回给上游 (已由 tool_result 替代)
                        continue;
                    }
                }
            }
            parts.extend(turn_tool_media_parts);
        }
    }

    // If this is a User message, check if we need to inject missing tool results
    if !is_assistant && !pending_tool_use_ids.is_empty() {
        let missing_ids: Vec<_> = pending_tool_use_ids
            .iter()
            .filter(|id| !current_turn_tool_result_ids.contains(*id))
            .cloned()
            .collect();

        if !missing_ids.is_empty() {
            tracing::warn!("[Elastic-Recovery] Injecting {} missing tool results into User message (IDs: {:?})", missing_ids.len(), missing_ids);
            for id in missing_ids.iter().rev() {
                // Insert in reverse order to maintain order at index 0? No, just insert at 0.
                let name = tool_id_to_name.get(id).cloned().unwrap_or(id.clone());
                let synthetic_part = json!({
                    "functionResponse": {
                        "name": name,
                        "response": {
                            "output": "Tool execution interrupted. No result provided."
                        },
                        "id": id
                    }
                });
                // Prepend to ensure they are present before any text
                parts.insert(0, synthetic_part);
            }
        }
        // All pending IDs are now handled (either present or injected)
        pending_tool_use_ids.clear();
    }

    // Fix for "Thinking enabled, assistant message must start with thinking block" 400 error
    // [Optimization] Apply this to ALL assistant messages in history, not just the last one.
    // Vertex AI requires every assistant message to start with a thinking block when thinking is enabled.
    if is_assistant && is_thinking_enabled {
        let _is_google_cloud = mapped_model.starts_with("projects/");
        let thought_idx = parts
            .iter()
            .position(|p| p.get("thought").and_then(|v| v.as_bool()) == Some(true));

        match thought_idx {
            Some(0) => {
                // Already at index 0! Nothing to do.
            }
            Some(idx) => {
                // Existing thought block is not at index 0, move it to index 0 (DO NOT insert a duplicate!)
                let thought_part = parts.remove(idx);
                parts.insert(0, thought_part);
            }
            None => {
                // 纯净线缆原则：客户端若原本无思考块，适配器严禁凭空伪造 "..." 占位块！
                // 缺失思考块的判定与状态机复活统一委托给进站流水线（InboundThinkingPipeline）
            }
        }
    }

    Ok(parts)
}

/// 构建 Contents (Messages)
fn build_google_content(
    msg: &Message,
    claude_req: &ClaudeRequest,
    is_thinking_enabled: bool,
    session_id: &str,
    msg_index: usize,
    allow_dummy_thought: bool,
    is_retry: bool,
    tool_id_to_name: &mut HashMap<String, String>,
    tool_name_to_schema: &HashMap<String, Value>,
    mapped_model: &str,
    last_thought_signature: &mut Option<String>,
    pending_tool_use_ids: &mut Vec<String>,
    last_user_task_text_normalized: &mut Option<String>,
    previous_was_tool_result: &mut bool,
    existing_tool_result_ids: &std::collections::HashSet<String>,
) -> Result<Value, String> {
    let role = if msg.role == "assistant" {
        "model"
    } else {
        &msg.role
    };

    // Proactive Tool Chain Repair:
    // If we are about to process an Assistant message, but we still have pending tool_use_ids,
    // it means the previous turn was interrupted or the user ignored the tool.
    // We MUST inject a synthetic User message with error results to close the loop.
    if role == "model" && !pending_tool_use_ids.is_empty() {
        tracing::warn!("[Elastic-Recovery] Detected interrupted tool chain (Assistant -> Assistant). Injecting synthetic User message for IDs: {:?}", pending_tool_use_ids);

        let synthetic_parts: Vec<serde_json::Value> = pending_tool_use_ids
            .iter()
            .filter(|id| !existing_tool_result_ids.contains(*id)) // [FIX #632] Only inject if ID is truly missing
            .map(|id| {
                let name = tool_id_to_name.get(id).cloned().unwrap_or(id.clone());
                json!({
                    "functionResponse": {
                        "name": name,
                        "response": {
                            "output": "Tool execution interrupted. No result provided."
                        },
                        "id": id
                    }
                })
            })
            .collect();

        if !synthetic_parts.is_empty() {
            return Ok(json!({
                "role": "user",
                "parts": synthetic_parts
            }));
        }
        // Clear pending IDs as we have handled them
        pending_tool_use_ids.clear();
    }

    let mut parts = build_contents(
        &msg.content,
        msg.role == "assistant",
        claude_req,
        is_thinking_enabled,
        session_id,
        msg_index,
        allow_dummy_thought,
        is_retry,
        tool_id_to_name,
        tool_name_to_schema,
        mapped_model,
        last_thought_signature,
        pending_tool_use_ids,
        last_user_task_text_normalized,
        previous_was_tool_result,
        existing_tool_result_ids,
    )?;

    if parts.is_empty() {
        if role == "user" {
            parts.push(json!({ "text": crate::proxy::mappers::common_utils::TRANSIT_DEFENSE_FALLBACK_TEXT }));
        } else {
            return Ok(json!(null)); // Indicate no content to add
        }
    }

    if role == "model" {
        reorder_gemini_parts(&mut parts);
    }

    Ok(json!({
        "role": role,
        "parts": parts
    }))
}

/// 构建 Contents (Messages)
fn build_google_contents(
    messages: &[Message],
    claude_req: &ClaudeRequest,
    tool_id_to_name: &mut HashMap<String, String>,
    tool_name_to_schema: &HashMap<String, Value>,
    is_thinking_enabled: bool,
    allow_dummy_thought: bool,
    mapped_model: &str,
    session_id: &str, // [NEW v3.3.17] Session ID for signature caching
    is_retry: bool,
    timing: &mut TransformTiming,
) -> Result<Value, String> {
    let mut contents = Vec::new();
    let mut last_thought_signature: Option<String> = None;
    let mut _accumulated_usage: Option<Value> = None;
    // Track pending tool_use IDs for recovery
    let mut pending_tool_use_ids: Vec<String> = Vec::new();

    // [NEW] 用于识别并过滤 Claude Code 重复回显的任务指令
    let mut last_user_task_text_normalized: Option<String> = None;
    let mut previous_was_tool_result = false;

    let _msg_count = messages.len();

    // [FIX #632] Pre-scan all messages to identify all tool_result IDs that ALREADY exist in the conversation.
    // This prevents Elastic-Recovery from injecting duplicate results if they are present later in the chain.
    let mut existing_tool_result_ids = std::collections::HashSet::new();
    for msg in messages {
        if let MessageContent::Array(blocks) = &msg.content {
            for block in blocks {
                if let ContentBlock::ToolResult { tool_use_id, .. } = block {
                    existing_tool_result_ids.insert(tool_use_id.clone());
                }
            }
        }
    }

    for (i, msg) in messages.iter().enumerate() {
        if msg.role == "assistant" {
            // CRITICAL: Reset last_thought_signature at the start of each assistant message
            // so signatures never bleed across turns!
            last_thought_signature = None;
        }

        let google_content = build_google_content(
            msg,
            claude_req,
            is_thinking_enabled,
            session_id,
            i,
            allow_dummy_thought,
            is_retry,
            tool_id_to_name,
            tool_name_to_schema,
            mapped_model,
            &mut last_thought_signature,
            &mut pending_tool_use_ids,
            &mut last_user_task_text_normalized,
            &mut previous_was_tool_result,
            &existing_tool_result_ids,
        )?;

        if !google_content.is_null() {
            contents.push(google_content);
        }
    }

    // [Removed] ensure_last_assistant_has_thinking
    // Corrupted signature issues proved we cannot fake thinking blocks.
    // Instead we rely on should_disable_thinking_due_to_history to prevent this state.

    // 思考回填：仅在开启思考且非 Gemini < 3 模型时恢复思维块与签名
    let should_finalize_thinking =
        is_thinking_enabled && !crate::proxy::model_specs::is_gemini_under_v3(mapped_model);

    let think_start = std::time::Instant::now();
    crate::proxy::pipeline::InboundThinkingPipeline::process_contents(
        &mut contents,
        mapped_model,
        should_finalize_thinking,
        Some(session_id),
        false,
    );
    timing.think_fill_micros = think_start.elapsed().as_micros() as u64;

    Ok(json!(contents))
}

/// Merge adjacent messages with the same role
fn merge_adjacent_roles(mut contents: Vec<Value>) -> Vec<Value> {
    if contents.is_empty() {
        return contents;
    }

    let mut merged = Vec::new();
    let mut current_msg = contents.remove(0);

    for msg in contents {
        let current_role = current_msg["role"].as_str().unwrap_or_default();
        let next_role = msg["role"].as_str().unwrap_or_default();

        if current_role == next_role {
            // Merge parts
            if let Some(current_parts) = current_msg.get_mut("parts").and_then(|p| p.as_array_mut())
            {
                if let Some(next_parts) = msg.get("parts").and_then(|p| p.as_array()) {
                    current_parts.extend(next_parts.clone());

                    // [FIX #709] Core Fix: After merging parts from adjacent messages,
                    // we must RE-SORT them to ensure any thinking blocks from the
                    // second message are moved to the very front of the combined array.
                    reorder_gemini_parts(current_parts);
                }
            }
        } else {
            merged.push(current_msg);
            current_msg = msg;
        }
    }
    merged.push(current_msg);
    merged
}

/// 构建 Tools
fn build_tools(
    tools: &Option<Vec<Tool>>,
    has_web_search: bool,
    mapped_model: &str,
) -> Result<Option<Value>, String> {
    if let Some(tools_list) = tools {
        let mut function_declarations: Vec<Value> = Vec::new();
        let has_google_search = has_web_search;

        let is_search_tool = |tool: &Tool| {
            tool.is_web_search()
                || tool.name.as_deref() == Some("google_search")
                || tool.name.as_deref() == Some("builtin_web_search")
                || tool.type_.as_deref() == Some("builtin_web_search")
        };
        // 只有搜索工具时，改映射为上游 googleSearch，不进客户端函数列表。
        // 旁边还有 Bash/Read 等函数工具时，web_search 与其他协议一样留在 functionDeclarations。
        let search_only =
            !tools_list.is_empty() && tools_list.iter().all(|tool| is_search_tool(tool));

        for tool in tools_list {
            if search_only && is_search_tool(tool) {
                continue;
            }
            let name = tool
                .name
                .as_deref()
                .or(tool.type_.as_deref())
                .unwrap_or("tool");

            let mut input_schema = tool.input_schema.clone().unwrap_or(json!({
                "type": "object",
                "properties": {}
            }));
            crate::proxy::common::json_schema::clean_json_schema(&mut input_schema);
            crate::proxy::mappers::openai::request::enforce_uppercase_types(&mut input_schema);

            function_declarations.push(json!({
                "name": name,
                "description": tool.description,
                "parameters": input_schema
            }));
        }

        let mut tool_list = Vec::new();

        // [优化] Gemini 2.0+ 及 3.0 系列模型通常支持混合工具调用 (Function Calling + Google Search)
        // 但由于反代使用的 Google v1internal 接口为受限环境，不支持 include_server_side_tool_invocations，混合调用会报 400 错误。
        // 因此在 v1internal 架构下，我们强制不开启混合工具调用以避免 400 报错。
        let supports_mixed_tools = false;

        if !function_declarations.is_empty() {
            // 保持客户端工具声明原序，不按 name 重排。
            let mut func_obj = serde_json::Map::new();
            func_obj.insert(
                "functionDeclarations".to_string(),
                json!(function_declarations),
            );
            tool_list.push(json!(func_obj));

            if has_google_search {
                if supports_mixed_tools {
                    tracing::info!(
                        "[Claude-Request] Enabling MIXED tool calling for {}: Function Calling + Google Search.",
                        mapped_model
                    );
                    let mut search_obj = serde_json::Map::new();
                    search_obj.insert("googleSearch".to_string(), json!({}));
                    tool_list.push(json!(search_obj));
                } else {
                    tracing::info!(
                        "[Claude-Request] Skipping googleSearch injection for {} due to existing function declarations. \
                         Older Gemini models may not support mixed tool types.",
                        mapped_model
                    );
                }
            }
        } else if has_google_search {
            let mut search_obj = serde_json::Map::new();
            search_obj.insert("googleSearch".to_string(), json!({}));
            tool_list.push(json!(search_obj));
        }

        if !tool_list.is_empty() {
            return Ok(Some(json!(tool_list)));
        }
    }

    Ok(None)
}

/// 构建 Generation Config
fn build_generation_config(
    claude_req: &ClaudeRequest,
    mapped_model: &str,
    _has_web_search: bool,
    is_thinking_enabled: bool,
    token: Option<&crate::proxy::token_manager::ProxyToken>, // [NEW]
) -> Value {
    let mut config = json!({});

    // Thinking 配置
    let client_switch = crate::proxy::pipeline::extract_client_thinking_switch(
        claude_req.thinking.as_ref().map(|t| t.type_.as_str()),
        claude_req
            .thinking
            .as_ref()
            .and_then(|t| t.budget_tokens.map(|b| b as u64)),
        claude_req
            .output_config
            .as_ref()
            .and_then(|c| c.effort.as_deref())
            .or_else(|| {
                claude_req
                    .thinking
                    .as_ref()
                    .and_then(|t| t.effort.as_deref())
            }),
    );

    let tb_config = crate::proxy::config::get_thinking_budget_config();
    let is_client_control =
        tb_config.control_source == crate::proxy::config::ThinkingControlSource::Client;
    let is_client_disabled = is_client_control && client_switch.is_disabled();

    let effort = claude_req
        .output_config
        .as_ref()
        .and_then(|c| c.effort.as_ref())
        .or_else(|| claude_req.thinking.as_ref().and_then(|t| t.effort.as_ref()));

    let client_effort = effort.map(|s| s.as_str());
    let client_budget = claude_req
        .thinking
        .as_ref()
        .and_then(|t| t.budget_tokens.map(|b| b as u64));

    if is_client_disabled {
        crate::proxy::pipeline::InboundThinkingPipeline::configure_inbound_thinking(
            mapped_model,
            &mut config,
            client_switch,
            None,
            None,
            token,
        );
    } else if is_thinking_enabled && !crate::proxy::model_specs::is_gemini_under_v3(mapped_model) {
        crate::proxy::pipeline::InboundThinkingPipeline::configure_inbound_thinking(
            mapped_model,
            &mut config,
            client_switch,
            client_effort,
            client_budget,
            token,
        );
    }

    if let Some(temp) = claude_req.temperature {
        config["temperature"] = json!(temp);
    }
    if let Some(top_p) = claude_req.top_p {
        config["topP"] = json!(top_p);
    }
    if let Some(top_k) = claude_req.top_k {
        config["topK"] = json!(top_k);
    }

    // max_tokens 映射为 maxOutputTokens。客户端没传时留给流水线按官方模型结构体补齐。
    let mut final_max_tokens: Option<i64> = claude_req.max_tokens.map(|t| t as i64);

    if let Some(thinking_config) = config.get("thinkingConfig") {
        if let Some(budget) = thinking_config
            .get("thinkingBudget")
            .and_then(|t| t.as_u64())
        {
            let current = final_max_tokens.unwrap_or(0);
            if current <= budget as i64 {
                let overhead = if mapped_model.contains("-image") {
                    2048
                } else {
                    8192
                };
                let boosted = (budget as i64 + overhead).min(65536);
                final_max_tokens = Some(boosted);
                tracing::info!(
                    "[Generation-Config] Bumping maxOutputTokens to {} due to thinking budget of {}",
                    boosted, budget
                );
            }
        }
    }

    if let Some(val) = final_max_tokens {
        // [FIX] Cap maxOutputTokens to safe upper limit (128000 for Claude 5.5, 65535 for Pro, 65536 for Flash, 64000 for Claude 4.6)
        let mapped_lower = mapped_model.to_lowercase();
        let safe_limit = if mapped_lower.contains("5-5") || mapped_lower.contains("5.5") {
            128000
        } else if mapped_lower.contains("claude") {
            64000
        } else if mapped_lower.contains("pro") {
            65535
        } else {
            65536
        };
        if val > safe_limit {
            tracing::warn!(
                "[Generation-Config] Capping maxOutputTokens from {} to {} to prevent 400 Invalid Argument",
                val, safe_limit
            );
            config["maxOutputTokens"] = json!(safe_limit);
        } else {
            config["maxOutputTokens"] = json!(val);
        }
    }

    config
}

/// Recursively remove 'thought' and 'thoughtSignature' fields
/// Used when downgrading thinking (e.g. during 400 retry)
pub fn clean_thinking_fields_recursive(val: &mut Value) {
    match val {
        Value::Object(map) => {
            map.remove("thought");
            map.remove("thoughtSignature");
            map.remove("thought_signature");
            for (_, v) in map.iter_mut() {
                clean_thinking_fields_recursive(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                clean_thinking_fields_recursive(v);
            }
        }
        _ => {}
    }
}

use crate::proxy::mappers::common_utils::is_model_compatible;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::common::json_schema::clean_json_schema;
    use crate::proxy::config::ThinkingBudgetConfig;

    #[test]
    fn test_agent_sdk_identity_is_normalized_for_antigravity() {
        let req: ClaudeRequest = serde_json::from_value(json!({
            "model": "claude-sonnet-4-6",
            "messages": [{"role": "user", "content": "Reply with ok"}],
            "system": [
                {
                    "type": "text",
                    "text": "x-anthropic-billing-header: cc_entrypoint=sdk-cli;"
                },
                {
                    "type": "text",
                    "text": CLAUDE_AGENT_SDK_IDENTITY,
                    "cache_control": {"type": "ephemeral"}
                }
            ]
        }))
        .expect("Agent SDK request should deserialize");

        let body =
            transform_claude_request_in(&req, "test-project", false, None, "test-session", None)
                .expect("Agent SDK request should transform");
        let system_parts = body["request"]["systemInstruction"]["parts"]
            .as_array()
            .expect("system instruction should contain parts");
        let system_texts = system_parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>();

        assert!(system_texts.contains(&CLAUDE_CODE_CLI_IDENTITY));
        assert!(!system_texts.contains(&CLAUDE_AGENT_SDK_IDENTITY));
        assert!(!system_texts.contains(&"x-anthropic-billing-header: cc_entrypoint=sdk-cli;"));
    }

    #[test]
    fn claude_transform_does_not_inject_legacy_stop_sequences() {
        let req: ClaudeRequest = serde_json::from_value(json!({
            "model": "claude-sonnet-4-6",
            "messages": [{"role": "user", "content": "Reply with ok"}]
        }))
        .expect("request should deserialize");

        let body =
            transform_claude_request_in(&req, "test-project", false, None, "test-session", None)
                .expect("request should transform");
        let gen_config = &body["request"]["generationConfig"];
        assert!(
            gen_config.get("stopSequences").is_none(),
            "legacy stopSequences must not be injected: {gen_config}"
        );
    }

    #[test]
    fn test_agent_sdk_identity_mention_is_not_rewritten() {
        let quoted_identity = format!("Compatibility note: {CLAUDE_AGENT_SDK_IDENTITY}");

        assert_eq!(
            normalize_claude_client_identity(&quoted_identity),
            quoted_identity
        );
    }

    #[test]
    fn test_ephemeral_injection_debug() {
        // This test simulates the issue where cache_control might be injected
        let json_with_null = json!({
            "model": "claude-3-5-sonnet-20241022",
            "messages": [
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "thinking",
                            "thinking": "test",
                            "signature": "sig_1234567890",
                            "cache_control": null
                        }
                    ]
                }
            ]
        });

        let req: ClaudeRequest = serde_json::from_value(json_with_null).unwrap();
        if let MessageContent::Array(blocks) = &req.messages[0].content {
            if let ContentBlock::Thinking { cache_control, .. } = &blocks[0] {
                assert!(
                    cache_control.is_none(),
                    "Deserialization should result in None for null cache_control"
                );
            }
        }

        // Now test serialization
        let serialized = serde_json::to_value(&req).unwrap();
        println!("Serialized: {}", serialized);
        assert!(serialized["messages"][0]["content"][0]
            .get("cache_control")
            .is_none());
    }

    #[test]
    fn test_simple_request() {
        let req = ClaudeRequest {
            model: "claude-sonnet-4-6".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Hello".to_string()),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        let result =
            transform_claude_request_in(&req, "test-project", false, None, "test_session", None);
        assert!(result.is_ok());

        let body = result.unwrap();
        assert_eq!(body["project"], "test-project");
        assert!(body["requestId"].as_str().unwrap().starts_with("agent/"));
    }

    #[test]
    fn test_clean_json_schema() {
        let mut schema = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "location": {
                    "type": "string",
                    "description": "The city and state, e.g. San Francisco, CA",
                    "minLength": 1,
                    "exclusiveMinimum": 0
                },
                "unit": {
                    "type": ["string", "null"],
                    "enum": ["celsius", "fahrenheit"],
                    "default": "celsius"
                },
                "date": {
                    "type": "string",
                    "format": "date"
                }
            },
            "required": ["location"]
        });

        clean_json_schema(&mut schema);

        // Check removed fields
        assert!(schema.get("$schema").is_none());
        assert!(schema.get("additionalProperties").is_none());
        assert!(schema["properties"]["location"].get("minLength").is_none());
        assert!(schema["properties"]["unit"].get("default").is_none());
        assert!(schema["properties"]["date"].get("format").is_none());

        // Check union type handling ["string", "null"] -> "string"
        assert_eq!(schema["properties"]["unit"]["type"], "string");

        // Check types are lowercased
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["location"]["type"], "string");
        assert_eq!(schema["properties"]["date"]["type"], "string");
    }

    #[test]
    fn test_complex_tool_result() {
        let req = ClaudeRequest {
            model: "claude-3-5-sonnet-20241022".to_string(),
            messages: vec![
                Message {
                    role: "user".to_string(),
                    content: MessageContent::String("Run command".to_string()),
                },
                Message {
                    role: "assistant".to_string(),
                    content: MessageContent::Array(vec![ContentBlock::ToolUse {
                        id: "call_1".to_string(),
                        name: "run_command".to_string(),
                        input: json!({"command": "ls"}),
                        signature: None,
                        cache_control: None,
                    }]),
                },
                Message {
                    role: "user".to_string(),
                    content: MessageContent::Array(vec![ContentBlock::ToolResult {
                        tool_use_id: "call_1".to_string(),
                        content: json!([
                            {"type": "text", "text": "file1.txt\n"},
                            {"type": "text", "text": "file2.txt"}
                        ]),
                        is_error: Some(false),
                    }]),
                },
            ],
            system: None,
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        let result =
            transform_claude_request_in(&req, "test-project", false, None, "test_session", None);
        assert!(result.is_ok());

        let body = result.unwrap();
        let contents = body["request"]["contents"].as_array().unwrap();

        // Check the tool result message (last message)
        let tool_resp_msg = &contents[2];
        let parts = tool_resp_msg["parts"].as_array().unwrap();
        let func_resp = &parts[0]["functionResponse"];

        assert_eq!(func_resp["name"], "run_command");
        assert_eq!(func_resp["id"], "call_1");

        // Verify merged content
        let resp_text = func_resp["response"]["output"].as_str().unwrap();
        assert!(resp_text.contains("file1.txt"));
        assert!(resp_text.contains("file2.txt"));
        assert!(resp_text.contains("\n"));
    }

    #[test]
    fn test_cache_control_cleanup() {
        // 模拟 VS Code 插件发送的包含 cache_control 的历史消息
        let req = ClaudeRequest {
            model: "claude-sonnet-4-6".to_string(),
            messages: vec![
                Message {
                    role: "user".to_string(),
                    content: MessageContent::String("Hello".to_string()),
                },
                Message {
                    role: "assistant".to_string(),
                    content: MessageContent::Array(vec![
                        ContentBlock::Thinking {
                            thinking: "Let me think...".to_string(),
                            signature: Some("sig123".to_string()),
                            cache_control: Some(json!({"type": "ephemeral"})), // 这个应该被清理
                        },
                        ContentBlock::Text {
                            text: "Here is my response".to_string(),
                        },
                    ]),
                },
                Message {
                    role: "user".to_string(),
                    content: MessageContent::Array(vec![ContentBlock::Image {
                        source: ImageSource {
                            source_type: "base64".to_string(),
                            media_type: Some("image/png".to_string()),
                            data: Some("iVBORw0KGgo=".to_string()),
                        },
                        cache_control: Some(json!({"type": "ephemeral"})), // 这个也应该被清理
                    }]),
                },
            ],
            system: None,
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        let result =
            transform_claude_request_in(&req, "test-project", false, None, "test_session", None);
        assert!(result.is_ok());

        // 验证请求成功转换
        let body = result.unwrap();
        assert_eq!(body["project"], "test-project");

        // 注意: cache_control 的清理发生在内部,我们无法直接从 JSON 输出验证
        // 但如果没有清理,后续发送到 Anthropic API 时会报错
        // 这个测试主要确保清理逻辑不会导致转换失败
    }

    #[test]
    fn test_thinking_mode_auto_disable_on_tool_use_history() {
        // [场景] 历史消息中有一个工具调用链，且 Assistant 消息没有 Thinking 块
        // 期望: 系统不再盲目降级禁用思考，而是保留 thinkingConfig 并补齐保底思考块
        let req = ClaudeRequest {
            model: "claude-sonnet-4-6".to_string(),
            messages: vec![
                Message {
                    role: "user".to_string(),
                    content: MessageContent::String("Check files".to_string()),
                },
                // Assistant 使用工具，但在非 Thinking 模式下
                Message {
                    role: "assistant".to_string(),
                    content: MessageContent::Array(vec![
                        ContentBlock::Text {
                            text: "Checking...".to_string(),
                        },
                        ContentBlock::ToolUse {
                            id: "tool_1".to_string(),
                            name: "list_files".to_string(),
                            input: json!({}),
                            cache_control: None,
                            signature: None,
                        },
                    ]),
                },
                // 用户返回工具结果
                Message {
                    role: "user".to_string(),
                    content: MessageContent::Array(vec![ContentBlock::ToolResult {
                        tool_use_id: "tool_1".to_string(),
                        content: serde_json::Value::String("file1.txt\nfile2.txt".to_string()),
                        is_error: Some(false),
                    }]),
                },
            ],
            system: None,
            tools: Some(vec![Tool {
                name: Some("list_files".to_string()),
                description: Some("List files".to_string()),
                input_schema: Some(json!({"type": "object"})),
                type_: None,
            }]),
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(1024),
                effort: None,
            }),
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        let result =
            transform_claude_request_in(&req, "test-project", false, None, "test_session", None);
        assert!(result.is_ok());

        let body = result.unwrap();
        let request = &body["request"];

        // 验证: generationConfig 中必须保留 thinkingConfig (不再因历史消息无签名或无思考块而被降级移除)
        let gen_config = request
            .get("generationConfig")
            .expect("Should have generationConfig");
        assert!(
            gen_config.get("thinkingConfig").is_some(),
            "thinkingConfig must be preserved per server-side thinking persistence policy"
        );

        // 验证: 遵循纯净线缆原则，不凭空伪造虚假思考块，同时保持 thinkingConfig 开启
        let contents = request["contents"].as_array().expect("Contents array");
        let assistant_msg = &contents[1];
        let parts = assistant_msg["parts"].as_array().expect("Parts array");
        assert!(
            parts.iter().all(|p| p.get("thought") != Some(&json!(true))),
            "Assistant message without thinking should not have fake thinking injected"
        );
    }

    #[test]
    fn test_thinking_block_not_prepend_when_disabled() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // 验证当 thinking 未启用且模型非思考模型时,不会补全 thinking 块
        let req = ClaudeRequest {
            model: "non-reasoning-model".to_string(),
            messages: vec![
                Message {
                    role: "user".to_string(),
                    content: MessageContent::String("Hello".to_string()),
                },
                Message {
                    role: "assistant".to_string(),
                    content: MessageContent::Array(vec![ContentBlock::Text {
                        text: "Response".to_string(),
                    }]),
                },
            ],
            system: None,
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None, // 未启用 thinking
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(
            &req,
            "test-project",
            false,
            None,
            "test_session_non_thinking",
            None,
        );
        assert!(result.is_ok());

        let body = result.unwrap();
        let contents = body["request"]["contents"].as_array().unwrap();

        let last_model_msg = contents
            .iter()
            .rev()
            .find(|c| c["role"] == "model")
            .unwrap();

        let parts = last_model_msg["parts"].as_array().unwrap();

        // 验证没有补全 thinking 块
        assert_eq!(parts.len(), 1, "Should only have the original text block");
        assert_eq!(parts[0]["text"], "Response");
    }

    #[test]
    fn test_thinking_block_empty_content_fix() {
        // [场景] 客户端发送了一个内容为空的 thinking 块
        // 期望: 自动填充 "..."
        let req = ClaudeRequest {
            model: "claude-sonnet-4-6".to_string(),
            messages: vec![Message {
                role: "assistant".to_string(),
                content: MessageContent::Array(vec![
                    ContentBlock::Thinking {
                        thinking: "".to_string(), // 空内容
                        signature: Some("sig".to_string()),
                        cache_control: None,
                    },
                    ContentBlock::Text {
                        text: "Hi".to_string(),
                    },
                ]),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(1024),
                effort: None,
            }),
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(
            &req,
            "test-project",
            false,
            None,
            "test_session_empty_content_fix_unique",
            None,
        );
        assert!(result.is_ok(), "Transformation failed");
        let body = result.unwrap();
        let contents = body["request"]["contents"].as_array().unwrap();
        let parts = contents[0]["parts"].as_array().unwrap();

        // 验证空/占位 thinking 块按流水线规范被安全丢弃，不污染后续正文
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], "Hi");
        assert!(parts[0].get("thought").is_none());
    }

    #[test]
    fn test_redacted_thinking_degradation() {
        // [场景] 客户端包含 RedactedThinking
        // 期望: 降级为普通文本，不带 thought: true
        let req = ClaudeRequest {
            model: "claude-sonnet-4-6".to_string(),
            messages: vec![Message {
                role: "assistant".to_string(),
                content: MessageContent::Array(vec![
                    ContentBlock::RedactedThinking {
                        data: "some data".to_string(),
                    },
                    ContentBlock::Text {
                        text: "Hi".to_string(),
                    },
                ]),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        let result =
            transform_claude_request_in(&req, "test-project", false, None, "test_session", None);
        assert!(result.is_ok());
        let body = result.unwrap();
        let parts = body["request"]["contents"][0]["parts"].as_array().unwrap();

        // 验证 RedactedThinking -> Text 存在且不带 thought: true
        let redacted_part = parts
            .iter()
            .find(|p| {
                p.get("text")
                    .and_then(|t| t.as_str())
                    .map_or(false, |t| t.contains("[Redacted Thinking: some data]"))
            })
            .expect("Should find redacted thinking degraded text");
        assert!(
            redacted_part.get("thought").is_none(),
            "Redacted thinking should NOT have thought: true"
        );
    }

    // ==================================================================================
    // [FIX #564] Test: Thinking blocks are sorted to be first after context compression
    // ==================================================================================
    #[test]
    fn test_thinking_blocks_sorted_first_after_compression() {
        // Simulate kilo context compression reordering: text BEFORE thinking
        let mut messages = vec![Message {
            role: "assistant".to_string(),
            content: MessageContent::Array(vec![
                // Wrong order: Text before Thinking (simulates kilo compression)
                ContentBlock::Text {
                    text: "Some regular text".to_string(),
                },
                ContentBlock::Thinking {
                    thinking: "My thinking process".to_string(),
                    signature: Some(
                        "valid_signature_1234567890_abcdefghij_klmnopqrstuvwxyz_test".to_string(),
                    ),
                    cache_control: None,
                },
                ContentBlock::Text {
                    text: "More text".to_string(),
                },
            ]),
        }];

        // Apply the fix
        sort_thinking_blocks_first(&mut messages);

        // Verify thinking is now first
        if let MessageContent::Array(blocks) = &messages[0].content {
            assert_eq!(blocks.len(), 3, "Should still have 3 blocks");
            assert!(
                matches!(blocks[0], ContentBlock::Thinking { .. }),
                "Thinking should be first"
            );
            assert!(
                matches!(blocks[1], ContentBlock::Text { .. }),
                "Text should be second"
            );
            assert!(
                matches!(blocks[2], ContentBlock::Text { .. }),
                "Text should be third"
            );

            // Verify content preserved
            if let ContentBlock::Thinking { thinking, .. } = &blocks[0] {
                assert_eq!(thinking, "My thinking process");
            }
        } else {
            panic!("Expected Array content");
        }
    }

    #[test]
    fn test_thinking_blocks_no_reorder_when_already_first() {
        // Correct order: Thinking already first - should not trigger reorder
        let mut messages = vec![Message {
            role: "assistant".to_string(),
            content: MessageContent::Array(vec![
                ContentBlock::Thinking {
                    thinking: "My thinking".to_string(),
                    signature: Some("sig123".to_string()),
                    cache_control: None,
                },
                ContentBlock::Text {
                    text: "Some text".to_string(),
                },
            ]),
        }];

        // Apply the fix (should be no-op)
        sort_thinking_blocks_first(&mut messages);

        // Verify order unchanged
        if let MessageContent::Array(blocks) = &messages[0].content {
            assert!(
                matches!(blocks[0], ContentBlock::Thinking { .. }),
                "Thinking should still be first"
            );
            assert!(
                matches!(blocks[1], ContentBlock::Text { .. }),
                "Text should still be second"
            );
        }
    }

    #[test]
    fn test_merge_consecutive_messages() {
        let mut messages = vec![
            Message {
                role: "user".to_string(),
                content: MessageContent::String("Hello".to_string()),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Array(vec![ContentBlock::Text {
                    text: "World".to_string(),
                }]),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::String("Hi".to_string()),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Array(vec![ContentBlock::ToolResult {
                    tool_use_id: "test_id".to_string(),
                    content: serde_json::json!("result"),
                    is_error: None,
                }]),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Array(vec![ContentBlock::Text {
                    text: "System Reminder".to_string(),
                }]),
            },
        ];

        merge_consecutive_messages(&mut messages);

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        if let MessageContent::Array(blocks) = &messages[0].content {
            assert_eq!(blocks.len(), 2);
            match &blocks[0] {
                ContentBlock::Text { text } => assert_eq!(text, "Hello"),
                _ => panic!("Expected text block"),
            }
            match &blocks[1] {
                ContentBlock::Text { text } => assert_eq!(text, "World"),
                _ => panic!("Expected text block"),
            }
        } else {
            panic!("Expected array content at index 0");
        }

        assert_eq!(messages[1].role, "assistant");

        assert_eq!(messages[2].role, "user");
        if let MessageContent::Array(blocks) = &messages[2].content {
            assert_eq!(blocks.len(), 2);
            match &blocks[0] {
                ContentBlock::ToolResult { tool_use_id, .. } => assert_eq!(tool_use_id, "test_id"),
                _ => panic!("Expected tool_result block"),
            }
            match &blocks[1] {
                ContentBlock::Text { text } => assert_eq!(text, "System Reminder"),
                _ => panic!("Expected text block"),
            }
        } else {
            panic!("Expected array content at index 2");
        }
    }
    #[test]
    fn test_default_max_tokens() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let req = ClaudeRequest {
            model: "non-reasoning-model".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Hello".to_string()),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        let result =
            transform_claude_request_in(&req, "test-v", false, None, "test_session", None).unwrap();
        // Official pipeline topology populates official default maxOutputTokens (65536)
        let gen_config = &result["request"]["generationConfig"];
        let max_output = gen_config.get("maxOutputTokens").and_then(Value::as_i64);
        assert_eq!(max_output, Some(65536));
    }
    #[test]
    fn test_claude_flash_thinking_budget_capping() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Use full path or ensure import of ThinkingConfig
        // transform_claude_request and models are needed.
        // Assuming models are available via super imports, but let's be explicit if needed.

        // Setup request with high budget
        let req = ClaudeRequest {
            model: "gemini-2.0-flash-thinking-exp".to_string(), // Contains "flash"
            messages: vec![],
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(32000),
                effort: None,
            }),
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None, // Added missing field
            stream: false,
            system: None,
            tools: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        let result =
            transform_claude_request_in(&req, "proj", false, None, "test_session", None).unwrap();
        let budget = result["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"]
            .as_u64()
            .unwrap();
        assert_eq!(budget, 24576); // capped by model_specs.get_thinking_budget("gemini-2.0-flash-thinking-exp")

        // Setup request for Pro thinking model (mock name for testing)
        let req_pro = ClaudeRequest {
            model: "gemini-2.0-pro-thinking-exp".to_string(), // Contains "thinking" but not "flash"
            messages: vec![],
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(32000),
                effort: None,
            }),
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None, // Added missing field
            stream: false,
            system: None,
            tools: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        // Should cap
        let result_pro =
            transform_claude_request_in(&req_pro, "proj", false, None, "test_session", None)
                .unwrap();
        assert_eq!(
            result_pro["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            49152
        );
    }

    #[test]
    fn test_gemini_pro_thinking_support() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Setup request for Gemini Pro (no -thinking suffix)
        let req = ClaudeRequest {
            model: "gemini-3-pro-preview".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Hello".to_string()),
            }],
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(16000),
                effort: None,
            }),
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: false,
            system: None,
            tools: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        // Transform
        let result =
            transform_claude_request_in(&req, "proj", false, None, "test_session", None).unwrap();
        let gen_config = &result["request"]["generationConfig"];

        // thinkingConfig should be present (not forced disabled)
        assert!(
            gen_config.get("thinkingConfig").is_some(),
            "thinkingConfig should be preserved for gemini-3-pro"
        );

        let budget = gen_config["thinkingConfig"]["thinkingBudget"]
            .as_u64()
            .unwrap();
        // In Auto mode, client's budget is ignored and bare gemini-3-pro defaults to 10001
        assert_eq!(budget, 10001);
    }

    #[test]
    fn test_gemini_pro_default_thinking() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Setup request for Gemini Pro WITHOUT thinking config
        let req = ClaudeRequest {
            model: "gemini-3-pro-preview".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Hello".to_string()),
            }],
            thinking: None, // No thinking config provided by client
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: false,
            system: None,
            tools: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        // Transform
        let result =
            transform_claude_request_in(&req, "proj", false, None, "test_session", None).unwrap();
        let gen_config = &result["request"]["generationConfig"];

        // thinkingConfig SHOULD be injected because of default-on logic
        assert!(
            gen_config.get("thinkingConfig").is_some(),
            "thinkingConfig should be auto-enabled for gemini-3-pro"
        );
    }

    #[test]
    fn test_claude_image_thinking_mode_disabled() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // 1. Force image thinking mode to "disabled"
        crate::proxy::config::update_image_thinking_mode(Some("disabled".to_string()));
        struct ImageResetGuard;
        impl Drop for ImageResetGuard {
            fn drop(&mut self) {
                crate::proxy::config::update_image_thinking_mode(Some("enabled".to_string()));
            }
        }
        let _guard = ImageResetGuard;

        // 2. Setup Claude request for an image model (mapped to gemini-3-pro-image)
        let req = ClaudeRequest {
            model: "gemini-3-pro-image".to_string(), // Explicitly use recognized image model
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Draw a cat".to_string()),
            }],
            thinking: None,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: false,
            system: None,
            tools: None,
            metadata: None,
            output_config: None,
            size: Some("1024x1024".to_string()),
            quality: Some("hd".to_string()),
            tool_choice: None,
        };

        // 3. Transform request
        let result =
            transform_claude_request_in(&req, "test-proj", false, None, "test_session", None)
                .unwrap();

        // 4. Verify thinkingConfig has includeThoughts: false
        let gen_config = result["request"]["generationConfig"]
            .as_object()
            .expect("Should have generationConfig");
        let thinking_config = gen_config
            .get("thinkingConfig")
            .and_then(|t| t.as_object())
            .expect("Should have thinkingConfig (explicitly disabled)");

        assert_eq!(thinking_config["includeThoughts"], false);
    }

    #[test]
    fn test_claude_adaptive_global_config() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Set global config to Adaptive + High effort
        let config = ThinkingBudgetConfig {
            mode: crate::proxy::config::ThinkingBudgetMode::Adaptive,
            claude_mode: crate::proxy::config::ThinkingBudgetMode::Custom,
            custom_value: 0,
            effort: Some("high".to_string()),
            claude_budget: 16384,
            claude_high: 16384,
            ..Default::default()
        };
        crate::proxy::config::update_thinking_budget_config(config);
        struct ResetGuard;
        impl Drop for ResetGuard {
            fn drop(&mut self) {
                crate::proxy::config::update_thinking_budget_config(ThinkingBudgetConfig::default());
            }
        }
        let _guard = ResetGuard;

        let req = ClaudeRequest {
            model: "claude-3-7-sonnet-thinking".to_string(), // thinking capable
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("test".to_string()),
            }],
            thinking: None, // No client thinking config
            stream: false,
            // ... minimal fields
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            system: None,
            tools: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        // Transform
        let result =
            transform_claude_request_in(&req, "test-proj", false, None, "test_session", None)
                .unwrap();

        let gen_config = result["request"]["generationConfig"].as_object().unwrap();
        let thinking_config = gen_config["thinkingConfig"].as_object().unwrap();

        // 网关模式下 Claude 模型走流水线：effort=high 对应 claude_high，不再按协议写 thinkingLevel
        assert_eq!(thinking_config["includeThoughts"], true);
        assert_eq!(thinking_config["thinkingBudget"], 16384);
        assert!(thinking_config.get("thinkingLevel").is_none());
        assert!(gen_config.get("topP").is_none());
        assert!(gen_config.get("topK").is_none());

        let max_output_tokens = gen_config["maxOutputTokens"].as_i64().unwrap();
        assert_eq!(max_output_tokens, 24576);
    }

    #[test]
    fn test_mixed_tools_injection_for_gemini_2_0() {
        // [场景] 使用 Gemini 2.0 模型，同时提供自定义工具和启用全网搜索
        // 期望: 转换后的请求应同时包含 googleSearch 和 functionDeclarations
        let req = ClaudeRequest {
            model: "claude-sonnet-4-6".to_string(), // 映射到 gemini-2.0-flash-exp
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Help me search and use tools".to_string()),
            }],
            system: None,
            tools: Some(vec![Tool {
                type_: None,
                name: Some("get_weather".to_string()),
                description: Some("Get weather".to_string()),
                input_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"}
                    }
                })),
            }]),
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        // 模拟映射到 Gemini 2.0
        let mapped_model = "gemini-2.0-flash-exp";

        // 这里我们直接测试 build_tools 函数 (它是 pub(crate) 且在同模块下)
        let result = build_tools(&req.tools, true, mapped_model);
        assert!(result.is_ok());

        let tools_val = result.unwrap().expect("Should have tools");
        let tools_arr = tools_val.as_array().expect("Tools should be an array");

        let has_google_search = tools_arr.iter().any(|t| t.get("googleSearch").is_some());
        let has_functions = tools_arr
            .iter()
            .any(|t| t.get("functionDeclarations").is_some());

        // 在 v1internal 接口受限环境下，强制禁用混合工具调用以避免 400 报错
        // 存在自定义工具时，优先使用 functionDeclarations，不注入 googleSearch
        assert!(
            !has_google_search,
            "v1internal should avoid mixed Google Search when functionDeclarations present"
        );
        assert!(has_functions, "Should have function declarations");
    }

    #[test]
    fn test_no_mixed_tools_for_older_gemini() {
        // [场景] 使用 Gemini 1.5 模型，同时提供自定义工具和启用全网搜索
        // 期望: 转换后的请求应只包含 functionDeclarations，googleSearch 被跳过以避免 400
        let req = ClaudeRequest {
            model: "claude-sonnet-4-6".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Help me search and use tools".to_string()),
            }],
            system: None,
            tools: Some(vec![Tool {
                type_: None,
                name: Some("get_weather".to_string()),
                description: Some("Get weather".to_string()),
                input_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"}
                    }
                })),
            }]),
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        // 模拟映射到 Gemini 1.5
        let mapped_model = "gemini-1.5-flash-002";

        // 测试 build_tools 函数
        let result = build_tools(&req.tools, true, mapped_model);
        assert!(result.is_ok());

        let tools_val = result.unwrap().expect("Should have tools");
        let tools_arr = tools_val.as_array().expect("Tools should be an array");

        let has_google_search = tools_arr.iter().any(|t| t.get("googleSearch").is_some());
        let has_functions = tools_arr
            .iter()
            .any(|t| t.get("functionDeclarations").is_some());

        assert!(
            !has_google_search,
            "Older Gemini models should NOT have mixed tools"
        );
        assert!(has_functions);
    }

    #[test]
    fn web_search_only_injects_google_search() {
        let tools = Some(vec![Tool {
            type_: Some("web_search_20250305".to_string()),
            name: Some("web_search".to_string()),
            description: None,
            input_schema: None,
        }]);
        let tools_val = build_tools(&tools, true, "gemini-3.8-flash-high")
            .unwrap()
            .expect("search-only request should still carry a tool");
        let tools_arr = tools_val.as_array().expect("tools array");
        assert!(
            tools_arr.iter().any(|t| t.get("googleSearch").is_some()),
            "web_search alone must inject googleSearch"
        );
        assert!(
            tools_arr
                .iter()
                .all(|t| t.get("functionDeclarations").is_none()),
            "web_search must not be declared as a client function"
        );
    }

    #[test]
    fn web_search_alongside_client_tools_stays_in_function_declarations() {
        let tools = Some(vec![
            Tool {
                type_: Some("web_search_20250305".to_string()),
                name: Some("web_search".to_string()),
                description: Some("Search".to_string()),
                input_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": { "query": { "type": "string" } }
                })),
            },
            Tool {
                type_: None,
                name: Some("Bash".to_string()),
                description: Some("Run a command".to_string()),
                input_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": { "command": { "type": "string" } }
                })),
            },
        ]);
        let tools_val = build_tools(&tools, true, "gemini-3.8-flash-high")
            .unwrap()
            .expect("mixed tools");
        let tools_arr = tools_val.as_array().expect("tools array");
        assert!(
            tools_arr.iter().all(|t| t.get("googleSearch").is_none()),
            "function tools and googleSearch must not be mixed"
        );
        let names: Vec<&str> = tools_arr
            .iter()
            .filter_map(|t| t.get("functionDeclarations")?.as_array())
            .flatten()
            .filter_map(|decl| decl.get("name")?.as_str())
            .collect();
        assert!(names.contains(&"web_search"));
        assert!(names.contains(&"Bash"));
    }

    #[test]
    fn test_model_supports_thinking() {
        // gemini-pro-agent is the mapped target of gemini-3.1-pro-high /
        // gemini-3-pro-high and is a forced-thinking Pro agent model.
        assert!(model_supports_thinking("gemini-pro-agent"));
        assert!(model_supports_thinking("gemini-3-pro"));
        assert!(model_supports_thinking("gemini-3-pro-preview"));
        assert!(model_supports_thinking("gemini-3.1-pro"));
        assert!(model_supports_thinking("gemini-3.1-pro-preview"));
        assert!(model_supports_thinking("gemini-2.0-pro"));
        assert!(model_supports_thinking("gemini-3-flash"));
        assert!(model_supports_thinking("gemini-3.1-flash"));
        assert!(model_supports_thinking("claude-opus-4-6-thinking"));

        // Keyword models (gemini/flash/pro/agent) now force server-side thinking.
        assert!(model_supports_thinking("gemini-2.5-pro"));
        assert!(model_supports_thinking("gemini-1.5-pro"));
    }

    #[test]
    fn test_thinking_block_preserved_with_empty_signature() {
        use crate::proxy::mappers::claude::thinking_utils::filter_invalid_thinking_blocks_with_family;

        let mut messages = vec![
            Message {
                role: "user".to_string(),
                content: MessageContent::String("Hello".to_string()),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Array(vec![
                    ContentBlock::Thinking {
                        thinking: "Considering the question deeply...".to_string(),
                        signature: Some("".to_string()), // Client sends empty signature
                        cache_control: None,
                    },
                    ContentBlock::Text {
                        text: "Here is my answer".to_string(),
                    },
                ]),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::String("Next question".to_string()),
            },
        ];

        // 1. filter_invalid_thinking_blocks_with_family must NOT drop the thinking block
        filter_invalid_thinking_blocks_with_family(&mut messages, Some("gemini"));
        if let MessageContent::Array(blocks) = &messages[1].content {
            assert_eq!(blocks.len(), 2, "Thinking block must NOT be dropped!");
            assert!(matches!(blocks[0], ContentBlock::Thinking { .. }));
        } else {
            panic!("Expected array content");
        }

        // 2. transform_claude_request_in 针对 Claude 模型，无有效签名时为防上游 400 应降级为 <think> 文本保留语义
        let req = ClaudeRequest {
            model: "claude-sonnet-4-6".to_string(),
            messages,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(12288),
                effort: None,
            }),
            system: None,
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        let result =
            transform_claude_request_in(&req, "test-proj", false, None, "test-session", None)
                .expect("Transform should succeed");

        let contents = result["request"]["contents"]
            .as_array()
            .expect("Contents array");
        let assistant_parts = contents[1]["parts"].as_array().expect("Assistant parts");
        assert_eq!(assistant_parts.len(), 2);
        assert!(assistant_parts[0]["text"]
            .as_str()
            .unwrap_or("")
            .contains("Considering the question deeply..."));
        assert_eq!(assistant_parts[1]["text"], "Here is my answer");
    }

    #[test]
    fn test_tool_call_inherits_real_signature_without_sentinel() {
        use crate::proxy::mappers::claude::thinking_utils::filter_invalid_thinking_blocks_with_family;
        let real_sig = "Ep4MCpsMARFNMg9NDlK9RXXz5Mzq9mniX9KSQBBzbUx3k85w/qDgtcE+28NH+1EvPeULAprqUquvYXGMzUXGy1xJoMnqdkC4vqebuhyd2Xhs0oz+OhqcOTwLhGYOG0KBKQ87Hfw4q/sMCSgf2gz4vFMa6V6kKMJepYlPXKFJJF4ok+W6lUt3PfYln8K9Dh7wB/40iHiZ2BnJd++6hfUwu9Bz1n795S50l0yCj84EaSCDDF334Erxq7Fo";

        let mut messages = vec![
            Message {
                role: "user".to_string(),
                content: MessageContent::String("List directory".to_string()),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Array(vec![
                    ContentBlock::Thinking {
                        thinking: "Confirming conversational readiness".to_string(),
                        signature: Some(real_sig.to_string()),
                        cache_control: None,
                    },
                    ContentBlock::ToolUse {
                        id: "call_548709".to_string(),
                        name: "list_directory".to_string(),
                        input: serde_json::json!({}),
                        signature: None,
                        cache_control: None,
                    },
                ]),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Array(vec![ContentBlock::ToolResult {
                    tool_use_id: "call_548709".to_string(),
                    content: serde_json::json!("file1.txt\nfile2.txt"),
                    is_error: None,
                }]),
            },
        ];

        // 1. filter_invalid_thinking_blocks_with_family must NOT strip real signature even if family cache is empty
        filter_invalid_thinking_blocks_with_family(&mut messages, Some("gemini"));
        if let MessageContent::Array(blocks) = &messages[1].content {
            assert_eq!(blocks.len(), 2);
            if let ContentBlock::Thinking { signature, .. } = &blocks[0] {
                assert_eq!(
                    signature.as_deref(),
                    Some(real_sig),
                    "Real signature must NOT be stripped!"
                );
            } else {
                panic!("Expected thinking block");
            }
        }

        // 2. transform_claude_request_in should map both thinking and functionCall with the real signature
        let req = ClaudeRequest {
            model: "gemini-3.8-flash-high".to_string(),
            messages,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(12288),
                effort: None,
            }),
            system: None,
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        let result =
            transform_claude_request_in(&req, "test-proj", false, None, "test-session", None)
                .expect("Transform should succeed");

        let contents = result["request"]["contents"]
            .as_array()
            .expect("Contents array");
        let assistant_parts = contents[1]["parts"].as_array().expect("Assistant parts");
        // Gemini 目标在历史轮次中按上游规范剥离思考文本，仅保留工具调用，且工具调用继承思考块真实签名
        assert_eq!(assistant_parts.len(), 1);
        assert_eq!(assistant_parts[0]["functionCall"]["name"], "list_directory");
        assert_eq!(
            assistant_parts[0]["thoughtSignature"], real_sig,
            "Gemini model functionCall must inherit the real signature from the thinking block"
        );
    }

    #[test]
    fn test_foreign_claude_signature_dropped_for_gemini() {
        use crate::proxy::mappers::claude::thinking_utils::filter_invalid_thinking_blocks_with_family;
        let foreign_claude_sig = "3mgp11XmVXq9InniGA4VAKd7c97NqFw+dWZt79Uz/w9znho88gSM76jv2bZmir7wI86Ixpha7eWdGuznAot4PNbe3+V9bgMTIEyUarn4MLAiiFVb830ZlM+H5ukQwXdD2Zv8nUSmmZTYinpLPGha8TORZAfpU1FJEvwyECel5+W7kc9kpTWrd8DqRNBTOz5EDtvoatiZgKv5SqInhGXK74SJ+PRIC6fNXvYG082HR6TsVxvVYaerz8A40rloIVTxRNK43h3Ecs1boxY4PZqBT8Yhl2qn/iZ+4Xt7FNkI0DAuS9iK0HYKMC4yw0OqKx/LeU+WFZlyc6hGm1BkzLY6yG97MH7kmJ0OPlBWgWFaTeL/uXuGJX6QkKObXN+phoq+kkF2vdFt/mdJMbdgfmSCVQ9037hGBhOHm0zN50KLkp1SxuAY1oWc+lDcI4ufWoyn";

        let mut messages = vec![
            Message {
                role: "user".to_string(),
                content: MessageContent::String("Run tool".to_string()),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Array(vec![
                    ContentBlock::Thinking {
                        thinking: "Thinking from foreign Claude model".to_string(),
                        signature: Some(foreign_claude_sig.to_string()),
                        cache_control: None,
                    },
                    ContentBlock::ToolUse {
                        id: "call_999999".to_string(),
                        name: "web_fetch".to_string(),
                        input: serde_json::json!({"url": "https://example.com"}),
                        signature: None,
                        cache_control: None,
                    },
                ]),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Array(vec![ContentBlock::ToolResult {
                    tool_use_id: "call_999999".to_string(),
                    content: serde_json::json!("ok"),
                    is_error: None,
                }]),
            },
        ];

        // 1. 验证 filter_invalid_thinking_blocks_with_family 会将非 Gemini 格式的异构签名从思考块中剔除
        filter_invalid_thinking_blocks_with_family(&mut messages, Some("gemini"));
        if let MessageContent::Array(blocks) = &messages[1].content {
            if let ContentBlock::Thinking { signature, .. } = &blocks[0] {
                assert!(
                    signature.is_none(),
                    "Foreign Claude signature must be stripped for Gemini target!"
                );
            }
        }

        // 2. 验证 transform_claude_request_in 在目标为 Gemini 时，思考块与工具调用的签名均安全降级为哨兵
        let req = ClaudeRequest {
            model: "gemini-3.7-flash-high".to_string(),
            messages,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(8192),
                effort: None,
            }),
            system: None,
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(
            &req,
            "test-proj",
            false,
            None,
            "test-session-foreign",
            None,
        )
        .expect("Transform should succeed");

        let contents = result["request"]["contents"]
            .as_array()
            .expect("Contents array");
        let assistant_parts = contents[1]["parts"].as_array().expect("Assistant parts");
        assert_eq!(assistant_parts.len(), 1);
        assert_eq!(assistant_parts[0]["functionCall"]["name"], "web_fetch");
        // 异构 Claude 签名被成功剥离，绝不继承该外来签名；出站门禁自动补齐安全哨兵防止上游 AST 校验 400
        assert_ne!(
            assistant_parts[0]
                .get("thoughtSignature")
                .and_then(|s| s.as_str()),
            Some(foreign_claude_sig)
        );
        assert_eq!(
            assistant_parts[0]["thoughtSignature"],
            crate::proxy::thinking_store::SENTINEL_SIGNATURE
        );
    }

    #[test]
    fn test_claude_request_with_corrupt_and_empty_images_defense() {
        let valid_png_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let req = ClaudeRequest {
            model: "claude-3-7-sonnet-20250219".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Array(vec![
                    ContentBlock::Text {
                        text: "Look at these images".to_string(),
                    },
                    // Corrupt image block 1 (empty data)
                    ContentBlock::Image {
                        source: ImageSource {
                            source_type: "base64".to_string(),
                            media_type: None,
                            data: None,
                        },
                        cache_control: None,
                    },
                    // Corrupt image block 2 (invalid short base64 +A==)
                    ContentBlock::Image {
                        source: ImageSource {
                            source_type: "base64".to_string(),
                            media_type: Some("image/png".to_string()),
                            data: Some("+A==".to_string()),
                        },
                        cache_control: None,
                    },
                    // Valid image block
                    ContentBlock::Image {
                        source: ImageSource {
                            source_type: "base64".to_string(),
                            media_type: Some("image/png".to_string()),
                            data: Some(valid_png_b64.to_string()),
                        },
                        cache_control: None,
                    },
                ]),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        let result =
            transform_claude_request_in(&req, "test-proj", false, None, "test-session", None)
                .expect("Transform must not fail on corrupt images");

        let contents = result["request"]["contents"].as_array().unwrap();
        let user_parts = contents[0]["parts"].as_array().unwrap();
        assert_eq!(user_parts.len(), 4);
        assert_eq!(user_parts[0]["text"], "Look at these images");
        assert_eq!(
            user_parts[1]["text"],
            "[Image: invalid or corrupted data omitted]"
        );
        assert_eq!(
            user_parts[2]["text"],
            "[Image: invalid or corrupted data omitted]"
        );
        assert!(user_parts[3].get("inlineData").is_some());
        assert_eq!(user_parts[3]["inlineData"]["mimeType"], "image/png");
        assert_eq!(user_parts[3]["inlineData"]["data"], valid_png_b64);
    }

    #[test]
    fn test_gemini_under_v3_thinking_disabled_and_no_thinking_config() {
        // 验证 gemini-2.5-flash 请求，即使客户端带或不带 thinking，也绝对不会注入 thinkingConfig
        let req = ClaudeRequest {
            model: "gemini-2.5-flash".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Hello 123".to_string()),
            }],
            thinking: None,
            max_tokens: Some(1024),
            temperature: None,
            top_p: None,
            top_k: None,
            stream: false,
            system: None,
            tools: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        let result =
            transform_claude_request_in(&req, "test-proj", false, None, "test-session", None)
                .unwrap();

        let gen_config = &result["request"]["generationConfig"];
        assert!(
            gen_config.get("thinkingConfig").is_none(),
            "gemini-2.5-flash must NOT have thinkingConfig"
        );
    }

    #[test]
    fn test_gemini_v3_flash_high_thinking_enabled_and_client_budget_ignored() {
        // 验证 gemini-3.7-flash-high 开启思考，并且客户端传的 99999 预算被忽略，强制使用档位字典的 10000
        let req = ClaudeRequest {
            model: "gemini-3.7-flash-high".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Hello".to_string()),
            }],
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(99999), // 客户端传入过大/错误预算
                effort: None,
            }),
            max_tokens: Some(1024),
            temperature: None,
            top_p: None,
            top_k: None,
            stream: false,
            system: None,
            tools: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        let result =
            transform_claude_request_in(&req, "test-proj", false, None, "test-session", None)
                .unwrap();

        let gen_config = &result["request"]["generationConfig"];
        let thinking_config = gen_config
            .get("thinkingConfig")
            .expect("gemini-3.7-flash-high must have thinkingConfig");

        assert_eq!(thinking_config["includeThoughts"], true);
        assert_eq!(
            thinking_config["thinkingBudget"], -1,
            "Client budget (99999) must be ignored in favor of tier dictionary budget (-1)"
        );
    }

    #[test]
    fn test_gemini_client_billing_metadata_filtering() {
        // [Issue #3452] Standalone billing header line should be filtered for Gemini targets
        assert!(is_gemini_client_billing_metadata(
            "gemini-3.8-flash-high",
            "x-anthropic-billing-header: cc_version=2.1.270.ffc; cc_entrypoint=claude-desktop-3p;"
        ));
        assert!(is_gemini_client_billing_metadata(
            "gemini-2.5-flash",
            "  x-anthropic-billing-header: cc_entrypoint=desktop; \n"
        ));

        // 风险清洗不分 model：非 Gemini 模型同样必须过滤
        // （任何客户端注入的计费元数据都是风险，与当前模型无关）
        assert!(is_gemini_client_billing_metadata(
            "claude-sonnet-4-6",
            "x-anthropic-billing-header: cc_version=2.1.270.ffc;"
        ));

        // Multiline text should NOT be filtered
        assert!(!is_gemini_client_billing_metadata(
            "gemini-3.8-flash-high",
            "x-anthropic-billing-header: test;\nSecond line prompt instruction"
        ));

        // Unrelated system prompt should NOT be filtered
        assert!(!is_gemini_client_billing_metadata(
            "gemini-3.8-flash-high",
            "You are an expert coder."
        ));
    }

    #[test]
    fn test_claude_desktop_billing_metadata_filtered_in_transform_for_gemini() {
        let req: ClaudeRequest = serde_json::from_value(json!({
            "model": "gemini-3.8-flash-high",
            "messages": [{"role": "user", "content": "Hello"}],
            "system": [
                {
                    "type": "text",
                    "text": "x-anthropic-billing-header: cc_version=2.1.270.ffc; cc_entrypoint=claude-desktop-3p;"
                },
                {
                    "type": "text",
                    "text": "You are a helpful assistant."
                }
            ]
        }))
        .expect("ClaudeRequest should deserialize");

        let body =
            transform_claude_request_in(&req, "test-project", false, None, "test-session", None)
                .expect("Request should transform");
        let system_parts = body["request"]["systemInstruction"]["parts"]
            .as_array()
            .expect("system instruction should contain parts");
        let system_texts = system_parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>();

        // Billing header must be filtered out to avoid Gemini 429 RESOURCE_EXHAUSTED (#3452)
        assert!(!system_texts
            .iter()
            .any(|t| t.contains("x-anthropic-billing-header:")));
        // Normal prompt must be preserved
        assert!(system_texts.contains(&"You are a helpful assistant."));
    }

    #[test]
    fn test_claude_tool_result_multimodal_string_extraction() {
        let fake_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let tool_content = format!(
            "Here is the screenshot: ![screen](data:image/png;base64,{}) and log text",
            fake_b64
        );

        let req: ClaudeRequest = serde_json::from_value(json!({
            "model": "gemini-2.5-flash",
            "messages": [
                {
                    "role": "user",
                    "content": "Take screenshot"
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool_use",
                            "id": "toolu_shot_1",
                            "name": "screenshot",
                            "input": {}
                        }
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "tool_result",
                            "tool_use_id": "toolu_shot_1",
                            "content": tool_content
                        }
                    ]
                }
            ]
        }))
        .expect("ClaudeRequest should deserialize");

        let body =
            transform_claude_request_in(&req, "test-project", false, None, "test-session", None)
                .expect("Request should transform");
        let contents = body["request"]["contents"]
            .as_array()
            .expect("contents array");
        assert_eq!(contents.len(), 4);

        // contents[2] 为 functionResponse (role: "model")
        let tool_parts = contents[2]["parts"].as_array().expect("tool turn parts");
        assert_eq!(tool_parts.len(), 1);
        assert!(tool_parts[0].get("functionResponse").is_some());

        let res_str = tool_parts[0]["functionResponse"]["response"]["output"]
            .as_str()
            .unwrap();
        assert!(!res_str.contains(fake_b64));
        assert!(res_str.contains("[Image: forwarded to visual input (image/png)]"));

        // contents[3] 为依据 [zwx-patch] 拆解出的 inlineData 视觉媒体轮次 (role: "user")
        let media_parts = contents[3]["parts"].as_array().expect("media turn parts");
        assert_eq!(media_parts.len(), 1);
        assert!(media_parts[0].get("inlineData").is_some());

        let inline_data = &media_parts[0]["inlineData"];
        assert_eq!(inline_data["mimeType"], "image/png");
        assert_eq!(inline_data["data"], fake_b64);
    }
}
