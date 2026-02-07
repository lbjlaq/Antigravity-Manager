// OpenAI 流式转换
use bytes::{Bytes, BytesMut};
use chrono::Utc;
use futures::{Stream, StreamExt};
use rand::Rng;
use serde_json::{json, Value};
use std::pin::Pin;
use tracing::debug;
use uuid::Uuid;

pub fn store_thought_signature(sig: &str, session_id: &str, message_count: usize) {
    if sig.len() < 50 {
        return;
    }

    crate::proxy::SignatureCache::global().cache_session_signature(
        session_id,
        sig.to_string(),
        message_count,
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

    Some(OpenAIUsage {
        prompt_tokens,
        completion_tokens,
        total_tokens,
        prompt_tokens_details: cached_tokens.map(|ct| PromptTokensDetails {
            cached_tokens: Some(ct),
        }),
        completion_tokens_details: None,
    })
}

pub fn create_openai_sse_stream(
    mut gemini_stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    model: String,
    session_id: String,
    message_count: usize,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>> {
    let mut buffer = BytesMut::new();

    // 在流开始时生成固定的 ID 和 timestamp，所有 chunk 共用
    let stream_id = format!("chatcmpl-{}", Uuid::new_v4());
    let created_ts = Utc::now().timestamp();

    let stream = async_stream::stream! {
        let mut emitted_tool_calls = std::collections::HashSet::new();
        let mut final_usage: Option<super::models::OpenAIUsage> = None;
        let mut error_occurred = false;  // [FIX] 标志位,避免双重 [DONE]

        // [P2 FIX] 添加心跳定时器
        let mut heartbeat_interval = tokio::time::interval(std::time::Duration::from_secs(15));
        heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                // 处理上游数据
                item = gemini_stream.next() => {
                    match item {
                        Some(Ok(bytes)) => {
                    // Verbose logging for debugging image fragmentation
                    debug!("[OpenAI-SSE] Received chunk: {} bytes", bytes.len());
                    buffer.extend_from_slice(&bytes);

                    // Process complete lines from buffer
                    while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                        let line_raw = buffer.split_to(pos + 1);
                        if let Ok(line_str) = std::str::from_utf8(&line_raw) {
                            let line = line_str.trim();
                            if line.is_empty() { continue; }

                            if line.starts_with("data: ") {
                                let json_part = line.trim_start_matches("data: ").trim();
                                if json_part == "[DONE]" {
                                    continue;
                                }

                                if let Ok(mut json) = serde_json::from_str::<Value>(json_part) {
                                    // Log raw chunk for debugging gemini-3 thoughts
                                    tracing::debug!("Gemini SSE Chunk: {}", json_part);

                                    // Handle v1internal wrapper if present
                                    let actual_data = if let Some(inner) = json.get_mut("response").map(|v| v.take()) {
                                        inner
                                    } else {
                                        json
                                    };

                                    // Capture usageMetadata if present
                                    if let Some(u) = actual_data.get("usageMetadata") {
                                        final_usage = extract_usage_metadata(u);
                                    }

                                    // Extract candidates
                                    if let Some(candidates) = actual_data.get("candidates").and_then(|c| c.as_array()) {
                                        for (idx, candidate) in candidates.iter().enumerate() {
                                            let parts = candidate.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array());

                                            let mut content_out = String::new();
                                            let mut thought_out = String::new();

                                            if let Some(parts_list) = parts {
                                                for part in parts_list {
                                                    let is_thought_part = part.get("thought")
                                                        .and_then(|v| v.as_bool())
                                                        .unwrap_or(false);

                                                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                                        if is_thought_part {
                                                            thought_out.push_str(text);
                                                        } else {
                                                            content_out.push_str(text);
                                                        }
                                                    }
                                                    // 捕获 thoughtSignature (Gemini 3 工具调用必需)
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

                                                    // Handle function call
                                                    if let Some(func_call) = part.get("functionCall") {
                                                        let call_key = serde_json::to_string(func_call).unwrap_or_default();
                                                        if !emitted_tool_calls.contains(&call_key) {
                                                            emitted_tool_calls.insert(call_key);

                                                            let name = func_call.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                                                            let args = func_call.get("args").unwrap_or(&json!({})).to_string();

                                                            // Generate stable ID
                                                            let mut hasher = std::collections::hash_map::DefaultHasher::new();
                                                            use std::hash::{Hash, Hasher};
                                                            serde_json::to_string(func_call).unwrap_or_default().hash(&mut hasher);
                                                            let call_id = format!("call_{:x}", hasher.finish());

                                                            // Emit tool_calls delta
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
                                                                            "index": 0,
                                                                            "id": call_id,
                                                                            "type": "function",
                                                                            "function": {
                                                                                "name": name,
                                                                                "arguments": args
                                                                            }
                                                                        }]
                                                                    },
                                                                    "finish_reason": serde_json::Value::Null
                                                                }]
                                                            });

                                                            let sse_out = format!("data: {}\n\n", serde_json::to_string(&tool_call_chunk).unwrap_or_default());
                                                            yield Ok::<Bytes, String>(Bytes::from(sse_out));
                                                        }
                                                    }
                                                }
                                            }


                                            // 处理联网搜索引文 (Grounding Metadata) - 流式
                                            if let Some(grounding) = candidate.get("groundingMetadata") {
                                                let mut grounding_text = String::new();

                                                // 1. 处理搜索词
                                                if let Some(queries) = grounding.get("webSearchQueries").and_then(|q| q.as_array()) {
                                                    let query_list: Vec<&str> = queries.iter().filter_map(|v| v.as_str()).collect();
                                                    if !query_list.is_empty() {
                                                        grounding_text.push_str("\n\n---\n**🔍 已为您搜索：** ");
                                                        grounding_text.push_str(&query_list.join(", "));
                                                    }
                                                }

                                                // 2. 处理来源链接 (Chunks)
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
                                                    content_out.push_str(&grounding_text);
                                                }
                                            }

                                            // 只有当 content 和 thought 都为空时才跳过
                                            if content_out.is_empty() && thought_out.is_empty() {
                                                // Skip empty chunks if no text/grounding/thought was found
                                                if candidate.get("finishReason").is_none() {
                                                    continue;
                                                }
                                            }

                                            // Extract finish reason
                                            let finish_reason = candidate.get("finishReason")
                                                .and_then(|f| f.as_str())
                                                .map(|f| match f {
                                                    "STOP" => "stop",
                                                    "MAX_TOKENS" => "length",
                                                    "SAFETY" => "content_filter",
                                                    "RECITATION" => "content_filter",
                                                    _ => f,
                                                });

                                            // Construct OpenAI SSE chunk
                                            // 如果有思考内容，先发送 reasoning_content chunk
                                            if !thought_out.is_empty() {
                                                let reasoning_chunk = json!({
                                                    "id": &stream_id,
                                                    "object": "chat.completion.chunk",
                                                    "created": created_ts,
                                                    "model": model,
                                                    "choices": [
                                                        {
                                                            "index": idx as u32,
                                                            "delta": {
                                                                "role": "assistant",
                                                                "content": serde_json::Value::Null,
                                                                "reasoning_content": thought_out
                                                            },
                                                            "finish_reason": serde_json::Value::Null
                                                        }
                                                    ]
                                                });
                                                let sse_out = format!("data: {}\n\n", serde_json::to_string(&reasoning_chunk).unwrap_or_default());
                                                yield Ok::<Bytes, String>(Bytes::from(sse_out));
                                            }

                                            // 发送正常 content chunk
                                            if !content_out.is_empty() || finish_reason.is_some() {
                                                let mut openai_chunk = json!({
                                                    "id": &stream_id,
                                                    "object": "chat.completion.chunk",
                                                    "created": created_ts,
                                                    "model": model,
                                                    "choices": [
                                                        {
                                                            "index": idx as u32,
                                                            "delta": {
                                                                "content": content_out
                                                            },
                                                            "finish_reason": finish_reason
                                                        }
                                                    ]
                                                });

                                                // [FIX] 将 usage 嵌入到 chunk 中
                                                if let Some(ref usage) = final_usage {
                                                     if let Ok(val) = serde_json::to_value(usage) {
                                                         openai_chunk["usage"] = val;
                                                     }
                                                }

                                                // [FIX] 如果是最后一个 chunk,标记 usage 已发送
                                                if finish_reason.is_some() {
                                                    final_usage = None;
                                                }

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
                            let (error_type, user_message, i18n_key) = classify_stream_error(&e);

                            tracing::error!(
                                error_type = %error_type,
                                user_message = %user_message,
                                i18n_key = %i18n_key,
                                raw_error = %e,
                                "OpenAI stream error occurred"
                            );

                            // 发送友好的 SSE 错误事件(包含 i18n_key 供前端翻译)
                            let error_chunk = json!({
                                "id": &stream_id,
                                "object": "chat.completion.chunk",
                                "created": created_ts,
                                "model": &model,
                                "choices": [],
                                "error": {
                                    "type": error_type,
                                    "message": user_message,
                                    "code": "stream_error",
                                    "i18n_key": i18n_key
                                }
                            });

                            let sse_out = format!("data: {}\n\n", serde_json::to_string(&error_chunk).unwrap_or_default());
                            yield Ok(Bytes::from(sse_out));
                            yield Ok(Bytes::from("data: [DONE]\n\n"));
                            error_occurred = true;  // [FIX] 标记错误已发生
                            break;
                        }
                        None => {
                            // 流结束
                            break;
                        }
                    }
                }

                // [P2 FIX] 发送心跳
                _ = heartbeat_interval.tick() => {
                    // 发送 SSE 注释作为心跳
                    yield Ok::<Bytes, String>(Bytes::from(": ping\n\n"));
                }
            }
        }

        // [FIX] 只有在没有错误时才发送 [DONE]
        // usage 已经嵌入到 finish_reason chunk,不需要单独发送
        if !error_occurred {
            yield Ok::<Bytes, String>(Bytes::from("data: [DONE]\n\n"));
        }
    };

    Box::pin(stream)
}

