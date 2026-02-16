// OpenAI Stream Collector
// Used for auto-converting streaming responses to JSON for non-streaming requests

use super::models::*;
use bytes::Bytes;
use futures::StreamExt;
use serde_json::Value;
use std::collections::HashMap;

/// Collects an OpenAI SSE stream into a complete OpenAIResponse
pub async fn collect_stream_to_json<S, E>(
    mut stream: S,
) -> Result<OpenAIResponse, String>
where
    S: futures::Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    let mut response = OpenAIResponse {
        id: "chatcmpl-unknown".to_string(),
        object: "chat.completion".to_string(),
        created: chrono::Utc::now().timestamp() as u64,
        model: "unknown".to_string(),
        choices: Vec::new(),
        usage: None,
    };

    let mut role: Option<String> = None;
    let mut content_parts: Vec<String> = Vec::new();
    let mut reasoning_parts: Vec<String> = Vec::new();
    let mut finish_reason: Option<String> = None;
    // Tool calls aggregation: index -> (id, type, name, arguments_parts)
    let mut tool_calls_map: HashMap<u32, (String, String, String, Vec<String>)> = HashMap::new();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("Stream error: {}", e))?;
        let text = String::from_utf8_lossy(&chunk);

        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("data: ") {
                let data_str = line.trim_start_matches("data: ").trim();
                if data_str == "[DONE]" {
                    continue;
                }

                if let Ok(json) = serde_json::from_str::<Value>(data_str) {
                    // Update meta fields
                    if let Some(id) = json.get("id").and_then(|v| v.as_str()) {
                        response.id = id.to_string();
                    }
                    if let Some(model) = json.get("model").and_then(|v| v.as_str()) {
                        response.model = model.to_string();
                    }
                    if let Some(created) = json.get("created").and_then(|v| v.as_u64()) {
                        response.created = created;
                    }

                    // Collect Usage
                    if let Some(usage) = json.get("usage") {
                        if let Ok(u) = serde_json::from_value::<OpenAIUsage>(usage.clone()) {
                            response.usage = Some(u);
                        }
                    }

                    // Collect Choices Delta
                    if let Some(choices) = json.get("choices").and_then(|v| v.as_array()) {
                        if let Some(choice) = choices.first() {
                            if let Some(delta) = choice.get("delta") {
                                // Role
                                if let Some(r) = delta.get("role").and_then(|v| v.as_str()) {
                                    role = Some(r.to_string());
                                }
                                
                                // Content
                                if let Some(c) = delta.get("content").and_then(|v| v.as_str()) {
                                    content_parts.push(c.to_string());
                                }

                                // Reasoning Content
                                if let Some(rc) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
                                    reasoning_parts.push(rc.to_string());
                                }

                                // Tool Calls aggregation by index
                                // [FIX] When multiple tool calls arrive with the same index but
                                // different IDs, treat them as SEPARATE tool calls instead of
                                // merging into one (which would concatenate their arguments).
                                if let Some(tcs) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                                    for tc in tcs {
                                        let raw_index = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                                        let new_id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");

                                        // If this index already has a DIFFERENT id, it's a new tool call
                                        // Assign it a unique index to avoid merging
                                        let index = if !new_id.is_empty() {
                                            if let Some(existing) = tool_calls_map.get(&raw_index) {
                                                if !existing.0.is_empty() && existing.0 != new_id {
                                                    // Find next available index
                                                    let mut next_idx = raw_index + 1;
                                                    while tool_calls_map.contains_key(&next_idx) {
                                                        next_idx += 1;
                                                    }
                                                    next_idx
                                                } else {
                                                    raw_index
                                                }
                                            } else {
                                                raw_index
                                            }
                                        } else {
                                            raw_index
                                        };

                                        let entry = tool_calls_map.entry(index).or_insert_with(|| {
                                            (String::new(), String::from("function"), String::new(), Vec::new())
                                        });
                                        
                                        if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                                            if !id.is_empty() {
                                                entry.0 = id.to_string();
                                            }
                                        }
                                        
                                        if let Some(tc_type) = tc.get("type").and_then(|v| v.as_str()) {
                                            if !tc_type.is_empty() {
                                                entry.1 = tc_type.to_string();
                                            }
                                        }
                                        
                                        if let Some(func) = tc.get("function") {
                                            if let Some(name) = func.get("name").and_then(|v| v.as_str()) {
                                                if !name.is_empty() {
                                                    entry.2 = name.to_string();
                                                }
                                            }
                                            if let Some(args) = func.get("arguments").and_then(|v| v.as_str()) {
                                                entry.3.push(args.to_string());
                                            }
                                        }
                                    }
                                }
                            }

                            if let Some(fr) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                                finish_reason = Some(fr.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // Construct final message
    let full_content = content_parts.join("");
    let full_reasoning = if reasoning_parts.is_empty() {
        None
    } else {
        Some(reasoning_parts.join(""))
    };

    // Build aggregated tool_calls
    let final_tool_calls: Option<Vec<ToolCall>> = if tool_calls_map.is_empty() {
        None
    } else {
        let mut calls: Vec<(u32, ToolCall)> = tool_calls_map
            .into_iter()
            .map(|(index, (id, tc_type, name, args_parts))| {
                (index, ToolCall {
                    id,
                    r#type: tc_type,
                    function: ToolFunction {
                        name,
                        arguments: args_parts.join(""),
                    },
                })
            })
            .collect();
        calls.sort_by_key(|(index, _)| *index);
        Some(calls.into_iter().map(|(_, tc)| tc).collect())
    };

    let message = OpenAIMessage {
        role: role.unwrap_or("assistant".to_string()),
        content: Some(OpenAIContent::String(full_content)),
        reasoning_content: full_reasoning,
        tool_calls: final_tool_calls,
        tool_call_id: None,
        name: None,
    };

    response.choices.push(Choice {
        index: 0,
        message,
        finish_reason: finish_reason.or(Some("stop".to_string())),
    });

    Ok(response)
}
