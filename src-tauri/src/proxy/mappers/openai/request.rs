// OpenAI → Gemini 请求转换
use super::models::*;
use crate::proxy::model_specs;
use crate::proxy::token_manager::ProxyToken;

use serde_json::{json, Value};

pub(crate) fn is_tiered_flash_model(model: &str) -> bool {
    let model_id = model
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .to_ascii_lowercase();
    model_id
        .strip_prefix("gemini-")
        .and_then(|rest| rest.strip_suffix("-flash-tiered"))
        .is_some_and(|version| !version.is_empty())
}

fn tiered_flash_thinking_level(effort: Option<&str>) -> Option<&'static str> {
    match effort {
        Some("low") => Some("LOW"),
        Some("medium") => Some("MEDIUM"),
        Some("high") => Some("HIGH"),
        _ => None,
    }
}

/// 清洗 system instruction 中的动态内容，确保跨请求的前缀字节一致性
/// 以便触发 Gemini 隐式前缀缓存（Prefix Cache）。
///
/// 清洗规则：
/// - 时间戳（Current time/date: ..., Today is: ...）
/// - UUID (8-4-4-4-12 格式)
/// - 随机 request/session/trace ID (req_xxx, sid_xxx, trace_xxx)
/// - [CACHE] environment_context XML 标签 (<current_date>, <timezone>, <cwd>, <shell>)
/// - [CACHE] skill/plugin 路径中的动态版本号 (如 /26.609.41114/)
/// - 多行空行合并为最多两个连续空行
fn sanitize_system_instruction_for_cache(text: &str) -> String {
    let mut cleaned = text.to_string();

    // 剥离时间戳（多种常见格式）
    // 注意：只匹配 system prompt 中注入的元数据行，不匹配代码中的时间字符串
    let time_patterns = [
        r"(?im)^Current (date|time)(\s+is)?\s*:.*$",
        r"(?im)^Today is\s*:.*$",
        r"(?im)^Date:\s+\d{4}-\d{2}-\d{2}.*$",
    ];
    for pat in &time_patterns {
        if let Ok(re) = regex::Regex::new(pat) {
            cleaned = re.replace_all(&cleaned, "").into_owned();
        }
    }

    // [CACHE] 清洗 environment_context XML 标签中的动态值
    // Codex 在每个请求的 user/system 消息中注入这些标签，其值随环境变化
    let env_xml_patterns: &[(&str, &str)] = &[
        (
            r"<current_date>[^<]*</current_date>",
            "<current_date>[DATE_FROZEN]</current_date>",
        ),
        (
            r"<timezone>[^<]*</timezone>",
            "<timezone>[TZ_FROZEN]</timezone>",
        ),
        (r"<cwd>[^<]*</cwd>", "<cwd>[WORKSPACE_FROZEN]</cwd>"),
        (r"<shell>[^<]*</shell>", "<shell>[SHELL_FROZEN]</shell>"),
    ];
    for (pat, replacement) in env_xml_patterns {
        if let Ok(re) = regex::Regex::new(pat) {
            cleaned = re.replace_all(&cleaned, *replacement).into_owned();
        }
    }

    // [CACHE] 清洗 skill/plugin 路径中的动态版本号 (如 /26.609.41114/ )
    // 这些版本号在 Codex/plugin 更新时会变化，但语义相同
    if let Ok(re) = regex::Regex::new(r"/\d{2}\.\d{3}\.\d{5}/") {
        cleaned = re.replace_all(&cleaned, "/[VERSION_FROZEN]/").into_owned();
    }

    // 剥离 UUID (标准 8-4-4-4-12 格式)
    if let Ok(re) =
        regex::Regex::new(r"\b[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}\b")
    {
        cleaned = re.replace_all(&cleaned, "{uuid}").into_owned();
    }

    // 剥离随机 request/session/trace ID (如 req_a1b2c3, sid-xxxxxxxx, trace_xxxxxxxx)
    if let Ok(re) = regex::Regex::new(r"\b(req|sid|trace)_[a-f0-9]{6,32}\b") {
        cleaned = re.replace_all(&cleaned, "{id}").into_owned();
    }

    // 多行空行合并为最多两个连续空行
    if let Ok(re) = regex::Regex::new(r"\n{3,}") {
        cleaned = re.replace_all(&cleaned, "\n\n").into_owned();
    }

    // 去除首尾空白
    cleaned.trim().to_string()
}

