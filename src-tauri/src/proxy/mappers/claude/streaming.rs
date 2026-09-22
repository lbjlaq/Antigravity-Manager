// Claude 流式响应转换 (Gemini SSE → Claude SSE)
// 对应 StreamingState + PartProcessor

use super::models::*;
use super::utils::to_claude_usage;
use crate::proxy::mappers::estimation_calibrator::get_calibrator;
// use crate::proxy::mappers::signature_store::store_thought_signature; // Deprecated
use crate::proxy::common::client_adapter::{ClientAdapter, SignatureBufferStrategy}; // [NEW]
use crate::proxy::SignatureCache;
use bytes::Bytes;
use serde_json::{json, Value};

/// Passthrough tool arguments for Gemini → Claude compatibility
pub fn remap_function_call_args(name: &str, args: &mut Value) {
    // 纯透传协议工具参数，不进行任何字段重命名与拦截改写
    if let Some(obj) = args.as_object() {
        tracing::debug!(
            "[Streaming] Tool Call (Passthrough): '{}' Args: {:?}",
            name,
            obj
        );
    }
}

/// 块类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockType {
    None,
    Text,
    Thinking,
    Function,
}

/// 签名管理器
pub struct SignatureManager {
    pending: Option<String>,
}

impl SignatureManager {
    pub fn new() -> Self {
        Self { pending: None }
    }

    pub fn store(&mut self, signature: Option<String>) {
        if signature.is_some() {
            self.pending = signature;
        }
    }

    pub fn consume(&mut self) -> Option<String> {
        self.pending.take()
    }

    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
}

/// 流式状态机
pub struct StreamingState {
    block_type: BlockType,
    pub block_index: usize,
    pub message_start_sent: bool,
    pub message_stop_sent: bool,
    used_tool: bool,
    signatures: SignatureManager,
    trailing_signature: Option<String>,
    pub web_search_query: Option<String>,
    pub grounding_chunks: Option<Vec<serde_json::Value>>,
    // [IMPROVED] Error recovery 状态追踪 (prepared for future use)
    #[allow(dead_code)]
    parse_error_count: usize,
    #[allow(dead_code)]
    last_valid_state: Option<BlockType>,
    // [NEW] Model tracking for signature cache
    pub model_name: Option<String>,
    // [NEW v3.3.17] Session ID for session-based signature caching
    pub session_id: Option<String>,
    // [NEW] Flag for context usage scaling
    pub scaling_enabled: bool,
    // [NEW] Context limit for smart threshold recovery (default to 1M)
    pub context_limit: u32,
    // [NEW] MCP XML Bridge 缓冲区
    pub mcp_xml_buffer: String,
    pub in_mcp_xml: bool,
    // [FIX] Estimated prompt tokens for calibrator learning
    pub estimated_prompt_tokens: Option<u32>,
    // [FIX #859] Post-thinking interruption tracking
    pub has_thinking: bool,
    pub has_content: bool,
    pub message_count: usize, // [NEW v4.0.0] Message count for rewind detection
    pub client_adapter: Option<std::sync::Arc<dyn ClientAdapter>>, // [FIX] Remove Box, use Arc<dyn> directly
    // [FIX #MCP] Registered tool names for fuzzy matching
    pub registered_tool_names: Vec<String>,
    // [FIX #3379] Track whether any text_delta was emitted this turn (guard G7)
    pub text_delta_emitted_this_turn: bool,
    pub thinking_acc: crate::proxy::thinking_store::TurnAccumulator,
}

impl StreamingState {
    pub fn new() -> Self {
        Self {
            block_type: BlockType::None,
            block_index: 0,
            message_start_sent: false,
            message_stop_sent: false,
            used_tool: false,
            signatures: SignatureManager::new(),
            trailing_signature: None,
            web_search_query: None,
            grounding_chunks: None,
            // [IMPROVED] 初始化 error recovery 字段
            parse_error_count: 0,
            last_valid_state: None,
            model_name: None,
            session_id: None,
            scaling_enabled: false,
            context_limit: 1_048_576, // Default to 1M
            mcp_xml_buffer: String::new(),
            in_mcp_xml: false,
            estimated_prompt_tokens: None,
            has_thinking: false,
            has_content: false,
            message_count: 0,
            client_adapter: None,
            registered_tool_names: Vec::new(),
            text_delta_emitted_this_turn: false,
            thinking_acc: crate::proxy::thinking_store::TurnAccumulator::new(),
        }
    }

    // [NEW] Set client adapter
    pub fn set_client_adapter(&mut self, adapter: Option<std::sync::Arc<dyn ClientAdapter>>) {
        self.client_adapter = adapter;
    }

    // [FIX #MCP] Set registered tool names for fuzzy matching
    pub fn set_registered_tool_names(&mut self, names: Vec<String>) {
        self.registered_tool_names = names;
    }

    /// 发送 SSE 事件
    pub fn emit(&self, event_type: &str, data: serde_json::Value) -> Bytes {
        let sse = format!(
            "event: {}\ndata: {}\n\n",
            event_type,
            serde_json::to_string(&data).unwrap_or_default()
        );
        Bytes::from(sse)
    }

    /// 发送 message_start 事件
    pub fn emit_message_start(&mut self, raw_json: &serde_json::Value) -> Bytes {
        if self.message_start_sent {
            return Bytes::new();
        }

        let usage = raw_json
            .get("usageMetadata")
            .and_then(|u| serde_json::from_value::<UsageMetadata>(u.clone()).ok())
            .map(|u| to_claude_usage(&u, self.scaling_enabled, self.context_limit));

        let mut message = json!({
            "id": raw_json.get("responseId")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| "msg_unknown"),
            "type": "message",
            "role": "assistant",
            "content": [],
            "model": raw_json.get("modelVersion")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            "stop_reason": null,
            "stop_sequence": null,
        });

        // Capture model name for signature cache
        if let Some(m) = raw_json.get("modelVersion").and_then(|v| v.as_str()) {
            self.model_name = Some(m.to_string());
        }

