use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
    body::Body,
};
use std::time::Instant;
use crate::proxy::server::AppState;
use crate::proxy::monitor::ProxyRequestLog;
use serde_json::Value;
use crate::proxy::middleware::auth::UserTokenIdentity;
use futures::StreamExt;

const MAX_REQUEST_LOG_SIZE: usize = 100 * 1024 * 1024; // 100MB
const MAX_RESPONSE_LOG_SIZE: usize = 100 * 1024 * 1024; // 100MB for image responses

/// Helper function to record User Token usage
fn record_user_token_usage(
    user_token_identity: &Option<UserTokenIdentity>,
    log: &ProxyRequestLog,
    user_agent: Option<String>,
) {
    if let Some(identity) = user_token_identity {
        let _ = crate::modules::user_token_db::record_token_usage_and_ip(
            &identity.token_id,
            log.client_ip.as_deref().unwrap_or("127.0.0.1"),
            log.model.as_deref().unwrap_or("unknown"),
            log.input_tokens.unwrap_or(0) as i32,
            log.output_tokens.unwrap_or(0) as i32,
            log.status as u16,
            user_agent,
        );
    }
}

pub async fn monitor_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let _logging_enabled = state.monitor.is_enabled();
    
    let method = request.method().to_string();
    let uri = request.uri().to_string();
    
    if uri.contains("event_logging") || uri.contains("/api/") || uri.starts_with("/internal/") {
        return next.run(request).await;
    }
    
    let start = Instant::now();
    
    // Extract client IP from headers (X-Forwarded-For or X-Real-IP)
    // IMPORTANT: Extract from Request headers, not Response headers (since we want the client's IP)
    // Note: We need to do this BEFORE consuming the request body if possible, or extract it from the original request
    let client_ip = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
        .or_else(|| {
            request
                .headers()
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        });
        
    let user_agent = request
        .headers()
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let mut model = if uri.contains("/v1beta/models/") {
        uri.split("/v1beta/models/")
            .nth(1)
            .and_then(|s| s.split(':').next())
            .map(|s| s.to_string())
    } else {
        None
    };

    let request_body_str;
    
    // [FIX] 从请求 extensions 提取 UserTokenIdentity (由 Auth 中间件注入)
    // 必须在处理 request body 之前提取，因为 into_parts() 后需要保留这个值
    let user_token_identity = request.extensions().get::<UserTokenIdentity>().cloned();
    
    let request = if method == "POST" {
        let (parts, body) = request.into_parts();
        match axum::body::to_bytes(body, MAX_REQUEST_LOG_SIZE).await {
            Ok(bytes) => {
                if model.is_none() {
                    model = serde_json::from_slice::<Value>(&bytes).ok().and_then(|v|
                        v.get("model").and_then(|m| m.as_str()).map(|s| s.to_string())
                    );
                }
                request_body_str = if let Ok(s) = std::str::from_utf8(&bytes) {
                    Some(s.to_string())
                } else {
                    Some("[Binary Request Data]".to_string())
                };
                Request::from_parts(parts, Body::from(bytes))
            }
            Err(_) => {
                request_body_str = None;
                Request::from_parts(parts, Body::empty())
            }
        }
    } else {
        request_body_str = None;
        request
    };
    
    let response = next.run(request).await;
    
    // user_token_identity 已在上面从请求 extensions 中提取
    
    let duration = start.elapsed().as_millis() as u64;
    let status = response.status().as_u16();
    
    let content_type = response.headers().get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Extract account email from X-Account-Email header if present
    let account_email = response
        .headers()
        .get("X-Account-Email")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Extract mapped model from X-Mapped-Model header if present
    let mapped_model = response
        .headers()
        .get("X-Mapped-Model")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let cursor_payload_kind = response
        .headers()
        .get("X-Cursor-Payload-Kind")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Determine protocol from URL path
    let protocol = if uri.contains("/cursor/chat/completions") {
        Some("openai".to_string())
    } else if uri.contains("/v1/messages") {
        Some("anthropic".to_string())
    } else if uri.contains("/v1beta/models") {
        Some("gemini".to_string())
    } else if uri.starts_with("/v1/") {
        Some("openai".to_string())
    } else {
        None
    };

    // Client IP has been extracted at the beginning of the function

    // Extract username from UserTokenIdentity if present
    let username = user_token_identity.as_ref().map(|identity| identity.username.clone());

    let monitor = state.monitor.clone();
    let mut log = ProxyRequestLog {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        method,
        url: uri,
        status,
        duration,
        model,
        mapped_model,
        account_email,
        client_ip,
        error: None,
        request_body: request_body_str,
        response_body: None,
        input_tokens: None,
        output_tokens: None,
        protocol,
        username,
        cursor_payload_kind,
    };


    if content_type.contains("text/event-stream") {
        let (parts, body) = response.into_parts();
        let mut stream = body.into_data_stream();
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        
        tokio::spawn(async move {
            let mut all_stream_data = Vec::new();
            let mut last_few_bytes = Vec::new();
            
            while let Some(chunk_res) = stream.next().await {
                if let Ok(chunk) = chunk_res {
                    all_stream_data.extend_from_slice(&chunk);
                    
                    if chunk.len() > 8192 {
                        last_few_bytes = chunk.slice(chunk.len()-8192..).to_vec();
                    } else {
                        last_few_bytes.extend_from_slice(&chunk);
                        if last_few_bytes.len() > 8192 {
                            last_few_bytes.drain(0..last_few_bytes.len()-8192);
                        }
                    }
                    let _ = tx.send(Ok::<_, axum::Error>(chunk)).await;
                } else if let Err(e) = chunk_res {
                    let _ = tx.send(Err(axum::Error::new(e))).await;
                }
            }
            
            // Parse and consolidate stream data into readable format
            if let Ok(full_response) = std::str::from_utf8(&all_stream_data) {
                let mut thinking_content = String::new();
                let mut response_content = String::new();
                let mut thinking_signature = String::new();
                let mut tool_calls: Vec<Value> = Vec::new();
                
                for line in full_response.lines() {
                    if !line.starts_with("data: ") {
                        continue;
                    }
                    let json_str = line.trim_start_matches("data: ").trim();
                    if json_str == "[DONE]" {
                        continue;
                    }
                    
                    if let Ok(json) = serde_json::from_str::<Value>(json_str) {
                        // OpenAI format: choices[0].delta.content / reasoning_content / tool_calls
                        if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
                            for choice in choices {
                                if let Some(delta) = choice.get("delta") {
                                    // Thinking/reasoning content
                                    if let Some(thinking) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
                                        thinking_content.push_str(thinking);
                                    }
                                    // Main response content
                                    if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                        response_content.push_str(content);
                                    }
                                    // Tool calls
                                    if let Some(delta_tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                                        for tc in delta_tool_calls {
                                            if let Some(index) = tc.get("index").and_then(|i| i.as_u64()) {
                                                let idx = index as usize;
                                                while tool_calls.len() <= idx {
                                                    tool_calls.push(serde_json::json!({
                                                        "id": "",
                                                        "type": "function",
                                                        "function": { "name": "", "arguments": "" }
                                                    }));
                                                }
                                                let current_tc = &mut tool_calls[idx];
                                                if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                                                    current_tc["id"] = Value::String(id.to_string());
                                                }
                                                if let Some(func) = tc.get("function") {
                                                    if let Some(name) = func.get("name").and_then(|v| v.as_str()) {
                                                        current_tc["function"]["name"] = Value::String(name.to_string());
                                                    }
                                                    if let Some(args) = func.get("arguments").and_then(|v| v.as_str()) {
                                                        let old_args = current_tc["function"]["arguments"].as_str().unwrap_or("");
                                                        current_tc["function"]["arguments"] = Value::String(format!("{}{}", old_args, args));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        
                        // Claude/Anthropic format: content_block_start, content_block_delta, etc.
                        let msg_type = json.get("type").and_then(|t| t.as_str());
                        match msg_type {
                            Some("content_block_start") => {
                                if let (Some(index), Some(block)) = (json.get("index").and_then(|i| i.as_u64()), json.get("content_block")) {
                                    let idx = index as usize;
                                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                                        let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                                        let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                        while tool_calls.len() <= idx {
                                            tool_calls.push(Value::Null);
                                        }
                                        tool_calls[idx] = serde_json::json!({
                                            "id": id,
                                            "type": "function",
                                            "function": { "name": name, "arguments": "" }
                                        });
                                    }
                                }
                            }
                            Some("content_block_delta") => {
                                if let (Some(index), Some(delta)) = (json.get("index").and_then(|i| i.as_u64()), json.get("delta")) {
                                    let idx = index as usize;
                                    
                                    // Tool use input delta
                                    if let Some(delta_json) = delta.get("input_json_delta").and_then(|v| v.as_str()) {
                                        if idx < tool_calls.len() && !tool_calls[idx].is_null() {
                                            let old_args = tool_calls[idx]["function"]["arguments"].as_str().unwrap_or("");
                                            tool_calls[idx]["function"]["arguments"] = Value::String(format!("{}{}", old_args, delta_json));
                                        }
                                    }
                                    // Legacy/Native thinking block
                                    if let Some(thinking) = delta.get("thinking").and_then(|v| v.as_str()) {
                                        thinking_content.push_str(thinking);
                                    }
                                    // Text content
                                    if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                        response_content.push_str(text);
                                    }
                                }
                            }
                            Some("message_delta") => {
                                if let Some(delta) = json.get("delta") {
                                    if let Some(usage) = delta.get("usage") {
                                        if let Some(output_tokens) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                                            log.output_tokens = Some(output_tokens as u32);
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                        
                        // Legacy Claude delta (for older implementations or simplified streams)
                        if msg_type.is_none() {
                            if let Some(delta) = json.get("delta") {
                                // Thinking block
                                if let Some(thinking) = delta.get("thinking").and_then(|v| v.as_str()) {
                                    thinking_content.push_str(thinking);
                                }
                                // Thinking signature
                                if let Some(sig) = delta.get("signature").and_then(|v| v.as_str()) {
                                    thinking_signature = sig.to_string();
                                }
                                // Text content
                                if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                    response_content.push_str(text);
                                }
                            }
                        }
                        
                        // Token usage extraction
                        if let Some(usage) = json.get("usage")
                            .or(json.get("usageMetadata"))
                            .or(json.get("response").and_then(|r| r.get("usage")))
                        {
                            log.input_tokens = usage.get("prompt_tokens")
                                .or(usage.get("input_tokens"))
                                .or(usage.get("promptTokenCount"))
                                .and_then(|v| v.as_u64())
                                .map(|v| v as u32);
                            log.output_tokens = usage.get("completion_tokens")
                                .or(usage.get("output_tokens"))
                                .or(usage.get("candidatesTokenCount"))
                                .and_then(|v| v.as_u64())
                                .map(|v| v as u32);
                            
                            if log.input_tokens.is_none() && log.output_tokens.is_none() {
                                log.output_tokens = usage.get("total_tokens")
                                    .or(usage.get("totalTokenCount"))
                                    .and_then(|v| v.as_u64())
                                    .map(|v| v as u32);
                            }
                        }
                    }
                }
                
                // Build consolidated response object
                let mut consolidated = serde_json::Map::new();
                
                if !thinking_content.is_empty() {
                    consolidated.insert("thinking".to_string(), Value::String(thinking_content));
                }
                if !thinking_signature.is_empty() {
                    consolidated.insert("thinking_signature".to_string(), Value::String(thinking_signature));
                }
                if !response_content.is_empty() {
                    consolidated.insert("content".to_string(), Value::String(response_content));
                }
                
                if !tool_calls.is_empty() {
                    let clean_tool_calls: Vec<Value> = tool_calls.into_iter().filter(|v| !v.is_null()).collect();
                    if !clean_tool_calls.is_empty() {
                        consolidated.insert("tool_calls".to_string(), Value::Array(clean_tool_calls));
                    }
                }
                if let Some(input) = log.input_tokens {
                    consolidated.insert("input_tokens".to_string(), Value::Number(input.into()));
                }
                if let Some(output) = log.output_tokens {
                    consolidated.insert("output_tokens".to_string(), Value::Number(output.into()));
                }
                
                if consolidated.is_empty() {
                    // Fallback: store raw SSE data if parsing failed
                    log.response_body = Some(full_response.to_string());
                } else {
                    log.response_body = Some(serde_json::to_string_pretty(&Value::Object(consolidated)).unwrap_or_else(|_| full_response.to_string()));
                }
            } else {
                log.response_body = Some(format!("[Binary Stream Data: {} bytes]", all_stream_data.len()));
            }
            
            // Fallback token extraction from tail if not already extracted
            if log.input_tokens.is_none() && log.output_tokens.is_none() {
                if let Ok(full_tail) = std::str::from_utf8(&last_few_bytes) {
                    for line in full_tail.lines().rev() {
                        if line.starts_with("data: ") && (line.contains("\"usage\"") || line.contains("\"usageMetadata\"")) {
                            let json_str = line.trim_start_matches("data: ").trim();
                            if let Ok(json) = serde_json::from_str::<Value>(json_str) {
                                if let Some(usage) = json.get("usage")
                                    .or(json.get("usageMetadata"))
                                    .or(json.get("response").and_then(|r| r.get("usage")))
                                {
                                    log.input_tokens = usage.get("prompt_tokens")
                                        .or(usage.get("input_tokens"))
                                        .or(usage.get("promptTokenCount"))
                                        .and_then(|v| v.as_u64())
                                        .map(|v| v as u32);
                                    log.output_tokens = usage.get("completion_tokens")
                                        .or(usage.get("output_tokens"))
                                        .or(usage.get("candidatesTokenCount"))
                                        .and_then(|v| v.as_u64())
                                        .map(|v| v as u32);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            
            if log.status >= 400 {
                log.error = Some("Stream Error or Failed".to_string());
            }

            // Record User Token Usage
            record_user_token_usage(&user_token_identity, &log, user_agent.clone());

            monitor.log_request(log).await;
        });

        Response::from_parts(parts, Body::from_stream(tokio_stream::wrappers::ReceiverStream::new(rx)))
    } else if content_type.contains("application/json") || content_type.contains("text/") {
        let (parts, body) = response.into_parts();
        match axum::body::to_bytes(body, MAX_RESPONSE_LOG_SIZE).await {
            Ok(bytes) => {
                if let Ok(s) = std::str::from_utf8(&bytes) {
                    if let Ok(json) = serde_json::from_str::<Value>(&s) {
                        // 支持 OpenAI "usage" 或 Gemini "usageMetadata"
                        if let Some(usage) = json.get("usage").or(json.get("usageMetadata")) {
                            log.input_tokens = usage.get("prompt_tokens")
                                .or(usage.get("input_tokens"))
                                .or(usage.get("promptTokenCount"))
                                .and_then(|v| v.as_u64())
                                .map(|v| v as u32);
                            log.output_tokens = usage.get("completion_tokens")
                                .or(usage.get("output_tokens"))
                                .or(usage.get("candidatesTokenCount"))
                                .and_then(|v| v.as_u64())
                                .map(|v| v as u32);
                                
                            if log.input_tokens.is_none() && log.output_tokens.is_none() {
                                log.output_tokens = usage.get("total_tokens")
                                    .or(usage.get("totalTokenCount"))
                                    .and_then(|v| v.as_u64())
                                    .map(|v| v as u32);
                            }
                        }
                    }
                    log.response_body = Some(s.to_string());
                } else {
                    log.response_body = Some("[Binary Response Data]".to_string());
                }
                
                if log.status >= 400 {
                    log.error = log.response_body.clone();
                }

                // Record User Token Usage
                record_user_token_usage(&user_token_identity, &log, user_agent.clone());

                monitor.log_request(log).await;
                Response::from_parts(parts, Body::from(bytes))
            }
            Err(_) => {
                log.response_body = Some("[Response too large (>100MB)]".to_string());

                // Record User Token Usage (even if too large)
                record_user_token_usage(&user_token_identity, &log, user_agent.clone());

                monitor.log_request(log).await;
                Response::from_parts(parts, Body::empty())
            }
        }
    } else {
        log.response_body = Some(format!("[{}]", content_type));

        // Record User Token Usage
        record_user_token_usage(&user_token_identity, &log, user_agent);

        monitor.log_request(log).await;
        response
    }
}
