// OpenAI 流式转换
use bytes::{Bytes, BytesMut};
use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use std::pin::Pin;
use std::sync::{Mutex, OnceLock};
use chrono::Utc;
use uuid::Uuid;
use tracing::debug;
use rand::Rng;

// === 全局 ThoughtSignature 存储 ===
// 用于在流式响应和后续请求之间传递签名，避免嵌入到用户可见的文本中
static GLOBAL_THOUGHT_SIG: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn get_thought_sig_storage() -> &'static Mutex<Option<String>> {
    GLOBAL_THOUGHT_SIG.get_or_init(|| Mutex::new(None))
}

/// 保存 thoughtSignature 到全局存储
/// 注意：只在新签名比现有签名更长时才存储，避免短签名覆盖有效签名
pub fn store_thought_signature(sig: &str) {
    if let Ok(mut guard) = get_thought_sig_storage().lock() {
        let should_store = match &*guard {
            None => true, // 没有签名，直接存储
            Some(existing) => sig.len() > existing.len(), // 只有新签名更长才存储
        };
        
        if should_store {
            tracing::info!("[ThoughtSig] 存储新签名 (长度: {}，替换旧长度: {:?})", 
                sig.len(), 
                guard.as_ref().map(|s| s.len())
            );
            *guard = Some(sig.to_string());
        } else {
            tracing::debug!("[ThoughtSig] 跳过短签名 (新长度: {}，现有长度: {})", 
                sig.len(), 
                guard.as_ref().map(|s| s.len()).unwrap_or(0)
            );
        }
    }
}

/// 获取并清除全局存储的 thoughtSignature
pub fn take_thought_signature() -> Option<String> {
    if let Ok(mut guard) = get_thought_sig_storage().lock() {
        guard.take()
    } else {
        None
    }
}

/// 获取全局存储的 thoughtSignature（不清除）
pub fn get_thought_signature() -> Option<String> {
    if let Ok(guard) = get_thought_sig_storage().lock() {
        guard.clone()
    } else {
        None
    }
}

pub fn create_openai_sse_stream(
    mut gemini_stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    model: String,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>> {
    let mut buffer = BytesMut::new();
    
    let stream = async_stream::stream! {
        while let Some(item) = gemini_stream.next().await {
            match item {
                Ok(bytes) => {
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
                                    tracing::info!("Gemini SSE Chunk: {}", json_part);

                                    // Handle v1internal wrapper if present
                                    let actual_data = if let Some(inner) = json.get_mut("response").map(|v| v.take()) {
                                        inner
                                    } else {
                                        json
                                    };

                                    // Extract components
                                    let candidates = actual_data.get("candidates").and_then(|c| c.as_array());
                                    let candidate = candidates.and_then(|c| c.get(0));
                                    let parts = candidate.and_then(|c| c.get("content")).and_then(|c| c.get("parts")).and_then(|p| p.as_array());

                                    let mut content_out = String::new();
                                    
                                    if let Some(parts_list) = parts {
                                        for part in parts_list {
                                            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                                content_out.push_str(text);
                                            }
                                            // Capture thought (Thinking Models)
                                            if let Some(_thought_text) = part.get("thought").and_then(|t| t.as_str()) {
                                                 // content_out.push_str(thought_text);
                                            }
                                            // 捕获 thoughtSignature (Gemini 3 工具调用必需)
                                            if let Some(sig) = part.get("thoughtSignature").or(part.get("thought_signature")).and_then(|s| s.as_str()) {
                                                store_thought_signature(sig);
                                            }

                                            if let Some(img) = part.get("inlineData") {
                                                let mime_type = img.get("mimeType").and_then(|v| v.as_str()).unwrap_or("image/png");
                                                let data = img.get("data").and_then(|v| v.as_str()).unwrap_or("");
                                                if !data.is_empty() {
                                                    content_out.push_str(&format!("![image](data:{};base64,{})", mime_type, data));
                                                }
                                            }
                                        }
                                    }

                                    // 处理联网搜索引文 (Grounding Metadata) - 流式
                                    if let Some(grounding) = candidate.and_then(|c| c.get("groundingMetadata")) {
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
                                            content_out.push_str(&grounding_text);
                                        }
                                    }

                                    if content_out.is_empty() {
                                        // Skip empty chunks if no text/grounding was found
                                        if candidate.and_then(|c| c.get("finishReason")).is_none() {
                                            continue;
                                        }
                                    }
                                        
                                    // Extract finish reason
                                    let finish_reason = candidate.and_then(|c| c.get("finishReason"))
                                        .and_then(|f| f.as_str())
                                        .map(|f| match f {
                                            "STOP" => "stop",
                                            "MAX_TOKENS" => "length",
                                            "SAFETY" => "content_filter",
                                            _ => f,
                                        });

                                    // Construct OpenAI SSE chunk
                                    let openai_chunk = json!({
                                        "id": format!("chatcmpl-{}", Uuid::new_v4()),
                                        "object": "chat.completion.chunk",
                                        "created": Utc::now().timestamp(),
                                        "model": model,
                                        "choices": [
                                            {
                                                "index": 0,
                                                "delta": {
                                                    "content": content_out
                                                },
                                                "finish_reason": finish_reason
                                            }
                                        ]
                                    });

                                    let sse_out = format!("data: {}\n\n", serde_json::to_string(&openai_chunk).unwrap_or_default());
                                    yield Ok::<Bytes, String>(Bytes::from(sse_out));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    yield Err(format!("Upstream error: {}", e));
                }
            }
        }
        // End of stream signal for OpenAI
        yield Ok::<Bytes, String>(Bytes::from("data: [DONE]\n\n"));
    };

    Box::pin(stream)
}