        if let Some(u) = usage {
            message["usage"] = json!(u);
        } else {
            message["usage"] = json!({
                "input_tokens": 0,
                "output_tokens": 0
            });
        }

        let result = self.emit(
            "message_start",
            json!({
                "type": "message_start",
                "message": message
            }),
        );

        self.message_start_sent = true;
        result
    }

    /// 开始新的内容块
    pub fn start_block(
        &mut self,
        block_type: BlockType,
        content_block: serde_json::Value,
    ) -> Vec<Bytes> {
        let mut chunks = Vec::new();
        if self.block_type != BlockType::None {
            chunks.extend(self.end_block());
        }

        chunks.push(self.emit(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": self.block_index,
                "content_block": content_block
            }),
        ));

        self.block_type = block_type;
        chunks
    }

    /// 结束当前内容块
    pub fn end_block(&mut self) -> Vec<Bytes> {
        if self.block_type == BlockType::None {
            return vec![];
        }

        let mut chunks = Vec::new();

        // Thinking 块结束时发送暂存的签名 (若上游未下发签名则回退到会话签名或哨兵签名)
        if self.block_type == BlockType::Thinking {
            let signature = if self.signatures.has_pending() {
                self.signatures.consume()
            } else {
                self.session_id
                    .as_deref()
                    .and_then(|sid| {
                        crate::proxy::SignatureCache::global().get_session_signature(sid)
                    })
                    .or_else(|| Some("skip_thought_signature_validator".to_string()))
            };

            if let Some(sig) = signature {
                chunks.push(self.emit_delta("signature_delta", json!({ "signature": sig })));
            }
        }

        chunks.push(self.emit(
            "content_block_stop",
            json!({
                "type": "content_block_stop",
                "index": self.block_index
            }),
        ));

        self.block_index += 1;
        self.block_type = BlockType::None;

        chunks
    }

    /// 发送 delta 事件
    pub fn emit_delta(&self, delta_type: &str, delta_content: serde_json::Value) -> Bytes {
        let mut delta = json!({ "type": delta_type });
        if let serde_json::Value::Object(map) = delta_content {
            for (k, v) in map {
                delta[k] = v;
            }
        }

        self.emit(
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": self.block_index,
                "delta": delta
            }),
        )
    }

    /// 发送结束事件
    pub fn emit_finish(
        &mut self,
        finish_reason: Option<&str>,
        usage_metadata: Option<&UsageMetadata>,
    ) -> Vec<Bytes> {
        let mut chunks = Vec::new();

        // 关闭最后一个块
        chunks.extend(self.end_block());

        // 处理 trailingSignature (B4/C3 场景)
        // [FIX] 只有当还没有发送过任何块时, 才能以 thinking 块结束(作为消息的开头)
        // 实际上, 对于 Claude 协议, 如果已经发送过 Text, 就不能在此追加 Thinking。
        // 这里的解决方案是: 只存储签名, 不再发送非法的末尾 Thinking 块。
        // 签名会通过 SignatureCache 在下一轮请求中自动恢复。
        if let Some(signature) = self.trailing_signature.take() {
            tracing::info!(
                "[Streaming] Captured trailing signature (len: {}), caching for session.",
                signature.len()
            );
            self.signatures.store(Some(signature));
            // 不再追加 chunks.push(self.emit("content_block_start", ...))
        }

        // 处理 grounding(web search) -> 转换为 Markdown 文本块
        if self.web_search_query.is_some() || self.grounding_chunks.is_some() {
            let mut grounding_text = String::new();

            // 1. 处理搜索词
            if let Some(query) = &self.web_search_query {
                if !query.is_empty() {
                    grounding_text.push_str("\n\n---\n**🔍 已为您搜索：** ");
                    grounding_text.push_str(query);
                }
            }

            // 2. 处理来源链接
            if let Some(chunks) = &self.grounding_chunks {
                let mut links = Vec::new();
                for (i, chunk) in chunks.iter().enumerate() {
                    if let Some(web) = chunk.get("web") {
                        let title = web
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("网页来源");
                        let uri = web.get("uri").and_then(|v| v.as_str()).unwrap_or("#");
                        links.push(format!("[{}] [{}]({})", i + 1, title, uri));
                    }
                }

                if !links.is_empty() {
                    grounding_text.push_str("\n\n**🌐 来源引文：**\n");
                    grounding_text.push_str(&links.join("\n"));
                }
            }

            let trimmed_grounding = grounding_text.trim();
            if !trimmed_grounding.is_empty() {
                // 发送一个新的 text 块
                chunks.push(self.emit(
                    "content_block_start",
                    json!({
                        "type": "content_block_start",
                        "index": self.block_index,
                        "content_block": { "type": "text", "text": "" }
                    }),
                ));
                chunks.push(self.emit_delta("text_delta", json!({ "text": trimmed_grounding })));
                chunks.push(self.emit(
                    "content_block_stop",
                    json!({ "type": "content_block_stop", "index": self.block_index }),
                ));
                self.block_index += 1;
            }
        }

        // 确定 stop_reason
        let stop_reason = if self.used_tool {
            "tool_use"
        } else if finish_reason == Some("MAX_TOKENS") {
            "max_tokens"
        } else {
            "end_turn"
        };

        let usage = usage_metadata
            .map(|u| {
                // [FIX] Record actual token usage for calibrator learning
                // Now properly pairs estimated tokens from request with actual tokens from response
                if let (Some(estimated), Some(actual)) =
                    (self.estimated_prompt_tokens, u.prompt_token_count)
                {
                    if estimated > 0 && actual > 0 {
                        get_calibrator().record(estimated, actual);
                        tracing::debug!(
                            "[Calibrator] Recorded: estimated={}, actual={}, ratio={:.2}x",
                            estimated,
                            actual,
                            actual as f64 / estimated as f64
                        );
                    }
                }
                to_claude_usage(u, self.scaling_enabled, self.context_limit)
            })
            .unwrap_or(Usage {
                input_tokens: 0,
                output_tokens: 0,
                cache_read_input_tokens: None,
                cache_creation_input_tokens: None,
                server_tool_use: None,
            });

        chunks.push(self.emit(
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": { "stop_reason": stop_reason, "stop_sequence": null },
                "usage": usage
            }),
        ));

        if !self.message_stop_sent {
            chunks.push(Bytes::from(
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ));
            self.message_stop_sent = true;
        }

        chunks
    }

    /// 标记使用了工具
    pub fn mark_tool_used(&mut self) {
        self.used_tool = true;
    }

    /// 获取当前块类型
    pub fn current_block_type(&self) -> BlockType {
        self.block_type
    }

    /// 获取当前块索引
    pub fn current_block_index(&self) -> usize {
        self.block_index
    }

    /// 存储签名
    pub fn store_signature(&mut self, signature: Option<String>) {
        self.signatures.store(signature);
    }

    /// 设置 trailing signature
    pub fn set_trailing_signature(&mut self, signature: Option<String>) {
        self.trailing_signature = signature;
    }

    /// 获取 trailing signature (仅用于检查)
    pub fn has_trailing_signature(&self) -> bool {
        self.trailing_signature.is_some()
    }

    /// 处理 SSE 解析错误，实现优雅降级
    ///
    /// 当 SSE stream 中发生解析错误时:
    /// 1. 安全关闭当前 block
    /// 2. 递增错误计数器
    /// 3. 在 debug 模式下输出错误信息
    #[allow(dead_code)] // Prepared for future error recovery implementation
    pub fn handle_parse_error(&mut self, raw_data: &str) -> Vec<Bytes> {
        let mut chunks = Vec::new();

        self.parse_error_count += 1;

        tracing::warn!(
            "[SSE-Parser] Parse error #{} occurred. Raw data length: {} bytes",
            self.parse_error_count,
            raw_data.len()
        );

        // 安全关闭当前 block
        if self.block_type != BlockType::None {
            self.last_valid_state = Some(self.block_type);
            chunks.extend(self.end_block());
        }

        // Debug 模式下输出详细错误信息
        #[cfg(debug_assertions)]
        {
            let preview = if raw_data.len() > 100 {
                format!(
                    "{}...",
                    crate::proxy::mappers::common_utils::safe_truncate_str(raw_data, 100)
                )
            } else {
                raw_data.to_string()
            };
            tracing::debug!("[SSE-Parser] Failed chunk preview: {}", preview);
        }

        // 错误率过高时发出警告并尝试发送错误信号
        if self.parse_error_count > 3 {
            // 降低阈值,更早通知用户
            tracing::error!(
                "[SSE-Parser] High error rate detected ({} errors). Stream may be corrupted.",
                self.parse_error_count
            );

            // [FIX] Explicitly signal error to client to prevent UI freeze
            // using standard SSE error event format
            // data: {"type": "error", "error": {...}}
            chunks.push(self.emit(
                "error",
                json!({
                    "type": "error",
                    "error": {
                        "type": "overloaded_error", // Use standard type
                        "message": "网络连接不稳定，请检查您的网络或代理设置。",
                    }
                }),
            ));
        }

        chunks
    }

    /// 重置错误状态 (recovery 后调用)
    #[allow(dead_code)]
    pub fn reset_error_state(&mut self) {
        self.parse_error_count = 0;
        self.last_valid_state = None;
    }

    /// 获取错误计数 (用于监控)
    #[allow(dead_code)]
    pub fn get_error_count(&self) -> usize {
        self.parse_error_count
    }
}

