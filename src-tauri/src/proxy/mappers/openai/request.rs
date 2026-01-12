// OpenAI → Gemini 请求转换
use super::models::*;
use super::streaming::get_thought_signature;
use crate::proxy::mappers::common::gemini_api_validator::validate_gemini_request;
use crate::proxy::mappers::common::gemini_detection::is_gemini_3_model;
use crate::proxy::mappers::common::thinking_level_mapper::determine_thinking_level;
use serde_json::{json, Value};

pub fn transform_openai_request(
    request: &OpenAIRequest,
    project_id: &str,
    mapped_model: &str,
) -> Result<Value, String> {
    // 将 OpenAI 工具转为 Value 数组以便探测
    let tools_val = request.tools.as_ref().map(|list| list.to_vec());

    // Resolve grounding config
    let config = crate::proxy::mappers::common_utils::resolve_request_config(
        &request.model,
        mapped_model,
        &tools_val,
    );

    tracing::debug!(
        "[Debug] OpenAI Request: original='{}', mapped='{}', type='{}', has_image_config={}",
        request.model,
        mapped_model,
        config.request_type,
        config.image_config.is_some()
    );

    // 1. 提取所有 System Message 并注入补丁
    let system_instructions: Vec<String> = request
        .messages
        .iter()
        .filter(|msg| msg.role == "system")
        .filter_map(|msg| {
            msg.content.as_ref().map(|c| match c {
                OpenAIContent::String(s) => s.clone(),
                OpenAIContent::Array(blocks) => blocks
                    .iter()
                    .filter_map(|b| {
                        if let OpenAIContentBlock::Text { text } = b {
                            Some(text.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            })
        })
        .collect();

    // Pre-scan to map tool_call_id to function name (for Codex)
    let mut tool_id_to_name = std::collections::HashMap::new();
    for msg in &request.messages {
        if let Some(tool_calls) = &msg.tool_calls {
            for call in tool_calls {
                let name = &call.function.name;
                let final_name = if name == "local_shell_call" {
                    "shell"
                } else {
                    name
                };
                tool_id_to_name.insert(call.id.clone(), final_name.to_string());
            }
        }
    }

    // 从全局存储获取 thoughtSignature (PR #93 支持)
    let global_thought_sig = get_thought_signature();
    if let Some(ref sig) = global_thought_sig {
        tracing::debug!("从全局存储获取到 thoughtSignature (长度: {})", sig.len());
    }

    // 2. 构建 Gemini contents (过滤掉 system)
    let contents: Vec<Value> = request
        .messages
        .iter()
        .filter(|msg| msg.role != "system")
        .map(|msg| {
            let role = match msg.role.as_str() {
                "assistant" => "model",
                "tool" | "function" => "user",
                _ => &msg.role,
            };

            let mut parts = Vec::new();

            // Handle content (multimodal or text)
            if let Some(content) = &msg.content {
                match content {
                    OpenAIContent::String(s) => {
                        if !s.is_empty() {
                            parts.push(json!({"text": s}));
                        }
                    }
                    OpenAIContent::Array(blocks) => {
                        for block in blocks {
                            match block {
                                OpenAIContentBlock::Text { text } => {
                                    parts.push(json!({"text": text}));
                                }
                                OpenAIContentBlock::ImageUrl { image_url } => {
                                    if image_url.url.starts_with("data:") {
                                        if let Some(pos) = image_url.url.find(",") {
                                            let mime_part = &image_url.url[5..pos];
                                            let mime_type = mime_part.split(';').next().unwrap_or("image/jpeg");
                                            let data = &image_url.url[pos + 1..];

                                            parts.push(json!({
                                                "inlineData": { "mimeType": mime_type, "data": data }
                                            }));
                                        }
                                    } else if image_url.url.starts_with("http") {
                                        parts.push(json!({
                                            "fileData": { "fileUri": &image_url.url, "mimeType": "image/jpeg" }
                                        }));
                                    } else {
                                        // [NEW] 处理本地文件路径 (file:// 或 Windows/Unix 路径)
                                        let file_path = if image_url.url.starts_with("file://") {
                                            // 移除 file:// 前缀
                                            #[cfg(target_os = "windows")]
                                            { image_url.url.trim_start_matches("file:///").replace('/', "\\") }
                                            #[cfg(not(target_os = "windows"))]
                                            { image_url.url.trim_start_matches("file://").to_string() }
                                        } else {
                                            image_url.url.clone()
                                        };

                                        tracing::debug!("[OpenAI-Request] Reading local image: {}", file_path);

                                        // 读取文件并转换为 base64
                                        if let Ok(file_bytes) = std::fs::read(&file_path) {
                                            use base64::Engine as _;
                                            let b64 = base64::engine::general_purpose::STANDARD.encode(&file_bytes);

                                            // 根据文件扩展名推断 MIME 类型
                                            let mime_type = if file_path.to_lowercase().ends_with(".png") {
                                                "image/png"
                                            } else if file_path.to_lowercase().ends_with(".gif") {
                                                "image/gif"
                                            } else if file_path.to_lowercase().ends_with(".webp") {
                                                "image/webp"
                                            } else {
                                                "image/jpeg"
                                            };

                                            parts.push(json!({
                                                "inlineData": { "mimeType": mime_type, "data": b64 }
                                            }));
                                            tracing::debug!("[OpenAI-Request] Successfully loaded image: {} ({} bytes)", file_path, file_bytes.len());
                                        } else {
                                            tracing::debug!("[OpenAI-Request] Failed to read local image: {}", file_path);
                                        }
                                    }
                                }
                                OpenAIContentBlock::AudioUrl { audio_url: _ } => {
                                    // [PR #311 部分合并] 暂时跳过 audio_url 处理
                                    // 完整实现需要下载音频文件并转换为 Gemini inlineData 格式
                                    // 这会与 v3.3.16 的 thinkingConfig 逻辑冲突，留待后续版本实现
                                    tracing::debug!("[OpenAI-Request] Skipping audio_url (not yet implemented in v3.3.16)");
                                }
                            }
                        }
                    }
                }
            }

            // Handle tool calls (assistant message)
            if let Some(tool_calls) = &msg.tool_calls {
                for tc in tool_calls.iter() {
                    /* 暂时移除：防止 Codex CLI 界面碎片化
                    if index == 0 && parts.is_empty() {
                         if mapped_model.contains("gemini-3") {
                              parts.push(json!({"text": "Thinking Process: Determining necessary tool actions."}));
                         }
                    }
                    */

                    let args = serde_json::from_str::<Value>(&tc.function.arguments).unwrap_or(json!({}));
                    let mut func_call_part = json!({
                        "functionCall": {
                            "name": if tc.function.name == "local_shell_call" { "shell" } else { &tc.function.name },
                            "args": args
                        }
                    });

                    // [修复] 为该消息内的所有工具调用注入 thoughtSignature (PR #114 优化)
                    if let Some(ref sig) = global_thought_sig {
                        func_call_part["thoughtSignature"] = json!(sig);
                    }

                    parts.push(func_call_part);
                }
            }

            // Handle tool response
            if msg.role == "tool" || msg.role == "function" {
                let name = msg.name.as_deref().unwrap_or("unknown");
                let final_name = if name == "local_shell_call" { "shell" }
                                else if let Some(id) = &msg.tool_call_id { tool_id_to_name.get(id).map_or(name, |s| s.as_str()) }
                                else { name };

                let content_val = match &msg.content {
                    Some(OpenAIContent::String(s)) => s.clone(),
                    Some(OpenAIContent::Array(blocks)) => blocks.iter().filter_map(|b| if let OpenAIContentBlock::Text { text } = b { Some(text.clone()) } else { None }).collect::<Vec<_>>().join("\n"),
                    None => "".to_string()
                };

                parts.push(json!({
                    "functionResponse": {
                       "name": final_name,
                       "response": { "result": content_val }
                    }
                }));
            }

            json!({ "role": role, "parts": parts })
        })
        .collect();

    // [PR #合并] 合并连续相同角色的消息 (Gemini 强制要求 user/model 交替)
    let mut merged_contents: Vec<Value> = Vec::new();
    for msg in contents {
        if let Some(last) = merged_contents.last_mut() {
            if last["role"] == msg["role"] {
                // 合并 parts
                if let (Some(last_parts), Some(msg_parts)) =
                    (last["parts"].as_array_mut(), msg["parts"].as_array())
                {
                    last_parts.extend(msg_parts.iter().cloned());
                    continue;
                }
            }
        }
        merged_contents.push(msg);
    }
    let contents = merged_contents;

    // 3. Build request body
    // [EPIC-011 Story-011-01] Detect Gemini 3.x thinking models, inject thinkingLevel config
    // CHANGED: Use centralized detection function that includes ALL Gemini 3 variants
    let is_gemini_3_thinking = is_gemini_3_model(mapped_model);

    let mut gen_config = json!({
        "maxOutputTokens": request.max_tokens.unwrap_or(64000),
        "temperature": request.temperature.unwrap_or(1.0),
        "topP": request.top_p.unwrap_or(1.0),
    });

    // [NEW] 支持多候选结果数量 (n -> candidateCount)
    if let Some(n) = request.n {
        gen_config["candidateCount"] = json!(n);
    }

    // [EPIC-011 Story-011-01 + Story-011-04] Inject thinkingConfig for Gemini 3.x (uses thinkingLevel API)
    // CRITICAL: Gemini 3.x uses thinkingLevel (enum), NOT thinkingBudget (integer)
    // [Story-011-04] OpenAI protocol auto-injects thinking with model-specific defaults
    // [Code-Review Fix] Support OpenAI reasoning_effort field for client control
    if is_gemini_3_thinking {
        // Determine thinking level from reasoning_effort or use defaults
        let thinking_level = if let Some(ref effort) = request.reasoning_effort {
            let effort_lower = effort.to_lowercase();
            // Map OpenAI reasoning_effort to Gemini thinkingLevel
            let level = match effort_lower.as_str() {
                "low" => {
                    tracing::info!(
                        category = "thinking_mapping",
                        model = %mapped_model,
                        reasoning_effort = %effort,
                        level = "LOW",
                        source = "openai_reasoning_effort",
                        "OpenAI reasoning_effort mapped to LOW"
                    );
                    "LOW"
                }
                "medium" => {
                    // CRITICAL: Pro models don't support MEDIUM
                    if mapped_model.contains("-flash") {
                        tracing::info!(
                            category = "thinking_mapping",
                            model = %mapped_model,
                            reasoning_effort = %effort,
                            level = "MEDIUM",
                            source = "openai_reasoning_effort",
                            "OpenAI reasoning_effort mapped to MEDIUM (Flash)"
                        );
                        "MEDIUM"
                    } else {
                        tracing::warn!(
                            category = "thinking_validation",
                            error_type = "medium_downgrade",
                            model = %mapped_model,
                            requested_level = "MEDIUM",
                            actual_level = "LOW",
                            reasoning_effort = %effort,
                            reason = "Pro models don't support MEDIUM, downgraded from OpenAI reasoning_effort='medium'",
                            "OpenAI reasoning_effort='medium' downgraded to LOW for Pro"
                        );
                        "LOW" // Pro: downgrade MEDIUM to LOW
                    }
                }
                "high" => {
                    tracing::info!(
                        category = "thinking_mapping",
                        model = %mapped_model,
                        reasoning_effort = %effort,
                        level = "HIGH",
                        source = "openai_reasoning_effort",
                        "OpenAI reasoning_effort mapped to HIGH"
                    );
                    "HIGH"
                }
                _ => {
                    // Invalid effort, use model defaults
                    tracing::warn!(
                        category = "thinking_validation",
                        error_type = "invalid_reasoning_effort",
                        model = %mapped_model,
                        reasoning_effort = %effort,
                        reason = "Invalid reasoning_effort value, using model defaults",
                        "Invalid OpenAI reasoning_effort, using model defaults"
                    );
                    determine_thinking_level(mapped_model, None)
                }
            };
            level
        } else {
            // No reasoning_effort specified, use model defaults
            // Flash default: MEDIUM (balance cost/quality)
            // Pro default: HIGH (maximize quality)
            determine_thinking_level(mapped_model, None)
        };

        gen_config["thinkingConfig"] = json!({
            "includeThoughts": true,
            "thinkingLevel": thinking_level
        });

        let source = if request.reasoning_effort.is_some() {
            "client-specified"
        } else {
            "auto-injected"
        };
        tracing::info!(
            "[OpenAI-Request] Gemini 3 thinkingLevel: {} (model: {}, {})",
            thinking_level,
            mapped_model,
            source
        );

        // [Story-013-06] Record analytics for cost tracking
        // Only spawn if Tokio runtime is available (tests don't provide one)
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn({
                let model = mapped_model.to_string();
                let level = thinking_level.to_string();
                async move {
                    crate::proxy::analytics::ANALYTICS
                        .record_request(&model, &level)
                        .await;
                }
            });
        }
    }

    if let Some(stop) = &request.stop {
        if stop.is_string() {
            gen_config["stopSequences"] = json!([stop]);
        } else if stop.is_array() {
            gen_config["stopSequences"] = stop.clone();
        }
    }

    if let Some(fmt) = &request.response_format {
        if fmt.r#type == "json_object" {
            gen_config["responseMimeType"] = json!("application/json");
        }
    }

    let mut inner_request = json!({
        "contents": contents,
        "generationConfig": gen_config,
        "safetySettings": [
            { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "OFF" },
            { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "OFF" },
            { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "OFF" },
            { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "OFF" },
            { "category": "HARM_CATEGORY_CIVIC_INTEGRITY", "threshold": "OFF" },
        ]
    });

    // 深度清理 [undefined] 字符串 (Cherry Studio 等客户端常见注入)
    crate::proxy::mappers::common_utils::deep_clean_undefined(&mut inner_request);

    // 4. Handle Tools (Merged Cleaning)
    if let Some(tools) = &request.tools {
        let mut function_declarations: Vec<Value> = Vec::new();
        for tool in tools.iter() {
            let mut gemini_func = if let Some(func) = tool.get("function") {
                func.clone()
            } else {
                let mut func = tool.clone();
                if let Some(obj) = func.as_object_mut() {
                    obj.remove("type");
                    obj.remove("strict");
                    obj.remove("additionalProperties");
                }
                func
            };

            if let Some(name) = gemini_func.get("name").and_then(|v| v.as_str()) {
                // 跳过内置联网工具名称，避免重复定义
                if name == "web_search" || name == "google_search" || name == "web_search_20250305"
                {
                    continue;
                }

                if name == "local_shell_call" {
                    if let Some(obj) = gemini_func.as_object_mut() {
                        obj.insert("name".to_string(), json!("shell"));
                    }
                }
            }

            // [NEW CRITICAL FIX] 清除函数定义根层级的非法字段 (解决报错持久化)
            if let Some(obj) = gemini_func.as_object_mut() {
                obj.remove("format");
                obj.remove("strict");
                obj.remove("additionalProperties");
                obj.remove("type"); // [NEW] Gemini 不支持在 FunctionDeclaration 根层级出现 type: "function"
            }

            if let Some(params) = gemini_func.get_mut("parameters") {
                // [DEEP FIX] 统一调用公共库清洗：展开 $ref 并剔除所有层级的 format/definitions
                crate::proxy::common::json_schema::clean_json_schema(params);

                // Gemini v1internal 要求：
                // 1. type 必须是大写 (OBJECT, STRING 等)
                // 2. 根对象必须有 "type": "OBJECT"
                if let Some(params_obj) = params.as_object_mut() {
                    if !params_obj.contains_key("type") {
                        params_obj.insert("type".to_string(), json!("OBJECT"));
                    }
                }

                // 递归转换 type 为大写 (符合 Protobuf 定义)
                enforce_uppercase_types(params);
            }
            function_declarations.push(gemini_func);
        }

        if !function_declarations.is_empty() {
            inner_request["tools"] = json!([{ "functionDeclarations": function_declarations }]);
        }
    }

    // [NEW] Antigravity 身份指令 (原始简化版)
    let antigravity_identity = "You are Antigravity, a powerful agentic AI coding assistant designed by the Google Deepmind team working on Advanced Agentic Coding.\n\
    You are pair programming with a USER to solve their coding task. The task may require creating a new codebase, modifying or debugging an existing codebase, or simply answering a question.\n\
    **Absolute paths only**\n\
    **Proactiveness**";

    // [HYBRID] 检查用户是否已提供 Antigravity 身份
    let user_has_antigravity = system_instructions
        .iter()
        .any(|s| s.contains("You are Antigravity"));

    let mut parts = Vec::new();

    // 1. Antigravity 身份 (如果需要, 作为独立 Part 插入)
    if !user_has_antigravity {
        parts.push(json!({"text": antigravity_identity}));
    }

    // 2. 追加用户指令 (作为独立 Parts)
    for inst in system_instructions {
        parts.push(json!({"text": inst}));
    }

    inner_request["systemInstruction"] = json!({
        "role": "user",
        "parts": parts
    });

    if config.inject_google_search {
        crate::proxy::mappers::common_utils::inject_google_search_tool(&mut inner_request);
    }

    if let Some(image_config) = config.image_config {
        if let Some(obj) = inner_request.as_object_mut() {
            obj.remove("tools");
            obj.remove("systemInstruction");
            let gen_config = obj.entry("generationConfig").or_insert_with(|| json!({}));
            if let Some(gen_obj) = gen_config.as_object_mut() {
                gen_obj.remove("thinkingConfig");
                gen_obj.remove("responseMimeType");
                gen_obj.remove("responseModalities");
                gen_obj.insert("imageConfig".to_string(), image_config);
            }
        }
    }

    // [EPIC-011 Story-011-03] Validate Gemini API format before sending to upstream
    if mapped_model.starts_with("gemini-") {
        if let Err(e) = validate_gemini_request(mapped_model, &inner_request) {
            tracing::error!("[OpenAI-Request] Gemini API validation failed: {}", e);
            return Err(format!("Gemini API validation error: {}", e));
        }
    }

    Ok(json!({
        "project": project_id,
        "requestId": format!("openai-{}", uuid::Uuid::new_v4()),
        "request": inner_request,
        "model": config.final_model,
        "userAgent": "antigravity",
        "requestType": config.request_type
    }))
}

fn enforce_uppercase_types(value: &mut Value) {
    if let Value::Object(map) = value {
        if let Some(Value::String(ref mut s)) = map.get_mut("type") {
            *s = s.to_uppercase();
        }
        if let Some(Value::Object(ref mut props)) = map.get_mut("properties") {
            for v in props.values_mut() {
                enforce_uppercase_types(v);
            }
        }
        if let Some(items) = map.get_mut("items") {
            enforce_uppercase_types(items);
        }
    } else if let Value::Array(arr) = value {
        for item in arr {
            enforce_uppercase_types(item);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_openai_request_multimodal() {
        let req = OpenAIRequest {
            model: "gpt-4-vision".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::Array(vec![
                    OpenAIContentBlock::Text { text: "What is in this image?".to_string() },
                    OpenAIContentBlock::ImageUrl { image_url: OpenAIImageUrl {
                        url: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==".to_string(),
                        detail: None
                    } }
                ])),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }],
            stream: false,
            n: None,
            max_tokens: None,
            temperature: None,
            top_p: None,
            stop: None,
            response_format: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            reasoning_effort: None,
            instructions: None,
            input: None,
            prompt: None,
        };

        let result = transform_openai_request(&req, "test-v", "gemini-1.5-flash").unwrap();
        let parts = &result["request"]["contents"][0]["parts"];
        assert_eq!(parts.as_array().unwrap().len(), 2);
        assert_eq!(parts[0]["text"].as_str().unwrap(), "What is in this image?");
        assert_eq!(
            parts[1]["inlineData"]["mimeType"].as_str().unwrap(),
            "image/png"
        );
    }
}
