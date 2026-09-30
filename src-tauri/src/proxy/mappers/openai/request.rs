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

/// Collect system/developer text without joining. A string content is one block;
/// a content array contributes one block per text part.
fn collect_system_instruction_blocks(request: &OpenAIRequest) -> Vec<String> {
    let mut blocks = Vec::new();

    if let Some(inst) = &request.instructions {
        if !inst.trim().is_empty() {
            blocks.push(inst.clone());
        }
    }

    // [JEIKCODE FROZEN SYSTEM PRINCIPLE]
    // 严格遵循系统指令绝对冻结法则：仅收集最开头的连续 system / developer 消息。
    // 一旦遇到首个非 system/developer 消息（即对话已进入多轮状态），立即停止收集！
    // 对话中途出现的任何 system/developer 消息一律保留在 contents 中作为 synthetic user 处理，
    // 绝对严禁提取并追加至 systemInstruction，杜绝顶层系统前缀突变破坏 KV Cache！
    for msg in &request.messages {
        if msg.role != "system" && msg.role != "developer" {
            break;
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

#[allow(dead_code)]
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
        false, // is_responses_api (Chat completions protocol)
    )
}

pub fn transform_openai_request_with_session(
    request: &OpenAIRequest,
    project_id: &str,
    mapped_model: &str,
    token: Option<&ProxyToken>,
    routing_session_id: &str,
    signature_read_key: Option<&str>,
    _is_responses_api: bool,
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
    // ThinkingStore must use the stable tenant-scoped store_key (request.session_id),
    // not the Responses routing / previous_response_id chain. Capture already writes
    // to store_key; hydrating with a different key leaves history as placeholder+sentinel.
    let thinking_store_key = request
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(signature_read_key)
        .unwrap_or(routing_session_id)
        .to_string();
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
    // Client thinking flags/budgets are ignored for enablement and fill.
    // Server authority: model-id heuristics + ThinkingStore hydrate/finalize only.
    let _user_enabled_thinking = request
        .thinking
        .as_ref()
        .map(|t| t.thinking_type.as_deref() == Some("enabled"))
        .unwrap_or(false);
    let _user_thinking_budget = request
        .thinking
        .as_ref()
        .and_then(|t| t.budget_tokens)
        .or_else(|| request.reasoning.as_ref().and_then(|r| r.max_tokens));

    let is_claude_model = mapped_model_lower.contains("claude");
    let is_claude_thinking = mapped_model_lower.ends_with("-thinking")
        || (is_claude_model && mapped_model_lower.contains("thinking"));
    let force_server_thinking = !is_under_v3
        && crate::proxy::thinking_store::any_model_forces_server_thinking(&[
            request.model.as_str(),
            mapped_model,
        ]);
    let is_thinking_model = is_gemini_3_thinking
        || is_claude_thinking
        || is_gemini_flash_thinking
        || force_server_thinking;

    // [NEW] 决定是否开启 Thinking 功能（纯服务端权威 vs 客户端直接控制）:
    // 网关控制模式下：仅按映射后的模型 ID / 强制思考启发式开启，忽略客户端 thinking.type / budget / effort。
    // 客户端直接控制模式下：若客户端显式关闭思考（type: disabled 或 budget: 0 或 effort: none），尊重客户端设置。
    let client_switch = crate::proxy::pipeline::extract_client_thinking_switch(
        request
            .thinking
            .as_ref()
            .and_then(|t| t.thinking_type.as_deref()),
        request
            .thinking
            .as_ref()
            .and_then(|t| t.budget_tokens.map(|b| b as u64))
            .or_else(|| {
                request
                    .reasoning
                    .as_ref()
                    .and_then(|r| r.max_tokens.map(|b| b as u64))
            }),
        request
            .reasoning_effort
            .as_deref()
            .or_else(|| request.reasoning.as_ref().and_then(|r| r.effort.as_deref()))
            .or_else(|| request.thinking.as_ref().and_then(|t| t.effort.as_deref())),
    );

    let tb_config = crate::proxy::config::get_thinking_budget_config();
    let is_client_control =
        tb_config.control_source == crate::proxy::config::ThinkingControlSource::Client;

    let is_client_disabled = is_client_control && client_switch.is_disabled();

    let actual_include_thinking = if is_client_disabled {
        false
    } else {
        !is_under_v3 && (is_thinking_model || force_server_thinking || is_client_control)
    };

    if _user_enabled_thinking || _user_thinking_budget.is_some() {
        tracing::debug!(
            "[OpenAI-Thinking] Ignoring client thinking enable/budget (enabled={}, budget={:?}); server model heuristics decide fill",
            _user_enabled_thinking,
            _user_thinking_budget
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

    // 遵循纯透传原则：不替换日期、路径、UUID 等任何动态字段，完整保留客户端与 Agent 的真实环境感知。
    // 身份声明归一化（Codex `based on GPT-x` / 厂商 `created by …` / Claude Agent SDK 等）已**统一收敛**
    // 到协议无关的提示词清洗流水线节点：`PromptSanitizer::normalize_system_identity`，在系统提示词头部
    // 窗口内做广谱匹配。依据 AGENTS.md「Pipeline First / Fix Strategy」——适配层只做参数归一化与协议转换，
    // 清洗一律由流水线统一处理，避免同一类 WAF 风险在四个协议里各写一份特例。
    let mut seen_system_instruction_keys = std::collections::HashSet::new();
    system_instructions.retain(|inst| {
        let key = inst.trim();
        !key.is_empty() && seen_system_instruction_keys.insert(key.to_string())
    });

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

    // 2. 构建 Gemini contents (过滤掉已作为 leading system 的指令，中途 system 消息就地转为 user 保持前缀)
    let leading_system_count = request
        .messages
        .iter()
        .take_while(|m| m.role == "system" || m.role == "developer")
        .count();

    // 找出 messages 中最后一个 assistant 角色的下标 (绝对索引)
    let _last_assistant_msg_idx = request
        .messages
        .iter()
        .enumerate()
        .rposition(|(_, m)| m.role == "assistant");

    let contents: Vec<Value> = request
        .messages
        .iter()
        .enumerate()
        .filter(|(idx, _)| *idx >= leading_system_count)
        .map(|(msg_index, msg)| {
            let role = match msg.role.as_str() {
                "assistant" => "model",
                "tool" | "function" => "user",
                "system" | "developer" => "user",
                _ => &msg.role,
            };

            let mut parts = Vec::new();

            let client_reasoning = msg
                .reasoning_content
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());

            if role == "model" {
                // [2026-09-27] 占位/缺失 reasoning 不再写 "..." 思考块。
                // 官方样本 9/24 轮是「无思考块 + 锚点带签名」，空/占位思考不出站；
                // 签名归位由流水线终审 place_turn_signature 按锚点规则处理。
                if actual_include_thinking {
                    if let Some(rc) = client_reasoning {
                        if !crate::proxy::thinking_store::is_placeholder_thought(rc) {
                            // 纯净线缆透传：客户端若自带签名则无损透传；缺失签名全权委托流水线统一对齐与回填
                            let mut thought_part = json!({
                                "text": rc,
                                "thought": true,
                            });
                            if let Some(ref sig) = msg.signature {
                                thought_part["thoughtSignature"] = json!(sig);
                            }
                            parts.push(thought_part);
                        }
                    }
                } else if let Some(rc) = client_reasoning {
                    // 思考关闭时，非占位思考降级为普通文本；占位/空直接跳过
                    if !crate::proxy::thinking_store::is_placeholder_thought(rc) {
                        parts.push(json!({ "text": rc }));
                    }
                }
            }

            // Handle content (multimodal or text)
            // [FIX] Skip standard content mapping for tool/function roles to avoid duplicate parts
            // These are handled below in the "Handle tool response" section.
            let is_tool_role = msg.role == "tool" || msg.role == "function";
            let is_mid_system_role = msg.role == "system" || msg.role == "developer";
            if let (Some(content), false) = (&msg.content, is_tool_role) {
                if is_mid_system_role {
                    // [JEIKCODE SYNTHETIC USER] 中途系统消息，转换为用户态下的 <system-reminder>，不破坏全局 systemInstruction 前缀
                    let sys_text = match content {
                        OpenAIContent::String(s) => s.clone(),
                        OpenAIContent::Array(blocks) => {
                            let mut joined = String::new();
                            for b in blocks {
                                if let OpenAIContentBlock::Text { text } = b {
                                    if !joined.is_empty() {
                                        joined.push('\n');
                                    }
                                    joined.push_str(text);
                                }
                            }
                            joined
                        }
                    };
                    let wrapped_reminder = crate::proxy::mappers::common_utils::wrap_in_system_reminder(&sys_text);
                    if !wrapped_reminder.is_empty() {
                        parts.push(json!({
                            "text": wrapped_reminder
                        }));
                    }
                } else {
                    let has_tools = msg.tool_calls.as_ref().map(|tc| !tc.is_empty()).unwrap_or(false);
                    match content {
                        OpenAIContent::String(s) => {
                            if !s.is_empty() && !(has_tools && crate::proxy::thinking_store::is_placeholder_thought(s)) {
                                parts.extend(crate::proxy::mappers::common_utils::parse_markdown_images_to_parts(s));
                            }
                        }
                        OpenAIContent::Array(blocks) => {
                            for block in blocks {
                                match block {
                                    OpenAIContentBlock::Text { text } => {
                                        if !(has_tools && crate::proxy::thinking_store::is_placeholder_thought(text)) {
                                            parts.extend(crate::proxy::mappers::common_utils::parse_markdown_images_to_parts(text));
                                        }
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

                    // 纯净线缆透传：客户端若自带签名则原样透传，未带则留空，全权委托进站流水线统一对齐与回填
                    if let Some(ref sig) = tc.signature {
                        func_call_part["thoughtSignature"] = json!(sig);
                    }

                    parts.push(func_call_part);
                }
            }

            // Handle tool response
            if msg.role == "tool" || msg.role == "function" {
                let name = msg.name.as_deref().unwrap_or("unknown");
                // 优先从紧邻的前置 assistant 消息中查找匹配该 tool_call_id 的工具名称 (精准杜绝长会话 ID 碰撞时全局 Map 覆盖错误)
                let matched_preceding_name = if let Some(ref target_id) = msg.tool_call_id {
                    let mut found = None;
                    for prev_idx in (0..msg_index).rev() {
                        if let Some(prev_msg) = request.messages.get(prev_idx) {
                            if prev_msg.role == "assistant" {
                                if let Some(ref calls) = prev_msg.tool_calls {
                                    for call in calls {
                                        if call.id == *target_id {
                                            found = if let Some(ref func) = call.function {
                                                Some(if func.name == "local_shell_call" { "shell".to_string() } else { func.name.clone() })
                                            } else if call.operation.is_some() || call.r#type == "apply_patch_call" {
                                                Some("apply_patch".to_string())
                                            } else {
                                                None
                                            };
                                            break;
                                        }
                                    }
                                }
                                break;
                            } else if prev_msg.role != "tool" && prev_msg.role != "function" {
                                break;
                            }
                        }
                    }
                    found
                } else {
                    None
                };

                let final_name = if let Some(ref p_name) = matched_preceding_name {
                    p_name.as_str()
                } else if name == "local_shell_call" {
                    "shell"
                } else if let Some(id) = &msg.tool_call_id {
                    tool_id_to_name.get(id).map(|s| s.as_str()).unwrap_or(name)
                } else {
                    name
                };

                let mut extra_parts = Vec::new();

                let content_val = match &msg.content {
                    Some(OpenAIContent::String(s)) => {
                        crate::proxy::mappers::common_utils::extract_multimodal_from_tool_text(
                            s,
                            &mut extra_parts,
                        )
                    }
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

                // [优化] 如果结果为空，注入显式确认信号，防止模型幻觉与 Gemini 400 校验异常
                let final_content = if content_val.trim().is_empty() {
                    "Command executed successfully.".to_string()
                } else {
                    content_val
                };

                let mut fr_part = json!({
                    "functionResponse": {
                       "name": final_name,
                       "response": { "output": final_content },
                       "id": msg.tool_call_id.clone().unwrap_or_default()
                    }
                });
                // 危险测试分支法则：tool 响应 (functionResponse) 绝不携带签名
                if let Some(obj) = fr_part.as_object_mut() {
                    obj.remove("thoughtSignature");
                    obj.remove("thought_signature");
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

    // 连续相同角色的消息**保持独立**（对齐官方形态）。
    //
    // 历史实现会合并它们，理由是 "Gemini 强制要求 user/model 交替"。但官方
    // Antigravity 报文里连续 user 轮与连续 model 轮都是常态，v1internal 上游
    // 并不要求严格交替；实测（2026-09-26，`gemini-3.8-flash-tiered` @ daily）
    // 两种形态均 200 且上下文理解一致。
    let mut merged_contents = contents;
    crate::proxy::pipeline::InboundThinkingPipeline::process_contents(
        &mut merged_contents,
        mapped_model,
        actual_include_thinking,
        Some(&thinking_store_key),
        false,
    );
    let mut contents = merged_contents;

    // Gemini requires conversations to start with a user turn, and functionCall turns
    // must immediately follow a user turn or a functionResponse turn.
    // If the conversation starts with a model turn (e.g. autonomous agent loops starting with tool calls),
    // inject a lightweight user primer to prevent 400 INVALID_ARGUMENT error.
    if contents.is_empty() {
        contents.push(json!({
            "role": "user",
            "parts": [{ "text": "Continue" }]
        }));
    } else if contents
        .first()
        .and_then(|f| f.get("role"))
        .and_then(|r| r.as_str())
        == Some("model")
    {
        contents.insert(
            0,
            json!({
                "role": "user",
                "parts": [{ "text": "Continue the task." }]
            }),
        );
    }

    // 3. 构建请求体

    let mut gen_config = json!({});
    if let Some(top_p) = request.top_p {
        gen_config["topP"] = json!(top_p);
    }
    if let Some(temp) = request.temperature {
        gen_config["temperature"] = json!(temp);
    }

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

    // 为 thinking 模型注入 thinkingConfig (使用流水线统一配置治理)
    if is_client_disabled {
        tracing::debug!(
            "[OpenAI-Request] Client direct control disabled thinking: removing thinkingConfig for {}",
            mapped_model
        );
        crate::proxy::pipeline::InboundThinkingPipeline::configure_inbound_thinking(
            mapped_model,
            &mut gen_config,
            client_switch,
            None,
            None,
            token,
        );
    } else if actual_include_thinking {
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
            // [CONFIGURABLE] 思考预算与思考配置：全协议统一由 InboundThinkingPipeline 流水线节点权威解析与治理
            let client_effort = request
                .reasoning_effort
                .as_deref()
                .or_else(|| request.reasoning.as_ref().and_then(|r| r.effort.as_deref()))
                .or_else(|| request.thinking.as_ref().and_then(|t| t.effort.as_deref()));

            let client_budget = request
                .thinking
                .as_ref()
                .and_then(|t| t.budget_tokens.map(|b| b as u64))
                .or_else(|| {
                    request
                        .reasoning
                        .as_ref()
                        .and_then(|r| r.max_tokens.map(|b| b as u64))
                });

            let resolved_budget =
                crate::proxy::pipeline::InboundThinkingPipeline::configure_inbound_thinking(
                    mapped_model,
                    &mut gen_config,
                    client_switch,
                    client_effort,
                    client_budget,
                    token,
                );

            if let Some(final_budget) = resolved_budget {
                if final_budget > 0 {
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

                    if let Some(max_tokens) = request.max_tokens {
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
                }
            }

            tracing::debug!(
                "[OpenAI-Request] Configured thinkingConfig for model {}: {:?} (source={:?})",
                mapped_model,
                gen_config["thinkingConfig"],
                tb_config.control_source
            );
        }
    }

    // Tiered Flash models: if resolved_budget was None (default) in gateway authority, ensure only includeThoughts is set
    if !is_client_control
        && is_tiered_flash_model(mapped_model)
        && gen_config["thinkingConfig"].get("thinkingBudget").is_none()
    {
        gen_config["thinkingConfig"] = json!({ "includeThoughts": true });
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

    // [PIPELINE] 统一清洗提示词与风控伪 Header（含 [undefined] 深度清理，见 PromptSanitizer）
    crate::proxy::mappers::prompt_sanitizer::PromptSanitizer::sanitize_gemini_payload(
        &mut inner_request,
    );

    // 4. Handle Tools (Merged Cleaning)
    let _is_codex_style = request.model.contains("codex")
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
                "pure_tools_v3:{raw_json}"
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

                if name_opt.is_none() {
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

                if let Some(params) = gemini_func.get_mut("parameters") {
                    // [DEEP FIX] 统一调用公共库清洗：展开 $ref 并剔除所有层级的 format/definitions
                    crate::proxy::common::json_schema::clean_json_schema(params);

                    // Gemini v1internal 要求：
                    // 1. type 必须是大写 (OBJECT, STRING 等)
                    // 2. 根对象必须有 "type": "OBJECT"，且必须声明 "properties" (即使为空)，杜绝 MALFORMED_FUNCTION_CALL
                    if let Some(params_obj) = params.as_object_mut() {
                        if !params_obj.contains_key("type") {
                            params_obj.insert("type".to_string(), json!("OBJECT"));
                        }
                        if !params_obj.contains_key("properties") {
                            params_obj.insert("properties".to_string(), json!({}));
                        }
                    }

                    // 递归转换 type 为大写 (符合 Protobuf 定义)
                    enforce_uppercase_types(params);
                } else {
                    gemini_func.as_object_mut().unwrap().insert(
                        "parameters".to_string(),
                        json!({
                            "type": "OBJECT",
                            "properties": {}
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

    // 保持客户端工具声明原序。按 name 重排会改掉前缀字节，Windows 原生客户端没有这一步。

    // Removed auto-inject since we handle it above now if Codex passes it.

    if !function_declarations.is_empty() {
        inner_request["tools"] = json!([{ "functionDeclarations": function_declarations }]);
    }

    // [tool_choice] 客户端有就传，没有就不传：
    // 若客户端显式指定了 tool_choice，将其规范化映射为标准的 Gemini toolConfig
    if let Some(tool_choice) = &request.tool_choice {
        if let Some(gemini_tc) =
            crate::proxy::mappers::common_utils::map_openai_tool_choice_to_gemini(tool_choice)
        {
            inner_request["toolConfig"] = gemini_tc;
        }
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
        // [REMOVED v4.8.2] toolConfig / tool_config 注入已移除（官方不带该字段），
        // googleSearch 工具声明本身已由 inject_google_search_tool 写入 tools。
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
        crate::proxy::common::session::apply_upstream_session(
            &mut inner_request,
            &t.account_id,
            &thinking_store_key,
        );
    }

    // [CACHE] 重建 inner_request 字段顺序——稳定前缀在前，动态内容在后
    // [CACHE] 统一委托进站流水线进行前缀拓扑规范化与对齐（Pipeline First 核心归一）
    crate::proxy::pipeline::InboundThinkingPipeline::align_google_request_prefix_topology_with_model(
        &mut inner_request,
        &config.final_model,
        None,
    );
    let reordered_request = inner_request;

    // requestId：官方 5 段形态，三适配器共用（含 unixMs → 幂等隔离）。
    // 历史教训：复用 session / message-count 的 ID 会把后续请求 pin 到一次更早的 429 结果。
    let request_id = super::super::common_utils::build_official_request_id(
        &thinking_store_key,
        message_count as u64,
    );

    // 官方客户端指纹（企业 / GCP 账号为 jetski）—— 三适配器共用，避免指纹漂移
    let (official_user_agent, _official_ide_type) =
        super::super::common_utils::resolve_official_fingerprint(token);

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
    let is_agent_request =
        config.request_type != "image_gen" && (has_tools || has_tool_interactions);

    let mut final_body = json!({
        "project": project_id,
        "requestId": request_id,
        "request": reordered_request,
        "model": config.final_model,
        "userAgent": official_user_agent,
    });

    if config.request_type == "image_gen" {
        final_body["requestType"] = json!("image_gen");
    } else if is_agent_request {
        final_body["requestType"] = json!("agent");
    }

    crate::proxy::pipeline::InboundThinkingPipeline::align_official_envelope(&mut final_body);

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

    // [CACHE:L3] 记录前缀哈希生命周期统计（Google v1internal 依赖 TPU 隐式前缀缓存，杜绝显式注入 cachedContent 造成 400）
    let cache_manager = crate::proxy::cache_manager::global_cache_manager();
    if cache_manager.lookup_prefix(&prefix_hash).is_some() {
        cache_manager.record_explicit_hit(&prefix_hash);
    }

    // [DEFENSE] 净化所有 contents 中的 inlineData，过滤或降级空数据/损坏数据
    if let Some(inner) = final_body.get_mut("request") {
        crate::proxy::mappers::common_utils::sanitize_gemini_payload_inline_data(inner);
    }

    (final_body, session_id, message_count, prefix_hash)
}

pub fn enforce_uppercase_types(value: &mut Value) {
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

    #[test]
    fn test_openai_aliases_max_completion_tokens_and_reasoning_max_tokens() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // 1. max_completion_tokens 及 max_output_tokens 别名支持
        let req1: OpenAIRequest = serde_json::from_value(json!({
            "model": "o3-mini",
            "messages": [{"role": "user", "content": "hello"}],
            "max_completion_tokens": 16384
        }))
        .unwrap();
        assert_eq!(req1.max_tokens, Some(16384));

        let req1_resp: OpenAIRequest = serde_json::from_value(json!({
            "model": "gpt-5",
            "messages": [{"role": "user", "content": "hello"}],
            "max_output_tokens": 128000
        }))
        .unwrap();
        assert_eq!(req1_resp.max_tokens, Some(128000));

        // 2. reasoning.max_tokens 与 reasoning.effort 支持
        let req2: OpenAIRequest = serde_json::from_value(json!({
            "model": "o3-mini",
            "messages": [{"role": "user", "content": "hello"}],
            "reasoning": {
                "effort": "high",
                "max_tokens": 8000
            }
        }))
        .unwrap();
        assert_eq!(
            req2.reasoning.as_ref().and_then(|r| r.effort.as_deref()),
            Some("high")
        );
        assert_eq!(
            req2.reasoning.as_ref().and_then(|r| r.max_tokens),
            Some(8000)
        );

        // 3. thinking.max_tokens 别名支持
        let req3: OpenAIRequest = serde_json::from_value(json!({
            "model": "o3-mini",
            "messages": [{"role": "user", "content": "hello"}],
            "thinking": {
                "type": "enabled",
                "max_tokens": 10240
            }
        }))
        .unwrap();
        assert_eq!(
            req3.thinking.as_ref().and_then(|t| t.budget_tokens),
            Some(10240)
        );

        // 4. 客户端控制模式下从 reasoning.max_tokens 提取 client_budget
        crate::proxy::config::update_thinking_budget_config(
            crate::proxy::config::ThinkingBudgetConfig {
                control_source: crate::proxy::config::ThinkingControlSource::Client,
                ..Default::default()
            },
        );
        let (body, _, _, _) = transform_openai_request(&req2, "test-p", "gemini-3.7-flash", None);
        crate::proxy::config::update_thinking_budget_config(
            crate::proxy::config::ThinkingBudgetConfig::default(),
        );
        assert_eq!(
            body["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            8000
        );
    }

    #[test]
    fn prompt_log_identity_cleanup_only_changes_system_instructions() {
        // 身份声明归一化由协议无关的流水线节点承担（`PromptSanitizer::sanitize_gemini_payload`），
        // 适配层只产出原样系统提示词。本用例校验「适配层产出 → 流水线清洗」的真实链路：
        // 系统提示词块内的身份声明被统一归一化为中性身份，user / tool 文本逐字保留。
        for old in [
            "You are Codex, an agent based on GPT-5.",
            "You are Codex, a coding agent based on GPT-5.",
            "You are Codex, an advanced coding agent based on GPT-6.",
        ] {
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
            let (mut body, _, _, _) =
                transform_openai_request(&req, "test-project", &req.model, None);

            // 适配层保持纯透传：身份声明在流水线节点介入前原样保留
            assert!(body["request"]["systemInstruction"]
                .to_string()
                .contains(old));

            crate::proxy::mappers::prompt_sanitizer::PromptSanitizer::sanitize_gemini_payload(
                &mut body,
            );

            let system = body["request"]["systemInstruction"].to_string();
            assert!(!system.contains(old));
            assert!(system.contains("Top-level: You are an AI Agent."));
            assert!(system.contains("System: You are an AI Agent."));
            assert!(system.contains("<model_switch>You are an AI Agent.</model_switch>"));
            // 用户提问与工具消息（含管道自身提示词）零改动
            let contents = body["request"]["contents"].to_string();
            assert_eq!(contents.matches(old).count(), 2);
        }
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
    fn tiered_flash_ignores_client_effort_and_keeps_include_thoughts_only() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Server-authoritative: client reasoning.effort must not set thinkingLevel, but populates thinkingBudget per tier.
        for model in ["gemini-3.8-flash-tiered", "gemini-9.9-flash-tiered"] {
            assert!(is_tiered_flash_model(model));
            for (effort, expected_budget) in [
                (None, -1),
                (Some("low"), 1000),
                (Some("medium"), 4000),
                (Some("high"), -1),
                (Some("xhigh"), -1),
            ] {
                let body = tiered_request_body(model, effort);
                let thinking = &body["request"]["generationConfig"]["thinkingConfig"];

                assert_eq!(body["model"], model);
                assert_eq!(thinking["includeThoughts"], true);
                assert!(thinking.get("thinkingLevel").is_none());
                assert_eq!(thinking["thinkingBudget"], expected_budget);
            }
        }
    }

    #[test]
    fn reasoning_effort_does_not_select_levels_for_pro_or_ordinary_flash() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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
    fn test_openai_reasoning_effort_authority_resolution() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // 1. 启发式模型忽略客户端 reasoning_effort
        let req_high: OpenAIRequest = serde_json::from_value(json!({
            "model": "gemini-3.7-flash-high",
            "messages": [{"role": "user", "content": "hi"}],
            "reasoning_effort": "low"
        }))
        .unwrap();
        let (body, _, _, _) =
            transform_openai_request(&req_high, "test-p", "gemini-3.7-flash-high", None);
        assert_eq!(
            body["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            -1
        );

        // 2. 裸模型 Flash 接管客户端 reasoning_effort
        let req_flash_high: OpenAIRequest = serde_json::from_value(json!({
            "model": "gemini-3-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "reasoning_effort": "high"
        }))
        .unwrap();
        let (body, _, _, _) =
            transform_openai_request(&req_flash_high, "test-p", "gemini-3-flash", None);
        assert_eq!(
            body["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            -1
        );

        let req_flash_low: OpenAIRequest = serde_json::from_value(json!({
            "model": "gemini-3-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "reasoning_effort": "low"
        }))
        .unwrap();
        let (body, _, _, _) =
            transform_openai_request(&req_flash_low, "test-p", "gemini-3-flash", None);
        assert_eq!(
            body["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            1000
        );

        // 3. 裸模型 Flash 客户端未填或试图关闭：绝不关闭思考，强制回填 -high (-1) 或 -medium (4000)
        let req_flash_none: OpenAIRequest = serde_json::from_value(json!({
            "model": "gemini-3-flash",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .unwrap();
        let (body, _, _, _) =
            transform_openai_request(&req_flash_none, "test-p", "gemini-3-flash", None);
        assert_eq!(
            body["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            -1
        );

        let req_flash_disabled: OpenAIRequest = serde_json::from_value(json!({
            "model": "gemini-3-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "reasoning_effort": "none"
        }))
        .unwrap();
        let (body, _, _, _) =
            transform_openai_request(&req_flash_disabled, "test-p", "gemini-3-flash", None);
        assert_eq!(
            body["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            4000
        );

        // 4. 裸模型 Flash 客户端传入自定义 budget_tokens：彻底被忽略，由服务端权威等级回填
        let req_flash_custom_budget: OpenAIRequest = serde_json::from_value(json!({
            "model": "gemini-3-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "thinking": {"budget_tokens": 12345}
        }))
        .unwrap();
        let (body, _, _, _) =
            transform_openai_request(&req_flash_custom_budget, "test-p", "gemini-3-flash", None);
        assert_eq!(
            body["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            -1
        );

        let req_flash_high_custom_budget: OpenAIRequest = serde_json::from_value(json!({
            "model": "gemini-3-flash",
            "messages": [{"role": "user", "content": "hi"}],
            "reasoning_effort": "high",
            "thinking": {"budget_tokens": 1234}
        }))
        .unwrap();
        let (body, _, _, _) = transform_openai_request(
            &req_flash_high_custom_budget,
            "test-p",
            "gemini-3-flash",
            None,
        );
        assert_eq!(
            body["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            -1
        );
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
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0], "agent");
        assert_eq!(parts[1].len(), 16);
        assert!(parts[2].parse::<i64>().is_ok());
        assert_eq!(parts[3].len(), 8);
        assert!(parts[4].parse::<u64>().is_ok());
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
            true,
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
        let texts: Vec<&str> = parts.iter().filter_map(|p| p["text"].as_str()).collect();

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
        use base64::Engine;
        let mut raw = vec![0x12u8, 42];
        raw.extend_from_slice(&[b'A'; 60]);
        let signature = base64::engine::general_purpose::STANDARD.encode(raw);
        crate::proxy::SignatureCache::global().cache_session_signature(
            &previous_response_id,
            signature.clone(),
            1,
        );
        let request = OpenAIRequest {
            model: "gemini-3.7-flash-high".to_string(),
            messages: vec![OpenAIMessage {
                role: "assistant".to_string(),
                tool_calls: Some(vec![ToolCall {
                    id: "call-parent".to_string(),
                    r#type: "function".to_string(),
                    function: Some(ToolFunction {
                        name: "test_tool".to_string(),
                        arguments: "{}".to_string(),
                    }),
                    ..Default::default()
                }]),
                ..Default::default()
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
            true,
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
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::proxy::config::update_thinking_budget_config(
            crate::proxy::config::ThinkingBudgetConfig::default(),
        );
        // [FIX #1592] Regression test for gemini-3-pro thinking budget capping
        let req = OpenAIRequest {
            model: "gemini-3-pro".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("test".into())),
                ..Default::default()
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
            budget, 10001,
            "Gemini-3-pro bare model budget defaults to medium dictionary budget (10001)"
        );
    }

    #[test]
    fn test_issue_1602_custom_mode_gemini_capping() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // [FIX #1602] Regression test for custom mode capping
        use crate::proxy::config::{
            update_thinking_budget_config, ThinkingBudgetConfig, ThinkingBudgetMode,
        };

        // 设置自定义模式，且数值超过 24k
        update_thinking_budget_config(ThinkingBudgetConfig {
            mode: ThinkingBudgetMode::Custom,
            custom_value: 32000,
            effort: None,
            ..Default::default()
        });
        struct ResetGuard;
        impl Drop for ResetGuard {
            fn drop(&mut self) {
                update_thinking_budget_config(ThinkingBudgetConfig::default());
            }
        }
        let _guard = ResetGuard;

        let req = OpenAIRequest {
            model: "gemini-2.0-flash-thinking".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("test".into())),
                ..Default::default()
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
        let _budget_claude = result_claude["request"]["generationConfig"]["thinkingConfig"]
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
                content: Some(OpenAIContent::Array(vec![
                    OpenAIContentBlock::Text { text: "What is in this image?".to_string() },
                    OpenAIContentBlock::ImageUrl { image_url: OpenAIImageUrl {
                        url: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==".to_string(),
                        detail: None
                    } }
                ])),
                ..Default::default()
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
                content: Some(OpenAIContent::Array(vec![
                    OpenAIContentBlock::Text {
                        text: "Describe this video".to_string(),
                    },
                    OpenAIContentBlock::VideoUrl {
                        video_url: OpenAIVideoUrl {
                            url: "data:video/mp4;base64,AAAA".to_string(),
                            mime_type: None,
                        },
                    },
                ])),
                ..Default::default()
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
        assert_eq!(parts[1]["inlineData"]["data"].as_str().unwrap(), "AAAA");
    }

    #[test]
    fn test_gemini_pro_thinking_injection() {
        let req = OpenAIRequest {
            model: "gemini-3-pro-preview".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Thinking test".to_string())),
                ..Default::default()
            }],
            stream: false,
            n: None,
            // Client enable + budget must be ignored under server-authoritative policy
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

        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Even Passthrough must NOT honor client budget anymore
        crate::proxy::config::update_thinking_budget_config(
            crate::proxy::config::ThinkingBudgetConfig {
                mode: crate::proxy::config::ThinkingBudgetMode::Passthrough,
                custom_value: 16000,
                effort: None,
                ..Default::default()
            },
        );
        struct PassthroughResetGuard;
        impl Drop for PassthroughResetGuard {
            fn drop(&mut self) {
                crate::proxy::config::update_thinking_budget_config(
                    crate::proxy::config::ThinkingBudgetConfig::default(),
                );
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
        // [ANTI-POLLUTION] model_specs budget only; client 16000 + Passthrough ignored; bare pro defaults to 10001
        assert_eq!(budget, 10001);
    }
    #[test]
    fn test_gemini_3_pro_image_not_thinking() {
        let req = OpenAIRequest {
            model: "gemini-3-pro-image-4k".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Generate a cat".to_string())),
                ..Default::default()
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
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let req = OpenAIRequest {
            model: "gpt-4".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Hello".to_string())),
                ..Default::default()
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
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::proxy::config::update_thinking_budget_config(
            crate::proxy::config::ThinkingBudgetConfig::default(),
        );

        let req = OpenAIRequest {
            model: "gpt-4".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Hello".to_string())),
                ..Default::default()
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
    fn test_vertex_ai_drops_sentinel_injection() {
        // [FIX #1650] Verify sentinel signature injection for Vertex AI models
        let req = OpenAIRequest {
            model: "claude-3-7-sonnet-thinking".to_string(), // Triggers is_thinking_model
            messages: vec![OpenAIMessage {
                role: "assistant".to_string(),
                reasoning_content: Some("Thinking...".to_string()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_123".to_string(),
                    r#type: "function".to_string(),
                    function: Some(ToolFunction {
                        name: "test_tool".to_string(),
                        arguments: "{}".to_string(),
                    }),
                    ..Default::default()
                }]),
                ..Default::default()
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

        // 铁律：functionCall **绝不**携带哨兵 —— 官方报文 0/23 处出现哨兵，
        // 它不属于 Antigravity 协议；签名归位统一交给流水线终审 `place_turn_signature`。
        assert!(
            tool_part.get("thoughtSignature").is_none(),
            "functionCall must not carry a sentinel signature"
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
                    tool_calls: Some(vec![ToolCall {
                        id: "call_flash_test".to_string(),
                        r#type: "function".to_string(),
                        function: Some(ToolFunction {
                            name: "get_weather".to_string(),
                            arguments: "{\"location\":\"Beijing\"}".to_string(),
                        }),
                        ..Default::default()
                    }]),
                    ..Default::default()
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

            // 铁律：无缓存签名时**留空**（字段缺席），绝不发明哨兵。
            // 官方报文里哨兵出现 0 次；签名缺失是被上游容忍的（在飞轮即缺席），
            // 且该轮签名由流水线终审 `place_turn_signature` 按锚点归位。
            assert!(
                tool_part.get("thoughtSignature").is_none(),
                "[{model}] functionCall must not carry a sentinel signature when unsigned"
            );
        }
    }

    #[test]
    fn test_openai_image_thinking_mode_disabled() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        // 1. Set global mode to disabled
        crate::proxy::config::update_image_thinking_mode(Some("disabled".to_string()));
        struct ImageResetGuard;
        impl Drop for ImageResetGuard {
            fn drop(&mut self) {
                crate::proxy::config::update_image_thinking_mode(Some("enabled".to_string()));
            }
        }
        let _guard = ImageResetGuard;

        let req = OpenAIRequest {
            model: "gemini-3-pro-image".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Draw a cat".to_string())),
                ..Default::default()
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
    }

    #[test]
    fn test_mixed_tools_injection_openai() {
        // 验证 OpenAI 协议在 Gemini 2.0+ 下支持混合工具
        let req = OpenAIRequest {
            model: "gpt-4o-online".to_string(), // -online 触发联网
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Hello".to_string())),
                ..Default::default()
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
        let (res_val, _sid, _msg_count, _) =
            transform_openai_request(&request, "test-v", "gemini-2.5-flash", None);
        let gen_config = &res_val["request"]["generationConfig"];
        assert_eq!(gen_config["responseMimeType"], "application/json");
        assert!(gen_config.get("responseSchema").is_some());
        let resp_schema = &gen_config["responseSchema"];
        assert_eq!(resp_schema["type"], "object");
        assert_eq!(resp_schema["properties"]["summary"]["type"], "object");
    }

    #[test]
    fn test_issue_3391_claude_without_thinking_suffix_incompatible_history() {
        // claude-sonnet-4-6 forces server thinking by model heuristic (not client enable).
        // Missing client reasoning_content still gets "..." + sentinel placeholder.
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
                    reasoning_content: None,
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
        assert!(
            gen_config.get("thinkingConfig").is_some(),
            "thinkingConfig must be present via server model heuristics"
        );

        let contents = result["request"]["contents"].as_array().unwrap();
        let assistant_msg = contents
            .iter()
            .find(|m| m["role"] == "model")
            .expect("Should have model message");
        let parts = assistant_msg["parts"].as_array().unwrap();
        // 【2026-09-27】客户端无 reasoning 时不再填充 "..." 占位思考块；
        // 无思考块是官方标准形态（锚点签名由流水线终审回填）。
        assert!(
            !parts
                .iter()
                .any(|p| p.get("thought") == Some(&serde_json::json!(true))),
            "No placeholder thinking block should be injected when client reasoning is absent"
        );
    }

    #[test]
    fn server_authoritative_ignores_client_reasoning_content_and_budget() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::proxy::config::update_thinking_budget_config(
            crate::proxy::config::ThinkingBudgetConfig {
                mode: crate::proxy::config::ThinkingBudgetMode::Passthrough,
                custom_value: 99999,
                effort: None,
                ..Default::default()
            },
        );
        struct ResetGuard;
        impl Drop for ResetGuard {
            fn drop(&mut self) {
                crate::proxy::config::update_thinking_budget_config(
                    crate::proxy::config::ThinkingBudgetConfig::default(),
                );
            }
        }
        let _guard = ResetGuard;

        let client_thought = "Detailed client reasoning thought process";
        let req = OpenAIRequest {
            model: "gemini-3.8-flash-high".to_string(),
            messages: vec![
                OpenAIMessage {
                    role: "user".to_string(),
                    content: Some(OpenAIContent::String("q1".to_string())),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "assistant".to_string(),
                    content: Some(OpenAIContent::String("a1".to_string())),
                    reasoning_content: Some(client_thought.to_string()),
                    signature: Some("fake_client_sig_that_must_be_ignored_in_chat_api".to_string()),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "user".to_string(),
                    content: Some(OpenAIContent::String("q2".to_string())),
                    ..Default::default()
                },
            ],
            thinking: Some(ThinkingConfig {
                thinking_type: Some("enabled".to_string()),
                budget_tokens: Some(16000),
                effort: Some("high".to_string()),
            }),
            ..Default::default()
        };

        let (result, _sid, _msg_count, _) =
            transform_openai_request(&req, "test-proj", "gemini-3.8-flash-high", None);

        let budget = result["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"]
            .as_i64()
            .expect("thinkingBudget from model_specs");
        assert_eq!(budget, -1, "client budget + Passthrough must be ignored");

        let contents = result["request"]["contents"].as_array().unwrap();
        let model_msg = contents
            .iter()
            .find(|c| c["role"] == "model")
            .expect("model turn");
        let thought = model_msg["parts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p.get("thought") == Some(&serde_json::json!(true)))
            .expect("thought part");
        // Reasoning content is preserved (Anthropic alignment)
        assert_eq!(thought["text"], client_thought);
        assert!(
            thought.get("thoughtSignature").is_none(),
            "Gemini thought parts do not carry signatures"
        );
        let dumped = serde_json::to_string(&result).unwrap();
        assert!(
            !dumped.contains("fake_client_sig_that_must_be_ignored"),
            "invalid client signature must not be forwarded"
        );
    }

    #[test]
    fn client_thinking_enable_ignored_for_non_thinking_model() {
        let req = OpenAIRequest {
            model: "gpt-4o".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("hi".to_string())),
                ..Default::default()
            }],
            thinking: Some(ThinkingConfig {
                thinking_type: Some("enabled".to_string()),
                budget_tokens: Some(8000),
                effort: Some("high".to_string()),
            }),
            ..Default::default()
        };

        let (result, _sid, _msg_count, _) =
            transform_openai_request(&req, "test-proj", "gpt-4o", None);
        let gen_config = &result["request"]["generationConfig"];
        assert!(
            gen_config.get("thinkingConfig").is_none(),
            "non-thinking model must not enable thinking from client flags"
        );
    }

    #[test]
    fn test_issue_3515_client_direct_control_disable_thinking() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        use crate::proxy::config::{
            update_thinking_budget_config, ThinkingBudgetConfig, ThinkingControlSource,
        };

        let mut config = ThinkingBudgetConfig::default();
        config.control_source = ThinkingControlSource::Client;
        update_thinking_budget_config(config);

        struct ResetGuard;
        impl Drop for ResetGuard {
            fn drop(&mut self) {
                crate::proxy::config::update_thinking_budget_config(ThinkingBudgetConfig::default());
            }
        }
        let _guard = ResetGuard;

        let req = OpenAIRequest {
            model: "gemini-3.8-flash-medium".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Hello".to_string())),
                ..Default::default()
            }],
            thinking: Some(ThinkingConfig {
                thinking_type: None,
                budget_tokens: Some(0),
                effort: None,
            }),
            ..Default::default()
        };

        let (result, _sid, _msg_count, _) =
            transform_openai_request(&req, "test-proj", "gemini-3.8-flash-medium", None);

        let gen_config = result["request"]["generationConfig"]
            .as_object()
            .expect("Should have generationConfig in request payload");

        // 客户端直接控制模式显式关闭思考：彻底不填任何转出报文的谷歌思考块字段和预算
        assert!(gen_config.get("thinkingConfig").is_none());
    }

    #[test]
    fn test_client_direct_control_all_scenarios_for_gemini_38_flash_tiered() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        use crate::proxy::config::{
            update_thinking_budget_config, ThinkingBudgetConfig, ThinkingControlSource,
        };

        let mut config = ThinkingBudgetConfig::default();
        config.control_source = ThinkingControlSource::Client;
        update_thinking_budget_config(config);

        struct ResetGuard;
        impl Drop for ResetGuard {
            fn drop(&mut self) {
                crate::proxy::config::update_thinking_budget_config(ThinkingBudgetConfig::default());
            }
        }
        let _guard = ResetGuard;

        // 场景 1: 客户端思考块全缺省（开关缺省就是默认开，预算留空交由上游自适应）
        let req_default = OpenAIRequest {
            model: "gemini-3.8-flash-tiered".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Hello".to_string())),
                ..Default::default()
            }],
            ..Default::default()
        };
        let (res1, _, _, _) =
            transform_openai_request(&req_default, "test-proj", "gemini-3.8-flash-tiered", None);
        let tc1 = res1["request"]["generationConfig"]["thinkingConfig"]
            .as_object()
            .expect("Should have thinkingConfig for default thinking");
        assert_eq!(tc1.get("includeThoughts"), Some(&json!(true)));
        assert!(tc1.get("thinkingBudget").is_none());
        assert!(tc1.get("thinkingLevel").is_none());

        // 场景 2: 客户端仅传思考等级 low（等级透传，绝不脑补 budget，必须带上 includeThoughts: true）
        let req_level = OpenAIRequest {
            model: "gemini-3.8-flash-tiered".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Hello".to_string())),
                ..Default::default()
            }],
            reasoning_effort: Some("low".to_string()),
            ..Default::default()
        };
        let (res2, _, _, _) =
            transform_openai_request(&req_level, "test-proj", "gemini-3.8-flash-tiered", None);
        let tc2 = res2["request"]["generationConfig"]["thinkingConfig"]
            .as_object()
            .expect("Should have thinkingConfig for level");
        assert_eq!(tc2.get("includeThoughts"), Some(&json!(true)));
        assert_eq!(tc2.get("thinkingLevel"), Some(&json!("LOW")));
        assert!(tc2.get("thinkingBudget").is_none());

        // 场景 3: 客户端仅传预算 8192（忠实透传预算，绝不脑补等级，必须带上 includeThoughts: true）
        let req_budget = OpenAIRequest {
            model: "gemini-3.8-flash-tiered".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Hello".to_string())),
                ..Default::default()
            }],
            thinking: Some(ThinkingConfig {
                thinking_type: Some("enabled".to_string()),
                budget_tokens: Some(8192),
                effort: None,
            }),
            ..Default::default()
        };
        let (res3, _, _, _) =
            transform_openai_request(&req_budget, "test-proj", "gemini-3.8-flash-tiered", None);
        let tc3 = res3["request"]["generationConfig"]["thinkingConfig"]
            .as_object()
            .expect("Should have thinkingConfig for budget");
        assert_eq!(tc3.get("includeThoughts"), Some(&json!(true)));
        assert_eq!(tc3.get("thinkingBudget"), Some(&json!(8192)));
        assert!(tc3.get("thinkingLevel").is_none());
        // 且验证 maxOutputTokens 自动垫高保证 > budget
        let max_out = res3["request"]["generationConfig"]["maxOutputTokens"]
            .as_i64()
            .unwrap();
        assert!(max_out > 8192);
    }

    #[test]
    fn test_issue_3515_gateway_control_preserves_medium_budget_when_client_budget_zero() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        use crate::proxy::config::{
            update_thinking_budget_config, ThinkingBudgetConfig, ThinkingControlSource,
        };

        let mut config = ThinkingBudgetConfig::default();
        config.control_source = ThinkingControlSource::Gateway;
        update_thinking_budget_config(config);

        struct ResetGuard;
        impl Drop for ResetGuard {
            fn drop(&mut self) {
                crate::proxy::config::update_thinking_budget_config(ThinkingBudgetConfig::default());
            }
        }
        let _guard = ResetGuard;

        let req = OpenAIRequest {
            model: "gemini-3.8-flash-medium".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Hello".to_string())),
                ..Default::default()
            }],
            thinking: Some(ThinkingConfig {
                thinking_type: None,
                budget_tokens: Some(0),
                effort: None,
            }),
            ..Default::default()
        };

        let (result, _sid, _msg_count, _) =
            transform_openai_request(&req, "test-proj", "gemini-3.8-flash-medium", None);

        let gen_config = result["request"]["generationConfig"]
            .as_object()
            .expect("Should have generationConfig in request payload");
        let thinking_config = gen_config["thinkingConfig"].as_object().unwrap();

        // 网关权威控制模式下，90% 用户行为保持不变，依然权威锁定 4000 预算
        assert_eq!(thinking_config.get("thinkingBudget"), Some(&json!(4000)));
        assert_eq!(thinking_config.get("includeThoughts"), Some(&json!(true)));
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
        let (res_val, _sid, _msg_count, _) =
            transform_openai_request(&request, "test-v", "gemini-3.8-flash-high", None);
        let contents = res_val["request"]["contents"]
            .as_array()
            .expect("contents must be an array");

        // First turn MUST be user
        assert_eq!(contents[0]["role"], "user");
        assert!(contents[0]["parts"][0]["text"].as_str().is_some());

        // Second turn MUST be model with functionCall
        assert_eq!(contents[1]["role"], "model");
        let has_func_call = contents[1]["parts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p.get("functionCall").is_some());
        assert!(has_func_call);

        // Third turn MUST be model with functionResponse (aligned with native Antigravity Gemini format)
        assert_eq!(contents[2]["role"], "model");
        let has_func_resp = contents[2]["parts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p.get("functionResponse").is_some());
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
        let (res_val, _, _, _) =
            transform_openai_request(&request, "test-v", "gemini-2.5-flash", None);
        assert!(
            res_val.get("requestType").is_none(),
            "Plain text request should not have requestType: 'agent'"
        );
    }

    #[test]
    fn test_openai_responses_api_vs_chat_api_thinking_and_signature() {
        let valid_client_sig = "B".repeat(60);
        let client_thought = "Responses API client thinking block";

        let req = OpenAIRequest {
            model: "gemini-3-pro".to_string(),
            messages: vec![
                OpenAIMessage {
                    role: "user".to_string(),
                    content: Some(OpenAIContent::String("first question".to_string())),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "assistant".to_string(),
                    content: Some(OpenAIContent::String("assistant answer".to_string())),
                    reasoning_content: Some(client_thought.to_string()),
                    signature: Some(valid_client_sig.clone()),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "user".to_string(),
                    content: Some(OpenAIContent::String("follow up".to_string())),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        // Chat 与 Responses 对同一份历史使用同一套签名规则
        let (resp_result, _, _, _) = transform_openai_request_with_session(
            &req,
            "test-proj",
            "gemini-3-pro",
            None,
            "routing-1",
            None,
            true,
        );
        let (chat_result, _, _, _) = transform_openai_request_with_session(
            &req,
            "test-proj",
            "gemini-3-pro",
            None,
            "routing-chat",
            None,
            false,
        );
        let model_parts = |body: &serde_json::Value| {
            let contents = body["request"]["contents"].as_array().unwrap();
            let model_msg = contents.iter().find(|m| m["role"] == "model").unwrap();
            model_msg["parts"].clone()
        };
        let resp_parts = model_parts(&resp_result);
        let chat_parts = model_parts(&chat_result);
        let thought = resp_parts
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p.get("thought") == Some(&json!(true)))
            .unwrap();
        assert_eq!(thought["text"], client_thought);
        assert!(thought.get("thoughtSignature").is_none());
        assert_eq!(resp_parts, chat_parts);
        let dumped = serde_json::to_string(&resp_parts).unwrap();
        assert!(
            !dumped.contains(&valid_client_sig),
            "a signature that fails Gemini validation must not survive on either protocol"
        );
    }

    #[test]
    fn test_shell_tool_preserves_description_parameter_for_gemini() {
        let req = OpenAIRequest {
            model: "gemini-2.5-pro".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("run command".to_string())),
                ..Default::default()
            }],
            tools: Some(vec![json!({
                "type": "function",
                "function": {
                    "name": "run_command",
                    "description": "Run a shell command",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "command": { "type": "string", "description": "CLI command" },
                            "description": { "type": "string", "description": "Optional human label" }
                        },
                        "required": ["command", "description"]
                    }
                }
            })]),
            ..Default::default()
        };

        let (result, _, _, _) = transform_openai_request_with_session(
            &req,
            "test-proj",
            "gemini-2.5-pro",
            None,
            "routing-1",
            None,
            false,
        );

        let tools = result["request"]["tools"].as_array().unwrap();
        let func_decls = tools[0]["functionDeclarations"].as_array().unwrap();
        let run_cmd = func_decls
            .iter()
            .find(|f| f["name"] == "run_command")
            .unwrap();
        let props = run_cmd["parameters"]["properties"].as_object().unwrap();
        assert!(props.contains_key("command"));
        assert!(
            props.contains_key("description"),
            "description parameter must be preserved under pure passthrough"
        );
        let req_arr = run_cmd["parameters"]["required"].as_array().unwrap();
        assert!(req_arr.iter().any(|v| v == "command"));
        assert!(
            req_arr.iter().any(|v| v == "description"),
            "description parameter must be preserved in required"
        );
    }

    #[test]
    fn test_multi_turn_responses_preserves_historical_signature_prefix() {
        let sid = format!("test-sess-{}", uuid::Uuid::new_v4());
        use base64::Engine;
        let mut raw1 = vec![0x12u8, 1];
        raw1.extend_from_slice(&[b'A'; 60]);
        let sig_round_1 = base64::engine::general_purpose::STANDARD.encode(raw1);

        let mut raw2 = vec![0x12u8, 2];
        raw2.extend_from_slice(&[b'B'; 60]);
        let sig_round_2 = base64::engine::general_purpose::STANDARD.encode(raw2);

        let call_1_id = format!("call_1_{}", uuid::Uuid::new_v4());
        let call_2_id = format!("call_2_{}", uuid::Uuid::new_v4());

        // 缓存第 1 轮工具的专属签名
        let prev_resp_id = format!("resp-prev-{}", uuid::Uuid::new_v4());
        crate::proxy::SignatureCache::global().cache_tool_signature(
            &prev_resp_id,
            &call_1_id,
            sig_round_1.clone(),
        );

        // 模拟第 2 轮刚完成，产生了会话级别的最新签名 sig_round_2 (通过 previous_response_id)
        crate::proxy::SignatureCache::global().cache_session_signature(
            &prev_resp_id,
            sig_round_2.clone(),
            3,
        );

        // 构造第 3 轮请求：包含历史第 1 轮、第 2 轮的完整上下文
        let req = OpenAIRequest {
            model: "gemini-3.8-flash-high".to_string(),
            messages: vec![
                OpenAIMessage {
                    role: "user".to_string(),
                    content: Some(OpenAIContent::String("第 1 轮指令".to_string())),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![ToolCall {
                        id: call_1_id.clone(),
                        r#type: "function".to_string(),
                        function: Some(ToolFunction {
                            name: "run_command".to_string(),
                            arguments: "{\"command\":\"ls\"}".to_string(),
                        }),
                        status: None,
                        call_id: None,
                        operation: None,
                        signature: None,
                    }]),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "tool".to_string(),
                    tool_call_id: Some(call_1_id),
                    content: Some(OpenAIContent::String("file1.txt".to_string())),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![ToolCall {
                        id: call_2_id.clone(),
                        r#type: "function".to_string(),
                        function: Some(ToolFunction {
                            name: "run_command".to_string(),
                            arguments: "{\"command\":\"cat file1.txt\"}".to_string(),
                        }),
                        status: None,
                        call_id: None,
                        operation: None,
                        signature: None,
                    }]),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "tool".to_string(),
                    tool_call_id: Some(call_2_id),
                    content: Some(OpenAIContent::String("hello world".to_string())),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "user".to_string(),
                    content: Some(OpenAIContent::String("第 3 轮指令".to_string())),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let (result, _, _, _) = transform_openai_request_with_session(
            &req,
            "test-proj",
            "gemini-3.8-flash-high",
            None,
            &sid,
            Some(&prev_resp_id),
            true, // is_responses_api
        );

        let contents = result["request"]["contents"].as_array().unwrap();

        // 验证：第 1 轮 model
        let model_1_parts = contents[1]["parts"].as_array().unwrap();
        let fc_1 = model_1_parts
            .iter()
            .find(|p| p.get("functionCall").is_some())
            .unwrap();
        let sig_1 = fc_1["thoughtSignature"].as_str().unwrap();
        assert_eq!(sig_1, sig_round_1);
        // 核心断言：历史第 1 轮绝不能被最新一轮的签名 sig_round_2 覆盖！
        assert_ne!(sig_1, sig_round_2, "历史第 1 轮绝不能被最新签名覆盖");

        // 验证：最新一条 model（第 2 轮）
        let model_2_parts = contents[3]["parts"].as_array().unwrap();
        let fc_2 = model_2_parts
            .iter()
            .find(|p| p.get("functionCall").is_some())
            .unwrap();
        let sig_2 = fc_2["thoughtSignature"].as_str().unwrap();
        // 最新一条 model 应当正确采纳 prev_resp_id 的签名
        assert_eq!(sig_2, sig_round_2, "最新一条 model 应当正确继承上一轮签名");
    }

    #[test]
    fn test_transform_openai_request_preserves_command_with_description() {
        let req = OpenAIRequest {
            model: "gemini-3.8-flash-high".to_string(),
            messages: vec![
                OpenAIMessage {
                    role: "user".to_string(),
                    content: Some(OpenAIContent::String("执行命令".to_string())),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "assistant".to_string(),
                    tool_calls: Some(vec![ToolCall {
                        id: "call_test_1".to_string(),
                        r#type: "function".to_string(),
                        function: Some(ToolFunction {
                            name: "run_command".to_string(),
                            arguments: serde_json::to_string(&json!({
                                "description": "Run: git checkout main",
                                "command": "git checkout main",
                                "shell": "default"
                            }))
                            .unwrap(),
                        }),
                        signature: None,
                        status: None,
                        call_id: None,
                        operation: None,
                    }]),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "tool".to_string(),
                    tool_call_id: Some("call_test_1".to_string()),
                    content: Some(OpenAIContent::String("Switched to branch main".to_string())),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let (result, _, _, _) =
            transform_openai_request(&req, "test-proj", "gemini-3.8-flash-high", None);
        let contents = result["request"]["contents"].as_array().unwrap();

        // 验证 assistant 的 functionCall.args 中的 command 绝对未被剔除
        let model_parts = contents[1]["parts"].as_array().unwrap();
        let fc = model_parts
            .iter()
            .find(|p| p.get("functionCall").is_some())
            .unwrap();
        let args = &fc["functionCall"]["args"];
        assert_eq!(
            args["command"], "git checkout main",
            "command 参数必须完好保留！"
        );
        assert_eq!(
            args["description"], "Run: git checkout main",
            "description 参数必须完好保留！"
        );
        assert_eq!(args["shell"], "default", "shell 参数必须完好保留！");
    }

    #[test]
    fn test_transform_openai_request_handles_colliding_tool_call_ids() {
        let req = OpenAIRequest {
            model: "gemini-3.8-flash-high".to_string(),
            messages: vec![
                OpenAIMessage {
                    role: "user".to_string(),
                    content: Some(OpenAIContent::String("修改文件".to_string())),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "assistant".to_string(),
                    tool_calls: Some(vec![ToolCall {
                        id: "call_duplicate_1025976".to_string(),
                        r#type: "function".to_string(),
                        function: Some(ToolFunction {
                            name: "edit_file".to_string(),
                            arguments: "{}".to_string(),
                        }),
                        signature: None,
                        status: None,
                        call_id: None,
                        operation: None,
                    }]),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "tool".to_string(),
                    tool_call_id: Some("call_duplicate_1025976".to_string()),
                    content: Some(OpenAIContent::String("ok".to_string())),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "assistant".to_string(),
                    tool_calls: Some(vec![ToolCall {
                        id: "call_duplicate_1025976".to_string(),
                        r#type: "function".to_string(),
                        function: Some(ToolFunction {
                            name: "run_command".to_string(),
                            arguments: "{\"command\":\"git status\"}".to_string(),
                        }),
                        signature: None,
                        status: None,
                        call_id: None,
                        operation: None,
                    }]),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "tool".to_string(),
                    tool_call_id: Some("call_duplicate_1025976".to_string()),
                    content: Some(OpenAIContent::String("clean".to_string())),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let (result, _, _, _) =
            transform_openai_request(&req, "test-proj", "gemini-3.8-flash-high", None);
        let contents = result["request"]["contents"].as_array().unwrap();

        // 验证第 1 轮工具响应的名字为 edit_file，绝不能被后续同 ID 的 run_command 覆盖
        let tool_resp_1 = contents[2]["parts"][0]["functionResponse"]["name"]
            .as_str()
            .unwrap();
        assert_eq!(
            tool_resp_1, "edit_file",
            "第 1 轮工具响应必须匹配其调用时的 edit_file 工具名"
        );

        // 验证第 2 轮工具响应的名字为 run_command
        let tool_resp_2 = contents[4]["parts"][0]["functionResponse"]["name"]
            .as_str()
            .unwrap();
        assert_eq!(
            tool_resp_2, "run_command",
            "第 2 轮工具响应必须匹配其调用时的 run_command 工具名"
        );
    }

    #[test]
    fn test_tool_result_multimodal_image_extraction_and_passthrough() {
        let fake_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let tool_content = format!(
            "Here is the screenshot: ![screen](data:image/png;base64,{}) and some log text",
            fake_b64
        );

        let req = OpenAIRequest {
            model: "gemini-2.5-flash".to_string(),
            messages: vec![
                OpenAIMessage {
                    role: "user".to_string(),
                    content: Some(OpenAIContent::String("Take screenshot".to_string())),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "assistant".to_string(),
                    tool_calls: Some(vec![ToolCall {
                        id: "call_shot_1".to_string(),
                        r#type: "function".to_string(),
                        function: Some(ToolFunction {
                            name: "screenshot".to_string(),
                            arguments: "{}".to_string(),
                        }),
                        ..Default::default()
                    }]),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "tool".to_string(),
                    tool_call_id: Some("call_shot_1".to_string()),
                    content: Some(OpenAIContent::String(tool_content)),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let (result, _, _, _) =
            transform_openai_request(&req, "test-proj", "gemini-2.5-flash", None);
        let contents = result["request"]["contents"].as_array().unwrap();
        let tool_turn_parts = contents[2]["parts"].as_array().unwrap();

        // 验证同时存在 functionResponse 和 inlineData 两个 parts
        assert_eq!(tool_turn_parts.len(), 2);
        assert!(tool_turn_parts[0].get("functionResponse").is_some());
        assert!(tool_turn_parts[1].get("inlineData").is_some());

        let inline_data = &tool_turn_parts[1]["inlineData"];
        assert_eq!(inline_data["mimeType"], "image/png");
        assert_eq!(inline_data["data"], fake_b64);

        // 验证文本中的 base64 已被替换为摘要说明，防止 functionResponse 体积膨胀
        let func_res_str = tool_turn_parts[0]["functionResponse"]["response"]["output"]
            .as_str()
            .unwrap();
        assert!(!func_res_str.contains(fake_b64));
        assert!(func_res_str.contains("[Image: forwarded to visual input (image/png)]"));
    }

    #[test]
    fn test_function_declarations_schema_sanitization() {
        let req = OpenAIRequest {
            model: "gemini-2.5-flash".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Hello".to_string())),
                ..Default::default()
            }],
            tools: Some(vec![json!({
                "type": "function",
                "function": {
                    "name": "complex_tool",
                    "description": "A complex tool\nwith multi-line\r\ndescriptions",
                    "parameters": {
                        "type": "object"
                        // 故意省略 properties
                    }
                }
            })]),
            ..Default::default()
        };

        let (result, _, _, _) =
            transform_openai_request(&req, "test-proj", "gemini-2.5-flash", None);
        let tools = result["request"]["tools"].as_array().unwrap();
        let func_decls = tools[0]["functionDeclarations"].as_array().unwrap();
        let decl = &func_decls[0];

        // 验证 description 保持原样排版格式（不超过 MAX_DESCRIPTION_LENGTH 时）
        assert_eq!(
            decl["description"],
            "A complex tool\nwith multi-line\r\ndescriptions"
        );

        // 验证 parameters 保证包含 OBJECT 和 properties: {}
        assert_eq!(decl["parameters"]["type"], "OBJECT");
        assert_eq!(decl["parameters"]["properties"], json!({}));
    }

    #[test]
    fn test_openai_tool_call_retrieves_signature_from_signature_cache() {
        let tool_id = "call_cached_test_999";
        let valid_gemini_sig = "EmIKYAFpFH0TDqviLY1vZ8EuHqBLLj5xxD+0hchYg2VaoyolUQRP+hSCsKRpSpj+yrQA2H27yVFnF7tlp5OHIUvTdZKKErAqILJzK5FG8RJg42jCaaI2/iwqoBuRd5BDVwBxaQ==";
        crate::proxy::SignatureCache::global().cache_tool_signature(
            "scope-test",
            tool_id,
            valid_gemini_sig.to_string(),
        );

        let req = OpenAIRequest {
            model: "gemini-3.8-flash".to_string(),
            session_id: Some("scope-test".to_string()),
            messages: vec![
                OpenAIMessage {
                    role: "user".to_string(),
                    content: Some(OpenAIContent::String("Run tool".to_string())),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "assistant".to_string(),
                    tool_calls: Some(vec![ToolCall {
                        id: tool_id.to_string(),
                        r#type: "function".to_string(),
                        function: Some(ToolFunction {
                            name: "bash".to_string(),
                            arguments: "{}".to_string(),
                        }),
                        signature: None, // 客户端未带签名 (标准 OpenAI 协议)
                        ..Default::default()
                    }]),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "tool".to_string(),
                    tool_call_id: Some(tool_id.to_string()),
                    content: Some(OpenAIContent::String("done".to_string())),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let (result, _, _, _) =
            transform_openai_request(&req, "test-proj", "gemini-3.8-flash-tiered", None);
        let contents = result["request"]["contents"].as_array().unwrap();

        // 查找 model 轮次中的 functionCall 部件
        let model_msg = contents
            .iter()
            .find(|c| c["role"] == "model")
            .expect("must find model message");
        let fc_part = model_msg["parts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p.get("functionCall").is_some())
            .expect("must find functionCall part");

        assert_eq!(
            fc_part["thoughtSignature"], valid_gemini_sig,
            "OpenAI adapter and pipeline must recover real tool signature from SignatureCache"
        );
    }
}
