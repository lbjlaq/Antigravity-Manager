// Claude mapper 模块
// 负责 Claude ↔ Gemini 协议转换

pub mod collector;
pub mod models;
pub mod request;
pub mod response;
pub mod streaming;
pub mod thinking_utils;
pub mod utils;

use crate::proxy::common::client_adapter::ClientAdapter;
pub use collector::collect_stream_to_json;
pub use models::*;
pub use request::{
    clean_cache_control_from_messages, merge_consecutive_messages, transform_claude_request_in,
    transform_claude_request_in_timed,
};
pub use response::transform_response;
pub use streaming::{BlockType, PartProcessor, StreamingState};
pub use thinking_utils::filter_invalid_thinking_blocks_with_family; // [NEW]

use crate::proxy::mappers::error_classifier::StreamErrorReport;
use crate::proxy::pipeline::{run_stream_lifecycle, ProtocolStreamHandler, StreamLifecycleConfig};
use bytes::Bytes;
use futures::Stream;
use std::pin::Pin;

/// Claude SSE 流协议渲染适配器（实现模板方法模式 ProtocolStreamHandler）
pub struct ClaudeStreamHandler {
    pub state: StreamingState,
    pub trace_id: String,
    pub email: String,
}

impl ProtocolStreamHandler for ClaudeStreamHandler {
    fn process_line(&mut self, line: &str) -> Vec<Bytes> {
        process_sse_line(line, &mut self.state, &self.trace_id, &self.email).unwrap_or_default()
    }

    fn is_terminal(&self) -> bool {
        self.state.message_stop_sent
    }

    fn has_content(&self) -> bool {
        self.state.has_content
    }

    fn has_thinking(&self) -> bool {
        self.state.has_thinking
    }

    fn is_thinking_active(&self) -> bool {
        self.state.current_block_type() == BlockType::Thinking
    }

    fn emit_finalize(&mut self) -> Vec<Bytes> {
        let mut out = Vec::new();

        // 协议守卫：若 message_stop 已发送，严禁追加任何事件帧，防止破坏客户端状态机
        if self.state.message_stop_sent {
            return out;
        }

        // [协议收敛] 思考后中断防护：若产生了思考但未产生任何正文，绝不可伪造文本或 message_stop！
        // 伪造收尾会导致 Claude Code 等客户端状态机误判为"生成正常终结"，从而抑制原生重试循环并使会话中毒。
        // 此处严格静默，交由 stream_lifecycle 识别思考未完成状态并如实透传 Err，促使客户端触发原生重试。
        if self.state.has_thinking && !self.state.has_content {
            tracing::warn!(
                "[{}] Stream finalized after thinking without content. Suppressing fake completion to allow client retry.",
                self.trace_id
            );
            return out;
        } else if !self.state.has_content && !self.state.has_thinking {
            // [FIX #3359] 空回复恢复兜底（例如健康探测单点提示词）
            if !self.state.message_start_sent {
                let dummy_start = serde_json::json!({
                    "responseId": format!("msg_recovered_{}", chrono::Utc::now().timestamp_millis()),
                    "modelVersion": "gemini-auto",
                });
                out.push(self.state.emit_message_start(&dummy_start));
            }

            out.extend(self.state.start_block(
                crate::proxy::mappers::claude::streaming::BlockType::Text,
                serde_json::json!({ "type": "text", "text": "." }),
            ));
            out.extend(self.state.end_block());
            self.state.has_content = true;

            let recovery_usage = crate::proxy::mappers::claude::models::Usage {
                input_tokens: 1,
                output_tokens: 1,
                cache_read_input_tokens: None,
                cache_creation_input_tokens: None,
                server_tool_use: None,
            };
            let delta = serde_json::json!({
                "type": "message_delta",
                "delta": { "stop_reason": "end_turn", "stop_sequence": null },
                "usage": recovery_usage,
            });
            out.push(self.state.emit("message_delta", delta));
            out.push(Bytes::from(
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ));
            self.state.message_stop_sent = true;
        }

        // 正常收尾终结事件
        out.extend(emit_force_stop(&mut self.state));
        out
    }