pub fn create_legacy_sse_stream(
    mut gemini_stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    model: String,
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
        while let Some(item) = gemini_stream.next().await {
            match item {
                Ok(bytes) => {
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
                                                // 捕获 thoughtSignature
                                                // 捕获 thoughtSignature 到全局存储
                                                if let Some(sig) = part.get("thoughtSignature").or(part.get("thought_signature")).and_then(|s| s.as_str()) {
                                                    store_thought_signature(sig);
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
                                    let legacy_chunk = json!({
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

                                    let json_str = serde_json::to_string(&legacy_chunk).unwrap_or_default();
                                    tracing::info!("Legacy Stream Chunk: {}", json_str); 
                                    let sse_out = format!("data: {}\n\n", json_str);
                                    yield Ok::<Bytes, String>(Bytes::from(sse_out));
                                }
                            }
                        }
                    }
                }
                Err(e) => yield Err(format!("Upstream error: {}", e)),
            }
        }
        tracing::info!("Stream finished. Yielding [DONE]");
        yield Ok::<Bytes, String>(Bytes::from("data: [DONE]\n\n"));
        // Final flush delay
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };

    Box::pin(stream)
}

pub fn create_codex_sse_stream(
    mut gemini_stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    _model: String,
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
        yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&created_ev).unwrap())));

        let mut full_content = String::new();
        let mut emitted_tool_calls = std::collections::HashSet::new();
        let mut last_finish_reason = "stop".to_string();

        while let Some(item) = gemini_stream.next().await {
            match item {
                Ok(bytes) => {
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
                                                    // Sanitize smart quotes to standard quotes for JSON compatibility
                                                    let clean_text = text.replace('“', "\"").replace('”', "\"");
                                                    delta_text.push_str(&clean_text);
                                                }
                                                /* 禁用思维链输出到正文
                                                if let Some(thought_text) = part.get("thought").and_then(|t| t.as_str()) {
                                                    let clean_thought = thought_text.replace('"', "\"").replace('"', "\"");
                                                    // delta_text.push_str(&clean_thought);
                                                }
                                                */
                                                // 捕获 thoughtSignature (Gemini 3 工具调用必需)
                                                // 存储到全局状态，不再嵌入到用户可见的文本中
                                                if let Some(sig) = part.get("thoughtSignature").or(part.get("thought_signature")).and_then(|s| s.as_str()) {
                                                    tracing::info!("[Codex-SSE] 捕获 thoughtSignature (长度: {})", sig.len());
                                                    store_thought_signature(sig);
                                                }
                                                // Handle function call in chunk with deduplication
                                                if let Some(func_call) = part.get("functionCall") {
                                                    let call_key = serde_json::to_string(func_call).unwrap_or_default();
                                                    if !emitted_tool_calls.contains(&call_key) {
                                                        emitted_tool_calls.insert(call_key);

                                                                                let name = func_call.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                                                                                let _args = func_call.get("args").unwrap_or(&json!({})).to_string();                                                        
                                                        // Stable ID generation based on hashed content to be consistent
                                                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                                                        use std::hash::{Hash, Hasher};
                                                        serde_json::to_string(func_call).unwrap_or_default().hash(&mut hasher);
                                                        let call_id = format!("call_{:x}", hasher.finish());
                                                        
                                                        // Parse args once
                                                        let fallback_args = json!({});
                                                        let args_obj = func_call.get("args").unwrap_or(&fallback_args);
                                                        // Fallback for function_call arguments string
                                                        let args_str = args_obj.to_string();

                                                        let name_str = name.to_string();
                                                        
                                                        // Determine event type based on tool name
                                                        // 使用 Option 来允许某些情况跳过工具调用
                                                        let maybe_item_added_ev: Option<Value> = if name_str == "shell" || name_str == "local_shell" {
                                                            // Map to local_shell_call
                                                            tracing::info!("[Debug] func_call: {}", serde_json::to_string(&func_call).unwrap_or_default());
                                                            tracing::info!("[Debug] args_obj: {}", serde_json::to_string(&args_obj).unwrap_or_default());
                                                            
                                                            // 解析命令：支持数组格式、字符串格式，以及空 args 情况
                                                            let cmd_vec: Vec<String> = if args_obj.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                                                                // args 为空时使用静默成功命令，避免任务中断
                                                                tracing::warn!("shell command args 为空，使用静默成功命令继续流程");
                                                                vec!["powershell.exe".to_string(), "-Command".to_string(), "exit 0".to_string()]
                                                            } else if let Some(arr) = args_obj.get("command").and_then(|v| v.as_array()) {
                                                                // 数组格式
                                                                arr.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect()
                                                            } else if let Some(cmd_str) = args_obj.get("command").and_then(|v| v.as_str()) {
                                                                // 字符串格式
                                                                if cmd_str.contains(' ') {
                                                                    vec!["powershell.exe".to_string(), "-Command".to_string(), cmd_str.to_string()]
                                                                } else {
                                                                    vec![cmd_str.to_string()]
                                                                }
                                                            } else {
                                                                // command 字段缺失，使用静默成功命令
                                                                tracing::warn!("shell command 缺少 command 字段，使用静默成功命令");
                                                                vec!["powershell.exe".to_string(), "-Command".to_string(), "exit 0".to_string()]
                                                            };
                                                            
                                                            tracing::info!("Shell 命令解析: {:?}", cmd_vec);
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

                                                        // 只有在有事件时才发送
                                                        if let Some(item_added_ev) = maybe_item_added_ev {
                                                            yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&item_added_ev).unwrap())));

                                                        // Emit response.output_item.done (matching the added event)
                                                        // 复用相同的 cmd_vec 逻辑
                                                        let item_done_ev = if name_str == "shell" || name_str == "local_shell" {
                                                            let cmd_vec_done: Vec<String> = if let Some(arr) = args_obj.get("command").and_then(|v| v.as_array()) {
                                                                arr.iter()
                                                                    .filter_map(|v| v.as_str())
                                                                    .map(|s| s.to_string())
                                                                    .collect()
                                                            } else if let Some(cmd_str) = args_obj.get("command").and_then(|v| v.as_str()) {
                                                                if cmd_str.contains(' ') {
                                                                    vec!["powershell.exe".to_string(), "-Command".to_string(), cmd_str.to_string()]
                                                                } else {
                                                                    vec![cmd_str.to_string()]
                                                                }
                                                            } else {
                                                                vec!["powershell.exe".to_string(), "-Command".to_string(), "echo 'Invalid command'".to_string()]
                                                            };
                                                            json!({
                                                                "type": "response.output_item.done",
                                                                "item": {
                                                                    "type": "local_shell_call",
                                                                    "status": "in_progress",
                                                                    "call_id": call_id,
                                                                     "action": {
                                                                        "type": "exec",
                                                                        "command": cmd_vec_done
                                                                    }
                                                                }
                                                            })
                                                        } else if name_str == "googleSearch" || name_str == "web_search" || name_str == "google_search" {
                                                            let query_val = args_obj.get("query").and_then(|v| v.as_str()).unwrap_or("");
                                                             json!({
                                                                "type": "response.output_item.done",
                                                                "item": {
                                                                    "type": "web_search_call",
                                                                    "status": "in_progress",
                                                                    "call_id": call_id,
                                                                    "action": {
                                                                        "type": "search",
                                                                        "query": query_val
                                                                    }
                                                                }
                                                            })
                                                        } else {
                                                            json!({
                                                                "type": "response.output_item.done",
                                                                "item": {
                                                                    "type": "function_call",
                                                                    "name": name,
                                                                    "arguments": args_str,
                                                                    "call_id": call_id
                                                                }
                                                            })
                                                        };

                                                        yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&item_done_ev).unwrap())));
                                                        } // 关闭 if let Some(item_added_ev)
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
                                    yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&delta_ev).unwrap())));
                                }
                            }
                        }
                    }
                }
                Err(e) => yield Err(format!("Upstream error: {}", e)),
            }
        }

        // 3. Emit response.output_item.done
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
        yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&item_done_ev).unwrap())));

        // SSOP: Check full_content for embedded JSON command signatures if no tools were emitted natively
        if emitted_tool_calls.is_empty() {
            // Try to find a JSON block containing "command"
            // Simple heuristic: look for { and }
            // We search for the *last* valid JSON block that has a "command" field, as the model might output reasoning first.
            
            let mut detected_cmd_val = None;
            let mut detected_cmd_type = "unknown";

            // Find all potential JSON start/end indices
            let chars: Vec<char> = full_content.chars().collect();
            let mut depth = 0;
            let mut start_idx = 0;
            
            // Scan for top-level JSON objects
            for (i, c) in chars.iter().enumerate() {
                if *c == '{' {
                    if depth == 0 { start_idx = i; }
                    depth += 1;
                } else if *c == '}' {
                    if depth > 0 {
                        depth -= 1;
                        if depth == 0 {
                            // Found a potential JSON object block [start_idx..=i]
                            let json_str: String = chars[start_idx..=i].iter().collect();
                            if let Ok(val) = serde_json::from_str::<Value>(&json_str) {
                                // Check for "command" field
                                if let Some(cmd_val) = val.get("command") {
                                    // Found a command! Identify type.
                                    // Case 1: "command": ["shell", ...] or ["ls", ...]
                                    if let Some(arr) = cmd_val.as_array() {
                                        if let Some(first) = arr.get(0).and_then(|v| v.as_str()) {
                                            if first == "shell" || first == "powershell" || first == "cmd" || first == "ls" || first == "git" || first == "echo" {
                                                detected_cmd_type = "shell";
                                                detected_cmd_val = Some(cmd_val.clone());
                                            }
                                        }
                                    } 
                                    // Case 2: "command": "shell" (String) and "args": { "command": "..." }
                                    // This matches the user's latest screenshot which failed SSOP.
                                    else if let Some(cmd_str) = cmd_val.as_str() {
                                        if cmd_str == "shell" || cmd_str == "local_shell" {
                                             // Enhanced matching for params/argument
                                             if let Some(args) = val.get("args").or(val.get("arguments")).or(val.get("params")) {
                                                  if let Some(inner_cmd) = args.get("command").or(args.get("code")).or(args.get("argument")) {
                                                      // We construct a synthetic array: ["shell", inner_cmd]
                                                      // So subsequent logic can process it.
                                                      // Actually, let's just grab the inner command string.
                                                      if let Some(inner_cmd_str) = inner_cmd.as_str() {
                                                          detected_cmd_type = "shell";
                                                          detected_cmd_val = Some(json!([inner_cmd_str]));
                                                      }
                                                  }
                                              }
                                        }
                                    }
                                }
                            } else {
                                // Fallback for malformed JSON (e.g. unescaped quotes)
                                // 注意: 使用安全的切片方法避免 UTF-8 边界 panic
                                if (json_str.contains("\"command\": \"shell\"") || json_str.contains("\"command\": \"local_shell\"")) 
                                   && (json_str.contains("\"argument\":") || json_str.contains("\"code\":")) {
                                    
                                    let keys = ["\"argument\":", "\"code\":", "\"command\":"];
                                    for key in keys {
                                        if let Some(pos) = json_str.find(key) {
                                            // 使用安全的 get() 方法替代直接索引
                                            let slice_start = pos + key.len();
                                            if let Some(slice_after_key) = json_str.get(slice_start..) {
                                                if let Some(quote_idx) = slice_after_key.find('"') {
                                                    let val_start_abs = slice_start + quote_idx + 1;
                                                    if let Some(last_quote_idx) = json_str.rfind('"') {
                                                        if last_quote_idx > val_start_abs {
                                                            // 使用 get() 安全获取子字符串
                                                            if let Some(raw_cmd) = json_str.get(val_start_abs..last_quote_idx) {
                                                                detected_cmd_type = "shell";
                                                                detected_cmd_val = Some(json!([raw_cmd]));
                                                                tracing::warn!("SSOP: Recovered malformed JSON command: {}", raw_cmd);
                                                                break;
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if let Some(cmd_val) = detected_cmd_val {
                if detected_cmd_type == "shell" {
                     let mut hasher = std::collections::hash_map::DefaultHasher::new();
                     use std::hash::{Hash, Hasher};
                     "ssop_shell_call".hash(&mut hasher); // Unique seed
                     serde_json::to_string(&cmd_val).unwrap_or_default().hash(&mut hasher);
                     let call_id = format!("call_{:x}", hasher.finish());

                     let mut cmd_vec: Vec<String> = cmd_val.as_array().unwrap().iter().map(|v| v.as_str().unwrap_or("").to_string()).collect();
                     
                     // Helper to ensure it runs in shell properly
                     // Problem: Model often outputs ["shell", "powershell", "-Command", ...]
                     // "shell" is not a valid executable on Windows. We must strip it if it's acting as a label.
                     if !cmd_vec.is_empty() && (cmd_vec[0] == "shell" || cmd_vec[0] == "local_shell") {
                         cmd_vec.remove(0);
                     }

                     // Now check if empty or needs wrapping
                     let final_cmd_vec = if cmd_vec.is_empty() {
                         vec!["powershell".to_string(), "-Command".to_string(), "echo 'Empty command'".to_string()]
                     } else if cmd_vec[0] == "powershell" || cmd_vec[0] == "cmd" || cmd_vec[0] == "git" || cmd_vec[0] == "python" || cmd_vec[0] == "node" {
                         cmd_vec
                     } else {
                         // Wrap generic commands (ls, dir, echo, etc) in powershell for Windows safety
                        // Use EncodedCommand to avoid quoting hell
                        // AND pipe to Out-String to avoid CLIXML object output which breaks Gemini
                        let raw_cmd = cmd_vec.join(" ");
                        let joined = format!("& {{ {} }} | Out-String", raw_cmd);
                        let utf16: Vec<u16> = joined.encode_utf16().collect();
                        let mut bytes = Vec::with_capacity(utf16.len() * 2);
                        for c in utf16 {
                            bytes.extend_from_slice(&c.to_le_bytes());
                        }
                        use base64::Engine as _;
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        
                        vec!["powershell".to_string(), "-EncodedCommand".to_string(), b64]
                    };

                     tracing::info!("SSOP: Detected Shell Command in Text, Injecting Event: {:?}", final_cmd_vec);

                     // Emit added
                     let item_added_ev = json!({
                        "type": "response.output_item.added",
                        "item": {
                            "type": "local_shell_call",
                            "status": "in_progress",
                            "call_id": &call_id,
                            "action": {
                                "type": "exec",
                                "command": final_cmd_vec
                            }
                        }
                    });
                    yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&item_added_ev).unwrap())));

                    // Emit done
                    let item_done_ev = json!({
                        "type": "response.output_item.done",
                        "item": {
                            "type": "local_shell_call",
                            "status": "in_progress",
                            "call_id": &call_id,
                             "action": {
                                "type": "exec",
                                "command": final_cmd_vec
                            }
                        }
                    });
                    yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&item_done_ev).unwrap())));
                }
            }
        }

        // 4. Emit response.completed
        let completed_ev = json!({
            "type": "response.completed",
            "response": {
                "id": &response_id,
                "object": "response",
                "status": "completed",
                "finish_reason": last_finish_reason,
                "usage": {
                    "input_tokens": 0,
                    "input_tokens_details": { "cached_tokens": 0 },
                    "output_tokens": 0,
                    "output_tokens_details": { "reasoning_tokens": 0 },
                    "total_tokens": 0
                }
            }
        });
        yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&completed_ev).unwrap())));
    };

    Box::pin(stream)
}
