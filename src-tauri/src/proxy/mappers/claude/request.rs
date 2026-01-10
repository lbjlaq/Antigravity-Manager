// Claude 请求转换 (Claude → Gemini v1internal)
// 对应 transformClaudeRequestIn

use super::models::*;
use crate::proxy::mappers::signature_store::get_thought_signature;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::OnceLock;
use tauri::Emitter;

// Global AppHandle for emitting Tauri events
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Set the global AppHandle (called once during server initialization)
pub fn set_app_handle(app: tauri::AppHandle) {
    let _ = APP_HANDLE.set(app);
}

/// Get the global AppHandle
pub fn get_app_handle() -> Option<&'static tauri::AppHandle> {
    APP_HANDLE.get()
}

/// Emit model fallback event to UI
fn emit_model_fallback_event(original_model: &str, fallback_model: &str) -> Result<(), String> {
    if let Some(app) = APP_HANDLE.get() {
        let payload = serde_json::json!({
            "original_model": original_model,
            "fallback_model": fallback_model,
            "reason": "High timeout rate (93.7%) with Claude Opus Thinking - see issue #497"
        });

        app.emit("proxy://model-fallback", payload)
            .map_err(|e| format!("Failed to emit model fallback event: {}", e))?;

        tracing::debug!(
            "[Model-Fallback-Event] Emitted UI notification: {} -> {}",
            original_model,
            fallback_model
        );
    }
    Ok(())
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
        { "category": "HARM_CATEGORY_CIVIC_INTEGRITY", "threshold": threshold_str },
    ])
}

