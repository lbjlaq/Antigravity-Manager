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

    // 新格式下 output_tokens 不含 thought/tool-use, 需要加回来。旧格式 candidatesTokenCount 已经包含它们
    let has_new_format = u.get("total_output_tokens").is_some();
    let completion_tokens = if has_new_format {
        raw_output_tokens + reasoning_tokens.unwrap_or(0) + tool_use_tokens.unwrap_or(0)
    } else {
        raw_output_tokens
    };

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
        let mut has_emitted_content = false;
        let mut tool_call_index = 0;
        let mut thinking_acc = crate::proxy::thinking_store::TurnAccumulator::new();

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
                                                            thinking_acc.ingest_part(part);
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

                                                                    // [FIX #1575 & #3430] 标准化并清洗 shell / PowerShell 等工具参数名称与必填字段
                                                                    super::response::normalize_and_sanitize_tool_args(name, &mut args);

                                                                    let final_name = super::response::resolve_shell_tool_name(name, &client_tool_names);

                                                                    let call_id = func_call
                                                                        .get("id")
                                                                        .and_then(|v| v.as_str())
                                                                        .map(|s| s.to_string())
                                                                        .unwrap_or_else(|| {
                                                                            let mut hasher = std::collections::hash_map::DefaultHasher::new();
                                                                            use std::hash::{Hash, Hasher};
                                                                            serde_json::to_string(func_call).unwrap_or_default().hash(&mut hasher);
                                                                            format!("call_{:x}", hasher.finish())
                                                                        });

                                                                    if let Some(sig) = part.get("thoughtSignature").or(part.get("thought_signature")).and_then(|s| s.as_str()) {
                                                                        crate::proxy::SignatureCache::global().cache_tool_signature(&call_id, sig.to_string());
                                                                    }
                                                                    thinking_acc.record_tool_id(name, &call_id);

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

                                                    let raw_finish_reason = candidate.get("finishReason").and_then(|f| f.as_str());
                                                    let is_malformed_function_call = raw_finish_reason == Some("MALFORMED_FUNCTION_CALL");

                                                    let gemini_finish_reason = raw_finish_reason.map(|f| match f {
                                                        "STOP" => "stop",
                                                        "MAX_TOKENS" => "length",
                                                        "SAFETY" => "content_filter",
                                                        "RECITATION" => "content_filter",
                                                        "MALFORMED_FUNCTION_CALL" => "stop",
                                                        _ => "stop",
                                                    });

                                                    // [FIX #1575] 如果发射了工具调用，强制设置为 tool_calls
                                                    // 解决 Gemini 返回 STOP 但有工具调用时，OpenAI 客户端认为对话已结束的问题
                                                    let finish_reason = if !emitted_tool_calls.is_empty() && gemini_finish_reason.is_some() {
                                                        Some("tool_calls")
                                                    } else {
                                                        gemini_finish_reason
                                                    };

                                                    // [FIX MALFORMED_FUNCTION_CALL] 若模型试图调用未配置的内部工具或格式异常导致提前中断，
                                                    // 且未生成正文内容，自动注入友好提示，避免客户端显示空白
                                                    if is_malformed_function_call && content_out.is_empty() && !has_emitted_content {
                                                        content_out.push_str("很抱歉，当前模型在尝试调取实时信息时遇到了格式异常。若需要查询实时天气或最新资讯，请尝试使用联网模式（模型名带 -online 后缀）或配置天气/搜索插件。");
                                                    }

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
                                                        if !content_out.is_empty() {
                                                            has_emitted_content = true;
                                                        }
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

        thinking_acc.commit(&session_id);
        if !error_occurred {
            if let Some(usage) = final_usage.take() {
                let usage_chunk = json!({
                    "id": &stream_id,
                    "object": "chat.completion.chunk",
                    "created": created_ts,
                    "model": &model,
                    "choices": [],
                    "usage": usage
                });
                yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&usage_chunk).unwrap_or_default())));
            }
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
        let mut thinking_acc = crate::proxy::thinking_store::TurnAccumulator::new();
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
                                                            thinking_acc.ingest_part(part);
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
                                                "STOP" => "stop", "MAX_TOKENS" => "length", "SAFETY" => "content_filter", "RECITATION" => "content_filter", _ => "stop",
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
        thinking_acc.commit(&session_id);
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

fn inject_seq(mut event: Value, seq: &mut u64) -> Value {
    if let Some(obj) = event.as_object_mut() {
        obj.insert("sequence_number".to_string(), json!(*seq));
    }
    *seq += 1;
    event
}

/// Serialize one named Responses API SSE frame.
///
/// Codex Desktop consumes the SSE `event` field as well as the JSON payload's
/// `type`. Keeping both in sync matches the native Responses stream and avoids
/// reasoning/tool lifecycle events being treated as anonymous data messages.
fn codex_sse_frame(event: &Value) -> Bytes {
    let event_name = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");
    let payload = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string());
    Bytes::from(format!("event: {event_name}\ndata: {payload}\n\n"))
}

