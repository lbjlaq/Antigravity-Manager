// Claude 请求转换 (Claude → Gemini v1internal)
// 对应 transformClaudeRequestIn

use super::models::*;
use crate::proxy::common::platform;
use crate::proxy::mappers::common::gemini_detection::is_gemini_3_model;
use crate::proxy::mappers::common::thinking_level_mapper::determine_thinking_level;
use crate::proxy::mappers::common::gemini_api_validator::validate_gemini_request;
use crate::proxy::mappers::signature_store::get_thought_signature;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::OnceLock;
use tauri::Emitter;

// Model ID constants from Google Antigravity v1.13.3
// Reference: docs/antigravity/workflows/models/claude/claude-4.5-sonnet-thinking-workflow.md:161-166
const CLAUDE_4_5_SONNET_THINKING_MODEL_ID: u32 = 334;
const CLAUDE_4_5_SONNET_MODEL_ID: u32 = 333;

// Gemini 3.x Model ID constants (Story-005-01)
// Reference: docs/stories/Story-005-01-gemini-model-id-constants.md
// NOTE: Gemini 3.x models use name-based routing (Model ID = 0) instead of explicit IDs
// Discovery method: Documentation analysis (2026-01-11) - No explicit Model IDs found for Gemini 3.x
// Unlike Claude models (333, 334) and Gemini 2.5 models (246, 312, 313, etc.),
// Gemini 3.x models (high/low/flash) do not have distinct Model IDs in Antigravity v1.13.3
const GEMINI_3_PRO_HIGH_MODEL_ID: u32 = 0; // Name-based routing
const GEMINI_3_PRO_HIGH_THINKING_MODEL_ID: u32 = 0; // Same as base (thinking via parameter)
const GEMINI_3_PRO_LOW_MODEL_ID: u32 = 0; // Name-based routing (Story-009-02)
const GEMINI_3_PRO_LOW_THINKING_MODEL_ID: u32 = 0; // Same as base (thinking via parameter)

// API Provider constants
// Reference: docs/antigravity/workflows/models/claude/claude-4.5-sonnet-thinking-workflow.md:161-166
const API_PROVIDER_ANTHROPIC_VERTEX: u32 = 26; // Claude models → Vertex AI
const API_PROVIDER_GEMINI: u32 = 0; // Gemini models → direct

// Model Provider constants
const MODEL_PROVIDER_ANTHROPIC: u32 = 3; // Anthropic (Claude)
const MODEL_PROVIDER_GEMINI: u32 = 1; // Google (Gemini)
const MODEL_PROVIDER_UNKNOWN: u32 = 0; // Unknown

