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

/// Known parameter remappings for Gemini → Claude compatibility
/// [FIX] Gemini sometimes uses different parameter names than specified in tool schema
pub fn remap_function_call_args(name: &str, args: &mut Value) {
    // [DEBUG] Always log incoming tool usage for diagnosis
    if let Some(obj) = args.as_object() {
        tracing::debug!("[Streaming] Tool Call: '{}' Args: {:?}", name, obj);
    }

    // [IMPORTANT] Claude Code CLI 的 EnterPlanMode 工具禁止携带任何参数
    // 代理层注入的 reason 参数会导致 InputValidationError
    if name == "EnterPlanMode" {
        if let Some(obj) = args.as_object_mut() {
            obj.clear();
        }
        return;
    }

    if let Some(obj) = args.as_object_mut() {
        // [IMPROVED] Case-insensitive matching for tool names
        match name.to_lowercase().as_str() {
            "grep" | "search" | "search_code_definitions" | "search_code_snippets" => {
                // [FIX] Gemini hallucination: maps parameter description to "description" field
                if let Some(desc) = obj.remove("description") {
                    if !obj.contains_key("pattern") {
                        obj.insert("pattern".to_string(), desc);
                        tracing::debug!("[Streaming] Remapped Grep: description → pattern");
                    }
                }

                // Gemini uses "query", Claude Code expects "pattern"
                if let Some(query) = obj.remove("query") {
                    if !obj.contains_key("pattern") {
                        obj.insert("pattern".to_string(), query);
                        tracing::debug!("[Streaming] Remapped Grep: query → pattern");
                    }
                }

                // [CRITICAL FIX] Claude Code uses "path" (string), NOT "paths" (array)!
                if !obj.contains_key("path") {
                    if let Some(paths) = obj.remove("paths") {
                        let path_str = if let Some(arr) = paths.as_array() {
                            arr.get(0)
                                .and_then(|v| v.as_str())
                                .unwrap_or(".")
                                .to_string()
                        } else if let Some(s) = paths.as_str() {
                            s.to_string()
                        } else {
                            ".".to_string()
                        };
                        obj.insert("path".to_string(), serde_json::json!(path_str));
                        tracing::debug!(
                            "[Streaming] Remapped Grep: paths → path(\"{}\")",
                            path_str
                        );
                    } else {
                        // Default to current directory if missing
                        obj.insert("path".to_string(), json!("."));
                        tracing::debug!("[Streaming] Added default path: \".\"");
                    }
                }

                // Note: We keep "-n" and "output_mode" if present as they are valid in Grep schema
            }
            "glob" => {
                // [FIX] Gemini hallucination: maps parameter description to "description" field
                if let Some(desc) = obj.remove("description") {
                    if !obj.contains_key("pattern") {
                        obj.insert("pattern".to_string(), desc);
                        tracing::debug!("[Streaming] Remapped Glob: description → pattern");
                    }
                }

                // Gemini uses "query", Claude Code expects "pattern"
                if let Some(query) = obj.remove("query") {
                    if !obj.contains_key("pattern") {
                        obj.insert("pattern".to_string(), query);
                        tracing::debug!("[Streaming] Remapped Glob: query → pattern");
                    }
                }

                // [CRITICAL FIX] Claude Code uses "path" (string), NOT "paths" (array)!
                if !obj.contains_key("path") {
                    if let Some(paths) = obj.remove("paths") {
                        let path_str = if let Some(arr) = paths.as_array() {
                            arr.get(0)
                                .and_then(|v| v.as_str())
                                .unwrap_or(".")
                                .to_string()
                        } else if let Some(s) = paths.as_str() {
                            s.to_string()
                        } else {
                            ".".to_string()
                        };
                        obj.insert("path".to_string(), serde_json::json!(path_str));
                        tracing::debug!(
                            "[Streaming] Remapped Glob: paths → path(\"{}\")",
                            path_str
                        );
                    } else {
                        // Default to current directory if missing
                        obj.insert("path".to_string(), json!("."));
                        tracing::debug!("[Streaming] Added default path: \".\"");
                    }
                }
            }
            "read" => {
                // Gemini might use "path" vs "file_path"
                if let Some(path) = obj.remove("path") {
                    if !obj.contains_key("file_path") {
                        obj.insert("file_path".to_string(), path);
                        tracing::debug!("[Streaming] Remapped Read: path → file_path");
                    }
                }
            }
            "ls" => {
                // LS tool: ensure "path" parameter exists
                if !obj.contains_key("path") {
                    obj.insert("path".to_string(), json!("."));
                    tracing::debug!("[Streaming] Remapped LS: default path → \".\"");
                }
            }
            other => {
                // [NEW] [Issue #785] Generic Property Mapping for all tools
                // If a tool has "paths" (array of 1) but no "path", convert it.
                let mut path_to_inject = None;
                if !obj.contains_key("path") {
                    if let Some(paths) = obj.get("paths").and_then(|v| v.as_array()) {
                        if paths.len() == 1 {
                            if let Some(p) = paths[0].as_str() {
                                path_to_inject = Some(p.to_string());
                            }
                        }
                    }
                }

                if let Some(path) = path_to_inject {
                    obj.insert("path".to_string(), json!(path));
                    tracing::debug!(
                        "[Streaming] Probabilistic fix for tool '{}': paths[0] → path(\"{}\")",
                        other,
                        path
                    );
                }
                tracing::debug!(
                    "[Streaming] Unmapped tool call processed via generic rules: {} (keys: {:?})",
                    other,
                    obj.keys()
                );
            }
        }
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

        // Thinking 块结束时发送暂存的签名
        if self.block_type == BlockType::Thinking && self.signatures.has_pending() {
            if let Some(signature) = self.signatures.consume() {
                chunks.push(self.emit_delta("signature_delta", json!({ "signature": signature })));
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
                format!("{}...", &raw_data[..100])
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

        if self.state.current_block_type() != BlockType::Text {
            chunks.extend(
                self.state
                    .start_block(BlockType::Text, json!({ "type": "text", "text": "" })),
            );
        }

        chunks.push(self.state.emit_delta("text_delta", json!({ "text": text })));

        chunks
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

        let mut tool_name = fc.name.clone();
        if tool_name.to_lowercase() == "search" {
            tool_name = "grep".to_string();
            tracing::debug!("[Streaming] Normalizing tool name: Search → grep");
        }

        // [FIX #MCP] MCP tool name fuzzy matching
        // Gemini often hallucinates incorrect MCP tool names, e.g.:
        //   "mcp__puppeteer_navigate" instead of "mcp__puppeteer__puppeteer_navigate"
        // We attempt to find the closest registered tool name.
        if tool_name.starts_with("mcp__") && !self.state.registered_tool_names.is_empty() {
            if !self.state.registered_tool_names.contains(&tool_name) {
                if let Some(matched) =
                    fuzzy_match_mcp_tool(&tool_name, &self.state.registered_tool_names)
                {
                    tracing::warn!(
                        "[FIX #MCP] Corrected MCP tool name: '{}' → '{}'",
                        tool_name,
                        matched
                    );
                    tool_name = matched;
                } else {
                    tracing::warn!(
                        "[FIX #MCP] No fuzzy match found for MCP tool '{}'. Passing as-is.",
                        tool_name
                    );
                }
            }
        }

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
        // [FIX] Remap args before serialization for Gemini → Claude compatibility
        if let Some(args) = &fc.args {
            let mut remapped_args = args.clone();

            let tool_name_title = fc.name.clone();
            // [OPTIMIZED] Only rename if it's "search" which is a known hallucination.
            // Avoid renaming "grep" to "Grep" if possible to protect signature,
            // unless we're sure Grep is the standard.
            let mut final_tool_name = tool_name_title;
            if final_tool_name.to_lowercase() == "search" {
                final_tool_name = "Grep".to_string();
            }
            remap_function_call_args(&final_tool_name, &mut remapped_args);

            let json_str =
                serde_json::to_string(&remapped_args).unwrap_or_else(|_| "{}".to_string());
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

/// [FIX #MCP] Fuzzy match an incorrect MCP tool name against registered tool names.
///
/// MCP tool naming convention: `mcp__<server_name>__<tool_name>`
/// Gemini often hallucinates by:
///   1. Dropping the server prefix: `mcp__navigate` → should be `mcp__puppeteer__puppeteer_navigate`
///   2. Merging server+tool: `mcp__puppeteer_navigate` → should be `mcp__puppeteer__puppeteer_navigate`
///   3. Partial name: `mcp__pup_navigate` → should be `mcp__puppeteer__puppeteer_navigate`
///
/// Strategy (in priority order):
///   1. Exact suffix match: if the hallucinated name's suffix exactly matches a registered tool's suffix
///   2. Suffix contained: if the hallucinated name (without `mcp__`) is contained in a registered tool name
///   3. Longest common subsequence scoring: picks the registered tool with the best LCS ratio
fn fuzzy_match_mcp_tool(hallucinated: &str, registered: &[String]) -> Option<String> {
    let mcp_tools: Vec<&String> = registered
        .iter()
        .filter(|name| name.starts_with("mcp__"))
        .collect();

    if mcp_tools.is_empty() {
        return None;
    }

    // Extract the part after "mcp__" for the hallucinated name
    let hallucinated_suffix = &hallucinated[5..]; // skip "mcp__"

    // Strategy 1: Exact suffix match
    // e.g., hallucinated = "mcp__puppeteer_navigate", registered = "mcp__puppeteer__puppeteer_navigate"
    // Check if any registered tool ends with the hallucinated suffix after `__`
    for tool in &mcp_tools {
        // For registered tool "mcp__server__tool_name", extract "tool_name"
        if let Some(last_sep) = tool.rfind("__") {
            let tool_suffix = &tool[last_sep + 2..];
            if hallucinated_suffix == tool_suffix {
                return Some(tool.to_string());
            }
        }
    }

    // Strategy 2: Suffix contained match
    // e.g., hallucinated = "mcp__puppeteer_navigate", check if "puppeteer_navigate" is a substring
    // of any registered tool's full name
    let mut contained_matches: Vec<(&String, usize)> = Vec::new();
    for tool in &mcp_tools {
        let tool_lower = tool.to_lowercase();
        let hall_lower = hallucinated_suffix.to_lowercase();
        if tool_lower.contains(&hall_lower) {
            contained_matches.push((tool, tool.len()));
        }
    }
    // Pick the shortest match (most specific)
    if !contained_matches.is_empty() {
        contained_matches.sort_by_key(|(_, len)| *len);
        return Some(contained_matches[0].0.to_string());
    }

    // Strategy 3: Normalized token overlap scoring
    // Split both names into tokens by '_' and '__', compute overlap ratio
    let hall_tokens: Vec<&str> = hallucinated_suffix
        .split(|c: char| c == '_')
        .filter(|s| !s.is_empty())
        .collect();

    if hall_tokens.is_empty() {
        return None;
    }

    let mut best_match: Option<String> = None;
    let mut best_score: f64 = 0.0;
    let threshold = 0.4; // Minimum overlap ratio to consider a match

    for tool in &mcp_tools {
        let tool_after_mcp = &tool[5..]; // skip "mcp__"
        let tool_tokens: Vec<&str> = tool_after_mcp
            .split(|c: char| c == '_')
            .filter(|s| !s.is_empty())
            .collect();

        if tool_tokens.is_empty() {
            continue;
        }

        // Count matching tokens
        let mut matches = 0;
        for ht in &hall_tokens {
            if tool_tokens.iter().any(|tt| tt.eq_ignore_ascii_case(ht)) {
                matches += 1;
            }
        }

        // Score = matching tokens / max(hall_tokens, tool_tokens)
        let max_len = hall_tokens.len().max(tool_tokens.len()) as f64;
        let score = matches as f64 / max_len;

        if score > best_score {
            best_score = score;
            best_match = Some(tool.to_string());
        }
    }

    if best_score >= threshold {
        tracing::debug!(
            "[FIX #MCP] Fuzzy match score for '{}': {:.2} -> {:?}",
            hallucinated,
            best_score,
            best_match
        );
        best_match
    } else {
        None
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

    #[test]
    fn test_fuzzy_match_mcp_tool_exact_suffix() {
        let registered = vec![
            "mcp__puppeteer__puppeteer_navigate".to_string(),
            "mcp__puppeteer__puppeteer_screenshot".to_string(),
            "mcp__filesystem__read_file".to_string(),
        ];

        // Gemini drops server prefix, produces: mcp__puppeteer_navigate
        // Should match mcp__puppeteer__puppeteer_navigate via suffix "puppeteer_navigate"
        let result = fuzzy_match_mcp_tool("mcp__puppeteer_navigate", &registered);
        assert_eq!(
            result,
            Some("mcp__puppeteer__puppeteer_navigate".to_string())
        );
    }

    #[test]
    fn test_fuzzy_match_mcp_tool_exact_match_no_correction() {
        let registered = vec!["mcp__puppeteer__puppeteer_navigate".to_string()];

        // Already correct - should not be called (the caller checks contains first)
        // But if called, should find it
        let result = fuzzy_match_mcp_tool("mcp__puppeteer__puppeteer_navigate", &registered);
        // It will match via suffix strategy
        assert!(result.is_some());
    }

    #[test]
    fn test_fuzzy_match_mcp_tool_suffix_contained() {
        let registered = vec![
            "mcp__puppeteer__puppeteer_navigate".to_string(),
            "mcp__puppeteer__puppeteer_click".to_string(),
        ];

        // Gemini produces a partial-but-contained name
        let result = fuzzy_match_mcp_tool("mcp__navigate", &registered);
        assert_eq!(
            result,
            Some("mcp__puppeteer__puppeteer_navigate".to_string())
        );
    }

    #[test]
    fn test_fuzzy_match_mcp_tool_token_overlap() {
        let registered = vec![
            "mcp__filesystem__read_file".to_string(),
            "mcp__filesystem__write_file".to_string(),
            "mcp__filesystem__list_directory".to_string(),
        ];

        // Gemini produces: mcp__read_file → should match mcp__filesystem__read_file
        let result = fuzzy_match_mcp_tool("mcp__read_file", &registered);
        assert_eq!(result, Some("mcp__filesystem__read_file".to_string()));
    }

    #[test]
    fn test_fuzzy_match_mcp_tool_no_match() {
        let registered = vec!["mcp__puppeteer__puppeteer_navigate".to_string()];

        // Completely unrelated name
        let result = fuzzy_match_mcp_tool("mcp__totally_unrelated_xyz", &registered);
        assert_eq!(result, None);
    }

    #[test]
    fn test_fuzzy_match_mcp_tool_no_mcp_tools() {
        let registered = vec!["regular_tool".to_string(), "another_tool".to_string()];

        // No MCP tools in registry
        let result = fuzzy_match_mcp_tool("mcp__puppeteer_navigate", &registered);
        assert_eq!(result, None);
    }

    #[test]
    fn test_fuzzy_match_mcp_tool_screenshot() {
        let registered = vec![
            "mcp__puppeteer__puppeteer_navigate".to_string(),
            "mcp__puppeteer__puppeteer_screenshot".to_string(),
            "mcp__puppeteer__puppeteer_click".to_string(),
        ];

        let result = fuzzy_match_mcp_tool("mcp__puppeteer_screenshot", &registered);
        assert_eq!(
            result,
            Some("mcp__puppeteer__puppeteer_screenshot".to_string())
        );
    }
}
