// Claude mapper 模块
// 负责 Claude ↔ Gemini 协议转换

pub mod collector;
pub mod models;
pub mod request;
pub mod response;
pub mod streaming;
pub mod thinking_utils;
pub mod utils;

pub use collector::collect_stream_to_json;
pub use models::*;
pub use request::{set_app_handle, transform_claude_request_in};
pub use response::transform_response;
pub use streaming::{PartProcessor, StreamingState};
pub use thinking_utils::close_tool_loop_for_thinking;

use bytes::Bytes;
use futures::Stream;
use std::pin::Pin;

/// 创建从 Gemini SSE 流到 Claude SSE 流的转换
pub fn create_claude_sse_stream(
    mut gemini_stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    trace_id: String,
    email: String,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>> {
    use async_stream::stream;
    use bytes::BytesMut;
    use futures::StreamExt;

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

                            if let Some(sse_chunks) = process_sse_line(line, &mut state, &trace_id, &email) {
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
fn process_sse_line(
    line: &str,
    state: &mut StreamingState,
    trace_id: &str,
    email: &str,
) -> Option<Vec<Bytes>> {
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

    // 捕获 groundingMetadata (Web Search)
    if let Some(candidate) = raw_json.get("candidates").and_then(|c| c.get(0)) {
        if let Some(grounding) = candidate.get("groundingMetadata") {
            // 提取搜索词
            if let Some(query) = grounding
                .get("webSearchQueries")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
            {
                state.web_search_query = Some(query.to_string());
            }

            // 提取结果块
            if let Some(chunks_arr) = grounding.get("groundingChunks").and_then(|v| v.as_array()) {
                state.grounding_chunks = Some(chunks_arr.clone());
            } else if let Some(chunks_arr) = grounding
                .get("grounding_metadata")
                .and_then(|m| m.get("groundingChunks"))
                .and_then(|v| v.as_array())
            {
                state.grounding_chunks = Some(chunks_arr.clone());
            }
        }
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

    // Process grounding metadata (googleSearch results) and append as citations
    // [DISABLED] Temporarily disabled to fix Cherry Studio compatibility
    // Cherry Studio doesn't recognize "web_search_tool_result" type, causing validation errors
    // Search results are still displayed via Markdown text block in streaming.rs (lines 341-381)
    // TODO: Research Antigravity2Api implementation for correct type mapping
    /*
    if let Some(grounding) = raw_json
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|cand| cand.get("groundingMetadata"))
    {
        if let Some(citation_chunks) = process_grounding_metadata(grounding, state) {
            chunks.extend(citation_chunks);
        }
    }
    */

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

        if let Some(ref u) = usage {
            let cached_tokens = u.cached_content_token_count.unwrap_or(0);
            let cache_info = if cached_tokens > 0 {
                format!(", Cached: {}", cached_tokens)
            } else {
                String::new()
            };

            tracing::info!(
                "[{}] ✓ Stream completed | Account: {} | In: {} tokens | Out: {} tokens{}",
                trace_id,
                email,
                u.prompt_token_count
                    .unwrap_or(0)
                    .saturating_sub(cached_tokens),
                u.candidates_token_count.unwrap_or(0),
                cache_info
            );
        }

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
            chunks.push(Bytes::from(
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ));
            state.message_stop_sent = true;
        }
        return chunks;
    }
    vec![]
}

/// Process grounding metadata from Gemini's googleSearch and emit as Claude web_search blocks
///
/// Epic-007: Grounding metadata processing - deferred to future sprint
/// TODO: Integrate when grounding support is fully implemented (Epic-007 continuation)
#[allow(dead_code)]
fn process_grounding_metadata(
    metadata: &serde_json::Value,
    state: &mut StreamingState,
) -> Option<Vec<Bytes>> {
    use serde_json::json;

    // Extract search queries and grounding chunks
    let search_queries = metadata
        .get("webSearchQueries")
        .and_then(|q| q.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();

    let grounding_chunks = metadata.get("groundingChunks").and_then(|c| c.as_array())?;

    if grounding_chunks.is_empty() {
        return None;
    }

    // Generate a unique tool_use_id
    let tool_use_id = format!(
        "srvtoolu_{}",
        crate::proxy::common::utils::generate_random_id()
    );

    // Build search results array
    let mut search_results = Vec::new();
    for chunk in grounding_chunks.iter() {
        if let Some(web) = chunk.get("web") {
            let title = web
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("Source");
            let uri = web.get("uri").and_then(|u| u.as_str()).unwrap_or("");
            if !uri.is_empty() {
                search_results.push(json!({
                    "url": uri,
                    "title": title,
                    "encrypted_content": "", // Gemini doesn't provide this
                    "page_age": null
                }));
            }
        }
    }

    if search_results.is_empty() {
        return None;
    }

    let search_query = search_queries
        .first()
        .map(|s| s.to_string())
        .unwrap_or_default();

    tracing::debug!(
        "[Grounding] Emitting {} search results for query: {}",
        search_results.len(),
        search_query
    );

    let mut chunks = Vec::new();

    // 1. Emit server_tool_use block (start)
    let server_tool_use_start = json!({
        "type": "content_block_start",
        "index": state.block_index,
        "content_block": {
            "type": "server_tool_use",
            "id": tool_use_id,
            "name": "web_search",
            "input": {
                "query": search_query
            }
        }
    });
    chunks.push(Bytes::from(format!(
        "event: content_block_start\ndata: {}\n\n",
        server_tool_use_start
    )));

    // server_tool_use block stop
    let server_tool_use_stop = json!({
        "type": "content_block_stop",
        "index": state.block_index
    });
    chunks.push(Bytes::from(format!(
        "event: content_block_stop\ndata: {}\n\n",
        server_tool_use_stop
    )));
    state.block_index += 1;

    // 2. Emit web_search_tool_result block (start)
    let tool_result_start = json!({
        "type": "content_block_start",
        "index": state.block_index,
        "content_block": {
            "type": "web_search_tool_result",
            "tool_use_id": tool_use_id,
            "content": search_results
        }
    });
    chunks.push(Bytes::from(format!(
        "event: content_block_start\ndata: {}\n\n",
        tool_result_start
    )));

    // web_search_tool_result block stop
    let tool_result_stop = json!({
        "type": "content_block_stop",
        "index": state.block_index
    });
    chunks.push(Bytes::from(format!(
        "event: content_block_stop\ndata: {}\n\n",
        tool_result_stop
    )));
    state.block_index += 1;

    Some(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_sse_line_done() {
        let mut state = StreamingState::new();
        let result = process_sse_line("data: [DONE]", &mut state, "test_id", "test@example.com");
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

        let result = process_sse_line(test_data, &mut state, "test_id", "test@example.com");
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
