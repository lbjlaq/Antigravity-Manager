// OpenAI 流式转换
use bytes::{Bytes, BytesMut};
use chrono::Utc;
use futures::{Stream, StreamExt};
use rand::Rng;
use serde_json::{json, Value};
use std::pin::Pin;
use tracing::debug;
use uuid::Uuid;

/// 保存 thoughtSignature 到会话缓存
pub fn store_thought_signature(sig: &str, session_id: &str, message_count: usize) {
    if sig.is_empty() {
        return;
    }

    // 2. [CRITICAL] 存储到 Session 隔离缓存 (对齐 Claude 协议)
    crate::proxy::SignatureCache::global().cache_session_signature(
        session_id,
        sig.to_string(),
        message_count,
    );

    tracing::debug!(
        "[ThoughtSig] 存储 Session 签名 (sid: {}, len: {}, msg_count: {})",
        session_id,
        sig.len(),
        message_count
    );
}

/// Extract and convert Gemini usageMetadata to OpenAI usage format
/// Supports both legacy v1internal format and new Interactions API format.
///
/// Key semantic difference:
/// - Old format: candidatesTokenCount = all output tokens (text + thinking + tool)
/// - New format: total_output_tokens = text + tool output only; thought tokens are separate (total_thought_tokens)
/// For Codex, we must sum them back together as `completion_tokens`.
fn extract_usage_metadata(u: &Value) -> Option<super::models::OpenAIUsage> {
    use super::models::{CompletionTokensDetails, OpenAIUsage, PromptTokensDetails};

    // 优先使用新格式字段，fallback 到旧格式
    let prompt_tokens = u
        .get("total_input_tokens")
        .or_else(|| u.get("promptTokenCount"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let raw_output_tokens = u
        .get("total_output_tokens")
        .or_else(|| u.get("candidatesTokenCount"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let raw_total_tokens = u
        .get("total_tokens")
        .or_else(|| u.get("totalTokenCount"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let cached_tokens = u
        .get("total_cached_tokens")
        .or_else(|| u.get("cachedContentTokenCount"))
        .or_else(|| u.get("cachedTokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let reasoning_tokens = u
        .get("total_thought_tokens")
        .or_else(|| u.get("totalThoughtTokens"))
        .or_else(|| u.get("thoughtsTokenCount"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let tool_use_tokens = u
        .get("total_tool_use_tokens")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let input_tokens_by_modality = u.get("input_tokens_by_modality").cloned();

    // 新格式下 output_tokens 不含 thought/tool-use, 需要加回来
    let completion_tokens =
        raw_output_tokens + reasoning_tokens.unwrap_or(0) + tool_use_tokens.unwrap_or(0);

    // cached_tokens is a subset of prompt_tokens. Keep prompt_tokens in the same
    // raw-input-token unit as Gemini usageMetadata so downstream logs can reconcile it.
    let final_total_tokens = raw_total_tokens.unwrap_or(prompt_tokens + completion_tokens);

    Some(OpenAIUsage {
        prompt_tokens,
        completion_tokens,
        total_tokens: final_total_tokens,
        prompt_tokens_details: cached_tokens.map(|ct| PromptTokensDetails {
            cached_tokens: Some(ct),
        }),
        completion_tokens_details: reasoning_tokens.map(|rt| CompletionTokensDetails {
            reasoning_tokens: Some(rt),
        }),
        input_tokens_by_modality,
        raw_output_tokens: Some(raw_output_tokens),
        total_thought_tokens: reasoning_tokens,
        total_tool_use_tokens: tool_use_tokens,
        gemini_total_tokens: raw_total_tokens,
    })
}

pub fn create_openai_sse_stream<S, E>(
    mut gemini_stream: Pin<Box<S>>,
    model: String,
    session_id: String,
    message_count: usize,
    client_tool_names: Option<std::collections::HashSet<String>>,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + ?Sized + 'static,
    E: std::fmt::Display + Send + 'static,
{
    let mut buffer = BytesMut::new();
    let stream_id = format!("chatcmpl-{}", Uuid::new_v4());
    let created_ts = Utc::now().timestamp();

    let empty_set = std::collections::HashSet::new();
    let client_tool_names = client_tool_names.unwrap_or(empty_set);

    let stream = async_stream::stream! {
        let mut emitted_tool_calls = std::collections::HashSet::new();
        let mut final_usage: Option<super::models::OpenAIUsage> = None;
        let mut error_occurred = false;
        let mut tool_call_index = 0;

        let mut heartbeat_interval = tokio::time::interval(std::time::Duration::from_secs(15));
        heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                item = gemini_stream.next() => {
                    match item {
                        Some(Ok(bytes)) => {
                            buffer.extend_from_slice(&bytes);
                            while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                                let line_raw = buffer.split_to(pos + 1);
                                if let Ok(line_str) = std::str::from_utf8(&line_raw) {
                                    let line = line_str.trim();
                                    if line.is_empty() { continue; }
                                    if line.starts_with("data: ") {
                                        let json_part = line.trim_start_matches("data: ").trim();
                                        if json_part == "[DONE]" { continue; }
                                        if let Ok(mut json) = serde_json::from_str::<Value>(json_part) {
                                            let actual_data = if let Some(inner) = json.get_mut("response").map(|v| v.take()) { inner } else { json };
                                            if let Some(u) = actual_data.get("usageMetadata") {
                                                final_usage = extract_usage_metadata(u);
                                            }

                                            if let Some(candidates) = actual_data.get("candidates").and_then(|c| c.as_array()) {
                                                // [DEBUG] 打印原始 candidate 以排查空回复问题
                                                if candidates.len() > 0 {
                                                     tracing::debug!("[Stream-Debug] Raw Candidate: {:?}", candidates[0]);
                                                }
                                                for (idx, candidate) in candidates.iter().enumerate() {
                                                    let parts = candidate.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array());
                                                    let mut content_out = String::new();
                                                    let mut thought_out = String::new();

                                                    if let Some(parts_list) = parts {
                                                        for part in parts_list {
                                                            let is_thought_part = part.get("thought").and_then(|v| v.as_bool()).unwrap_or(false);
                                                            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                                                let clean_text = text.replace("<think>\n", "").replace("<think>", "").replace("\n</think>", "").replace("</think>", "");
                                                                if is_thought_part {
                                                                    // thought 内容只写入 thought_out（给支持 reasoning_content 的客户端），防止客户端重复显示思维过程
                                                                    thought_out.push_str(&clean_text);
                                                                }
                                                                else { content_out.push_str(&clean_text); }
                                                            }
                                                            if let Some(sig) = part.get("thoughtSignature").or(part.get("thought_signature")).and_then(|s| s.as_str()) {
                                                                store_thought_signature(sig, &session_id, message_count);
                                                            }
                                                            if let Some(img) = part.get("inlineData") {
                                                                let mime_type = img.get("mimeType").and_then(|v| v.as_str()).unwrap_or("image/png");
                                                                let data = img.get("data").and_then(|v| v.as_str()).unwrap_or("");
                                                                if !data.is_empty() {
                                                                    content_out.push_str(&format!("![image](data:{};base64,{})", mime_type, data));
                                                                }
                                                            }
                                                            if let Some(func_call) = part.get("functionCall") {
                                                                let call_key = serde_json::to_string(func_call).unwrap_or_default();
                                                                if !emitted_tool_calls.contains(&call_key) {
                                                                    emitted_tool_calls.insert(call_key);
                                                                    let name = func_call.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                                                                    let mut args = func_call.get("args").unwrap_or(&json!({})).clone();

                                                                    // [FIX #1575] 标准化 shell 工具参数名称
                                                                    // Gemini 可能使用 cmd/code/script 等替代参数名，统一为 command
                                                                    if name == "shell" || name == "bash" || name == "local_shell" {
                                                                        if let Some(obj) = args.as_object_mut() {
                                                                            if !obj.contains_key("command") {
                                                                                for alt_key in &["cmd", "code", "script", "shell_command"] {
                                                                                    if let Some(val) = obj.remove(*alt_key) {
                                                                                        obj.insert("command".to_string(), val);
                                                                                        debug!("[OpenAI-Stream] Normalized shell arg '{}' -> 'command'", alt_key);
                                                                                        break;
                                                                                    }
                                                                                }
                                                                            }
                                                                        }
                                                                    }

                                                                    let final_name = super::response::resolve_shell_tool_name(name, &client_tool_names);

                                                                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                                                                    use std::hash::{Hash, Hasher};
                                                                    serde_json::to_string(func_call).unwrap_or_default().hash(&mut hasher);
                                                                    let call_id = format!("call_{:x}", hasher.finish());

                                                                    let args_str = serde_json::to_string(&args).unwrap_or_default();
                                                                    let tool_call_chunk = json!({
                                                                        "id": &stream_id,
                                                                        "object": "chat.completion.chunk",
                                                                        "created": created_ts,
                                                                        "model": &model,
                                                                        "choices": [{
                                                                            "index": idx as u32,
                                                                            "delta": {
                                                                                "role": "assistant",
                                                                                "tool_calls": [{
                                                                                    "index": tool_call_index,
                                                                                    "id": call_id,
                                                                                    "type": "function",
                                                                                    "function": { "name": final_name, "arguments": args_str }
                                                                                }]
                                                                            },
                                                                            "finish_reason": serde_json::Value::Null
                                                                        }]
                                                                    });

                                                                    tool_call_index += 1;
                                                                    let sse_out = format!("data: {}\n\n", serde_json::to_string(&tool_call_chunk).unwrap_or_default());
                                                                    yield Ok::<Bytes, String>(Bytes::from(sse_out));
                                                                }
                                                            }
                                                        }
                                                    }

                                                    if let Some(grounding) = candidate.get("groundingMetadata") {
                                                        let mut grounding_text = String::new();
                                                        if let Some(queries) = grounding.get("webSearchQueries").and_then(|q| q.as_array()) {
                                                            let query_list: Vec<&str> = queries.iter().filter_map(|v| v.as_str()).collect();
                                                            if !query_list.is_empty() {
                                                                grounding_text.push_str("\n\n---\n**🔍 已为您搜索：** ");
                                                                grounding_text.push_str(&query_list.join(", "));
                                                            }
                                                        }
                                                        if let Some(chunks) = grounding.get("groundingChunks").and_then(|c| c.as_array()) {
                                                            let mut links = Vec::new();
                                                            for (i, chunk) in chunks.iter().enumerate() {
                                                                if let Some(web) = chunk.get("web") {
                                                                    let title = web.get("title").and_then(|v| v.as_str()).unwrap_or("网页来源");
                                                                    let uri = web.get("uri").and_then(|v| v.as_str()).unwrap_or("#");
                                                                    links.push(format!("[{}] [{}]({})", i + 1, title, uri));
                                                                }
                                                            }
                                                            if !links.is_empty() {
                                                                grounding_text.push_str("\n\n**🌐 来源引文：**\n");
                                                                grounding_text.push_str(&links.join("\n"));
                                                            }
                                                        }
                                                        if !grounding_text.is_empty() { content_out.push_str(&grounding_text); }
                                                    }

                                                    let gemini_finish_reason = candidate.get("finishReason").and_then(|f| f.as_str()).map(|f| match f {
                                                        "STOP" => "stop",
                                                        "MAX_TOKENS" => "length",
                                                        "SAFETY" => "content_filter",
                                                        "RECITATION" => "content_filter",
                                                        _ => f,
                                                    });

                                                    // [FIX #1575] 如果发射了工具调用，强制设置为 tool_calls
                                                    // 解决 Gemini 返回 STOP 但有工具调用时，OpenAI 客户端认为对话已结束的问题
                                                    let finish_reason = if !emitted_tool_calls.is_empty() && gemini_finish_reason.is_some() {
                                                        Some("tool_calls")
                                                    } else {
                                                        gemini_finish_reason
                                                    };

                                                    if !thought_out.is_empty() {
                                                        let reasoning_chunk = json!({
                                                            "id": &stream_id,
                                                            "object": "chat.completion.chunk",
                                                            "created": created_ts,
                                                            "model": &model,
                                                            "choices": [{
                                                                "index": idx as u32,
                                                                "delta": { "role": "assistant", "content": serde_json::Value::Null, "reasoning_content": thought_out },
                                                                "finish_reason": serde_json::Value::Null
                                                            }]
                                                        });
                                                        let sse_out = format!("data: {}\n\n", serde_json::to_string(&reasoning_chunk).unwrap_or_default());
                                                        yield Ok::<Bytes, String>(Bytes::from(sse_out));
                                                    }

                                                    if !content_out.is_empty() || finish_reason.is_some() {
                                                        let mut openai_chunk = json!({
                                                            "id": &stream_id,
                                                            "object": "chat.completion.chunk",
                                                            "created": created_ts,
                                                            "model": &model,
                                                            "choices": [{
                                                                "index": idx as u32,
                                                                "delta": { "content": content_out },
                                                                "finish_reason": finish_reason
                                                            }]
                                                        });
                                                        if finish_reason.is_some() {
                                                            if let Some(ref usage) = final_usage {
                                                                openai_chunk["usage"] = serde_json::to_value(usage).unwrap();
                                                            }
                                                        }
                                                        if finish_reason.is_some() { final_usage = None; }
                                                        let sse_out = format!("data: {}\n\n", serde_json::to_string(&openai_chunk).unwrap_or_default());
                                                        yield Ok::<Bytes, String>(Bytes::from(sse_out));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Some(Err(e)) => {
                            use crate::proxy::mappers::error_classifier::classify_stream_error;
                            let (error_type, user_msg, i18n_key) = classify_stream_error(&e);
                            tracing::error!("OpenAI Stream Error: {}", e);
                            let error_chunk = json!({
                                "id": &stream_id, "object": "chat.completion.chunk", "created": created_ts, "model": &model, "choices": [],
                                "error": { "type": error_type, "message": user_msg, "code": "stream_error", "i18n_key": i18n_key }
                            });
                            yield Ok(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&error_chunk).unwrap_or_default())));
                            yield Ok(Bytes::from("data: [DONE]\n\n"));
                            error_occurred = true;
                            break;
                        }
                        None => break,
                    }
                }
                _ = heartbeat_interval.tick() => {
                    yield Ok::<Bytes, String>(Bytes::from(": ping\n\n"));
                }
            }
        }

        // [FIX #1732] Flush remaining buffer to prevent hang on network fragmentation
        if !buffer.is_empty() {
            if let Ok(line_str) = std::str::from_utf8(&buffer) {
                let line = line_str.trim();
                if !line.is_empty() && line.starts_with("data: ") {
                    let json_part = line.trim_start_matches("data: ").trim();
                    if json_part != "[DONE]" {
                        // Re-use logic for processing the last line
                        // (Note: In a more complex refactor we'd extract this to a function,
                        // but for a targeted fix, processing the terminal data chunk is safer)
                        tracing::debug!("[OpenAI-SSE] Flushing remaining {} bytes in buffer", buffer.len());
                    }
                }
            }
        }

        if !error_occurred {
            yield Ok::<Bytes, String>(Bytes::from("data: [DONE]\n\n"));
        }
    };
    Box::pin(stream)
}

pub fn create_legacy_sse_stream<S, E>(
    mut gemini_stream: Pin<Box<S>>,
    model: String,
    session_id: String,
    message_count: usize,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + ?Sized + 'static,
    E: std::fmt::Display + Send + 'static,
{
    let mut buffer = BytesMut::new();
    let charset = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    let random_str: String = (0..28)
        .map(|_| {
            let idx = rng.gen_range(0..charset.len());
            charset.chars().nth(idx).unwrap()
        })
        .collect();
    let stream_id = format!("cmpl-{}", random_str);
    let created_ts = Utc::now().timestamp();

    let stream = async_stream::stream! {
        let mut final_usage: Option<super::models::OpenAIUsage> = None;
        let mut error_occurred = false;
        let mut heartbeat_interval = tokio::time::interval(std::time::Duration::from_secs(15));
        heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                item = gemini_stream.next() => {
                    match item {
                        Some(Ok(bytes)) => {
                            buffer.extend_from_slice(&bytes);
                            while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                                let line_raw = buffer.split_to(pos + 1);
                                if let Ok(line_str) = std::str::from_utf8(&line_raw) {
                                    let line = line_str.trim();
                                    if line.is_empty() { continue; }
                                    if line.starts_with("data: ") {
                                        let json_part = line.trim_start_matches("data: ").trim();
                                        if json_part == "[DONE]" { continue; }
                                        if let Ok(mut json) = serde_json::from_str::<Value>(json_part) {
                                            let actual_data = if let Some(inner) = json.get_mut("response").map(|v| v.take()) { inner } else { json };
                                            if let Some(u) = actual_data.get("usageMetadata") { final_usage = extract_usage_metadata(u); }

                                            let mut content_out = String::new();
                                            if let Some(candidates) = actual_data.get("candidates").and_then(|c| c.as_array()) {
                                                if let Some(candidate) = candidates.get(0) {
                                                    if let Some(parts) = candidate.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                                                        for part in parts {
                                                            let is_thought = part.get("thought").and_then(|v| v.as_bool()).unwrap_or(false);
                                                            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                                                let clean_text = text.replace("<think>\n", "").replace("<think>", "").replace("\n</think>", "").replace("</think>", "");
                                                                content_out.push_str(&clean_text);
                                                            }
                                                            if let Some(sig) = part.get("thoughtSignature").or(part.get("thought_signature")).and_then(|s| s.as_str()) {
                                                                store_thought_signature(sig, &session_id, message_count);
                                                            }
                                                        }
                                                    }
                                                }
                                            }

                                            let finish_reason = actual_data.get("candidates").and_then(|c| c.as_array()).and_then(|c| c.get(0)).and_then(|c| c.get("finishReason")).and_then(|f| f.as_str()).map(|f| match f {
                                                "STOP" => "stop", "MAX_TOKENS" => "length", "SAFETY" => "content_filter", _ => f,
                                            });

                                            let mut legacy_chunk = json!({
                                                "id": &stream_id, "object": "text_completion", "created": created_ts, "model": &model,
                                                "choices": [{ "text": content_out, "index": 0, "logprobs": null, "finish_reason": finish_reason }]
                                            });
                                            if let Some(ref usage) = final_usage { legacy_chunk["usage"] = serde_json::to_value(usage).unwrap(); }
                                            if finish_reason.is_some() { final_usage = None; }
                                            yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&legacy_chunk).unwrap_or_default())));
                                        }
                                    }
                                }
                            }
                        }
                        Some(Err(e)) => {
                            use crate::proxy::mappers::error_classifier::classify_stream_error;
                            let (error_type, user_msg, i18n_key) = classify_stream_error(&e);
                            tracing::error!("Legacy Stream Error: {}", e);
                            let error_chunk = json!({
                                "id": &stream_id, "object": "text_completion", "created": created_ts, "model": &model, "choices": [],
                                "error": { "type": error_type, "message": user_msg, "code": "stream_error", "i18n_key": i18n_key }
                            });
                            yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&error_chunk).unwrap_or_default())));
                            yield Ok::<Bytes, String>(Bytes::from("data: [DONE]\n\n"));
                            error_occurred = true;
                            break;
                        }
                        None => break,
                    }
                }
                _ = heartbeat_interval.tick() => { yield Ok::<Bytes, String>(Bytes::from(": ping\n\n")); }
            }
        }
        if !error_occurred {
            yield Ok::<Bytes, String>(Bytes::from("data: [DONE]\n\n"));
        }
    };
    Box::pin(stream)
}

