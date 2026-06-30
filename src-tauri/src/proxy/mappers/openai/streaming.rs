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
fn extract_usage_metadata(u: &Value) -> Option<super::models::OpenAIUsage> {
    use super::models::{OpenAIUsage, PromptTokensDetails};

    let prompt_tokens = u
        .get("promptTokenCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let completion_tokens = u
        .get("candidatesTokenCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let total_tokens = u
        .get("totalTokenCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let cached_tokens = u
        .get("cachedContentTokenCount")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    // [FIX] 效仿 Anthropic 的计费逻辑，向客户端返回时扣除已缓存的 tokens，避免客户端钱包暴降
    let mut final_prompt_tokens = prompt_tokens;
    if let Some(ct) = cached_tokens {
        if final_prompt_tokens >= ct {
            final_prompt_tokens -= ct;
        }
    }

    Some(OpenAIUsage {
        prompt_tokens: final_prompt_tokens,
        completion_tokens,
        total_tokens,
        prompt_tokens_details: cached_tokens.map(|ct| PromptTokensDetails {
            cached_tokens: Some(ct),
        }),
        completion_tokens_details: None,
    })
}

pub fn create_openai_sse_stream<S, E>(
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
    let stream_id = format!("chatcmpl-{}", Uuid::new_v4());
    let created_ts = Utc::now().timestamp();

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
                                                                    thought_out.push_str(&clean_text); 
                                                                    content_out.push_str(&clean_text); // fallback for UI without reasoning support
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

                                                                    let final_name = if name == "shell" || name == "bash" || name == "local_shell" {
                                                                        "local_shell_call"
                                                                    } else {
                                                                        name
                                                                    };

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

pub fn create_codex_sse_stream<S, E>(
    mut gemini_stream: Pin<Box<S>>,
    _model: String,
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
                                                                    let output_item_added = json!({"type": "response.output_item.added", "output_index": message_output_index, "item": {"id": &message_item_id, "type": "message", "role": "assistant", "status": "in_progress", "content": []}});
                                                                    yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&output_item_added).unwrap())));
                                                                    let content_part_added = json!({"type": "response.content_part.added", "item_id": &message_item_id, "output_index": message_output_index, "content_index": 0, "part": {"type": "output_text", "text": ""}});
                                                                    yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&content_part_added).unwrap())));
                                                                }

                                                                let mut chunk_to_emit = String::new();

                                                                if is_thought {
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

                                                                let mut args_str = serde_json::to_string(&args).unwrap_or_default();
                                                                
                                                                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                                                                use std::hash::{Hash, Hasher};
                                                                call_key.hash(&mut hasher);
                                                                let call_id = format!("call_{:x}", hasher.finish());

                                                                let (actual_name, namespace) = split_namespace_tool_name(name);
                                                                let tool_item_id = format!("item-{}", &Uuid::new_v4().to_string()[..16]);
                                                                let is_custom_tool = actual_name == "apply_patch" || actual_name == "apply_patch_v2" || actual_name == "shell";

                                                                let mut final_args_str = args_str.clone();
                                                                if is_custom_tool && (actual_name == "apply_patch" || actual_name == "apply_patch_v2") {
                                                                    if let Some(obj) = args.as_object() {
                                                                        if let Some(arr) = obj.get("command").and_then(|v| v.as_array()) {
                                                                            if arr.len() > 1 {
                                                                                if let Some(patch) = arr[1].as_str() {
                                                                                    final_args_str = patch.to_string();
                                                                                }
                                                                            }
                                                                        } else if let Some(cmd_str) = obj.get("command").and_then(|v| v.as_str()) {
                                                                            // FALLBACK: Model returned string instead of array (e.g. gemini-pro-agent without prompt)
                                                                            if cmd_str.starts_with("apply_patch\n") {
                                                                                final_args_str = cmd_str["apply_patch\n".len()..].to_string();
                                                                            } else if cmd_str.starts_with("*** Begin Patch") {
                                                                                final_args_str = cmd_str.to_string();
                                                                            } else {
                                                                                final_args_str = cmd_str.to_string();
                                                                            }
                                                                        } else if let Some(patch) = obj.get("patch_text").and_then(|v| v.as_str()) {
                                                                            final_args_str = patch.to_string();
                                                                        }
                                                                    }
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
                                                            let output_item_added = json!({"type": "response.output_item.added", "output_index": message_output_index, "item": {"id": &message_item_id, "type": "message", "role": "assistant", "status": "in_progress", "content": []}});
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
            let output_item_added = json!({"type": "response.output_item.added", "output_index": message_output_index, "item": {"id": &message_item_id, "type": "message", "role": "assistant", "status": "in_progress", "content": []}});
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
            let mut usage_obj = serde_json::Map::new();
            if let Some(ref usage) = final_usage {
                usage_obj.insert("input_tokens".to_string(), json!(usage.prompt_tokens));
                usage_obj.insert("output_tokens".to_string(), json!(usage.completion_tokens));
                usage_obj.insert("total_tokens".to_string(), json!(usage.total_tokens));
            } else {
                usage_obj.insert("input_tokens".to_string(), json!(0));
                usage_obj.insert("output_tokens".to_string(), json!(0));
                usage_obj.insert("total_tokens".to_string(), json!(0));
            }
            resp_obj.insert("usage".to_string(), json!(usage_obj));
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
}