/// 清理消息中的 cache_control 字段
/// 
/// 这个函数会深度遍历所有消息内容块,移除 cache_control 字段。
/// 这是必要的,因为:
/// 1. VS Code 等客户端会将历史消息(包含 cache_control)原封不动发回
/// 2. Anthropic API 不接受请求中包含 cache_control 字段
/// 3. 即使是转发到 Gemini,也应该清理以保持协议纯净性
fn clean_cache_control_from_messages(messages: &mut [Message]) {
    for msg in messages.iter_mut() {
        if let MessageContent::Array(blocks) = &mut msg.content {
            for block in blocks.iter_mut() {
                match block {
                    ContentBlock::Thinking { cache_control, .. } => {
                        if cache_control.is_some() {
                            tracing::debug!("[Cache-Control-Cleaner] Removed cache_control from Thinking block");
                            *cache_control = None;
                        }
                    }
                    ContentBlock::Image { cache_control, .. } => {
                        if cache_control.is_some() {
                            tracing::debug!("[Cache-Control-Cleaner] Removed cache_control from Image block");
                            *cache_control = None;
                        }
                    }
                    ContentBlock::Document { cache_control, .. } => {
                        if cache_control.is_some() {
                            tracing::debug!("[Cache-Control-Cleaner] Removed cache_control from Document block");
                            *cache_control = None;
                        }
                    }
                    ContentBlock::ToolUse { cache_control, .. } => {
                        if cache_control.is_some() {
                            tracing::debug!("[Cache-Control-Cleaner] Removed cache_control from ToolUse block");
                            *cache_control = None;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// 转换 Claude 请求为 Gemini v1internal 格式
pub fn transform_claude_request_in(
    claude_req: &ClaudeRequest,
    project_id: &str,
) -> Result<Value, String> {
    // [CRITICAL FIX] 预先清理所有消息中的 cache_control 字段
    // 这解决了 VS Code 插件等客户端在多轮对话中将历史消息的 cache_control 字段
    // 原封不动发回导致的 "Extra inputs are not permitted" 错误
    let mut cleaned_req = claude_req.clone();
    clean_cache_control_from_messages(&mut cleaned_req.messages);
    let claude_req = &cleaned_req; // 后续使用清理后的请求

    // 检测是否有联网工具 (server tool or built-in tool)
    let has_web_search_tool = claude_req
        .tools
        .as_ref()
        .map(|tools| {
            tools.iter().any(|t| {
                t.is_web_search() 
                    || t.name.as_deref() == Some("google_search")
                    || t.type_.as_deref() == Some("web_search_20250305")
            })
        })
        .unwrap_or(false);

    // 用于存储 tool_use id -> name 映射
    let mut tool_id_to_name: HashMap<String, String> = HashMap::new();

    // 1. System Instruction (注入动态身份防护)
    let system_instruction = build_system_instruction(&claude_req.system, &claude_req.model);

    //  Map model name (Use standard mapping)
    // [IMPROVED] 提取 web search 模型为常量，便于维护
    const WEB_SEARCH_FALLBACK_MODEL: &str = "gemini-2.5-flash";

    let mapped_model = if has_web_search_tool {
        tracing::debug!(
            "[Claude-Request] Web search tool detected, using fallback model: {}",
            WEB_SEARCH_FALLBACK_MODEL
        );
        WEB_SEARCH_FALLBACK_MODEL.to_string()
    } else {
        crate::proxy::common::model_mapping::map_claude_model_to_gemini(&claude_req.model)
    };

    // 🆕 [FALLBACK REMOVED] Claude Opus Thinking → Gemini Pro High
    // Unconditional fallback removed! Now using conditional fallback in claude.rs handler
    // Fallback only triggers when ALL accounts are rate-limited for claude-opus-4-5-thinking
    // See: src-tauri/src/proxy/handlers/claude.rs:503-542
    
    // 将 Claude 工具转为 Value 数组以便探测联网
    let tools_val: Option<Vec<Value>> = claude_req.tools.as_ref().map(|list| {
        list.iter().map(|t| serde_json::to_value(t).unwrap_or(json!({}))).collect()
    });


    // Resolve grounding config
    let config = crate::proxy::mappers::common_utils::resolve_request_config(&claude_req.model, &mapped_model, &tools_val);
    
    // [CRITICAL FIX] Disable dummy thought injection for Vertex AI
    // [CRITICAL FIX] Disable dummy thought injection for Vertex AI
    // Vertex AI rejects thinking blocks without valid signatures
    // Even if thinking is enabled, we should NOT inject dummy blocks for historical messages
    let allow_dummy_thought = false;
    
    // Check if thinking is enabled in the request
    let mut is_thinking_enabled = claude_req
        .thinking
        .as_ref()
        .map(|t| t.type_ == "enabled")
        .unwrap_or_else(|| {
            // [Claude Code v2.0.67+] Default thinking enabled for Opus 4.5
            // If no thinking config is provided, enable by default for Opus models
            should_enable_thinking_by_default(&claude_req.model)
        });

    // [NEW FIX] Check if target model supports thinking
    // Claude models: thinking via "-thinking" suffix in model name
    // Gemini models: thinking via thinkingConfig parameter in API request (NOT in model name!)
    let target_model_supports_thinking = mapped_model.contains("-thinking")
        || mapped_model.starts_with("claude-")
        || mapped_model.starts_with("gemini-");
    
    if is_thinking_enabled && !target_model_supports_thinking {
        tracing::warn!(
            "[Thinking-Mode] Target model '{}' does not support thinking. Force disabling thinking mode.",
            mapped_model
        );
        is_thinking_enabled = false;
    }

    // [New Strategy] 智能降级: 检查历史消息是否与 Thinking 模式兼容
    // 如果处于未带 Thinking 的工具调用链中，必须临时禁用 Thinking
    if is_thinking_enabled {
        let should_disable = should_disable_thinking_due_to_history(&claude_req.messages);
        if should_disable {
             tracing::warn!("[Thinking-Mode] Automatically disabling thinking checks due to incompatible tool-use history (mixed application)");
             is_thinking_enabled = false;
        }
    }

    // [FIX #295 & #298] If thinking enabled but no signature available,
    // disable thinking to prevent Gemini 3 Pro rejection
    if is_thinking_enabled {
        let global_sig = get_thought_signature();
        
        // Check if there are any thinking blocks in message history
        let has_thinking_history = claude_req.messages.iter().any(|m| {
            if m.role == "assistant" {
                if let MessageContent::Array(blocks) = &m.content {
                    return blocks.iter().any(|b| matches!(b, ContentBlock::Thinking { .. }));
                }
            }
            false
        });
        
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

        // [FIX #298] For first-time thinking requests (no thinking history),
        // we use permissive mode and let upstream handle validation.
        // We only enforce strict signature checks when function calls are involved.
        let needs_signature_check = has_function_calls;
        
        if !has_thinking_history && is_thinking_enabled {
             tracing::info!(
                "[Thinking-Mode] First thinking request detected. Using permissive mode - \
                 signature validation will be handled by upstream API."
            );
        }

        if needs_signature_check
            && !has_valid_signature_for_function_calls(&claude_req.messages, &global_sig)
        {
            tracing::warn!(
                "[Thinking-Mode] [FIX #295] No valid signature found for function calls. \
                 Disabling thinking to prevent Gemini 3 Pro rejection."
            );
            is_thinking_enabled = false;
        }
    }

    // 4. Generation Config & Thinking (Pass final is_thinking_enabled)
    let generation_config = build_generation_config(claude_req, has_web_search_tool, is_thinking_enabled, &mapped_model);

    // 2. Contents (Messages)
    let contents = build_contents(
        &claude_req.messages,
        &mut tool_id_to_name,
        is_thinking_enabled,
        allow_dummy_thought,
        &mapped_model,
    )?;

    // 3. Tools
    let tools = build_tools(&claude_req.tools, has_web_search_tool)?;

    // 5. Safety Settings (configurable via GEMINI_SAFETY_THRESHOLD env var)
    let safety_settings = build_safety_settings();

    // Build inner request
    let mut inner_request = json!({
        "contents": contents,
        "safetySettings": safety_settings,
    });

    // 深度清理 [undefined] 字符串 (Cherry Studio 等客户端常见注入)
    crate::proxy::mappers::common_utils::deep_clean_undefined(&mut inner_request);

    if let Some(sys_inst) = system_instruction {
        inner_request["systemInstruction"] = sys_inst;
    }

    if !generation_config.is_null() {
        inner_request["generationConfig"] = generation_config;
    }

    if let Some(tools_val) = tools {
        inner_request["tools"] = tools_val;
        // 显式设置工具配置模式为 VALIDATED
        inner_request["toolConfig"] = json!({
            "functionCallingConfig": {
                "mode": "VALIDATED"
            }
        });
    }

    // Inject googleSearch tool if needed (and not already done by build_tools)
    if config.inject_google_search && !has_web_search_tool {
        crate::proxy::mappers::common_utils::inject_google_search_tool(&mut inner_request);
    }

    // Inject imageConfig if present (for image generation models)
    if let Some(image_config) = config.image_config {
        if let Some(obj) = inner_request.as_object_mut() {
            // 1. Remove tools (image generation does not support tools)
            obj.remove("tools");

            // 2. Remove systemInstruction (image generation does not support system prompts)
            obj.remove("systemInstruction");

            // 3. Clean generationConfig (remove thinkingConfig, responseMimeType, responseModalities etc.)
            let gen_config = obj.entry("generationConfig").or_insert_with(|| json!({}));
            if let Some(gen_obj) = gen_config.as_object_mut() {
                gen_obj.remove("thinkingConfig");
                gen_obj.remove("responseMimeType");
                gen_obj.remove("responseModalities");
                gen_obj.insert("imageConfig".to_string(), image_config);
            }
        }
    }

    // 生成 requestId
    let request_id = format!("agent-{}", uuid::Uuid::new_v4());

    // 构建最终请求体
    let mut body = json!({
        "project": project_id,
        "requestId": request_id,
        "request": inner_request,
        "model": config.final_model,
        "userAgent": "antigravity",
        "requestType": config.request_type,
    });

    // 如果提供了 metadata.user_id，则复用为 sessionId
    if let Some(metadata) = &claude_req.metadata {
        if let Some(user_id) = &metadata.user_id {
            body["request"]["sessionId"] = json!(user_id);
        }
    }

    // [CRITICAL LOG] Log final request structure for debugging INVALID_ARGUMENT errors
    if let Some(gen_config) = body["request"].get("generationConfig") {
        if let Some(thinking_config) = gen_config.get("thinkingConfig") {
            let max_tokens = gen_config.get("maxOutputTokens").and_then(|v| v.as_u64()).unwrap_or(0);
            tracing::warn!(
                "[Claude-Request] THINKING MODE: model={}, maxOutputTokens={}, thinkingConfig={}",
                body["model"].as_str().unwrap_or("unknown"),
                max_tokens,
                serde_json::to_string(thinking_config).unwrap_or_default()
            );
        }
    }

    Ok(body)
}

/// 检查是否因为历史消息原因需要禁用 Thinking
/// 
/// 场景: 如果最后一条 Assistant 消息处于 Tool Use 流程中，但没有 Thinking 块，
/// 说明这是一个由非 Thinking 模型发起的流程。此时强制开启 Thinking 会导致:
/// "final assistant message must start with a thinking block" 错误。
/// 我们无法伪造合法的 Thinking (因为签名问题)，唯一的解法是本轮请求暂时禁用 Thinking。
fn should_disable_thinking_due_to_history(messages: &[Message]) -> bool {
    // 逆序查找最后一条 Assistant 消息
    for msg in messages.iter().rev() {
        if msg.role == "assistant" {
            if let MessageContent::Array(blocks) = &msg.content {
                let has_tool_use = blocks.iter().any(|b| matches!(b, ContentBlock::ToolUse { .. }));
                let has_thinking = blocks.iter().any(|b| matches!(b, ContentBlock::Thinking { .. }));
                
                // 如果有工具调用，但没有 Thinking 块 -> 不兼容
                if has_tool_use && !has_thinking {
                    tracing::info!("[Thinking-Mode] Detected ToolUse without Thinking in history. Requesting disable.");
                    return true;
                }
            }
            // 只要找到最近的一条 Assistant 消息就结束检查
            // 因为验证规则主要针对当前的闭环状态
            return false;
        }
    }
    false
}

/// Check if thinking mode should be enabled by default for a given model
///
/// Claude Code v2.0.67+ enables thinking by default for Opus 4.5 models.
/// This function determines if the model should have thinking enabled
/// when no explicit thinking configuration is provided.
fn should_enable_thinking_by_default(model: &str) -> bool {
    let model_lower = model.to_lowercase();

    // Enable thinking by default for Opus 4.5 variants
    if model_lower.contains("opus-4-5") || model_lower.contains("opus-4.5") {
        tracing::debug!(
            "[Thinking-Mode] Auto-enabling thinking for Opus 4.5 model: {}",
            model
        );
        return true;
    }

    // Also enable for explicit thinking model variants
    if model_lower.contains("-thinking") {
        return true;
    }

    false
}

/// Minimum length for a valid thought_signature
const MIN_SIGNATURE_LENGTH: usize = 50;

/// [FIX #295] Check if we have any valid signature available for function calls
/// This prevents Gemini 3 Pro from rejecting requests due to missing thought_signature
fn has_valid_signature_for_function_calls(
    messages: &[Message],
    global_sig: &Option<String>,
) -> bool {
    // 1. Check global store
    if let Some(sig) = global_sig {
        if sig.len() >= MIN_SIGNATURE_LENGTH {
            return true;
        }
    }

    // 2. Check if any message has a thinking block with valid signature
    for msg in messages.iter().rev() {
        if msg.role == "assistant" {
            if let MessageContent::Array(blocks) = &msg.content {
                for block in blocks {
                    if let ContentBlock::Thinking {
                        signature: Some(sig),
                        ..
                    } = block
                    {
                        if sig.len() >= MIN_SIGNATURE_LENGTH {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// 构建 System Instruction (支持动态身份映射与 Prompt 隔离)
fn build_system_instruction(system: &Option<SystemPrompt>, _model_name: &str) -> Option<Value> {
    let mut parts = Vec::new();

    // [NEW] Antigravity 身份指令 (原始简化版)
    let antigravity_identity = "You are Antigravity, a powerful agentic AI coding assistant designed by the Google Deepmind team working on Advanced Agentic Coding.\n\
    You are pair programming with a USER to solve their coding task. The task may require creating a new codebase, modifying or debugging an existing codebase, or simply answering a question.\n\
    **Absolute paths only**\n\
    **Proactiveness**";
    
    // [HYBRID] 检查用户是否已提供 Antigravity 身份
    let mut user_has_antigravity = false;
    if let Some(sys) = system {
        match sys {
            SystemPrompt::String(text) => {
                if text.contains("You are Antigravity") {
                    user_has_antigravity = true;
                }
            }
            SystemPrompt::Array(blocks) => {
                for block in blocks {
                    if block.block_type == "text" && block.text.contains("You are Antigravity") {
                        user_has_antigravity = true;
                        break;
                    }
                }
            }
        }
    }

    // 如果用户没有提供 Antigravity 身份,则注入
    if !user_has_antigravity {
        parts.push(json!({"text": antigravity_identity}));
    }

    // 添加用户的系统提示词
    if let Some(sys) = system {
        match sys {
            SystemPrompt::String(text) => {
                parts.push(json!({"text": text}));
            }
            SystemPrompt::Array(blocks) => {
                for block in blocks {
                    if block.block_type == "text" {
                        parts.push(json!({"text": block.text}));
                    }
                }
            }
        }
    }

    // 如果用户没有提供任何系统提示词,添加结束标记
    if !user_has_antigravity {
        parts.push(json!({"text": "\n--- [SYSTEM_PROMPT_END] ---"}));
    }

    Some(json!({
        "role": "user",
        "parts": parts
    }))
}

/// 构建 Contents (Messages)
fn build_contents(
    messages: &[Message],
    tool_id_to_name: &mut HashMap<String, String>,
    is_thinking_enabled: bool,
    allow_dummy_thought: bool,
    mapped_model: &str,
) -> Result<Value, String> {
    let mut contents = Vec::new();
    let mut last_thought_signature: Option<String> = None;

    let _msg_count = messages.len();
    for (_i, msg) in messages.iter().enumerate() {
        let role = if msg.role == "assistant" {
            "model"
        } else {
            &msg.role
        };

        let mut parts = Vec::new();

        match &msg.content {
            MessageContent::String(text) => {
                if text != "(no content)" {
                    if !text.trim().is_empty() {
                        parts.push(json!({"text": text.trim()}));
                    }
                }
            }
            MessageContent::Array(blocks) => {
                for item in blocks {
                    match item {
                        ContentBlock::Text { text } => {
                            if text != "(no content)" {
                                parts.push(json!({"text": text}));
                            }
                        }
                        ContentBlock::Thinking { thinking, signature, .. } => {
                            tracing::debug!("[DEBUG-TRANSFORM] Processing thinking block. Sig: {:?}", signature);
                            
                            // [HOTFIX] Gemini Protocol Enforcement: Thinking block MUST be the first block.
                            // If we already have content (like Text), we must downgrade this thinking block to Text.
                            if !parts.is_empty() {
                                tracing::warn!("[Claude-Request] Thinking block found at non-zero index (prev parts: {}). Downgrading to Text.", parts.len());
                                if !thinking.is_empty() {
                                    parts.push(json!({
                                        "text": thinking
                                    }));
                                }
                                continue;
                            }
                            
                            // [FIX] If thinking is disabled (smart downgrade), convert ALL thinking blocks to text
                            // to avoid "thinking is disabled but message contains thinking" error
                            if !is_thinking_enabled {
                                tracing::warn!("[Claude-Request] Thinking disabled. Downgrading thinking block to text.");
                                if !thinking.is_empty() {
                                    parts.push(json!({
                                        "text": thinking
                                    }));
                                }
                                continue;
                            }
                            
                            // [FIX] Empty thinking blocks cause "Field required" errors.
                            // We downgrade them to Text to avoid structural errors and signature mismatch.
                            if thinking.is_empty() {
                                tracing::warn!("[Claude-Request] Empty thinking block detected. Downgrading to Text.");
                                parts.push(json!({
                                    "text": "..."
                                }));
                                continue;
                            }

                            let mut part = json!({
                                "text": thinking,
                                "thought": true, // [CRITICAL FIX] Vertex AI v1internal requires thought: true to distinguish from text
                            });
                            // [New] 递归清理黑名单字段（如 cache_control）
                            crate::proxy::common::json_schema::clean_json_schema(&mut part);

                            // [CRITICAL FIX] Do NOT add skip_thought_signature_validator for Vertex AI
                            // If no signature, the block should have been filtered out
                            if signature.is_none() {
                                tracing::warn!("[Claude-Request] Thinking block without signature (should have been filtered!)");
                            }

                            if let Some(sig) = signature {
                                // [NEW] Cross-Model Compatibility Check
                                // Verify if the signature belongs to a compatible model family
                                let cached_family = crate::proxy::SignatureCache::global().get_signature_family(sig);
                                if let Some(family) = cached_family {
                                    if !is_model_compatible(&family, &mapped_model) {
                                        tracing::warn!(
                                            "[Thinking-Compatibility] Incompatible signature detected (Family: {}, Target: {}). Dropping signature.",
                                            family, mapped_model
                                        );
                                         parts.push(json!({
                                            "text": thinking
                                        }));
                                        continue;
                                    }
                                }

                                last_thought_signature = Some(sig.clone());
                                part["thoughtSignature"] = json!(sig);
                            }
                            parts.push(part);
                        }
                        ContentBlock::RedactedThinking { data } => {
                            // [FIX] 将 RedactedThinking 作为普通文本处理，保留上下文
                            tracing::debug!("[Claude-Request] Degrade RedactedThinking to text");
                            parts.push(json!({
                                "text": format!("[Redacted Thinking: {}]", data)
                            }));
                            continue;
                        }
                        ContentBlock::Image { source, .. } => {
                            if source.source_type == "base64" {
                                parts.push(json!({
                                    "inlineData": {
                                        "mimeType": source.media_type,
                                        "data": source.data
                                    }
                                }));
                            }
                        }
                        ContentBlock::Document { source, .. } => {
                            if source.source_type == "base64" {
                                parts.push(json!({
                                    "inlineData": {
                                        "mimeType": source.media_type,
                                        "data": source.data
                                    }
                                }));
                            }
                        }
                        ContentBlock::ToolUse { id, name, input, signature, .. } => {
                            let mut part = json!({
                                "functionCall": {
                                    "name": name,
                                    "args": input,
                                    "id": id
                                }
                            });
                            
                            // [New] 递归清理参数中可能存在的非法校验字段
                            crate::proxy::common::json_schema::clean_json_schema(&mut part);

                            // 存储 id -> name 映射
                            tool_id_to_name.insert(id.clone(), name.clone());

                            // Signature resolution logic (Priority: Client -> Context -> Cache -> Global Store)
                            // [CRITICAL FIX] Do NOT use skip_thought_signature_validator for Vertex AI
                            // Vertex AI rejects this sentinel value, so we only add thoughtSignature if we have a real one
                            let final_sig = signature.as_ref()
                                .or(last_thought_signature.as_ref())
                                .cloned()
                                .or_else(|| {
                                    // [NEW] Try layer 1 cache (Tool ID -> Signature)
                                    crate::proxy::SignatureCache::global().get_tool_signature(id)
                                        .map(|s| {
                                            tracing::info!("[Claude-Request] Recovered signature from cache for tool_id: {}", id);
                                            s
                                        })
                                })
                                .or_else(|| {
                                    let global_sig = get_thought_signature();
                                    if global_sig.is_some() {
                                        tracing::info!("[Claude-Request] Using global thought_signature fallback (length: {})", 
                                            global_sig.as_ref().unwrap().len());
                                    }
                                    global_sig
                                });
                            // Only add thoughtSignature if we have a valid one
                            // Do NOT add skip_thought_signature_validator - Vertex AI rejects it

                            if let Some(sig) = final_sig {
                                part["thoughtSignature"] = json!(sig);
                            }
                            parts.push(part);
                        }
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                            ..
                        } => {
                            // 优先使用之前记录的 name，否则用 tool_use_id
                            let func_name = tool_id_to_name
                                .get(tool_use_id)
                                .cloned()
                                .unwrap_or_else(|| tool_use_id.clone());

                            // 处理 content：可能是一个内容块数组或单字符串
                            let mut merged_content = match content {
                                serde_json::Value::String(s) => s.clone(),
                                serde_json::Value::Array(arr) => arr
                                    .iter()
                                    .filter_map(|block| {
                                        if let Some(text) =
                                            block.get("text").and_then(|v| v.as_str())
                                        {
                                            Some(text)
                                        } else {
                                            None
                                        }
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n"),
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

                            let mut part = json!({
                                "functionResponse": {
                                    "name": func_name,
                                    "response": {"result": merged_content},
                                    "id": tool_use_id
                                }
                            });

                            // [修复] Tool Result 也需要回填签名（如果上下文中有）
                            if let Some(sig) = last_thought_signature.as_ref() {
                                part["thoughtSignature"] = json!(sig);
                            }

                            parts.push(part);
                        }
                        // ContentBlock::RedactedThinking handled above at line 583
                        ContentBlock::ServerToolUse { .. } | ContentBlock::WebSearchToolResult { .. } => {
                            // 搜索结果 block 不应由客户端发回给上游 (已由 tool_result 替代)
                            continue;
                        }
                    }
                }
            }
        }

        // Fix for "Thinking enabled, assistant message must start with thinking block" 400 error
        // [Optimization] Apply this to ALL assistant messages in history, not just the last one.
        // Vertex AI requires every assistant message to start with a thinking block when thinking is enabled.
        if allow_dummy_thought && role == "model" && is_thinking_enabled {
            let has_thought_part = parts
                .iter()
                .any(|p| {
                    p.get("thought").and_then(|v| v.as_bool()).unwrap_or(false)
                        || p.get("thoughtSignature").is_some()
                        || p.get("thought").and_then(|v| v.as_str()).is_some() // 某些情况下可能是 text + thought: true 的组合
                });

            if !has_thought_part {
                // Prepend a dummy thinking block to satisfy Gemini v1internal requirements
                parts.insert(
                    0,
                    json!({
                        "text": "Thinking...",
                        "thought": true
                    }),
                );
                tracing::debug!("Injected dummy thought block for historical assistant message at index {}", contents.len());
            } else {
                // [Crucial Check] 即使有 thought 块，也必须保证它位于 parts 的首位 (Index 0)
                // 且必须包含 thought: true 标记
                let first_is_thought = parts.get(0).map_or(false, |p| {
                    (p.get("thought").is_some() || p.get("thoughtSignature").is_some())
                    && p.get("text").is_some() // 对于 v1internal，通常 text + thought: true 才是合规的思维块
                });

                if !first_is_thought {
                    // 如果首项不符合思维块特征，强制补入一个
                    parts.insert(
                        0,
                        json!({
                            "text": "...",
                            "thought": true
                        }),
                    );
                    tracing::debug!("First part of model message at {} is not a valid thought block. Prepending dummy.", contents.len());
                } else {
                    // 确保首项包含了 thought: true (防止只有 signature 的情况)
                    if let Some(p0) = parts.get_mut(0) {
                        if p0.get("thought").is_none() {
                             p0.as_object_mut().map(|obj| obj.insert("thought".to_string(), json!(true)));
                        }
                    }
                }
            }
        }

        if parts.is_empty() {
            continue;
        }

        contents.push(json!({
            "role": role,
            "parts": parts
        }));
    }



    // [Removed] ensure_last_assistant_has_thinking 
    // Corrupted signature issues proved we cannot fake thinking blocks.
    // Instead we rely on should_disable_thinking_due_to_history to prevent this state.

    // [FIX P3-3] Strict Role Alternation (Message Merging)
    // Merge adjacent messages with the same role to satisfy Gemini's strict alternation rule
    let mut merged_contents = merge_adjacent_roles(contents);

    // [FIX P3-5] Orphaned Tool Result Cleanup
    // Remove tool_result blocks that reference non-existent tool_use_id
    // This prevents "unexpected tool_use_id found in tool_result blocks" errors
    merged_contents = remove_orphaned_tool_results(merged_contents);

    // [FIX P3-6] Function Call Order Validation
    // Validate strict ordering: user → functionCall → functionResponse → functionCall
    if let Err(e) = validate_function_call_order(&merged_contents) {
        tracing::error!("[Function-Call-Order] {}", e);
        // Don't fail completely - try to recover by returning partially cleaned content
        // The upstream API will provide better error messages
    }

    // [FIX P3-4] Deep "Un-thinking" Cleanup
    // If thinking is disabled (e.g. smart downgrade), recursively remove any stray 'thought'/'thoughtSignature'
    // This is critical because converting Thinking->Text isn't enough; metadata must be gone.
    if !is_thinking_enabled {
        for msg in &mut merged_contents {
            clean_thinking_fields_recursive(msg);
        }
    }

    // [FIX P3-7] Ensure strict role alternation
    // After all processing, verify that roles properly alternate
    // If we detect consecutive same roles with function interactions, split them
    let final_contents = enforce_strict_role_alternation(merged_contents);

    Ok(json!(final_contents))
}

/// Enforce strict role alternation (user ↔ model)
/// If consecutive messages have same role and contain function interactions,
/// split them to maintain proper ordering
fn enforce_strict_role_alternation(contents: Vec<Value>) -> Vec<Value> {
    let mut result: Vec<Value> = Vec::new();

    for msg in contents {
        if let Some(last) = result.last() {
            let last_role = last.get("role").and_then(|r| r.as_str()).unwrap_or("");
            let current_role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");

            if last_role == current_role {
                let current_has_function = contains_function_interaction(&msg);
                let last_has_function = contains_function_interaction(last);

                if current_has_function || last_has_function {
                    // Split: can't merge these due to function interactions
                    result.push(msg);
                    continue;
                }
            }
        }
        result.push(msg);
    }

    result
}

/// Check if a message contains function calls or responses
fn contains_function_interaction(msg: &Value) -> bool {
    if let Some(parts) = msg.get("parts").and_then(|p| p.as_array()) {
        for part in parts {
            if part.get("functionCall").is_some() || part.get("functionResponse").is_some() {
                return true;
            }
        }
    }
    false
}

/// Remove orphaned tool_result blocks (tool results with no matching tool_use)
/// This prevents "unexpected tool_use_id found in tool_result blocks" errors
fn remove_orphaned_tool_results(mut contents: Vec<Value>) -> Vec<Value> {
    // First pass: collect all valid tool_use_ids
    let mut valid_tool_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for msg in &contents {
        if let Some(parts) = msg.get("parts").and_then(|p| p.as_array()) {
            for part in parts {
                if let Some(function_call) = part.get("functionCall") {
                    if let Some(id) = function_call.get("id").and_then(|v| v.as_str()) {
                        valid_tool_ids.insert(id.to_string());
                        tracing::debug!("[Tool-Result-Cleanup] Found valid tool_use_id: {}", id);
                    }
                }
            }
        }
    }

    // Second pass: remove tool_result blocks with invalid tool_use_id
    let mut cleaned_contents = Vec::new();
    let mut removed_count = 0;

    for msg in contents {
        let mut msg = msg;
        if let Some(parts) = msg.get_mut("parts").and_then(|p| p.as_array_mut()) {
            let original_len = parts.len();
            parts.retain(|part| {
                if let Some(function_response) = part.get("functionResponse") {
                    if let Some(id) = function_response.get("id").and_then(|v| v.as_str()) {
                        if !valid_tool_ids.contains(id) {
                            tracing::warn!(
                                "[Tool-Result-Cleanup] Removing orphaned tool_result for unknown tool_use_id: {}",
                                id
                            );
                            removed_count += 1;
                            return false;
                        }
                    }
                }
                true
            });

            if original_len != parts.len() {
                tracing::info!(
                    "[Tool-Result-Cleanup] Cleaned {} orphaned tool_result(s) from message with role {}",
                    original_len - parts.len(),
                    msg["role"].as_str().unwrap_or("unknown")
                );
            }
        }

        // Only keep messages that still have parts
        if let Some(parts) = msg.get("parts").and_then(|p| p.as_array()) {
            if !parts.is_empty() {
                cleaned_contents.push(msg);
            }
        }
    }

    if removed_count > 0 {
        tracing::warn!(
            "[Tool-Result-Cleanup] Total orphaned tool results removed: {}",
            removed_count
        );
    }

    cleaned_contents
}

/// Validate and enforce proper message ordering for function calls
/// Gemini API requires: user → functionCall → functionResponse → functionCall
fn validate_function_call_order(contents: &[Value]) -> Result<(), String> {
    for (i, msg) in contents.iter().enumerate() {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("unknown");
        let has_function_call = msg.get("parts")
            .and_then(|p| p.as_array())
            .map(|parts| parts.iter().any(|p| p.get("functionCall").is_some()))
            .unwrap_or(false);

        if has_function_call {
            // Check previous message
            if i > 0 {
                let prev_role = contents[i - 1].get("role")
                    .and_then(|r| r.as_str())
                    .unwrap_or("unknown");

                let prev_has_function_response = contents[i - 1].get("parts")
                    .and_then(|p| p.as_array())
                    .map(|parts| parts.iter().any(|p| p.get("functionResponse").is_some()))
                    .unwrap_or(false);

                // Function calls must follow user or functionResponse
                if prev_role != "user" && !prev_has_function_response {
                    return Err(format!(
                        "Invalid function call order at index {}: functionCall in role '{}' must follow 'user' or 'functionResponse', found previous role '{}'",
                        i, role, prev_role
                    ));
                }
            } else {
                // First message is a functionCall - invalid
                return Err(format!(
                    "Invalid function call order at index 0: functionCall cannot be first message, must start with 'user'"
                ));
            }
        }
    }
    Ok(())
}

/// Merge adjacent messages with the same role
/// IMPORTANT: Never merge messages containing functionCall or functionResponse
/// as this violates Gemini's strict turn alternation rule
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
            // [CRITICAL FIX] Do NOT merge if either message contains function calls/responses
            // This preserves strict turn order: user -> functionCall -> functionResponse -> functionCall
            if contains_function_interaction(&current_msg) || contains_function_interaction(&msg) {
                tracing::debug!("[Role-Merge] Skipping merge due to function interaction in role {}", current_role);
                merged.push(current_msg);
                current_msg = msg;
            } else {
                // Safe to merge - no function interactions
                if let Some(current_parts) = current_msg.get_mut("parts").and_then(|p| p.as_array_mut()) {
                    if let Some(next_parts) = msg.get("parts").and_then(|p| p.as_array()) {
                        current_parts.extend(next_parts.clone());
                    }
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
fn build_tools(tools: &Option<Vec<Tool>>, has_web_search: bool) -> Result<Option<Value>, String> {
    if let Some(tools_list) = tools {
        let mut function_declarations: Vec<Value> = Vec::new();
        let mut has_google_search = has_web_search;

        for tool in tools_list {
            // 1. Detect server tools / built-in tools like web_search
            if tool.is_web_search() {
                has_google_search = true;
                continue;
            }

            if let Some(t_type) = &tool.type_ {
                if t_type == "web_search_20250305" {
                    has_google_search = true;
                    continue;
                }
            }

            // 2. Detect by name
            if let Some(name) = &tool.name {
                if name == "web_search" || name == "google_search" {
                    has_google_search = true;
                    continue;
                }

                // 3. Client tools require input_schema
                let mut input_schema = tool.input_schema.clone().unwrap_or(json!({
                    "type": "object",
                    "properties": {}
                }));
                crate::proxy::common::json_schema::clean_json_schema(&mut input_schema);

                function_declarations.push(json!({
                    "name": name,
                    "description": tool.description,
                    "parameters": input_schema
                }));
            }
        }

        let mut tool_obj = serde_json::Map::new();

        // [修复] 解决 "Multiple tools are supported only when they are all search tools" 400 错误
        // 原理：Gemini v1internal 接口非常挑剔，通常不允许在同一个工具定义中混用 Google Search 和 Function Declarations。
        // 对于 Claude CLI 等携带 MCP 工具的客户端，必须优先保证 Function Declarations 正常工作。
        if !function_declarations.is_empty() {
            // 如果有本地工具，则只使用本地工具，放弃注入的 Google Search
            tool_obj.insert("functionDeclarations".to_string(), json!(function_declarations));

            // [IMPROVED] 记录跳过 googleSearch 注入的原因
            if has_google_search {
                tracing::info!(
                    "[Claude-Request] Skipping googleSearch injection due to {} existing function declarations. \
                     Gemini v1internal does not support mixed tool types.",
                    function_declarations.len()
                );
            }
        } else if has_google_search {
            // 只有在没有本地工具时，才允许注入 Google Search
            tool_obj.insert("googleSearch".to_string(), json!({}));
        }

        if !tool_obj.is_empty() {
            return Ok(Some(json!([tool_obj])));
        }
    }

    Ok(None)
}

/// 构建 Generation Config
fn build_generation_config(
    claude_req: &ClaudeRequest,
    has_web_search: bool,
    is_thinking_enabled: bool,
    mapped_model: &str
) -> Value {
    let mut config = json!({});

    // Thinking 配置
    if let Some(thinking) = &claude_req.thinking {
        // [New Check] 必须 is_thinking_enabled 为真才生成 thinkingConfig
        if thinking.type_ == "enabled" && is_thinking_enabled {
            let mut thinking_config = json!({"includeThoughts": true});

            if let Some(budget_tokens) = thinking.budget_tokens {
                let mut budget = budget_tokens;

                // [CRITICAL FIX] Apply model-specific thinking budget limits
                // Different models have different maximum thinking budgets
                if has_web_search || mapped_model.contains("gemini-2.5-flash") {
                    // Gemini 2.5 Flash: max 24576
                    budget = budget.min(24576);
                } else if mapped_model.contains("claude") {
                    // Claude models (Sonnet, Opus): max 32000
                    budget = budget.min(32000);
                } else if mapped_model.contains("gemini") {
                    // Other Gemini models: max 32000
                    budget = budget.min(32000);
                }

                thinking_config["thinkingBudget"] = json!(budget);
            }

            config["thinkingConfig"] = thinking_config;
        }
    }

    // 其他参数
    if let Some(temp) = claude_req.temperature {
        config["temperature"] = json!(temp);
    }
    if let Some(top_p) = claude_req.top_p {
        config["topP"] = json!(top_p);
    }
    if let Some(top_k) = claude_req.top_k {
        config["topK"] = json!(top_k);
    }

    // Effort level mapping (Claude API v2.0.67+)
    // Maps Claude's output_config.effort to temperature parameter
    // [FIX] Google Cloud Code API doesn't support effortLevel parameter
    // Instead, map effort to temperature to control output quality:
    //   - high effort → low temperature (0.3) = more precise, detailed
    //   - medium effort → balanced temperature (0.7)
    //   - low effort → high temperature (1.0) = faster, less detailed
    if let Some(output_config) = &claude_req.output_config {
        if let Some(effort) = &output_config.effort {
            // Only apply if user hasn't explicitly set temperature
            if claude_req.temperature.is_none() {
                let temperature = match effort.to_lowercase().as_str() {
                    "high" => 0.3,
                    "medium" => 0.7,
                    "low" => 1.0,
                    _ => 0.7 // Default to balanced
                };
                config["temperature"] = json!(temperature);
                tracing::debug!(
                    "[Generation-Config] Effort level mapped to temperature: {} -> {}",
                    effort,
                    temperature
                );
            } else {
                tracing::debug!(
                    "[Generation-Config] User-specified temperature takes precedence over effort level"
                );
            }
        }
    }

    // web_search 强制 candidateCount=1
    /*if has_web_search {
        config["candidateCount"] = json!(1);
    }*/

    // max_tokens 映射为 maxOutputTokens
    // [FIX] Respect claude_req.max_tokens and ensure max_tokens > budget_tokens
    let max_output_tokens = if let Some(max_tokens) = claude_req.max_tokens {
        // Ensure max_tokens > thinking.budget_tokens (strict inequality required by Claude API)
        if let Some(thinking) = &claude_req.thinking {
            if let Some(budget) = thinking.budget_tokens {
                // [CRITICAL] Use same clamping logic as thinkingConfig
                let clamped_budget = if has_web_search || mapped_model.contains("gemini-2.5-flash") {
                    budget.min(24576)
                } else if mapped_model.contains("claude") {
                    budget.min(32000)
                } else if mapped_model.contains("gemini") {
                    budget.min(32000)
                } else {
                    budget
                };

                // Claude API requirement: max_tokens must be STRICTLY greater than budget_tokens
                if max_tokens <= clamped_budget {
                    let adjusted = clamped_budget + 100;
                    tracing::warn!(
                        "[Generation-Config] max_tokens ({}) <= budget_tokens ({}). Auto-adjusting to {}",
                        max_tokens, clamped_budget, adjusted
                    );
                    adjusted
                } else {
                    max_tokens
                }
            } else {
                max_tokens
            }
        } else {
            max_tokens
        }
    } else {
        // Fallback to default if client doesn't specify
        64000
    };

    config["maxOutputTokens"] = json!(max_output_tokens);

    // [优化] 设置全局停止序列,防止流式输出冗余
    config["stopSequences"] = json!([
        "<|user|>",
        "<|endoftext|>",
        "<|end_of_turn|>",
        "[DONE]",
        "\n\nHuman:"
    ]);

    config
}

/// Recursively remove 'thought' and 'thoughtSignature' fields
/// Used when downgrading thinking (e.g. during 400 retry)
pub fn clean_thinking_fields_recursive(val: &mut Value) {
    match val {
        Value::Object(map) => {
            map.remove("thought");
            map.remove("thoughtSignature");
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


/// Check if two model strings are compatible (same family)
fn is_model_compatible(cached: &str, target: &str) -> bool {
    // Simple heuristic: check if they share the same base prefix
    // e.g. "gemini-1.5-pro" vs "gemini-1.5-pro-002" -> Compatible
    // "gemini-1.5-pro" vs "gemini-2.0-flash" -> Incompatible
    
    // Normalize
    let c = cached.to_lowercase();
    let t = target.to_lowercase();
    
    if c == t { return true; }
    
    // Check specific families
    if c.contains("gemini-1.5") && t.contains("gemini-1.5") { return true; }
    if c.contains("gemini-2.0") && t.contains("gemini-2.0") { return true; }
    if c.contains("claude-3-5") && t.contains("claude-3-5") { return true; }
    if c.contains("claude-3-7") && t.contains("claude-3-7") { return true; }
    
    // Fallback: strict match required
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::common::json_schema::clean_json_schema;

    #[test]
    fn test_simple_request() {
        let req = ClaudeRequest {
            model: "claude-sonnet-4-5".to_string(),
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
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let body = result.unwrap();
        assert_eq!(body["project"], "test-project");
        assert!(body["requestId"].as_str().unwrap().starts_with("agent-"));
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
                    content: MessageContent::Array(vec![
                        ContentBlock::ToolUse {
                            id: "call_1".to_string(),
                            name: "run_command".to_string(),
                            input: json!({"command": "ls"}),
                            signature: None,
                            cache_control: None,
                        }
                    ]),
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
        };

        let result = transform_claude_request_in(&req, "test-project");
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
        let resp_text = func_resp["response"]["result"].as_str().unwrap();
        assert!(resp_text.contains("file1.txt"));
        assert!(resp_text.contains("file2.txt"));
        assert!(resp_text.contains("\n"));
    }

    #[test]
    fn test_cache_control_cleanup() {
        // 模拟 VS Code 插件发送的包含 cache_control 的历史消息
        let req = ClaudeRequest {
            model: "claude-sonnet-4-5".to_string(),
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
                    content: MessageContent::Array(vec![
                        ContentBlock::Image {
                            source: ImageSource {
                                source_type: "base64".to_string(),
                                media_type: "image/png".to_string(),
                                data: "iVBORw0KGgo=".to_string(),
                            },
                            cache_control: Some(json!({"type": "ephemeral"})), // 这个也应该被清理
                        },
                    ]),
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
        };

        let result = transform_claude_request_in(&req, "test-project");
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
        // 期望: 系统自动降级，禁用 Thinking 模式，以避免 400 错误
        let req = ClaudeRequest {
            model: "claude-sonnet-4-5".to_string(),
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
                            signature: None 
                        },
                    ]),
                },
                // 用户返回工具结果
                Message {
                    role: "user".to_string(),
                    content: MessageContent::Array(vec![
                        ContentBlock::ToolResult {
                            tool_use_id: "tool_1".to_string(),
                            content: serde_json::Value::String("file1.txt\nfile2.txt".to_string()),
                            is_error: Some(false),
                            // cache_control: None, // removed
                        },
                    ]),
                },
            ],
            system: None,
            tools: Some(vec![
                Tool {
                    name: Some("list_files".to_string()),
                    description: Some("List files".to_string()),
                    input_schema: Some(json!({"type": "object"})),
                    type_: None,
                    // cache_control: None, // removed
                }
            ]),
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(1024),
            }),
            metadata: None,
            output_config: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let body = result.unwrap();
        let request = &body["request"];

        // 验证: generationConfig 中不应包含 thinkingConfig (因为被降级了)
        // 即使请求中明确启用了 thinking
        if let Some(gen_config) = request.get("generationConfig") {
             assert!(gen_config.get("thinkingConfig").is_none(), "thinkingConfig should be removed due to downgrade");
        }
        
        // 验证: 依然能生成有效的请求体
        assert!(request.get("contents").is_some());
    }



    #[test]
    fn test_thinking_block_not_prepend_when_disabled() {
        // 验证当 thinking 未启用时,不会补全 thinking 块
        let req = ClaudeRequest {
            model: "claude-sonnet-4-5".to_string(),
            messages: vec![
                Message {
                    role: "user".to_string(),
                    content: MessageContent::String("Hello".to_string()),
                },
                Message {
                    role: "assistant".to_string(),
                    content: MessageContent::Array(vec![
                        ContentBlock::Text {
                            text: "Response".to_string(),
                        },
                    ]),
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
        };

        let result = transform_claude_request_in(&req, "test-project");
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
            model: "claude-sonnet-4-5".to_string(),
            messages: vec![
                Message {
                    role: "assistant".to_string(),
                    content: MessageContent::Array(vec![
                        ContentBlock::Thinking {
                            thinking: "".to_string(), // 空内容
                            signature: Some("sig".to_string()),
                            cache_control: None,
                        },
                        ContentBlock::Text { text: "Hi".to_string() }
                    ]),
                },
            ],
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
            }),
            metadata: None,
            output_config: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok(), "Transformation failed");
        let body = result.unwrap();
        let contents = body["request"]["contents"].as_array().unwrap();
        let parts = contents[0]["parts"].as_array().unwrap();
        
        // 验证 thinking 块
        assert_eq!(parts[0]["text"], "...", "Empty thinking should be filled with ...");
        assert!(parts[0].get("thought").is_none(), "Empty thinking should be downgraded to text");
    }

    #[test]
    fn test_redacted_thinking_degradation() {
        // [场景] 客户端包含 RedactedThinking
        // 期望: 降级为普通文本，不带 thought: true
        let req = ClaudeRequest {
            model: "claude-sonnet-4-5".to_string(),
            messages: vec![
                Message {
                    role: "assistant".to_string(),
                    content: MessageContent::Array(vec![
                        ContentBlock::RedactedThinking {
                            data: "some data".to_string(),
                        },
                         ContentBlock::Text { text: "Hi".to_string() }
                    ]),
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
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());
        let body = result.unwrap();
        let parts = body["request"]["contents"][0]["parts"].as_array().unwrap();

        // 验证 RedactedThinking -> Text
        let text = parts[0]["text"].as_str().unwrap();
        assert!(text.contains("[Redacted Thinking: some data]"));
        assert!(parts[0].get("thought").is_none(), "Redacted thinking should NOT have thought: true");
    }
}
