// Claude mapper 模块
// 负责 Claude ↔ Gemini 协议转换

pub mod models;
pub mod request;
pub mod response;
pub mod streaming;
pub mod utils;

pub use models::*;
pub use request::transform_claude_request_in;
pub use response::transform_response;
pub use streaming::{StreamingState, PartProcessor};

use bytes::Bytes;
use futures::Stream;
use std::pin::Pin;

/// 创建从 Gemini SSE 流到 Claude SSE 流的转换
pub fn create_claude_sse_stream(
    mut gemini_stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>> {
    use async_stream::stream;
    use futures::StreamExt;
    use bytes::BytesMut;

    Box::pin(stream! {
        let mut state = StreamingState::new();
        let mut buffer = BytesMut::new();

        while let Some(chunk_result) = gemini_stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    buffer.extend_from_slice(&chunk);
                    
                    // Process complete lines
                    while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                        let line_raw = buffer.split_to(pos + 1);
                        if let Ok(line_str) = std::str::from_utf8(&line_raw) {
                            let line = line_str.trim();
                            if line.is_empty() { continue; }

                            if let Some(sse_chunks) = process_sse_line(line, &mut state) {
                                for sse_chunk in sse_chunks {
                                    yield Ok(sse_chunk);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    yield Err(format!("Stream error: {}", e));
                    break;
                }
            }
        }
        
        // Ensure termination events are sent
        for chunk in emit_force_stop(&mut state) {
            yield Ok(chunk);
        }
    })
}

/// 处理单行 SSE 数据
fn process_sse_line(line: &str, state: &mut StreamingState) -> Option<Vec<Bytes>> {
    if !line.starts_with("data: ") {
        return None;
    }

    let data_str = line[6..].trim();
    if data_str.is_empty() {
        return None;
    }

    if data_str == "[DONE]" {
        let chunks = emit_force_stop(state);
        if chunks.is_empty() {
            return None;
        }
        return Some(chunks);
    }

    // 解析 JSON
    let json_value: serde_json::Value = match serde_json::from_str(data_str) {
        Ok(v) => v,
        Err(_) => return None,
    };

    let mut chunks = Vec::new();

    // 解包 response 字段 (如果存在)
    let raw_json = json_value.get("response").unwrap_or(&json_value);

    // 发送 message_start
    if !state.message_start_sent {
        chunks.push(state.emit_message_start(raw_json));
    }

    // 处理所有 parts
    if let Some(parts) = raw_json
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|cand| cand.get("content"))
        .and_then(|content| content.get("parts"))
        .and_then(|p| p.as_array())
    {
        for part_value in parts {
            if let Ok(part) = serde_json::from_value::<GeminiPart>(part_value.clone()) {
                let mut processor = PartProcessor::new(state);
                chunks.extend(processor.process(&part));
            }
        }
    }

    // 检查是否结束
    if let Some(finish_reason) = raw_json
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|cand| cand.get("finishReason"))
        .and_then(|f| f.as_str())
    {
        let usage = raw_json
            .get("usageMetadata")
            .and_then(|u| serde_json::from_value::<UsageMetadata>(u.clone()).ok());

        chunks.extend(state.emit_finish(Some(finish_reason), usage.as_ref()));
    }

    if chunks.is_empty() {
        None
    } else {
        Some(chunks)
    }
}

/// 发送强制结束事件
pub fn emit_force_stop(state: &mut StreamingState) -> Vec<Bytes> {
    if !state.message_stop_sent {
        let mut chunks = state.emit_finish(None, None);
        if chunks.is_empty() {
            chunks.push(Bytes::from("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"));
            state.message_stop_sent = true;
        }
        return chunks;
    }
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_sse_line_done() {
        let mut state = StreamingState::new();
        let result = process_sse_line("data: [DONE]", &mut state);
        
        assert!(result.is_some());
        let chunks = result.unwrap();
        assert!(!chunks.is_empty());

        let all_text: String = chunks
            .iter()
            .map(|b| String::from_utf8(b.to_vec()).unwrap_or_default())
            .collect();
        assert!(all_text.contains("message_stop"));
    }

    #[test]
    fn test_process_sse_line_with_text() {
        let mut state = StreamingState::new();
        
        let test_data = r#"data: {"candidates":[{"content":{"parts":[{"text":"Hello"}]}}],"usageMetadata":{},"modelVersion":"test","responseId":"123"}"#;
        
        let result = process_sse_line(test_data, &mut state);
        assert!(result.is_some());
        
        let chunks = result.unwrap();
        assert!(!chunks.is_empty());
        
        // 应该包含 message_start 和 text delta
        let all_text: String = chunks
            .iter()
            .map(|b| String::from_utf8(b.to_vec()).unwrap_or_default())
            .collect();
        
        assert!(all_text.contains("message_start"));
        assert!(all_text.contains("content_block_start"));
        assert!(all_text.contains("Hello"));
    }
}
