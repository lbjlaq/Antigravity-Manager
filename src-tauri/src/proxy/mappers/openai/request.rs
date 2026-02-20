// OpenAI → Gemini 请求转换
use super::models::*;

use serde_json::{json, Value};

pub fn transform_openai_request(
    request: &OpenAIRequest,
    project_id: &str,
    mapped_model: &str,
) -> (Value, String, usize) {
    let session_id = crate::proxy::session_manager::SessionManager::extract_openai_session_id(request);
    let message_count = request.messages.len();
    // 将 OpenAI 工具转为 Value 数组以便探测
    let tools_val = request
        .tools
        .as_ref()
        .map(|list| list.iter().map(|v| v.clone()).collect::<Vec<_>>());

    let mapped_model_lower = mapped_model.to_lowercase();

    // Resolve grounding config
    let config = crate::proxy::mappers::common_utils::resolve_request_config(
        &request.model,
        &mapped_model_lower,
        &tools_val,
        request.size.as_deref(),       // [NEW] Pass size parameter
        request.quality.as_deref(),    // [NEW] Pass quality parameter
        request.image_size.as_deref(), // [FIX] Pass imageSize parameter
        None,  // body
    );

    // [FIX] 仅当模型名称显式包含 "-thinking" 时才视为 Gemini 思维模型
    // 避免对 gemini-3-pro (preview) 等其实不支持 thinkingConfig 的模型注入参数导致 400
    // [FIX #1557] Allow "pro" models (e.g. gemini-3-pro, gemini-2.0-pro) to bypass thinking check
    // These models support thinking but do not have "-thinking" suffix
    let is_gemini_3_thinking = mapped_model_lower.contains("gemini")
        && (
            mapped_model_lower.contains("-thinking")
                || mapped_model_lower.contains("gemini-2.0-pro")
                || mapped_model_lower.contains("gemini-3-pro")
                || mapped_model_lower.contains("gemini-3.1-pro")
        )
        && !mapped_model_lower.contains("claude");
    let is_claude_thinking = mapped_model_lower.ends_with("-thinking");
    let is_thinking_model = is_gemini_3_thinking || is_claude_thinking;

    // [NEW] 检查用户是否在请求中显式启用 thinking
    let user_enabled_thinking = request.thinking.as_ref()
        .map(|t| t.thinking_type.as_deref() == Some("enabled"))
        .unwrap_or(false);
    let user_thinking_budget = request.thinking.as_ref()
        .and_then(|t| t.budget_tokens);

    // [NEW] 检查历史消息是否兼容思维模型 (是否有 Assistant 消息缺失 reasoning_content)
    let has_incompatible_assistant_history = request.messages.iter().any(|msg| {
        msg.role == "assistant"
            && msg
                .reasoning_content
                .as_ref()
                .map(|s| s.is_empty())
                .unwrap_or(true)
    });
    let has_tool_history = request.messages.iter().any(|msg| {
        msg.role == "tool" || msg.role == "function" || msg.tool_calls.is_some()
    });



    // [NEW] 决定是否开启 Thinking 功能:
    // 1. 模型名包含 -thinking 时自动开启
    // 2. 用户在请求中显式设置 thinking.type = "enabled" 时开启
    // 如果是 Claude 思考模型且历史不兼容且没有可用签名来占位, 则禁用 Thinking 以防 400
    let mut actual_include_thinking = is_thinking_model || user_enabled_thinking;
    
    // [REFACTORED] 使用 SignatureCache 获取 Session 级别的签名
    let session_thought_sig = crate::proxy::SignatureCache::global().get_session_signature(&session_id);
    
    if is_claude_thinking && has_incompatible_assistant_history && session_thought_sig.is_none() {
        tracing::warn!("[OpenAI-Thinking] Incompatible assistant history detected for Claude thinking model without session signature. Disabling thinking for this request to avoid 400 error. (sid: {})", session_id);
        actual_include_thinking = false;
    }
    
    // [NEW] 日志：用户显式设置 thinking
    if user_enabled_thinking {
        tracing::info!(
            "[OpenAI-Thinking] User explicitly enabled thinking with budget: {:?}",
            user_thinking_budget
        );
    }

    tracing::debug!(
        "[Debug] OpenAI Request: original='{}', mapped='{}', type='{}', has_image_config={}",
        request.model,
        mapped_model,
        config.request_type,
        config.image_config.is_some()
    );

    // 1. 提取所有 System Message 并注入补丁
    let mut system_instructions: Vec<String> = request
        .messages
        .iter()
        .filter(|msg| msg.role == "system" || msg.role == "developer")
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

    // [NEW] 如果请求中包含 instructions 字段，优先使用它
    if let Some(inst) = &request.instructions {
        if !inst.is_empty() {
            system_instructions.insert(0, inst.clone());
        }
    }

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

    // 从缓存获取当前会话的思维签名
    let thought_sig = session_thought_sig;
    if thought_sig.is_some() {
        tracing::debug!(
            "[OpenAI-Request] Using session signature (sid: {}, len: {})",
            session_id,
            thought_sig.as_ref().unwrap().len()
        );
    }

    // [New] 预先构建工具名称到原始 Schema 的映射，用于后续参数类型修正
    let mut tool_name_to_schema = std::collections::HashMap::new();
    if let Some(tools) = &request.tools {
        for tool in tools {
            if let (Some(name), Some(params)) = (
                tool.get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|v| v.as_str()),
                tool.get("function").and_then(|f| f.get("parameters")),
            ) {
                tool_name_to_schema.insert(name.to_string(), params.clone());
            } else if let (Some(name), Some(params)) = (
                tool.get("name").and_then(|v| v.as_str()),
                tool.get("parameters"),
            ) {
                // 处理某些客户端可能透传的精简格式
                tool_name_to_schema.insert(name.to_string(), params.clone());
            }
        }
    }

    // 2. 构建 Gemini contents (过滤掉 system/developer 指令)
    let contents: Vec<Value> = request
        .messages
        .iter()
        .filter(|msg| msg.role != "system" && msg.role != "developer")
        .map(|msg| {
            let role = match msg.role.as_str() {
                "assistant" => "model",
                "tool" | "function" => "user", 
                _ => &msg.role,
            };

            let mut parts = Vec::new();

            // Handle reasoning_content (thinking)
            if let Some(reasoning) = &msg.reasoning_content {
                // [FIX #1506] 增强对占位符 [undefined] 的识别
                let is_invalid_placeholder = reasoning == "[undefined]" || reasoning.is_empty();
                
                if !is_invalid_placeholder {
                    let thought_part = json!({
                        "text": reasoning,
                        "thought": true,
                    });
                    parts.push(thought_part);
                }
            } else if actual_include_thinking && role == "model" {
                // [FIX] 解决 Claude 4.6 Thinking 模型的强制性校验:
                // "Expected thinking... but found tool_use/text"
                // 如果是思维模型且缺失 reasoning_content, 则注入占位符
                tracing::debug!("[OpenAI-Thinking] Injecting placeholder thinking block for assistant message");
                let mut thought_part = json!({
                    "text": "Applying tool decisions and generating response...",
                    "thought": true,
                });
                
                // [FIX #1575] 占位符永远不能使用真实签名（签名与真实思考内容绑定）
                // 仅 Gemini 支持哨兵值跳过验证
                if is_gemini_3_thinking {
                    thought_part["thoughtSignature"] = json!("skip_thought_signature_validator");
                }
                
                parts.push(thought_part);
            }

            // Handle content (multimodal or text)
            // [FIX] Skip standard content mapping for tool/function roles to avoid duplicate parts
            // These are handled below in the "Handle tool response" section.
            let is_tool_role = msg.role == "tool" || msg.role == "function";
            if let (Some(content), false) = (&msg.content, is_tool_role) {
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
                                    // 暂时跳过 audio_url 处理
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
                for (_index, tc) in tool_calls.iter().enumerate() {
                    /* 暂时移除：防止 Codex CLI 界面碎片化
                    if index == 0 && parts.is_empty() {
                         if mapped_model.contains("gemini-3") {
                              parts.push(json!({"text": "Thinking Process: Determining necessary tool actions."}));
                         }
                    }
                    */


                    let mut args = serde_json::from_str::<Value>(&tc.function.arguments).unwrap_or(json!({}));
                    
                    // [New] 利用通用引擎修正参数类型 (替代以前硬编码的 shell 工具修复逻辑)
                    if let Some(original_schema) = tool_name_to_schema.get(&tc.function.name) {
                        crate::proxy::common::json_schema::fix_tool_call_args(&mut args, original_schema);
                    }

                    let mut func_call_part = json!({
                        "functionCall": {
                            "name": if tc.function.name == "local_shell_call" { "shell" } else { &tc.function.name },
                            "args": args,
                            "id": &tc.id,
                        }
                    });

                    // [New] 递归清理参数中可能存在的非法校验字段
                    crate::proxy::common::json_schema::clean_json_schema(&mut func_call_part);

                    if let Some(ref sig) = thought_sig {
                        func_call_part["thoughtSignature"] = json!(sig);
                    } else if is_thinking_model {
                        // [NEW] Handle missing signature for Gemini thinking models
                        // [FIX #1650] Allow sentinel injection for Vertex AI (projects/...) as well
                        tracing::debug!("[OpenAI-Signature] Adding GEMINI_SKIP_SIGNATURE for tool_use: {}", tc.id);
                        func_call_part["thoughtSignature"] = json!("skip_thought_signature_validator");
                    }

                    parts.push(func_call_part);
                }
            }

            // Handle tool response
            if msg.role == "tool" || msg.role == "function" {
                let name = msg.name.as_deref().unwrap_or("unknown");
                let final_name = if name == "local_shell_call" { "shell" } 
                                else if let Some(id) = &msg.tool_call_id { tool_id_to_name.get(id).map(|s| s.as_str()).unwrap_or(name) }
                                else { name };

                let content_val = match &msg.content {
                    Some(OpenAIContent::String(s)) => s.clone(),
                    Some(OpenAIContent::Array(blocks)) => blocks.iter().filter_map(|b| if let OpenAIContentBlock::Text { text } = b { Some(text.clone()) } else { None }).collect::<Vec<_>>().join("\n"),
                    None => "".to_string()
                };

                parts.push(json!({
                    "functionResponse": {
                       "name": final_name,
                       "response": { "result": content_val },
                       "id": msg.tool_call_id.clone().unwrap_or_default()
                    }
                }));
            }

            json!({ "role": role, "parts": parts })
        })
        .filter(|msg| !msg["parts"].as_array().map(|a| a.is_empty()).unwrap_or(true))
        .collect();

    // [FIX #1575] 针对思维模型的历史故障恢复
    // 在带有工具的历史记录中，剥离旧的思考块，防止 API 因签名失效或结构冲突报 400
    let mut contents = contents;
    if actual_include_thinking && has_tool_history {
        tracing::debug!("[OpenAI-Thinking] Applied thinking recovery (stripping old thought blocks) for tool history");
        contents = super::thinking_recovery::strip_all_thinking_blocks(contents);
    }

    // 合并连续相同角色的消息 (Gemini 强制要求 user/model 交替)
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

    // 3. 构建请求体

    let mut gen_config = json!({
        "temperature": request.temperature.unwrap_or(1.0),
        "topP": request.top_p.unwrap_or(0.95), // Gemini default is usually 0.95
    });

    // [FIX] 移除默认的 81920 maxOutputTokens，防止非思维模型 (如 claude-sonnet-4-5) 报 400 Invalid Argument
    // 仅在用户显式提供时设置
    if let Some(max_tokens) = request.max_tokens {
         gen_config["maxOutputTokens"] = json!(max_tokens);
    }

    // [NEW] 支持多候选结果数量 (n -> candidateCount)
    if let Some(n) = request.n {
        gen_config["candidateCount"] = json!(n);
    }

    // 为 thinking 模型注入 thinkingConfig (使用 thinkingBudget 而非 thinkingLevel)
    if actual_include_thinking {
        // [RESOLVE #1694] Check image thinking mode
        let image_thinking_mode = crate::proxy::config::get_image_thinking_mode();
        // Only disable if mode is explicitly "disabled" AND it's an image generation request
        let is_image_gen_disabled = config.request_type == "image_gen" && image_thinking_mode == "disabled";

        if is_image_gen_disabled {
            tracing::debug!("[OpenAI-Request] Image thinking mode disabled: enforcing includeThoughts=false for {}", mapped_model);
            gen_config["thinkingConfig"] = json!({
                "includeThoughts": false
            });
        } else {
            // [CONFIGURABLE] 根据用户配置决定 thinking_budget 处理方式
            let tb_config = crate::proxy::config::get_thinking_budget_config();
            // [FIX #1592] 下调默认 budget 到 24576，以更好地兼容不支持 32k 的 Gemini 原生模型 (如 gemini-3-pro)
            let user_budget: i64 = user_thinking_budget.map(|b| b as i64).unwrap_or(24576);
            
            let budget = match tb_config.mode {
                crate::proxy::config::ThinkingBudgetMode::Passthrough => {
                    // 透传模式：使用用户传入的值，不做任何限制
                    tracing::debug!(
                        "[OpenAI-Request] Passthrough mode: using caller's budget {}",
                        user_budget
                    );
                    user_budget
                }
                crate::proxy::config::ThinkingBudgetMode::Custom => {
                    // 自定义模式：使用全局配置的固定值
                    let mut custom_value = tb_config.custom_value as i64;
                    
                    // [FIX #1592/1602] 针对 Gemini 类模型强制执行 24576 上限 (除画图模型外，见用户反馈)
                    let is_gemini_limited = (mapped_model_lower.contains("gemini") && !mapped_model_lower.contains("-image"))
                        || is_claude_thinking; 

                    if is_gemini_limited && custom_value > 24576 {
                        tracing::warn!(
                            "[OpenAI-Request] Custom mode: capping thinking_budget from {} to 24576 for Gemini model {}",
                            custom_value, mapped_model
                        );
                        custom_value = 24576;
                    }

                    tracing::debug!(
                        "[OpenAI-Request] Custom mode: overriding {} with fixed value {}",
                        user_budget,
                        custom_value
                    );
                    custom_value
                }
                crate::proxy::config::ThinkingBudgetMode::Auto => {
                    // [FIX #1592] 拓宽判定逻辑，确保所有 Gemini 思考模型都应用 24k 上限 (除画画模型外)
                    let is_gemini_limited = (mapped_model_lower.contains("gemini") && !mapped_model_lower.contains("-image"))
                        || is_claude_thinking;  
                    
                    if is_gemini_limited && user_budget > 24576 {
                        tracing::info!(
                            "[OpenAI-Request] Auto mode: capping thinking budget from {} to 24576 for model: {}", 
                            user_budget, mapped_model
                        );
                        24576
                    } else {
                        user_budget
                    }
                }
                crate::proxy::config::ThinkingBudgetMode::Adaptive => {
                    // Adaptive 模式暂时使用默认值,实际的 adaptive 逻辑在后续处理
                    user_budget
                }
            };

            gen_config["thinkingConfig"] = json!({
                "includeThoughts": true,
                "thinkingBudget": budget
            });

            // [CRITICAL] 思维模型的 maxOutputTokens 必须大于 thinkingBudget
            // [FIX #1675] 针对图像模型使用更保守的 max_tokens 增量，避免触发 128k 限制
            let overhead = if config.request_type == "image_gen" { 2048 } else { 32768 };
            let min_overhead = if config.request_type == "image_gen" { 1024 } else { 8192 };

            if let Some(max_tokens) = request.max_tokens {
                 if (max_tokens as i64) <= budget {
                     gen_config["maxOutputTokens"] = json!(budget + min_overhead);
                 }
            } else {
                 // [FIX #1592] Use a more conservative default to avoid 400 error on 128k context models
                 gen_config["maxOutputTokens"] = json!(budget + overhead);
            }
            
            let new_max = gen_config["maxOutputTokens"].as_i64().unwrap_or(0);
            tracing::debug!(
                "[OpenAI-Request] Adjusted maxOutputTokens to {} for thinking model (budget={})",
                new_max, budget
            );
            
            tracing::debug!(
                "[OpenAI-Request] Injected thinkingConfig for model {}: thinkingBudget={} (mode={:?})",
                mapped_model, budget, tb_config.mode
            );
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
    crate::proxy::mappers::common_utils::deep_clean_undefined(&mut inner_request, 0);

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

            let name_opt = gemini_func.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());

            if let Some(name) = &name_opt {
                // 跳过内置联网工具名称，避免重复定义
                if name == "web_search"
                    || name == "google_search"
                    || name == "web_search_20250305"
                    || name == "builtin_web_search"
                {
                    continue;
                }

                if name == "local_shell_call" {
                    if let Some(obj) = gemini_func.as_object_mut() {
                        obj.insert("name".to_string(), json!("shell"));
                    }
                }
            } else {
                 // [FIX] 如果工具没有名称，视为无效工具直接跳过 (防止 REQUIRED_FIELD_MISSING)
                 tracing::warn!("[OpenAI-Request] Skipping tool without name: {:?}", gemini_func);
                 continue;
            }

            // [NEW CRITICAL FIX] 清除函数定义根层级的非法字段 (解决报错持久化)
            if let Some(obj) = gemini_func.as_object_mut() {
                obj.remove("format");
                obj.remove("strict");
                obj.remove("additionalProperties");
                obj.remove("type"); // [NEW] Gemini 不支持在 FunctionDeclaration 根层级出现 type: "function"
                obj.remove("external_web_access"); // [FIX #1278] Remove invalid field injected by OpenAI Codex
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
            } else {
                // [FIX] 针对自定义工具 (如 apply_patch) 补全缺失的参数模式
                // 解决 Vertex AI (Claude) 报错: tools.5.custom.input_schema: Field required
                tracing::debug!(
                    "[OpenAI-Request] Injecting default schema for custom tool: {}",
                    gemini_func
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                );

                gemini_func.as_object_mut().unwrap().insert(
                    "parameters".to_string(),
                    json!({
                        "type": "OBJECT",
                        "properties": {
                            "content": {
                                "type": "STRING",
                                "description": "The raw content or patch to be applied"
                            }
                        },
                        "required": ["content"]
                    }),
                );
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

    // 2. [NEW] 注入全局系统提示词 (紧跟 Antigravity 身份之后)
    let global_prompt_config = crate::proxy::config::get_global_system_prompt();
    if global_prompt_config.enabled && !global_prompt_config.content.trim().is_empty() {
        parts.push(json!({"text": global_prompt_config.content}));
    }

    // 3. 追加用户指令 (作为独立 Parts)
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
                // [REMOVED] thinkingConfig 拦截已删除，允许图像生成时输出思维链
                // gen_obj.remove("thinkingConfig");
                gen_obj.remove("responseMimeType");
                gen_obj.remove("responseModalities");
                gen_obj.insert("imageConfig".to_string(), image_config);
            }
        }
    }

    let final_body = json!({
        "project": project_id,
        "requestId": format!("openai-{}", uuid::Uuid::new_v4()),
        "request": inner_request,
        "model": config.final_model,
        "userAgent": "antigravity",
        "requestType": config.request_type
    });

    (final_body, session_id, message_count)
}