/// Part 处理器
pub struct PartProcessor<'a> {
    state: &'a mut StreamingState,
}

impl<'a> PartProcessor<'a> {
    pub fn new(state: &'a mut StreamingState) -> Self {
        Self { state }
    }

    /// 处理单个 part
    pub fn process(&mut self, part: &GeminiPart) -> Vec<Bytes> {
        let mut chunks = Vec::new();
        // [FIX #545] Decode Base64 signature if present (Gemini sends Base64, Claude expects Raw)
        let signature = part.thought_signature.as_ref().map(|sig| {
            // Try to decode as base64
            use base64::Engine;
            match base64::engine::general_purpose::STANDARD.decode(sig) {
                Ok(decoded_bytes) => {
                    match String::from_utf8(decoded_bytes) {
                        Ok(decoded_str) => {
                            tracing::debug!(
                                "[Streaming] Decoded base64 signature (len {} -> {})",
                                sig.len(),
                                decoded_str.len()
                            );
                            decoded_str
                        }
                        Err(_) => sig.clone(), // Not valid UTF-8, keep as is
                    }
                }
                Err(_) => sig.clone(), // Not base64, keep as is
            }
        });

        // 1. FunctionCall 处理
        if let Some(fc) = &part.function_call {
            // 先处理 trailingSignature (B4/C3 场景)
            if self.state.has_trailing_signature() {
                chunks.extend(self.state.end_block());
                if let Some(trailing_sig) = self.state.trailing_signature.take() {
                    chunks.push(self.state.emit(
                        "content_block_start",
                        json!({
                            "type": "content_block_start",
                            "index": self.state.current_block_index(),
                            "content_block": { "type": "thinking", "thinking": "" }
                        }),
                    ));
                    chunks.push(
                        self.state
                            .emit_delta("thinking_delta", json!({ "thinking": "" })),
                    );
                    chunks.push(
                        self.state
                            .emit_delta("signature_delta", json!({ "signature": trailing_sig })),
                    );
                    chunks.extend(self.state.end_block());
                }
            }

            chunks.extend(self.process_function_call(fc, signature));
            // [FIX #859] Mark that we have received actual content (tool use)
            self.state.has_content = true;
            return chunks;
        }

        // 2. Text 处理
        if let Some(text) = &part.text {
            if part.thought.unwrap_or(false) {
                // Thinking
                chunks.extend(self.process_thinking(text, signature));
            } else {
                // 普通 Text
                chunks.extend(self.process_text(text, signature));
            }
        }

        // 3. InlineData (Image) 处理
        if let Some(img) = &part.inline_data {
            let mime_type = &img.mime_type;
            let data = &img.data;
            if !data.is_empty() {
                let markdown_img = format!("![image](data:{};base64,{})", mime_type, data);
                chunks.extend(self.process_text(&markdown_img, None));
            }
        }

        chunks
    }