pub fn create_legacy_sse_stream(
    mut gemini_stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    model: String,
    session_id: String,
    message_count: usize,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>> {
    let mut buffer = BytesMut::new();

    // Generate constant alphanumeric ID (mimics OpenAI base62 format)
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
        let mut error_occurred = false;  // [FIX] 标志位,避免双重 [DONE]

        // [P2 FIX] 添加心跳定时器
        let mut heartbeat_interval = tokio::time::interval(std::time::Duration::from_secs(15));
        heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                // 处理上游数据
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

                                    // Capture usageMetadata if present
                                    if let Some(u) = actual_data.get("usageMetadata") {
                                        final_usage = extract_usage_metadata(u);
                                    }

                                    let mut content_out = String::new();
                                    if let Some(candidates) = actual_data.get("candidates").and_then(|c| c.as_array()) {
                                        if let Some(parts) = candidates.get(0).and_then(|c| c.get("content")).and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                                            for part in parts {
                                                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                                    content_out.push_str(text);
                                                }
                                                /* 禁用思维链输出到正文
                                                if let Some(thought_text) = part.get("thought").and_then(|t| t.as_str()) {
                                                    // // content_out.push_str(thought_text);
                                                }
                                                */
                                                // Capture thoughtSignature into session cache
                                                if let Some(sig) = part.get("thoughtSignature").or(part.get("thought_signature")).and_then(|s| s.as_str()) {
                                                    store_thought_signature(sig, &session_id, message_count);
                                                }
                                            }
                                        }
                                    }

                                    let finish_reason = actual_data.get("candidates")
                                        .and_then(|c| c.as_array())
                                        .and_then(|c| c.get(0))
                                        .and_then(|c| c.get("finishReason"))
                                        .and_then(|f| f.as_str())
                                        .map(|f| match f {
                                            "STOP" => "stop",
                                            "MAX_TOKENS" => "length",
                                            "SAFETY" => "content_filter",
                                            _ => f,
                                        });

                                    // Construct LEGACY completion chunk - STRICT VERSION
                                    let mut legacy_chunk = json!({
                                        "id": &stream_id,
                                        "object": "text_completion",
                                        "created": created_ts,
                                        "model": &model,
                                        "choices": [
                                            {
                                                "text": content_out,
                                                "index": 0,
                                                "logprobs": null,
                                                "finish_reason": finish_reason // Will be null if None
                                            }
                                        ]
                                    });

                                    // [FIX] 将 usage 嵌入到 chunk 中
                                    if let Some(ref usage) = final_usage {
                                        if let Ok(val) = serde_json::to_value(usage) {
                                            legacy_chunk["usage"] = val;
                                        }
                                    }

                                    // [FIX] 如果是最后一个 chunk,标记 usage 已发送
                                    if finish_reason.is_some() {
                                        final_usage = None;
                                    }

                                    let json_str = serde_json::to_string(&legacy_chunk).unwrap_or_default();
                                    tracing::debug!("Legacy Stream Chunk: {}", json_str);
                                    let sse_out = format!("data: {}\n\n", json_str);
                                    yield Ok::<Bytes, String>(Bytes::from(sse_out));
                                }
                            }
                        }
                    }
                }
                        Some(Err(e)) => {
                            use crate::proxy::mappers::error_classifier::classify_stream_error;
                            let (error_type, user_message, i18n_key) = classify_stream_error(&e);

                            tracing::error!(
                                error_type = %error_type,
                                user_message = %user_message,
                                i18n_key = %i18n_key,
                                raw_error = %e,
                                "Legacy stream error occurred"
                            );

                            // 发送友好的 SSE 错误事件(包含 i18n_key 供前端翻译)
                            let error_chunk = json!({
                                "id": &stream_id,
                                "object": "text_completion",
                                "created": created_ts,
                                "model": &model,
                                "choices": [],
                                "error": {
                                    "type": error_type,
                                    "message": user_message,
                                    "code": "stream_error",
                                    "i18n_key": i18n_key
                                }
                            });

                            let sse_out = format!("data: {}\n\n", serde_json::to_string(&error_chunk).unwrap_or_default());
                            yield Ok(Bytes::from(sse_out));
                            yield Ok(Bytes::from("data: [DONE]\n\n"));
                            error_occurred = true;  // [FIX] 标记错误已发生
                            break;
                        }
                        None => {
                            // 流结束
                            break;
                        }
                    }
                }

                // [P2 FIX] 发送心跳
                _ = heartbeat_interval.tick() => {
                    // 发送 SSE 注释作为心跳
                    yield Ok::<Bytes, String>(Bytes::from(": ping\n\n"));
                }
            }
        }

        // [FIX] 只有在没有错误时才发送 [DONE]
        // usage 已经嵌入到 finish_reason chunk,不需要单独发送
        if !error_occurred {
            tracing::debug!("Stream finished. Yielding [DONE]");
            yield Ok::<Bytes, String>(Bytes::from("data: [DONE]\n\n"));
            // Final flush delay
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    };

    Box::pin(stream)
}

