// Claude 非流式响应转换 (Gemini → Claude)
// 对应 NonStreamingProcessor

use super::models::*;
use super::utils::to_claude_usage;

/// Known parameter remappings for Gemini → Claude compatibility
/// [FIX] Gemini sometimes uses different parameter names than specified in tool schema
fn remap_function_call_args(tool_name: &str, args: &mut serde_json::Value) {
    if let Some(obj) = args.as_object_mut() {
        match tool_name {
            "Grep" => {
                // Gemini uses "query", Claude Code expects "pattern"
                if let Some(query) = obj.remove("query") {
                    if !obj.contains_key("pattern") {
                        obj.insert("pattern".to_string(), query);
                        tracing::debug!("[Response] Remapped Grep: query → pattern");
                    }
                }
            }
            "Glob" => {
                // Similar remapping if needed
                if let Some(query) = obj.remove("query") {
                    if !obj.contains_key("pattern") {
                        obj.insert("pattern".to_string(), query);
                        tracing::debug!("[Response] Remapped Glob: query → pattern");
                    }
                }
            }
            "Read" => {
                // Gemini might use "path" vs "file_path"
                if let Some(path) = obj.remove("path") {
                    if !obj.contains_key("file_path") {
                        obj.insert("file_path".to_string(), path);
                        tracing::debug!("[Response] Remapped Read: path → file_path");
                    }
                }
            }
            _ => {}
        }
    }
}

/// 非流式响应处理器
pub struct NonStreamingProcessor {
    content_blocks: Vec<ContentBlock>,
    text_builder: String,
    thinking_builder: String,
    thinking_signature: Option<String>,
    trailing_signature: Option<String>,
    has_tool_call: bool,
}

impl NonStreamingProcessor {
    pub fn new() -> Self {
        Self {
            content_blocks: Vec::new(),
            text_builder: String::new(),
            thinking_builder: String::new(),
            thinking_signature: None,
            trailing_signature: None,
            has_tool_call: false,
        }
    }

    /// 处理 Gemini 响应并转换为 Claude 响应
    pub fn process(&mut self, gemini_response: &GeminiResponse) -> ClaudeResponse {
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
        let signature = part.thought_signature.clone();

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

            // [FIX] Remap args for Gemini → Claude compatibility
            let mut args = fc.args.clone().unwrap_or(serde_json::json!({}));
            remap_function_call_args(&fc.name, &mut args);

            let mut tool_use = ContentBlock::ToolUse {
                id: tool_id,
                name: fc.name.clone(),
                input: args,
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

        self.content_blocks.push(ContentBlock::Text {
            text: self.text_builder.clone(),
        });
        self.text_builder.clear();
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
            .map(|u| to_claude_usage(u))
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

/// 转换 Gemini 响应为 Claude 响应 (公共接口)
pub fn transform_response(gemini_response: &GeminiResponse) -> Result<ClaudeResponse, String> {
    let mut processor = NonStreamingProcessor::new();
    Ok(processor.process(gemini_response))
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
            model_version: Some("gemini-2.5-pro".to_string()),
            response_id: Some("resp_123".to_string()),
        };

        let result = transform_response(&gemini_resp);
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
            model_version: Some("gemini-2.5-pro".to_string()),
            response_id: Some("resp_456".to_string()),
        };

        let result = transform_response(&gemini_resp);
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