    fn emit_initial_error(&mut self, error_report: &StreamErrorReport) -> Vec<Bytes> {
        // [FIX #3634 契约对齐] 严禁下发 overloaded_error。
        // Claude Code 2.1.293 的 PU(e) 只要检测到 overloaded_error 字符串，立即触发 30 分钟冷却并在未产生内容前进入 10 次指数退避重试风暴。
        // 首包未就绪时发生错误统一使用 Anthropic 标准的 api_error，促使客户端转入 retryWithoutStreaming 单次重试或直接暴露错误。
        let err_type = "api_error";
        let error_json = serde_json::json!({
            "type": "error",
            "error": {
                "type": err_type,
                "message": error_report.client_message(),
                "function": error_report.function,
                "call_site": error_report.call_site(),
                "params": error_report.params,
            }
        });
        vec![self.state.emit("error", error_json)]
    }

    fn on_finish(&mut self) {
        if let Some(sid) = self.state.session_id.clone() {
            let acc = std::mem::take(&mut self.state.thinking_acc);
            acc.commit(&sid);
        }
    }
}

/// 创建从 Gemini SSE 流到 Claude SSE 流的转换（委托至通用 run_stream_lifecycle 引擎）
#[allow(clippy::too_many_arguments)]
pub fn create_claude_sse_stream<S, E>(
    gemini_stream: Pin<Box<S>>,
    trace_id: String,
    email: String,
    session_id: Option<String>, // [NEW v3.3.17] Session ID for signature caching
    scaling_enabled: bool,      // [NEW] Flag for context usage scaling
    context_limit: u32,
    estimated_prompt_tokens: Option<u32>, // [FIX] Estimated tokens for calibrator learning
    message_count: usize,                 // [NEW v4.0.0] Message count for rewind detection
    client_adapter: Option<std::sync::Arc<dyn ClientAdapter>>, // [NEW] Adapter reference
    registered_tool_names: Vec<String>,   // [FIX #MCP] Tool names for fuzzy matching
) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + ?Sized + 'static,
    E: std::fmt::Display + Send + 'static,
{
    let mut state = StreamingState::new();
    state.session_id = session_id;
    state.message_count = message_count;
    state.scaling_enabled = scaling_enabled;
    state.context_limit = context_limit;
    state.estimated_prompt_tokens = estimated_prompt_tokens;
    state.set_client_adapter(client_adapter);
    state.set_registered_tool_names(registered_tool_names);

    let session_str = state.session_id.clone().unwrap_or_else(|| "-".to_string());
    let trace_info = format!(
        "trace={} session={} messages={}",
        trace_id, session_str, message_count
    );

    let handler = ClaudeStreamHandler {
        state,
        trace_id,
        email,
    };

    let config = StreamLifecycleConfig::new("claude", "create_claude_sse_stream", trace_info);

    run_stream_lifecycle(gemini_stream, handler, config)
}