pub fn create_codex_sse_stream(
    mut gemini_stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    _model: String,
    session_id: String,
    message_count: usize,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>> {
    let mut buffer = BytesMut::new();

    // Generate alphanumeric ID
    let charset = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    let random_str: String = (0..24)
        .map(|_| {
            let idx = rng.gen_range(0..charset.len());
            charset.chars().nth(idx).unwrap()
        })
        .collect();
    let response_id = format!("resp-{}", random_str);

    let stream = async_stream::stream! {
        // 1. Emit response.created
        let created_ev = json!({
            "type": "response.created",
            "response": {
                "id": &response_id,
                "object": "response"
            }
        });
        let created_json = serde_json::to_string(&created_ev).unwrap_or_else(|_| "{}".to_string());
        yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", created_json)));

        let mut full_content = String::new();
        let mut emitted_tool_calls = std::collections::HashSet::new();
        let mut last_finish_reason = "stop".to_string();
        let mut accumulated_usage: Option<super::models::OpenAIUsage> = None;

        // [P2 FIX] Add heartbeat interval for Codex stream
        let mut heartbeat_interval = tokio::time::interval(std::time::Duration::from_secs(15));
        heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                // Heartbeat
                _ = heartbeat_interval.tick() => {
                    yield Ok::<Bytes, String>(Bytes::from(": ping\n\n"));
                }

                // Upstream data
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

                                        // Capture usageMetadata if present
                                        if let Some(u) = actual_data.get("usageMetadata") {
                                            accumulated_usage = extract_usage_metadata(u);
                                        }

                                        // Capture finish reason
                                        if let Some(candidates) = actual_data.get("candidates").and_then(|c| c.as_array()) {
                                            if let Some(candidate) = candidates.get(0) {
                                                if let Some(reason) = candidate.get("finishReason").and_then(|r| r.as_str()) {
                                                    last_finish_reason = match reason {
                                                        "STOP" => "stop".to_string(),
                                                        "MAX_TOKENS" => "length".to_string(),
                                                        _ => "stop".to_string(),
                                                    };
                                                }
                                            }
                                        }

                                        // text delta
                                        let mut delta_text = String::new();
                                        if let Some(candidates) = actual_data.get("candidates").and_then(|c| c.as_array()) {
                                            if let Some(candidate) = candidates.get(0) {
                                                if let Some(parts) = candidate.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                                                    for part in parts {
                                                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                                            let clean_text = text.replace('“', "\"").replace('”', "\"");
                                                            delta_text.push_str(&clean_text);
                                                        }

                                                        // 捕获 thoughtSignature
                                                        if let Some(sig) = part.get("thoughtSignature").or(part.get("thought_signature")).and_then(|s| s.as_str()) {
                                                            tracing::debug!("[Codex-SSE] 捕获 thoughtSignature (长度: {})", sig.len());
                                                            store_thought_signature(sig, &session_id, message_count);
                                                        }

                                                        // Handle function call in chunk with deduplication
                                                        if let Some(func_call) = part.get("functionCall") {
                                                            let call_key = serde_json::to_string(func_call).unwrap_or_default();
                                                            if !emitted_tool_calls.contains(&call_key) {
                                                                emitted_tool_calls.insert(call_key);

                                                                let name = func_call.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                                                                let name_str = name.to_string();

                                                                let fallback_args = json!({});
                                                                let args_obj = func_call.get("args").unwrap_or(&fallback_args);
                                                                let args_str = args_obj.to_string();

                                                                // Use content-based hash for call_id
                                                                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                                                                use std::hash::{Hash, Hasher};
                                                                name_str.hash(&mut hasher);
                                                                args_str.hash(&mut hasher);
                                                                let call_id = format!("call_{:x}", hasher.finish());

                                                                // Determine event type based on tool name
                                                                let maybe_item_added_ev: Option<Value> = if name_str == "shell" || name_str == "local_shell" {
                                                                    // Map to local_shell_call
                                                                    let cmd_vec: Vec<String> = if args_obj.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                                                                        vec!["powershell.exe".to_string(), "-Command".to_string(), "exit 0".to_string()]
                                                                    } else if let Some(arr) = args_obj.get("command").and_then(|v| v.as_array()) {
                                                                        arr.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect()
                                                                    } else if let Some(cmd_str) = args_obj.get("command").and_then(|v| v.as_str()) {
                                                                        if cmd_str.contains(' ') {
                                                                            vec!["powershell.exe".to_string(), "-Command".to_string(), cmd_str.to_string()]
                                                                        } else {
                                                                            vec![cmd_str.to_string()]
                                                                        }
                                                                    } else {
                                                                        vec!["powershell.exe".to_string(), "-Command".to_string(), "exit 0".to_string()]
                                                                    };

                                                                    Some(json!({
                                                                        "type": "response.output_item.added",
                                                                        "item": {
                                                                            "type": "local_shell_call",
                                                                            "status": "in_progress",
                                                                            "call_id": &call_id,
                                                                            "action": {
                                                                                "type": "exec",
                                                                                "command": cmd_vec
                                                                            }
                                                                        }
                                                                    }))
                                                                } else if name_str == "googleSearch" || name_str == "web_search" || name_str == "google_search" {
                                                                    // Map to web_search_call
                                                                    let query_val = args_obj.get("query").and_then(|v| v.as_str()).unwrap_or("");
                                                                    Some(json!({
                                                                        "type": "response.output_item.added",
                                                                        "item": {
                                                                            "type": "web_search_call",
                                                                            "status": "in_progress",
                                                                            "call_id": &call_id,
                                                                            "action": {
                                                                                "type": "search",
                                                                                "query": query_val
                                                                            }
                                                                        }
                                                                    }))
                                                                } else {
                                                                    // Default function_call
                                                                    Some(json!({
                                                                        "type": "response.output_item.added",
                                                                        "item": {
                                                                            "type": "function_call",
                                                                            "name": name,
                                                                            "arguments": args_str,
                                                                            "call_id": &call_id
                                                                        }
                                                                    }))
                                                                };

                                                                if let Some(item_added_ev) = maybe_item_added_ev {
                                                                    let added_json = serde_json::to_string(&item_added_ev).unwrap_or_else(|_| "{}".to_string());
                                                                    yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", added_json)));

                                                                    // Emit response.output_item.done
                                                                    let mut item_done_ev = item_added_ev.clone();
                                                                    if let Some(obj) = item_done_ev.as_object_mut() {
                                                                        obj.insert("type".to_string(), json!("response.output_item.done"));
                                                                    }
                                                                    let done_json = serde_json::to_string(&item_done_ev).unwrap_or_else(|_| "{}".to_string());
                                                                    yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", done_json)));
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        if !delta_text.is_empty() {
                                            full_content.push_str(&delta_text);
                                            // 2. Emit response.output_text.delta
                                            let delta_ev = json!({
                                                "type": "response.output_text.delta",
                                                "delta": delta_text
                                            });
                                            let delta_json = serde_json::to_string(&delta_ev).unwrap_or_else(|_| "{}".to_string());
                                            yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", delta_json)));
                                        }
                                    }
                                }
                            }
                        }
                        Some(Err(e)) => {
                            use crate::proxy::mappers::error_classifier::classify_stream_error;
                            let (error_type, user_message, i18n_key) = classify_stream_error(&e);
                            let error_ev = json!({
                                "type": "error",
                                "error": {
                                    "type": error_type,
                                    "message": user_message,
                                    "code": "stream_error",
                                    "i18n_key": i18n_key
                                }
                            });
                            let error_json = serde_json::to_string(&error_ev).unwrap_or_else(|_| "{}".to_string());
                            yield Ok(Bytes::from(format!("data: {}\n\n", error_json)));
                            break;
                        }
                        None => {
                            break;
                        }
                    }
                }
            }
        }

        // 3. Emit response.output_item.done for the main message
        let item_done_ev = json!({
            "type": "response.output_item.done",
            "item": {
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "output_text",
                        "text": full_content
                    }
                ]
            }
        });
        let item_done_json = serde_json::to_string(&item_done_ev).unwrap_or_else(|_| "{}".to_string());
        yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", item_done_json)));

        // 4. Emit response.completed
        let completed_ev = json!({
            "type": "response.completed",
            "response": {
                "id": &response_id,
                "object": "response",
                "status": "completed",
                "finish_reason": last_finish_reason,
                "usage": accumulated_usage.map(|u| json!({
                    "input_tokens": u.prompt_tokens,
                    "input_tokens_details": { "cached_tokens": 0 },
                    "output_tokens": u.completion_tokens,
                    "output_tokens_details": { "reasoning_tokens": 0 },
                    "total_tokens": u.total_tokens
                })).unwrap_or(json!({
                    "input_tokens": 0,
                    "input_tokens_details": { "cached_tokens": 0 },
                    "output_tokens": 0,
                    "output_tokens_details": { "reasoning_tokens": 0 },
                    "total_tokens": 0
                }))
            }
        });
        let completed_json = serde_json::to_string(&completed_ev).unwrap_or_else(|_| "{}".to_string());
        yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", completed_json)));
    };

    Box::pin(stream)
}
