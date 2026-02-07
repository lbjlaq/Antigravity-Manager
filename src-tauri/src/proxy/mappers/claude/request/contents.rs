// Contents Builder - Message transformation logic
// Converts Claude messages to Gemini v1internal format

use super::thinking::{is_model_compatible, MIN_SIGNATURE_LENGTH};
use crate::proxy::mappers::claude::models::*;
use crate::proxy::mappers::tool_result_compressor;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// Build parts from message content
pub fn build_contents(
    content: &MessageContent,
    is_assistant: bool,
    _claude_req: &ClaudeRequest,
    is_thinking_enabled: bool,
    session_id: &str,
    allow_dummy_thought: bool,
    is_retry: bool,
    tool_id_to_name: &mut HashMap<String, String>,
    tool_name_to_schema: &HashMap<String, Value>,
    mapped_model: &str,
    last_thought_signature: &mut Option<String>,
    pending_tool_use_ids: &mut Vec<String>,
    last_user_task_text_normalized: &mut Option<String>,
    previous_was_tool_result: &mut bool,
    _existing_tool_result_ids: &HashSet<String>,
) -> Result<Vec<Value>, String> {
    let mut parts = Vec::new();
    let mut current_turn_tool_result_ids = HashSet::new();
    let mut saw_non_thinking = false;

    match content {
        MessageContent::String(text) => {
            if text != "(no content)" && !text.trim().is_empty() {
                parts.push(json!({"text": text.trim()}));
            }
        }
        MessageContent::Array(blocks) => {
            for item in blocks {
                match item {
                    ContentBlock::Text { text } => {
                        if text != "(no content)" {
                            // Task deduplication: skip if identical to previous user task
                            if !is_assistant && *previous_was_tool_result {
                                if let Some(last_task) = last_user_task_text_normalized {
                                    let current_normalized =
                                        text.replace(|c: char| c.is_whitespace(), "");
                                    if !current_normalized.is_empty()
                                        && current_normalized == *last_task
                                    {
                                        tracing::info!(
                                            "[Claude-Request] Dropping duplicated task text echo (len: {})",
                                            text.len()
                                        );
                                        continue;
                                    }
                                }
                            }

                            parts.push(json!({"text": text}));
                            saw_non_thinking = true;

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

                        // Thinking block MUST be first
                        if saw_non_thinking || !parts.is_empty() {
                            tracing::warn!(
                                "[Claude-Request] Thinking block found at non-zero index. Downgrading to Text."
                            );
                            if !thinking.is_empty() {
                                parts.push(json!({"text": thinking}));
                                saw_non_thinking = true;
                            }
                            continue;
                        }

                        // If thinking disabled, convert to text
                        if !is_thinking_enabled {
                            tracing::warn!(
                                "[Claude-Request] Thinking disabled. Downgrading thinking block to text."
                            );
                            if !thinking.is_empty() {
                                parts.push(json!({"text": thinking}));
                            }
                            continue;
                        }

                        // Empty thinking blocks cause errors
                        if thinking.is_empty() {
                            tracing::warn!(
                                "[Claude-Request] Empty thinking block detected. Downgrading to Text."
                            );
                            parts.push(json!({"text": "..."}));
                            continue;
                        }

                        // Signature validation
                        if let Some(sig) = signature {
                            if sig.len() < MIN_SIGNATURE_LENGTH {
                                tracing::warn!(
                                    "[Thinking-Signature] Signature too short (len: {} < {}), downgrading to text.",
                                    sig.len(),
                                    MIN_SIGNATURE_LENGTH
                                );
                                parts.push(json!({"text": thinking}));
                                saw_non_thinking = true;
                                continue;
                            }

                            let cached_family =
                                crate::proxy::SignatureCache::global().get_signature_family(sig);

                            match cached_family {
                                Some(family) => {
                                    let compatible =
                                        !is_retry && is_model_compatible(&family, mapped_model);

                                    if !compatible {
                                        tracing::warn!(
                                            "[Thinking-Signature] {} signature (Family: {}, Target: {}). Downgrading to text.",
                                            if is_retry { "Stripping historical" } else { "Incompatible" },
                                            family,
                                            mapped_model
                                        );
                                        parts.push(json!({"text": thinking}));
                                        saw_non_thinking = true;
                                        continue;
                                    }
                                    *last_thought_signature = Some(sig.clone());
                                    let mut part = json!({
                                        "text": thinking,
                                        "thought": true,
                                        "thoughtSignature": sig
                                    });
                                    crate::proxy::common::json_schema::clean_json_schema(&mut part);
                                    parts.push(part);
                                }
                                None => {
                                    if sig.len() >= MIN_SIGNATURE_LENGTH {
                                        tracing::debug!(
                                            "[Thinking-Signature] Unknown signature origin but valid length (len: {}), using as-is.",
                                            sig.len()
                                        );
                                        *last_thought_signature = Some(sig.clone());
                                        let mut part = json!({
                                            "text": thinking,
                                            "thought": true,
                                            "thoughtSignature": sig
                                        });
                                        crate::proxy::common::json_schema::clean_json_schema(
                                            &mut part,
                                        );
                                        parts.push(part);
                                    } else {
                                        tracing::warn!(
                                            "[Thinking-Signature] Unknown signature origin and too short (len: {}). Downgrading to text.",
                                            sig.len()
                                        );
                                        parts.push(json!({"text": thinking}));
                                        saw_non_thinking = true;
                                        continue;
                                    }
                                }
                            }
                        } else {
                            tracing::warn!(
                                "[Thinking-Signature] No signature provided. Downgrading to text."
                            );
                            parts.push(json!({"text": thinking}));
                            saw_non_thinking = true;
                        }
                    }
                    ContentBlock::RedactedThinking { data } => {
                        tracing::debug!("[Claude-Request] Degrade RedactedThinking to text");
                        parts.push(json!({
                            "text": format!("[Redacted Thinking: {}]", data)
                        }));
                        saw_non_thinking = true;
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
                            saw_non_thinking = true;
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

                        // Fix tool call args using schema
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

                        if is_assistant {
                            pending_tool_use_ids.push(id.clone());
                        }

                        tool_id_to_name.insert(id.clone(), name.clone());

                        // Signature resolution: Client -> Context -> Session -> Tool -> Global
                        let final_sig = signature
                            .as_ref()
                            .or(last_thought_signature.as_ref())
                            .cloned()
                            .or_else(|| {
                                crate::proxy::SignatureCache::global()
                                    .get_session_signature(session_id)
                                    .map(|s| {
                                        tracing::info!(
                                            "[Claude-Request] Recovered signature from SESSION cache (session: {}, len: {})",
                                            session_id,
                                            s.len()
                                        );
                                        s
                                    })
                            })
                            .or_else(|| {
                                crate::proxy::SignatureCache::global()
                                    .get_tool_signature(id)
                                    .map(|s| {
                                        tracing::info!(
                                            "[Claude-Request] Recovered signature from TOOL cache for tool_id: {}",
                                            id
                                        );
                                        s
                                    })
                            });

                        // Validate signature before using
                        if let Some(sig) = final_sig {
                            if is_retry && signature.is_none() {
                                tracing::warn!(
                                    "[Tool-Signature] Skipping signature backfill for tool_use: {} during retry.",
                                    id
                                );
                            } else if sig.len() < MIN_SIGNATURE_LENGTH {
                                tracing::warn!(
                                    "[Tool-Signature] Signature too short for tool_use: {} (len: {} < {}), skipping.",
                                    id,
                                    sig.len(),
                                    MIN_SIGNATURE_LENGTH
                                );
                            } else {
                                let cached_family = crate::proxy::SignatureCache::global()
                                    .get_signature_family(&sig);

                                let should_use_sig = match cached_family {
                                    Some(family) => {
                                        if is_model_compatible(&family, mapped_model) {
                                            true
                                        } else {
                                            tracing::warn!(
                                                "[Tool-Signature] Incompatible signature for tool_use: {} (Family: {}, Target: {})",
                                                id,
                                                family,
                                                mapped_model
                                            );
                                            false
                                        }
                                    }
                                    None => {
                                        if sig.len() >= MIN_SIGNATURE_LENGTH {
                                            tracing::debug!(
                                                "[Tool-Signature] Unknown signature origin but valid length for tool_use: {}, using as-is.",
                                                id
                                            );
                                            true
                                        } else if is_thinking_enabled {
                                            tracing::warn!(
                                                "[Tool-Signature] Unknown signature origin and too short for tool_use: {}. Dropping in thinking mode.",
                                                id
                                            );
                                            false
                                        } else {
                                            true
                                        }
                                    }
                                };
                                if should_use_sig {
                                    part["thoughtSignature"] = json!(sig);
                                }
                            }
                        } else {
                            // Handle missing signature for Gemini thinking models
                            let is_google_cloud = mapped_model.starts_with("projects/");
                            if is_thinking_enabled && !is_google_cloud {
                                tracing::debug!(
                                    "[Tool-Signature] Adding GEMINI_SKIP_SIGNATURE for tool_use: {}",
                                    id
                                );
                                part["thoughtSignature"] = json!("skip_thought_signature_validator");
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
                        current_turn_tool_result_ids.insert(tool_use_id.clone());
                        let func_name = tool_id_to_name
                            .get(tool_use_id)
                            .cloned()
                            .unwrap_or_else(|| tool_use_id.clone());

                        // Tool output compression
                        let mut compacted_content = content.clone();
                        if let Some(blocks) = compacted_content.as_array_mut() {
                            tool_result_compressor::sanitize_tool_result_blocks(blocks);
                        }

                        // Extract text content, remove images
                        let mut merged_content = match &compacted_content {
                            serde_json::Value::String(s) => s.clone(),
                            serde_json::Value::Array(arr) => arr
                                .iter()
                                .filter_map(|block| {
                                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                                        Some(text.to_string())
                                    } else if block.get("source").is_some() {
                                        if block.get("type").and_then(|v| v.as_str()) == Some("image")
                                        {
                                            Some("[image omitted to save context]".to_string())
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join("\n"),
                            _ => content.to_string(),
                        };

                        // Truncate long results
                        const MAX_TOOL_RESULT_CHARS: usize = 200_000;
                        if merged_content.len() > MAX_TOOL_RESULT_CHARS {
                            tracing::warn!(
                                "Truncating tool result from {} chars to {}",
                                merged_content.len(),
                                MAX_TOOL_RESULT_CHARS
                            );
                            let mut truncated = merged_content
                                .chars()
                                .take(MAX_TOOL_RESULT_CHARS)
                                .collect::<String>();
                            truncated.push_str("\n...[truncated output]");
                            merged_content = truncated;
                        }

                        // Handle empty results
                        if merged_content.trim().is_empty() {
                            merged_content = if is_error.unwrap_or(false) {
                                "Tool execution failed with no output.".to_string()
                            } else {
                                "Command executed successfully.".to_string()
                            };
                        }

                        parts.push(json!({
                            "functionResponse": {
                                "name": func_name,
                                "response": {"result": merged_content},
                                "id": tool_use_id
                            }
                        }));

                        // Backfill signature for tool result
                        if let Some(sig) = last_thought_signature.as_ref() {
                            if let Some(last_part) = parts.last_mut() {
                                last_part["thoughtSignature"] = json!(sig);
                            }
                        }

                        *previous_was_tool_result = true;
                    }
                    ContentBlock::ServerToolUse { .. } | ContentBlock::WebSearchToolResult { .. } => {
                        continue;
                    }
                }
            }
        }
    }

    // Inject missing tool results for user messages
    if !is_assistant && !pending_tool_use_ids.is_empty() {
        let missing_ids: Vec<_> = pending_tool_use_ids
            .iter()
            .filter(|id| !current_turn_tool_result_ids.contains(*id))
            .cloned()
            .collect();

        if !missing_ids.is_empty() {
            tracing::warn!(
                "[Elastic-Recovery] Injecting {} missing tool results into User message (IDs: {:?})",
                missing_ids.len(),
                missing_ids
            );
            for id in missing_ids.iter().rev() {
                let name = tool_id_to_name.get(id).cloned().unwrap_or(id.clone());
                let synthetic_part = json!({
                    "functionResponse": {
                        "name": name,
                        "response": {
                            "result": "Tool execution interrupted. No result provided."
                        },
                        "id": id
                    }
                });
                parts.insert(0, synthetic_part);
            }
        }
        pending_tool_use_ids.clear();
    }

    // Inject dummy thought block for assistant messages
    if allow_dummy_thought && is_assistant && is_thinking_enabled {
        let has_thought_part = parts.iter().any(|p| {
            p.get("thought").and_then(|v| v.as_bool()).unwrap_or(false)
                || p.get("thoughtSignature").is_some()
                || p.get("thought").and_then(|v| v.as_str()).is_some()
        });

        if !has_thought_part {
            parts.insert(
                0,
                json!({
                    "text": "Thinking...",
                    "thought": true
                }),
            );
            tracing::debug!(
                "Injected dummy thought block for historical assistant message at index {}",
                parts.len()
            );
        } else {
            let first_is_thought = parts.first().map_or(false, |p| {
                (p.get("thought").is_some() || p.get("thoughtSignature").is_some())
                    && p.get("text").is_some()
            });

            if !first_is_thought {
                parts.insert(
                    0,
                    json!({
                        "text": "...",
                        "thought": true
                    }),
                );
                tracing::debug!(
                    "First part of model message is not a valid thought block. Prepending dummy."
                );
            } else if let Some(p0) = parts.first_mut() {
                if p0.get("thought").is_none() {
                    p0.as_object_mut()
                        .map(|obj| obj.insert("thought".to_string(), json!(true)));
                }
            }
        }
    }

    Ok(parts)
}

/// Build a single Google content message
fn build_google_content(
    msg: &Message,
    claude_req: &ClaudeRequest,
    is_thinking_enabled: bool,
    session_id: &str,
    allow_dummy_thought: bool,
    is_retry: bool,
    tool_id_to_name: &mut HashMap<String, String>,
    tool_name_to_schema: &HashMap<String, Value>,
    mapped_model: &str,
    last_thought_signature: &mut Option<String>,
    pending_tool_use_ids: &mut Vec<String>,
    last_user_task_text_normalized: &mut Option<String>,
    previous_was_tool_result: &mut bool,
    existing_tool_result_ids: &HashSet<String>,
) -> Result<Value, String> {
    let role = if msg.role == "assistant" {
        "model"
    } else {
        &msg.role
    };

    // Proactive Tool Chain Repair
    if role == "model" && !pending_tool_use_ids.is_empty() {
        tracing::warn!(
            "[Elastic-Recovery] Detected interrupted tool chain. Injecting synthetic User message for IDs: {:?}",
            pending_tool_use_ids
        );

        let synthetic_parts: Vec<Value> = pending_tool_use_ids
            .iter()
            .filter(|id| !existing_tool_result_ids.contains(*id))
            .map(|id| {
                let name = tool_id_to_name.get(id).cloned().unwrap_or(id.clone());
                json!({
                    "functionResponse": {
                        "name": name,
                        "response": {
                            "result": "Tool execution interrupted. No result provided."
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
        pending_tool_use_ids.clear();
    }

    let parts = build_contents(
        &msg.content,
        msg.role == "assistant",
        claude_req,
        is_thinking_enabled,
        session_id,
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
        return Ok(json!(null));
    }

    Ok(json!({
        "role": role,
        "parts": parts
    }))
}

/// Build all Google contents from messages
pub fn build_google_contents(
    messages: &[Message],
    claude_req: &ClaudeRequest,
    tool_id_to_name: &mut HashMap<String, String>,
    tool_name_to_schema: &HashMap<String, Value>,
    is_thinking_enabled: bool,
    allow_dummy_thought: bool,
    mapped_model: &str,
    session_id: &str,
    is_retry: bool,
) -> Result<Value, String> {
    let mut contents = Vec::new();
    let mut last_thought_signature: Option<String> = None;
    let mut pending_tool_use_ids: Vec<String> = Vec::new();
    let mut last_user_task_text_normalized: Option<String> = None;
    let mut previous_was_tool_result = false;

    // Pre-scan all existing tool_result IDs
    let mut existing_tool_result_ids = HashSet::new();
    for msg in messages {
        if let MessageContent::Array(blocks) = &msg.content {
            for block in blocks {
                if let ContentBlock::ToolResult { tool_use_id, .. } = block {
                    existing_tool_result_ids.insert(tool_use_id.clone());
                }
            }
        }
    }

    for msg in messages.iter() {
        let google_content = build_google_content(
            msg,
            claude_req,
            is_thinking_enabled,
            session_id,
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

    // Merge adjacent messages with same role
    let mut merged_contents = merge_adjacent_roles(contents);

    // Deep cleanup if thinking disabled
    if !is_thinking_enabled {
        for msg in &mut merged_contents {
            super::cleanup::clean_thinking_fields_recursive(msg);
        }
    }

    Ok(json!(merged_contents))
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
            if let Some(current_parts) = current_msg.get_mut("parts").and_then(|p| p.as_array_mut())
            {
                if let Some(next_parts) = msg.get("parts").and_then(|p| p.as_array()) {
                    current_parts.extend(next_parts.clone());
                    super::sorting::reorder_gemini_parts(current_parts);
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
