// Claude 非流式响应转换 (Gemini → Claude)
// 对应 NonStreamingProcessor

use super::models::*;
use super::utils::to_claude_usage;
use serde_json::json;

/// Known parameter remappings for Gemini → Claude compatibility
/// [FIX] Gemini sometimes uses different parameter names than specified in tool schema
fn remap_function_call_args(tool_name: &str, args: &mut serde_json::Value) {
    // [DEBUG] Always log incoming tool usage for diagnosis
    if let Some(obj) = args.as_object() {
        tracing::debug!("[Response] Tool Call: '{}' Args: {:?}", tool_name, obj);
    }

    if let Some(obj) = args.as_object_mut() {
        // [IMPROVED] Case-insensitive matching for tool names
        // [IMPROVED] Case-insensitive matching for tool names
        match tool_name.to_lowercase().as_str() {
            "grep" | "search" | "search_code_definitions" | "search_code_snippets" => {
                // [FIX] Gemini hallucination: maps parameter description to "description" field
                if let Some(desc) = obj.remove("description") {
                    if !obj.contains_key("pattern") {
                        obj.insert("pattern".to_string(), desc);
                        tracing::debug!("[Response] Remapped Grep: description → pattern");
                    }
                }

                // Gemini uses "query", Claude Code expects "pattern"
                if let Some(query) = obj.remove("query") {
                    if !obj.contains_key("pattern") {
                        obj.insert("pattern".to_string(), query);
                        tracing::debug!("[Response] Remapped Grep: query → pattern");
                    }
                }

                // [CRITICAL FIX] Claude Code uses "path" (string), NOT "paths" (array)!
                if !obj.contains_key("path") {
                    if let Some(paths) = obj.remove("paths") {
                        let path_str = if let Some(arr) = paths.as_array() {
                            arr.get(0).and_then(|v| v.as_str()).unwrap_or(".").to_string()
                        } else if let Some(s) = paths.as_str() {
                            s.to_string()
                        } else {
                            ".".to_string()
                        };
                        obj.insert("path".to_string(), serde_json::json!(path_str));
                        tracing::debug!("[Response] Remapped Grep: paths → path(\"{}\")", path_str);
                    } else {
                        // Default to current directory if missing
                        obj.insert("path".to_string(), json!("."));
                        tracing::debug!("[Response] Added default path: \".\"");
                    }
                }

                // Note: We keep "-n" and "output_mode" if present as they are valid in Grep schema
            },
            "glob" => {
                // [FIX] Gemini hallucination: maps parameter description to "description" field
                if let Some(desc) = obj.remove("description") {
                    if !obj.contains_key("pattern") {
                        obj.insert("pattern".to_string(), desc);
                        tracing::debug!("[Response] Remapped Glob: description → pattern");
                    }
                }

                // Gemini uses "query", Claude Code expects "pattern"
                if let Some(query) = obj.remove("query") {
                    if !obj.contains_key("pattern") {
                        obj.insert("pattern".to_string(), query);
                        tracing::debug!("[Response] Remapped Glob: query → pattern");
                    }
                }

                // [CRITICAL FIX] Claude Code uses "path" (string), NOT "paths" (array)!
                if !obj.contains_key("path") {
                    if let Some(paths) = obj.remove("paths") {
                        let path_str = if let Some(arr) = paths.as_array() {
                            arr.get(0).and_then(|v| v.as_str()).unwrap_or(".").to_string()
                        } else if let Some(s) = paths.as_str() {
                            s.to_string()
                        } else {
                            ".".to_string()
                        };
                        obj.insert("path".to_string(), serde_json::json!(path_str));
                        tracing::debug!("[Response] Remapped Glob: paths → path(\"{}\")", path_str);
                    } else {
                        // Default to current directory if missing
                        obj.insert("path".to_string(), json!("."));
                        tracing::debug!("[Response] Added default path: \".\"");
                    }
                }
            },
            "read" => {
                // Gemini might use "path" vs "file_path"
                if let Some(path) = obj.remove("path") {
                    if !obj.contains_key("file_path") {
                        obj.insert("file_path".to_string(), path);
                        tracing::debug!("[Response] Remapped Read: path → file_path");
                    }
                }
            },
            "ls" => {
                // LS tool: ensure "path" parameter exists
                if !obj.contains_key("path") {
                    obj.insert("path".to_string(), serde_json::json!("."));
                    tracing::debug!("[Response] Remapped LS: default path → \".\"");
                }
            },
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
                    obj.insert("path".to_string(), serde_json::json!(path));
                    tracing::debug!(
                        "[Response] Probabilistic fix for tool '{}': paths[0] → path(\"{}\")",
                        other,
                        path
                    );
                }
                tracing::debug!(
                    "[Response] Unmapped tool call processed via generic rules: {} (keys: {:?})",
                    other,
                    obj.keys()
                );
            },
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
    pub has_tool_call: bool,
    pub scaling_enabled: bool,
    pub context_limit: u32,
    pub session_id: Option<String>,
    pub model_name: String,
    pub message_count: usize, // [NEW v4.0.0] Message count for rewind detection
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
        }
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
                        },
                        Err(_) => sig.clone(), // Not valid UTF-8, keep as is
                    }
                },
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
                format!("{}-{}", fc.name, crate::proxy::common::utils::generate_random_id())
            });

            let mut tool_name = fc.name.clone();
            // [OPTIMIZED] Only rename if it's "search" which is a known hallucination.
            // Avoid renaming "grep" to "Grep" if possible to protect signature.
            if tool_name.to_lowercase() == "search" {
                tool_name = "Grep".to_string();
            }

            // [FIX] Remap args for Gemini → Claude compatibility
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
            self.content_blocks.push(ContentBlock::Text { text: current_text });
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
) -> Result<ClaudeResponse, String> {
    let mut processor = NonStreamingProcessor::new(session_id, model_name, message_count);
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
        );
        assert!(result.is_ok());

        let claude_resp = result.unwrap();
        assert_eq!(claude_resp.role, "assistant");
        assert_eq!(claude_resp.stop_reason, "end_turn");
        assert_eq!(claude_resp.content.len(), 1);

        match &claude_resp.content[0] {
            ContentBlock::Text { text } => {
                assert_eq!(text, "Hello, world!");
            },
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
        );
        assert!(result.is_ok());

        let claude_resp = result.unwrap();
        assert_eq!(claude_resp.content.len(), 2);

        match &claude_resp.content[0] {
            ContentBlock::Thinking { thinking, signature, .. } => {
                assert_eq!(thinking, "Let me think...");
                assert_eq!(signature.as_deref(), Some("sig123"));
            },
            _ => panic!("Expected Thinking block"),
        }

        match &claude_resp.content[1] {
            ContentBlock::Text { text } => {
                assert_eq!(text, "The answer is 42");
            },
            _ => panic!("Expected Text block"),
        }
    }
}
