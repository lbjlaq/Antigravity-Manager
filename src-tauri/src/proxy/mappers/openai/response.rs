// OpenAI 协议响应转换模块
use super::models::*;
use serde_json::Value;

/// 标准化并清洗 shell / PowerShell / DSH (DeepSeek Harness) 等工具参数
/// 1. 将 cmd / code / script / shell_command / input 等别名重命名为 command
/// 2. [DSH tool-pwsh / tool-bash & WorkBuddy]：
///    - DSH 严格校验 `command` (string) 和 `description` (string) 两个字段必须都存在且非空。
///    - 若模型将实际命令写在 description / text / prompt 中，且 command 缺失，则优先提取恢复为真实 command。
///    - 若 command 存在但缺失 description，则基于 command 自动推导截取生成 description。
///    - 若两者皆无，则填充安全占位命令并保证 description 完整，防止客户端崩溃 (Issue #3430 & #3440)。
/// 3. [DSH tool-workflow]：
///    - DSH 严格校验 `script` (string) 和 `meta` (object with `name` and `description`)。
///    - 若模型返回扁平结构的 `name` / `description`，自动归拢装配进 `meta` 对象中，确保运行期校验通过。
/// 纯透传协议工具参数，不进行任何字段截断、生成或改写
pub fn normalize_and_sanitize_tool_args(tool_name: &str, args: &mut Value) {
    if let Some(obj) = args.as_object() {
        tracing::debug!(
            "[OpenAI] Tool Call (Passthrough): '{}' Args: {:?}",
            tool_name,
            obj
        );
    }
}

pub fn resolve_shell_tool_name(
    model_tool_name: &str,
    _client_tool_names: &std::collections::HashSet<String>,
) -> String {
    // 纯透传工具名称，不进行任何改写
    model_tool_name.to_string()
}