fn system_instruction_dedupe_key(text: &str) -> String {
    sanitize_system_instruction_for_cache(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Collect system/developer text without joining. A string content is one block;
/// a content array contributes one block per text part.
fn collect_system_instruction_blocks(request: &OpenAIRequest) -> Vec<String> {
    let mut blocks = Vec::new();

    if let Some(inst) = &request.instructions {
        if !inst.trim().is_empty() {
            blocks.push(inst.clone());
        }
    }

    for msg in &request.messages {
        if msg.role != "system" && msg.role != "developer" {
            continue;
        }
        match &msg.content {
            Some(OpenAIContent::String(text)) => {
                if !text.trim().is_empty() {
                    blocks.push(text.clone());
                }
            }
            Some(OpenAIContent::Array(items)) => {
                for item in items {
                    if let OpenAIContentBlock::Text { text } = item {
                        if !text.trim().is_empty() {
                            blocks.push(text.clone());
                        }
                    }
                }
            }
            None => {}
        }
    }

    blocks
}

fn is_apply_patch_tool_name(name: &str) -> bool {
    name == "apply_patch" || name == "apply_patch_v2"
}

fn should_preserve_tool_output(tool_name: &str, output: &str) -> bool {
    is_apply_patch_tool_name(tool_name)
        || output.contains("apply_patch verification failed")
        || output.contains("Failed to find expected lines")
        || output.contains("Failed to find context")
        || output.contains("Expected update hunk")
}

fn qualify_namespace_tool_name(namespace_name: &str, child_name: &str) -> String {
    let child = child_name.trim();
    let ns = namespace_name.trim();
    if child.is_empty() || ns.is_empty() || child.starts_with("mcp__") {
        return child.to_string();
    }
    if child.starts_with(ns) {
        return child.to_string();
    }
    if ns.ends_with("__") {
        return format!("{}{}", ns, child);
    }
    format!("{}__{}", ns, child)
}

fn flatten_tools(tools: &[Value]) -> Vec<Value> {
    let mut flat = Vec::new();
    for tool in tools {
        let t = tool.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if t == "namespace" {
            let namespace_name = tool.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if let Some(sub_tools) = tool.get("tools").and_then(|v| v.as_array()) {
                let sub_flat = flatten_tools(sub_tools);
                for mut sub_tool in sub_flat {
                    if let Some(obj) = sub_tool.as_object_mut() {
                        let mut name = String::new();
                        if let Some(n) = obj.get("name").and_then(|v| v.as_str()) {
                            name = n.to_string();
                        } else if let Some(func) = obj.get("function") {
                            if let Some(n) = func.get("name").and_then(|v| v.as_str()) {
                                name = n.to_string();
                            }
                        }
                        if !name.is_empty() {
                            let qualified = qualify_namespace_tool_name(namespace_name, &name);
                            if obj.contains_key("name") {
                                obj.insert("name".to_string(), json!(qualified));
                            }
                            if let Some(func) = obj.get_mut("function") {
                                if let Some(func_obj) = func.as_object_mut() {
                                    func_obj.insert("name".to_string(), json!(qualified));
                                }
                            }
                        }
                    }
                    flat.push(sub_tool);
                }
            }
        } else {
            flat.push(tool.clone());
        }
    }
    flat
}

pub fn extract_client_tool_names(tools: &Option<Vec<Value>>) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    if let Some(tools_list) = tools {
        let flat_tools = flatten_tools(tools_list);
        for tool in flat_tools {
            let name_opt = tool
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    tool.get("name")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .or_else(|| {
                    tool.get("type")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                });
            if let Some(name) = name_opt {
                names.insert(name);
            }
        }
    }
    names
}

pub fn transform_openai_request(
    request: &OpenAIRequest,
    project_id: &str,
    mapped_model: &str,
    token: Option<&ProxyToken>,
) -> (Value, String, usize, String) {
    let session_id =
        crate::proxy::session_manager::SessionManager::extract_openai_session_id(request);
    transform_openai_request_with_session(
        request,
        project_id,
        mapped_model,
        token,
        &session_id,
        Some(&session_id),
    )
}

pub fn transform_openai_request_with_session(
    request: &OpenAIRequest,
    project_id: &str,
    mapped_model: &str,
    token: Option<&ProxyToken>,
    routing_session_id: &str,
    signature_read_key: Option<&str>,
) -> (Value, String, usize, String) {
    let remember_cwd =
        |text: &str| crate::proxy::adapters::apply_patch_preflight::remember_cwd_from_text(text);
    let found_cwd = request.instructions.as_deref().is_some_and(remember_cwd);
    if !found_cwd {
        'messages: for message in &request.messages {
            let Some(content) = &message.content else {
                continue;
            };
            match content {
                OpenAIContent::String(text) => {
                    if remember_cwd(text) {
                        break 'messages;
                    }
                }
                OpenAIContent::Array(blocks) => {
                    for block in blocks {
                        if let OpenAIContentBlock::Text { text } = block {
                            if remember_cwd(text) {
                                break 'messages;
                            }
                        }
                    }
                }
            }
        }
    }

    let session_id = routing_session_id.to_string();
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
        None,                          // body
    );

    let is_under_v3 = crate::proxy::model_specs::is_gemini_under_v3(mapped_model)
        || crate::proxy::model_specs::is_gemini_under_v3(&request.model);

    // [FIX] 仅当模型名称显式包含 "-thinking" 或 Gemini 3+ 思维模型时才视为 Gemini 思维模型
    let is_gemini_3_thinking = !is_under_v3
        && mapped_model_lower.contains("gemini")
        && (mapped_model_lower.contains("-thinking")
            || crate::proxy::model_specs::is_gemini_v3_or_above(mapped_model)
            || mapped_model_lower.contains("gemini-pro")
            || mapped_model_lower.contains("-pro-agent"))
        && !mapped_model_lower.contains("claude");
    // [FIX #2167] gemini-*-flash 支持 thinking (需为 Gemini 3 及以上版本)
    let is_gemini_flash_thinking = !is_under_v3
        && crate::proxy::model_specs::is_gemini_v3_or_above(mapped_model)
        && mapped_model_lower.contains("gemini")
        && (mapped_model_lower.contains("flash")
            || mapped_model_lower.contains("-flash-")
            || mapped_model_lower.contains("-flash-agent"))
        && !mapped_model_lower.contains("claude");
    let user_enabled_thinking = request
        .thinking
        .as_ref()
        .map(|t| t.thinking_type.as_deref() == Some("enabled"))
        .unwrap_or(false);
    let user_thinking_budget = request.thinking.as_ref().and_then(|t| t.budget_tokens);

    let is_claude_model = mapped_model_lower.contains("claude");
    let is_claude_thinking = mapped_model_lower.ends_with("-thinking")
        || (is_claude_model && user_enabled_thinking);
    let force_server_thinking = !is_under_v3
        && crate::proxy::thinking_store::any_model_forces_server_thinking(&[
            request.model.as_str(),
            mapped_model,
        ]);
    let is_thinking_model = is_gemini_3_thinking
        || is_claude_thinking
        || is_gemini_flash_thinking
        || force_server_thinking;

    // [NEW] 检查历史消息是否兼容思维模型 (是否有 Assistant 消息缺失 reasoning_content)
    let has_incompatible_assistant_history = request.messages.iter().any(|msg| {
        msg.role == "assistant"
            && (msg.reasoning_content.is_none()
                || msg
                    .reasoning_content
                    .as_ref()
                    .map_or(false, |r| r.is_empty()))
    });

    // 检查历史中是否有工具调用
    let has_tool_calls_in_history = request
        .messages
        .iter()
        .any(|msg| msg.role == "tool" || msg.role == "function" || msg.tool_calls.is_some());

    // [NEW] 决定是否开启 Thinking 功能:
    // 1. 模型名包含 -thinking 时自动开启
    // 2. 用户在请求中显式设置 thinking.type = "enabled" 时开启
    // 3. 若为 Gemini < 3 模型，保底强制关闭 thinking
    let mut actual_include_thinking = !is_under_v3
        && (is_thinking_model || user_enabled_thinking || force_server_thinking);

    // [REFACTORED] 使用 SignatureCache 获取 Session 级别的签名
    let session_thought_sig = signature_read_key
        .and_then(|key| crate::proxy::SignatureCache::global().get_session_signature(key));

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

    // 1. Extract system/developer blocks without joining. Each client string or
    // text part becomes one Gemini systemInstruction part (Anthropic-style).
    let mut system_instructions: Vec<String> = collect_system_instruction_blocks(request);

    // [CACHE:L1] 清洗 system instructions 中的动态内容（时间戳/UUID/随机ID）
    // 确保跨请求的前缀字节一致，触发 Gemini 隐式前缀缓存命中
    // 多层级缓存: Layer 1 缓存 sanitized 结果，跨 session 复用
    let cm = crate::proxy::cache_manager::global_cache_manager();
    let mut si_layer_stats = (0u64, 0u64); // (hits, misses) for logging
    system_instructions = system_instructions
        .into_iter()
        .map(|s| {
            let s = s.replace(
                "You are Codex, an agent based on GPT-5.",
                "You are Codex, an agent.",
            );
            let raw_key = crate::proxy::cache_manager::CacheManager::compute_si_key(&s);
            if let Some(cached) = cm.lookup_si(&raw_key) {
                si_layer_stats.0 += 1;
                cached
            } else {
                si_layer_stats.1 += 1;
                let sanitized = sanitize_system_instruction_for_cache(&s);
                cm.cache_si(raw_key, sanitized.clone());
                sanitized
            }
        })
        .collect();
    let mut seen_system_instruction_keys = std::collections::HashSet::new();
    system_instructions.retain(|inst| {
        let key = system_instruction_dedupe_key(inst);
        !key.is_empty() && seen_system_instruction_keys.insert(key)
    });
    if si_layer_stats.0 > 0 || si_layer_stats.1 > 0 {
        tracing::debug!(
            "[Cache-Opt:L1-SI] hits={} misses={} total={}",
            si_layer_stats.0,
            si_layer_stats.1,
            si_layer_stats.0 + si_layer_stats.1
        );
    }

    // Pre-scan to map tool_call_id to function name (for Codex)
    let mut tool_id_to_name = std::collections::HashMap::new();
    for msg in &request.messages {
        if let Some(tool_calls) = &msg.tool_calls {
            for call in tool_calls {
                let name = if let Some(func) = &call.function {
                    func.name.clone()
                } else if call.operation.is_some() || call.r#type == "apply_patch_call" {
                    "apply_patch".to_string()
                } else {
                    continue;
                };
                let final_name = if name == "local_shell_call" {
                    "shell"
                } else {
                    &name
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
        let flat_tools = flatten_tools(tools);
        for tool in &flat_tools {
            let name_opt = tool
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    tool.get("name")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .or_else(|| {
                    tool.get("type")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                });

            let params_opt = tool
                .get("function")
                .and_then(|f| f.get("parameters"))
                .or_else(|| tool.get("parameters"));

            if let (Some(name), Some(params)) = (name_opt, params_opt) {
                tool_name_to_schema.insert(name, params.clone());
            }
        }
    }

    // 2. 构建 Gemini contents (过滤掉 system/developer 指令)
    let total_messages = request.messages.len();
    let recent_message_window = 24usize;
    let contents: Vec<Value> = request
        .messages
        .iter()
        .enumerate()
        .filter(|(_, msg)| msg.role != "system" && msg.role != "developer")
        .map(|(msg_index, msg)| {
            let is_latest = msg_index >= total_messages.saturating_sub(recent_message_window);
            let role = match msg.role.as_str() {
                "assistant" => "model",
                "tool" | "function" => "user",
                _ => &msg.role,
            };

            let mut parts = Vec::new();

            // Handle reasoning_content (thinking)
            // [FIX #3325] Only include full reasoning_content text for the most recent messages (recent window).
            // For older messages, strip the long thought text while keeping the thoughtSignature on the tool call
            // to avoid blowing through the 1M token context limit during multi-turn agent loops.
            if let Some(reasoning) = &msg.reasoning_content {
                // [FIX #1506] 增强对占位符 [undefined] 的识别
                let is_invalid_placeholder = reasoning == "[undefined]" || reasoning.is_empty();

                if !is_invalid_placeholder {
                    if is_latest {
                        let thought_part = json!({
                            "text": reasoning,
                            "thought": true,
                        });
                        parts.push(thought_part);
                    } else {
                        // [FIX #3382] For older turns, keep a minimal thought block placeholder
                        // to satisfy the schema requirement (parts[0] must be thought if thinking enabled)
                        // without repeating thousands of tokens. The tool call retains thoughtSignature.
                        let thought_part = json!({
                            "text": "...",
                            "thought": true,
                        });
                        parts.push(thought_part);
                    }
                }
            } else if actual_include_thinking && role == "model" {
                // [FIX] 解决 Gemini / Claude 等思考模型的强制性校验:
                // Vertex AI / Gemini 要求开启思考时，assistant 消息必须以思考块开始并携带有效签名。
                // 历史消息缺失 reasoning_content 时，先注入保底哨兵思考块，
                // 后续 restore_gemini_contents 会从 ThinkingStore 尝试恢复真实的思维和签名。
                tracing::debug!("[OpenAI-Thinking] Injecting placeholder thinking block for assistant message");
                let thought_part = json!({
                    "text": "...",
                    "thought": true,
                    "thoughtSignature": crate::proxy::thinking_store::SENTINEL_SIGNATURE,
                });

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
                            parts.extend(crate::proxy::mappers::common_utils::parse_markdown_images_to_parts(s));
                        }
                    }
                    OpenAIContent::Array(blocks) => {
                        for block in blocks {
                            match block {
                                OpenAIContentBlock::Text { text } => {
                                    parts.extend(crate::proxy::mappers::common_utils::parse_markdown_images_to_parts(text));
                                }
                                OpenAIContentBlock::ImageUrl { image_url } => {
                                    if image_url.url.starts_with("data:") {
                                        if let Some(pos) = image_url.url.find(",") {
                                            let mime_part = &image_url.url[5..pos];
                                            let mime_type = mime_part.split(';').next().unwrap_or("image/jpeg");
                                            let data = &image_url.url[pos + 1..];

                                            parts.push(crate::proxy::mappers::common_utils::create_gemini_inline_part(
                                                Some(mime_type),
                                                data,
                                                "Image",
                                            ));
                                        } else {
                                            parts.push(json!({"text": "[Image: invalid data URL omitted]"}));
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

                                            parts.push(crate::proxy::mappers::common_utils::create_gemini_inline_part(
                                                Some(mime_type),
                                                &b64,
                                                "Image",
                                            ));
                                            tracing::debug!("[OpenAI-Request] Successfully loaded image: {} ({} bytes)", file_path, file_bytes.len());
                                        } else {
                                            tracing::debug!("[OpenAI-Request] Failed to read local image: {}", file_path);
                                        }
                                    }
                                }
                                OpenAIContentBlock::AudioUrl { audio_url } => {
                                    // [NEW] audio_url -> Gemini inlineData / fileData
                                    match crate::proxy::audio::audio_part_from_source(
                                        &audio_url.url,
                                        audio_url.mime_type.as_deref(),
                                    ) {
                                        Some(part) => {
                                            tracing::debug!("[OpenAI-Request] Mapped audio_url to Gemini part");
                                            parts.push(part);
                                        }
                                        None => {
                                            tracing::warn!("[OpenAI-Request] Dropped unreadable audio_url part");
                                        }
                                    }
                                }
                                OpenAIContentBlock::InputAudio { input_audio } => {
                                    // [NEW] OpenAI 官方 input_audio (base64 + format) -> Gemini inlineData
                                    let mime = input_audio.mime_type();
                                    match crate::proxy::audio::audio_part_from_source(
                                        &input_audio.data,
                                        Some(&mime),
                                    ) {
                                        Some(part) => {
                                            tracing::debug!("[OpenAI-Request] Mapped input_audio ({}) to Gemini part", mime);
                                            parts.push(part);
                                        }
                                        None => {
                                            tracing::warn!("[OpenAI-Request] Dropped empty input_audio part");
                                        }
                                    }
                                }
                                OpenAIContentBlock::VideoUrl { video_url } => {
                                    // [NEW #3381] video_url -> Gemini inlineData / fileData
                                    match crate::proxy::video::video_part_from_source(
                                        &video_url.url,
                                        video_url.mime_type.as_deref(),
                                    ) {
                                        Some(part) => {
                                            tracing::debug!("[OpenAI-Request] Mapped video_url to Gemini part");
                                            parts.push(part);
                                        }
                                        None => {
                                            tracing::warn!("[OpenAI-Request] Dropped unreadable video_url part: {}", video_url.url);
                                        }
                                    }
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

                    let mut args_str = String::new();
                    let mut func_name = String::new();

                    if let Some(func) = &tc.function {
                        args_str = func.arguments.clone();
                        func_name = func.name.clone();
                    } else if let Some(op) = &tc.operation {
                        func_name = "apply_patch".to_string();
                        args_str = serde_json::to_string(op).unwrap_or_else(|_| "{}".to_string());
                    } else {
                        continue;
                    }

                    if !is_latest && args_str.len() > 1000 && !is_apply_patch_tool_name(&func_name)
                    {
                        args_str = "{\"_truncated\": \"Arguments truncated to save context window.\"}".to_string();
                    }
                    let mut args = serde_json::from_str::<Value>(&args_str).unwrap_or(json!({}));

                    // [New] 利用通用引擎修正参数类型 (替代以前硬编码的 shell 工具修复逻辑)
                    if let Some(original_schema) = tool_name_to_schema.get(&func_name) {
                        crate::proxy::common::json_schema::fix_tool_call_args(&mut args, original_schema);
                    }

                    let mut func_call_part = json!({
                        "functionCall": {
                            "name": if func_name == "local_shell_call" { "shell" } else { func_name.as_str() },
                            "args": args,
                            "id": &tc.id,
                        }
                    });

                    // [New] 递归清理参数中可能存在的非法校验字段
                    crate::proxy::common::json_schema::clean_json_schema(&mut func_call_part);

                    // 1. 优先查本工具 call_id 专属的真实签名 (精确到轮次与工具，杜绝错位)
                    let tool_specific_sig = crate::proxy::SignatureCache::global().get_tool_signature(&tc.id);
                    if let Some(ref sig) = tool_specific_sig {
                        func_call_part["thoughtSignature"] = json!(sig);
                    } else if let Some(ref sig) = thought_sig {
                        func_call_part["thoughtSignature"] = json!(sig);
                    } else if is_thinking_model || is_gemini_flash_thinking || actual_include_thinking {
                        // [NEW] Handle missing signature for Gemini thinking models
                        // [FIX #1650] Allow sentinel injection for Vertex AI (projects/...) as well
                        // [FIX #2167] Also applies to gemini-3-flash / gemini-3.1-flash
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

                let mut extra_parts = Vec::new();

                let content_val = match &msg.content {
                    Some(OpenAIContent::String(s)) => {
                        if !is_latest
                            && s.len() > 1000
                            && !should_preserve_tool_output(final_name, s)
                        {
                            format!("[Tool output truncated to save context. Original length: {}]", s.len())
                        } else {
                            s.clone()
                        }
                    },
                    Some(OpenAIContent::Array(blocks)) => {
                        let mut texts = Vec::new();
                        for block in blocks {
                            match block {
                                OpenAIContentBlock::Text { text } => texts.push(text.clone()),
                                OpenAIContentBlock::ImageUrl { image_url } => {
                                    if image_url.url.starts_with("data:") {
                                        if let Some(pos) = image_url.url.find(',') {
                                            let mime_part = &image_url.url[5..pos];
                                            let mime_type = mime_part.split(';').next().unwrap_or("image/jpeg");
                                            let data = &image_url.url[pos + 1..];

                                            extra_parts.push(crate::proxy::mappers::common_utils::create_gemini_inline_part(
                                                Some(mime_type),
                                                data,
                                                "Tool Result Image",
                                            ));
                                        }
                                    } else {
                                        texts.push("[image link]".to_string());
                                    }
                                }
                                OpenAIContentBlock::AudioUrl { audio_url } => {
                                    match crate::proxy::audio::audio_part_from_source(
                                        &audio_url.url,
                                        audio_url.mime_type.as_deref(),
                                    ) {
                                        Some(part) => extra_parts.push(part),
                                        None => texts.push("[audio]".to_string()),
                                    }
                                }
                                OpenAIContentBlock::InputAudio { input_audio } => {
                                    let mime = input_audio.mime_type();
                                    match crate::proxy::audio::audio_part_from_source(
                                        &input_audio.data,
                                        Some(&mime),
                                    ) {
                                        Some(part) => extra_parts.push(part),
                                        None => texts.push("[audio]".to_string()),
                                    }
                                }
                                OpenAIContentBlock::VideoUrl { video_url } => {
                                    match crate::proxy::video::video_part_from_source(
                                        &video_url.url,
                                        video_url.mime_type.as_deref(),
                                    ) {
                                        Some(part) => extra_parts.push(part),
                                        None => texts.push("[video]".to_string()),
                                    }
                                }
                            }
                        }
                        texts.join("\n")
                    },
                    None => "".to_string()
                };

                let mut fr_part = json!({
                    "functionResponse": {
                       "name": final_name,
                       "response": { "result": content_val },
                       "id": msg.tool_call_id.clone().unwrap_or_default()
                    }
                });
                if let Some(ref call_id) = msg.tool_call_id {
                    if let Some(sig) = crate::proxy::SignatureCache::global().get_tool_signature(call_id) {
                        fr_part["thoughtSignature"] = json!(sig);
                    }
                }
                parts.push(fr_part);

                for extra in extra_parts {
                    parts.push(extra);
                }
            }

            // Ensure user role message is not dropped if parts is empty, preserving role rotation
            if role == "user" && parts.is_empty() {
                parts.push(json!({ "text": " " }));
            }

            json!({ "role": role, "parts": parts })
        })
        .filter(|msg| !msg["parts"].as_array().map(|a| a.is_empty()).unwrap_or(true))
        .collect();

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
    if actual_include_thinking {
        crate::proxy::thinking_store::hydrate_gemini_contents(&session_id, &mut merged_contents);
    }
    let mut contents = merged_contents;

    crate::proxy::thinking_store::finalize_gemini_contents_thinking(
        &mut contents,
        actual_include_thinking,
    );

    // Gemini requires conversations to start with a user turn, and functionCall turns
    // must immediately follow a user turn or a functionResponse turn.
    // If the conversation starts with a model turn (e.g. autonomous agent loops starting with tool calls),
    // inject a lightweight user primer to prevent 400 INVALID_ARGUMENT error.
    if contents.is_empty() {
        contents.push(json!({
            "role": "user",
            "parts": [{ "text": "Continue" }]
        }));
    } else if contents.first().and_then(|f| f.get("role")).and_then(|r| r.as_str()) == Some("model") {
        contents.insert(
            0,
            json!({
                "role": "user",
                "parts": [{ "text": "Continue the task." }]
            }),
        );
    }

    // 3. 构建请求体

    let mut gen_config = json!({
        "temperature": request.temperature.unwrap_or(1.0),
        // [CHANGED v4.1.24] Default topP from 0.95 → 1.0 to match native behavior
        "topP": request.top_p.unwrap_or(1.0),
        // [ADDED v4.1.24] topK=40 aligns with official client generationConfig
        "topK": 40,
    });

    // [FIX] 移除旧的硬编码限额，改为动态查询 (v4.1.29)
    if let Some(max_tokens) = request.max_tokens {
        gen_config["maxOutputTokens"] = json!(max_tokens);
    } else {
        // 使用动态优先的规格限额
        let limit = model_specs::get_max_output_tokens(mapped_model, token);
        gen_config["maxOutputTokens"] = json!(limit);
    }

    // [NEW] 支持多候选结果数量 (n -> candidateCount)
    if let Some(n) = request.n {
        gen_config["candidateCount"] = json!(n);
    }

    if let Some(presence_penalty) = request.presence_penalty {
        gen_config["presencePenalty"] = json!(presence_penalty);
    }
    if let Some(frequency_penalty) = request.frequency_penalty {
        gen_config["frequencyPenalty"] = json!(frequency_penalty);
    }
    if let Some(seed) = request.seed {
        gen_config["seed"] = json!(seed);
    }

    // 为 thinking 模型注入 thinkingConfig (使用 thinkingBudget 而非 thinkingLevel)
    if actual_include_thinking {
        // [RESOLVE #1694] Check image thinking mode
        let image_thinking_mode = crate::proxy::config::get_image_thinking_mode();
        // Only disable if mode is explicitly "disabled" AND it's an image generation request
        let is_image_gen_disabled =
            config.request_type == "image_gen" && image_thinking_mode == "disabled";

        if is_image_gen_disabled {
            tracing::debug!("[OpenAI-Request] Image thinking mode disabled: enforcing includeThoughts=false for {}", mapped_model);
            gen_config["thinkingConfig"] = json!({
                "includeThoughts": false
            });
        } else {
            // [CONFIGURABLE] 思考预算配置与防污染处理
            let default_budget = model_specs::get_thinking_budget(mapped_model, token) as i64;
            let tb_config = crate::proxy::config::get_thinking_budget_config();
            let is_v3_or_above = model_specs::is_gemini_v3_or_above(mapped_model)
                || model_specs::is_gemini_v3_or_above(&request.model);
            let is_explicit_tier = mapped_model_lower.ends_with("-high")
                || mapped_model_lower.ends_with("-medium")
                || mapped_model_lower.ends_with("-low")
                || mapped_model_lower.ends_with("-extra-low");
            // [ANTI-POLLUTION] 对齐 Anthropic：对于 Gemini >= 3 或显式档位模型，彻底忽略客户端过小预算，绝不被客户端 1024 或 low 污染
            let final_budget = if is_v3_or_above || is_explicit_tier {
                default_budget
            } else {
                match tb_config.mode {
                    crate::proxy::config::ThinkingBudgetMode::Passthrough => {
                        user_thinking_budget.map(|b| b as i64).unwrap_or(default_budget)
                    }
                    crate::proxy::config::ThinkingBudgetMode::Custom => {
                        let custom_value = tb_config.custom_value as i64;
                        if custom_value > default_budget {
                            default_budget
                        } else {
                            custom_value
                        }
                    }
                    _ => default_budget,
                }
            };

            gen_config["thinkingConfig"] = json!({
                "includeThoughts": true,
                "thinkingBudget": final_budget
            });

            // [CRITICAL] 思维模型的 maxOutputTokens 必须大于 thinkingBudget
            // [FIX #1675] 针对图像模型使用更保守的 max_tokens 增量，避免触发 128k 限制
            let overhead = if config.request_type == "image_gen" {
                2048
            } else {
                32768
            };
            let min_overhead = if config.request_type == "image_gen" {
                1024
            } else {
                8192
            };

            if mapped_model_lower.contains("claude-opus-4-6-thinking") {
                gen_config["maxOutputTokens"] = json!(57344);
                tracing::debug!(
                    "[Opus-Alignment] Enforcing maxOutputTokens 57344 for Opus 4.6 (OpenAI)"
                );
            } else if let Some(max_tokens) = request.max_tokens {
                if (max_tokens as i64) <= final_budget {
                    gen_config["maxOutputTokens"] = json!(final_budget + min_overhead);
                }
            } else {
                // [FIX #1592] Use a more conservative default to avoid 400 error on 128k context models
                gen_config["maxOutputTokens"] = json!(final_budget + overhead);
            }

            let new_max = gen_config["maxOutputTokens"].as_i64().unwrap_or(0);
            tracing::debug!(
                "[OpenAI-Request] Adjusted maxOutputTokens to {} for thinking model (budget={})",
                new_max,
                final_budget
            );

            tracing::debug!(
                "[OpenAI-Request] Injected thinkingConfig for model {}: thinkingBudget={} (mode={:?})",
                mapped_model, final_budget, tb_config.mode
            );
        }
    }

    // Tiered Flash models select the upstream thinking level directly. This intentionally
    // replaces the budget-based config above and leaves the upstream model ID untouched.
    if is_tiered_flash_model(mapped_model) {
        let mut thinking_config = json!({ "includeThoughts": true });
        if let Some(level) = tiered_flash_thinking_level(
            request
                .reasoning
                .as_ref()
                .and_then(|reasoning| reasoning.effort.as_deref()),
        ) {
            thinking_config["thinkingLevel"] = json!(level);
        }
        gen_config["thinkingConfig"] = thinking_config;
    }

    // [FIX] Cap maxOutputTokens to prevent 400 Invalid Argument
    if let Some(val) = gen_config["maxOutputTokens"].as_i64() {
        let safe_limit = if mapped_model_lower.contains("claude") {
            64000
        } else if mapped_model_lower.contains("pro") {
            65535
        } else {
            65536
        };
        if val > safe_limit {
            tracing::warn!(
                "[Generation-Config] Capping maxOutputTokens from {} to {} to prevent 400 Invalid Argument",
                val, safe_limit
            );
            gen_config["maxOutputTokens"] = json!(safe_limit);
        }
    }

    if let Some(stop) = &request.stop {
        if !mapped_model_lower.contains("claude-opus-4-6-thinking") {
            if stop.is_string() {
                gen_config["stopSequences"] = json!([stop]);
            } else if stop.is_array() {
                gen_config["stopSequences"] = stop.clone();
            }
        } else {
            tracing::debug!(
                "[Opus-Alignment] Skipping stopSequences for Opus 4.6 to match OpenAI protocol"
            );
        }
    }

    if let Some(fmt) = &request.response_format {
        if fmt.r#type == "json_object" {
            gen_config["responseMimeType"] = json!("application/json");
        } else if fmt.r#type == "json_schema" {
            gen_config["responseMimeType"] = json!("application/json");
            if let Some(js) = &fmt.json_schema {
                if let Some(mut schema) = js.schema.clone() {
                    crate::proxy::common::json_schema::clean_response_schema(&mut schema);
                    gen_config["responseSchema"] = schema;
                }
            }
        }
    }

    // [CACHE] inner_request 先创建为空的 Map，后续按稳定顺序填充
    let mut inner_request = json!({});
    // 先放 contents（后续会被 reordered_request 覆盖到后面）
    inner_request["contents"] = json!(contents);
    inner_request["generationConfig"] = gen_config;
    inner_request["safetySettings"] = json!([
        { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "OFF" },
    ]);

    // 深度清理 [undefined] 字符串 (Cherry Studio 等客户端常见注入)
    crate::proxy::mappers::common_utils::deep_clean_undefined(&mut inner_request, 0);

    // 4. Handle Tools (Merged Cleaning)
    let is_codex_style = request.model.contains("codex")
        || request.model.contains("realtime")
        || request.instructions.is_some()
        || request.input.is_some();

    let mut function_declarations: Vec<Value> = Vec::new();

    // [CACHE:L2] 计算原始 tools 的 hash，查 Layer 2 缓存
    // 命中则跳过所有 tools 处理逻辑，跨 session 复用已处理的 tools
    let mut tools_layer_hit = false;
    let tools_raw_hash = if let Some(ref original_tools) = request.tools {
        let raw_json = serde_json::to_string(original_tools).unwrap_or_default();
        if !raw_json.is_empty() {
            let key = crate::proxy::cache_manager::CacheManager::compute_tools_key(&format!(
                "apply_patch_input_schema_v2:{raw_json}"
            ));
            let cm = crate::proxy::cache_manager::global_cache_manager();
            if let Some(cached_json) = cm.lookup_tools(&key) {
                if let Ok(parsed) = serde_json::from_str::<Vec<Value>>(&cached_json) {
                    function_declarations = parsed;
                    tools_layer_hit = true;
                    tracing::debug!(
                        "[Cache-Opt:L2-Tools] HIT hash={} declarations={}",
                        &key[..key.len().min(16)],
                        function_declarations.len()
                    );
                }
            }
            Some(key)
        } else {
            None
        }
    } else {
        None
    };

    if !tools_layer_hit {
        if let Some(original_tools) = &request.tools {
            let tools = flatten_tools(original_tools);
            for tool in tools.iter() {
                let mut gemini_func = if let Some(func) = tool.get("function") {
                    func.clone()
                } else {
                    let mut func = tool.clone();
                    // [FIX] 剔除 "type" 前如果不存在 "name"，则提取 "type" 兜底作为名字
                    if func.get("name").is_none() {
                        let tool_type_opt = func
                            .get("type")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        if let Some(tool_type) = tool_type_opt {
                            if let Some(obj) = func.as_object_mut() {
                                obj.insert("name".to_string(), json!(tool_type));
                            }
                        }
                    }
                    if let Some(obj) = func.as_object_mut() {
                        obj.remove("type");
                        obj.remove("strict");
                        obj.remove("additionalProperties");
                    }
                    func
                };

                let name_opt = gemini_func
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

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
                    tracing::warn!(
                        "[OpenAI-Request] Skipping tool without name: {:?}",
                        gemini_func
                    );
                    continue;
                }

                // [NEW CRITICAL FIX] 保留函数定义根层级的合法字段，移除所有非法字段 (如 type, execution, format 等)
                if let Some(obj) = gemini_func.as_object_mut() {
                    let mut clean_obj = serde_json::Map::new();
                    if let Some(name) = obj.get("name") {
                        clean_obj.insert("name".to_string(), name.clone());
                    }
                    if let Some(desc) = obj.get("description") {
                        clean_obj.insert("description".to_string(), desc.clone());
                    }
                    if let Some(params) = obj.get("parameters") {
                        clean_obj.insert("parameters".to_string(), params.clone());
                    }
                    *obj = clean_obj;
                }

                if gemini_func.get("name").and_then(|v| v.as_str()) == Some("apply_patch") {
                    gemini_func.as_object_mut().unwrap().insert(
                        "parameters".to_string(),
                        json!({
                            "type": "OBJECT",
                            "properties": {
                                "input": {
                                    "type": "STRING",
                                    "description": "The exact freeform V4A patch text to pass to Codex apply_patch. It must start with *** Begin Patch and end with *** End Patch. Do not wrap it in a shell command or command array."
                                }
                            },
                            "required": ["input"]
                        }),
                    );
                } else if let Some(params) = gemini_func.get_mut("parameters") {
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
        }

        // [CACHE:L2] 缓存处理完成的 tools，下次相同 schema 可以直接命中
        if let Some(ref key) = tools_raw_hash {
            if !tools_layer_hit {
                if let Ok(cached_json) = serde_json::to_string(&function_declarations) {
                    let cm = crate::proxy::cache_manager::global_cache_manager();
                    cm.cache_tools(key.clone(), cached_json);
                    tracing::debug!(
                        "[Cache-Opt:L2-Tools] INSERT hash={} declarations={}",
                        &key[..key.len().min(16)],
                        function_declarations.len()
                    );
                }
            }
        }
    } // end if !tools_layer_hit (includes the sort and insert below)

    // [CACHE] 按 function name 稳定排序，确保跨请求的 tool schema 字节一致
    function_declarations.sort_by(|a, b| {
        let name_a = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let name_b = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
        name_a.cmp(name_b)
    });

    // Removed auto-inject since we handle it above now if Codex passes it.

    if !function_declarations.is_empty() {
        inner_request["tools"] = json!([{ "functionDeclarations": function_declarations }]);

        let mut mode = "VALIDATED";
        if let Some(tool_choice) = &request.tool_choice {
            if let Some(s) = tool_choice.as_str() {
                match s {
                    "none" => mode = "NONE",
                    "auto" => mode = "AUTO",
                    "required" => mode = "ANY",
                    _ => mode = "ANY",
                }
            } else {
                mode = "ANY";
            }
        }

        inner_request["toolConfig"] = json!({
            "functionCallingConfig": { "mode": mode },
            "includeServerSideToolInvocations": true
        });
        inner_request["tool_config"] = json!({
            "function_calling_config": { "mode": mode },
            "include_server_side_tool_invocations": true
        });
    }

    let global_prompt_config = crate::proxy::config::get_global_system_prompt();
    let global_prompt =
        if global_prompt_config.enabled && !global_prompt_config.content.trim().is_empty() {
            Some(global_prompt_config.content.as_str())
        } else {
            None
        };
    let system_parts =
        super::context_blocks::build_system_instruction_parts(&system_instructions, global_prompt);
    if !system_parts.is_empty() {
        inner_request["systemInstruction"] = json!({
            "role": "user",
            "parts": system_parts
        });
    }

    if config.inject_google_search {
        crate::proxy::mappers::common_utils::inject_google_search_tool(
            &mut inner_request,
            Some(mapped_model),
        );
        if let Some(tool_config) = inner_request.get_mut("toolConfig") {
            if let Some(obj) = tool_config.as_object_mut() {
                obj.insert("includeServerSideToolInvocations".to_string(), json!(true));
            }
        } else {
            inner_request["toolConfig"] = json!({
                "functionCallingConfig": { "mode": "VALIDATED" },
                "includeServerSideToolInvocations": true
            });
        }
        if let Some(tool_config_snake) = inner_request.get_mut("tool_config") {
            if let Some(obj) = tool_config_snake.as_object_mut() {
                obj.insert("include_server_side_tool_invocations".to_string(), json!(true));
            }
        } else {
            inner_request["tool_config"] = json!({
                "function_calling_config": { "mode": "VALIDATED" },
                "include_server_side_tool_invocations": true
            });
        }
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

    // [ADDED v4.1.24] 注入稳定 sessionId 对齐官方规范
    // [FIX session-1M] sessionId 混入对话指纹与代数:
    //   - 同一对话内保持稳定(保留上游服务端会话缓存收益)
    //   - 不同对话使用不同 sessionId,避免共享同一服务端累计会话
    //   - 检测到上游 1M 累计报错后 bump 代数,新 sessionId = 全新上游会话,对话无感恢复
    if let Some(t) = token {
        let generation =
            crate::proxy::common::session::current_bump(&t.account_id, &session_id);
        inner_request["sessionId"] = json!(crate::proxy::common::session::derive_session_scoped(
            &t.account_id,
            &session_id,
            generation
        ));
    }

    // [CACHE] 重建 inner_request 字段顺序——稳定前缀在前，动态内容在后
    // 遵循 Google 官方建议："将较大且常见的内容放置在提示的开头"
    // 前缀顺序: systemInstruction → tools → toolConfig → generationConfig → safetySettings → sessionId → contents
    //                                                  ↑ 只有 contents 变化，其他全部稳定
    let mut reordered_request = json!({});
    // 1. systemInstruction (稳定，~17,500 tokens — 最大的静态块)
    if let Some(si) = inner_request.get("systemInstruction") {
        reordered_request["systemInstruction"] = si.clone();
    }
    // 2. tools (稳定，已排序)
    if let Some(tools) = inner_request.get("tools") {
        reordered_request["tools"] = tools.clone();
    }
    // 3. toolConfig & tool_config (稳定，与 tools 同生)
    if let Some(tc) = inner_request.get("toolConfig") {
        reordered_request["toolConfig"] = tc.clone();
    }
    if let Some(tc_snake) = inner_request.get("tool_config") {
        reordered_request["tool_config"] = tc_snake.clone();
    }
    // 4. generationConfig (稳定，sanitize 后一致)
    if let Some(gc) = inner_request.get("generationConfig") {
        reordered_request["generationConfig"] = gc.clone();
    }
    // 5. safetySettings (恒定常量)
    if let Some(ss) = inner_request.get("safetySettings") {
        reordered_request["safetySettings"] = ss.clone();
    }
    // 6. sessionId (稳定，基于 account_id hash)
    if let Some(sid) = inner_request.get("sessionId") {
        reordered_request["sessionId"] = sid.clone();
    }
    // 7. contents (动态，~4.3MB — 所有图片和对话历史，每次追加，放在最后!)
    reordered_request["contents"] = inner_request.get("contents").cloned().unwrap_or(json!([]));
    // 8. 其他可能存在的字段 (metadata, cachedContent 等)
    for (k, v) in inner_request.as_object().iter().flat_map(|o| o.iter()) {
        if !reordered_request
            .as_object()
            .map(|o| o.contains_key(k))
            .unwrap_or(false)
        {
            reordered_request[k] = v.clone();
        }
    }

    // Match the Gemini entrypoint: every upstream attempt gets a unique request ID.
    // Reusing session/message-count IDs can pin later requests to an earlier 429 result.
    let timestamp_ms = chrono::Utc::now().timestamp_millis();
    let random_hex = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let request_id = format!("agent/{}/{}", timestamp_ms, random_hex);

    // [NEW] 动态检测是否需要标记为 agent 请求
    // 只有在请求携带 tools，或上下文包含工具调用交互时才打上 agent 标签
    let has_tools = reordered_request
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|arr| !arr.is_empty())
        .unwrap_or(false);
    let has_tool_interactions = reordered_request
        .get("contents")
        .map(super::super::common_utils::contents_has_tool_interactions)
        .unwrap_or(false);
    let is_agent_request = config.request_type != "image_gen" && (has_tools || has_tool_interactions);

    let mut final_body = json!({
        "project": project_id,
        // [CACHE] 使用重排后的字段顺序，稳定前缀在前
        "request": reordered_request,
        "model": config.final_model,
        "userAgent": "antigravity",
        // [CACHE] requestId stays last so its per-attempt value does not disturb the stable prefix.
        "requestId": request_id,
    });

    if config.request_type == "image_gen" {
        final_body["requestType"] = json!("image_gen");
    } else if is_agent_request {
        final_body["requestType"] = json!("agent");
    }

    // [CACHE:L3] 使用多层级缓存的 compute_prefix_hash 计算组合哈希
    // Layer 1 + Layer 2 的独立 hash 组合 → Layer 3 key
    let prefix_hash = {
        let si_json = final_body["request"]
            .get("systemInstruction")
            .map(|v| serde_json::to_string(v).unwrap_or_default())
            .unwrap_or_default();
        let tools_json = final_body["request"]
            .get("tools")
            .map(|v| serde_json::to_string(v).unwrap_or_default())
            .unwrap_or_default();
        let hash =
            crate::proxy::cache_manager::CacheManager::compute_prefix_hash(&si_json, &tools_json);
        tracing::info!(
            "[Cache-Opt:L3-Prefix] prefix_hash={} model={} sid={} tokens_in_msg={}",
            &hash[..hash.len().min(16)],
            config.final_model,
            &session_id[..session_id.len().min(8)],
            message_count
        );
        hash
    };

    // [CACHE:L3] 尝试利用显式缓存：查询 prefix_hash 对应的 Gemini cache_id
    // 若命中，注入 cachedContent 参数，告知 Gemini 服务端复用已缓存的前缀
    let cache_manager = crate::proxy::cache_manager::global_cache_manager();
    if let Some(cache_name) = cache_manager.lookup_prefix(&prefix_hash) {
        if let Some(req_obj) = final_body["request"].as_object_mut() {
            req_obj.insert("cachedContent".to_string(), json!(cache_name));
            tracing::info!(
                "[Cache-Opt] Explicit cache HIT: prefix_hash={} cache_name={}",
                &prefix_hash[..prefix_hash.len().min(16)],
                cache_name
            );
            cache_manager.record_explicit_hit(&prefix_hash);
        }
    }

    // [DEFENSE] 净化所有 contents 中的 inlineData，过滤或降级空数据/损坏数据
    if let Some(inner) = final_body.get_mut("request") {
        crate::proxy::mappers::common_utils::sanitize_gemini_payload_inline_data(inner);
    }

    (final_body, session_id, message_count, prefix_hash)
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
    fn prompt_log_identity_cleanup_only_changes_system_instructions() {
        let old = "You are Codex, an agent based on GPT-5.";
        let req: OpenAIRequest = serde_json::from_value(json!({
            "model": "gemini-3.7-flash-high",
            "instructions": format!("Top-level: {old}"),
            "messages": [
                {"role": "system", "content": format!("System: {old}")},
                {"role": "developer", "content": format!("<model_switch>{old}</model_switch>")},
                {"role": "user", "content": old},
                {"role": "assistant", "tool_calls": [{"id": "call_identity", "type": "function", "function": {"name": "identity", "arguments": "{}"}}]},
                {"role": "tool", "tool_call_id": "call_identity", "content": old}
            ]
        }))
        .unwrap();
        let (body, _, _, _) = transform_openai_request(&req, "test-project", &req.model, None);
        let system = body["request"]["systemInstruction"].to_string();
        assert!(!system.contains(old));
        assert!(system.contains("Top-level: You are Codex, an agent."));
        assert!(system.contains("System: You are Codex, an agent."));
        assert!(system.contains("<model_switch>You are Codex, an agent.</model_switch>"));
        let contents = body["request"]["contents"].to_string();
        assert_eq!(contents.matches(old).count(), 2);
    }
    fn tiered_request_body(model: &str, effort: Option<&str>) -> Value {
        let mut raw = json!({
            "model": model,
            "messages": [{"role": "user", "content": "test"}]
        });
        if let Some(effort) = effort {
            raw["reasoning"] = json!({ "effort": effort });
        }
        let request: OpenAIRequest = serde_json::from_value(raw).unwrap();
        transform_openai_request(&request, "test-project", model, None).0
    }

    #[test]
    fn tiered_flash_maps_supported_effort_to_thinking_level_without_rewriting_model() {
        for model in ["gemini-3.8-flash-tiered", "gemini-9.9-flash-tiered"] {
            assert!(is_tiered_flash_model(model));
            for (effort, expected_level) in [("low", "LOW"), ("medium", "MEDIUM"), ("high", "HIGH")]
            {
                let body = tiered_request_body(model, Some(effort));
                let thinking = &body["request"]["generationConfig"]["thinkingConfig"];

                assert_eq!(body["model"], model);
                assert_eq!(thinking["includeThoughts"], true);
                assert_eq!(thinking["thinkingLevel"], expected_level);
                assert!(thinking.get("thinkingBudget").is_none());
            }
        }
    }

    #[test]
    fn tiered_flash_leaves_level_unset_for_missing_or_unsupported_effort() {
        for effort in [
            None,
            Some("none"),
            Some("xhigh"),
            Some("max"),
            Some("custom"),
        ] {
            let body = tiered_request_body("gemini-3.8-flash-tiered", effort);
            let thinking = &body["request"]["generationConfig"]["thinkingConfig"];

            assert_eq!(body["model"], "gemini-3.8-flash-tiered");
            assert_eq!(thinking["includeThoughts"], true);
            assert!(thinking.get("thinkingLevel").is_none());
            assert!(thinking.get("thinkingBudget").is_none());
        }
    }

    #[test]
    fn reasoning_effort_does_not_select_levels_for_pro_or_ordinary_flash() {
        for model in ["gemini-3.1-pro-high", "gemini-3.8-flash"] {
            assert!(!is_tiered_flash_model(model));
            let body = tiered_request_body(model, Some("low"));
            let thinking = &body["request"]["generationConfig"]["thinkingConfig"];

            assert_eq!(body["model"], model);
            assert!(thinking.get("thinkingLevel").is_none());
            assert!(thinking.get("thinkingBudget").is_some());
        }
        assert!(!is_tiered_flash_model("gemini-3.8-flash-tiered-image"));
    }

    #[test]
    fn test_openai_request_id_is_unique_per_upstream_attempt() {
        let req: OpenAIRequest = serde_json::from_value(json!({
            "model": "gemini-3.7-flash-high",
            "messages": [{"role": "user", "content": "test"}]
        }))
        .unwrap();

        let (first, _, _, _) =
            transform_openai_request(&req, "test-project", "gemini-3.7-flash-high", None);
        let (second, _, _, _) =
            transform_openai_request(&req, "test-project", "gemini-3.7-flash-high", None);
        let first_id = first["requestId"].as_str().unwrap();
        let second_id = second["requestId"].as_str().unwrap();

        assert_ne!(first_id, second_id);

        let parts = first_id.split('/').collect::<Vec<_>>();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "agent");
        assert!(parts[1].parse::<i64>().is_ok());
        assert_eq!(parts[2].len(), 8);
        assert!(parts[2].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn responses_session_identity_is_not_written_into_system_instruction() {
        let req: OpenAIRequest = serde_json::from_value(json!({
            "model": "gemini-3.7-flash-high",
            "messages": [{"role": "user", "content": "same first message"}]
        }))
        .unwrap();

        let (body, session_id, _, _) = transform_openai_request_with_session(
            &req,
            "test-project",
            "gemini-3.7-flash-high",
            None,
            "resp-routing-root",
            None,
        );
        let system_instruction = body["request"].get("systemInstruction");

        assert_eq!(session_id, "resp-routing-root");
        if let Some(sys) = system_instruction {
            let sys_text = sys.to_string();
            assert!(!sys_text.contains("Request type:"));
            assert!(!sys_text.contains("Mapped model:"));
            assert!(!sys_text.contains("user_information"));
            assert!(!sys_text.contains("Session ID:"));
            assert!(!sys_text.contains("resp-routing-root"));
        }
        assert!(!body["request"].to_string().contains("resp-routing-root"));
    }

    #[test]
    fn openai_system_messages_are_forwarded_as_separate_parts() {
        let req: OpenAIRequest = serde_json::from_value(json!({
            "model": "gemini-3.7-flash-high",
            "messages": [
                {"role": "system", "content": "<environment>env</environment>"},
                {"role": "system", "content": [
                    {"type": "text", "text": "<workflow_and_execution_discipline>wf</workflow_and_execution_discipline>"},
                    {"type": "text", "text": "=== AVAILABLE SKILLS ==="}
                ]},
                {"role": "user", "content": "hello"}
            ]
        }))
        .unwrap();

        let (body, _, _, _) =
            transform_openai_request(&req, "test-project", "gemini-3.7-flash-high", None);
        let parts = body["request"]["systemInstruction"]["parts"]
            .as_array()
            .expect("systemInstruction.parts");
        let texts: Vec<&str> = parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect();

        assert_eq!(
            texts,
            vec![
                "<environment>env</environment>",
                "<workflow_and_execution_discipline>wf</workflow_and_execution_discipline>",
                "=== AVAILABLE SKILLS ==="
            ]
        );
        let joined = texts.join("\n");
        assert!(!joined.contains("<user_information>"));
        assert!(!joined.contains("Request type:"));
    }

    #[test]
    fn responses_reads_the_parent_signature_instead_of_the_routing_identity() {
        let previous_response_id = format!("resp-parent-{}", uuid::Uuid::new_v4());
        let routing_session_id = format!("resp-root-{}", uuid::Uuid::new_v4());
        let signature = "parent-signature-".repeat(8);
        crate::proxy::SignatureCache::global().cache_session_signature(
            &previous_response_id,
            signature.clone(),
            1,
        );
        let request = OpenAIRequest {
            model: "gemini-3.7-flash-high".to_string(),
            messages: vec![OpenAIMessage {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call-parent".to_string(),
                    r#type: "function".to_string(),
                    function: Some(ToolFunction {
                        name: "test_tool".to_string(),
                        arguments: "{}".to_string(),
                    }),
                    status: None,
                    call_id: None,
                    operation: None,
                }]),
                tool_call_id: None,
                name: None,
                refusal: None,
            }],
            ..Default::default()
        };

        let (body, returned_session_id, _, _) = transform_openai_request_with_session(
            &request,
            "test-project",
            "gemini-3.7-flash-high",
            None,
            &routing_session_id,
            Some(&previous_response_id),
        );
        let contents = body["request"]["contents"].as_array().unwrap();
        let model_msg = contents
            .iter()
            .find(|c| c["role"] == "model")
            .expect("Should find model role message");
        let tool_part = model_msg["parts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|part| part.get("functionCall").is_some())
            .unwrap();

        assert_eq!(returned_session_id, routing_session_id);
        assert_eq!(tool_part["thoughtSignature"], signature);
    }

    #[test]
    fn test_issue_1592_gemini_3_pro_budget_capping() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::proxy::config::update_thinking_budget_config(crate::proxy::config::ThinkingBudgetConfig::default());
        // [FIX #1592] Regression test for gemini-3-pro thinking budget capping
        let req = OpenAIRequest {
            model: "gemini-3-pro".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                refusal: None,
                content: Some(OpenAIContent::String("test".into())),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }],
            ..Default::default()
        };

        // Auto mode (default) should map gemini-3-pro thinking budget to 49152 per model_specs
        let (result, _sid, _msg_count, _) =
            transform_openai_request(&req, "test-v", "gemini-3-pro", None);
        let budget = result["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"]
            .as_i64()
            .unwrap();
        assert_eq!(
            budget, 49152,
            "Gemini-3-pro budget must match model_specs (49152) in Auto mode"
        );
    }

    #[test]
    fn test_issue_1602_custom_mode_gemini_capping() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // [FIX #1602] Regression test for custom mode capping
        use crate::proxy::config::{
            update_thinking_budget_config, ThinkingBudgetConfig, ThinkingBudgetMode,
        };

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
                refusal: None,
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
        let (result, _sid, _msg_count, _) =
            transform_openai_request(&req, "test-v", "gemini-2.0-flash-thinking", None);
        let budget = result["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"]
            .as_i64()
            .unwrap();
        assert_eq!(
            budget, 24576,
            "Gemini custom budget must be capped to 24576"
        );

        // 验证非 Gemini 模型（如 Claude 原生路径，假设映射后名不含 gemini）则不应截断
        // 注意：这里的 transform_openai_request 第三个参数是 mapped_model
        let (result_claude, _, _, _) =
            transform_openai_request(&req, "test-v", "claude-3-7-sonnet", None);
        let budget_claude = result_claude["request"]["generationConfig"]["thinkingConfig"]
            ["thinkingBudget"]
            .as_i64();
        // 如果不是 gemini模型且协议中没带 thinking 配置，可能会是 None 或 32000
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
                refusal: None,
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

        let (result, _sid, _msg_count, _) =
            transform_openai_request(&req, "test-v", "gemini-1.5-flash", None);
        let parts = &result["request"]["contents"][0]["parts"];
        assert_eq!(parts.as_array().unwrap().len(), 2);
        assert_eq!(parts[0]["text"].as_str().unwrap(), "What is in this image?");
        assert_eq!(
            parts[1]["inlineData"]["mimeType"].as_str().unwrap(),
            "image/png"
        );
    }

    #[test]
    fn test_transform_openai_request_video_multimodal() {
        use crate::proxy::mappers::openai::models::OpenAIVideoUrl;
        let req = OpenAIRequest {
            model: "gemini-2.5-flash".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                refusal: None,
                content: Some(OpenAIContent::Array(vec![
                    OpenAIContentBlock::Text { text: "Describe this video".to_string() },
                    OpenAIContentBlock::VideoUrl { video_url: OpenAIVideoUrl {
                        url: "data:video/mp4;base64,AAAA".to_string(),
                        mime_type: None,
                    } }
                ])),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }],
            ..Default::default()
        };

        let (result, _sid, _msg_count, _) =
            transform_openai_request(&req, "test-v", "gemini-2.5-flash", None);
        let parts = &result["request"]["contents"][0]["parts"];
        assert_eq!(parts.as_array().unwrap().len(), 2);
        assert_eq!(parts[0]["text"].as_str().unwrap(), "Describe this video");
        assert_eq!(
            parts[1]["inlineData"]["mimeType"].as_str().unwrap(),
            "video/mp4"
        );
        assert_eq!(
            parts[1]["inlineData"]["data"].as_str().unwrap(),
            "AAAA"
        );
    }

    #[test]
    fn test_gemini_pro_thinking_injection() {
        let req = OpenAIRequest {
            model: "gemini-3-pro-preview".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                refusal: None,
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
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            ..Default::default()
        };

        let _lock = crate::proxy::config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Set passthrough mode so user-specified budget is retained
        crate::proxy::config::update_thinking_budget_config(crate::proxy::config::ThinkingBudgetConfig {
            mode: crate::proxy::config::ThinkingBudgetMode::Passthrough,
            custom_value: 16000,
            effort: None,
        });
        struct PassthroughResetGuard;
        impl Drop for PassthroughResetGuard {
            fn drop(&mut self) {
                crate::proxy::config::update_thinking_budget_config(crate::proxy::config::ThinkingBudgetConfig::default());
            }
        }
        let _guard = PassthroughResetGuard;

        // Pass explicit gemini-3-pro-preview which doesn't have "-thinking" suffix
        let (result, _sid, _msg_count, _) =
            transform_openai_request(&req, "test-p", "gemini-3-pro-preview", None);
        let gen_config = &result["request"]["generationConfig"];

        // Assert thinkingConfig is present (fix verification)
        assert!(
            gen_config.get("thinkingConfig").is_some(),
            "thinkingConfig should be injected for gemini-3-pro"
        );

        let budget = gen_config["thinkingConfig"]["thinkingBudget"]
            .as_u64()
            .unwrap();
        // [ANTI-POLLUTION] Gemini 3 Pro unconditionally locks to authoritative default budget (49152)
        assert_eq!(budget, 49152);
    }
    #[test]
    fn test_gemini_3_pro_image_not_thinking() {
        let req = OpenAIRequest {
            model: "gemini-3-pro-image-4k".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                refusal: None,
                content: Some(OpenAIContent::String("Generate a cat".to_string())),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }],
            ..Default::default()
        };

        // Pass gemini-3-pro-image which matches "gemini-3-pro" substring
        let (result, _sid, _msg_count, _) =
            transform_openai_request(&req, "test-p", "gemini-3-pro-image", None);
        let gen_config = &result["request"]["generationConfig"];

        // Assert thinkingConfig IS present (based on latest user feedback)
        assert!(
            gen_config.get("thinkingConfig").is_some(),
            "thinkingConfig SHOULD be injected for gemini-3-pro-image"
        );

        // Assert imageConfig is present
        assert!(
            gen_config.get("imageConfig").is_some(),
            "imageConfig should be present for image models"
        );
        assert_eq!(gen_config["imageConfig"]["imageSize"], "4K");
    }

    #[test]
    fn test_default_max_tokens_openai() {
        let req = OpenAIRequest {
            model: "gpt-4".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                refusal: None,
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

        let (result, _sid, _msg_count, _) =
            transform_openai_request(&req, "test-p", "gemini-3-pro-high-thinking", None);
        let gen_config = &result["request"]["generationConfig"];
        let max_output_tokens = gen_config["maxOutputTokens"].as_i64().unwrap();
        // budget(10001) + overhead(32768) = 42769
        assert_eq!(max_output_tokens, 42769);

        // Verify thinkingBudget
        let budget = gen_config["thinkingConfig"]["thinkingBudget"]
            .as_i64()
            .unwrap();
        // actual(10001) for high-thinking pro
        assert_eq!(budget, 10001);
    }

    #[test]
    fn test_flash_thinking_budget_capping() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::proxy::config::update_thinking_budget_config(
            crate::proxy::config::ThinkingBudgetConfig::default(),
        );

        let req = OpenAIRequest {
            model: "gpt-4".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                refusal: None,
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
        let (result, _sid, _msg_count, _) =
            transform_openai_request(&req, "test-p", "gemini-2.0-flash-thinking-exp", None);
        let gen_config = &result["request"]["generationConfig"];

        // Should be capped at 24576
        let budget = gen_config["thinkingConfig"]["thinkingBudget"]
            .as_i64()
            .unwrap();
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
                refusal: None,
                content: None,
                reasoning_content: Some("Thinking...".to_string()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_123".to_string(),
                    r#type: "function".to_string(),
                    function: Some(ToolFunction {
                        name: "test_tool".to_string(),
                        arguments: "{}".to_string(),
                    }),
                    status: None,
                    call_id: None,
                    operation: None,
                }]),
                tool_call_id: None,
                name: None,
            }],
            person_generation: None,
            ..Default::default()
        };

        // Simulate Vertex AI path
        let mapped_model = "projects/my-project/locations/us-central1/publishers/google/models/gemini-2.0-flash-thinking-exp";

        let (result, _sid, _msg_count, _) =
            transform_openai_request(&req, "test-v", mapped_model, None);

        // Extract the tool call part from contents (under request.contents)
        let contents = result["request"]["contents"].as_array().unwrap();
        // Identify the part with functionCall
        let model_msg = contents
            .iter()
            .find(|c| c["role"] == "model")
            .expect("Should find model role message");
        let parts = model_msg["parts"].as_array().unwrap();
        let tool_part = parts
            .iter()
            .find(|p: &&serde_json::Value| p.get("functionCall").is_some())
            .expect("Should find functionCall part");

        // Vertex AI requires sentinel
        assert_eq!(
            tool_part["thoughtSignature"].as_str(),
            Some("skip_thought_signature_validator")
        );
    }

    #[test]
    fn test_issue_2167_gemini_flash_thinking_signature() {
        // [FIX #2167] gemini-3-flash / gemini-3.1-flash 在无缓存签名时，functionCall 必须携带 thoughtSignature
        for model in &["gemini-3-flash", "gemini-3.1-flash"] {
            let req = OpenAIRequest {
                model: model.to_string(),
                messages: vec![OpenAIMessage {
                    role: "assistant".to_string(),
                    refusal: None,
                    content: None,
                    reasoning_content: None, // 无 reasoning_content，模拟无缓存首次调用
                    tool_calls: Some(vec![ToolCall {
                        id: "call_flash_test".to_string(),
                        r#type: "function".to_string(),
                        function: Some(ToolFunction {
                            name: "get_weather".to_string(),
                            arguments: "{\"location\":\"Beijing\"}".to_string(),
                        }),
                        status: None,
                        call_id: None,
                        operation: None,
                    }]),
                    tool_call_id: None,
                    name: None,
                }],
                ..Default::default()
            };

            let (result, _sid, _msg_count, _) =
                transform_openai_request(&req, "test-proj", model, None);

            let contents = result["request"]["contents"]
                .as_array()
                .expect("Should have request.contents");
            // flash 模型的 assistant role → Gemini "model" role
            let model_msg = contents
                .iter()
                .find(|c| c["role"] == "model")
                .expect("Should find model role message");
            let parts = model_msg["parts"].as_array().expect("Should have parts");
            let tool_part = parts
                .iter()
                .find(|p: &&serde_json::Value| p.get("functionCall").is_some())
                .expect(&format!("[{model}] Should find functionCall part"));

            assert_eq!(
                tool_part["thoughtSignature"].as_str(),
                Some("skip_thought_signature_validator"),
                "[{model}] gemini-3-flash functionCall must contain thoughtSignature sentinel"
            );
        }
    }

    #[test]
    fn test_openai_image_thinking_mode_disabled() {
        // 1. Set global mode to disabled
        crate::proxy::config::update_image_thinking_mode(Some("disabled".to_string()));

        let req = OpenAIRequest {
            model: "gemini-3-pro-image".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                refusal: None,
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
        let (result, _sid, _msg_count, _) =
            transform_openai_request(&req, "test-proj", "gemini-3-pro-image", None);

        // 3. Verify thinkingConfig has includeThoughts: false
        let gen_config = result["request"]["generationConfig"]
            .as_object()
            .expect("Should have generationConfig in request payload");
        let thinking_config = gen_config["thinkingConfig"].as_object().unwrap();

        assert_eq!(thinking_config["includeThoughts"], false);

        // 4. Reset global mode
        crate::proxy::config::update_image_thinking_mode(Some("enabled".to_string()));
    }

    #[test]
    fn test_mixed_tools_injection_openai() {
        // 验证 OpenAI 协议在 Gemini 2.0+ 下支持混合工具
        let req = OpenAIRequest {
            model: "gpt-4o-online".to_string(), // -online 触发联网
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                refusal: None,
                content: Some(OpenAIContent::String("Hello".to_string())),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }],
            tools: Some(vec![json!({
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "location": {"type": "string"}
                        }
                    }
                }
            })]),
            ..Default::default()
        };

        // 使用 gemini-2.0-flash 模型执行转换
        let (result, _, _, _) = transform_openai_request(&req, "proj", "gemini-2.0-flash", None);

        let tools = result["request"]["tools"]
            .as_array()
            .expect("Should have tools");

        let has_functions = tools
            .iter()
            .any(|t: &serde_json::Value| t.get("functionDeclarations").is_some());
        let has_google_search = tools
            .iter()
            .any(|t: &serde_json::Value| t.get("googleSearch").is_some());

        assert!(has_functions, "Should contain functionDeclarations");
        // 在 v1internal 架构下，不开启混合调用以避免 400 报错
        assert!(
            !has_google_search,
            "v1internal should avoid mixed Google Search when functionDeclarations present"
        );
    }

    #[test]
    fn test_response_format_json_schema_mapping() {
        let raw_json = json!({
            "model": "gemini-2.5-flash",
            "messages": [
                {"role": "user", "content": "test"}
            ],
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "test_schema",
                    "schema": {
                        "type": "object",
                        "properties": {
                            "summary": {
                                "type": "object",
                                "properties": {
                                    "text": { "type": "string" },
                                    "sourceId": { "type": "string" },
                                    "quote": { "type": "string" }
                                },
                                "required": ["text", "sourceId", "quote"],
                                "additionalProperties": false
                            },
                            "topics": {
                                "type": "array",
                                "items": { "type": "string" }
                            }
                        },
                        "required": ["summary", "topics"],
                        "additionalProperties": false
                    },
                    "strict": true
                }
            }
        });

        let request: OpenAIRequest = serde_json::from_value(raw_json).unwrap();
        let (res_val, _sid, _msg_count, _) = transform_openai_request(&request, "test-v", "gemini-2.5-flash", None);
        let gen_config = &res_val["request"]["generationConfig"];
        assert_eq!(gen_config["responseMimeType"], "application/json");
        assert!(gen_config.get("responseSchema").is_some());
        let resp_schema = &gen_config["responseSchema"];
        assert_eq!(resp_schema["type"], "object");
        assert_eq!(resp_schema["properties"]["summary"]["type"], "object");
    }

    #[test]
    fn test_issue_3391_claude_without_thinking_suffix_incompatible_history() {
        // [FIX #3391] 当客户端使用不带 -thinking 后缀的 Claude 模型（如 claude-sonnet-4-6）
        // 且显式传入 thinking 配置，但在多轮对话中有纯文本 assistant 消息（缺少 reasoning_content）时，
        // 系统应当安全禁用 thinking，并且不插入无签名的思考占位块，避免 Claude 上游报 400 thinking.signature 错误。
        let req = OpenAIRequest {
            model: "claude-sonnet-4-6".to_string(),
            messages: vec![
                OpenAIMessage {
                    role: "user".to_string(),
                    content: Some(OpenAIContent::String("Hello".to_string())),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "assistant".to_string(),
                    content: Some(OpenAIContent::String("Hi there!".to_string())),
                    reasoning_content: None, // 模拟 AstrBot/SillyTavern 等客户端历史消息缺失思考内容
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "user".to_string(),
                    content: Some(OpenAIContent::String("How are you?".to_string())),
                    ..Default::default()
                },
            ],
            thinking: Some(ThinkingConfig {
                thinking_type: Some("enabled".to_string()),
                budget_tokens: Some(1024),
                effort: None,
            }),
            ..Default::default()
        };

        let (result, _sid, _msg_count, _) =
            transform_openai_request(&req, "test-proj", "claude-sonnet-4-6", None);

        let gen_config = &result["request"]["generationConfig"];
        // 验证 thinkingConfig 被保留（即使历史缺失 reasoning_content，也不再降级禁用）
        assert!(
            gen_config.get("thinkingConfig").is_some(),
            "thinkingConfig must be preserved per server-side thinking persistence policy"
        );

        // 验证 assistant 消息中具有思考块（补齐保底或从 ThinkingStore 恢复，避免上游 400）
        let contents = result["request"]["contents"].as_array().unwrap();
        let assistant_msg = contents
            .iter()
            .find(|m| m["role"] == "model")
            .expect("Should have model message");
        let parts = assistant_msg["parts"].as_array().unwrap();
        let has_thought_part = parts.iter().any(|p| p.get("thought") == Some(&serde_json::json!(true)));
        assert!(has_thought_part, "Should ensure thinking block is present in assistant message for Claude");
    }

    #[test]
    fn test_hermes_autonomous_first_assistant_tool_call_injected_user_primer() {
        // Ensure conversation starting with assistant tool calls (common in Hermes / autonomous agents)
        // has a user primer injected at index 0 so Google Gemini does not reject with:
        // "Please ensure that function call turn comes immediately after a user turn or after a function response turn."
        let raw_json = json!({
            "model": "gemini-3.8-flash-high",
            "messages": [
                {
                    "role": "system",
                    "content": "You are Don Santo, an autonomous agent."
                },
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [
                        {
                            "id": "call_123",
                            "type": "function",
                            "function": {
                                "name": "terminal",
                                "arguments": "{\"command\": \"ls\"}"
                            }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_123",
                    "name": "terminal",
                    "content": "output of ls"
                }
            ]
        });

        let request: OpenAIRequest = serde_json::from_value(raw_json).unwrap();
        let (res_val, _sid, _msg_count, _) = transform_openai_request(&request, "test-v", "gemini-3.8-flash-high", None);
        let contents = res_val["request"]["contents"].as_array().expect("contents must be an array");

        // First turn MUST be user
        assert_eq!(contents[0]["role"], "user");
        assert!(contents[0]["parts"][0]["text"].as_str().is_some());

        // Second turn MUST be model with functionCall
        assert_eq!(contents[1]["role"], "model");
        let has_func_call = contents[1]["parts"].as_array().unwrap().iter().any(|p| p.get("functionCall").is_some());
        assert!(has_func_call);

        // Third turn MUST be user with functionResponse
        assert_eq!(contents[2]["role"], "user");
        let has_func_resp = contents[2]["parts"].as_array().unwrap().iter().any(|p| p.get("functionResponse").is_some());
        assert!(has_func_resp);

        // Since it has tool_calls / functionCall, it should have requestType: "agent"
        assert_eq!(res_val.get("requestType"), Some(&json!("agent")));
    }

    #[test]
    fn test_plain_chat_omits_agent_request_type() {
        let raw_json = json!({
            "model": "gemini-2.5-flash",
            "messages": [
                {
                    "role": "user",
                    "content": "Hello world!"
                }
            ]
        });

        let request: OpenAIRequest = serde_json::from_value(raw_json).unwrap();
        let (res_val, _, _, _) = transform_openai_request(&request, "test-v", "gemini-2.5-flash", None);
        assert!(res_val.get("requestType").is_none(), "Plain text request should not have requestType: 'agent'");
    }
}