/// 处理单行 SSE 数据
fn process_sse_line(
    line: &str,
    state: &mut StreamingState,
    trace_id: &str,
    email: &str,
) -> Option<Vec<Bytes>> {
    // [FIX #3634 静默防火墙契约] 一旦 message_stop 已下发，严禁继续向客户端下发任何数据帧。
    // Claude Code 2.1.293 收到 message_stop 后的任何数据帧均会触发 StreamMalformedEventError("closed") (TJ("closed"))。
    if state.message_stop_sent {
        return None;
    }

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

    // 捕获 usageMetadata（无论是否携带 finishReason，均持续缓存在 state.last_usage 中）
    if let Some(usage_val) = raw_json.get("usageMetadata") {
        if let Ok(parsed_usage) = serde_json::from_value::<UsageMetadata>(usage_val.clone()) {
            state.last_usage = Some(parsed_usage);
        }
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
            state.thinking_acc.ingest_part(part_value);
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

    // 检查是否结束（兼容候选集与根级的 finishReason / finish_reason 命名）
    let finish_reason = raw_json
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|cand| {
            cand.get("finishReason")
                .or_else(|| cand.get("finish_reason"))
        })
        .and_then(|f| f.as_str())
        .or_else(|| {
            raw_json
                .get("finishReason")
                .or_else(|| raw_json.get("finish_reason"))
                .and_then(|f| f.as_str())
        });

    if let Some(finish_reason) = finish_reason {
        if state.has_thinking && !state.has_content {
            tracing::warn!(
                "[{}] Upstream emitted finishReason '{}' after thinking without content. Suppressing message_stop to allow native client retry.",
                trace_id,
                finish_reason
            );
        } else {
            let usage = raw_json
                .get("usageMetadata")
                .and_then(|u| serde_json::from_value::<UsageMetadata>(u.clone()).ok())
                .or_else(|| state.last_usage.clone());

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
        let usage = state.last_usage.clone();
        let mut chunks = state.emit_finish(None, usage.as_ref());
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
#[allow(dead_code)] // Temporarily disabled for Cherry Studio compatibility, kept for future use
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

    #[tokio::test]
    async fn test_thinking_only_interruption_propagates_error_without_fake_completion() {
        use futures::StreamExt;

        // 1. 模拟一个只发送 Thinking 然后就结束的流 (无任何正文)
        let mock_stream = async_stream::stream! {
            let thinking_json = serde_json::json!({
                "candidates": [{
                    "content": {
                        "parts": [{ "text": "Thinking...", "thought": true }]
                    }
                }],
                "modelVersion": "gemini-2.0-flash-thinking",
                "responseId": "msg_interrupted"
            });
            yield Ok::<_, String>(bytes::Bytes::from(format!("data: {}\n\n", thinking_json)));
            // 然后突然结束 (没有 Text, 没有 Usage, 直接 None)
        };

        // 2. 创建转换后的流
        let mut claude_stream = create_claude_sse_stream(
            Box::pin(mock_stream),
            "trace_test".to_string(),
            "test@example.com".to_string(),
            None,
            false,
            1_000,
            None,
            1,          // message_count
            None,       // client_adapter
            Vec::new(), // registered_tool_names
        );

        // 3. 收集输出
        let mut all_chunks = Vec::new();
        let mut error_propagated = false;
        while let Some(result) = claude_stream.next().await {
            match result {
                Ok(bytes) => all_chunks.push(String::from_utf8(bytes.to_vec()).unwrap()),
                Err(e) => {
                    error_propagated = true;
                    assert!(e.contains("thinking"));
                }
            }
        }
        let output = all_chunks.join("");

        // 4. 验证思考内容已正常输出
        assert!(output.contains("Thinking..."));

        // 5. [核心契约] 严禁注入合成提示文本欺骗客户端
        assert!(
            !output.contains("Recovered by Antigravity"),
            "Must NOT inject synthetic recovery text on thinking-only cutoff"
        );

        // 6. [核心契约] 严禁发送 message_stop 或 end_turn 伪造正常终结，确保触发客户端原生重试
        assert!(
            !output.contains("event: message_stop"),
            "Must NOT emit message_stop when only thinking was emitted"
        );
        assert!(
            !output.contains(r#""stop_reason":"end_turn""#),
            "Must NOT emit end_turn when only thinking was emitted"
        );

        // 7. 必须向外层透传底层错误
        assert!(
            error_propagated,
            "Stream must propagate Err on thinking-only cutoff to trigger client native retry"
        );
    }

    #[tokio::test]
    async fn test_create_claude_sse_stream_mid_stream_interruption_propagates_error_and_does_not_fake_end_turn(
    ) {
        use futures::StreamExt;

        // 模拟一个发送了部分内容后发生网络中断的流
        let mock_stream = async_stream::stream! {
            yield Ok::<_, String>(bytes::Bytes::from("data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hello world\"}]}}]}\n\n"));
            yield Err("error reading a body from connection: connection reset by peer".to_string());
        };

        let mut claude_stream = create_claude_sse_stream(
            Box::pin(mock_stream),
            "trace_mid_err_test".to_string(),
            "test@example.com".to_string(),
            None,
            false,
            1_000,
            None,
            1,
            None,
            Vec::new(),
        );

        let mut all_chunks = Vec::new();
        let mut error_propagated = false;
        while let Some(result) = claude_stream.next().await {
            match result {
                Ok(bytes) => all_chunks.push(String::from_utf8(bytes.to_vec()).unwrap()),
                Err(e) => {
                    error_propagated = true;
                    assert!(e.contains("connection reset by peer"));
                }
            }
        }
        let output = all_chunks.join("");

        // 1. 必须包含已输出的正文
        assert!(
            output.contains("Hello world"),
            "Output must contain text emitted before error"
        );

        // 2. 严禁注入假截断系统提示
        assert!(
            !output.contains("Response truncated by Antigravity"),
            "Must NOT inject synthetic truncation notice into user content"
        );

        // 3. 严禁伪造正常收尾 message_delta(end_turn) 或 message_stop 欺骗客户端，确保客户端识别到断流并自动触发重试
        assert!(
            !output.contains(r#""stop_reason":"end_turn""#),
            "Must NOT fake stop_reason end_turn on mid-stream drop"
        );
        assert!(
            !output.contains("event: message_stop"),
            "Must NOT emit message_stop on mid-stream drop"
        );

        // 4. 严禁在已输出内容后发送 event: error 或 overloaded_error，必须向外层传播真实 Err
        assert!(
            !output.contains("event: error"),
            "Must NOT emit event: error after content was already emitted"
        );
        assert!(
            !output.contains(r#""type":"overloaded_error""#),
            "Must NOT emit overloaded_error which triggers infinite client retries"
        );
        assert!(
            error_propagated,
            "Mid-stream network error must propagate as Err to trigger client native retry"
        );
    }

    #[tokio::test]
    async fn test_create_claude_sse_stream_initial_error_emits_standard_api_error_without_trailing_events(
    ) {
        use futures::StreamExt;

        // 模拟一个在发出任何内容前就发生连接错误的流
        let mock_stream = async_stream::stream! {
            yield Err::<bytes::Bytes, _>("error connecting to upstream: connection refused".to_string());
        };

        let mut claude_stream = create_claude_sse_stream(
            Box::pin(mock_stream),
            "trace_init_err_test".to_string(),
            "test@example.com".to_string(),
            None,
            false,
            1_000,
            None,
            1,
            None,
            Vec::new(),
        );

        let mut all_chunks = Vec::new();
        while let Some(result) = claude_stream.next().await {
            if let Ok(bytes) = result {
                all_chunks.push(String::from_utf8(bytes.to_vec()).unwrap());
            }
        }
        let output = all_chunks.join("");

        // 1. 必须包含 event: error
        assert!(
            output.contains("event: error"),
            "Output must contain event: error"
        );

        // 2. 错误类型必须是 Anthropic 标准的 api_error，绝不能是引发恶性重试的 overloaded_error
        assert!(
            output.contains(r#""type":"api_error""#),
            "Error event must contain standard Anthropic type 'api_error', got: {}",
            output
        );
        assert!(
            !output.contains(r#""type":"overloaded_error""#),
            "Error event must not use overloaded_error"
        );
        assert!(
            !output.contains(r#""type":"stream_error""#),
            "Error event must not leak internal non-standard 'stream_error'"
        );

        // 3. [FIX #3621 关键断言] 发生 error 后必须立即终止，严禁追加发送 message_stop 破坏协议状态机！
        assert!(
            !output.contains("event: message_stop"),
            "Must NOT emit message_stop after event: error"
        );

        // 4. 同时必须保留诊断信息
        assert!(output.contains("fn=create_claude_sse_stream"));
    }

    #[tokio::test]
    async fn test_thinking_only_with_finish_reason_stop_propagates_error_without_fake_message_stop()
    {
        use futures::StreamExt;

        // 模拟一个发送了 Thinking 后，上游直接以 finishReason: "STOP" 结束但未产生任何正文的异常流
        let mock_stream = async_stream::stream! {
            let chunk_json = serde_json::json!({
                "candidates": [{
                    "content": {
                        "parts": [{ "text": "Deep thinking completed.", "thought": true }]
                    },
                    "finishReason": "STOP"
                }],
                "modelVersion": "gemini-2.0-flash-thinking",
                "responseId": "msg_normal_thinking_only",
                "usageMetadata": {
                    "promptTokenCount": 10,
                    "candidatesTokenCount": 50
                }
            });
            yield Ok::<_, String>(bytes::Bytes::from(format!("data: {}\n\n", chunk_json)));
        };

        let mut claude_stream = create_claude_sse_stream(
            Box::pin(mock_stream),
            "trace_normal_thinking_test".to_string(),
            "test@example.com".to_string(),
            None,
            false,
            1_000,
            None,
            1,
            None,
            Vec::new(),
        );

        let mut all_chunks = Vec::new();
        let mut error_propagated = false;
        while let Some(result) = claude_stream.next().await {
            match result {
                Ok(bytes) => all_chunks.push(String::from_utf8(bytes.to_vec()).unwrap()),
                Err(e) => {
                    error_propagated = true;
                    assert!(e.contains("thinking"));
                }
            }
        }
        let output = all_chunks.join("");

        // 1. 验证包含了 Thinking 内容
        assert!(output.contains("Deep thinking completed."));

        // 2. 严禁下发 message_stop 或 end_turn 伪造正常收尾
        assert!(
            !output.contains("event: message_stop"),
            "Must NOT emit message_stop when stream only has thinking"
        );
        assert!(
            !output.contains(r#""stop_reason":"end_turn""#),
            "Must NOT emit stop_reason end_turn when stream only has thinking"
        );

        // 3. 必须透传错误促使客户端自动重试
        assert!(
            error_propagated,
            "Must propagate Err when upstream finishes after thinking without text"
        );
    }

    #[tokio::test]
    async fn test_claude_code_2_1_293_state_machine_contract() {
        use futures::StreamExt;

        // 模拟上游在下发思考块（带 thought_signature）之后，因复杂 SVG 生成在 100s 计算期发生网络断开 (TCP RST)
        let mock_stream = async_stream::stream! {
            let chunk_thinking = serde_json::json!({
                "candidates": [{
                    "content": {
                        "parts": [{
                            "text": "Planning SVG coordinates...",
                            "thought": true
                        }]
                    }
                }],
                "modelVersion": "gemini-2.5-flash",
                "responseId": "msg_svg_test"
            });
            yield Ok::<_, String>(bytes::Bytes::from(format!("data: {}\n\n", chunk_thinking)));
            // 上游网络中断
            yield Err("error reading a body from connection: connection reset by peer".to_string());
        };

        let mut claude_stream = create_claude_sse_stream(
            Box::pin(mock_stream),
            "trace_svg_293".to_string(),
            "test@example.com".to_string(),
            Some("session_svg_test".to_string()),
            false,
            1_000,
            None,
            1,
            None,
            Vec::new(),
        );

        let mut all_chunks = Vec::new();
        let mut error_propagated = false;
        while let Some(result) = claude_stream.next().await {
            match result {
                Ok(bytes) => all_chunks.push(String::from_utf8(bytes.to_vec()).unwrap()),
                Err(_) => {
                    error_propagated = true;
                }
            }
        }
        let output = all_chunks.join("");

        // 验证 Claude Code 客户端原生断流重试契约：
        // 规则 1: 严禁伪造正常收尾 message_delta(stop_reason: "end_turn") 或 message_stop，确保客户端识别到断流并自动触发原生重试
        assert!(
            !output.contains(r#""stop_reason":"end_turn""#),
            "Must NOT fake stop_reason end_turn on mid-stream drop"
        );
        assert!(
            !output.contains("event: message_stop"),
            "Must NOT emit message_stop on mid-stream drop"
        );

        // 规则 2: 严禁向客户端下发任何 event: error 或 "overloaded_error"
        // 否则直接触发 Claude Code 的 PU() 判定为过载并进入 30 分钟惩罚性冷冻及指数退避死循环
        assert!(
            !output.contains("event: error"),
            "Must NOT emit event: error on mid-stream drop"
        );
        assert!(
            !output.contains(r#""type":"overloaded_error""#),
            "Must NEVER emit overloaded_error which triggers RYe exponential backoff in Claude Code"
        );

        // 规则 3: 必须向外层透传底层传输错误 Err，促使客户端捕获 ConnectionLost 并自动触发重试继续输出
        assert!(
            error_propagated,
            "Stream must propagate transport Err so client triggers native auto-retry and continues output"
        );
    }

    #[tokio::test]
    async fn test_create_claude_sse_stream_terminates_immediately_on_finish_reason_without_waiting_for_upstream_eof(
    ) {
        use futures::StreamExt;

        // 模拟真实 Gemini 流：下发带有 finishReason: "STOP" 的最终帧，但之后上游连接维持活跃长达 60s（不发送 EOF）
        let mock_stream = async_stream::stream! {
            let chunk_content = serde_json::json!({
                "candidates": [{
                    "content": {
                        "parts": [{ "text": "Task finished successfully." }]
                    },
                    "finishReason": "STOP"
                }],
                "modelVersion": "gemini-2.5-flash",
                "responseId": "msg_term_test",
                "usageMetadata": {
                    "promptTokenCount": 100,
                    "candidatesTokenCount": 50
                }
            });
            yield Ok::<_, String>(bytes::Bytes::from(format!("data: {}\n\n", chunk_content)));
            // 模拟上游 TCP 连接未关闭挂起 60 秒
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            yield Ok::<_, String>(bytes::Bytes::from("data: never_reached\n\n"));
        };

        let mut claude_stream = create_claude_sse_stream(
            Box::pin(mock_stream),
            "trace_term_test".to_string(),
            "test@example.com".to_string(),
            None,
            false,
            1_000,
            None,
            1,
            None,
            Vec::new(),
        );

        let mut all_chunks = Vec::new();
        while let Ok(Some(result)) =
            tokio::time::timeout(std::time::Duration::from_millis(500), claude_stream.next()).await
        {
            if let Ok(bytes) = result {
                all_chunks.push(String::from_utf8(bytes.to_vec()).unwrap());
            }
        }
        let output = all_chunks.join("");

        // 1. 验证包含终止 token 与终结事件
        assert!(
            output.contains(r#""stop_reason":"end_turn""#),
            "Must emit stop_reason: end_turn"
        );
        assert!(
            output.contains("event: message_stop"),
            "Must emit message_stop"
        );
        assert!(!output.contains("never_reached"));

        // 2. 关键断言：此时流必须已经完全结束（立刻返回 None，不阻塞等待上游 60s EOF）
        let next_item =
            tokio::time::timeout(std::time::Duration::from_millis(100), claude_stream.next()).await;
        assert_eq!(
            next_item,
            Ok(None),
            "Stream must terminate immediately upon message_stop without waiting for upstream EOF"
        );
    }

    #[tokio::test]
    async fn test_multichunk_token_usage_preserved_when_finish_reason_has_no_metadata() {
        use futures::StreamExt;

        // 模拟多 Chunk 流：
        // Chunk 1: 发送内容与 usageMetadata (prompt: 20, candidates: 88)
        // Chunk 2: 仅发送 finishReason: "STOP"，不包含 usageMetadata
        let mock_stream = async_stream::stream! {
            let chunk_1 = serde_json::json!({
                "candidates": [{
                    "content": {
                        "parts": [{ "text": "Analyzing code..." }]
                    }
                }],
                "modelVersion": "gemini-2.5-pro",
                "responseId": "msg_chunk_1",
                "usageMetadata": {
                    "promptTokenCount": 20,
                    "candidatesTokenCount": 88
                }
            });
            yield Ok::<_, String>(bytes::Bytes::from(format!("data: {}\n\n", chunk_1)));

            let chunk_2 = serde_json::json!({
                "candidates": [{
                    "finishReason": "STOP"
                }],
                "modelVersion": "gemini-2.5-pro",
                "responseId": "msg_chunk_2"
            });
            yield Ok::<_, String>(bytes::Bytes::from(format!("data: {}\n\n", chunk_2)));
        };

        let mut claude_stream = create_claude_sse_stream(
            Box::pin(mock_stream),
            "trace_usage_cache_test".to_string(),
            "test@example.com".to_string(),
            None,
            false,
            1_000,
            None,
            1,
            None,
            Vec::new(),
        );

        let mut all_chunks = Vec::new();
        while let Some(result) = claude_stream.next().await {
            if let Ok(bytes) = result {
                all_chunks.push(String::from_utf8(bytes.to_vec()).unwrap());
            }
        }
        let output = all_chunks.join("");

        // 验证 message_delta 中保留了 Chunk 1 的 Token 统计，而非归零
        assert!(
            output.contains(r#""output_tokens":88"#),
            "Output tokens must be preserved from earlier chunk when finishReason has no usageMetadata, got: {}",
            output
        );
        assert!(
            output.contains(r#""stop_reason":"end_turn""#),
            "Must emit stop_reason: end_turn"
        );
        assert!(
            output.contains("event: message_stop"),
            "Must emit message_stop"
        );
    }

    #[tokio::test]
    async fn test_clean_upstream_eof_without_finish_reason_emits_valid_completion_triplet() {
        use futures::StreamExt;

        // 模拟正常生成文本后，上游连接正常 EOF (返回 None) 但未显式下发 finishReason
        let mock_stream = async_stream::stream! {
            let chunk = serde_json::json!({
                "candidates": [{
                    "content": {
                        "parts": [{ "text": "Code refactored cleanly." }]
                    }
                }],
                "modelVersion": "gemini-2.5-flash",
                "responseId": "msg_eof_test",
                "usageMetadata": {
                    "promptTokenCount": 15,
                    "candidatesTokenCount": 35
                }
            });
            yield Ok::<_, String>(bytes::Bytes::from(format!("data: {}\n\n", chunk)));
            // 直接结束流 (EOF)
        };

        let mut claude_stream = create_claude_sse_stream(
            Box::pin(mock_stream),
            "trace_eof_test".to_string(),
            "test@example.com".to_string(),
            None,
            false,
            1_000,
            None,
            1,
            None,
            Vec::new(),
        );

        let mut all_chunks = Vec::new();
        while let Some(result) = claude_stream.next().await {
            if let Ok(bytes) = result {
                all_chunks.push(String::from_utf8(bytes.to_vec()).unwrap());
            }
        }
        let output = all_chunks.join("");

        // 验证 Claude Code 终止三元组 complete: bf !== null && Kw && fd === null
        // 1. content_block_stop 必须发出，确保 active block 指针 fd 被重置为 null (防止 StreamTruncatedError)
        assert!(
            output.contains("event: content_block_stop"),
            "Must close content block before message_stop"
        );

        // 2. message_delta 必须发出有效 stop_reason ("end_turn") 且保留 usage
        assert!(
            output.contains(r#""stop_reason":"end_turn""#),
            "Must emit stop_reason end_turn on clean EOF"
        );
        assert!(
            output.contains(r#""output_tokens":35"#),
            "Must preserve usage on clean EOF"
        );

        // 3. message_stop 必须发出
        assert!(
            output.contains("event: message_stop"),
            "Must emit message_stop on clean EOF"
        );
    }

    #[test]
    fn test_silent_firewall_drops_rogue_frames_after_message_stop() {
        let mut state = StreamingState::new();

        // 1. 模拟正常结束帧
        let done_line = r#"data: {"candidates":[{"content":{"parts":[{"text":"Done"}]},"finishReason":"STOP"}]}"#;
        let chunks = process_sse_line(done_line, &mut state, "trace_fw", "test@example.com");
        assert!(chunks.is_some());
        assert!(
            state.message_stop_sent,
            "State must record message_stop_sent"
        );

        // 2. 模拟上游越界下发的流尾部多余帧 (rogue frames)
        let trailing_line =
            r#"data: {"candidates":[{"content":{"parts":[{"text":"Extra text"}]}}]}"#;
        let dropped = process_sse_line(trailing_line, &mut state, "trace_fw", "test@example.com");
        assert!(
            dropped.is_none(),
            "Silent firewall must drop any frames arriving after message_stop"
        );

        let trailing_done = "data: [DONE]";
        let dropped_done =
            process_sse_line(trailing_done, &mut state, "trace_fw", "test@example.com");
        assert!(
            dropped_done.is_none(),
            "Silent firewall must drop [DONE] after message_stop"
        );
    }

    #[tokio::test]
    async fn test_empty_response_emits_single_message_delta_and_message_stop() {
        use futures::StreamExt;

        // 模拟完全空的流（如单点健康探测或模型直接返回空包）
        let mock_stream = async_stream::stream! {
            if false {
                yield Ok::<bytes::Bytes, String>(bytes::Bytes::new());
            }
        };

        let mut claude_stream = create_claude_sse_stream(
            Box::pin(mock_stream),
            "trace_empty_test".to_string(),
            "test@example.com".to_string(),
            None,
            false,
            1_000,
            None,
            1,
            None,
            Vec::new(),
        );

        let mut all_chunks = Vec::new();
        while let Some(result) = claude_stream.next().await {
            if let Ok(bytes) = result {
                all_chunks.push(String::from_utf8(bytes.to_vec()).unwrap());
            }
        }
        let output = all_chunks.join("");

        // 验证空响应恢复仅发出一次 message_delta 与一次 message_stop
        let delta_count = output.matches("event: message_delta").count();
        let stop_count = output.matches("event: message_stop").count();
        assert_eq!(
            delta_count, 1,
            "Empty response must emit exactly one message_delta, got: {}",
            output
        );
        assert_eq!(
            stop_count, 1,
            "Empty response must emit exactly one message_stop, got: {}",
            output
        );
        assert!(
            output.contains(r#""stop_reason":"end_turn""#),
            "Must emit stop_reason: end_turn"
        );
    }
}