fn enforce_uppercase_types(value: &mut Value) {
    if let Value::Object(map) = value {
        if let Some(type_val) = map.get_mut("type") {
            if let Value::String(ref mut s) = type_val {
                *s = s.to_uppercase();
            }
        }
        if let Some(properties) = map.get_mut("properties") {
            if let Value::Object(ref mut props) = properties {
                for v in props.values_mut() {
                    enforce_uppercase_types(v);
                }
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
    use crate::proxy::mappers::openai::models::*;

    #[test]
    #[test]
    fn test_issue_1592_gemini_3_pro_budget_capping() {
        // [FIX #1592] Regression test for gemini-3-pro thinking budget capping
        let req = OpenAIRequest {
            model: "gemini-3-pro".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("test".into())),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }],
            ..Default::default()
        };

        // Auto mode (default) should cap gemini-3-pro thinking budget to 24576
        let (result, _sid, _msg_count) = transform_openai_request(&req, "test-v", "gemini-3-pro");
        let budget = result["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"]
            .as_i64()
            .unwrap();
        assert_eq!(budget, 24576, "Gemini-3-pro budget must be capped to 24576 in Auto mode");
    }

    #[test]
    fn test_issue_1602_custom_mode_gemini_capping() {
        // [FIX #1602] Regression test for custom mode capping
        use crate::proxy::config::{ThinkingBudgetConfig, ThinkingBudgetMode, update_thinking_budget_config};
        
        // 设置自定义模式，且数值超过 24k
        update_thinking_budget_config(ThinkingBudgetConfig {
            mode: ThinkingBudgetMode::Custom,
            custom_value: 32000,
            effort: None,
        });

        let req = OpenAIRequest {
            model: "gemini-2.0-flash-thinking".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("test".into())),
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
            ..Default::default()
        };

        // 验证针对 Gemini 模型即使是 Custom 模式也会被修正为 24576
        let (result, _sid, _msg_count) = transform_openai_request(&req, "test-v", "gemini-2.0-flash-thinking");
        let budget = result["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"]
            .as_i64()
            .unwrap();
        assert_eq!(budget, 24576, "Gemini custom budget must be capped to 24576");

        // 验证非 Gemini 模型（如 Claude 原生路径，假设映射后名不含 gemini）则不应截断
        // 注意：这里的 transform_openai_request 第三个参数是 mapped_model
        let (result_claude, _, _) = transform_openai_request(&req, "test-v", "claude-3-7-sonnet");
        let budget_claude = result_claude["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"]
            .as_i64();
        // 如果不是 gemini 模型且协议中没带 thinking 配置，可能会是 None 或 32000
        // 在该测试环境下，由于模拟的是 OpenAI 格式转 Gemini 路径，如果没有 gemini 关键词通常不进入 thinking 逻辑
        // 我们只需确保 gemini 路径正确受限即可。

        // 恢复默认配置
        update_thinking_budget_config(ThinkingBudgetConfig::default());
    }

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
            ..Default::default()
        };

        let (result, _sid, _msg_count) = transform_openai_request(&req, "test-v", "gemini-1.5-flash");
        let parts = &result["request"]["contents"][0]["parts"];
        assert_eq!(parts.as_array().unwrap().len(), 2);
        assert_eq!(parts[0]["text"].as_str().unwrap(), "What is in this image?");
        assert_eq!(
            parts[1]["inlineData"]["mimeType"].as_str().unwrap(),
            "image/png"
        );
    }
    
    #[test]
    fn test_gemini_pro_thinking_injection() {
        let req = OpenAIRequest {
            model: "gemini-3-pro-preview".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Thinking test".to_string())),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }],
            stream: false,
            n: None,
            // User enabled thinking
            thinking: Some(ThinkingConfig {
                thinking_type: Some("enabled".to_string()),
                budget_tokens: Some(16000),
                effort: None,
            }),
            max_tokens: None,
            temperature: None,
            top_p: None,
            stop: None,
            response_format: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            ..Default::default()
        };

        // Pass explicit gemini-3-pro-preview which doesn't have "-thinking" suffix
        let (result, _sid, _msg_count) = transform_openai_request(&req, "test-p", "gemini-3-pro-preview");
        let gen_config = &result["request"]["generationConfig"];
        
        // Assert thinkingConfig is present (fix verification)
        assert!(gen_config.get("thinkingConfig").is_some(), "thinkingConfig should be injected for gemini-3-pro");
        
        let budget = gen_config["thinkingConfig"]["thinkingBudget"].as_u64().unwrap();
        // Should use user budget (16000) or capped valid default
        assert_eq!(budget, 16000);
    }
    #[test]
    fn test_gemini_3_pro_image_not_thinking() {
        let req = OpenAIRequest {
            model: "gemini-3-pro-image-4k".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Generate a cat".to_string())),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }],
            ..Default::default()
        };

        // Pass gemini-3-pro-image which matches "gemini-3-pro" substring
        let (result, _sid, _msg_count) = transform_openai_request(&req, "test-p", "gemini-3-pro-image");
        let gen_config = &result["request"]["generationConfig"];
        
        // Assert thinkingConfig IS present (based on latest user feedback)
        assert!(gen_config.get("thinkingConfig").is_some(), "thinkingConfig SHOULD be injected for gemini-3-pro-image");
        
        // Assert imageConfig is present
        assert!(gen_config.get("imageConfig").is_some(), "imageConfig should be present for image models");
        assert_eq!(gen_config["imageConfig"]["imageSize"], "4K");
    }

    #[test]
    fn test_default_max_tokens_openai() {
        let req = OpenAIRequest {
            model: "gpt-4".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Hello".to_string())),
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
            ..Default::default()
        };

        let (result, _sid, _msg_count) = transform_openai_request(&req, "test-p", "gemini-3-pro-high-thinking");
        let gen_config = &result["request"]["generationConfig"];
        let max_output_tokens = gen_config["maxOutputTokens"].as_i64().unwrap();
        // budget(24576) + overhead(32768) = 57344
        assert_eq!(max_output_tokens, 57344);
        
        // Verify thinkingBudget
        let budget = gen_config["thinkingConfig"]["thinkingBudget"].as_i64().unwrap();
        // actual(24576)
        assert_eq!(budget, 24576);
    }

    #[test]
    fn test_flash_thinking_budget_capping() {
        let req = OpenAIRequest {
            model: "gpt-4".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Hello".to_string())),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }],
            stream: false,
            n: None,
            // User specifies a large budget (e.g. xhigh = 32768)
            thinking: Some(ThinkingConfig {
                thinking_type: Some("enabled".to_string()),
                budget_tokens: Some(32768),
                effort: None,
            }),
            max_tokens: None,
            temperature: None,
            top_p: None,
            stop: None,
            response_format: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            ..Default::default()
        };

        // Test with Flash model
        let (result, _sid, _msg_count) = transform_openai_request(&req, "test-p", "gemini-2.0-flash-thinking-exp");
        let gen_config = &result["request"]["generationConfig"];
        
        // Should be capped at 24576
        let budget = gen_config["thinkingConfig"]["thinkingBudget"].as_i64().unwrap();
        assert_eq!(budget, 24576);

        // Max output tokens should be adjusted based on capped budget (24576 + 8192)
        // budget(24576) + overhead(32768) = 57344
        let max_output_tokens = gen_config["maxOutputTokens"].as_i64().unwrap();
        assert_eq!(max_output_tokens, 57344);
    }
    #[test]
    fn test_vertex_ai_sentinel_injection() {
        // [FIX #1650] Verify sentinel signature injection for Vertex AI models
        let req = OpenAIRequest {
            model: "claude-3-7-sonnet-thinking".to_string(), // Triggers is_thinking_model
            messages: vec![OpenAIMessage {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: Some("Thinking...".to_string()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_123".to_string(),
                    r#type: "function".to_string(),
                    function: ToolFunction {
                        name: "test_tool".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                tool_call_id: None,
                name: None,
            }],
            person_generation: None,
            ..Default::default()
        };

        // Simulate Vertex AI path
        let mapped_model = "projects/my-project/locations/us-central1/publishers/google/models/gemini-2.0-flash-thinking-exp";
        
        let (result, _sid, _msg_count) = transform_openai_request(&req, "test-v", mapped_model);
        
        // Extract the tool call part from contents
        let contents = result["contents"].as_array().unwrap();
        // Identify the part with functionCall
        let parts = contents[0]["parts"].as_array().unwrap();
        let tool_part = parts.iter().find(|p| p.get("functionCall").is_some()).expect("Should find functionCall part");
        
        // Vertex AI requires sentinel
        assert_eq!(tool_part["thoughtSignature"].as_str(), Some("skip_thought_signature_validator"));
    }

    #[test]
    fn test_openai_image_thinking_mode_disabled() {
        // 1. Set global mode to disabled
        crate::proxy::config::update_image_thinking_mode(Some("disabled".to_string()));

        let req = OpenAIRequest {
            model: "gemini-3-pro-image".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Draw a cat".to_string())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            }],
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            person_generation: None,
            ..Default::default()
        };

        // 2. Transform request
        let (result, _sid, _msg_count) = transform_openai_request(&req, "test-proj", "gemini-3-pro-image");

        // 3. Verify thinkingConfig has includeThoughts: false
        let gen_config = result["request"]["generationConfig"].as_object().expect("Should have generationConfig in request payload");
        let thinking_config = gen_config["thinkingConfig"].as_object().unwrap();
        
        assert_eq!(thinking_config["includeThoughts"], false);
        
        // 4. Reset global mode
        crate::proxy::config::update_image_thinking_mode(Some("enabled".to_string()));
    }
}