    /// 处理 Thinking
    fn process_thinking(&mut self, text: &str, signature: Option<String>) -> Vec<Bytes> {
        let mut chunks = Vec::new();

        // 处理之前的 trailingSignature
        if self.state.has_trailing_signature() {
            chunks.extend(self.state.end_block());
            if let Some(trailing_sig) = self.state.trailing_signature.take() {
                chunks.push(self.state.emit(
                    "content_block_start",
                    json!({
                        "type": "content_block_start",
                        "index": self.state.current_block_index(),
                        "content_block": { "type": "thinking", "thinking": "" }
                    }),
                ));
                chunks.push(
                    self.state
                        .emit_delta("thinking_delta", json!({ "thinking": "" })),
                );
                chunks.push(
                    self.state
                        .emit_delta("signature_delta", json!({ "signature": trailing_sig })),
                );
                chunks.extend(self.state.end_block());
            }
        }

        // 开始或继续 thinking 块
        if self.state.current_block_type() != BlockType::Thinking {
            chunks.extend(self.state.start_block(
                BlockType::Thinking,
                json!({ "type": "thinking", "thinking": "" }),
            ));
        }

        // [FIX #859] Mark that we have received thinking content
        self.state.has_thinking = true;

        if !text.is_empty() {
            chunks.push(
                self.state
                    .emit_delta("thinking_delta", json!({ "thinking": text })),
            );
        }

        // [NEW] Apply Client Adapter Strategy
        let use_fifo = self
            .state
            .client_adapter
            .as_ref()
            .map(|a| a.signature_buffer_strategy() == SignatureBufferStrategy::Fifo)
            .unwrap_or(false);

        // [IMPROVED] Store signature to global cache
        if let Some(ref sig) = signature {
            // 1. Cache family if we know the model
            if let Some(model) = &self.state.model_name {
                SignatureCache::global().cache_thinking_family(sig.clone(), model.clone());
            }

            // 2. [NEW v3.3.17] Cache to session-based storage for tool loop recovery
            if let Some(session_id) = &self.state.session_id {
                // If FIFO strategy is enabled, use a unique index for each signature (e.g. timestamp or counter)
                // However, our cache implementation currently keys by session_id.
                // For FIFO, we might just rely on the fact that we are processing in order.
                // But specifically for opencode, it might be calling tools in parallel or sequence.

                SignatureCache::global().cache_session_signature(
                    session_id,
                    sig.clone(),
                    self.state.message_count,
                );
                tracing::debug!(
                    "[Claude-SSE] Cached signature to session {} (length: {}) [FIFO: {}]",
                    session_id,
                    sig.len(),
                    use_fifo
                );
            }

            tracing::debug!(
                "[Claude-SSE] Captured thought_signature from thinking block (length: {})",
                sig.len()
            );
        }

        // 暂存签名 (for local block handling)
        // If FIFO, we strictly follow the sequence. The default logic is effectively LIFO for a single turn
        // (store latest, consume at end).
        // For opencode, we just want to ensure we capture IT.
        self.state.store_signature(signature);

        chunks
    }