fn split_namespace_tool_name(qualified_name: &str) -> (String, Option<String>) {
    let name = qualified_name.trim();
    if name.starts_with("mcp__") {
        return (name.to_string(), None);
    }
    if let Some(pos) = name.find("__") {
        if pos > 0 {
            let namespace = name[..pos].to_string();
            let actual_name = name[pos + 2..].to_string();
            return (actual_name, Some(namespace));
        }
    }
    (name.to_string(), None)
}

fn extract_apply_patch_input(args: &Value) -> String {
    if let Some(obj) = args.as_object() {
        if let Some(input) = obj.get("input").and_then(|v| v.as_str()) {
            return input.to_string();
        }
        if let Some(arr) = obj.get("command").and_then(|v| v.as_array()) {
            if arr.len() > 1 {
                if let Some(patch) = arr[1].as_str() {
                    return patch.to_string();
                }
            }
        }
        if let Some(cmd_str) = obj.get("command").and_then(|v| v.as_str()) {
            if let Some(patch) = cmd_str.strip_prefix("apply_patch\n") {
                return patch.to_string();
            }
            if let Some(patch) = cmd_str.strip_prefix("apply_patch ") {
                return patch.to_string();
            }
            return cmd_str.to_string();
        }
        for key in ["patch_text", "patch", "diff", "content"] {
            if let Some(patch) = obj.get(key).and_then(|v| v.as_str()) {
                return patch.to_string();
            }
        }
    }
    args.as_str()
        .map(str::to_string)
        .unwrap_or_else(|| serde_json::to_string(args).unwrap_or_default())
}

