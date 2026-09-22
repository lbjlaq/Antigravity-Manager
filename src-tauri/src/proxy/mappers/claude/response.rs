// Claude 非流式响应转换 (Gemini → Claude)
// 对应 NonStreamingProcessor

use super::models::*;
use super::utils::to_claude_usage;

/// Passthrough tool arguments for Gemini → Claude compatibility
fn remap_function_call_args(tool_name: &str, args: &mut serde_json::Value) {
    // 纯透传协议工具参数，不进行任何字段重命名与拦截改写
    if let Some(obj) = args.as_object() {
        tracing::debug!(
            "[Response] Tool Call (Passthrough): '{}' Args: {:?}",
            tool_name,
            obj
        );
    }
}

/// 非流式响应处理器
pub struct NonStreamingProcessor {
    content_blocks: Vec<ContentBlock>,
    text_builder: String,
    thinking_builder: String,
    thinking_signature: Option<String>,
    trailing_signature: Option<String>,
    pub has_tool_call: bool,
    pub scaling_enabled: bool,
    pub context_limit: u32,
    pub session_id: Option<String>,
    pub model_name: String,
    pub message_count: usize, // [NEW v4.0.0] Message count for rewind detection
    // [FIX #3379] Registered tool names for call:default_api leakage detection
    pub registered_tool_names: Vec<String>,
}

impl NonStreamingProcessor {
    pub fn new(session_id: Option<String>, model_name: String, message_count: usize) -> Self {
        Self {
            content_blocks: Vec::new(),
            text_builder: String::new(),
            thinking_builder: String::new(),
            thinking_signature: None,
            trailing_signature: None,
            has_tool_call: false,
            scaling_enabled: false,
            context_limit: 1_048_576, // Default to 1M
            session_id,
            model_name,
            message_count,
            registered_tool_names: Vec::new(),
        }
    }

    // [FIX #3379] Set registered tool names for leakage detection
    pub fn set_registered_tool_names(&mut self, names: Vec<String>) {
        self.registered_tool_names = names;
    }

    /// 处理 Gemini 响应并转换为 Claude 响应
    pub fn process(
        &mut self,
        gemini_response: &GeminiResponse,
        scaling_enabled: bool,
        context_limit: u32,
    ) -> ClaudeResponse {
        self.scaling_enabled = scaling_enabled;
        self.context_limit = context_limit;
        // 获取 parts
        let empty_parts = vec![];
        let parts = gemini_response
            .candidates
            .as_ref()
            .and_then(|c| c.get(0))
            .and_then(|candidate| candidate.content.as_ref())
            .map(|content| &content.parts)
            .unwrap_or(&empty_parts);

        // 处理所有 parts
        for part in parts {
            self.process_part(part);
        }

        // 处理 grounding(web search) -> 转换为 server_tool_use / web_search_tool_result
        if let Some(candidate) = gemini_response.candidates.as_ref().and_then(|c| c.get(0)) {
            if let Some(grounding) = &candidate.grounding_metadata {
                self.process_grounding(grounding);
            }
        }

        // 刷新剩余内容
        self.flush_thinking();
        self.flush_text();

        // 处理 trailingSignature (空 text 带签名)
        if let Some(signature) = self.trailing_signature.take() {
            self.content_blocks.push(ContentBlock::Thinking {
                thinking: String::new(),
                signature: Some(signature),
                cache_control: None,
            });
        }

        // 构建响应
        self.build_response(gemini_response)
    }