pub fn create_codex_sse_stream<S, E>(
    mut gemini_stream: Pin<Box<S>>,
    model: String,
    session_id: String,
    message_count: usize,
    _assistant_turn_index: usize,
    response_id: String,
    completion_tx: Option<
        tokio::sync::oneshot::Sender<(Vec<Value>, tokio::sync::oneshot::Sender<()>)>,
    >,
    cache_tool_calls: bool,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + ?Sized + 'static,
    E: std::fmt::Display + Send + 'static,
{
    let mut buffer = BytesMut::new();
    let item_id_prefix = uuid::Uuid::new_v4().simple().to_string();
    let message_item_id = format!("msg_{}_0", &item_id_prefix[..16]);
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut completion_tx = completion_tx;
    let stream = async_stream::stream! {
        let mut sequence_number: u64 = 0;

        // Native Responses lifecycle: created must be followed by in_progress.
        let lifecycle_response = json!({
            "id": &response_id,
            "object": "response",
            "created_at": created_at,
            "status": "in_progress",
            "model": &model,
            "output": [],
            "error": null,
            "incomplete_details": null,
            "usage": null
        });
        let created_ev = json!({ "type": "response.created", "response": lifecycle_response.clone() });
        let created_ev = inject_seq(created_ev, &mut sequence_number);
        yield Ok::<Bytes, String>(codex_sse_frame(&created_ev));
        let in_progress_ev = json!({ "type": "response.in_progress", "response": lifecycle_response });
        let in_progress_ev = inject_seq(in_progress_ev, &mut sequence_number);
        yield Ok::<Bytes, String>(codex_sse_frame(&in_progress_ev));

        let mut message_item_emitted = false;
        let mut reasoning_open = false;
        let mut reasoning_item_seq: u32 = 0;
        let mut active_reasoning_item_id = String::new();

        let mut emitted_tool_calls = std::collections::HashSet::new();
        let mut accumulated_text = String::new();
        let mut accumulated_thinking = String::new();
        let mut thinking_acc = crate::proxy::thinking_store::TurnAccumulator::new();
        let mut has_seen_tool_calls = false;
        let mut final_finish_reason: Option<String> = None;

        let mut final_outputs_map: std::collections::BTreeMap<u32, serde_json::Value> = std::collections::BTreeMap::new();
        let mut next_output_index: u32 = 0;
        let mut message_output_index: u32 = 0;
        let mut reasoning_output_index: u32 = 0;
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
                                                if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
                                                    final_finish_reason = Some(reason.to_string());
                                                }
                                                if let Some(parts) = candidate.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                                                    for part in parts {
                                                        thinking_acc.ingest_part(part);
                                                        let is_thought = part.get("thought").and_then(|v| v.as_bool()).unwrap_or(false);

                                                        // Close the reasoning summary before opening normal text
                                                        // or a tool item so output item lifecycles never overlap.
                                                        let is_text_or_tool = part.get("text").is_some() || part.get("functionCall").is_some() || part.get("inlineData").is_some();
                                                        if is_text_or_tool && !is_thought && reasoning_open {
                                                            let text_done = json!({
                                                                "type": "response.reasoning_summary_text.done",
                                                                "item_id": &active_reasoning_item_id,
                                                                "output_index": reasoning_output_index,
                                                                "summary_index": 0,
                                                                "text": &accumulated_thinking
                                                            });
                                                            let text_done = inject_seq(text_done, &mut sequence_number);
                                                            yield Ok::<Bytes, String>(codex_sse_frame(&text_done));

                                                            let summary_part_done = json!({
                                                                "type": "response.reasoning_summary_part.done",
                                                                "item_id": &active_reasoning_item_id,
                                                                "output_index": reasoning_output_index,
                                                                "summary_index": 0,
                                                                "part": {
                                                                    "type": "summary_text",
                                                                    "text": &accumulated_thinking
                                                                }
                                                            });
                                                            let summary_part_done = inject_seq(summary_part_done, &mut sequence_number);
                                                            yield Ok::<Bytes, String>(codex_sse_frame(&summary_part_done));

                                                            let reasoning_item = json!({
                                                                "id": &active_reasoning_item_id,
                                                                "type": "reasoning",
                                                                "status": "completed",
                                                                "summary": [{
                                                                    "type": "summary_text",
                                                                    "text": &accumulated_thinking
                                                                }]
                                                            });

                                                            let done_ev = json!({
                                                                "type": "response.output_item.done",
                                                                "output_index": reasoning_output_index,
                                                                "item": &reasoning_item
                                                            });
                                                            let done_ev = inject_seq(done_ev, &mut sequence_number);
                                                            yield Ok::<Bytes, String>(codex_sse_frame(&done_ev));

                                                            final_outputs_map.insert(reasoning_output_index, reasoning_item);
                                                            reasoning_open = false;
                                                        }

                                                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                                            let clean_text = text.replace("<think>\n", "").replace("<think>", "").replace("\n</think>", "").replace("</think>", "");
                                                            if !clean_text.is_empty() {
                                                                if is_thought && message_item_emitted {
                                                                    // Once ordinary assistant text has started, it is the
                                                                    // authoritative result for this response. A late thought
                                                                    // delta must not be appended to it or open an overlapping
                                                                    // reasoning item.
                                                                    tracing::warn!("[Codex-Stream] Dropping late thought delta after assistant text started");
                                                                } else if is_thought {
                                                                    if !reasoning_open {
                                                                        reasoning_output_index = next_output_index;
                                                                        next_output_index += 1;
                                                                        active_reasoning_item_id = format!(
                                                                            "rs_{}_{}",
                                                                            &item_id_prefix[..16],
                                                                            reasoning_item_seq
                                                                        );
                                                                        reasoning_item_seq += 1;
                                                                        accumulated_thinking.clear();

                                                                        let output_item_added = json!({"type": "response.output_item.added", "output_index": reasoning_output_index, "item": {"id": &active_reasoning_item_id, "type": "reasoning", "status": "in_progress", "summary": []}});
                                                                        let output_item_added = inject_seq(output_item_added, &mut sequence_number);
                                                                        yield Ok::<Bytes, String>(codex_sse_frame(&output_item_added));

                                                                        let part_added = json!({"type": "response.reasoning_summary_part.added", "item_id": &active_reasoning_item_id, "output_index": reasoning_output_index, "summary_index": 0, "part": {"type": "summary_text", "text": ""}});
                                                                        let part_added = inject_seq(part_added, &mut sequence_number);
                                                                        yield Ok::<Bytes, String>(codex_sse_frame(&part_added));

                                                                        reasoning_open = true;
                                                                    }

                                                                    accumulated_thinking.push_str(&clean_text);
                                                                    let delta_ev = json!({
                                                                        "type": "response.reasoning_summary_text.delta",
                                                                        "item_id": &active_reasoning_item_id,
                                                                        "output_index": reasoning_output_index,
                                                                        "summary_index": 0,
                                                                        "delta": clean_text
                                                                    });
                                                                    let delta_ev = inject_seq(delta_ev, &mut sequence_number);
                                                                    yield Ok::<Bytes, String>(codex_sse_frame(&delta_ev));
                                                                } else {
                                                                    if !message_item_emitted {
                                                                        message_item_emitted = true;
                                                                        message_output_index = next_output_index;
                                                                        next_output_index += 1;
                                                                        let output_item_added = json!({"type": "response.output_item.added", "output_index": message_output_index, "item": {"id": &message_item_id, "type": "message", "role": "assistant", "phase": "commentary", "status": "in_progress", "content": []}});
                                                                        let output_item_added = inject_seq(output_item_added, &mut sequence_number);
                                                                        yield Ok::<Bytes, String>(codex_sse_frame(&output_item_added));
                                                                        let content_part_added = json!({"type": "response.content_part.added", "item_id": &message_item_id, "output_index": message_output_index, "content_index": 0, "part": {"type": "output_text", "text": "", "annotations": []}});
                                                                        let content_part_added = inject_seq(content_part_added, &mut sequence_number);
                                                                        yield Ok::<Bytes, String>(codex_sse_frame(&content_part_added));
                                                                    }

                                                                    accumulated_text.push_str(&clean_text);
                                                                    let delta_ev = json!({
                                                                        "type": "response.output_text.delta",
                                                                        "item_id": &message_item_id,
                                                                        "output_index": message_output_index,
                                                                        "content_index": 0,
                                                                        "delta": clean_text
                                                                    });
                                                                    let delta_ev = inject_seq(delta_ev, &mut sequence_number);
                                                                    yield Ok::<Bytes, String>(codex_sse_frame(&delta_ev));
                                                                }
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

                                                                // [FIX #1575 & #3430] 标准化并清洗 shell / PowerShell 等工具参数名称与必填字段
                                                                super::response::normalize_and_sanitize_tool_args(name, &mut args);

                                                                let args_str = serde_json::to_string(&args).unwrap_or_default();

                                                                let call_id = func_call
                                                                    .get("id")
                                                                    .and_then(|v| v.as_str())
                                                                    .map(|s| s.to_string())
                                                                    .unwrap_or_else(|| {
                                                                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                                                                        use std::hash::{Hash, Hasher};
                                                                        call_key.hash(&mut hasher);
                                                                        format!("call_{:x}", hasher.finish())
                                                                    });

                                                                if let Some(sig) = part.get("thoughtSignature").or(part.get("thought_signature")).and_then(|s| s.as_str()) {
                                                                    crate::proxy::SignatureCache::global().cache_tool_signature(&call_id, sig.to_string());
                                                                }
                                                                thinking_acc.record_tool_id(name, &call_id);

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
                                                                            model: &model,
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

                                                                has_seen_tool_calls = true;

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
                                                                let added_ev = inject_seq(added_ev, &mut sequence_number);
                                                                yield Ok::<Bytes, String>(codex_sse_frame(&added_ev));

                                                                let mut delta_ev = json!({
                                                                    "type": if is_custom_tool { "response.custom_tool_call_input.delta" } else { "response.function_call_arguments.delta" },
                                                                    "item_id": &tool_item_id,
                                                                    "output_index": tool_output_index,
                                                                    "delta": &final_args_str
                                                                });
                                                                if is_custom_tool {
                                                                    delta_ev["call_id"] = json!(&call_id);
                                                                }
                                                                let delta_ev = inject_seq(delta_ev, &mut sequence_number);
                                                                yield Ok::<Bytes, String>(codex_sse_frame(&delta_ev));

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
                                                                let args_done_ev = inject_seq(args_done_ev, &mut sequence_number);
                                                                yield Ok::<Bytes, String>(codex_sse_frame(&args_done_ev));

                                                                let done_ev = json!({
                                                                    "type": "response.output_item.done",
                                                                    "output_index": tool_output_index,
                                                                    "item": item_obj
                                                                });
                                                                let done_ev = inject_seq(done_ev, &mut sequence_number);
                                                                yield Ok::<Bytes, String>(codex_sse_frame(&done_ev));

                                                                let tc_val = item_obj.clone();
                                                                if cache_tool_calls {
                                                                    crate::proxy::handlers::openai::insert_cached_tool_call(call_id.clone(), tc_val.clone());
                                                                }
                                                                if is_custom_tool && (actual_name == "apply_patch" || actual_name == "apply_patch_v2") {
                                                                    crate::proxy::adapters::apply_patch_trace::emit(
                                                                        &crate::proxy::adapters::apply_patch_trace::ApplyPatchTrace {
                                                                            source: "gemini_native",
                                                                            model: &model,
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
                                                            let output_item_added = inject_seq(output_item_added, &mut sequence_number);
                                                            yield Ok::<Bytes, String>(codex_sse_frame(&output_item_added));
                                                            let content_part_added = json!({"type": "response.content_part.added", "item_id": &message_item_id, "output_index": message_output_index, "content_index": 0, "part": {"type": "output_text", "text": "", "annotations": []}});
                                                            let content_part_added = inject_seq(content_part_added, &mut sequence_number);
                                                            yield Ok::<Bytes, String>(codex_sse_frame(&content_part_added));
                                                        }
                                                        accumulated_text.push_str(&grounding_text);
                                                        let delta_ev = json!({
                                                            "type": "response.output_text.delta",
                                                            "item_id": &message_item_id,
                                                            "output_index": message_output_index,
                                                            "content_index": 0,
                                                            "delta": grounding_text
                                                        });
                                                        let delta_ev = inject_seq(delta_ev, &mut sequence_number);
                                                        yield Ok::<Bytes, String>(codex_sse_frame(&delta_ev));
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
                _ = heartbeat_interval.tick() => {
                    yield Ok::<Bytes, String>(Bytes::from(": ping\n\n"));
                }
            }
        }

        // Finalize any reasoning summary still open when the upstream stream ends.
        if reasoning_open {
            let text_done = json!({
                "type": "response.reasoning_summary_text.done",
                "item_id": &active_reasoning_item_id,
                "output_index": reasoning_output_index,
                "summary_index": 0,
                "text": &accumulated_thinking
            });
            let text_done = inject_seq(text_done, &mut sequence_number);
            yield Ok::<Bytes, String>(codex_sse_frame(&text_done));

            let summary_part_done = json!({
                "type": "response.reasoning_summary_part.done",
                "item_id": &active_reasoning_item_id,
                "output_index": reasoning_output_index,
                "summary_index": 0,
                "part": {
                    "type": "summary_text",
                    "text": &accumulated_thinking
                }
            });
            let summary_part_done = inject_seq(summary_part_done, &mut sequence_number);
            yield Ok::<Bytes, String>(codex_sse_frame(&summary_part_done));

            let reasoning_item = json!({
                "id": &active_reasoning_item_id,
                "type": "reasoning",
                "status": "completed",
                "summary": [{
                    "type": "summary_text",
                    "text": &accumulated_thinking
                }]
            });

            let done_ev = json!({
                "type": "response.output_item.done",
                "output_index": reasoning_output_index,
                "item": &reasoning_item
            });
            let done_ev = inject_seq(done_ev, &mut sequence_number);
            yield Ok::<Bytes, String>(codex_sse_frame(&done_ev));

            final_outputs_map.insert(reasoning_output_index, reasoning_item);
        }

        // A proxy-generated diagnostic (for example an invalid apply_patch) may
        // only become available after the upstream stream has ended. Open the
        // message lazily here so it is not silently dropped behind reasoning.
        if !message_item_emitted && !accumulated_text.is_empty() {
            message_item_emitted = true;
            message_output_index = next_output_index;
            let output_item_added = json!({"type": "response.output_item.added", "output_index": message_output_index, "item": {"id": &message_item_id, "type": "message", "role": "assistant", "phase": "commentary", "status": "in_progress", "content": []}});
            let output_item_added = inject_seq(output_item_added, &mut sequence_number);
            yield Ok::<Bytes, String>(codex_sse_frame(&output_item_added));
            let content_part_added = json!({"type": "response.content_part.added", "item_id": &message_item_id, "output_index": message_output_index, "content_index": 0, "part": {"type": "output_text", "text": "", "annotations": []}});
            let content_part_added = inject_seq(content_part_added, &mut sequence_number);
            yield Ok::<Bytes, String>(codex_sse_frame(&content_part_added));
        }

        if message_item_emitted {
            let text_done = json!({
                "type": "response.output_text.done",
                "item_id": &message_item_id,
                "output_index": message_output_index,
                "content_index": 0,
                "text": &accumulated_text
            });
            let text_done = inject_seq(text_done, &mut sequence_number);
            yield Ok::<Bytes, String>(codex_sse_frame(&text_done));

            let content_part_done = json!({
                "type": "response.content_part.done",
                "item_id": &message_item_id,
                "output_index": message_output_index,
                "content_index": 0,
                "part": {
                    "type": "output_text",
                    "text": &accumulated_text,
                    "annotations": []
                }
            });
            let content_part_done = inject_seq(content_part_done, &mut sequence_number);
            yield Ok::<Bytes, String>(codex_sse_frame(&content_part_done));

            // Tool rounds are process commentary. Only a response with no tool
            // call is the authoritative final answer that remains expanded.
            let message_phase = if has_seen_tool_calls { "commentary" } else { "final_answer" };
            let message_item = json!({
                "id": &message_item_id,
                "type": "message",
                "role": "assistant",
                "phase": message_phase,
                "status": "completed",
                "content": [{
                    "type": "output_text",
                    "text": &accumulated_text,
                    "annotations": []
                }]
            });

            let output_item_done = json!({
                "type": "response.output_item.done",
                "output_index": message_output_index,
                "item": message_item.clone()
            });
            let output_item_done = inject_seq(output_item_done, &mut sequence_number);
            yield Ok::<Bytes, String>(codex_sse_frame(&output_item_done));

            final_outputs_map.insert(message_output_index, message_item);
        }

        let final_outputs: Vec<serde_json::Value> = final_outputs_map.into_values().collect();

        let missing_actionable_output = !message_item_emitted && !has_seen_tool_calls;
        let terminal_status = if missing_actionable_output {
            "incomplete"
        } else {
            match final_finish_reason.as_deref() {
                Some("MAX_TOKENS")
                | Some("SAFETY")
                | Some("RECITATION")
                | Some("BLOCKLIST")
                | Some("PROHIBITED_CONTENT")
                | Some("SPII")
                | Some("IMAGE_SAFETY")
                | Some("IMAGE_PROHIBITED_CONTENT")
                | None => "incomplete",
                _ => "completed",
            }
        };
        let terminal_type = format!("response.{terminal_status}");
        let incomplete_details = if terminal_status == "incomplete" {
            let reason = match final_finish_reason.as_deref() {
                Some("MAX_TOKENS") => "max_output_tokens",
                Some("SAFETY")
                | Some("RECITATION")
                | Some("BLOCKLIST")
                | Some("PROHIBITED_CONTENT")
                | Some("SPII")
                | Some("IMAGE_SAFETY")
                | Some("IMAGE_PROHIBITED_CONTENT") => "content_filter",
                _ => "interrupted",
            };
            json!({"reason": reason})
        } else {
            Value::Null
        };
        let terminal_error = if missing_actionable_output {
            json!({
                "code": "empty_response",
                "message": "Gemini stream ended without a final assistant message or tool call."
            })
        } else if final_finish_reason.is_none() {
            json!({
                "code": "upstream_interrupted",
                "message": "Gemini stream ended without finishReason."
            })
        } else {
            Value::Null
        };
        let completed_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if terminal_status == "completed" {
            if let Some(tx) = completion_tx.take() {
                let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
                if tx.send((final_outputs.clone(), ack_tx)).is_err() || ack_rx.await.is_err() {
                    return;
                }
            }
        }

        thinking_acc.clone().commit(&session_id);
        if session_id != response_id {
            thinking_acc.commit(&response_id);
        }

        let mut completed_ev = json!({
            "type": terminal_type,
            "response": {
                "id": &response_id,
                "object": "response",
                "created_at": created_at,
                "completed_at": completed_at,
                "status": terminal_status,
                "model": &model,
                "output": final_outputs,
                "incomplete_details": incomplete_details,
                "error": terminal_error
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

        let completed_ev = inject_seq(completed_ev, &mut sequence_number);
        yield Ok::<Bytes, String>(codex_sse_frame(&completed_ev));
    };
    Box::pin(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use serde_json::json;

    async fn collect_codex_stream(chunks: Vec<Value>) -> (String, Vec<Value>) {
        collect_codex_stream_with_cache(chunks, true).await
    }

    async fn collect_codex_stream_with_cache(
        chunks: Vec<Value>,
        cache_tool_calls: bool,
    ) -> (String, Vec<Value>) {
        let items: Vec<Result<Bytes, String>> = chunks
            .into_iter()
            .map(|chunk| Ok(Bytes::from(format!("data: {chunk}\n\n"))))
            .collect();
        let mut stream = create_codex_sse_stream(
            Box::pin(stream::iter(items)),
            "gemini-pro-agent".to_string(),
            "test-codex-session".to_string(),
            0,
            0,
            "resp-test-codex-session".to_string(),
            None,
            cache_tool_calls,
        );

        let mut raw = String::new();
        while let Some(item) = stream.next().await {
            raw.push_str(&String::from_utf8_lossy(&item.expect("codex stream item")));
        }
        let events = raw
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter_map(|data| serde_json::from_str::<Value>(data).ok())
            .collect();
        (raw, events)
    }

    #[tokio::test]
    async fn responses_store_false_emits_complete_tool_call_without_caching_it() {
        let (_, events) = collect_codex_stream_with_cache(vec![json!({
            "response": {"candidates": [{
                "finishReason": "STOP",
                "content": {"parts": [{"functionCall": {"name":"shell_command", "args":{"command":"pwd"}}}]}
            }]}
        })], false).await;
        let completed = events
            .iter()
            .find(|event| event["type"] == "response.completed")
            .expect("completed response");
        let call = completed["response"]["output"]
            .as_array()
            .expect("output")
            .iter()
            .find(|item| item["type"] == "function_call")
            .expect("function call");
        let call_id = call["call_id"].as_str().expect("call id");
        assert_eq!(call["name"], "shell_command");
        assert_eq!(
            serde_json::from_str::<Value>(call["arguments"].as_str().expect("arguments"))
                .expect("JSON arguments"),
            json!({"command":"pwd"})
        );
        assert!(crate::proxy::handlers::openai::get_cached_tool_call(call_id).is_none());
        assert!(events
            .iter()
            .any(|event| event["type"] == "response.output_item.done"
                && event["item"]["call_id"] == call_id));
    }

    #[tokio::test]
    async fn codex_response_id_matches_saved_session_key() {
        let response_id = format!("resp-test-{}", uuid::Uuid::new_v4());
        let upstream = vec![Ok::<Bytes, String>(Bytes::from(
            "data: {\"response\":{\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"thought\":true,\"text\":\"Checking.\"},{\"text\":\"done\"}]}}]}}\n\n",
        ))];
        let (completion_tx, completion_rx) =
            tokio::sync::oneshot::channel::<(Vec<Value>, tokio::sync::oneshot::Sender<()>)>();
        let save_response_id = response_id.clone();
        let save_task = tokio::spawn(async move {
            let (outputs, ack_tx) = completion_rx.await.expect("completed output");
            crate::proxy::http_session_store::save_session_delta(
                save_response_id,
                None,
                vec![json!({"id": "user-1", "role": "user", "content": "hello"})],
                outputs,
                String::new(),
                "gemini-pro-agent".to_string(),
                "routing-test-session".to_string(),
            )
            .await;
            ack_tx.send(()).expect("acknowledge session save");
        });
        let mut stream = create_codex_sse_stream(
            Box::pin(stream::iter(upstream)),
            "gemini-pro-agent".to_string(),
            "test-session".to_string(),
            1,
            0,
            response_id.clone(),
            Some(completion_tx),
            true,
        );

        let mut raw = String::new();
        let mut saw_completed = false;
        while let Some(item) = stream.next().await {
            let item = item.expect("stream item");
            let text = String::from_utf8_lossy(&item);
            if text.contains("event: response.completed") {
                saw_completed = true;
                let restored = crate::proxy::http_session_store::get_session(&response_id)
                    .await
                    .expect("session exists when response.completed is visible");
                assert_eq!(restored.input_items[0]["id"], "user-1");
                let completed: Value = serde_json::from_str(
                    text.lines()
                        .find_map(|line| line.strip_prefix("data: "))
                        .expect("completion data"),
                )
                .expect("completion JSON");
                assert_eq!(restored.input_items[1]["type"], "reasoning");
                assert_eq!(restored.input_items[1]["summary"][0]["text"], "Checking.");
                assert_eq!(
                    &restored.input_items[1..],
                    completed["response"]["output"].as_array().expect("output")
                );
                assert!(restored
                    .input_items
                    .iter()
                    .any(|item| item["role"] == "assistant"));
            }
            raw.push_str(&text);
        }
        save_task.await.expect("session save task");

        assert!(saw_completed);
        assert!(raw.contains(&format!("\"id\":\"{response_id}\"")));
    }

    #[test]
    fn response_branches_store_signatures_under_their_own_response_ids() {
        let branch_a = format!("resp-signature-a-{}", uuid::Uuid::new_v4());
        let branch_b = format!("resp-signature-b-{}", uuid::Uuid::new_v4());
        let signature_a = "a".repeat(64);
        let signature_b = "b".repeat(64);
        store_thought_signature(&signature_a, &branch_a, 1);
        store_thought_signature(&signature_b, &branch_b, 1);

        assert_eq!(
            crate::proxy::SignatureCache::global().get_session_signature(&branch_a),
            Some(signature_a)
        );
        assert_eq!(
            crate::proxy::SignatureCache::global().get_session_signature(&branch_b),
            Some(signature_b)
        );
    }

    #[tokio::test]
    async fn test_codex_reasoning_summary_and_tool_are_distinct_output_items() {
        let (raw, events) = collect_codex_stream(vec![
            json!({
                "response": {
                    "candidates": [{
                        "content": {"parts": [{"text": "Inspecting the workspace.", "thought": true}]}
                    }]
                }
            }),
            json!({
                "response": {
                    "candidates": [{
                        "content": {"parts": [{
                            "functionCall": {"name": "shell_command", "args": {"command": "Get-ChildItem"}}
                        }]}
                    }]
                }
            }),
            json!({
                "response": {
                    "candidates": [{
                        "finishReason": "STOP",
                        "content": {"parts": [{"text": ""}]}
                    }]
                }
            }),
        ])
        .await;

        assert!(raw.starts_with("event: response.created\ndata: "));
        let names: Vec<&str> = events
            .iter()
            .filter_map(|event| event["type"].as_str())
            .collect();
        assert_eq!(names[0], "response.created");
        assert_eq!(names[1], "response.in_progress");
        assert!(!names.contains(&"response.output_text.delta"));
        assert_eq!(
            &names[3..8],
            &[
                "response.reasoning_summary_part.added",
                "response.reasoning_summary_text.delta",
                "response.reasoning_summary_text.done",
                "response.reasoning_summary_part.done",
                "response.output_item.done",
            ]
        );
        assert!(names.contains(&"response.function_call_arguments.delta"));
        assert_eq!(names.last().copied(), Some("response.completed"));

        for (expected, event) in events.iter().enumerate() {
            assert_eq!(event["sequence_number"], expected as u64);
        }
        assert!(events
            .iter()
            .filter(|event| event["type"]
                .as_str()
                .is_some_and(|name| name.starts_with("response.reasoning_summary_")))
            .all(|event| event["summary_index"] == 0
                && event["output_index"] == 0
                && event["item_id"] == events[2]["item"]["id"]));

        let added: Vec<&Value> = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.added")
            .collect();
        assert_eq!(added.len(), 2);
        assert_eq!(added[0]["item"]["type"], "reasoning");
        assert_eq!(added[0]["item"]["summary"], json!([]));
        assert_eq!(added[0]["item"]["status"], "in_progress");
        assert!(added[0]["item"]["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("rs_")));
        assert_eq!(added[0]["output_index"], 0);
        assert_eq!(added[1]["item"]["type"], "function_call");
        assert_eq!(added[1]["output_index"], 1);

        let completed = events.last().expect("terminal event");
        let output = completed["response"]["output"]
            .as_array()
            .expect("completed output");
        assert_eq!(output.len(), 2);
        assert_eq!(output[0]["type"], "reasoning");
        assert_eq!(output[0]["summary"][0]["text"], "Inspecting the workspace.");
        assert_eq!(output[0], events[7]["item"]);
        assert_eq!(events[8]["item"], added[1]["item"]);
        assert_eq!(events[9]["item_id"], output[1]["id"]);
        assert_eq!(added[1]["item"]["call_id"], output[1]["call_id"]);
        assert!(!raw.contains("**Thinking**"));
        assert_eq!(output[1]["type"], "function_call");
    }

    #[tokio::test]
    async fn test_codex_final_message_is_promoted_and_persisted() {
        let (_, events) = collect_codex_stream(vec![
            json!({"candidates": [{"content": {"parts": [{"text": "Checking ", "thought": true}]}}]}),
            json!({"candidates": [{"content": {"parts": [{"text": "results.", "thought": true}]}}]}),
            json!({
                "candidates": [{
                    "content": {"parts": [{"text": "The task is complete."}]}
                }]
            }),
            json!({
                "candidates": [{
                    "finishReason": "STOP",
                    "content": {"parts": [{"text": ""}]}
                }]
            }),
        ])
        .await;

        let added = events
            .iter()
            .find(|event| {
                event["type"] == "response.output_item.added" && event["item"]["type"] == "message"
            })
            .expect("message added");
        assert_eq!(added["item"]["phase"], "commentary");

        let done = events
            .iter()
            .find(|event| {
                event["type"] == "response.output_item.done" && event["item"]["type"] == "message"
            })
            .expect("message done");
        assert_eq!(done["item"]["phase"], "final_answer");
        assert_eq!(done["item"]["content"][0]["text"], "The task is complete.");
        assert_eq!(done["item"]["content"][0]["annotations"], json!([]));
        assert_eq!(
            events[3]["part"],
            json!({"type": "summary_text", "text": ""})
        );
        assert_eq!(events[4]["delta"], "Checking ");
        assert_eq!(events[5]["delta"], "results.");
        assert_eq!(events[4]["item_id"], events[5]["item_id"]);
        assert_eq!(events[6]["text"], "Checking results.");
        assert_eq!(
            events[7]["part"],
            json!({"type": "summary_text", "text": "Checking results."})
        );
        assert_eq!(events[8]["type"], "response.output_item.done");
        assert_eq!(events[9]["item"], added["item"]);

        let terminal = events.last().expect("terminal event");
        assert_eq!(terminal["type"], "response.completed");
        assert_eq!(terminal["response"]["status"], "completed");
        assert_eq!(terminal["response"]["output"][0], events[8]["item"]);
        assert_eq!(terminal["response"]["output"][1], done["item"]);
        assert_eq!(terminal["response"]["output"][1]["phase"], "final_answer");
        assert_eq!(
            terminal["response"]["output"][1]["content"][0]["text"],
            "The task is complete."
        );
    }

    #[tokio::test]
    async fn test_codex_reasoning_summary_is_closed_at_stream_end() {
        let (_, events) = collect_codex_stream(vec![json!({
            "candidates": [{"content": {"parts": [{"text": "Checking.", "thought": true}]}}]
        })])
        .await;
        assert_eq!(events[5]["type"], "response.reasoning_summary_text.done");
        assert_eq!(events[5]["text"], "Checking.");
        assert_eq!(events[6]["type"], "response.reasoning_summary_part.done");
        assert_eq!(
            events[6]["part"],
            json!({"type": "summary_text", "text": "Checking."})
        );
        assert_eq!(events[7]["type"], "response.output_item.done");
        assert_eq!(events[7]["item"]["type"], "reasoning");
        assert_eq!(events[7]["item"]["status"], "completed");
        assert_eq!(events[7]["item"]["summary"], json!([events[6]["part"]]));
        let terminal = events.last().expect("terminal event");
        assert_eq!(terminal["response"]["output"], json!([events[7]["item"]]));
        assert_eq!(terminal["type"], "response.incomplete");
        assert_eq!(
            terminal["response"]["incomplete_details"]["reason"],
            "interrupted"
        );
        assert_eq!(terminal["response"]["error"]["code"], "empty_response");
    }

    #[tokio::test]
    async fn test_codex_late_thought_does_not_mix_into_final_answer() {
        let (raw, events) = collect_codex_stream(vec![
            json!({
                "candidates": [{
                    "content": {"parts": [{"text": "The task is complete."}]}
                }]
            }),
            json!({
                "candidates": [{
                    "content": {"parts": [{"text": "late private thought", "thought": true}]}
                }]
            }),
            json!({
                "candidates": [{
                    "finishReason": "STOP",
                    "content": {"parts": [{"text": ""}]}
                }]
            }),
        ])
        .await;

        assert!(!raw.contains("late private thought"));
        assert!(events.iter().all(|event| {
            !(event["type"] == "response.output_item.added"
                && event["item"]["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("rs_")))
        }));
        let terminal = events.last().expect("terminal event");
        assert_eq!(terminal["type"], "response.completed");
        assert_eq!(
            terminal["response"]["output"][0]["content"][0]["text"],
            "The task is complete."
        );
    }

    #[tokio::test]
    async fn test_codex_tool_round_message_stays_commentary() {
        let (_, events) = collect_codex_stream(vec![
            json!({
                "candidates": [{
                    "content": {"parts": [{"text": "I will inspect the files first."}]}
                }]
            }),
            json!({
                "candidates": [{
                    "content": {"parts": [{
                        "functionCall": {"name": "shell_command", "args": {"command": "Get-ChildItem"}}
                    }]}
                }]
            }),
            json!({
                "candidates": [{
                    "finishReason": "STOP",
                    "content": {"parts": [{"text": ""}]}
                }]
            }),
        ])
        .await;

        let message_done = events
            .iter()
            .find(|event| {
                event["type"] == "response.output_item.done" && event["item"]["type"] == "message"
            })
            .expect("message done");
        assert_eq!(message_done["item"]["phase"], "commentary");

        let terminal = events.last().expect("terminal event");
        assert_eq!(terminal["type"], "response.completed");
        let output = terminal["response"]["output"]
            .as_array()
            .expect("completed output");
        assert_eq!(output[0]["type"], "message");
        assert_eq!(output[0]["phase"], "commentary");
        assert_eq!(output[1]["type"], "function_call");
    }

    #[tokio::test]
    async fn test_codex_empty_stop_is_incomplete_instead_of_blank_final_answer() {
        let (_, events) = collect_codex_stream(vec![json!({
            "candidates": [{
                "finishReason": "STOP",
                "content": {"parts": [{"text": ""}]}
            }]
        })])
        .await;

        assert!(events.iter().all(|event| {
            !(event["type"] == "response.output_item.added" && event["item"]["type"] == "message")
        }));
        let terminal = events.last().expect("terminal event");
        assert_eq!(terminal["type"], "response.incomplete");
        assert_eq!(terminal["response"]["status"], "incomplete");
        assert_eq!(terminal["response"]["error"]["code"], "empty_response");
    }

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

        let items: Vec<Result<Bytes, reqwest::Error>> =
            vec![Ok(Bytes::from(format!("data: {}\n\n", chunk_json)))];

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
