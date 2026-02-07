// Main Request Transformation
// Converts Claude requests to Gemini v1internal format

use super::cleanup::{clean_cache_control_from_messages, deep_clean_cache_control};
use super::contents::build_google_contents;
use super::generation::build_generation_config;
use super::safety::build_safety_settings;
use super::sorting::{merge_consecutive_messages, sort_thinking_blocks_first};
use super::system::build_system_instruction;
use super::thinking::{
    has_valid_signature_for_function_calls, should_disable_thinking_due_to_history,
    should_enable_thinking_by_default,
};
use super::tools::build_tools;
use crate::proxy::mappers::claude::models::*;
use crate::proxy::session_manager::SessionManager;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Transform Claude request to Gemini v1internal format
pub fn transform_claude_request_in(
    claude_req: &ClaudeRequest,
    project_id: &str,
    is_retry: bool,
) -> Result<Value, String> {
    // Pre-clean all cache_control fields from messages
    let mut cleaned_req = claude_req.clone();

    // Merge consecutive same-role messages
    merge_consecutive_messages(&mut cleaned_req.messages);

    clean_cache_control_from_messages(&mut cleaned_req.messages);

    // Pre-sort thinking blocks to be first in assistant messages
    sort_thinking_blocks_first(&mut cleaned_req.messages);

    let claude_req = &cleaned_req;

    // Generate session ID for signature tracking
    let session_id = SessionManager::extract_session_id(claude_req);
    tracing::debug!("[Claude-Request] Session ID: {}", session_id);

    // Detect web search tool
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

    // Tool ID to name mapping
    let mut tool_id_to_name: HashMap<String, String> = HashMap::new();

    // Detect MCP tools
    let has_mcp_tools = claude_req
        .tools
        .as_ref()
        .map(|tools| {
            tools.iter().any(|t| {
                t.name
                    .as_deref()
                    .map(|n| n.starts_with("mcp__"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    // Build tool name to schema mapping
    let mut tool_name_to_schema = HashMap::new();
    if let Some(tools) = &claude_req.tools {
        for tool in tools {
            if let (Some(name), Some(schema)) = (&tool.name, &tool.input_schema) {
                tool_name_to_schema.insert(name.clone(), schema.clone());
            }
        }
    }

    // Build System Instruction
    let system_instruction =
        build_system_instruction(&claude_req.system, &claude_req.model, has_mcp_tools);

    // Map model name
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

    // Convert tools to Value array for grounding detection
    let tools_val: Option<Vec<Value>> = claude_req.tools.as_ref().map(|list| {
        list.iter()
            .map(|t| serde_json::to_value(t).unwrap_or(json!({})))
            .collect()
    });

    // Resolve request config
    let config = crate::proxy::mappers::common_utils::resolve_request_config(
        &claude_req.model,
        &mapped_model,
        &tools_val,
        claude_req.size.as_deref(),
        claude_req.quality.as_deref(),
    );

    // Disable dummy thought injection for Vertex AI
    let allow_dummy_thought = false;

    // Check if thinking is enabled
    let mut is_thinking_enabled = claude_req
        .thinking
        .as_ref()
        .map(|t| t.type_ == "enabled")
        .unwrap_or_else(|| should_enable_thinking_by_default(&claude_req.model));

    // Check if target model supports thinking
    let mapped_model_lower = mapped_model.to_lowercase();
    let target_model_supports_thinking = mapped_model_lower.contains("-thinking")
        || mapped_model_lower.starts_with("claude-")
        || mapped_model_lower.contains("gemini-2.0-pro")
        || mapped_model_lower.contains("gemini-3-pro");

    if is_thinking_enabled && !target_model_supports_thinking {
        tracing::warn!(
            "[Thinking-Mode] Target model '{}' does not support thinking. Force disabling.",
            mapped_model
        );
        is_thinking_enabled = false;
    }

    // Smart downgrade: check if history is compatible with thinking
    if is_thinking_enabled {
        let should_disable = should_disable_thinking_due_to_history(&claude_req.messages);
        if should_disable {
            tracing::warn!(
                "[Thinking-Mode] Automatically disabling thinking due to incompatible tool-use history"
            );
            is_thinking_enabled = false;
        }
    }

    // Check signature availability for function calls
    if is_thinking_enabled {
        let global_sig = crate::proxy::SignatureCache::global().get_session_signature(&session_id);

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

        let has_function_calls = claude_req.messages.iter().any(|m| {
            if let MessageContent::Array(blocks) = &m.content {
                blocks
                    .iter()
                    .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
            } else {
                false
            }
        });

        let needs_signature_check = has_function_calls;

        if !has_thinking_history && is_thinking_enabled {
            tracing::info!(
                "[Thinking-Mode] First thinking request detected. Using permissive mode."
            );
        }

        if needs_signature_check
            && !has_valid_signature_for_function_calls(
                &claude_req.messages,
                &global_sig,
                &session_id,
            )
        {
            tracing::warn!(
                "[Thinking-Mode] No valid signature found for function calls. Disabling thinking."
            );
            is_thinking_enabled = false;
        }
    }

    // Build Generation Config
    let generation_config =
        build_generation_config(claude_req, has_web_search_tool, is_thinking_enabled);

    // Build Contents
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
    )?;

    // Build Tools
    let tools = build_tools(&claude_req.tools, has_web_search_tool)?;

    // Build Safety Settings
    let safety_settings = build_safety_settings();

    // Build inner request
    let mut inner_request = json!({
        "contents": contents,
        "safetySettings": safety_settings,
    });

    // Deep clean undefined strings
    crate::proxy::mappers::common_utils::deep_clean_undefined(&mut inner_request);

    if let Some(sys_inst) = system_instruction {
        inner_request["systemInstruction"] = sys_inst;
    }

    if !generation_config.is_null() {
        inner_request["generationConfig"] = generation_config;
    }

    if let Some(tools_val) = tools {
        inner_request["tools"] = tools_val;
        inner_request["toolConfig"] = json!({
            "functionCallingConfig": {
                "mode": "VALIDATED"
            }
        });
    }

    // Inject googleSearch tool if needed
    if config.inject_google_search && !has_web_search_tool {
        crate::proxy::mappers::common_utils::inject_google_search_tool(&mut inner_request);
    }

    // Inject imageConfig if present
    if let Some(image_config) = config.image_config {
        if let Some(obj) = inner_request.as_object_mut() {
            obj.remove("tools");
            obj.remove("systemInstruction");

            let gen_config = obj.entry("generationConfig").or_insert_with(|| json!({}));
            if let Some(gen_obj) = gen_config.as_object_mut() {
                gen_obj.remove("thinkingConfig");
                gen_obj.remove("responseMimeType");
                gen_obj.remove("responseModalities");
                gen_obj.insert("imageConfig".to_string(), image_config);
            }
        }
    }

    // Generate requestId
    let request_id = format!("agent-{}", uuid::Uuid::new_v4());

    // Build final request body
    let mut body = json!({
        "project": project_id,
        "requestId": request_id,
        "request": inner_request,
        "model": config.final_model,
        "userAgent": "antigravity",
        "requestType": config.request_type,
    });

    // Add sessionId from metadata if available
    if let Some(metadata) = &claude_req.metadata {
        if let Some(user_id) = &metadata.user_id {
            body["request"]["sessionId"] = json!(user_id);
        }
    }

    // Final deep clean of all cache_control fields
    deep_clean_cache_control(&mut body);
    tracing::debug!("[DEBUG-593] Final deep clean complete, request ready to send");

    Ok(body)
}