pub fn create_codex_sse_stream<S, E>(
    mut gemini_stream: Pin<Box<S>>,
    _model: String,
    session_id: String,
    message_count: usize,
    assistant_turn_index: usize,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + ?Sized + 'static,
    E: std::fmt::Display + Send + 'static,
{
    let mut buffer = BytesMut::new();
    let charset = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    let random_str: String = (0..24)
        .map(|_| {
            let idx = rng.gen_range(0..charset.len());
            charset.chars().nth(idx).unwrap()
        })
        .collect();
    let response_id = format!("resp-{}", random_str);
    let message_item_id = format!("msg_{}_0", &random_str[..16]);

    let stream = async_stream::stream! {
        // 1. response.created
        let created_ev = json!({ "type": "response.created", "response": { "id": &response_id, "object": "response", "status": "in_progress", "output": [] } });
        yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&created_ev).unwrap())));

        let mut message_item_emitted = false;
        let mut in_thought_block = false;
        let mut emitted_thought_header = false;

        let mut emitted_tool_calls = std::collections::HashSet::new();
        let mut accumulated_text = String::new();
        let mut accumulated_thinking = String::new();

        let mut final_outputs_map: std::collections::BTreeMap<u32, serde_json::Value> = std::collections::BTreeMap::new();
        let mut next_output_index: u32 = 0;
        let mut message_output_index: u32 = 0;
        let mut final_usage: Option<super::models::OpenAIUsage> = None;
        let mut heartbeat_interval = tokio::time::interval(std::time::Duration::from_secs(15));
        heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                item = gemini_stream.next() => {
                    match item {
                        Some(Ok(bytes)) => {
                            buffer.extend_from_slice(&bytes);
                            while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                                let line_raw = buffer.split_to(pos + 1);
                                if let Ok(line_str) = std::str::from_utf8(&line_raw) {
                                    let line = line_str.trim();
                                    if line.is_empty() || !line.starts_with("data: ") { continue; }
                                    let json_part = line.trim_start_matches("data: ").trim();
                                    if json_part == "[DONE]" { continue; }

                                    if let Ok(mut json) = serde_json::from_str::<Value>(json_part) {
                                        let actual_data = if let Some(inner) = json.get_mut("response").map(|v| v.take()) { inner } else { json };

                                        if let Some(u) = actual_data.get("usageMetadata") {
                                            final_usage = extract_usage_metadata(u);
                                        }

                                        if let Some(candidates) = actual_data.get("candidates").and_then(|c| c.as_array()) {
                                            if candidates.len() > 0 {
                                                tracing::debug!("[Codex-Stream-Debug] Raw Candidate: {:?}", candidates[0]);
                                            }
                                            if let Some(candidate) = candidates.get(0) {
                                                if let Some(parts) = candidate.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                                                    for part in parts {
                                                        let is_thought = part.get("thought").and_then(|v| v.as_bool()).unwrap_or(false);
                                                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                                            let clean_text = text.replace("<think>\n", "").replace("<think>", "").replace("\n</think>", "").replace("</think>", "");
                                                            if !clean_text.is_empty() {
                                                                if !message_item_emitted {
                                                                    message_item_emitted = true;
                                                                    message_output_index = next_output_index;
                                                                    next_output_index += 1;
                                                                    let output_item_added = json!({"type": "response.output_item.added", "output_index": message_output_index, "item": {"id": &message_item_id, "type": "message", "role": "assistant", "phase": "commentary", "status": "in_progress", "content": []}});
                                                                    yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&output_item_added).unwrap())));
                                                                    let content_part_added = json!({"type": "response.content_part.added", "item_id": &message_item_id, "output_index": message_output_index, "content_index": 0, "part": {"type": "output_text", "text": ""}});
                                                                    yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&content_part_added).unwrap())));
                                                                }

                                                                let mut chunk_to_emit = String::new();

                                                                if is_thought {
                                                                    accumulated_thinking.push_str(&clean_text);
                                                                    if !in_thought_block {
                                                                        in_thought_block = true;
                                                                        if !emitted_thought_header {
                                                                            emitted_thought_header = true;
                                                                        } else {
                                                                            chunk_to_emit.push_str("\n> ");
                                                                        }
                                                                    }
                                                                    chunk_to_emit.push_str(&clean_text.replace("\n", "\n> "));
                                                                } else {
                                                                    if in_thought_block {
                                                                        in_thought_block = false;
                                                                        chunk_to_emit.push_str("\n\n");
                                                                    }
                                                                    chunk_to_emit.push_str(&clean_text);
                                                                }

                                                                accumulated_text.push_str(&chunk_to_emit);

                                                                let delta_ev = json!({
                                                                    "type": "response.output_text.delta",
                                                                    "item_id": &message_item_id,
                                                                    "output_index": message_output_index,
                                                                    "content_index": 0,
                                                                    "delta": chunk_to_emit
                                                                });
                                                                yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&delta_ev).unwrap())));
                                                            }
                                                        }
                                                        if let Some(sig) = part.get("thoughtSignature").or(part.get("thought_signature")).and_then(|s| s.as_str()) {
                                                            store_thought_signature(sig, &session_id, message_count);
                                                        }
                                                        if let Some(func_call) = part.get("functionCall") {
                                                            let call_key = serde_json::to_string(func_call).unwrap_or_default();
                                                            if !emitted_tool_calls.contains(&call_key) {
                                                                emitted_tool_calls.insert(call_key.clone());

                                                                let name = func_call.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                                                                let mut args = func_call.get("args").unwrap_or(&json!({})).clone();

                                                                if name == "shell" || name == "bash" || name == "local_shell" {
                                                                    if let Some(obj) = args.as_object_mut() {
                                                                        if !obj.contains_key("command") {
                                                                            for alt_key in &["cmd", "code", "script", "shell_command"] {
                                                                                if let Some(val) = obj.remove(*alt_key) {
                                                                                    obj.insert("command".to_string(), val);
                                                                                    break;
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                }

                                                                let args_str = serde_json::to_string(&args).unwrap_or_default();

                                                                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                                                                use std::hash::{Hash, Hasher};
                                                                call_key.hash(&mut hasher);
                                                                let call_id = format!("call_{:x}", hasher.finish());

                                                                let (actual_name, namespace) = split_namespace_tool_name(name);
                                                                let tool_item_id = format!("item-{}", &Uuid::new_v4().to_string()[..16]);
                                                                let is_custom_tool = actual_name == "apply_patch" || actual_name == "apply_patch_v2" || actual_name == "shell";

                                                                let mut final_args_str = args_str.clone();
                                                                let mut apply_patch_repairs_value: Option<Value> = None;
                                                                let mut apply_patch_validation: Option<(usize, String)> = None;
                                                                if is_custom_tool && (actual_name == "apply_patch" || actual_name == "apply_patch_v2") {
                                                                    let extracted_patch = extract_apply_patch_input(&args);
                                                                    let (optimized_patch, repairs) =
                                                                        crate::proxy::adapters::apply_patch_preflight::optimize_patch(
                                                                            &extracted_patch,
                                                                            None,
                                                                            true,
                                                                        );
                                                                    if !repairs.is_empty() {
                                                                        apply_patch_repairs_value = Some(
                                                                            crate::proxy::adapters::apply_patch_preflight::repairs_to_value(&repairs),
                                                                        );
                                                                    }
                                                                    final_args_str = optimized_patch;
                                                                    apply_patch_validation =
                                                                        crate::proxy::adapters::apply_patch_preflight::validate_v4a_for_codex(
                                                                            &final_args_str,
                                                                        );
                                                                }

                                                                let mut item_obj = json!({
                                                                    "id": &tool_item_id,
                                                                    "type": if is_custom_tool { "custom_tool_call" } else { "function_call" },
                                                                    "status": "completed",
                                                                    "name": actual_name,
                                                                    "call_id": &call_id,
                                                                });
                                                                if is_custom_tool {
                                                                    item_obj["input"] = json!(&final_args_str);
                                                                } else {
                                                                    item_obj["arguments"] = json!(&final_args_str);
                                                                }
                                                                if let Some(ns) = namespace {
                                                                    item_obj["namespace"] = json!(ns);
                                                                }

                                                                let tool_output_index = next_output_index;
                                                                next_output_index += 1;

                                                                if let Some((line, message)) = apply_patch_validation.as_ref() {
                                                                    crate::proxy::adapters::apply_patch_trace::emit(
                                                                        &crate::proxy::adapters::apply_patch_trace::ApplyPatchTrace {
                                                                            source: "gemini_native",
                                                                            model: &_model,
                                                                            call_id: &call_id,
                                                                            fc_id: &tool_item_id,
                                                                            args_raw: &args_str,
                                                                            input: &final_args_str,
                                                                            interrupted: false,
                                                                            json_truncation: None,
                                                                            v4a_truncation: None,
                                                                            v4a_validation: Some((*line, message.as_str())),
                                                                            decision: "incomplete",
                                                                            repairs: apply_patch_repairs_value.as_ref(),
                                                                        },
                                                                    );
                                                                    if accumulated_text.is_empty() {
                                                                        accumulated_text = format!(
                                                                            "apply_patch 格式非法，已停止执行以避免重复失败。第 {line} 行：{message}"
                                                                        );
                                                                    }
                                                                    continue;
                                                                }

                                                                let mut added_item = item_obj.clone();
                                                                added_item["status"] = json!("in_progress");
                                                                if is_custom_tool {
                                                                    added_item["input"] = json!("");
                                                                } else {
                                                                    added_item["arguments"] = json!("");
                                                                }
                                                                let added_ev = json!({
                                                                    "type": "response.output_item.added",
                                                                    "output_index": tool_output_index,
                                                                    "item": added_item
                                                                });
                                                                yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&added_ev).unwrap())));

                                                                let mut delta_ev = json!({
                                                                    "type": if is_custom_tool { "response.custom_tool_call_input.delta" } else { "response.function_call_arguments.delta" },
                                                                    "item_id": &tool_item_id,
                                                                    "output_index": tool_output_index,
                                                                    "delta": &final_args_str
                                                                });
                                                                if is_custom_tool {
                                                                    delta_ev["call_id"] = json!(&call_id);
                                                                }
                                                                yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&delta_ev).unwrap())));

                                                                let mut args_done_ev = json!({
                                                                    "type": if is_custom_tool { "response.custom_tool_call_input.done" } else { "response.function_call_arguments.done" },
                                                                    "item_id": &tool_item_id,
                                                                    "output_index": tool_output_index,
                                                                });
                                                                if is_custom_tool {
                                                                    args_done_ev["call_id"] = json!(&call_id);
                                                                    args_done_ev["input"] = json!(&final_args_str);
                                                                } else {
                                                                    args_done_ev["arguments"] = json!(&final_args_str);
                                                                }
                                                                yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&args_done_ev).unwrap())));

                                                                let done_ev = json!({
                                                                    "type": "response.output_item.done",
                                                                    "output_index": tool_output_index,
                                                                    "item": item_obj
                                                                });
                                                                yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&done_ev).unwrap())));

                                                                let tc_val = item_obj.clone();
                                                                crate::proxy::handlers::openai::insert_cached_tool_call(call_id.clone(), tc_val.clone());
                                                                if is_custom_tool && (actual_name == "apply_patch" || actual_name == "apply_patch_v2") {
                                                                    crate::proxy::adapters::apply_patch_trace::emit(
                                                                        &crate::proxy::adapters::apply_patch_trace::ApplyPatchTrace {
                                                                            source: "gemini_native",
                                                                            model: &_model,
                                                                            call_id: &call_id,
                                                                            fc_id: &tool_item_id,
                                                                            args_raw: &args_str,
                                                                            input: &final_args_str,
                                                                            interrupted: false,
                                                                            json_truncation: None,
                                                                            v4a_truncation: None,
                                                                            v4a_validation: None,
                                                                            decision: "completed",
                                                                            repairs: apply_patch_repairs_value.as_ref(),
                                                                        },
                                                                    );
                                                                }
                                                                // [FIX] 按发出顺序追踪输出项
                                                                final_outputs_map.insert(tool_output_index, tc_val);
                                                            }
                                                        }
                                                    }

                                                }

                                                // 处理 groundingMetadata (搜索引文)
                                                if let Some(grounding) = candidate.get("groundingMetadata") {
                                                    let mut grounding_text = String::new();
                                                    if let Some(queries) = grounding.get("webSearchQueries").and_then(|q| q.as_array()) {
                                                        let query_list: Vec<&str> = queries.iter().filter_map(|v| v.as_str()).collect();
                                                        if !query_list.is_empty() {
                                                            grounding_text.push_str("\n\n---\n**🔍 已为您搜索：** ");
                                                            grounding_text.push_str(&query_list.join(", "));
                                                        }
                                                    }
                                                    if let Some(chunks) = grounding.get("groundingChunks").and_then(|c| c.as_array()) {
                                                        let mut links = Vec::new();
                                                        for (i, chunk) in chunks.iter().enumerate() {
                                                            if let Some(web) = chunk.get("web") {
                                                                let title = web.get("title").and_then(|v| v.as_str()).unwrap_or("网页来源");
                                                                let uri = web.get("uri").and_then(|v| v.as_str()).unwrap_or("#");
                                                                links.push(format!("[{}] [{}]({})", i + 1, title, uri));
                                                            }
                                                        }
                                                        if !links.is_empty() {
                                                            grounding_text.push_str("\n\n**🌐 来源引文：**\n");
                                                            grounding_text.push_str(&links.join("\n"));
                                                        }
                                                    }
                                                    if !grounding_text.is_empty() {
                                                        if !message_item_emitted {
                                                            message_item_emitted = true;
                                                            message_output_index = next_output_index;
                                                            next_output_index += 1;
                                                            let output_item_added = json!({"type": "response.output_item.added", "output_index": message_output_index, "item": {"id": &message_item_id, "type": "message", "role": "assistant", "phase": "commentary", "status": "in_progress", "content": []}});
                                                            yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&output_item_added).unwrap())));
                                                            let content_part_added = json!({"type": "response.content_part.added", "item_id": &message_item_id, "output_index": message_output_index, "content_index": 0, "part": {"type": "output_text", "text": ""}});
                                                            yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&content_part_added).unwrap())));
                                                        }
                                                        accumulated_text.push_str(&grounding_text);
                                                        let delta_ev = json!({
                                                            "type": "response.output_text.delta",
                                                            "item_id": &message_item_id,
                                                            "output_index": message_output_index,
                                                            "content_index": 0,
                                                            "delta": grounding_text
                                                        });
                                                        yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&delta_ev).unwrap())));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Some(Err(_)) => break,
                        None => break,
                    }
                }
                _ = heartbeat_interval.tick() => { yield Ok::<Bytes, String>(Bytes::from(": ping\n\n")); }
            }
        }

        let mut emit_empty = false;
        if !message_item_emitted && final_outputs_map.is_empty() {
            emit_empty = true;
            message_output_index = next_output_index;
            let output_item_added = json!({"type": "response.output_item.added", "output_index": message_output_index, "item": {"id": &message_item_id, "type": "message", "role": "assistant", "phase": "commentary", "status": "in_progress", "content": []}});
            yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&output_item_added).unwrap())));
            let content_part_added = json!({"type": "response.content_part.added", "item_id": &message_item_id, "output_index": message_output_index, "content_index": 0, "part": {"type": "output_text", "text": ""}});
            yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&content_part_added).unwrap())));
        }

        if message_item_emitted || emit_empty {
            let text_done = json!({
                "type": "response.output_text.done",
                "item_id": &message_item_id,
                "output_index": message_output_index,
                "content_index": 0,
                "text": &accumulated_text
            });
            yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&text_done).unwrap())));

            let content_part_done = json!({
                "type": "response.content_part.done",
                "item_id": &message_item_id,
                "output_index": message_output_index,
                "content_index": 0,
                "part": {
                    "type": "output_text",
                    "text": &accumulated_text
                }
            });
            yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&content_part_done).unwrap())));

            let message_item = json!({
                "id": &message_item_id,
                "type": "message",
                "role": "assistant",
                "phase": "final_answer",
                "status": "completed",
                "content": [{
                    "type": "output_text",
                    "text": &accumulated_text
                }]
            });

            let output_item_done = json!({
                "type": "response.output_item.done",
                "output_index": message_output_index,
                "item": message_item.clone()
            });
            yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&output_item_done).unwrap())));

            final_outputs_map.insert(message_output_index, message_item);
        }

        // Cache the reasoning text for next turn
        if !accumulated_thinking.is_empty() {
            crate::proxy::SignatureCache::global().cache_session_reasoning(
                &session_id,
                accumulated_thinking,
                assistant_turn_index,
            );
        }

        let mut final_outputs: Vec<serde_json::Value> = final_outputs_map.into_values().collect();

        let mut completed_ev = json!({
            "type": "response.completed",
            "response": {
                "id": &response_id,
                "object": "response",
                "status": "completed",
                "output": final_outputs
            }
        });

        if let Some(resp_obj) = completed_ev.get_mut("response").and_then(|r| r.as_object_mut()) {
            if let Some(ref usage) = final_usage {
                resp_obj.insert("usage".to_string(), usage.to_responses_usage_value());
            } else {
                resp_obj.insert(
                    "usage".to_string(),
                    json!({
                        "input_tokens": 0,
                        "input_tokens_details": {
                            "cached_tokens": 0
                        },
                        "output_tokens": 0,
                        "output_tokens_details": {
                            "reasoning_tokens": 0
                        },
                        "total_tokens": 0
                    }),
                );
            }
        }

        yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&completed_ev).unwrap())));
    };
    Box::pin(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use serde_json::json;

    #[tokio::test]
    async fn test_openai_streaming_usage_only_at_end() {
        // Chunk 1: Partial content, no usage
        let chunk1_json = json!({
            "candidates": [{
                "content": {
                    "parts": [{ "text": "Hello" }]
                }
            }]
        });

        // Chunk 2: Finish reason + Usage metadata
        let chunk2_json = json!({
            "candidates": [{
                "finishReason": "STOP",
                "content": {
                    "parts": [{ "text": "" }]
                }
            }],
            "usageMetadata": {
                "promptTokenCount": 5,
                "candidatesTokenCount": 2,
                "totalTokenCount": 7
            }
        });

        // Use a helper to create the stream items compatible with the required signature
        let items: Vec<Result<Bytes, reqwest::Error>> = vec![
            Ok(Bytes::from(format!("data: {}\n\n", chunk1_json))),
            Ok(Bytes::from(format!("data: {}\n\n", chunk2_json))),
        ];

        let gemini_stream = Box::pin(stream::iter(items));

        let mut openai_stream = create_openai_sse_stream(
            gemini_stream,
            "gemini-1.5-flash".to_string(),
            "test-session".to_string(),
            0,
            None,
        );

        let mut chunks = Vec::new();
        while let Some(result) = openai_stream.next().await {
            if let Ok(bytes) = result {
                let s = String::from_utf8_lossy(&bytes).to_string();
                for line in s.lines() {
                    if line.starts_with("data: ") && !line.contains("[DONE]") {
                        chunks.push(line.to_string());
                    }
                }
            }
        }

        let mut found_usage = false;
        let mut found_finish = false;

        for (i, chunk_str) in chunks.iter().enumerate() {
            let json_str = chunk_str.trim_start_matches("data: ").trim();
            let json: Value = serde_json::from_str(json_str).unwrap();

            if i < chunks.len() - 1 {
                assert!(
                    json.get("usage").is_none(),
                    "Usage should not be in intermediate chunks. Found in chunk {}",
                    i
                );
            } else {
                if let Some(usage) = json.get("usage") {
                    found_usage = true;
                    assert_eq!(usage["prompt_tokens"], 5);
                    assert_eq!(usage["completion_tokens"], 2);
                    assert_eq!(usage["total_tokens"], 7);
                }
                if let Some(choices) = json.get("choices") {
                    if let Some(choice) = choices.get(0) {
                        if let Some(finish_reason) = choice.get("finish_reason") {
                            if finish_reason.as_str() == Some("stop") {
                                found_finish = true;
                            }
                        }
                    }
                }
            }
        }
        assert!(found_usage, "Usage should be found in the last chunk");
        assert!(found_finish, "Finish reason should be strictly 'stop'");
    }

    #[tokio::test]
    async fn test_openai_streaming_reasoning_content() {
        // Chunk with thought part
        let chunk_json = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        { "text": "Thinking...", "thought": true },
                        { "text": "Hello world" }
                    ]
                }
            }]
        });

        let items: Vec<Result<Bytes, reqwest::Error>> = vec![
            Ok(Bytes::from(format!("data: {}\n\n", chunk_json))),
        ];

        let gemini_stream = Box::pin(stream::iter(items));

        let mut openai_stream = create_openai_sse_stream(
            gemini_stream,
            "gemini-1.5-flash".to_string(),
            "test-session".to_string(),
            0,
            None,
        );

        let mut chunks = Vec::new();
        while let Some(result) = openai_stream.next().await {
            if let Ok(bytes) = result {
                let s = String::from_utf8_lossy(&bytes).to_string();
                for line in s.lines() {
                    if line.starts_with("data: ") && !line.contains("[DONE]") {
                        chunks.push(line.to_string());
                    }
                }
            }
        }

        let mut has_reasoning = false;
        let mut has_content = false;

        for chunk_str in &chunks {
            let json_str = chunk_str.trim_start_matches("data: ").trim();
            let json: Value = serde_json::from_str(json_str).unwrap();

            if let Some(choices) = json.get("choices") {
                if let Some(choice) = choices.get(0) {
                    if let Some(delta) = choice.get("delta") {
                        if let Some(rc) = delta.get("reasoning_content") {
                            assert_eq!(rc.as_str().unwrap(), "Thinking...");
                            has_reasoning = true;
                            // content should be null or not match thinking process
                            if let Some(content) = delta.get("content") {
                                assert!(content.is_null());
                            }
                        }
                        if let Some(c) = delta.get("content") {
                            if c.is_string() {
                                assert_eq!(c.as_str().unwrap(), "Hello world");
                                has_content = true;
                                assert!(delta.get("reasoning_content").is_none());
                            }
                        }
                    }
                }
            }
        }

        assert!(has_reasoning, "Should stream reasoning_content");
        assert!(has_content, "Should stream content");
    }
}