    /// 处理普通 Text
    fn process_text(&mut self, text: &str, signature: Option<String>) -> Vec<Bytes> {
        let mut chunks = Vec::new();

        // 空 text 带签名 - 暂存
        if text.is_empty() {
            if signature.is_some() {
                self.state.set_trailing_signature(signature);
            }
            return chunks;
        }

        // [FIX #859] Mark that we have received actual content (text)
        self.state.has_content = true;

        // 处理之前的 trailingSignature
        if self.state.has_trailing_signature() {
            chunks.extend(self.state.end_block());
            if let Some(trailing_sig) = self.state.trailing_signature.take() {
                chunks.push(self.state.emit(
                    "content_block_start",
                    json!({
                        "type": "content_block_start",
                        "index": self.state.current_block_index(),
                        "content_block": { "type": "thinking", "thinking": "" }
                    }),
                ));
                chunks.push(
                    self.state
                        .emit_delta("thinking_delta", json!({ "thinking": "" })),
                );
                chunks.push(
                    self.state
                        .emit_delta("signature_delta", json!({ "signature": trailing_sig })),
                );
                chunks.extend(self.state.end_block());
            }
        }

        // 非空 text 带签名 - 立即处理
        if signature.is_some() {
            // [FIX] 为保护签名, 签名所在的 Text 块直接发送
            // 注意: 不得在此开启 thinking 块, 因为之前可能已有非 thinking 内容。
            // 这种情况下, 我们只需确签被缓存在状态中。
            self.state.store_signature(signature);

            chunks.extend(
                self.state
                    .start_block(BlockType::Text, json!({ "type": "text", "text": "" })),
            );
            chunks.push(self.state.emit_delta("text_delta", json!({ "text": text })));
            chunks.extend(self.state.end_block());

            return chunks;
        }

        // Ordinary text (without signature)

        // [NEW] MCP XML Bridge: Intercept and parse <mcp__...> tags
        if text.contains("<mcp__") || self.state.in_mcp_xml {
            self.state.in_mcp_xml = true;
            self.state.mcp_xml_buffer.push_str(text);

            // Check if we have a complete tag in the buffer
            if self.state.mcp_xml_buffer.contains("</mcp__")
                && self.state.mcp_xml_buffer.contains('>')
            {
                let buffer = self.state.mcp_xml_buffer.clone();
                if let Some(start_idx) = buffer.find("<mcp__") {
                    if let Some(tag_end_idx) = buffer[start_idx..].find('>') {
                        let actual_tag_end = start_idx + tag_end_idx;
                        let tool_name = &buffer[start_idx + 1..actual_tag_end];
                        let end_tag = format!("</{}>", tool_name);

                        if let Some(close_idx) = buffer.find(&end_tag) {
                            let input_str = &buffer[actual_tag_end + 1..close_idx];
                            let input_json: serde_json::Value =
                                serde_json::from_str(input_str.trim())
                                    .unwrap_or_else(|_| json!({ "input": input_str.trim() }));

                            // 构造并发送 tool_use
                            let fc = FunctionCall {
                                name: tool_name.to_string(),
                                args: Some(input_json),
                                id: Some(format!("{}-xml", tool_name)),
                            };

                            let tool_chunks = self.process_function_call(&fc, None);

                            // 清理缓冲区并重置状态
                            self.state.mcp_xml_buffer.clear();
                            self.state.in_mcp_xml = false;

                            // 处理标签之前可能存在的非 XML 文本
                            if start_idx > 0 {
                                let prefix_text = &buffer[..start_idx];
                                // 这里不能递归。直接 emit 之前的 text 块。
                                if self.state.current_block_type() != BlockType::Text {
                                    chunks.extend(self.state.start_block(
                                        BlockType::Text,
                                        json!({ "type": "text", "text": "" }),
                                    ));
                                }
                                chunks.push(
                                    self.state
                                        .emit_delta("text_delta", json!({ "text": prefix_text })),
                                );
                            }

                            chunks.extend(tool_chunks);

                            // 处理标签之后可能存在的非 XML 文本
                            let suffix = &buffer[close_idx + end_tag.len()..];
                            if !suffix.is_empty() {
                                // 递归处理后缀内容
                                chunks.extend(self.process_text(suffix, None));
                            }

                            return chunks;
                        }
                    }
                }
            }
            // While in XML, don't emit text deltas
            return vec![];
        }

        // [FIX #3379] call:default_api:* leakage recovery bridge
        // Attempt to recover leaked tool-call text as a proper tool_use block.
        // Strict Fail-Closed: any unmet guard falls through to normal text_delta.
        if let Some(recovery_chunks) = self.try_recover_call_default_api_text(text) {
            return recovery_chunks;
        }

        if self.state.current_block_type() != BlockType::Text {
            chunks.extend(
                self.state
                    .start_block(BlockType::Text, json!({ "type": "text", "text": "" })),
            );
        }

        self.state.text_delta_emitted_this_turn = true;
        chunks.push(self.state.emit_delta("text_delta", json!({ "text": text })));

        chunks
    }

    // -------------------------------------------------------------------------
    // [FIX #3379] Helpers: call:default_api:* leakage detection & recovery
    // -------------------------------------------------------------------------

    /// Attempt to extract `(tool_name, args_str)` from a raw `call:default_api:*` text.
    /// Returns None if the text does not strictly match the expected prefix format.
    fn try_parse_call_default_api(text: &str) -> Option<(String, String)> {
        const PREFIX: &str = "call:default_api:";
        let trimmed = text.trim();
        if !trimmed.starts_with(PREFIX) {
            return None;
        }
        let rest = &trimmed[PREFIX.len()..];
        // Tool name ends at the first `{` or `(` delimiter
        let tool_end = rest.find(|c| c == '{' || c == '(').unwrap_or(rest.len());
        let tool_name = rest[..tool_end].trim().to_string();
        if tool_name.is_empty() {
            return None;
        }
        let args_str = rest[tool_end..].trim().to_string();
        Some((tool_name, args_str))
    }

    /// Two-phase JSON argument parser for Gemini's loose call:default_api format.
    ///
    /// Phase 1: standard `serde_json` parse (handles well-formed JSON).
    /// Phase 2: lenient key-quoting heuristic for unquoted keys (e.g. `{file_path:foo}`).
    /// Returns `None` (guard G6) if both phases fail.
    fn parse_loose_json_args(args_str: &str) -> Option<serde_json::Value> {
        if args_str.is_empty() {
            // No-arg tool call → valid empty object
            return Some(serde_json::json!({}));
        }

        // Phase 1: strict JSON
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(args_str) {
            if v.is_object() {
                return Some(v);
            }
        }

        // Phase 2: lenient key-quoting for Gemini's internal pseudo-JSON
        // Only attempt if the string looks like a {…} block.
        let s = args_str.trim();
        if !s.starts_with('{') || !s.ends_with('}') {
            return None;
        }
        let inner = &s[1..s.len() - 1];

        // Naïve key-quoting: add double-quotes around bare identifier keys.
        // This covers the most common Gemini output patterns.
        let mut result = String::from("{");
        let mut in_value = false;
        let mut i = 0;
        let chars: Vec<char> = inner.chars().collect();
        while i < chars.len() {
            let c = chars[i];
            match c {
                ',' if !in_value => {
                    result.push(',');
                    i += 1;
                }
                ':' => {
                    result.push(':');
                    in_value = true;
                    i += 1;
                }
                '"' if in_value => {
                    // Quoted value — pass through until closing quote
                    result.push('"');
                    i += 1;
                    while i < chars.len() && chars[i] != '"' {
                        if chars[i] == '\\' {
                            result.push('\\');
                            i += 1;
                        }
                        if i < chars.len() {
                            result.push(chars[i]);
                            i += 1;
                        }
                    }
                    if i < chars.len() {
                        result.push('"');
                        i += 1;
                    }
                    in_value = false;
                }
                _ if !in_value => {
                    // Bare key — wrap with quotes
                    let key_start = i;
                    while i < chars.len() && chars[i] != ':' && chars[i] != ',' {
                        i += 1;
                    }
                    let key: String = chars[key_start..i].iter().collect();
                    let key_trimmed = key.trim();
                    result.push('"');
                    result.push_str(key_trimmed);
                    result.push('"');
                }
                _ => {
                    // Bare value — collect until `,` or end
                    let val_start = i;
                    while i < chars.len() && chars[i] != ',' {
                        result.push(chars[i]);
                        i += 1;
                    }
                    let _ = val_start; // consumed inline
                    in_value = false;
                }
            }
        }
        result.push('}');

        serde_json::from_str::<serde_json::Value>(&result)
            .ok()
            .filter(|v| v.is_object())
    }