pub fn transform_openai_response(
    gemini_response: &Value,
    session_id: Option<&str>,
    message_count: usize,
    client_tool_names: Option<&std::collections::HashSet<String>>,
) -> OpenAIResponse {
    let empty_set = std::collections::HashSet::new();
    let client_tool_names = client_tool_names.unwrap_or(&empty_set);

    // 解包 response 字段
    let raw = gemini_response.get("response").unwrap_or(gemini_response);

    let mut choices = Vec::new();

    // 支持多候选结果 (n > 1)
    if let Some(candidates) = raw.get("candidates").and_then(|c| c.as_array()) {
        for (idx, candidate) in candidates.iter().enumerate() {
            let mut content_out = String::new();
            let mut thought_out = String::new();
            let mut tool_calls = Vec::new();

            // 提取 content 和 tool_calls
            if let Some(parts) = candidate
                .get("content")
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array())
            {
                for part in parts {
                    // 捕获 thoughtSignature (Gemini 3 工具调用必需)
                    if let Some(sig) = part
                        .get("thoughtSignature")
                        .or(part.get("thought_signature"))
                        .and_then(|s| s.as_str())
                    {
                        if let Some(sid) = session_id {
                            super::streaming::store_thought_signature(sig, sid, message_count);
                        }
                    }

                    // 检查该 part 是否是思考内容 (thought: true)
                    let is_thought_part = part
                        .get("thought")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    // 文本部分
                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                        if is_thought_part {
                            // thought: true 时，text 是思考内容
                            thought_out.push_str(text);
                        } else {
                            // 正常内容
                            content_out.push_str(text);
                        }
                    }

                    // 工具调用部分
                    if let Some(fc) = part.get("functionCall") {
                        let name = fc.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                        let mut args_json =
                            fc.get("args").unwrap_or(&serde_json::json!({})).clone();

                        // [FIX #1575 & #3430] 标准化并清洗 shell / PowerShell 等工具参数名称与必填字段
                        normalize_and_sanitize_tool_args(name, &mut args_json);

                        let arguments_str = args_json.to_string();
                        let final_name = resolve_shell_tool_name(name, client_tool_names);

                        let id = fc
                            .get("id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| format!("{}-{}", final_name, uuid::Uuid::new_v4()));

                        if let Some(sig) = part
                            .get("thoughtSignature")
                            .or(part.get("thought_signature"))
                            .and_then(|s| s.as_str())
                        {
                            crate::proxy::SignatureCache::global()
                                .cache_tool_signature(&id, sig.to_string());
                        }

                        tool_calls.push(ToolCall {
                            id,
                            r#type: "function".to_string(),
                            function: Some(ToolFunction {
                                name: final_name.to_string(),
                                arguments: arguments_str,
                            }),
                            signature: None,
                            status: None,
                            call_id: None,
                            operation: None,
                        });
                    }

                    // 图片处理 (响应中直接返回图片的情况)
                    if let Some(img) = part.get("inlineData") {
                        let mime_type = img
                            .get("mimeType")
                            .and_then(|v| v.as_str())
                            .unwrap_or("image/png");
                        let data = img.get("data").and_then(|v| v.as_str()).unwrap_or("");
                        if !data.is_empty() {
                            content_out
                                .push_str(&format!("![image](data:{};base64,{})", mime_type, data));
                        }
                    }

                    // 处理原生代码执行 (executableCode)
                    if let Some(exec_code) = part.get("executableCode") {
                        let lang = exec_code
                            .get("language")
                            .and_then(|v| v.as_str())
                            .unwrap_or("python");
                        let code = exec_code.get("code").and_then(|v| v.as_str()).unwrap_or("");
                        if !code.is_empty() {
                            content_out.push_str(&format!(
                                "\n\n```{}\n{}\n```\n",
                                lang.to_lowercase(),
                                code
                            ));
                        }
                    }

                    // 处理代码执行结果 (codeExecutionResult)
                    if let Some(exec_result) = part.get("codeExecutionResult") {
                        let output = exec_result
                            .get("output")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if !output.is_empty() {
                            content_out.push_str(&format!(
                                "\n**Execution Output:**\n```text\n{}\n```\n",
                                output
                            ));
                        }
                    }
                }
                if let Some(sid) = session_id {
                    crate::proxy::thinking_store::capture_gemini_parts(sid, parts);
                }
            }

            // 提取并处理该候选结果的联网搜索引文 (Grounding Metadata)
            if let Some(grounding) = candidate.get("groundingMetadata") {
                let mut grounding_text = String::new();

                // 1. 处理搜索词
                if let Some(queries) = grounding.get("webSearchQueries").and_then(|q| q.as_array())
                {
                    let query_list: Vec<&str> = queries.iter().filter_map(|v| v.as_str()).collect();
                    if !query_list.is_empty() {
                        grounding_text.push_str("\n\n---\n**🔍 已为您搜索：** ");
                        grounding_text.push_str(&query_list.join(", "));
                    }
                }

                // 2. 处理来源链接 (Chunks)
                if let Some(chunks) = grounding.get("groundingChunks").and_then(|c| c.as_array()) {
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

                if !grounding_text.is_empty() {
                    content_out.push_str(&grounding_text);
                }
            }

            // 提取传统的 citationMetadata
            if let Some(citation) = candidate.get("citationMetadata") {
                if let Some(sources) = citation.get("citationSources").and_then(|s| s.as_array()) {
                    let mut links = Vec::new();
                    for (i, source) in sources.iter().enumerate() {
                        if let Some(uri) = source.get("uri").and_then(|v| v.as_str()) {
                            // 由于有时没有 title，直接用 URI 当标题
                            links.push(format!("[{}] [{}]({})", i + 1, uri, uri));
                        }
                    }
                    if !links.is_empty() {
                        content_out.push_str("\n\n**📚 引用来源：**\n");
                        content_out.push_str(&links.join("\n"));
                    }
                }
            }

            let raw_finish_reason = candidate.get("finishReason").and_then(|f| f.as_str());
            let is_malformed_function_call = raw_finish_reason == Some("MALFORMED_FUNCTION_CALL");

            let finish_reason = raw_finish_reason
                .map(|f| match f {
                    "STOP" => "stop",
                    "MAX_TOKENS" => "length",
                    "SAFETY" => "content_filter",
                    "RECITATION" => "content_filter",
                    "MALFORMED_FUNCTION_CALL" => "stop",
                    _ => "stop",
                })
                .unwrap_or("stop");

            let refusal_val = if finish_reason == "content_filter" {
                Some("生成由于安全策略或背诵保护被中止".to_string())
            } else {
                None
            };

            // [FIX MALFORMED_FUNCTION_CALL] 避免客户端空白
            if is_malformed_function_call && content_out.is_empty() {
                content_out.push_str("很抱歉，当前模型在尝试调取实时信息时遇到了格式异常。若需要查询实时天气或最新资讯，请尝试使用联网模式（模型名带 -online 后缀）或配置天气/搜索插件。");
            }

            choices.push(Choice {
                index: idx as u32,
                message: OpenAIMessage {
                    role: "assistant".to_string(),
                    content: if content_out.is_empty() {
                        None
                    } else {
                        Some(OpenAIContent::String(content_out))
                    },
                    reasoning_content: if thought_out.is_empty() {
                        None
                    } else {
                        Some(thought_out)
                    },
                    signature: None,
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    tool_call_id: None,
                    name: None,
                    refusal: refusal_val,
                },
                finish_reason: Some(finish_reason.to_string()),
            });
        }
    }

    // 如果 candidates 为空，但存在 promptFeedback（被安全拦截），伪造一个被拒绝的 choice
    if choices.is_empty() {
        if let Some(feedback) = raw.get("promptFeedback") {
            let reason = feedback
                .get("blockReason")
                .and_then(|v| v.as_str())
                .unwrap_or("UNKNOWN");
            let refusal_msg = format!("请求由于安全策略被拦截 (blockReason: {})", reason);
            choices.push(Choice {
                index: 0,
                message: OpenAIMessage {
                    role: "assistant".to_string(),
                    content: None,
                    reasoning_content: None,
                    signature: None,
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                    refusal: Some(refusal_msg),
                },
                finish_reason: Some("content_filter".to_string()),
            });
        }
    }

    // Extract and map usage metadata from Gemini to OpenAI format
    // Supports both legacy v1internal format (promptTokenCount/candidatesTokenCount/totalTokenCount/cachedContentTokenCount)
    // and new Interactions API format (total_input_tokens/total_output_tokens/total_thought_tokens/total_cached_tokens)
    let usage = raw.get("usageMetadata").map(|u| {
        let canonical = crate::proxy::pipeline::CanonicalUsage::from_gemini(u);
        let mut usage = super::models::OpenAIUsage::from(&canonical);
        usage.input_tokens_by_modality = u.get("input_tokens_by_modality").cloned();
        usage.total_tool_use_tokens = u
            .get("total_tool_use_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        usage
    });

    OpenAIResponse {
        id: raw
            .get("responseId")
            .and_then(|v| v.as_str())
            .unwrap_or("resp_unknown")
            .to_string(),
        object: "chat.completion".to_string(),
        created: chrono::Utc::now().timestamp() as u64,
        model: raw
            .get("modelVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string(),
        choices,
        usage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_transform_openai_response() {
        let gemini_resp = json!({
            "candidates": [{
                "content": {
                    "parts": [{"text": "Hello!"}]
                },
                "finishReason": "STOP"
            }],
            "modelVersion": "gemini-2.5-flash",
            "responseId": "resp_123"
        });

        let result = transform_openai_response(&gemini_resp, Some("session-123"), 1, None);
        assert_eq!(result.object, "chat.completion");
        let content = match result.choices[0].message.content.as_ref().unwrap() {
            OpenAIContent::String(s) => s,
            _ => panic!("Expected string content"),
        };
        assert_eq!(content, "Hello!");
        assert_eq!(result.choices[0].finish_reason, Some("stop".to_string()));
    }

    #[test]
    fn test_usage_metadata_mapping() {
        let gemini_resp = json!({
            "candidates": [{
                "content": {"parts": [{"text": "Hello!"}]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 100,
                "candidatesTokenCount": 50,
                "totalTokenCount": 150,
                "cachedContentTokenCount": 25
            },
            "modelVersion": "gemini-2.5-flash",
            "responseId": "resp_123"
        });

        let result = transform_openai_response(&gemini_resp, Some("session-123"), 1, None);

        assert!(result.usage.is_some());
        let usage = result.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
        assert!(usage.prompt_tokens_details.is_some());
        assert_eq!(usage.prompt_tokens_details.unwrap().cached_tokens, Some(25));
    }

    #[test]
    fn test_interactions_usage_metadata_mapping() {
        let gemini_resp = json!({
            "candidates": [{
                "content": {"parts": [{"text": "Hello!"}]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "input_tokens_by_modality": [
                    {
                        "modality": "text",
                        "tokens": 7
                    }
                ],
                "total_cached_tokens": 0,
                "total_input_tokens": 7,
                "total_output_tokens": 20,
                "total_thought_tokens": 22,
                "total_tokens": 49,
                "total_tool_use_tokens": 0
            },
            "modelVersion": "gemini-3-flash-preview",
            "responseId": "resp_123"
        });

        let result = transform_openai_response(&gemini_resp, Some("session-123"), 1, None);
        let usage = result.usage.unwrap();

        assert_eq!(usage.prompt_tokens, 7);
        assert_eq!(usage.completion_tokens, 42);
        assert_eq!(usage.total_tokens, 49);
        assert_eq!(
            usage
                .completion_tokens_details
                .as_ref()
                .unwrap()
                .reasoning_tokens,
            Some(22)
        );

        let responses_usage = usage.to_responses_usage_value();
        assert_eq!(responses_usage["input_tokens"], 7);
        assert_eq!(responses_usage["input_tokens_details"]["cached_tokens"], 0);
        assert_eq!(responses_usage["output_tokens"], 42);
        assert_eq!(
            responses_usage["output_tokens_details"]["reasoning_tokens"],
            22
        );
        assert_eq!(responses_usage["total_tokens"], 49);
    }

    #[test]
    fn test_response_without_usage_metadata() {
        let gemini_resp = json!({
            "candidates": [{
                "content": {"parts": [{"text": "Hello!"}]},
                "finishReason": "STOP"
            }],
            "modelVersion": "gemini-2.5-flash",
            "responseId": "resp_123"
        });

        let result = transform_openai_response(&gemini_resp, Some("session-123"), 1, None);
        assert!(result.usage.is_none());
    }

    #[test]
    fn test_normalize_and_sanitize_tool_args_passthrough() {
        let mut args = json!({
            "cmd": "ls -la /tmp",
            "description": "Custom description",
            "arbitrary_field": 123
        });
        normalize_and_sanitize_tool_args("shell", &mut args);
        // 验证纯透传：参数原样保持，没有任何字段被重命名、删除或注入
        assert_eq!(args["cmd"], "ls -la /tmp");
        assert_eq!(args["description"], "Custom description");
        assert_eq!(args["arbitrary_field"], 123);
        assert!(!args.as_object().unwrap().contains_key("command"));
    }
}
