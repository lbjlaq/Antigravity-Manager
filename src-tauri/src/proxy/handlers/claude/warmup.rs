// Warmup Request Detection and Response Generation
// Intercepts Claude Code warmup requests to save quota

use axum::{
    body::Body,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::proxy::mappers::claude::ClaudeRequest;

/// Detect if this is a Warmup request
/// 
/// Claude Code sends warmup requests every 10 seconds, with characteristics:
/// 1. User message content starts with or contains "Warmup"
/// 2. tool_result content is a "Warmup" error
/// 3. Message loop pattern: assistant sends tool call, user returns Warmup error
pub fn is_warmup_request(request: &ClaudeRequest) -> bool {
    // Only check the LATEST message for Warmup characteristics.
    // Scanning history caused a "poisoned session" bug where one historical Warmup
    // message would cause all subsequent user inputs to be intercepted.
    
    if let Some(msg) = request.messages.last() {
        match &msg.content {
            crate::proxy::mappers::claude::models::MessageContent::String(s) => {
                // Check if simple text starts with Warmup (and is short)
                if s.trim().starts_with("Warmup") && s.len() < 100 {
                    return true;
                }
            },
            crate::proxy::mappers::claude::models::MessageContent::Array(arr) => {
                for block in arr {
                    match block {
                        crate::proxy::mappers::claude::models::ContentBlock::Text { text } => {
                            let trimmed = text.trim();
                            if trimmed == "Warmup" || trimmed.starts_with("Warmup\n") {
                                return true;
                            }
                        },
                        crate::proxy::mappers::claude::models::ContentBlock::ToolResult { 
                            content, is_error, .. 
                        } => {
                            let content_str = if let Some(s) = content.as_str() {
                                s.to_string()
                            } else {
                                content.to_string()
                            };
                            
                            // If it's an error and starts with Warmup, it's a warmup signal
                            if *is_error == Some(true) && content_str.trim().starts_with("Warmup") {
                                return true;
                            }
                        },
                        _ => {}
                    }
                }
            }
        }
    }
    
    false
}

/// Create a mock response for Warmup requests
/// 
/// Returns a simple response without consuming upstream quota
pub fn create_warmup_response(request: &ClaudeRequest, is_stream: bool) -> Response {
    let model = &request.model;
    let message_id = format!("msg_warmup_{}", chrono::Utc::now().timestamp_millis());
    
    if is_stream {
        // Streaming response: send standard SSE event sequence
        let events = vec![
            // message_start
            format!(
                "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"id\":\"{}\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"{}\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{{\"input_tokens\":1,\"output_tokens\":0}}}}}}\n\n",
                message_id, model
            ),
            // content_block_start
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n".to_string(),
            // content_block_delta
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"OK\"}}\n\n".to_string(),
            // content_block_stop
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n".to_string(),
            // message_delta
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":1}}\n\n".to_string(),
            // message_stop
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string(),
        ];
        
        let body = events.join("");
        
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .header(header::CONNECTION, "keep-alive")
            .header("X-Warmup-Intercepted", "true")
            .body(Body::from(body))
            .expect("Failed to build warmup response")
    } else {
        // Non-streaming response
        let response = json!({
            "id": message_id,
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "text",
                "text": "OK"
            }],
            "model": model,
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {
                "input_tokens": 1,
                "output_tokens": 1
            }
        });
        
        (
            StatusCode::OK,
            [("X-Warmup-Intercepted", "true")],
            Json(response)
        ).into_response()
    }
}
