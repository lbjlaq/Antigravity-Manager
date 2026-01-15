// OpenAI Stream 收集器 - 将 SSE 流转换为完整的 JSON 响应
// 用于非 Stream 请求的自动转换

use super::models::*;
use bytes::Bytes;
use futures::StreamExt;
use serde_json::Value;
use std::io;

/// SSE 事件类型
#[derive(Debug, Clone)]
struct SseEvent {
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

/// 将 OpenAI SSE Stream 收集为完整的 OpenAIResponse
pub async fn collect_openai_stream_to_json<S>(
    mut stream: S,
) -> Result<OpenAIResponse, String>
where
    S: futures::Stream<Item = Result<Bytes, io::Error>> + Unpin,
{
    let mut chunks = Vec::new();
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
                        chunks.push(SseEvent { data });
                    }
                    current_data.clear();
                }
            } else if let Some((key, value)) = parse_sse_line(line) {
                if key == "data" {
                    if value == "[DONE]" {
                        break;
                    }
                    current_data = value;
                }
            }
        }
    }

    // 2. 重建 OpenAIResponse
    let mut response = OpenAIResponse {
        id: "chatcmpl-unknown".to_string(),
        object: "chat.completion".to_string(),
        created: chrono::Utc::now().timestamp() as u64,
        model: String::new(),
        choices: vec![],
    };

    let mut content = String::new();
    let mut reasoning_content = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut finish_reason: Option<String> = None;

    for event in chunks {
        // 提取基本信息
        if let Some(id) = event.data.get("id").and_then(|v| v.as_str()) {
            response.id = id.to_string();
        }
        if let Some(model) = event.data.get("model").and_then(|v| v.as_str()) {
            response.model = model.to_string();
        }
        if let Some(created) = event.data.get("created").and_then(|v| v.as_u64()) {
            response.created = created;
        }

        // 处理 choices
        if let Some(choices_arr) = event.data.get("choices").and_then(|v| v.as_array()) {
            for choice in choices_arr {
                if let Some(delta) = choice.get("delta") {
                    // 累积 content
                    if let Some(text) = delta.get("content").and_then(|v| v.as_str()) {
                        content.push_str(text);
                    }

                    // 累积 reasoning_content (思考过程)
                    if let Some(reasoning) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
                        reasoning_content.push_str(reasoning);
                    }

                    // 累积 tool_calls
                    if let Some(tc_arr) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                        for tc in tc_arr {
                            let index = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                            
                            // 确保 tool_calls 有足够的空间
                            while tool_calls.len() <= index {
                                tool_calls.push(ToolCall {
                                    id: String::new(),
                                    r#type: "function".to_string(),
                                    function: ToolFunction {
                                        name: String::new(),
                                        arguments: String::new(),
                                    },
                                });
                            }

                            if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                                tool_calls[index].id = id.to_string();
                            }
                            if let Some(func) = tc.get("function") {
                                if let Some(name) = func.get("name").and_then(|v| v.as_str()) {
                                    tool_calls[index].function.name = name.to_string();
                                }
                                if let Some(args) = func.get("arguments").and_then(|v| v.as_str()) {
                                    tool_calls[index].function.arguments.push_str(args);
                                }
                            }
                        }
                    }
                }

                // 获取 finish_reason
                if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                    finish_reason = Some(reason.to_string());
                }
            }
        }

        // OpenAIResponse 没有 usage 字段，跳过
    }

    // 3. 构建最终的 choice
    let message = if !tool_calls.is_empty() {
        OpenAIMessage {
            role: "assistant".to_string(),
            content: if content.is_empty() { None } else { Some(OpenAIContent::String(content)) },
            tool_calls: Some(tool_calls),
            reasoning_content: if reasoning_content.is_empty() { None } else { Some(reasoning_content) },
            tool_call_id: None,
            name: None,
        }
    } else {
        OpenAIMessage {
            role: "assistant".to_string(),
            content: Some(OpenAIContent::String(content)),
            tool_calls: None,
            reasoning_content: if reasoning_content.is_empty() { None } else { Some(reasoning_content) },
            tool_call_id: None,
            name: None,
        }
    };

    response.choices.push(Choice {
        index: 0,
        message,
        finish_reason,
    });

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    #[tokio::test]
    async fn test_collect_simple_chat_response() {
        let sse_data = vec![
            "data: {\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1234567890,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1234567890,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" World\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1234567890,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        ];

        let byte_stream = stream::iter(
            sse_data.into_iter().map(|s| Ok::<Bytes, io::Error>(Bytes::from(s)))
        );

        let result = collect_openai_stream_to_json(byte_stream).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.id, "chatcmpl-123");
        assert_eq!(response.model, "gpt-4");
        assert_eq!(response.choices.len(), 1);
        
        if let Some(OpenAIContent::String(text)) = &response.choices[0].message.content {
            assert_eq!(text, "Hello World");
        } else {
            panic!("Expected String content");
        }
    }
}