    /// Fail-Closed recovery: attempts to convert a `call:default_api:*` text leak
    /// into a proper `tool_use` block by passing 7 strict guards.
    ///
    /// Returns `Some(chunks)` when recovery succeeds, or `None` to let the caller
    /// fall through to the normal `text_delta` path.
    fn try_recover_call_default_api_text(&mut self, text: &str) -> Option<Vec<bytes::Bytes>> {
        // G1: Tools must have been registered for this request
        if self.state.registered_tool_names.is_empty() {
            return None;
        }

        // G3: Strict prefix check before doing any expensive work
        if !text.trim().starts_with("call:default_api:") {
            return None;
        }

        let (tool_name, args_str) = Self::try_parse_call_default_api(text)?;

        // G5: The entire text (trimmed) must consist solely of this one expression.
        // This prevents documentation strings like "Use call:default_api:Read{...}" from
        // being mistakenly executed.
        let full_expr = format!(
            "call:default_api:{}{}",
            tool_name,
            if args_str.is_empty() {
                String::new()
            } else {
                format!(" {}", args_str)
            }
        );
        // Allow minor whitespace variance but reject any surrounding prose
        let trimmed_text = text.trim();
        // The text must start with call:default_api:<tool_name> and end right after the args block
        if !trimmed_text.starts_with(&format!("call:default_api:{}", tool_name)) {
            return None;
        }
        // After the tool name and args there must be nothing else (ignore trailing whitespace)
        let after_tool_name = &trimmed_text["call:default_api:".len() + tool_name.len()..].trim();
        // after_tool_name is either empty (no args) or is the args string itself
        if !after_tool_name.is_empty()
            && !after_tool_name.starts_with('{')
            && !after_tool_name.starts_with('(')
        {
            // There is non-arg text after the tool name — reject
            return None;
        }
        let _ = full_expr; // suppress unused warning

        // G4: Tool name must be in the registered whitelist (exact match, case-insensitive)
        let matched_name = self
            .state
            .registered_tool_names
            .iter()
            .find(|n| n.eq_ignore_ascii_case(&tool_name))
            .cloned()?;

        // G2: Current turn must NOT have already produced a structured functionCall
        if self.state.used_tool {
            tracing::warn!(
                "[#3379] Detected call:default_api:{} leakage but used_tool=true; skipping recovery",
                tool_name
            );
            return None;
        }

        // G7: No text_delta must have been emitted this turn
        if self.state.text_delta_emitted_this_turn {
            tracing::warn!(
                "[#3379] Detected call:default_api:{} leakage but text_delta already emitted this turn; skipping recovery",
                tool_name
            );
            return None;
        }

        // G6: Arguments must parse as a valid JSON object
        let parsed_args = Self::parse_loose_json_args(&args_str)?;

        // All guards passed — perform recovery
        tracing::warn!(
            "[#3379-RECOVERY] Recovering call:default_api:{} as tool_use (args_len={})",
            matched_name,
            args_str.len()
        );

        let fc = FunctionCall {
            name: matched_name,
            args: Some(parsed_args),
            id: None,
        };

        Some(self.process_function_call(&fc, None))
    }

