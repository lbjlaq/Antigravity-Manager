//! Streaming state machine for Claude SSE conversion.
//!
//! This module contains the StreamingState state machine that tracks
//! the current state of SSE streaming and manages content blocks.

use bytes::Bytes;
use serde_json::{json, Value};

use crate::proxy::mappers::claude::models::*;
use crate::proxy::mappers::claude::utils::to_claude_usage;
use crate::proxy::mappers::estimation_calibrator::get_calibrator;

/// Block type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockType {
    None,
    Text,
    Thinking,
    Function,
}

/// Signature manager for handling thought signatures.
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

/// Streaming state machine.
pub struct StreamingState {
    block_type: BlockType,
    pub block_index: usize,
    pub message_start_sent: bool,
    pub message_stop_sent: bool,
    used_tool: bool,
    signatures: SignatureManager,
    pub trailing_signature: Option<String>,
    pub web_search_query: Option<String>,
    pub grounding_chunks: Option<Vec<Value>>,
    // Error recovery state tracking
    #[allow(dead_code)]
    parse_error_count: usize,
    #[allow(dead_code)]
    last_valid_state: Option<BlockType>,
    // Model tracking for signature cache
    pub model_name: Option<String>,
    // Session ID for session-based signature caching
    pub session_id: Option<String>,
    // Flag for context usage scaling
    pub scaling_enabled: bool,
    // Context limit for smart threshold recovery (default to 1M)
    pub context_limit: u32,
    // MCP XML Bridge buffer
    pub mcp_xml_buffer: String,
    pub in_mcp_xml: bool,
    // Estimated prompt tokens for calibrator learning
    pub estimated_prompt_tokens: Option<u32>,
    // Post-thinking interruption tracking
    pub has_thinking: bool,
    pub has_content: bool,
    pub message_count: usize,
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
        }
    }

    /// Emit SSE event.
    pub fn emit(&self, event_type: &str, data: Value) -> Bytes {
        let sse = format!(
            "event: {}\ndata: {}\n\n",
            event_type,
            serde_json::to_string(&data).unwrap_or_default()
        );
        Bytes::from(sse)
    }

    /// Emit message_start event.
    pub fn emit_message_start(&mut self, raw_json: &Value) -> Bytes {
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

    /// Start a new content block.
    pub fn start_block(&mut self, block_type: BlockType, content_block: Value) -> Vec<Bytes> {
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

    /// End current content block.
    pub fn end_block(&mut self) -> Vec<Bytes> {
        if self.block_type == BlockType::None {
            return vec![];
        }

        let mut chunks = Vec::new();

        // Send pending signature when thinking block ends
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

    /// Emit delta event.
    pub fn emit_delta(&self, delta_type: &str, delta_content: Value) -> Bytes {
        let mut delta = json!({ "type": delta_type });
        if let Value::Object(map) = delta_content {
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

    /// Emit finish events.
    pub fn emit_finish(
        &mut self,
        finish_reason: Option<&str>,
        usage_metadata: Option<&UsageMetadata>,
    ) -> Vec<Bytes> {
        let mut chunks = Vec::new();

        // Close last block
        chunks.extend(self.end_block());

        // Handle trailingSignature (B4/C3 scenario)
        if let Some(signature) = self.trailing_signature.take() {
            tracing::info!(
                "[Streaming] Captured trailing signature (len: {}), caching for session.",
                signature.len()
            );

            // [FIX] Persist signature to global cache if session_id is available
            if let Some(session_id) = &self.session_id {
                crate::proxy::SignatureCache::global().cache_session_signature(
                    session_id,
                    signature.clone(),
                    self.message_count,
                );
                tracing::info!(
                     "[Streaming] Persisted signature to global cache for session: {} (msg_count={})",
                     session_id,
                     self.message_count
                 );
            }

            self.signatures.store(Some(signature));
        }

        // Handle grounding (web search) -> convert to Markdown text block
        if self.web_search_query.is_some() || self.grounding_chunks.is_some() {
            chunks.extend(self.emit_grounding_block());
        }

        // Determine stop_reason
        let stop_reason = if self.used_tool {
            "tool_use"
        } else if finish_reason == Some("MAX_TOKENS") {
            "max_tokens"
        } else {
            "end_turn"
        };

        let usage = usage_metadata
            .map(|u| {
                // Record actual token usage for calibrator learning
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

    /// Emit grounding block for web search results.
    fn emit_grounding_block(&mut self) -> Vec<Bytes> {
        let mut chunks = Vec::new();
        let mut grounding_text = String::new();

        // Process search query
        if let Some(query) = &self.web_search_query {
            if !query.is_empty() {
                grounding_text.push_str("\n\n---\n**\u{1F50D} Searched: ** ");
                grounding_text.push_str(query);
            }
        }

        // Process source links
        if let Some(grounding_chunks) = &self.grounding_chunks {
            let mut links = Vec::new();
            for (i, chunk) in grounding_chunks.iter().enumerate() {
                if let Some(web) = chunk.get("web") {
                    let title = web
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Web Source");
                    let uri = web.get("uri").and_then(|v| v.as_str()).unwrap_or("#");
                    links.push(format!("[{}] [{}]({})", i + 1, title, uri));
                }
            }

            if !links.is_empty() {
                grounding_text.push_str("\n\n**\u{1F310} Sources:**\n");
                grounding_text.push_str(&links.join("\n"));
            }
        }

        if !grounding_text.is_empty() {
            chunks.push(self.emit(
                "content_block_start",
                json!({
                    "type": "content_block_start",
                    "index": self.block_index,
                    "content_block": { "type": "text", "text": "" }
                }),
            ));
            chunks.push(self.emit_delta("text_delta", json!({ "text": grounding_text })));
            chunks.push(self.emit(
                "content_block_stop",
                json!({ "type": "content_block_stop", "index": self.block_index }),
            ));
            self.block_index += 1;
        }

        chunks
    }

    /// Mark that a tool was used.
    pub fn mark_tool_used(&mut self) {
        self.used_tool = true;
    }

    /// Get current block type.
    pub fn current_block_type(&self) -> BlockType {
        self.block_type
    }

    /// Get current block index.
    pub fn current_block_index(&self) -> usize {
        self.block_index
    }

    /// Store signature.
    pub fn store_signature(&mut self, signature: Option<String>) {
        self.signatures.store(signature);
    }

    /// Set trailing signature.
    pub fn set_trailing_signature(&mut self, signature: Option<String>) {
        self.trailing_signature = signature;
    }

    /// Check if has trailing signature.
    pub fn has_trailing_signature(&self) -> bool {
        self.trailing_signature.is_some()
    }

    /// Handle SSE parse error with graceful degradation.
    #[allow(dead_code)]
    pub fn handle_parse_error(&mut self, raw_data: &str) -> Vec<Bytes> {
        let mut chunks = Vec::new();

        self.parse_error_count += 1;

        tracing::warn!(
            "[SSE-Parser] Parse error #{} occurred. Raw data length: {} bytes",
            self.parse_error_count,
            raw_data.len()
        );

        // Safely close current block
        if self.block_type != BlockType::None {
            self.last_valid_state = Some(self.block_type);
            chunks.extend(self.end_block());
        }

        // Debug mode: output detailed error info
        #[cfg(debug_assertions)]
        {
            let preview = if raw_data.len() > 100 {
                format!("{}...", &raw_data[..100])
            } else {
                raw_data.to_string()
            };
            tracing::debug!("[SSE-Parser] Failed chunk preview: {}", preview);
        }

        // High error rate: emit warning and error signal
        if self.parse_error_count > 3 {
            tracing::error!(
                "[SSE-Parser] High error rate detected ({} errors). Stream may be corrupted.",
                self.parse_error_count
            );

            chunks.push(self.emit(
                "error",
                json!({
                    "type": "error",
                    "error": {
                        "type": "network_error",
                        "message": "Network connection unstable, please check your network or proxy settings.",
                        "code": "stream_decode_error",
                        "details": {
                            "error_count": self.parse_error_count,
                            "suggestion": "Please try: 1) Check network connection 2) Switch proxy node 3) Retry later"
                        }
                    }
                }),
            ));
        }

        chunks
    }

    /// Reset error state (call after recovery).
    #[allow(dead_code)]
    pub fn reset_error_state(&mut self) {
        self.parse_error_count = 0;
        self.last_valid_state = None;
    }

    /// Get error count (for monitoring).
    #[allow(dead_code)]
    pub fn get_error_count(&self) -> usize {
        self.parse_error_count
    }
}