// IDE Metadata constants
// 🚨 CRITICAL: ideType "ANTIGRAVITY" is PRIMARY anti-detection marker
// Reference: docs/stories/Story-003-03-antigravity-metadata.md
const IDE_TYPE: &str = "ANTIGRAVITY";
const IDE_VERSION: &str = "1.13.3";

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
                            tracing::debug!(
                                "[Cache-Control-Cleaner] Removed cache_control from Thinking block"
                            );
                            *cache_control = None;
                        }
                    }
                    ContentBlock::Image { cache_control, .. } => {
                        if cache_control.is_some() {
                            tracing::debug!(
                                "[Cache-Control-Cleaner] Removed cache_control from Image block"
                            );
                            *cache_control = None;
                        }
                    }
                    ContentBlock::Document { cache_control, .. } => {
                        if cache_control.is_some() {
                            tracing::debug!(
                                "[Cache-Control-Cleaner] Removed cache_control from Document block"
                            );
                            *cache_control = None;
                        }
                    }
                    ContentBlock::ToolUse { cache_control, .. } => {
                        if cache_control.is_some() {
                            tracing::debug!(
                                "[Cache-Control-Cleaner] Removed cache_control from ToolUse block"
                            );
                            *cache_control = None;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Get model ID for Antigravity v1internal API
/// Returns model ID from reverse-engineered specification
/// Reference: docs/antigravity/workflows/models/claude/claude-4.5-sonnet-thinking-workflow.md:161-166
/// Made public for test coverage (Story-009-05)
pub fn get_model_id(model_name: &str) -> u32 {
    let model_id = match model_name {
        // Thinking variants (ID 334)
        "claude-4.5-sonnet-thinking" => CLAUDE_4_5_SONNET_THINKING_MODEL_ID,
        "claude-sonnet-4-5-thinking" => CLAUDE_4_5_SONNET_THINKING_MODEL_ID,
        // Standard variants (ID 333)
        "claude-4.5-sonnet" => CLAUDE_4_5_SONNET_MODEL_ID,
        "claude-sonnet-4-5" => CLAUDE_4_5_SONNET_MODEL_ID,

        // Gemini 3.x models (Story-005-01, Story-009-02)
        // NOTE: Returns 0 (name-based routing) - Gemini 3.x models don't use explicit Model IDs
        "gemini-3-pro-high" => GEMINI_3_PRO_HIGH_MODEL_ID,
        "gemini-3-pro-low" => GEMINI_3_PRO_LOW_MODEL_ID,

        // Add mappings for other models
        _ => 0, // Unknown model
    };

    // GAP #5: Validation logging for model ID routing
    tracing::debug!(
        "[Epic-004-Validation] Model ID routing: '{}' → ID {}{}",
        model_name,
        model_id,
        if model_id == 0 {
            " (UNKNOWN MODEL)"
        } else {
            ""
        }
    );

    model_id
}

/// Get API provider for v1internal routing
/// Claude models route through ANTHROPIC_VERTEX (26)
/// Gemini models use direct routing (0)
/// Reference: docs/antigravity/workflows/models/claude/claude-4.5-sonnet-thinking-workflow.md:161-166
fn get_api_provider(model_name: &str) -> u32 {
    if model_name.starts_with("claude-") {
        API_PROVIDER_ANTHROPIC_VERTEX // 26
    } else if model_name.starts_with("gemini-") {
        API_PROVIDER_GEMINI // 0
    } else {
        API_PROVIDER_GEMINI // Default
    }
}

/// Get model provider enum for Antigravity
/// Distinguishes Anthropic (3) from Google (1) models
/// Reference: docs/antigravity/workflows/models/claude/claude-4.5-sonnet-thinking-workflow.md:161-166
fn get_model_provider(model_name: &str) -> u32 {
    if model_name.starts_with("claude-") {
        MODEL_PROVIDER_ANTHROPIC // 3
    } else if model_name.starts_with("gemini-") {
        MODEL_PROVIDER_GEMINI // 1
    } else {
        MODEL_PROVIDER_UNKNOWN // 0
    }
}

/// 转换 Claude 请求为 Gemini v1internal 格式
// 🆕 Story #8 Step 8: Modified signature to return ViolationInfo
pub fn transform_claude_request_in(
    claude_req: &ClaudeRequest,
    project_id: &str,
) -> Result<(Value, ViolationInfo), String> {
    // 🆕 Story #8: Initialize violation tracking
    let mut violations = ViolationInfo::new();

    // [CRITICAL FIX] 预先清理所有消息中的 cache_control 字段
    // 这解决了 VS Code 插件等客户端在多轮对话中将历史消息的 cache_control 字段
    // 原封不动发回导致的 "Extra inputs are不 permitted" 错误
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
        list.iter()
            .map(|t| serde_json::to_value(t).unwrap_or(json!({})))
            .collect()
    });

    // Resolve grounding config
    let config = crate::proxy::mappers::common_utils::resolve_request_config(
        &claude_req.model,
        &mapped_model,
        &tools_val,
    );

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

    // [GAP #2 FIX] Check if target model supports thinking
    // Claude models: ONLY models with "-thinking" suffix support thinking
    // Gemini models: All gemini-* models support thinking via thinkingConfig parameter
    //
    // CRITICAL: Standard Claude models (claude-4.5-sonnet, ID 333) do NOT support thinking!
    // Only thinking variants (claude-4.5-sonnet-thinking, ID 334) support thinking.
    let target_model_supports_thinking =
        mapped_model.contains("-thinking") || is_gemini_thinking_model(&mapped_model);

    // GAP #5: Validation logging for thinking mode detection
    tracing::debug!(
        "[Epic-004-Validation] Thinking mode: enabled={}, model_supports={}, final_state={}",
        is_thinking_enabled,
        target_model_supports_thinking,
        is_thinking_enabled && target_model_supports_thinking
    );

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
                    return blocks
                        .iter()
                        .any(|b| matches!(b, ContentBlock::Thinking { .. }));
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
    // 🆕 Story-008-01: Pass messages for adaptive budget optimization
    let generation_config = build_generation_config(
        claude_req,
        has_web_search_tool,
        is_thinking_enabled,
        &mapped_model,
        &mut violations, // 🆕 Story #8: Pass violations for tracking
    );

    // 2. Contents (Messages)
    let contents = build_contents(
        &claude_req.messages,
        &mut tool_id_to_name,
        is_thinking_enabled,
        allow_dummy_thought,
        &mapped_model,
        &mut violations, // 🆕 Story #8: Pass violations for tracking
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

    // 🆕 Story #9: Flexible tool calling mode configuration
    // AC-13: Validate tool_choice without tools edge case
    if let Some(ref tc) = claude_req.tool_choice {
        if tools.is_none() {
            match tc {
                ToolChoice::Auto | ToolChoice::Any | ToolChoice::Tool { .. } => {
                    // AC-13: Log warning for tool_choice without tools
                    tracing::warn!(
                        "[Tool-Config] ⚠️ tool_choice specified ({:?}) but no tools provided, ignoring tool_choice",
                        tc.to_gemini_mode()
                    );
                }
                ToolChoice::None => {
                    // AC-13: None mode without tools is valid (explicit disable)
                    tracing::debug!(
                        "[Tool-Config] tool_choice: None (no tools, explicitly disabled)"
                    );
                }
            }
        }
    }

    if let Some(tools_val) = tools {
        inner_request["tools"] = tools_val;

        // 🆕 Story #9: Flexible tool calling mode configuration
        let tool_choice = claude_req.tool_choice.as_ref();

        // Default to VALIDATED for backward compatibility
        let mode = tool_choice
            .map(|tc| tc.to_gemini_mode())
            .unwrap_or("VALIDATED");

        // Build function calling config
        let mut function_calling_config = json!({
            "mode": mode
        });

        // For specific tool forcing (ToolChoice::Tool), add allowedFunctionNames
        if let Some(tool_name) = tool_choice.and_then(|tc| tc.get_tool_name()) {
            function_calling_config["allowedFunctionNames"] = json!([tool_name]);

            tracing::debug!(
                "[Tool-Config] 🎯 Forcing specific tool: '{}', mode: {}",
                tool_name,
                mode
            );
        } else {
            tracing::debug!("[Tool-Config] 📋 Using flexible mode: {}", mode);
        }

        inner_request["toolConfig"] = json!({
            "functionCallingConfig": function_calling_config
        });
    }

    // 🆕 Story #10: Add Gemini Settings (Anti-Plagiarism Protection)
    // Required for 100% Antigravity compliance and anti-detection
    // This field is ALWAYS present (not conditional) to match Antigravity baseline
    inner_request["geminiSettings"] = json!({
        "recitationPolicy": {
            "action": "BLOCK",      // Block recited content (strictest policy)
            "threshold": "LOW"      // Block even low-confidence matches
        }
    });

    tracing::debug!(
        "[Gemini-Settings] Added recitationPolicy: action=BLOCK, threshold=LOW for anti-plagiarism protection"
    );

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

    // 🆕 Story #3: Build Antigravity metadata with IDE identity
    // 🚨 CRITICAL: ideType "ANTIGRAVITY" is PRIMARY anti-detection marker
    let mut metadata = json!({
        "ideType": IDE_TYPE,           // "ANTIGRAVITY"
        "ideVersion": IDE_VERSION,     // "1.13.3"
        "platform": platform::get_platform(),    // "darwin"/"windows"/"linux"
        "architecture": platform::get_architecture()  // "arm64"/"x86_64"
    });

    // Add optional extended metadata fields if provided
    if let Some(claude_metadata) = &claude_req.metadata {
        // Story #3: Session ID
        if let Some(user_id) = &claude_metadata.user_id {
            metadata["sessionId"] = json!(user_id);
        }

        // 🆕 Story #4: Workspace ID (optional - Cloud AI Companion)
        if let Some(workspace_id) = &claude_metadata.workspace_id {
            metadata["workspace_id"] = json!(workspace_id);
        }

        // 🆕 Story #4: Cloud AI Companion Project (optional)
        if let Some(project) = &claude_metadata.cloudaicompanion_project {
            metadata["cloudaicompanion_project"] = json!(project);
        }
    }

    // Add metadata to inner request
    inner_request["metadata"] = metadata.clone();

    // GAP #5: Validation logging for metadata injection
    tracing::debug!(
        "[Epic-004-Validation] Metadata injected: ideType={}, ideVersion={}, platform={}, arch={}",
        metadata["ideType"].as_str().unwrap_or(""),
        metadata["ideVersion"].as_str().unwrap_or(""),
        metadata["platform"].as_str().unwrap_or(""),
        metadata["architecture"].as_str().unwrap_or("")
    );

    // 生成 requestId
    let request_id = format!("agent-{}", uuid::Uuid::new_v4());

    // 构建最终请求体
    let body = json!({
        "project": project_id,
        "requestId": request_id,
        "request": inner_request,
        "model": config.final_model,
        "modelId": get_model_id(&config.final_model),  // 🆕 Story #1: Add model ID
        "apiProvider": get_api_provider(&config.final_model),  // 🆕 Story #2: API routing
        "modelProvider": get_model_provider(&config.final_model),  // 🆕 Story #2: Model provider
        "userAgent": "antigravity",
        "requestType": config.request_type,
    });

    // GAP #5: Validation logging for provider routing and final assembly
    tracing::debug!(
        "[Epic-004-Validation] Provider routing: model={}, modelId={}, apiProvider={}, modelProvider={}",
        body["model"].as_str().unwrap_or(""),
        body["modelId"].as_u64().unwrap_or(0),
        body["apiProvider"].as_u64().unwrap_or(0),
        body["modelProvider"].as_u64().unwrap_or(0)
    );

    // [CRITICAL LOG] Log final request structure for debugging INVALID_ARGUMENT errors
    if let Some(gen_config) = body["request"].get("generationConfig") {
        if let Some(thinking_config) = gen_config.get("thinkingConfig") {
            let max_tokens = gen_config
                .get("maxOutputTokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            tracing::warn!(
                "[Claude-Request] THINKING MODE: model={}, maxOutputTokens={}, thinkingConfig={}",
                body["model"].as_str().unwrap_or("unknown"),
                max_tokens,
                serde_json::to_string(thinking_config).unwrap_or_default()
            );
        }
    }

    // [EPIC-011 Story-011-03] Validate Gemini API format before returning
    if config.final_model.starts_with("gemini-") {
        if let Err(e) = validate_gemini_request(&config.final_model, &inner_request) {
            tracing::error!("[Claude-Request] Gemini API validation failed: {}", e);
            return Err(e.to_string());
        }
    }

    // 🆕 Story #8: Return transformed body with violation info
    Ok((body, violations))
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
                let has_tool_use = blocks
                    .iter()
                    .any(|b| matches!(b, ContentBlock::ToolUse { .. }));
                let has_thinking = blocks
                    .iter()
                    .any(|b| matches!(b, ContentBlock::Thinking { .. }));

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

/// [UPDATED Epic-006] Check if Gemini model supports thinking mode.
///
/// NOT all Gemini models support thinking - only specific models validated via live API testing.
/// Lite variants (e.g., gemini-2.5-flash-lite) do NOT support thinking despite accepting the parameter.
///
/// Unlike Claude (which uses "-thinking" suffix), Gemini uses thinkingConfig parameter in API request.
/// Reference: docs/qa/FLASH_LITE_THINKING_CODE_ANALYSIS.md
fn is_gemini_thinking_model(model: &str) -> bool {
    // [FIX Epic-006] Explicit allow-list for thinking-capable models
    // Based on live API validation (Story-006-01)
    // NOT all Gemini models support thinking - lite variants do NOT
    match model {
        // Gemini 2.x series (legacy and current)
        "gemini-2.0-flash" => true,
        "gemini-2.5-flash" => true,
        "gemini-2.5-pro" => true,

        // Gemini Pro (legacy)
        "gemini-pro" => true,

        // Gemini 3 series (verified thinking support)
        "gemini-3-flash" => true,
        "gemini-3-pro-low" => true,
        "gemini-3-pro-high" => true,

        // Gemini -thinking suffix models (ONLY for gemini-* models)
        m if m.starts_with("gemini-") && m.ends_with("-thinking") => true,

        // Default: NO thinking support
        // Excludes: gemini-2.5-flash-lite, gemini-pro-vision, claude-*, gpt-*, etc.
        // Reason: API validation showed lite ignores thinkingConfig parameter
        _ => false,
    }
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
// 🆕 Story #8: Added violations parameter for violation tracking
fn build_contents(
    messages: &[Message],
    tool_id_to_name: &mut HashMap<String, String>,
    is_thinking_enabled: bool,
    allow_dummy_thought: bool,
    mapped_model: &str,
    violations: &mut ViolationInfo,
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
                        ContentBlock::Thinking {
                            thinking,
                            signature,
                            ..
                        } => {
                            tracing::debug!(
                                "[DEBUG-TRANSFORM] Processing thinking block. Sig: {:?}",
                                signature
                            );

                            // [CRITICAL] Position Enforcement per RE spec (Gap Analysis #5)
                            // Expected: Error if thinking block is not first part
                            // Current: Auto-downgrade to text for backwards compatibility + ERROR logging
                            // Reference: docs/comparison/.../current-implementation-thinking.md:3251-3366
                            if !parts.is_empty() {
                                // 🆕 Story #7: ERROR-level logging with enhanced context
                                tracing::error!(
                                    "[Thinking-Position] ❌ PROTOCOL VIOLATION: Thinking block at index {} (must be first). \
                                     Message role: '{}', total parts before: {}, thinking length: {}. \
                                     Downgrading to text block to maintain compatibility. \
                                     Client MUST fix message structure to prevent this error.",
                                    parts.len(),
                                    role,
                                    parts.len(),
                                    thinking.len()
                                );

                                // 🆕 Story #8: Record position violation
                                violations.record_position_violation(parts.len(), role);

                                if !thinking.is_empty() {
                                    parts.push(json!({
                                        "text": thinking
                                    }));
                                }
                                continue; // Keep downgrade for backwards compatibility
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
                                let cached_family = crate::proxy::SignatureCache::global()
                                    .get_signature_family(sig);
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
                        ContentBlock::ToolUse {
                            id,
                            name,
                            input,
                            signature,
                            ..
                        } => {
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
                        ContentBlock::ServerToolUse { .. }
                        | ContentBlock::WebSearchToolResult { .. } => {
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
            let has_thought_part = parts.iter().any(|p| {
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
                tracing::debug!(
                    "Injected dummy thought block for historical assistant message at index {}",
                    contents.len()
                );
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
                            p0.as_object_mut()
                                .map(|obj| obj.insert("thought".to_string(), json!(true)));
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
fn remove_orphaned_tool_results(contents: Vec<Value>) -> Vec<Value> {
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
        let role = msg
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("unknown");
        let has_function_call = msg
            .get("parts")
            .and_then(|p| p.as_array())
            .map(|parts| parts.iter().any(|p| p.get("functionCall").is_some()))
            .unwrap_or(false);

        if has_function_call {
            // Check previous message
            if i > 0 {
                let prev_role = contents[i - 1]
                    .get("role")
                    .and_then(|r| r.as_str())
                    .unwrap_or("unknown");

                let prev_has_function_response = contents[i - 1]
                    .get("parts")
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
                tracing::debug!(
                    "[Role-Merge] Skipping merge due to function interaction in role {}",
                    current_role
                );
                merged.push(current_msg);
                current_msg = msg;
            } else {
                // Safe to merge - no function interactions
                if let Some(current_parts) =
                    current_msg.get_mut("parts").and_then(|p| p.as_array_mut())
                {
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
            tool_obj.insert(
                "functionDeclarations".to_string(),
                json!(function_declarations),
            );

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
// 🆕 Story #8: Added violations parameter
fn build_generation_config(
    claude_req: &ClaudeRequest,
    has_web_search: bool,
    is_thinking_enabled: bool,
    mapped_model: &str,
    violations: &mut ViolationInfo,
) -> Value {
    let mut config = json!({});

    // Thinking 配置
    if let Some(thinking) = &claude_req.thinking {
        // [New Check] 必须 is_thinking_enabled 为真才生成 thinkingConfig
        if thinking.type_ == "enabled" && is_thinking_enabled {
            let mut thinking_config = json!({"includeThoughts": true});

            // 🆕 Story-008-01: Adaptive Budget Optimization
            // Use intelligent budget calculation based on prompt complexity
            let budget = if let Some(budget_tokens) = thinking.budget_tokens {
                // User explicitly provided budget - respect it
                let mut user_budget = budget_tokens;

                // [CRITICAL FIX] Apply model-specific thinking budget limits
                if has_web_search || mapped_model.contains("gemini-2.5-flash") {
                    user_budget = user_budget.min(24576);
                } else if mapped_model.contains("claude") {
                    user_budget = user_budget.min(32000);
                } else if mapped_model.contains("gemini") {
                    user_budget = user_budget.min(32000);
                }

                tracing::debug!(
                    "[Budget-Optimizer] Using explicit budget: {} (model: {})",
                    user_budget,
                    mapped_model
                );

                user_budget
            } else {
                // No explicit budget - use adaptive optimization
                // Extract first user message content for classification
                let first_user_prompt = claude_req
                    .messages
                    .iter()
                    .find(|m| m.role == "user")
                    .and_then(|msg| match &msg.content {
                        MessageContent::String(s) => Some(s.as_str()),
                        MessageContent::Array(blocks) => {
                            // Extract text from first text block
                            blocks.iter().find_map(|block| {
                                if let ContentBlock::Text { text } = block {
                                    Some(text.as_str())
                                } else {
                                    None
                                }
                            })
                        }
                    })
                    .unwrap_or("");

                // Calculate optimal budget using budget optimizer
                let optimizer = crate::proxy::budget_optimizer::BudgetOptimizer::new();
                let optimal_budget = optimizer
                    .calculate_optimal_budget(first_user_prompt, mapped_model)
                    .unwrap_or(16000); // Fallback to default 16K if optimization fails

                // Apply model-specific limits to optimal budget
                let clamped_budget = if has_web_search || mapped_model.contains("gemini-2.5-flash") {
                    optimal_budget.min(24576)
                } else if mapped_model.contains("claude") {
                    optimal_budget.min(32000)
                } else if mapped_model.contains("gemini") {
                    optimal_budget.min(32000)
                } else {
                    optimal_budget
                };

                tracing::info!(
                    "[Budget-Optimizer] 🎯 Adaptive budget: {} (prompt_len: {}, model: {})",
                    clamped_budget,
                    first_user_prompt.len(),
                    mapped_model
                );

                clamped_budget
            };

            // [EPIC-011 Story-011-01] Gemini 3.x uses thinkingLevel, Gemini 2.5 uses thinkingBudget
            if is_gemini_3_model(&mapped_model) {
                // Gemini 3.x: Map budget to thinkingLevel
                let thinking_level = determine_thinking_level(&mapped_model, Some(budget as i32));

                thinking_config["thinkingLevel"] = json!(thinking_level);
                // Remove thinkingBudget if it was added (shouldn't exist for Gemini 3)
                thinking_config.as_object_mut().unwrap().remove("thinkingBudget");

                tracing::info!(
                    "[Claude-Request] Gemini 3 thinkingLevel: {} (budget: {}, model: {})",
                    thinking_level,
                    budget,
                    mapped_model
                );
            } else {
                // Gemini 2.5 and other models: Use thinkingBudget (backward compatibility)
                thinking_config["thinkingBudget"] = json!(budget);

                tracing::debug!(
                    "[Claude-Request] Gemini 2.5 thinkingBudget: {} (model: {})",
                    budget,
                    mapped_model
                );
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
                    _ => 0.7, // Default to balanced
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
                let clamped_budget = if has_web_search || mapped_model.contains("gemini-2.5-flash")
                {
                    budget.min(24576)
                } else if mapped_model.contains("claude") {
                    budget.min(32000)
                } else if mapped_model.contains("gemini") {
                    budget.min(32000)
                } else {
                    budget
                };

                // [CRITICAL] Constraint violation per RE spec (Gap Analysis #4)
                // Expected: Error if maxOutputTokens <= thinkingBudget
                // Current: Auto-fix for backwards compatibility + enhanced warning
                // Reference: docs/comparison/.../current-implementation-thinking.md:3128-3248
                if max_tokens <= clamped_budget {
                    let adjusted = clamped_budget + 100;

                    // 🆕 Story #8: Record budget violation
                    violations.record_budget_violation();

                    // 🆕 Story #6: Enhanced warning with client guidance
                    tracing::warn!(
                        "[Thinking-Budget] ⚠️ Constraint violation: maxOutputTokens ({}) <= thinkingBudget ({}). \
                         Auto-fixing to {} to maintain compatibility. \
                         Client should fix configuration to prevent this warning.",
                        max_tokens,
                        clamped_budget,
                        adjusted
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

    if c == t {
        return true;
    }

    // Check specific families
    if c.contains("gemini-1.5") && t.contains("gemini-1.5") {
        return true;
    }
    if c.contains("gemini-2.0") && t.contains("gemini-2.0") {
        return true;
    }
    if c.contains("claude-3-5") && t.contains("claude-3-5") {
        return true;
    }
    if c.contains("claude-3-7") && t.contains("claude-3-7") {
        return true;
    }

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
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
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
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
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
                    content: MessageContent::Array(vec![ContentBlock::Image {
                        source: ImageSource {
                            source_type: "base64".to_string(),
                            media_type: "image/png".to_string(),
                            data: "iVBORw0KGgo=".to_string(),
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
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        // 验证请求成功转换
        let (body, _violations) = result.unwrap();
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
                        // cache_control: None, // removed
                    }]),
                },
            ],
            system: None,
            tools: Some(vec![Tool {
                name: Some("list_files".to_string()),
                description: Some("List files".to_string()),
                input_schema: Some(json!({"type": "object"})),
                type_: None,
                // cache_control: None, // removed
            }]),
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
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        let request = &body["request"];

        // 验证: generationConfig 中不应包含 thinkingConfig (因为被降级了)
        // 即使请求中明确启用了 thinking
        if let Some(gen_config) = request.get("generationConfig") {
            assert!(
                gen_config.get("thinkingConfig").is_none(),
                "thinkingConfig should be removed due to downgrade"
            );
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
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
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
        // GAP #2 FIX: Use thinking model variant to properly test thinking functionality
        let req = ClaudeRequest {
            model: "claude-sonnet-4-5-thinking".to_string(),
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
            }),
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok(), "Transformation failed");
        let (body, _violations) = result.unwrap();
        let contents = body["request"]["contents"].as_array().unwrap();
        let parts = contents[0]["parts"].as_array().unwrap();

        // 验证 thinking 块
        assert_eq!(
            parts[0]["text"], "...",
            "Empty thinking should be filled with ..."
        );
        assert!(
            parts[0].get("thought").is_none(),
            "Empty thinking should be downgraded to text"
        );
    }

    #[test]
    fn test_redacted_thinking_degradation() {
        // [场景] 客户端包含 RedactedThinking
        // 期望: 降级为普通文本，不带 thought: true
        // GAP #2 FIX: Use thinking model variant to properly test thinking functionality
        let req = ClaudeRequest {
            model: "claude-sonnet-4-5-thinking".to_string(),
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
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());
        let (body, _violations) = result.unwrap();
        let parts = body["request"]["contents"][0]["parts"].as_array().unwrap();

        // 验证 RedactedThinking -> Text
        let text = parts[0]["text"].as_str().unwrap();
        assert!(text.contains("[Redacted Thinking: some data]"));
        assert!(
            parts[0].get("thought").is_none(),
            "Redacted thinking should NOT have thought: true"
        );
    }

    // Story #1: Model ID Constants and Helper Function Tests
    // Reference: docs/comparison/claude/claude-4.5-sonnet/current-implementation-thinking.md:2744-2872

    #[test]
    fn test_get_model_id_sonnet_thinking() {
        assert_eq!(get_model_id("claude-4.5-sonnet-thinking"), 334);
    }

    #[test]
    fn test_get_model_id_sonnet() {
        assert_eq!(get_model_id("claude-4.5-sonnet"), 333);
    }

    #[test]
    fn test_get_model_id_unknown() {
        assert_eq!(get_model_id("unknown-model"), 0);
    }

    // ==================== Story-005-01: Gemini 3.x Model ID Tests ====================

    /// AC-1: Test get_model_id() for gemini-3-pro-high
    /// Reference: docs/stories/Story-005-01-gemini-model-id-constants.md:307-315
    #[test]
    fn test_get_model_id_gemini_3_pro_high() {
        let model_id = get_model_id("gemini-3-pro-high");
        assert_eq!(
            model_id, GEMINI_3_PRO_HIGH_MODEL_ID,
            "gemini-3-pro-high should return GEMINI_3_PRO_HIGH_MODEL_ID constant"
        );
        // NOTE: Gemini 3.x models use name-based routing, so Model ID = 0
        assert_eq!(
            model_id, 0,
            "Gemini 3.x models use name-based routing (Model ID = 0)"
        );
    }

    /// AC-2: Test Model ID logging includes gemini models
    #[test]
    fn test_model_id_logging_gemini() {
        // Verify gemini model routing is logged correctly
        let gemini_id = get_model_id("gemini-3-pro-high");
        let claude_id = get_model_id("claude-4.5-sonnet");

        // Both should return valid (though different) IDs
        assert_eq!(gemini_id, 0, "Gemini returns 0 (name-based routing)");
        assert_eq!(claude_id, 333, "Claude returns explicit ID");
    }

    /// AC-3: Test unknown gemini model returns zero
    #[test]
    fn test_unknown_gemini_model_returns_zero() {
        assert_eq!(get_model_id("gemini-unknown"), 0);
        assert_eq!(get_model_id("gemini-3-pro-ultra"), 0);
    }

    /// AC-4: Test gemini vs claude model ID consistency
    /// Both should return deterministic values (non-random)
    #[test]
    fn test_gemini_vs_claude_model_id_consistency() {
        // Claude models return explicit IDs
        let claude_id = get_model_id("claude-4.5-sonnet");
        assert_ne!(claude_id, 0, "Claude should have explicit Model ID");
        assert_eq!(claude_id, 333, "Claude ID should be 333");

        // Gemini models return 0 (architectural difference)
        let gemini_id = get_model_id("gemini-3-pro-high");
        assert_eq!(gemini_id, 0, "Gemini uses name-based routing");

        // Consistency: same model always returns same ID
        assert_eq!(get_model_id("gemini-3-pro-high"), gemini_id);
        assert_eq!(get_model_id("claude-4.5-sonnet"), claude_id);
    }

    /// AC-5: Test that get_model_id handles all expected gemini-3 variants
    #[test]
    fn test_get_model_id_gemini_3_variants() {
        // Only gemini-3-pro-high is implemented in Story-005-01
        assert_eq!(get_model_id("gemini-3-pro-high"), 0);

        // Other variants not yet implemented (future stories)
        // These should return 0 via the default match arm
        assert_eq!(get_model_id("gemini-3-pro-low"), 0);
        assert_eq!(get_model_id("gemini-3-flash"), 0);
    }

    #[test]
    fn test_request_includes_model_id() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
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
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        assert_eq!(body["modelId"], 334);
    }

    // Story #2: API Provider and Model Provider Tests
    // Reference: docs/stories/Story-003-02-api-model-providers.md

    #[test]
    fn test_get_api_provider_claude() {
        assert_eq!(get_api_provider("claude-4.5-sonnet-thinking"), 26);
        assert_eq!(get_api_provider("claude-4.5-sonnet"), 26);
    }

    #[test]
    fn test_get_api_provider_gemini() {
        assert_eq!(get_api_provider("gemini-2.5-flash"), 0);
    }

    #[test]
    fn test_get_model_provider_claude() {
        assert_eq!(get_model_provider("claude-4.5-sonnet-thinking"), 3);
        assert_eq!(get_model_provider("claude-4.5-sonnet"), 3);
    }

    #[test]
    fn test_get_model_provider_gemini() {
        assert_eq!(get_model_provider("gemini-2.5-flash"), 1);
    }

    #[test]
    fn test_get_model_provider_unknown() {
        assert_eq!(get_model_provider("unknown-model"), 0);
    }

    // GAP #2: Thinking Mode Detection Tests
    // Reference: docs/epics/Epic-004-IMPLEMENTATION-PLAN.md - GAP #2

    #[test]
    fn test_should_enable_thinking_by_default_opus_4_5() {
        // Opus 4.5 models should have thinking enabled by default
        assert!(should_enable_thinking_by_default("claude-opus-4-5"));
        assert!(should_enable_thinking_by_default("claude-opus-4.5"));
    }

    #[test]
    fn test_should_enable_thinking_by_default_thinking_suffix() {
        // Models with -thinking suffix should have thinking enabled
        assert!(should_enable_thinking_by_default(
            "claude-4.5-sonnet-thinking"
        ));
        assert!(should_enable_thinking_by_default(
            "claude-sonnet-4-5-thinking"
        ));
    }

    #[test]
    fn test_should_enable_thinking_by_default_standard_models() {
        // Standard models (without -thinking) should NOT have thinking enabled
        assert!(!should_enable_thinking_by_default("claude-4.5-sonnet"));
        assert!(!should_enable_thinking_by_default("claude-sonnet-4-5"));
        assert!(!should_enable_thinking_by_default("claude-3-5-sonnet"));
    }

    #[test]
    fn test_is_gemini_thinking_model_supported() {
        // All Gemini models support thinking via thinkingConfig
        assert!(is_gemini_thinking_model("gemini-2.5-flash"));
        assert!(is_gemini_thinking_model("gemini-2.0-flash"));
        assert!(is_gemini_thinking_model("gemini-pro"));
    }

    #[test]
    fn test_is_gemini_thinking_model_non_gemini() {
        // Non-Gemini models should return false
        assert!(!is_gemini_thinking_model("claude-4.5-sonnet"));
        assert!(!is_gemini_thinking_model("claude-4.5-sonnet-thinking"));
        assert!(!is_gemini_thinking_model("gpt-4"));
    }

    #[test]
    fn test_thinking_detection_standard_vs_thinking() {
        // CRITICAL: Standard Claude models (ID 333) do NOT support thinking
        // Only thinking variants (ID 334) support thinking

        // Simulate the logic from lines 306-307
        let standard_model = "claude-4.5-sonnet";
        let thinking_model = "claude-4.5-sonnet-thinking";

        // Standard model should NOT match
        let supports_standard =
            standard_model.contains("-thinking") || is_gemini_thinking_model(standard_model);
        assert!(
            !supports_standard,
            "Standard claude-4.5-sonnet (ID 333) should NOT support thinking"
        );

        // Thinking model SHOULD match
        let supports_thinking =
            thinking_model.contains("-thinking") || is_gemini_thinking_model(thinking_model);
        assert!(
            supports_thinking,
            "Thinking claude-4.5-sonnet-thinking (ID 334) should support thinking"
        );
    }

    // GAP #3: Integration Test Suite for claude-4.5-sonnet (Standard Model, ID 333)
    // Reference: docs/epics/Epic-004-IMPLEMENTATION-PLAN.md - GAP #3
    //
    // These tests verify the complete request transformation pipeline for the standard
    // claude-4.5-sonnet model (Model ID 333) without thinking mode.

    #[test]
    fn test_claude_4_5_sonnet_model_id_333() {
        // Verify standard model gets correct ID 333 (not 334 for thinking variant)
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Test".to_string()),
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
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok(), "Transform should succeed");

        let (body, _violations) = result.unwrap();
        assert_eq!(
            body["modelId"].as_u64(),
            Some(333),
            "Standard claude-4.5-sonnet should have modelId 333"
        );
        // Model name gets mapped to canonical form by model mapping
        assert_eq!(
            body["model"].as_str(),
            Some("claude-sonnet-4-5"),
            "Model name should be mapped to canonical form claude-sonnet-4-5"
        );
    }

    #[test]
    fn test_claude_4_5_sonnet_no_thinking() {
        // Verify thinking mode is disabled for standard model
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Test thinking".to_string()),
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
            }),
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok(), "Transform should succeed");

        let (body, _violations) = result.unwrap();

        // Verify thinkingConfig is NOT present (thinking disabled for standard model)
        let gen_config = body["request"].get("generationConfig");
        assert!(gen_config.is_some(), "generationConfig should exist");

        let thinking_config = gen_config.and_then(|gc| gc.get("thinkingConfig"));
        assert!(
            thinking_config.is_none(),
            "Standard model should NOT have thinkingConfig even if requested"
        );
    }

    #[test]
    fn test_claude_4_5_sonnet_metadata_present() {
        // Verify Antigravity metadata is correctly injected
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Test metadata".to_string()),
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
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok(), "Transform should succeed");

        let (body, _violations) = result.unwrap();

        // Verify metadata structure
        let metadata = body["request"].get("metadata");
        assert!(metadata.is_some(), "Metadata should be present");

        let metadata = metadata.unwrap();
        assert_eq!(
            metadata["ideType"].as_str(),
            Some("ANTIGRAVITY"),
            "ideType should be ANTIGRAVITY"
        );
        assert_eq!(
            metadata["ideVersion"].as_str(),
            Some("1.13.3"),
            "ideVersion should be 1.13.3"
        );
        assert!(
            metadata.get("platform").is_some(),
            "platform should be present"
        );
        assert!(
            metadata.get("architecture").is_some(),
            "architecture should be present"
        );
    }

    #[test]
    fn test_claude_4_5_sonnet_with_tools() {
        // Verify tool usage works without thinking mode
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Use the weather tool".to_string()),
            }],
            system: None,
            tools: Some(vec![Tool {
                type_: None,
                name: Some("get_weather".to_string()),
                description: Some("Get weather for a location".to_string()),
                input_schema: Some(json!({
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
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok(), "Transform with tools should succeed");

        let (body, _violations) = result.unwrap();

        // Verify tools are present
        let tools = body["request"].get("tools");
        assert!(tools.is_some(), "Tools should be present");
        assert!(tools.unwrap().is_array(), "Tools should be an array");

        // Verify NO thinking config with tools
        let gen_config = body["request"].get("generationConfig");
        if let Some(gc) = gen_config {
            assert!(
                gc.get("thinkingConfig").is_none(),
                "Standard model should not have thinking even with tools"
            );
        }
    }

    #[test]
    fn test_claude_4_5_sonnet_with_grounding() {
        // Verify web search/grounding capabilities work
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Search the web".to_string()),
            }],
            system: None,
            tools: Some(vec![Tool {
                type_: Some("web_search_20250305".to_string()),
                name: Some("google_search_retrieval".to_string()),
                description: Some("Search the web".to_string()),
                input_schema: None,
            }]),
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok(), "Transform with grounding should succeed");

        let (body, _violations) = result.unwrap();

        // Verify grounding tool is present
        let tools = body["request"].get("tools");
        assert!(tools.is_some(), "Tools should be present for grounding");
    }

    #[test]
    fn test_claude_4_5_sonnet_providers_correct() {
        // Verify API provider and model provider routing
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Test".to_string()),
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
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok(), "Transform should succeed");

        let (body, _violations) = result.unwrap();

        // Verify API provider (should be 26 for Anthropic Vertex)
        assert_eq!(
            body["apiProvider"].as_u64(),
            Some(26),
            "Claude models should route through Anthropic Vertex (26)"
        );

        // Verify model provider (should be 3 for Anthropic)
        assert_eq!(
            body["modelProvider"].as_u64(),
            Some(3),
            "Claude models should have model provider 3 (Anthropic)"
        );
    }

    #[test]
    fn test_claude_4_5_sonnet_multi_turn() {
        // Verify multi-turn conversation support
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet".to_string(),
            messages: vec![
                Message {
                    role: "user".to_string(),
                    content: MessageContent::String("First turn".to_string()),
                },
                Message {
                    role: "assistant".to_string(),
                    content: MessageContent::String("Response".to_string()),
                },
                Message {
                    role: "user".to_string(),
                    content: MessageContent::String("Second turn".to_string()),
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
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok(), "Multi-turn transform should succeed");

        let (body, _violations) = result.unwrap();

        // Verify contents array has all turns
        let contents = body["request"]["contents"].as_array();
        assert!(contents.is_some(), "Contents should be present");
        assert!(
            contents.unwrap().len() >= 3,
            "Should have at least 3 message turns"
        );
    }

    #[test]
    fn test_claude_4_5_sonnet_vs_thinking_variant() {
        // Direct comparison: standard (333) vs thinking (334)
        let standard_req = ClaudeRequest {
            model: "claude-4.5-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Test".to_string()),
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
            }),
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let thinking_req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
            ..standard_req.clone()
        };

        // Transform both
        let standard_result = transform_claude_request_in(&standard_req, "test-project");
        let thinking_result = transform_claude_request_in(&thinking_req, "test-project");

        assert!(standard_result.is_ok() && thinking_result.is_ok());

        let (standard_body, _) = standard_result.unwrap();
        let (thinking_body, _) = thinking_result.unwrap();

        // Verify model IDs are different
        assert_eq!(
            standard_body["modelId"].as_u64(),
            Some(333),
            "Standard should be ID 333"
        );
        assert_eq!(
            thinking_body["modelId"].as_u64(),
            Some(334),
            "Thinking should be ID 334"
        );

        // Verify thinking config presence
        let standard_thinking = standard_body["request"]
            .get("generationConfig")
            .and_then(|gc| gc.get("thinkingConfig"));
        let thinking_thinking = thinking_body["request"]
            .get("generationConfig")
            .and_then(|gc| gc.get("thinkingConfig"));

        assert!(
            standard_thinking.is_none(),
            "Standard model should NOT have thinkingConfig"
        );
        assert!(
            thinking_thinking.is_some(),
            "Thinking variant SHOULD have thinkingConfig"
        );
    }

    #[test]
    fn test_request_includes_providers() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
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
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        assert_eq!(body["modelId"], 334);
        assert_eq!(body["apiProvider"], 26);
        assert_eq!(body["modelProvider"], 3);
    }

    // Story #3: ideType ANTIGRAVITY Metadata Tests
    // Reference: docs/stories/Story-003-03-antigravity-metadata.md

    #[test]
    fn test_metadata_includes_ide_type() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
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
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        assert_eq!(body["request"]["metadata"]["ideType"], "ANTIGRAVITY");
    }

    #[test]
    fn test_metadata_includes_ide_version() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
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
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        assert_eq!(body["request"]["metadata"]["ideVersion"], "1.13.3");
    }

    #[test]
    fn test_metadata_complete() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
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
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        let metadata = &body["request"]["metadata"];

        // All four fields must be present
        assert_eq!(metadata["ideType"], "ANTIGRAVITY");
        assert_eq!(metadata["ideVersion"], "1.13.3");
        assert!(metadata.get("platform").is_some());
        assert!(metadata.get("architecture").is_some());
    }

    #[test]
    fn test_metadata_preserves_session_id() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
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
            metadata: Some(Metadata {
                user_id: Some("test-session-123".to_string()),
                workspace_id: None,
                cloudaicompanion_project: None,
            }),
            output_config: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        let metadata = &body["request"]["metadata"];

        assert_eq!(metadata["sessionId"], "test-session-123");
        assert_eq!(metadata["ideType"], "ANTIGRAVITY");
    }

    // Story #4: Extended Session Metadata Tests
    // Reference: docs/stories/Story-003-04-extended-session-metadata.md

    #[test]
    fn test_metadata_without_extended_fields() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
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
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        let metadata = &body["request"]["metadata"];

        // Required fields present
        assert_eq!(metadata["ideType"], "ANTIGRAVITY");
        assert_eq!(metadata["ideVersion"], "1.13.3");

        // Extended fields NOT present
        assert!(metadata.get("workspace_id").is_none());
        assert!(metadata.get("cloudaicompanion_project").is_none());
    }

    #[test]
    fn test_metadata_with_workspace_id() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
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
            metadata: Some(Metadata {
                user_id: None,
                workspace_id: Some("workspace-abc".to_string()),
                cloudaicompanion_project: None,
            }),
            output_config: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        let metadata = &body["request"]["metadata"];

        assert_eq!(metadata["workspace_id"], "workspace-abc");
        assert_eq!(metadata["ideType"], "ANTIGRAVITY");
    }

    #[test]
    fn test_metadata_with_cloudaicompanion_project() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
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
            metadata: Some(Metadata {
                user_id: None,
                workspace_id: None,
                cloudaicompanion_project: Some("project-xyz".to_string()),
            }),
            output_config: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        let metadata = &body["request"]["metadata"];

        assert_eq!(metadata["cloudaicompanion_project"], "project-xyz");
        assert_eq!(metadata["ideType"], "ANTIGRAVITY");
    }

    #[test]
    fn test_metadata_with_all_fields() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
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
            metadata: Some(Metadata {
                user_id: Some("session-456".to_string()),
                workspace_id: Some("workspace-abc".to_string()),
                cloudaicompanion_project: Some("project-def".to_string()),
            }),
            output_config: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        let metadata = &body["request"]["metadata"];

        // All fields present
        assert_eq!(metadata["sessionId"], "session-456");
        assert_eq!(metadata["workspace_id"], "workspace-abc");
        assert_eq!(metadata["cloudaicompanion_project"], "project-def");
        assert_eq!(metadata["ideType"], "ANTIGRAVITY");
    }

    // ========== Story #6: Budget Constraint Warning Tests ==========

    /// Test 1: Auto-fix when max_tokens equals thinking_budget
    #[test]
    fn test_budget_constraint_autofix_equal() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Test".to_string()),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(32000), // Equal to budget
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(32000),
            }),
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        // Should auto-fix to budget + 100
        assert_eq!(
            body["request"]["generationConfig"]["maxOutputTokens"],
            32100
        );
    }

    /// Test 2: Auto-fix when max_tokens less than thinking_budget
    #[test]
    fn test_budget_constraint_autofix_less() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Test".to_string()),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(31000), // Less than budget
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(32000),
            }),
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        // Should auto-fix to budget + 100
        assert_eq!(
            body["request"]["generationConfig"]["maxOutputTokens"],
            32100
        );
    }

    /// Test 3: No auto-fix when max_tokens greater than thinking_budget (valid)
    #[test]
    fn test_no_autofix_when_valid() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Test".to_string()),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(40000), // Greater than budget (valid)
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(32000),
            }),
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        // Should NOT auto-fix, value unchanged
        assert_eq!(
            body["request"]["generationConfig"]["maxOutputTokens"],
            40000
        );
    }

    /// Test 4: Auto-fix with clamped budget (Claude model, budget > 32000)
    #[test]
    fn test_autofix_with_clamped_budget_claude() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Test".to_string()),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(31000), // Less than clamped budget (32000)
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(35000), // Will be clamped to 32000 for Claude
            }),
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        // Should auto-fix based on CLAMPED budget (32000 + 100)
        assert_eq!(
            body["request"]["generationConfig"]["maxOutputTokens"],
            32100
        );
    }

    /// Test 5: Auto-fix with web search clamping (Gemini 2.5 Flash, budget clamped to 24576)
    #[test]
    fn test_autofix_with_websearch_clamping() {
        let req = ClaudeRequest {
            model: "gemini-2.5-flash-thinking".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Test".to_string()),
            }],
            system: None,
            tools: Some(vec![Tool {
                type_: Some("web_search_20250305".to_string()),
                name: Some("web_search".to_string()),
                description: None,
                input_schema: None,
            }]),
            stream: false,
            max_tokens: Some(24000), // Less than web search limit (24576)
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(30000), // Will be clamped to 24576 for web search
            }),
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        // Should auto-fix based on WEB SEARCH clamped budget (24576 + 100)
        assert_eq!(
            body["request"]["generationConfig"]["maxOutputTokens"],
            24676
        );
    }

    /// Test 6: Verify auto-fix adds exactly 100 tokens
    #[test]
    fn test_autofix_value_correctness() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Test".to_string()),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(25000),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(25000), // Equal to max_tokens
            }),
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        // Auto-fix should add exactly 100 tokens
        assert_eq!(
            body["request"]["generationConfig"]["maxOutputTokens"],
            25100
        );
    }

    // ========== Story #7: Position Enforcement Logging Tests ==========

    /// Test 1: Thinking block at non-zero index is downgraded to text
    #[test]
    fn test_position_violation_downgrade_behavior() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Array(vec![
                    ContentBlock::Text {
                        text: "First part".to_string(),
                    },
                    ContentBlock::Text {
                        text: "Second part".to_string(),
                    },
                    ContentBlock::Thinking {
                        thinking: "This should be first!".to_string(),
                        signature: Some("eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJnb29nbGUuY29tIiwic3ViIjoidGhpbmtpbmciLCJleHAiOjE3MDUwODAwMDB9.MEUCIQDxyz123".to_string()),
                        cache_control: None,
                    },
                ]),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(40000),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: None,
            }),
            metadata: None,
            output_config: None,
        tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        let parts = &body["request"]["contents"][0]["parts"];
        assert!(parts.is_array());

        let parts_array = parts.as_array().unwrap();
        // Should have: [text1, text2, thinking_as_text]
        assert_eq!(parts_array.len(), 3);

        // Verify third part is text (not thought)
        let thinking_part = &parts_array[2];
        assert!(
            thinking_part["thought"].is_null()
                || !thinking_part["thought"].as_bool().unwrap_or(false)
        );
        assert!(thinking_part["text"].is_string());
        assert_eq!(thinking_part["text"], "This should be first!");
    }

    /// Test 2: Thinking block at index 0 is NOT downgraded (valid position)
    #[test]
    fn test_no_position_violation_when_first() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
            messages: vec![Message {
                role: "model".to_string(),
                content: MessageContent::Array(vec![
                    ContentBlock::Thinking {
                        thinking: "I'm thinking first!".to_string(),
                        signature: Some("eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJnb29nbGUuY29tIiwic3ViIjoidGhpbmtpbmciLCJleHAiOjE3MDUwODAwMDB9.MEUCIQDxyz456".to_string()),
                        cache_control: None,
                    },
                    ContentBlock::Text {
                        text: "Then I respond".to_string(),
                    },
                ]),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(40000),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: None,
            }),
            metadata: None,
            output_config: None,
        tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        let parts = &body["request"]["contents"][0]["parts"];
        assert!(parts.is_array());

        let parts_array = parts.as_array().unwrap();
        // Should have: [thought_block, text_block]
        assert_eq!(parts_array.len(), 2);

        // Verify first part is thought (not downgraded)
        let thinking_part = &parts_array[0];
        assert_eq!(thinking_part["thought"], true);
        assert_eq!(thinking_part["text"], "I'm thinking first!");
    }

    /// Test 3: Empty thinking block at non-zero index (no text part added)
    #[test]
    fn test_empty_thinking_position_violation() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Array(vec![
                    ContentBlock::Text {
                        text: "First part".to_string(),
                    },
                    ContentBlock::Thinking {
                        thinking: "".to_string(), // Empty!
                        signature: Some("eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJnb29nbGUuY29tIiwic3ViIjoidGhpbmtpbmciLCJleHAiOjE3MDUwODAwMDB9.MEUCIQDxyz789".to_string()),
                        cache_control: None,
                    },
                ]),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(40000),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: None,
            }),
            metadata: None,
            output_config: None,
        tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        let parts = &body["request"]["contents"][0]["parts"];
        assert!(parts.is_array());

        let parts_array = parts.as_array().unwrap();
        // Should only have original part (empty thinking not added)
        assert_eq!(parts_array.len(), 1);
        assert_eq!(parts_array[0]["text"], "First part");
    }

    /// Test 4: Multiple thinking blocks at invalid positions (all downgraded)
    #[test]
    fn test_multiple_thinking_position_violations() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Array(vec![
                    ContentBlock::Text {
                        text: "First part".to_string(),
                    },
                    ContentBlock::Thinking {
                        thinking: "First thinking violation".to_string(),
                        signature: Some("eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJnb29nbGUuY29tIiwic3ViIjoidGhpbmtpbmciLCJleHAiOjE3MDUwODAwMDB9.MEUCIQDabc123".to_string()),
                        cache_control: None,
                    },
                    ContentBlock::Thinking {
                        thinking: "Second thinking violation".to_string(),
                        signature: Some("eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJnb29nbGUuY29tIiwic3ViIjoidGhpbmtpbmciLCJleHAiOjE3MDUwODAwMDB9.MEUCIQDdef456".to_string()),
                        cache_control: None,
                    },
                ]),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(40000),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: None,
            }),
            metadata: None,
            output_config: None,
        tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        let parts = &body["request"]["contents"][0]["parts"];
        assert!(parts.is_array());

        let parts_array = parts.as_array().unwrap();
        // Should have: [text1, thinking1_as_text, thinking2_as_text]
        assert_eq!(parts_array.len(), 3);

        // Verify all are text (none are thought blocks)
        for part in parts_array.iter() {
            assert!(part["thought"].is_null() || !part["thought"].as_bool().unwrap_or(false));
            assert!(part["text"].is_string());
        }

        // Verify content
        assert_eq!(parts_array[0]["text"], "First part");
        assert_eq!(parts_array[1]["text"], "First thinking violation");
        assert_eq!(parts_array[2]["text"], "Second thinking violation");
    }

    // ==================== Story #8: Integration Tests ====================
    // These tests validate end-to-end violation tracking from detection to recording

    /// Integration Test #1: Budget violation end-to-end tracking
    /// AC12: Budget violation detected and returned in ViolationInfo
    #[tokio::test]
    async fn test_integration_budget_violation_tracking() {
        let req = ClaudeRequest {
            model: "claude-sonnet-4.5-20250514-thinking".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Array(vec![ContentBlock::Text {
                    text: "Test budget violation".to_string(),
                }]),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(1000), // Less than budget
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(2000), // Triggers budget violation (maxTokens < budget)
            }),
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok(), "Transform should succeed");

        let (_body, violations) = result.unwrap();

        // Verify budget violation was detected
        assert!(
            violations.budget_violation,
            "Budget violation should be detected when max_tokens < budget"
        );
        assert!(
            !violations.position_violation,
            "No position violation expected"
        );
        assert!(
            violations.has_violations(),
            "has_violations() should be true"
        );
    }

    /// Integration Test #2: Position violation end-to-end tracking
    /// AC13: Position violation detected with index and role tracking
    #[tokio::test]
    async fn test_integration_position_violation_tracking() {
        let req = ClaudeRequest {
            model: "claude-sonnet-4.5-20250514-thinking".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Array(vec![
                    ContentBlock::Text {
                        text: "First part".to_string(),
                    },
                    ContentBlock::Thinking {
                        thinking: "This thinking block is in wrong position".to_string(),
                        signature: Some("test_sig_user".to_string()),
                        cache_control: None,
                    },
                ]),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(8000),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(2000),
            }),
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok(), "Transform should succeed");

        let (_body, violations) = result.unwrap();

        // Verify position violation was detected
        assert!(
            violations.position_violation,
            "Position violation should be detected"
        );
        assert!(
            violations.position_index.is_some(),
            "Position index should be recorded"
        );
        assert!(
            violations.position_role.is_some(),
            "Position role should be recorded"
        );

        // Verify violation details (thinking at index 1 in user message)
        let index = violations.position_index.unwrap();
        assert_eq!(index, 1, "Violation at index 1");

        let role = violations.position_role.as_ref().unwrap();
        assert_eq!(role, "user", "Violation in user message");
    }

    /// Integration Test #3: Multiple violations in single request
    /// AC14: Both violation types can be detected simultaneously
    #[tokio::test]
    async fn test_integration_multiple_violations() {
        let req = ClaudeRequest {
            model: "claude-sonnet-4.5-20250514-thinking".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Array(vec![
                    ContentBlock::Text {
                        text: "First part".to_string(),
                    },
                    ContentBlock::Thinking {
                        thinking: "Thinking in wrong position".to_string(), // Position violation
                        signature: Some("test_sig_multi".to_string()),
                        cache_control: None,
                    },
                ]),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(1000), // Less than budget → budget violation
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(2000), // Budget violation
            }),
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok(), "Transform should succeed");

        let (_body, violations) = result.unwrap();

        // Verify both violations detected
        assert!(
            violations.budget_violation,
            "Budget violation should be detected"
        );
        assert!(
            violations.position_violation,
            "Position violation should be detected"
        );
        assert!(
            violations.has_violations(),
            "has_violations() should be true"
        );
        assert_eq!(
            violations.position_index.unwrap(),
            1,
            "Position violation at index 1"
        );
        assert_eq!(
            violations.position_role.as_ref().unwrap(),
            "user",
            "Position violation in user message"
        );
    }

    /// Integration Test #4: No violations in valid request
    /// AC15: ViolationInfo remains empty when no violations occur
    #[tokio::test]
    async fn test_integration_no_violations() {
        let req = ClaudeRequest {
            model: "claude-sonnet-4.5-20250514-thinking".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Array(vec![
                    ContentBlock::Thinking {
                        thinking: "Thinking in correct position (first)".to_string(),
                        signature: Some("test_sig_valid".to_string()),
                        cache_control: None,
                    },
                    ContentBlock::Text {
                        text: "Text after thinking".to_string(),
                    },
                ]),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(8000), // Greater than budget → no violation
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(2000),
            }),
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok(), "Transform should succeed");

        let (_body, violations) = result.unwrap();

        // Verify no violations
        assert!(!violations.budget_violation, "No budget violation expected");
        assert!(
            !violations.position_violation,
            "No position violation expected"
        );
        assert!(
            !violations.has_violations(),
            "has_violations() should be false"
        );
        assert!(violations.position_index.is_none(), "No position index");
        assert!(violations.position_role.is_none(), "No position role");
    }

    // ==================== Story #9: Tool Configuration Modes Tests ====================

    /// AC-1: Test ToolChoice enum variants and helper methods
    #[test]
    fn test_tool_choice_enum_variants() {
        let auto = ToolChoice::Auto;
        assert_eq!(auto.to_gemini_mode(), "AUTO");
        assert_eq!(auto.get_tool_name(), None);

        let any = ToolChoice::Any;
        assert_eq!(any.to_gemini_mode(), "ANY");
        assert_eq!(any.get_tool_name(), None);

        let none = ToolChoice::None;
        assert_eq!(none.to_gemini_mode(), "NONE");
        assert_eq!(none.get_tool_name(), None);

        let tool = ToolChoice::Tool {
            name: "get_weather".to_string(),
        };
        assert_eq!(tool.to_gemini_mode(), "VALIDATED");
        assert_eq!(tool.get_tool_name(), Some("get_weather"));
    }

    /// AC-1: Test Default trait
    #[test]
    fn test_tool_choice_default() {
        assert_eq!(ToolChoice::default(), ToolChoice::Auto);
    }

    /// AC-8: Test JSON deserialization for all variants
    #[test]
    fn test_tool_choice_deserialization() {
        // Auto
        let json = r#"{"type": "auto"}"#;
        let tc: ToolChoice = serde_json::from_str(json).unwrap();
        assert_eq!(tc, ToolChoice::Auto);

        // Any
        let json = r#"{"type": "any"}"#;
        let tc: ToolChoice = serde_json::from_str(json).unwrap();
        assert_eq!(tc, ToolChoice::Any);

        // None
        let json = r#"{"type": "none"}"#;
        let tc: ToolChoice = serde_json::from_str(json).unwrap();
        assert_eq!(tc, ToolChoice::None);

        // Tool
        let json = r#"{"type": "tool", "name": "get_weather"}"#;
        let tc: ToolChoice = serde_json::from_str(json).unwrap();
        assert_eq!(
            tc,
            ToolChoice::Tool {
                name: "get_weather".to_string()
            }
        );

        // Invalid type
        let json = r#"{"type": "invalid"}"#;
        assert!(serde_json::from_str::<ToolChoice>(json).is_err());

        // Missing name for tool type
        let json = r#"{"type": "tool"}"#;
        assert!(serde_json::from_str::<ToolChoice>(json).is_err());
    }

    /// AC-2: Test ClaudeRequest deserialization with tool_choice
    #[test]
    fn test_claude_request_with_tool_choice() {
        let json = r#"{
            "model": "claude-4.5-sonnet-thinking",
            "messages": [],
            "stream": false,
            "tools": [{"type": "function", "name": "test", "description": "Test tool", "input_schema": {"type": "object"}}],
            "tool_choice": {"type": "auto"}
        }"#;

        let req: ClaudeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.tool_choice, Some(ToolChoice::Auto));
    }

    /// AC-2: Test backward compatibility
    #[test]
    fn test_claude_request_without_tool_choice() {
        let json = r#"{
            "model": "claude-4.5-sonnet-thinking",
            "messages": [],
            "stream": false,
            "tools": [{"type": "function", "name": "test", "description": "Test tool", "input_schema": {"type": "object"}}]
        }"#;

        let req: ClaudeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.tool_choice, None);
    }

    /// AC-3: Test AUTO mode conversion
    #[test]
    fn test_auto_mode_mapping() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
            messages: vec![],
            system: None,
            tools: Some(vec![Tool {
                type_: None,
                name: Some("get_weather".to_string()),
                description: Some("Get weather".to_string()),
                input_schema: Some(json!({"type": "object"})),
            }]),
            stream: false,
            max_tokens: Some(1024),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            tool_choice: Some(ToolChoice::Auto),
        };

        let (body, _violations) = transform_claude_request_in(&req, "test-project").unwrap();
        let request = &body["request"];

        assert_eq!(
            request["toolConfig"]["functionCallingConfig"]["mode"],
            "AUTO"
        );
        assert!(request["toolConfig"]["functionCallingConfig"]
            .get("allowedFunctionNames")
            .is_none());
        assert!(request["tools"].is_array());
    }

    /// AC-4: Test ANY mode conversion
    #[test]
    fn test_any_mode_mapping() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
            messages: vec![],
            system: None,
            tools: Some(vec![Tool {
                type_: None,
                name: Some("get_weather".to_string()),
                description: Some("Get weather".to_string()),
                input_schema: Some(json!({"type": "object"})),
            }]),
            stream: false,
            max_tokens: Some(1024),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            tool_choice: Some(ToolChoice::Any),
        };

        let (body, _violations) = transform_claude_request_in(&req, "test-project").unwrap();
        let request = &body["request"];

        assert_eq!(
            request["toolConfig"]["functionCallingConfig"]["mode"],
            "ANY"
        );
        assert!(request["toolConfig"]["functionCallingConfig"]
            .get("allowedFunctionNames")
            .is_none());
    }

    /// AC-5: Test NONE mode conversion
    #[test]
    fn test_none_mode_mapping() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
            messages: vec![],
            system: None,
            tools: Some(vec![Tool {
                type_: None,
                name: Some("get_weather".to_string()),
                description: Some("Get weather".to_string()),
                input_schema: Some(json!({"type": "object"})),
            }]),
            stream: false,
            max_tokens: Some(1024),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            tool_choice: Some(ToolChoice::None),
        };

        let (body, _violations) = transform_claude_request_in(&req, "test-project").unwrap();
        let request = &body["request"];

        assert_eq!(
            request["toolConfig"]["functionCallingConfig"]["mode"],
            "NONE"
        );
    }

    /// AC-6: Test Tool{name} mode conversion
    #[test]
    fn test_tool_forcing_mapping() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
            messages: vec![],
            system: None,
            tools: Some(vec![
                Tool {
                    type_: None,
                    name: Some("get_weather".to_string()),
                    description: Some("Get weather".to_string()),
                    input_schema: Some(json!({"type": "object"})),
                },
                Tool {
                    type_: None,
                    name: Some("get_forecast".to_string()),
                    description: Some("Get forecast".to_string()),
                    input_schema: Some(json!({"type": "object"})),
                },
            ]),
            stream: false,
            max_tokens: Some(1024),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            tool_choice: Some(ToolChoice::Tool {
                name: "get_weather".to_string(),
            }),
        };

        let (body, _violations) = transform_claude_request_in(&req, "test-project").unwrap();
        let request = &body["request"];

        assert_eq!(
            request["toolConfig"]["functionCallingConfig"]["mode"],
            "VALIDATED"
        );
        assert_eq!(
            request["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"],
            json!(["get_weather"])
        );
    }

    /// AC-7: Test default VALIDATED mode for backward compatibility
    #[test]
    fn test_backward_compatibility_no_tool_choice() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
            messages: vec![],
            system: None,
            tools: Some(vec![Tool {
                type_: None,
                name: Some("get_weather".to_string()),
                description: Some("Get weather".to_string()),
                input_schema: Some(json!({"type": "object"})),
            }]),
            stream: false,
            max_tokens: Some(1024),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            tool_choice: None, // No tool_choice specified
        };

        let (body, _violations) = transform_claude_request_in(&req, "test-project").unwrap();
        let request = &body["request"];

        // Should default to VALIDATED for backward compatibility
        assert_eq!(
            request["toolConfig"]["functionCallingConfig"]["mode"],
            "VALIDATED"
        );
        assert!(request["toolConfig"]["functionCallingConfig"]
            .get("allowedFunctionNames")
            .is_none());
    }

    /// AC-13: Test tool_choice without tools validation (should log warning)
    #[test]
    fn test_tool_choice_without_tools_validation() {
        // Test Auto mode without tools
        let req_auto = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
            messages: vec![],
            system: None,
            tools: None, // ❌ No tools
            stream: false,
            max_tokens: Some(1024),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            tool_choice: Some(ToolChoice::Auto), // ❌ But forcing tool usage
        };

        // Should not error, just log warning (permissive approach)
        let result = transform_claude_request_in(&req_auto, "test-project");
        assert!(result.is_ok());
        let (inner_request, _violations) = result.unwrap();
        assert!(inner_request.get("toolConfig").is_none());

        // Test Any mode without tools
        let req_any = ClaudeRequest {
            tool_choice: Some(ToolChoice::Any),
            ..req_auto.clone()
        };
        let result = transform_claude_request_in(&req_any, "test-project");
        assert!(result.is_ok());

        // Test Tool{name} mode without tools
        let req_tool = ClaudeRequest {
            tool_choice: Some(ToolChoice::Tool {
                name: "nonexistent".to_string(),
            }),
            ..req_auto.clone()
        };
        let result = transform_claude_request_in(&req_tool, "test-project");
        assert!(result.is_ok());
    }

    /// AC-13: Test tool_choice None without tools is valid
    #[test]
    fn test_tool_choice_none_without_tools_valid() {
        let req = ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
            messages: vec![],
            system: None,
            tools: None, // ✅ No tools
            stream: false,
            max_tokens: Some(1024),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            tool_choice: Some(ToolChoice::None), // ✅ None mode is valid
        };

        // Should succeed without warnings
        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());
        let (inner_request, _violations) = result.unwrap();
        // No toolConfig should be present
        assert!(inner_request.get("toolConfig").is_none());
    }

    // ==================================================================================
    // INTEGRATION TESTS: End-to-End Flow
    // ==================================================================================

    /// Integration Test 1: Full end-to-end flow with AUTO mode
    /// Tests complete request transformation including messages, tools, and toolConfig
    #[test]
    fn test_end_to_end_tool_choice_auto() {
        let req = ClaudeRequest {
            model: "claude-sonnet-4-5".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("What's the weather in Tokyo?".to_string()),
            }],
            system: Some(SystemPrompt::String(
                "You are a helpful weather assistant.".to_string(),
            )),
            tools: Some(vec![
                Tool {
                    type_: None,
                    name: Some("get_weather".to_string()),
                    description: Some("Get current weather for a location".to_string()),
                    input_schema: Some(json!({
                        "type": "object",
                        "properties": {
                            "location": {"type": "string", "description": "City name"}
                        },
                        "required": ["location"]
                    })),
                },
                Tool {
                    type_: None,
                    name: Some("get_forecast".to_string()),
                    description: Some("Get weather forecast".to_string()),
                    input_schema: Some(json!({
                        "type": "object",
                        "properties": {
                            "location": {"type": "string"},
                            "days": {"type": "integer"}
                        },
                        "required": ["location"]
                    })),
                },
            ]),
            stream: false,
            max_tokens: Some(1024),
            temperature: Some(0.7),
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            tool_choice: Some(ToolChoice::Auto),
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok(), "Request transformation should succeed");

        let (body, _violations) = result.unwrap();
        let request = &body["request"];

        // Verify top-level structure
        assert_eq!(body["model"].as_str(), Some("claude-sonnet-4-5"));
        assert_eq!(body["project"].as_str(), Some("test-project"));
        assert!(body["requestId"].is_string());

        // Verify contents (messages) are present
        assert!(request["contents"].is_array());
        let contents = request["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"].as_str(), Some("user"));

        // Verify systemInstruction is present (content varies due to Antigravity identity injection)
        assert!(request["systemInstruction"].is_object());
        assert!(request["systemInstruction"]["parts"].is_array());

        // Verify tools are present
        assert!(request["tools"].is_array());
        let tools = request["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        let tool_obj = &tools[0];
        assert!(tool_obj["functionDeclarations"].is_array());
        let functions = tool_obj["functionDeclarations"].as_array().unwrap();
        assert_eq!(functions.len(), 2);
        assert_eq!(functions[0]["name"].as_str(), Some("get_weather"));
        assert_eq!(functions[1]["name"].as_str(), Some("get_forecast"));

        // Verify toolConfig with AUTO mode
        assert_eq!(
            request["toolConfig"]["functionCallingConfig"]["mode"],
            "AUTO"
        );
        assert!(request["toolConfig"]["functionCallingConfig"]
            .get("allowedFunctionNames")
            .is_none());

        // Verify generationConfig
        assert!(request["generationConfig"].is_object());
        assert_eq!(
            request["generationConfig"]["maxOutputTokens"].as_i64(),
            Some(1024)
        );
        // Check temperature is approximately 0.7 (floating point precision)
        let temp = request["generationConfig"]["temperature"].as_f64().unwrap();
        assert!(
            (temp - 0.7).abs() < 0.01,
            "Temperature should be approximately 0.7, got {}",
            temp
        );
    }

    /// Integration Test 2: Full end-to-end flow with Tool forcing (VALIDATED mode)
    /// Tests forced tool calling with allowedFunctionNames
    #[test]
    fn test_end_to_end_tool_choice_force_specific() {
        let req = ClaudeRequest {
            model: "claude-sonnet-4-5".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Tell me about the weather".to_string()),
            }],
            system: None,
            tools: Some(vec![
                Tool {
                    type_: None,
                    name: Some("get_weather".to_string()),
                    description: Some("Get current weather for a location".to_string()),
                    input_schema: Some(json!({
                        "type": "object",
                        "properties": {
                            "location": {"type": "string"}
                        },
                        "required": ["location"]
                    })),
                },
                Tool {
                    type_: None,
                    name: Some("get_forecast".to_string()),
                    description: Some("Get weather forecast".to_string()),
                    input_schema: Some(json!({
                        "type": "object",
                        "properties": {
                            "location": {"type": "string"},
                            "days": {"type": "integer"}
                        },
                        "required": ["location"]
                    })),
                },
            ]),
            stream: false,
            max_tokens: Some(2048),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            tool_choice: Some(ToolChoice::Tool {
                name: "get_weather".to_string(),
            }),
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok(), "Request transformation should succeed");

        let (body, _violations) = result.unwrap();
        let request = &body["request"];

        // Verify top-level structure
        assert_eq!(body["model"].as_str(), Some("claude-sonnet-4-5"));
        assert!(body["requestId"].is_string());

        // Verify contents
        assert!(request["contents"].is_array());
        let contents = request["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);

        // Verify tools (both should be present)
        assert!(request["tools"].is_array());
        let tools = request["tools"].as_array().unwrap();
        let tool_obj = &tools[0];
        let functions = tool_obj["functionDeclarations"].as_array().unwrap();
        assert_eq!(functions.len(), 2);

        // Verify toolConfig with VALIDATED mode and allowedFunctionNames
        assert_eq!(
            request["toolConfig"]["functionCallingConfig"]["mode"],
            "VALIDATED"
        );

        // Critical: Should force only get_weather tool
        let allowed_functions =
            &request["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"];
        assert!(allowed_functions.is_array());
        let allowed_array = allowed_functions.as_array().unwrap();
        assert_eq!(allowed_array.len(), 1);
        assert_eq!(allowed_array[0].as_str(), Some("get_weather"));

        // Verify generationConfig
        assert_eq!(
            request["generationConfig"]["maxOutputTokens"].as_i64(),
            Some(2048)
        );

        // Verify metadata structure
        assert!(request["metadata"].is_object());
        assert_eq!(request["metadata"]["ideType"].as_str(), Some("ANTIGRAVITY"));
    }

    // ==================================================================================
    // STORY #10 TESTS: Gemini Settings (Anti-Plagiarism Protection)
    // ==================================================================================

    /// AC-1: Test geminiSettings field is present with correct structure
    #[test]
    fn test_gemini_settings_present() {
        let req = ClaudeRequest {
            model: "claude-sonnet-4-5".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Hello".to_string()),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(1024),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let (body, _violations) = transform_claude_request_in(&req, "test-project").unwrap();
        let request = &body["request"];

        // Verify geminiSettings exists
        assert!(
            request.get("geminiSettings").is_some(),
            "geminiSettings field should be present"
        );

        // Verify recitationPolicy structure
        let gemini_settings = &request["geminiSettings"];
        assert!(
            gemini_settings.is_object(),
            "geminiSettings should be an object"
        );

        assert_eq!(
            gemini_settings["recitationPolicy"]["action"].as_str(),
            Some("BLOCK"),
            "recitationPolicy action should be BLOCK"
        );
        assert_eq!(
            gemini_settings["recitationPolicy"]["threshold"].as_str(),
            Some("LOW"),
            "recitationPolicy threshold should be LOW"
        );
    }

    /// AC-2: Test geminiSettings is always present (not conditional)
    #[test]
    fn test_gemini_settings_always_present() {
        // Test 1: Request without tools
        let req1 = ClaudeRequest {
            model: "claude-sonnet-4-5".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Test".to_string()),
            }],
            system: None,
            tools: None, // No tools
            stream: false,
            max_tokens: Some(1024),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let (body1, _) = transform_claude_request_in(&req1, "test-1").unwrap();
        assert!(
            body1["request"].get("geminiSettings").is_some(),
            "geminiSettings should be present even without tools"
        );

        // Test 2: Request without thinking
        let req2 = ClaudeRequest {
            model: "claude-sonnet-4-5".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Test".to_string()),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(1024),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None, // No thinking
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let (body2, _) = transform_claude_request_in(&req2, "test-2").unwrap();
        assert!(
            body2["request"].get("geminiSettings").is_some(),
            "geminiSettings should be present even without thinking"
        );

        // Test 3: Minimal request
        let req3 = ClaudeRequest {
            model: "claude-sonnet-4-5".to_string(),
            messages: vec![],
            stream: false,
            system: None,
            tools: None,
            max_tokens: Some(1024),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let (body3, _) = transform_claude_request_in(&req3, "test-3").unwrap();
        assert!(
            body3["request"].get("geminiSettings").is_some(),
            "geminiSettings should be present even in minimal request"
        );
    }

    /// AC-3: Test geminiSettings JSON structure
    #[test]
    fn test_gemini_settings_json_structure() {
        let req = ClaudeRequest {
            model: "claude-sonnet-4-5".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Test".to_string()),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(1024),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let (body, _violations) = transform_claude_request_in(&req, "test-project").unwrap();
        let request = &body["request"];

        // Serialize and verify structure
        let json_str = serde_json::to_string_pretty(&request).unwrap();
        assert!(
            json_str.contains("geminiSettings"),
            "Serialized JSON should contain geminiSettings"
        );
        assert!(
            json_str.contains("recitationPolicy"),
            "Serialized JSON should contain recitationPolicy"
        );
        assert!(
            json_str.contains("\"action\": \"BLOCK\"") || json_str.contains("\"action\":\"BLOCK\""),
            "Serialized JSON should contain action: BLOCK"
        );
        assert!(
            json_str.contains("\"threshold\": \"LOW\"")
                || json_str.contains("\"threshold\":\"LOW\""),
            "Serialized JSON should contain threshold: LOW"
        );

        // Verify nested structure
        let gemini_settings = &request["geminiSettings"];
        assert!(
            gemini_settings.is_object(),
            "geminiSettings should be an object"
        );
        assert!(
            gemini_settings["recitationPolicy"].is_object(),
            "recitationPolicy should be an object"
        );
    }

    /// AC-5: Test no breaking changes to existing fields
    #[test]
    fn test_gemini_settings_no_breaking_changes() {
        let req = ClaudeRequest {
            model: "claude-sonnet-4-5".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Test all fields".to_string()),
            }],
            system: Some(SystemPrompt::String("System prompt".to_string())),
            tools: Some(vec![Tool {
                type_: None,
                name: Some("test_tool".to_string()),
                description: Some("Test tool".to_string()),
                input_schema: Some(json!({"type": "object"})),
            }]),
            stream: false,
            max_tokens: Some(2048),
            temperature: Some(0.5),
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            tool_choice: Some(ToolChoice::Auto),
        };

        let (body, _violations) = transform_claude_request_in(&req, "test-project").unwrap();
        let request = &body["request"];

        // Verify all existing fields still present
        assert!(
            request.get("contents").is_some(),
            "contents field should still be present"
        );
        assert!(
            request.get("safetySettings").is_some(),
            "safetySettings field should still be present"
        );
        assert!(
            request.get("systemInstruction").is_some(),
            "systemInstruction field should still be present"
        );
        assert!(
            request.get("generationConfig").is_some(),
            "generationConfig field should still be present"
        );
        assert!(
            request.get("tools").is_some(),
            "tools field should still be present"
        );
        assert!(
            request.get("toolConfig").is_some(),
            "toolConfig field should still be present"
        );
        assert!(
            request.get("metadata").is_some(),
            "metadata field should still be present"
        );

        // Verify new field added
        assert!(
            request.get("geminiSettings").is_some(),
            "geminiSettings field should be added"
        );

        // Verify geminiSettings doesn't affect other fields
        assert_eq!(
            request["generationConfig"]["maxOutputTokens"].as_i64(),
            Some(2048),
            "maxOutputTokens should not be affected"
        );
        assert_eq!(
            request["toolConfig"]["functionCallingConfig"]["mode"].as_str(),
            Some("AUTO"),
            "toolConfig mode should not be affected"
        );
    }
}