    /// Process FunctionCall and capture signature for global storage
    fn process_function_call(
        &mut self,
        fc: &FunctionCall,
        signature: Option<String>,
    ) -> Vec<Bytes> {
        let mut chunks = Vec::new();

        self.state.mark_tool_used();

        let tool_id = fc.id.clone().unwrap_or_else(|| {
            format!(
                "{}-{}",
                fc.name,
                crate::proxy::common::utils::generate_random_id()
            )
        });

        let tool_name = fc.name.clone();

        // Record real tool_id into TurnAccumulator for precise session/fingerprint recovery
        self.state.thinking_acc.record_tool_id(&tool_name, &tool_id);

        // 1. 发送 content_block_start (input 为空对象)
        let mut tool_use = json!({
            "type": "tool_use",
            "id": tool_id,
            "name": tool_name,
            "input": {} // 必须为空，参数通过 delta 发送
        });

        if let Some(ref sig) = signature {
            tool_use["signature"] = json!(sig);

            // 2. Cache tool signature (Layer 1 recovery)
            SignatureCache::global().cache_tool_signature(&tool_id, sig.clone());

            // 3. [NEW v3.3.17] Cache to session-based storage
            if let Some(session_id) = &self.state.session_id {
                SignatureCache::global().cache_session_signature(
                    session_id,
                    sig.clone(),
                    self.state.message_count,
                );
            }

            tracing::debug!(
                "[Claude-SSE] Captured thought_signature for function call (length: {})",
                sig.len()
            );
        }

        chunks.extend(self.state.start_block(BlockType::Function, tool_use));

        // 2. 发送 input_json_delta (完整的参数 JSON 字符串)
        // [FIX #Bug2/#Bug4] ALWAYS emit input_json_delta, even for empty/null args.
        // Claude protocol requires this delta before content_block_stop.
        // Skipping it causes clients (Claude Code) to misinterpret tool calls as text.
        {
            let json_str = if let Some(args) = &fc.args {
                let mut remapped_args = args.clone();
                remap_function_call_args(&fc.name, &mut remapped_args);
                serde_json::to_string(&remapped_args).unwrap_or_else(|_| "{}".to_string())
            } else {
                // [FIX #Bug4] No args provided (e.g. EnterPlanMode): emit empty JSON object
                tracing::debug!(
                    "[Streaming] Tool '{}' has no args, emitting empty input_json_delta",
                    fc.name
                );
                "{}".to_string()
            };

            chunks.push(
                self.state
                    .emit_delta("input_json_delta", json!({ "partial_json": json_str })),
            );
        }

        // 3. 结束块
        chunks.extend(self.state.end_block());

        chunks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_manager() {
        let mut mgr = SignatureManager::new();
        assert!(!mgr.has_pending());

        mgr.store(Some("sig123".to_string()));
        assert!(mgr.has_pending());

        let sig = mgr.consume();
        assert_eq!(sig, Some("sig123".to_string()));
        assert!(!mgr.has_pending());
    }

    #[test]
    fn test_streaming_state_emit() {
        let state = StreamingState::new();
        let chunk = state.emit("test_event", json!({"foo": "bar"}));

        let s = String::from_utf8(chunk.to_vec()).unwrap();
        assert!(s.contains("event: test_event"));
        assert!(s.contains("\"foo\":\"bar\""));
    }

    #[test]
    fn test_process_function_call_deltas() {
        let mut state = StreamingState::new();
        let mut processor = PartProcessor::new(&mut state);

        let fc = FunctionCall {
            name: "test_tool".to_string(),
            args: Some(json!({"arg": "value"})),
            id: Some("call_123".to_string()),
        };

        // Create a dummy GeminiPart with function_call
        let part = GeminiPart {
            text: None,
            function_call: Some(fc),
            inline_data: None,
            thought: None,
            thought_signature: None,
            function_response: None,
        };

        let chunks = processor.process(&part);
        let output = chunks
            .iter()
            .map(|b| String::from_utf8(b.to_vec()).unwrap())
            .collect::<Vec<_>>()
            .join("");

        // Verify sequence:
        // 1. content_block_start with empty input
        assert!(output.contains(r#""type":"content_block_start""#));
        assert!(output.contains(r#""name":"test_tool""#));
        assert!(output.contains(r#""input":{}"#));

        // 2. input_json_delta with serialized args
        assert!(output.contains(r#""type":"content_block_delta""#));
        assert!(output.contains(r#""type":"input_json_delta""#));
        // partial_json should contain escaped JSON string
        assert!(output.contains(r#"partial_json":"{\"arg\":\"value\"}"#));

        // 3. content_block_stop
        assert!(output.contains(r#""type":"content_block_stop""#));
    }

    /// [FIX #Bug4] Tool with args=None MUST still emit input_json_delta with "{}"
    #[test]
    fn test_process_function_call_no_args_emits_empty_delta() {
        let mut state = StreamingState::new();
        let mut processor = PartProcessor::new(&mut state);

        let fc = FunctionCall {
            name: "EnterPlanMode".to_string(),
            args: None, // No args at all
            id: Some("call_no_args".to_string()),
        };

        let part = GeminiPart {
            text: None,
            function_call: Some(fc),
            inline_data: None,
            thought: None,
            thought_signature: None,
            function_response: None,
        };

        let chunks = processor.process(&part);
        let output = chunks
            .iter()
            .map(|b| String::from_utf8(b.to_vec()).unwrap())
            .collect::<Vec<_>>()
            .join("");

        // Must have tool_use block start
        assert!(output.contains(r#""type":"content_block_start""#));
        assert!(output.contains(r#""name":"EnterPlanMode""#));

        // [FIX #Bug4] Must emit input_json_delta even for None args
        assert!(
            output.contains(r#""type":"input_json_delta""#),
            "MUST emit input_json_delta even when args is None; output={}",
            &output[..output.len().min(600)]
        );
        assert!(
            output.contains(r#""partial_json":"{}""#),
            "input_json_delta must be empty JSON object for None args"
        );

        // Must close block
        assert!(output.contains(r#""type":"content_block_stop""#));
        assert!(state.used_tool);
    }

    /// [FIX #Bug2] Tool with args=Some({}) (empty obj) MUST still emit input_json_delta
    #[test]
    fn test_process_function_call_empty_args_emits_delta() {
        let mut state = StreamingState::new();
        let mut processor = PartProcessor::new(&mut state);

        let fc = FunctionCall {
            name: "SomeTool".to_string(),
            args: Some(json!({})), // Empty args object
            id: Some("call_empty".to_string()),
        };

        let part = GeminiPart {
            text: None,
            function_call: Some(fc),
            inline_data: None,
            thought: None,
            thought_signature: None,
            function_response: None,
        };

        let chunks = processor.process(&part);
        let output = chunks
            .iter()
            .map(|b| String::from_utf8(b.to_vec()).unwrap())
            .collect::<Vec<_>>()
            .join("");

        // [FIX #Bug2] Must emit input_json_delta for empty object args
        assert!(
            output.contains(r#""type":"input_json_delta""#),
            "Must emit input_json_delta for empty args object; output={}",
            &output[..output.len().min(600)]
        );
        assert!(state.used_tool);
    }

    // -------------------------------------------------------------------------
    // [FIX #3379] Tests: call:default_api:* leakage recovery
    // -------------------------------------------------------------------------

    /// Helper: build a PartProcessor with registered tool names
    fn make_processor_with_tools<'a>(
        state: &'a mut StreamingState,
        tools: Vec<&str>,
    ) -> PartProcessor<'a> {
        state.set_registered_tool_names(tools.into_iter().map(|s| s.to_string()).collect());
        PartProcessor::new(state)
    }

    /// Collect all SSE chunks into a single string
    fn chunks_to_string(chunks: &[bytes::Bytes]) -> String {
        chunks
            .iter()
            .map(|b| String::from_utf8_lossy(b).to_string())
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn test_3379_positive_recovery_standard_json() {
        // Positive: registered tool, standard JSON args, no prose → should recover
        let mut state = StreamingState::new();
        let mut processor = make_processor_with_tools(&mut state, vec!["Read"]);

        let text = r#"call:default_api:Read{"file_path":"/tmp/foo.txt","limit":100}"#;
        let part = GeminiPart {
            text: Some(text.to_string()),
            function_call: None,
            inline_data: None,
            thought: None,
            thought_signature: None,
            function_response: None,
        };
        let chunks = processor.process(&part);
        let output = chunks_to_string(&chunks);

        // Should produce a tool_use block, not a text_delta
        assert!(
            output.contains(r#""type":"content_block_start""#),
            "Expected tool_use block_start, got: {}",
            output
        );
        assert!(
            output.contains(r#""name":"Read""#),
            "Expected tool name Read"
        );
        assert!(
            !output.contains("text_delta"),
            "Must NOT produce text_delta for recovered call"
        );
        assert!(state.used_tool, "used_tool must be true after recovery");
    }

    #[test]
    fn test_3379_negative_tool_not_registered() {
        // G4 guard: tool name not in whitelist → text_delta
        let mut state = StreamingState::new();
        let mut processor = make_processor_with_tools(&mut state, vec!["Write"]);

        let text = r#"call:default_api:Read{"file_path":"/tmp/foo.txt"}"#;
        let part = GeminiPart {
            text: Some(text.to_string()),
            function_call: None,
            inline_data: None,
            thought: None,
            thought_signature: None,
            function_response: None,
        };
        let chunks = processor.process(&part);
        let output = chunks_to_string(&chunks);

        assert!(
            output.contains("text_delta"),
            "Unregistered tool must fall through to text_delta"
        );
        assert!(!state.used_tool, "used_tool must remain false");
    }

    #[test]
    fn test_3379_negative_no_tools_registered() {
        // G1 guard: no tools in request → text_delta
        let mut state = StreamingState::new(); // no registered_tool_names
        let mut processor = PartProcessor::new(&mut state);

        let text = r#"call:default_api:Read{"file_path":"/tmp/foo.txt"}"#;
        let part = GeminiPart {
            text: Some(text.to_string()),
            function_call: None,
            inline_data: None,
            thought: None,
            thought_signature: None,
            function_response: None,
        };
        let chunks = processor.process(&part);
        let output = chunks_to_string(&chunks);

        assert!(
            output.contains("text_delta"),
            "Empty registered_tool_names must fall through to text_delta"
        );
    }

    #[test]
    fn test_3379_negative_surrounding_prose() {
        // G5 guard: text not solely the call expression → text_delta
        let mut state = StreamingState::new();
        let mut processor = make_processor_with_tools(&mut state, vec!["Read"]);

        let text = "Here is what I am doing: call:default_api:Read{\"file_path\":\"/tmp/foo.txt\"}";
        let part = GeminiPart {
            text: Some(text.to_string()),
            function_call: None,
            inline_data: None,
            thought: None,
            thought_signature: None,
            function_response: None,
        };
        let chunks = processor.process(&part);
        let output = chunks_to_string(&chunks);

        assert!(
            output.contains("text_delta"),
            "Prose-wrapped call must fall through to text_delta"
        );
        assert!(!state.used_tool);
    }

    #[test]
    fn test_3379_negative_broken_json_args() {
        // G6 guard: args not valid JSON → text_delta
        let mut state = StreamingState::new();
        let mut processor = make_processor_with_tools(&mut state, vec!["Read"]);

        let text = "call:default_api:Read{file_path: NOT JSON !!!}";
        let part = GeminiPart {
            text: Some(text.to_string()),
            function_call: None,
            inline_data: None,
            thought: None,
            thought_signature: None,
            function_response: None,
        };
        let chunks = processor.process(&part);
        let output = chunks_to_string(&chunks);

        assert!(
            output.contains("text_delta"),
            "Broken JSON args must fall through to text_delta"
        );
        assert!(!state.used_tool);
    }

    #[test]
    fn test_3379_negative_text_delta_already_emitted() {
        // G7 guard: text_delta already emitted this turn → skip recovery
        let mut state = StreamingState::new();
        state.set_registered_tool_names(vec!["Read".to_string()]);
        // Simulate a prior text_delta having been emitted
        state.text_delta_emitted_this_turn = true;

        let mut processor = PartProcessor::new(&mut state);
        let text = r#"call:default_api:Read{"file_path":"/tmp/foo.txt"}"#;
        let part = GeminiPart {
            text: Some(text.to_string()),
            function_call: None,
            inline_data: None,
            thought: None,
            thought_signature: None,
            function_response: None,
        };
        let chunks = processor.process(&part);
        let output = chunks_to_string(&chunks);

        assert!(
            output.contains("text_delta"),
            "G7 must prevent recovery after text_delta was emitted"
        );
        assert!(!state.used_tool);
    }

    #[test]
    fn test_3379_parse_loose_json_standard() {
        // parse_loose_json_args: standard JSON passes Phase 1
        let result =
            PartProcessor::parse_loose_json_args(r#"{"file_path":"/tmp/foo","limit":100}"#);
        assert!(result.is_some());
        let v = result.unwrap();
        assert_eq!(v["file_path"], "/tmp/foo");
        assert_eq!(v["limit"], 100);
    }

    #[test]
    fn test_3379_parse_loose_json_empty() {
        // Empty args → valid empty object
        let result = PartProcessor::parse_loose_json_args("");
        assert_eq!(result, Some(serde_json::json!({})));
    }

    #[test]
    fn test_3379_regression_native_function_call_unaffected() {
        // Regression: native functionCall must still produce tool_use unaffected
        let mut state = StreamingState::new();
        state.set_registered_tool_names(vec!["Read".to_string()]);
        let mut processor = PartProcessor::new(&mut state);

        let fc = FunctionCall {
            name: "Read".to_string(),
            args: Some(json!({"file_path": "/tmp/native.txt"})),
            id: Some("call_native_001".to_string()),
        };
        let part = GeminiPart {
            text: None,
            function_call: Some(fc),
            inline_data: None,
            thought: None,
            thought_signature: None,
            function_response: None,
        };
        let chunks = processor.process(&part);
        let output = chunks_to_string(&chunks);

        assert!(
            output.contains(r#""type":"content_block_start""#),
            "Native functionCall must still produce tool_use"
        );
        assert!(output.contains(r#""name":"Read""#));
        assert!(!output.contains("text_delta"));
        assert!(state.used_tool);
    }
}
