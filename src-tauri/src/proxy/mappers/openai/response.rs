// OpenAI 协议响应转换模块
use super::models::*;
use serde_json::Value;

pub fn is_shell_or_terminal_tool(tool_name: &str) -> bool {
    matches!(
        tool_name.to_ascii_lowercase().as_str(),
        "shell"
            | "bash"
            | "local_shell"
            | "local_shell_call"
            | "powershell"
            | "pwsh"
            | "terminal"
            | "cmd"
            | "run_command"
            | "execute_command"
    )
}

pub fn is_workflow_tool(tool_name: &str) -> bool {
    matches!(
        tool_name.to_ascii_lowercase().as_str(),
        "workflow" | "run_workflow" | "execute_workflow"
    )
}

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
pub fn normalize_and_sanitize_tool_args(tool_name: &str, args: &mut Value) {
    if is_workflow_tool(tool_name) {
        if let Some(obj) = args.as_object_mut() {
            // 1. 规范化 script 字段
            if !obj.contains_key("script") {
                for alt in &["code", "content", "command", "workflow", "body"] {
                    if let Some(val) = obj.remove(*alt) {
                        obj.insert("script".to_string(), val);
                        break;
                    }
                }
            }
            // 若 script 为空或缺失，给一个合法的默认占位脚本
            let script_empty = match obj.get("script") {
                None => true,
                Some(v) => v.as_str().map(|s| s.trim().is_empty()).unwrap_or(false),
            };
            if script_empty {
                obj.insert(
                    "script".to_string(),
                    Value::String("// Workflow action logged\nreturn true;".to_string()),
                );
            }

            // 2. 规范化 meta 字段: { name: string, description: string }
            let mut meta_obj = match obj.remove("meta") {
                Some(Value::Object(m)) => m,
                _ => serde_json::Map::new(),
            };

            // 从顶层回捞可能被打平的 name 和 description
            if !meta_obj.contains_key("name") {
                if let Some(top_name) = obj.remove("name") {
                    meta_obj.insert("name".to_string(), top_name);
                } else {
                    meta_obj.insert(
                        "name".to_string(),
                        Value::String("dsh_workflow_task".to_string()),
                    );
                }
            }
            if !meta_obj.contains_key("description") {
                if let Some(top_desc) = obj.remove("description") {
                    meta_obj.insert("description".to_string(), top_desc);
                } else {
                    let desc = obj
                        .get("script")
                        .and_then(|v| v.as_str())
                        .map(|s| {
                            let first_line = s.lines().next().unwrap_or("Execute workflow");
                            let clean = first_line.trim().trim_start_matches("//").trim();
                            if clean.is_empty() {
                                "Execute workflow script"
                            } else {
                                clean
                            }
                        })
                        .unwrap_or("Execute workflow script");
                    meta_obj.insert("description".to_string(), Value::String(desc.to_string()));
                }
            }

            obj.insert("meta".to_string(), Value::Object(meta_obj));
        }
        return;
    }

    if !is_shell_or_terminal_tool(tool_name) {
        return;
    }

    if let Some(obj) = args.as_object_mut() {
        // 1. 别名归一化至 command
        if !obj.contains_key("command") {
            for alt_key in &["cmd", "code", "script", "shell_command", "input"] {
                if let Some(val) = obj.remove(*alt_key) {
                    obj.insert("command".to_string(), val);
                    tracing::debug!(
                        "[OpenAI] Normalized tool '{}' arg '{}' -> 'command'",
                        tool_name,
                        alt_key
                    );
                    break;
                }
            }
        }

        // 2. 检查 command 是否缺失或为空
        let command_missing = match obj.get("command") {
            None => true,
            Some(v) => v.as_str().map(|s| s.trim().is_empty()).unwrap_or(false),
        };

        if command_missing {
            // 尝试查看模型是否把真实命令直接写在了 description 里
            let raw_desc = obj
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();

            // 如果 description 看起来像一条可执行命令（如包含管道、常见命令开头等），不要盲目覆盖成 echo
            let is_likely_command = !raw_desc.is_empty()
                && (raw_desc.starts_with("git ")
                    || raw_desc.starts_with("ls ")
                    || raw_desc.starts_with("dir ")
                    || raw_desc.starts_with("cd ")
                    || raw_desc.starts_with("cat ")
                    || raw_desc.starts_with("cargo ")
                    || raw_desc.starts_with("npm ")
                    || raw_desc.starts_with("pnpm ")
                    || raw_desc.starts_with("yarn ")
                    || raw_desc.starts_with("node ")
                    || raw_desc.starts_with("python ")
                    || raw_desc.starts_with("Get-")
                    || raw_desc.starts_with("Set-")
                    || raw_desc.contains(" | ")
                    || raw_desc.contains(";"));

            if is_likely_command {
                obj.insert("command".to_string(), Value::String(raw_desc));
            } else {
                let desc_for_log = if raw_desc.is_empty() {
                    "Action logged"
                } else {
                    raw_desc.as_str()
                };

                let safe_desc: String = desc_for_log
                    .chars()
                    .filter(|c| c.is_alphanumeric() || " _-:.,/".contains(*c))
                    .collect();
                let trimmed = safe_desc.trim();
                let safe_title = if trimmed.is_empty() {
                    "Action logged"
                } else {
                    trimmed
                };

                let fallback_cmd = format!("echo \"[OK: Action logged - {}]\"", safe_title);
                obj.insert("command".to_string(), Value::String(fallback_cmd));
                tracing::warn!(
                    tool = %tool_name,
                    description = %raw_desc,
                    "Injected safe fallback 'command' into tool call arguments to prevent downstream client crash (Issue #3430)"
                );
            }
        }

        // 3. [Issue #3440] DSH (DeepSeek Harness) 强依赖 `description` 字段（必填 string）。
        //    如果缺失 description，必须从 command 生成简要描述，防止 DSH 抛出 description is required 崩溃。
        let desc_missing = match obj.get("description") {
            None => true,
            Some(v) => v.as_str().map(|s| s.trim().is_empty()).unwrap_or(false),
        };

        if desc_missing {
            let cmd_str = obj
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("Execute shell command")
                .trim();
            // 取命令的前 60 字符作为简要描述
            let auto_desc = if cmd_str.len() > 60 {
                format!("Run: {}...", &cmd_str[..57])
            } else if !cmd_str.is_empty() {
                format!("Run: {}", cmd_str)
            } else {
                "Execute command".to_string()
            };
            obj.insert("description".to_string(), Value::String(auto_desc));
        }
    }
}