    /// 处理单个 part
    fn process_part(&mut self, part: &GeminiPart) {
        let signature = part.thought_signature.as_ref().map(|sig| {
            use base64::Engine;
            match base64::engine::general_purpose::STANDARD.decode(sig) {
                Ok(decoded_bytes) => {
                    match String::from_utf8(decoded_bytes) {
                        Ok(decoded_str) => {
                            tracing::debug!(
                                "[Response] Decoded base64 signature (len {} -> {})",
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

        // [FIX #765] Cache signature in NonStreamingProcessor
        if let Some(sig) = &signature {
            if let Some(s_id) = &self.session_id {
                crate::proxy::SignatureCache::global().cache_session_signature(
                    s_id,
                    sig.to_string(),
                    self.message_count,
                );
                crate::proxy::SignatureCache::global()
                    .cache_thinking_family(sig.to_string(), self.model_name.clone());
                tracing::debug!(
                    "[Claude-Response] Cached signature (len: {}) for session: {}",
                    sig.len(),
                    s_id
                );
            }
        }

        // 1. FunctionCall 处理
        if let Some(fc) = &part.function_call {
            self.flush_thinking();
            self.flush_text();

            // 处理 trailingSignature (B4/C3 场景)
            if let Some(trailing_sig) = self.trailing_signature.take() {
                self.content_blocks.push(ContentBlock::Thinking {
                    thinking: String::new(),
                    signature: Some(trailing_sig),
                    cache_control: None,
                });
            }

            self.has_tool_call = true;

            // 生成 tool_use id
            let tool_id = fc.id.clone().unwrap_or_else(|| {
                format!(
                    "{}-{}",
                    fc.name,
                    crate::proxy::common::utils::generate_random_id()
                )
            });

            let tool_name = fc.name.clone();

            // 纯透传协议参数
            let mut args = fc.args.clone().unwrap_or(serde_json::json!({}));
            remap_function_call_args(&tool_name, &mut args);

            let mut tool_use = ContentBlock::ToolUse {
                id: tool_id,
                name: tool_name,
                input: args.clone(),
                signature: None,
                cache_control: None,
            };

            // 只使用 FC 自己的签名
            if let ContentBlock::ToolUse { signature: sig, .. } = &mut tool_use {
                *sig = signature;
            }

            self.content_blocks.push(tool_use);
            return;
        }

        // 2. Text 处理
        if let Some(text) = &part.text {
            if part.thought.unwrap_or(false) {
                // Thinking part
                self.flush_text();

                // 处理 trailingSignature
                if let Some(trailing_sig) = self.trailing_signature.take() {
                    self.flush_thinking();
                    self.content_blocks.push(ContentBlock::Thinking {
                        thinking: String::new(),
                        signature: Some(trailing_sig),
                        cache_control: None,
                    });
                }

                self.thinking_builder.push_str(text);
                if signature.is_some() {
                    self.thinking_signature = signature;
                }
            } else {
                // 普通 Text
                if text.is_empty() {
                    // 空 text 带签名 - 暂存到 trailingSignature
                    if signature.is_some() {
                        self.trailing_signature = signature;
                    }
                    return;
                }

                self.flush_thinking();

                // 处理之前的 trailingSignature
                if let Some(trailing_sig) = self.trailing_signature.take() {
                    self.flush_text();
                    self.content_blocks.push(ContentBlock::Thinking {
                        thinking: String::new(),
                        signature: Some(trailing_sig),
                        cache_control: None,
                    });
                }

                self.text_builder.push_str(text);

                // 非空 text 带签名 - 立即刷新并输出空 thinking 块
                if let Some(sig) = signature {
                    self.flush_text();
                    self.content_blocks.push(ContentBlock::Thinking {
                        thinking: String::new(),
                        signature: Some(sig),
                        cache_control: None,
                    });
                }
            }
        }

        // 3. InlineData (Image) 处理
        if let Some(img) = &part.inline_data {
            self.flush_thinking();

            let mime_type = &img.mime_type;
            let data = &img.data;
            if !data.is_empty() {
                let markdown_img = format!("![image](data:{};base64,{})", mime_type, data);
                self.text_builder.push_str(&markdown_img);
                self.flush_text();
            }
        }
    }

    /// 处理 Grounding 元数据 (Web Search 结果)
    fn process_grounding(&mut self, grounding: &GroundingMetadata) {
        let mut grounding_text = String::new();

        // 1. 处理搜索词
        if let Some(queries) = &grounding.web_search_queries {
            if !queries.is_empty() {
                grounding_text.push_str("\n\n---\n**🔍 已为您搜索：** ");
                grounding_text.push_str(&queries.join(", "));
            }
        }

        // 2. 处理来源链接 (Chunks)
        if let Some(chunks) = &grounding.grounding_chunks {
            let mut links = Vec::new();
            for (i, chunk) in chunks.iter().enumerate() {
                if let Some(web) = &chunk.web {
                    let title = web.title.as_deref().unwrap_or("网页来源");
                    let uri = web.uri.as_deref().unwrap_or("#");
                    links.push(format!("[{}] [{}]({})", i + 1, title, uri));
                }
            }

            if !links.is_empty() {
                grounding_text.push_str("\n\n**🌐 来源引文：**\n");
                grounding_text.push_str(&links.join("\n"));
            }
        }

        if !grounding_text.is_empty() {
            // 在常规内容前后刷新并插入文本
            self.flush_thinking();
            self.flush_text();
            self.text_builder.push_str(&grounding_text);
            self.flush_text();
        }
    }

    /// 刷新 text builder
    fn flush_text(&mut self) {
        if self.text_builder.is_empty() {
            return;
        }

        let mut current_text = self.text_builder.clone();
        self.text_builder.clear();

        // [FIX #3379] call:default_api:* leakage detection (non-streaming path)
        // Symmetric to streaming.rs try_recover_call_default_api_text.
        // Guards applied: G1 (tools registered), G3 (strict prefix), G4 (whitelist),
        //                 G5 (no surrounding prose), G6 (valid JSON args).
        // G2/G7 not needed in non-streaming path since we process the full response at once.
        if !self.registered_tool_names.is_empty() {
            let trimmed = current_text.trim();
            if trimmed.starts_with("call:default_api:") {
                const PREFIX: &str = "call:default_api:";
                let rest = &trimmed[PREFIX.len()..];
                let tool_end = rest.find(|c| c == '{' || c == '(').unwrap_or(rest.len());
                let tool_name = rest[..tool_end].trim();
                let args_str = rest[tool_end..].trim();

                if !tool_name.is_empty() {
                    // G5: no surrounding prose — text must be only the expression
                    let after_name = trimmed[PREFIX.len() + tool_name.len()..].trim();
                    let g5_ok = after_name.is_empty()
                        || after_name.starts_with('{')
                        || after_name.starts_with('(');

                    if g5_ok {
                        // G4: whitelist check
                        let matched = self
                            .registered_tool_names
                            .iter()
                            .find(|n| n.eq_ignore_ascii_case(tool_name))
                            .cloned();

                        if let Some(matched_name) = matched {
                            // G6: parse args
                            let parsed = if args_str.is_empty() {
                                Some(serde_json::json!({}))
                            } else {
                                serde_json::from_str::<serde_json::Value>(args_str)
                                    .ok()
                                    .filter(|v| v.is_object())
                            };

                            if let Some(parsed_args) = parsed {
                                tracing::warn!(
                                    "[#3379-RECOVERY] Non-streaming: recovering call:default_api:{} as tool_use",
                                    matched_name
                                );
                                let tool_id = format!(
                                    "{}-3379-{}",
                                    matched_name,
                                    crate::proxy::common::utils::generate_random_id()
                                );
                                self.content_blocks.push(ContentBlock::ToolUse {
                                    id: tool_id,
                                    name: matched_name,
                                    input: parsed_args,
                                    signature: None,
                                    cache_control: None,
                                });
                                self.has_tool_call = true;
                                // current_text fully consumed as a tool_use — nothing else to emit
                                return;
                            } else {
                                tracing::warn!(
                                    "[#3379] Non-streaming: call:default_api:{} detected but args parse failed; emitting as text",
                                    tool_name
                                );
                            }
                        } else {
                            tracing::warn!(
                                "[#3379] Non-streaming: call:default_api:{} detected but tool not in whitelist; emitting as text",
                                tool_name
                            );
                        }
                    } else {
                        tracing::warn!(
                            "[#3379] Non-streaming: call:default_api:{} detected but G5 (no prose) failed; emitting as text",
                            tool_name
                        );
                    }
                }
            }
        }

        // [NEW] MCP XML Bridge: 循环解析文本中可能存在的 XML 标签
        while let Some(start_idx) = current_text.find("<mcp__") {
            if let Some(tag_end_idx) = current_text[start_idx..].find('>') {
                let actual_tag_end = start_idx + tag_end_idx;
                let tool_name = &current_text[start_idx + 1..actual_tag_end];
                let end_tag = format!("</{}>", tool_name);

                if let Some(close_idx) = current_text.find(&end_tag) {
                    // 1. 处理标签前的文本
                    if start_idx > 0 {
                        self.content_blocks.push(ContentBlock::Text {
                            text: current_text[..start_idx].to_string(),
                        });
                    }

                    // 2. 解析 XML 内容并转换为 ToolUse
                    let input_str = &current_text[actual_tag_end + 1..close_idx];
                    let input_json: serde_json::Value = serde_json::from_str(input_str.trim())
                        .unwrap_or_else(|_| serde_json::json!({ "input": input_str.trim() }));

                    self.content_blocks.push(ContentBlock::ToolUse {
                        id: format!("{}-xml", tool_name),
                        name: tool_name.to_string(),
                        input: input_json,
                        signature: None,
                        cache_control: None,
                    });
                    self.has_tool_call = true;

                    // 3. 继续处理剩余文本
                    current_text = current_text[close_idx + end_tag.len()..].to_string();
                    continue;
                }
            }
            // 如果 XML 格式不完整, 退出循环并按普通文本处理
            break;
        }

        if !current_text.is_empty() {
            self.content_blocks
                .push(ContentBlock::Text { text: current_text });
        }
    }

    /// 刷新 thinking builder
    fn flush_thinking(&mut self) {
        // 如果既没有内容也没有签名，直接返回
        if self.thinking_builder.is_empty() && self.thinking_signature.is_none() {
            return;
        }

        let thinking = self.thinking_builder.clone();
        let signature = self.thinking_signature.take();

        self.content_blocks.push(ContentBlock::Thinking {
            thinking,
            signature,
            cache_control: None,
        });
        self.thinking_builder.clear();
    }

    /// 构建最终响应
    fn build_response(&self, gemini_response: &GeminiResponse) -> ClaudeResponse {
        let finish_reason = gemini_response
            .candidates
            .as_ref()
            .and_then(|c| c.get(0))
            .and_then(|candidate| candidate.finish_reason.as_deref());

        let stop_reason = if self.has_tool_call {
            "tool_use"
        } else if finish_reason == Some("MAX_TOKENS") {
            "max_tokens"
        } else {
            "end_turn"
        };

        let usage = gemini_response
            .usage_metadata
            .as_ref()
            .map(|u| to_claude_usage(u, self.scaling_enabled, self.context_limit))
            .unwrap_or(Usage {
                input_tokens: 0,
                output_tokens: 0,
                cache_read_input_tokens: None,
                cache_creation_input_tokens: None,
                server_tool_use: None,
            });

        ClaudeResponse {
            id: gemini_response.response_id.clone().unwrap_or_else(|| {
                format!("msg_{}", crate::proxy::common::utils::generate_random_id())
            }),
            type_: "message".to_string(),
            role: "assistant".to_string(),
            model: gemini_response.model_version.clone().unwrap_or_default(),
            content: self.content_blocks.clone(),
            stop_reason: stop_reason.to_string(),
            stop_sequence: None,
            usage,
        }
    }
}

pub fn transform_response(
    gemini_response: &GeminiResponse,
    scaling_enabled: bool,
    context_limit: u32,
    session_id: Option<String>,
    model_name: String,
    message_count: usize, // [NEW v4.0.0] Message count for rewind detection
    registered_tool_names: Vec<String>, // [FIX #3379] For call:default_api leakage recovery
) -> Result<ClaudeResponse, String> {
    let mut processor = NonStreamingProcessor::new(session_id, model_name, message_count);
    processor.set_registered_tool_names(registered_tool_names);
    Ok(processor.process(gemini_response, scaling_enabled, context_limit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_text_response() {
        let gemini_resp = GeminiResponse {
            candidates: Some(vec![Candidate {
                content: Some(GeminiContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart {
                        text: Some("Hello, world!".to_string()),
                        thought: None,
                        thought_signature: None,
                        function_call: None,
                        function_response: None,
                        inline_data: None,
                    }],
                }),
                finish_reason: Some("STOP".to_string()),
                index: Some(0),
                grounding_metadata: None,
            }]),
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: Some(10),
                candidates_token_count: Some(5),
                total_token_count: Some(15),
                cached_content_token_count: None,
            }),
            model_version: Some("gemini-2.5-flash".to_string()),
            response_id: Some("resp_123".to_string()),
        };

        let result = transform_response(
            &gemini_resp,
            false,
            1_000_000,
            None,
            "gemini-2.5-flash".to_string(),
            1,
            vec![], // registered_tool_names: not needed for this test
        );
        assert!(result.is_ok());

        let claude_resp = result.unwrap();
        assert_eq!(claude_resp.role, "assistant");
        assert_eq!(claude_resp.stop_reason, "end_turn");
        assert_eq!(claude_resp.content.len(), 1);

        match &claude_resp.content[0] {
            ContentBlock::Text { text } => {
                assert_eq!(text, "Hello, world!");
            }
            _ => panic!("Expected Text block"),
        }
    }

    #[test]
    fn test_thinking_with_signature() {
        let gemini_resp = GeminiResponse {
            candidates: Some(vec![Candidate {
                content: Some(GeminiContent {
                    role: "model".to_string(),
                    parts: vec![
                        GeminiPart {
                            text: Some("Let me think...".to_string()),
                            thought: Some(true),
                            thought_signature: Some("sig123".to_string()),
                            function_call: None,
                            function_response: None,
                            inline_data: None,
                        },
                        GeminiPart {
                            text: Some("The answer is 42".to_string()),
                            thought: None,
                            thought_signature: None,
                            function_call: None,
                            function_response: None,
                            inline_data: None,
                        },
                    ],
                }),
                finish_reason: Some("STOP".to_string()),
                index: Some(0),
                grounding_metadata: None,
            }]),
            usage_metadata: None,
            model_version: Some("gemini-2.5-flash".to_string()),
            response_id: Some("resp_456".to_string()),
        };

        let result = transform_response(
            &gemini_resp,
            false,
            1_000_000,
            None,
            "gemini-2.5-flash".to_string(),
            1,
            vec![], // registered_tool_names: not needed for this test
        );
        assert!(result.is_ok());

        let claude_resp = result.unwrap();
        assert_eq!(claude_resp.content.len(), 2);

        match &claude_resp.content[0] {
            ContentBlock::Thinking {
                thinking,
                signature,
                ..
            } => {
                assert_eq!(thinking, "Let me think...");
                assert_eq!(signature.as_deref(), Some("sig123"));
            }
            _ => panic!("Expected Thinking block"),
        }

        match &claude_resp.content[1] {
            ContentBlock::Text { text } => {
                assert_eq!(text, "The answer is 42");
            }
            _ => panic!("Expected Text block"),
        }
    }
}
