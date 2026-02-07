// Stream 收集器 - 将 SSE 流转换为完整的 JSON 响应
// 用于非 Stream 请求的自动转换

use super::models::*;
use bytes::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};
use std::io;

/// SSE 事件类型
#[derive(Debug, Clone)]
struct SseEvent {
    event_type: String,
    data: Value,
}

/// 解析 SSE 行
fn parse_sse_line(line: &str) -> Option<(String, String)> {
    if let Some(colon_pos) = line.find(':') {
        let key = &line[..colon_pos];
        let value = line[colon_pos + 1..].trim_start();
        Some((key.to_string(), value.to_string()))
    } else {
        None
    }
}

/// 将 SSE Stream 收集为完整的 Claude Response
///
/// 此函数接收一个 SSE 字节流，解析所有事件，并重建完整的 ClaudeResponse 对象。
/// 这使得非 Stream 客户端可以透明地享受 Stream 模式的配额优势。
pub async fn collect_stream_to_json<S>(
    mut stream: S,
) -> Result<ClaudeResponse, String>
where
    S: futures::Stream<Item = Result<Bytes, io::Error>> + Unpin,
{
    let mut events = Vec::new();
    let mut current_event_type = String::new();
    let mut current_data = String::new();

    // 1. 收集所有 SSE 事件
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("Stream error: {}", e))?;
        let text = String::from_utf8_lossy(&chunk);

        for line in text.lines() {
            if line.is_empty() {
                // 空行表示事件结束
                if !current_data.is_empty() {
                    if let Ok(data) = serde_json::from_str::<Value>(&current_data) {
                        events.push(SseEvent {
                            event_type: current_event_type.clone(),
                            data,
                        });
                    }
                    current_event_type.clear();
                    current_data.clear();
                }
            } else if let Some((key, value)) = parse_sse_line(line) {
                match key.as_str() {
                    "event" => current_event_type = value,
                    "data" => current_data = value,
                    _ => {}
                }
            }
        }
    }

    // 2. 重建 ClaudeResponse
    let mut response = ClaudeResponse {
        id: "msg_unknown".to_string(),
        type_: "message".to_string(),
        role: "assistant".to_string(),
        model: String::new(),
        content: Vec::new(),
        stop_reason: "end_turn".to_string(),
        stop_sequence: None,
        usage: Usage {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_input_tokens: None,
            cache_creation_input_tokens: None,
            server_tool_use: None,
        },
    };

    // 用于累积内容块
    let mut current_text = String::new();
    let mut current_thinking = String::new();
    let mut current_signature: Option<String> = None;
    let mut current_tool_use: Option<Value> = None;
    let mut current_tool_input = String::new();

    for event in events {
        match event.event_type.as_str() {
            "message_start" => {
                // 提取基本信息
                if let Some(message) = event.data.get("message") {
                    if let Some(id) = message.get("id").and_then(|v| v.as_str()) {
                        response.id = id.to_string();
                    }
                    if let Some(model) = message.get("model").and_then(|v| v.as_str()) {
                        response.model = model.to_string();
                    }
                    if let Some(usage) = message.get("usage") {
                        if let Ok(u) = serde_json::from_value::<Usage>(usage.clone()) {
                            response.usage = u;
                        }
                    }
                }
            }

            "content_block_start" => {
                if let Some(content_block) = event.data.get("content_block") {
                    if let Some(block_type) = content_block.get("type").and_then(|v| v.as_str()) {
                        match block_type {
                            "text" => current_text.clear(),
                            "thinking" => {
                                current_thinking.clear();
                                // Extract signature from content_block
                                current_signature = content_block.get("signature")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());
                            }
                            "tool_use" => {
                                current_tool_use = Some(content_block.clone());
                                current_tool_input.clear();
                            }
                            _ => {}
                        }
                    }
                }
            }

            "content_block_delta" => {
                if let Some(delta) = event.data.get("delta") {
                    if let Some(delta_type) = delta.get("type").and_then(|v| v.as_str()) {
                        match delta_type {
                            "text_delta" => {
                                if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                    current_text.push_str(text);
                                }
                            }
                            "thinking_delta" => {
                                if let Some(thinking) = delta.get("thinking").and_then(|v| v.as_str()) {
                                    current_thinking.push_str(thinking);
                                }
                                // In case signature comes in delta (less likely but possible update)
                                if let Some(sig) = delta.get("signature").and_then(|v| v.as_str()) {
                                    current_signature = Some(sig.to_string());
                                }
                            }
                            "input_json_delta" => {
                                if let Some(partial_json) = delta.get("partial_json").and_then(|v| v.as_str()) {
                                    current_tool_input.push_str(partial_json);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            "content_block_stop" => {
                // 完成当前块
                if !current_text.is_empty() {
                    response.content.push(ContentBlock::Text {
                        text: current_text.clone(),
                    });
                    current_text.clear();
                } else if !current_thinking.is_empty() {
                    response.content.push(ContentBlock::Thinking {
                        thinking: current_thinking.clone(),
                        signature: current_signature.take(),
                        cache_control: None,
                    });
                    current_thinking.clear();
                } else if let Some(tool_use) = current_tool_use.take() {
                    // 构建 tool_use 块
                    let id = tool_use.get("id").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                    let name = tool_use.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                    let input = if !current_tool_input.is_empty() {
                        serde_json::from_str(&current_tool_input).unwrap_or(json!({}))
                    } else {
                        json!({})
                    };

                    response.content.push(ContentBlock::ToolUse {
                        id,
                        name,
                        input,
                        signature: None,
                        cache_control: None,
                    });
                    current_tool_input.clear();
                }
            }

            "message_delta" => {
                if let Some(delta) = event.data.get("delta") {
                    if let Some(stop_reason) = delta.get("stop_reason").and_then(|v| v.as_str()) {
                        response.stop_reason = stop_reason.to_string();
                    }
                }
                if let Some(usage) = event.data.get("usage") {
                    if let Ok(u) = serde_json::from_value::<Usage>(usage.clone()) {
                        response.usage = u;
                    }
                }
            }

            "message_stop" => {
                // Stream 结束
                break;
            }

            "error" => {
                // 错误事件
                return Err(format!("Stream error: {:?}", event.data));
            }

            _ => {
                // 忽略未知事件类型
            }
        }
    }

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    #[tokio::test]
    async fn test_collect_simple_text_response() {
        // 模拟一个简单的文本响应 SSE 流
        let sse_data = vec![
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_123\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-3-5-sonnet\",\"content\":[],\"stop_reason\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" World\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ];

        let byte_stream = stream::iter(
            sse_data.into_iter().map(|s| Ok::<Bytes, io::Error>(Bytes::from(s)))
        );

        let result = collect_stream_to_json(byte_stream).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.id, "msg_123");
        assert_eq!(response.model, "claude-3-5-sonnet");
        assert_eq!(response.content.len(), 1);
        
        if let ContentBlock::Text { text } = &response.content[0] {
            assert_eq!(text, "Hello World");
        } else {
            panic!("Expected Text block");
        }
    }

    #[tokio::test]
    async fn test_collect_thinking_response_with_signature() {
        // 模拟一个包含 Thinking Block 和签名的 SSE 流
        let sse_data = vec![
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_think\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-3-7-sonnet\",\"content\":[],\"stop_reason\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n",
            // content_block_start 中包含 signature
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\", \"signature\": \"sig_123456\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"I am \"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"thinking\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":10}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ];

        let byte_stream = stream::iter(
            sse_data.into_iter().map(|s| Ok::<Bytes, io::Error>(Bytes::from(s)))
        );

        let result = collect_stream_to_json(byte_stream).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        
        if let ContentBlock::Thinking { thinking, signature, .. } = &response.content[0] {
            assert_eq!(thinking, "I am thinking");
            // 验证签名是否被正确提取
            assert_eq!(signature.as_deref(), Some("sig_123456"));
        } else {
            panic!("Expected Thinking block");
        }
    }
}