pub fn resolve_shell_tool_name(
    model_tool_name: &str,
    client_tool_names: &std::collections::HashSet<String>,
) -> String {
    if model_tool_name == "shell"
        || model_tool_name == "bash"
        || model_tool_name == "local_shell"
        || model_tool_name == "local_shell_call"
    {
        if client_tool_names.contains(model_tool_name) {
            return model_tool_name.to_string();
        }
        for name in &["local_shell_call", "bash", "shell", "local_shell"] {
            if client_tool_names.contains(*name) {
                return name.to_string();
            }
        }
        "local_shell_call".to_string()
    } else {
        model_tool_name.to_string()
    }
}
fn extract_apply_patch_input(args: &Value) -> String {
    if let Some(obj) = args.as_object() {
        if let Some(input) = obj.get("input").and_then(|v| v.as_str()) {
            return input.to_string();
        }
        if let Some(arr) = obj.get("command").and_then(|v| v.as_array()) {
            if arr.len() > 1 {
                if let Some(patch) = arr[1].as_str() {
                    return patch.to_string();
                }
            }
        }
        if let Some(cmd_str) = obj.get("command").and_then(|v| v.as_str()) {
            if let Some(patch) = cmd_str.strip_prefix("apply_patch\n") {
                return patch.to_string();
            }
            if let Some(patch) = cmd_str.strip_prefix("apply_patch ") {
                return patch.to_string();
            }
            return cmd_str.to_string();
        }
        for key in ["patch_text", "patch", "diff", "content"] {
            if let Some(patch) = obj.get(key).and_then(|v| v.as_str()) {
                return patch.to_string();
            }
        }
    }
    args.as_str()
        .map(str::to_string)
        .unwrap_or_else(|| serde_json::to_string(args).unwrap_or_default())
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

                        let mut arguments_str = args_json.to_string();

                        // [FIX] Codex CLI apply_patch freeform raw string
                        if name == "apply_patch" || name == "apply_patch_v2" {
                            let extracted_patch = extract_apply_patch_input(&args_json);
                            let (optimized_patch, _) =
                                crate::proxy::adapters::apply_patch_preflight::optimize_patch(
                                    &extracted_patch,
                                    None,
                                    true,
                                );
                            arguments_str = optimized_patch;
                            if let Some((line, message)) =
                                crate::proxy::adapters::apply_patch_preflight::validate_v4a_for_codex(
                                    &arguments_str,
                                )
                            {
                                if !content_out.is_empty() {
                                    content_out.push('\n');
                                }
                                content_out.push_str(&format!(
                                    "apply_patch 格式非法，已停止执行以避免重复失败。第 {line} 行：{message}"
                                ));
                                continue;
                            }
                        }

                        let final_name = resolve_shell_tool_name(name, client_tool_names);

                        let id = fc
                            .get("id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| format!("{}-{}", final_name, uuid::Uuid::new_v4()));

                        if let Some(sig) = part.get("thoughtSignature").or(part.get("thought_signature")).and_then(|s| s.as_str()) {
                            crate::proxy::SignatureCache::global().cache_tool_signature(&id, sig.to_string());
                        }

                        tool_calls.push(ToolCall {
                            id,
                            r#type: "function".to_string(),
                            function: Some(ToolFunction {
                                name: final_name.to_string(),
                                arguments: arguments_str,
                            }),
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
    let usage = raw.get("usageMetadata").and_then(|u| {
        // 优先使用新格式字段，fallback 到旧格式
        let prompt_tokens = u
            .get("total_input_tokens")
            .or_else(|| u.get("promptTokenCount"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let raw_output_tokens = u
            .get("total_output_tokens")
            .or_else(|| u.get("candidatesTokenCount"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let raw_total_tokens = u
            .get("total_tokens")
            .or_else(|| u.get("totalTokenCount"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let cached_tokens = u
            .get("total_cached_tokens")
            .or_else(|| u.get("cachedContentTokenCount"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        // [NEW] 从新格式提取 reasoning/thought tokens
        let reasoning_tokens = u
            .get("total_thought_tokens")
            .or_else(|| u.get("totalThoughtTokens"))
            .or_else(|| u.get("thoughtsTokenCount"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let tool_use_tokens = u
            .get("total_tool_use_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let input_tokens_by_modality = u.get("input_tokens_by_modality").cloned();

        // New Interactions usage keeps thought/tool-use tokens separate from
        // total_output_tokens. Legacy candidatesTokenCount already includes those.
        let has_new_format = u.get("total_output_tokens").is_some();
        let completion_tokens = if has_new_format {
            raw_output_tokens + reasoning_tokens.unwrap_or(0) + tool_use_tokens.unwrap_or(0)
        } else {
            raw_output_tokens
        };

        // Keep prompt_tokens as Gemini's raw input token count. cached_tokens is a
        // subset of the prompt, not an amount to subtract from it.
        let final_total_tokens = raw_total_tokens.unwrap_or(prompt_tokens + completion_tokens);

        Some(super::models::OpenAIUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens: final_total_tokens,
            prompt_tokens_details: cached_tokens.map(|ct| super::models::PromptTokensDetails {
                cached_tokens: Some(ct),
            }),
            completion_tokens_details: reasoning_tokens.map(|rt| {
                super::models::CompletionTokensDetails {
                    reasoning_tokens: Some(rt),
                }
            }),
            input_tokens_by_modality,
            raw_output_tokens: Some(raw_output_tokens),
            total_thought_tokens: reasoning_tokens,
            total_tool_use_tokens: tool_use_tokens,
            gemini_total_tokens: raw_total_tokens,
        })
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
    fn test_normalize_and_sanitize_tool_args_shell_alias() {
        let mut args = json!({
            "cmd": "ls -la /tmp"
        });
        normalize_and_sanitize_tool_args("shell", &mut args);
        assert_eq!(args["command"], "ls -la /tmp");
        assert!(!args.as_object().unwrap().contains_key("cmd"));
    }

    #[test]
    fn test_normalize_and_sanitize_tool_args_powershell_missing_command_with_description() {
        // [Issue #3430] WorkBuddy PowerShell tool call with only description
        let mut args = json!({
            "description": "列出目录内容"
        });
        normalize_and_sanitize_tool_args("PowerShell", &mut args);
        assert_eq!(
            args["command"],
            "echo \"[OK: Action logged - 列出目录内容]\""
        );
        assert_eq!(args["description"], "列出目录内容");
    }

    #[test]
    fn test_normalize_and_sanitize_tool_args_bash_empty_command_fallback() {
        let mut args = json!({
            "command": "   ",
            "description": "Fetch status"
        });
        normalize_and_sanitize_tool_args("Bash", &mut args);
        assert_eq!(
            args["command"],
            "echo \"[OK: Action logged - Fetch status]\""
        );
    }

    #[test]
    fn test_normalize_and_sanitize_tool_args_unrelated_tool_untouched() {
        let mut args = json!({
            "query": "select * from users"
        });
        normalize_and_sanitize_tool_args("sql_query", &mut args);
        assert!(!args.as_object().unwrap().contains_key("command"));
        assert_eq!(args["query"], "select * from users");
    }

    #[test]
    fn test_transform_openai_response_sanitizes_powershell_tool_call() {
        let gemini_resp = json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "functionCall": {
                            "name": "PowerShell",
                            "args": {
                                "description": "查看当前系统信息"
                            }
                        }
                    }]
                },
                "finishReason": "STOP"
            }]
        });

        let result = transform_openai_response(&gemini_resp, None, 1, None);
        let tool_calls = result.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].function.as_ref().unwrap().name, "PowerShell");

        let parsed_args: Value =
            serde_json::from_str(&tool_calls[0].function.as_ref().unwrap().arguments).unwrap();
        assert_eq!(
            parsed_args["command"],
            "echo \"[OK: Action logged - 查看当前系统信息]\""
        );
    }

    #[test]
    fn test_normalize_and_sanitize_tool_args_dsh_pwsh_missing_description() {
        // [Issue #3440] DSH tool-pwsh requires both command AND description
        let mut args = json!({
            "command": "Get-ChildItem -Path ./src -Recurse | Select-Object -First 10"
        });
        normalize_and_sanitize_tool_args("pwsh", &mut args);
        assert_eq!(
            args["command"],
            "Get-ChildItem -Path ./src -Recurse | Select-Object -First 10"
        );
        assert!(args.get("description").is_some());
        assert!(args["description"].as_str().unwrap().starts_with("Run: "));
    }

    #[test]
    fn test_normalize_and_sanitize_tool_args_dsh_pwsh_command_in_description() {
        // [Issue #3440] Gemini puts actual command inside description
        let mut args = json!({
            "description": "git status -s"
        });
        normalize_and_sanitize_tool_args("pwsh", &mut args);
        assert_eq!(args["command"], "git status -s");
        assert_eq!(args["description"], "git status -s");
    }

    #[test]
    fn test_normalize_and_sanitize_tool_args_dsh_workflow_flattened() {
        // [Issue #3440] DSH tool-workflow requires script and meta: { name, description }
        let mut args = json!({
            "script": "console.log('running test');",
            "name": "run_test",
            "description": "Run unit test suite"
        });
        normalize_and_sanitize_tool_args("workflow", &mut args);
        assert_eq!(args["script"], "console.log('running test');");
        assert!(args.get("meta").is_some());
        assert_eq!(args["meta"]["name"], "run_test");
        assert_eq!(args["meta"]["description"], "Run unit test suite");
        assert!(!args.as_object().unwrap().contains_key("name"));
    }

    #[test]
    fn test_normalize_and_sanitize_tool_args_dsh_workflow_missing_meta() {
        let mut args = json!({
            "code": "// Auto workflow\nreturn 42;"
        });
        normalize_and_sanitize_tool_args("workflow", &mut args);
        assert_eq!(args["script"], "// Auto workflow\nreturn 42;");
        assert_eq!(args["meta"]["name"], "dsh_workflow_task");
        assert_eq!(args["meta"]["description"], "Auto workflow");
    }
}
