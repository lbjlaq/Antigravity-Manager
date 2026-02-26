// Gemini v1internal 包装/解包
use serde_json::{json, Value};

/// 包装请求体为 v1internal 格式
pub fn wrap_request(
    body: &Value,
    project_id: &str,
    mapped_model: &str,
    account_id: Option<&str>,
    session_id: Option<&str>,
    max_output_tokens_cap: Option<u64>, // [NEW] 动态或静态默认限额，优先级：动态账号数据 > 静态默认表 > 131072
) -> Value {
    // 优先使用传入的 mapped_model，其次尝试从 body 获取
    let original_model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or(mapped_model);

    // 如果 mapped_model 是空的，则使用 original_model
    let final_model_name = if !mapped_model.is_empty() {
        mapped_model
    } else {
        original_model
    };

    // [ADDED v4.1.24] 计算 message_count 供 requestId 使用
    let message_count = body.get("contents")
        .and_then(|c| c.as_array())
        .map(|a| a.len())
        .unwrap_or(1);

    // 复制 body 以便修改
    let mut inner_request = body.clone();

    // 深度清理 [undefined] 字符串 (Cherry Studio 等客户端常见注入)
    crate::proxy::mappers::common_utils::deep_clean_undefined(&mut inner_request, 0);

    // [FIX #1522] Inject dummy IDs for Claude models in Gemini protocol
    // Google v1internal requires 'id' for tool calls when the model is Claude,
    // even though the standard Gemini protocol doesn't have it.
    let is_target_claude = final_model_name.to_lowercase().contains("claude");

    if let Some(contents) = inner_request
        .get_mut("contents")
        .and_then(|c| c.as_array_mut())
    {
        for content in contents {
            // 每条消息维护独立的计数器，确保 Call 和对应的 Response 生成相同的 ID (兜底规则)
            let mut name_counters: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();

            if let Some(parts) = content.get_mut("parts").and_then(|p| p.as_array_mut()) {
                for part in parts {
                    if let Some(obj) = part.as_object_mut() {
                        // 1. 处理 functionCall (Assistant 请求调用工具)
                        if let Some(fc) = obj.get_mut("functionCall") {
                            if fc.get("id").is_none() && is_target_claude {
                                let name =
                                    fc.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                                let count = name_counters.entry(name.to_string()).or_insert(0);
                                let call_id = format!("call_{}_{}", name, count);
                                *count += 1;

                                fc.as_object_mut()
                                    .unwrap()
                                    .insert("id".to_string(), json!(call_id));
                                tracing::debug!("[Gemini-Wrap] Request stage: Injected missing call_id '{}' for Claude model", call_id);
                            }
                        }

                        // 2. 处理 functionResponse (User 回复工具结果)
                        if let Some(fr) = obj.get_mut("functionResponse") {
                            if fr.get("id").is_none() && is_target_claude {
                                // 启发：如果客户端（如 OpenCode）在响应时没带 ID，说明它收到响应时就没 ID。
                                // 我们在这里生成的 ID 必须与我们在 inject_ids_to_response 中注入响应的 ID 一致。
                                let name =
                                    fr.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                                let count = name_counters.entry(name.to_string()).or_insert(0);
                                let call_id = format!("call_{}_{}", name, count);
                                *count += 1;

                                fr.as_object_mut()
                                    .unwrap()
                                    .insert("id".to_string(), json!(call_id));
                                tracing::debug!("[Gemini-Wrap] Request stage: Injected synced response_id '{}' for Claude model", call_id);
                            }
                        }

                        // 3. 处理 thoughtSignature (原有逻辑保持)
                        if obj.contains_key("functionCall") && obj.get("thoughtSignature").is_none()
                        {
                            if let Some(s_id) = session_id {
                                if let Some(sig) = crate::proxy::SignatureCache::global()
                                    .get_session_signature(s_id)
                                {
                                    obj.insert("thoughtSignature".to_string(), json!(sig));
                                    tracing::debug!("[Gemini-Wrap] Injected signature (len: {}) for session: {}", sig.len(), s_id);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // [FIX Issue #1355] Gemini Flash thinking budget capping
    // [CONFIGURABLE] 现在改为遵循全局 Thinking Budget 配置
    // [FIX #1557] Also apply to Pro/Thinking models to ensure budget processing
    // [FIX #1557] Auto-inject thinkingConfig if missing for these models
    let lower_model = final_model_name.to_lowercase();
    if lower_model.contains("flash")
        || lower_model.contains("pro")
        || lower_model.contains("thinking")
    {
        // [NEW] Extract OpenAI-style max_tokens before mutably borrowing gen_config
        let req_max_tokens = inner_request.get("max_tokens").and_then(|v| v.as_u64());

        // Determine model family and capability beforehand to avoid borrow checker conflicts
        let is_claude = lower_model.contains("claude");
        let is_preview = lower_model.contains("preview");
        let should_inject = lower_model.contains("thinking")
            || (lower_model.contains("gemini-2.0-pro") && !is_preview)
            || (lower_model.contains("gemini-3-pro") && !is_preview)
            || (lower_model.contains("gemini-3.1-pro") && !is_preview);

        if should_inject {
            // Scope for borrowing inner_request/gen_config
            let mut has_thinking = false;
            if is_claude {
                has_thinking = inner_request.get("thinking").is_some();
            } else {
                if let Some(gc) = inner_request.get("generationConfig").and_then(|v| v.as_object()) {
                    has_thinking = gc.get("thinkingConfig").is_some();
                }
            }

            if !has_thinking {
                tracing::debug!(
                    "[Gemini-Wrap] Auto-injecting default thinking for {}",
                    final_model_name
                );

                // [FIX] 统一注入到 generationConfig.thinkingConfig
                // Claude 模型使用 16000 预算，Gemini 使用 24576
                let default_budget = if is_claude { 16000 } else { 24576 };
                
                let gen_config = inner_request
                    .as_object_mut()
                    .unwrap()
                    .entry("generationConfig")
                    .or_insert(json!({}))
                    .as_object_mut()
                    .unwrap();
                
                gen_config.insert(
                    "thinkingConfig".to_string(),
                    json!({
                        "includeThoughts": true,
                        "thinkingBudget": default_budget
                    }),
                );
            }
        }

        // Re-acquire gen_config to satisfy borrow checker and scope requirements for later logic
        let gen_config = inner_request
            .as_object_mut()
            .unwrap()
            .entry("generationConfig")
            .or_insert(json!({}))
            .as_object_mut()
            .unwrap();

        // [ADDED v4.1.24] Inject topK=40 and topP=1.0 if not present to match official client
        if !gen_config.contains_key("topK") {
            gen_config.insert("topK".to_string(), json!(40));
        }
        if !gen_config.contains_key("topP") {
            gen_config.insert("topP".to_string(), json!(1.0));
        }

        // [FIX] Convert v1beta thinkingLevel (string) to v1internal thinkingBudget (number).
        // Clients (e.g. OpenClaw, Cline) may send thinkingLevel which v1internal does not accept,
        // causing 400 INVALID_ARGUMENT. Convert before any budget processing below.
        if let Some(thinking_config) = gen_config.get_mut("thinkingConfig") {
            if let Some(level) = thinking_config.get("thinkingLevel").and_then(|v| v.as_str()).map(|s| s.to_uppercase()) {
                let budget: i64 = match level.as_str() {
                    "NONE" => 0,
                    "LOW" => 4096,
                    "MEDIUM" => 8192,
                    "HIGH" => 24576,
                    _ => 8192, // safe default
                };
                tracing::info!(
                    "[Gemini-Wrap] Converting thinkingLevel '{}' to thinkingBudget {}",
                    level, budget
                );
                if let Some(tc) = thinking_config.as_object_mut() {
                    tc.remove("thinkingLevel");
                    tc.insert("thinkingBudget".to_string(), json!(budget));
                }
            }
        }

        if let Some(thinking_config) = gen_config.get_mut("thinkingConfig") {
            if let Some(budget_val) = thinking_config.get("thinkingBudget") {
                if let Some(budget_i64) = budget_val.as_i64() {
                    // [NEW] -1 indicates native dynamic mode, skip capping
                    if budget_i64 != -1 {
                        let budget = budget_i64 as u64;
                        let tb_config = crate::proxy::config::get_thinking_budget_config();
                        let final_budget = match tb_config.mode {
                            crate::proxy::config::ThinkingBudgetMode::Passthrough => budget,
                            crate::proxy::config::ThinkingBudgetMode::Custom => {
                                let val = tb_config.custom_value as u64;
                                let is_limited = (final_model_name.contains("gemini")
                                    || final_model_name.contains("thinking"))
                                    && !final_model_name.contains("-image");

                                if is_limited && val > 24576 {
                                    24576
                                } else {
                                    val
                                }
                            }
                            crate::proxy::config::ThinkingBudgetMode::Auto => {
                                let is_limited = (final_model_name.contains("gemini")
                                    || final_model_name.contains("thinking"))
                                    && !final_model_name.contains("-image");

                                if is_limited && budget > 24576 {
                                    24576
                                } else {
                                    budget
                                }
                            }
                            crate::proxy::config::ThinkingBudgetMode::Adaptive => budget,
                        };

                        if final_budget != budget {
                            thinking_config["thinkingBudget"] = json!(final_budget);
                        }
                    }
                }
            }
        }

        // [FIX #1747] Ensure max_tokens (maxOutputTokens) is greater than thinking_budget
        // Google v1internal requires maxOutputTokens > thinkingBudget.
        // [FIX #1825] Handle adaptive fallback (incl. -1 and thinkingLevel)
        let thinking_config_opt = gen_config.get("thinkingConfig");
        let is_adaptive = thinking_config_opt.map_or(false, |t| {
            t.get("thinkingLevel").is_some() || t.get("thinkingBudget").and_then(|v| v.as_i64()) == Some(-1)
        }) || (thinking_config_opt.and_then(|t| t.get("thinkingBudget").and_then(|v| v.as_u64())) == Some(32768) && is_claude);

        if let Some(thinking_config) = gen_config.get("thinkingConfig") {
            let budget_opt = thinking_config.get("thinkingBudget").and_then(|v| v.as_i64());
            
            // For adaptive or dynamic mode, we only need to ensure max tokens is large.
            // For fixed budget, we must satisfy maxOutputTokens > thinkingBudget.
            let current_max = gen_config
                .get("maxOutputTokens")
                .and_then(|v| v.as_u64())
                .or(req_max_tokens);

            if is_adaptive {
                if current_max.map_or(true, |m| m < 131072) {
                     gen_config.insert("maxOutputTokens".to_string(), json!(131072));
                }
            } else if let Some(budget_i64) = budget_opt {
                if budget_i64 > 0 {
                    let budget = budget_i64 as u64;
                    let min_required_max = budget + 8192;
                    if current_max.map_or(true, |m| m <= budget) {
                        tracing::info!(
                            "[Gemini-Wrap] Bumping maxOutputTokens from {:?} to {} to satisfy thinkingBudget ({})",
                            current_max, min_required_max, budget
                        );
                        gen_config.insert("maxOutputTokens".to_string(), json!(min_required_max));
                    }
                }
            }
        }
    }

    // [NEW] 按模型对 maxOutputTokens 进行三层限额 (Dynamic > Static Default > 131072)
    // 修复: gemini-cli 等客户端发送的 131072 超过部分模型支持的上限，导致 v1internal 返回 400 INVALID_ARGUMENT
    {
        let final_cap = crate::proxy::mappers::model_limits::get_model_output_limit(
            final_model_name,
            max_output_tokens_cap,
        );
        let gen_config = inner_request
            .as_object_mut()
            .unwrap()
            .entry("generationConfig")
            .or_insert(serde_json::json!({}))
            .as_object_mut()
            .unwrap();
        if let Some(current) = gen_config.get("maxOutputTokens").and_then(|v| v.as_u64()) {
            if current > final_cap {
                tracing::debug!(
                    "[Gemini-Wrap] Capped maxOutputTokens from {} to {} for model {} (dynamic={:?})",
                    current, final_cap, final_model_name, max_output_tokens_cap
                );
                gen_config.insert("maxOutputTokens".to_string(), serde_json::json!(final_cap));
            }
        }
    }

    // This caused upstream to return empty/invalid responses, leading to 'NoneType' object has no attribute 'strip' in Python clients.
    // relying on upstream defaults or user provided values is safer.

    // 提取 tools 列表以进行联网探测 (Gemini 风格可能是嵌套的)
    let tools_val: Option<Vec<Value>> = inner_request
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|arr| arr.clone());

    // [FIX] Extract OpenAI-compatible image parameters from root (for gemini-3-pro-image)
    let size = body.get("size").and_then(|v| v.as_str());
    let quality = body.get("quality").and_then(|v| v.as_str());
    let image_size = body.get("imageSize").and_then(|v| v.as_str()); // [NEW] Direct imageSize support

    // Use shared grounding/config logic
    let config = crate::proxy::mappers::common_utils::resolve_request_config(
        original_model,
        final_model_name,
        &tools_val,
        size,       // [FIX] Pass size parameter
        quality,    // [FIX] Pass quality parameter
        image_size, // [NEW] Pass direct imageSize parameter
        Some(body), // [NEW] Pass request body for imageConfig parsing
    );

    // Clean tool declarations (remove forbidden Schema fields like multipleOf, and remove redundant search decls)
    if let Some(tools) = inner_request.get_mut("tools") {
        if let Some(tools_arr) = tools.as_array_mut() {
            for tool in tools_arr {
                if let Some(decls) = tool.get_mut("functionDeclarations") {
                    if let Some(decls_arr) = decls.as_array_mut() {
                        // 1. 过滤掉联网关键字函数
                        decls_arr.retain(|decl| {
                            if let Some(name) = decl.get("name").and_then(|v| v.as_str()) {
                                if name == "web_search" || name == "google_search" {
                                    return false;
                                }
                            }
                            true
                        });

                        // 2. 清洗剩余 Schema
                        // [FIX] Gemini CLI 使用 parametersJsonSchema，而标准 Gemini API 使用 parameters
                        // 需要将 parametersJsonSchema 重命名为 parameters
                        for decl in decls_arr {
                            // 检测并转换字段名
                            if let Some(decl_obj) = decl.as_object_mut() {
                                // 如果存在 parametersJsonSchema，将其重命名为 parameters
                                if let Some(params_json_schema) =
                                    decl_obj.remove("parametersJsonSchema")
                                {
                                    let mut params = params_json_schema;
                                    crate::proxy::common::json_schema::clean_json_schema(
                                        &mut params,
                                    );
                                    decl_obj.insert("parameters".to_string(), params);
                                } else if let Some(params) = decl_obj.get_mut("parameters") {
                                    // 标准 parameters 字段
                                    crate::proxy::common::json_schema::clean_json_schema(params);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    tracing::debug!(
        "[Debug] Gemini Wrap: original='{}', mapped='{}', final='{}', type='{}'",
        original_model,
        final_model_name,
        config.final_model,
        config.request_type
    );

    // Inject googleSearch tool if needed
    if config.inject_google_search {
        crate::proxy::mappers::common_utils::inject_google_search_tool(&mut inner_request);
    }

    // Inject imageConfig if present (for image generation models)
    if let Some(image_config) = config.image_config {
        if let Some(obj) = inner_request.as_object_mut() {
            // 1. Filter tools: remove tools for image gen
            obj.remove("tools");

            // 2. Remove systemInstruction (image generation does not support system prompts)
            obj.remove("systemInstruction");

            // [FIX] Ensure 'role' field exists for all contents (Native clients might omit it)
            if let Some(contents) = obj.get_mut("contents").and_then(|c| c.as_array_mut()) {
                for content in contents {
                    if let Some(c_obj) = content.as_object_mut() {
                        if !c_obj.contains_key("role") {
                            c_obj.insert("role".to_string(), json!("user"));
                        }
                    }
                }
            }

            // 3. Clean generationConfig (remove responseMimeType, responseModalities etc.)
            let gen_config = obj.entry("generationConfig").or_insert_with(|| json!({}));
            if let Some(gen_obj) = gen_config.as_object_mut() {
                // [NEW] 根据全局配置决定是否保留 thinkingConfig
                let image_thinking_mode = crate::proxy::config::get_image_thinking_mode();
                tracing::debug!("[Gemini-Wrap] Image thinking mode: {}", image_thinking_mode);
                
                if image_thinking_mode == "disabled" {
                    // [FIX] Explicitly disable thinking instead of just removing the config
                    // Removing it might cause the model to fallback to default (which might be ON)
                    gen_obj.insert("thinkingConfig".to_string(), json!({
                        "includeThoughts": false
                    }));
                    tracing::debug!("[Gemini-Wrap] Image thinking mode disabled: set includeThoughts=false");
                }
                
                gen_obj.remove("responseMimeType");
                gen_obj.remove("responseModalities"); // Cherry Studio sends this, might conflict
                gen_obj.insert("imageConfig".to_string(), image_config);
            }
        }
    } else {
        // [NEW] 只在非图像生成模式下注入 Antigravity 身份 (原始简化版)
        let antigravity_identity = "You are Antigravity, a powerful agentic AI coding assistant designed by the Google Deepmind team working on Advanced Agentic Coding.\n\
        You are pair programming with a USER to solve their coding task. The task may require creating a new codebase, modifying or debugging an existing codebase, or simply answering a question.\n\
        **Absolute paths only**\n\
        **Proactiveness**";

        // [HYBRID] 检查是否已有 systemInstruction
        if let Some(system_instruction) = inner_request.get_mut("systemInstruction") {
            // [NEW] 补全 role: user
            if let Some(obj) = system_instruction.as_object_mut() {
                if !obj.contains_key("role") {
                    obj.insert("role".to_string(), json!("user"));
                }
            }

            if let Some(parts) = system_instruction.get_mut("parts") {
                if let Some(parts_array) = parts.as_array_mut() {
                    // 检查第一个 part 是否已包含 Antigravity 身份
                    let has_antigravity = parts_array
                        .get(0)
                        .and_then(|p| p.get("text"))
                        .and_then(|t| t.as_str())
                        .map(|s| s.contains("You are Antigravity"))
                        .unwrap_or(false);

                    if !has_antigravity {
                        // 在前面插入 Antigravity 身份
                        parts_array.insert(0, json!({"text": antigravity_identity}));
                    }

                    // [NEW] 注入全局系统提示词 (紧跟 Antigravity 身份之后，用户指令之前)
                    let global_prompt_config = crate::proxy::config::get_global_system_prompt();
                    if global_prompt_config.enabled
                        && !global_prompt_config.content.trim().is_empty()
                    {
                        // 插入位置：Antigravity 身份之后 (index 1)
                        let insert_pos = if has_antigravity { 1 } else { 1 };
                        if insert_pos <= parts_array.len() {
                            parts_array
                                .insert(insert_pos, json!({"text": global_prompt_config.content}));
                        } else {
                            parts_array.push(json!({"text": global_prompt_config.content}));
                        }
                    }
                }
            }
        } else {
            // 没有 systemInstruction,创建一个新的
            let mut parts = vec![json!({"text": antigravity_identity})];
            // [NEW] 注入全局系统提示词
            let global_prompt_config = crate::proxy::config::get_global_system_prompt();
            if global_prompt_config.enabled && !global_prompt_config.content.trim().is_empty() {
                parts.push(json!({"text": global_prompt_config.content}));
            }
            inner_request["systemInstruction"] = json!({
                "role": "user",
                "parts": parts
            });
        }
    }

    // [ADDED v4.1.24] 扩展 toolConfig 到 VALIDATED 模式
    if inner_request.get("tools").is_some() && !inner_request.get("toolConfig").is_some() {
        inner_request["toolConfig"] = json!({
            "functionCallingConfig": { "mode": "VALIDATED" }
        });
    }

    // [ADDED v4.1.24] 注入基于账号的稳定 sessionId
    if let Some(account_id_str) = account_id {
        inner_request["sessionId"] = json!(crate::proxy::common::session::derive_session_id(account_id_str));
    }

    let sid = session_id.unwrap_or("default");
    let final_request = json!({
        "project": project_id,
        // [CHANGED v4.1.24] Structured requestId to match official format
        "requestId": format!("agent/antigravity/{}/{}", &sid[..sid.len().min(8)], message_count),
        "request": inner_request,
        "model": config.final_model,
        "userAgent": "antigravity",
        // [CHANGED v4.1.24] Use "agent" for all non-image requests
        "requestType": if config.request_type == "image_gen" { "image_gen" } else { "agent" }
    });

    final_request
}

#[cfg(test)]
mod test_fixes {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_wrap_request_with_signature() {
        let session_id = "test-session-sig";
        let signature = "test-signature-must-be-longer-than-fifty-characters-to-be-cached-by-signature-cache-12345"; // > 50 chars
        crate::proxy::SignatureCache::global().cache_session_signature(
            session_id,
            signature.to_string(),
            1,
        );

        let body = json!({
            "model": "gemini-pro",
            "contents": [{
                "role": "user",
                "parts": [{
                    "functionCall": {
                        "name": "get_weather",
                        "args": {"location": "London"}
                    }
                }]
            }]
        });

        let result = wrap_request(&body, "proj", "gemini-pro", Some(session_id), None);
        let injected_sig = result["request"]["contents"][0]["parts"][0]["thoughtSignature"]
            .as_str()
            .unwrap();
        assert_eq!(injected_sig, signature);
    }
}

/// 解包响应（提取 response 字段）
pub fn unwrap_response(response: &Value) -> Value {
    response.get("response").unwrap_or(response).clone()
}

/// [NEW v3.3.18] 为 Claude 模型的 Gemini 响应自动注入 Tool ID
///
/// 目点是为了让客户端（如 OpenCode/Vercel AI SDK）能感知到 ID，
/// 并在下一轮对话中原样带回，从而满足 Google v1internal 对 Claude 模型的校验。
pub fn inject_ids_to_response(response: &mut Value, model_name: &str) {
    if !model_name.to_lowercase().contains("claude") {
        return;
    }

    if let Some(candidates) = response
        .get_mut("candidates")
        .and_then(|c| c.as_array_mut())
    {
        for candidate in candidates {
            if let Some(parts) = candidate
                .get_mut("content")
                .and_then(|c| c.get_mut("parts"))
                .and_then(|p| p.as_array_mut())
            {
                let mut name_counters: std::collections::HashMap<String, usize> =
                    std::collections::HashMap::new();
                for part in parts {
                    if let Some(fc) = part.get_mut("functionCall").and_then(|f| f.as_object_mut()) {
                        if fc.get("id").is_none() {
                            let name = fc.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                            let count = name_counters.entry(name.to_string()).or_insert(0);
                            let call_id = format!("call_{}_{}", name, count);
                            *count += 1;

                            fc.insert("id".to_string(), json!(call_id));
                            tracing::debug!("[Gemini-Wrap] Response stage: Injected synthetic call_id '{}' for client", call_id);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_wrap_request() {
        let body = json!({
            "model": "gemini-2.5-flash",
            "contents": [{"role": "user", "parts": [{"text": "Hi"}]}]
        });

        let result = wrap_request(&body, "test-project", "gemini-2.5-flash", None, None);
        assert_eq!(result["project"], "test-project");
        assert_eq!(result["model"], "gemini-2.5-flash");
        assert!(result["requestId"].as_str().unwrap().starts_with("agent-"));
    }

    #[test]
    fn test_unwrap_response() {
        let wrapped = json!({
            "response": {
                "candidates": [{"content": {"parts": [{"text": "Hello"}]}}]
            }
        });

        let result = unwrap_response(&wrapped);
        assert!(result.get("candidates").is_some());
        assert!(result.get("response").is_none());
    }

    #[test]
    fn test_antigravity_identity_injection_with_role() {
        let body = json!({
            "model": "gemini-pro",
            "messages": []
        });

        let result = wrap_request(&body, "test-proj", "gemini-pro", None, None);

        // 验证 systemInstruction
        let sys = result
            .get("request")
            .unwrap()
            .get("systemInstruction")
            .unwrap();
    }

    #[test]
    fn test_gemini_flash_thinking_budget_capping() {
        // Ensure default config (Auto mode)
        crate::proxy::config::update_thinking_budget_config(crate::proxy::config::ThinkingBudgetConfig::default());

        let body = json!({
            "model": "gemini-2.0-flash-thinking-exp",
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingBudget": 32000
                }
            }
        });

        // Test with Flash model
        let result = wrap_request(&body, "test-proj", "gemini-2.0-flash-thinking-exp", None, None);
        let req = result.get("request").unwrap();
        let gen_config = req.get("generationConfig").unwrap();
        let budget = gen_config["thinkingConfig"]["thinkingBudget"]
            .as_u64()
            .unwrap();

        // Should be capped at 24576
        assert_eq!(budget, 24576);

        // Test with Pro model (should NOT cap)
        let body_pro = json!({
            "model": "gemini-2.0-pro-exp",
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingBudget": 32000
                }
            }
        });
        let result_pro = wrap_request(&body_pro, "test-proj", "gemini-2.0-pro-exp", None, None);
        let budget_pro = result_pro["request"]["generationConfig"]["thinkingConfig"]
            ["thinkingBudget"]
            .as_u64()
            .unwrap();
        // [FIX #1592] Pro models now also capped to 24576 in wrap_request logic
        assert_eq!(budget_pro, 24576);
    }



    #[test]
    fn test_image_thinking_mode_disabled() {
        // 1. Set global mode to disabled
        crate::proxy::config::update_image_thinking_mode(Some("disabled".to_string()));

        // 2. Create a request for an image model (which triggers the image logic)
        // Note: resolve_request_config needs to return image_config for the logic to trigger
        // So we use a model name that resolves to image_gen
        let body = json!({
            "model": "gemini-3-pro-image-2k",
            "contents": [{"role": "user", "parts": [{"text": "Draw a cat"}]}]
        });

        let result = wrap_request(&body, "test-proj", "gemini-3-pro-image-2k", None, None);
        let req = result.get("request").unwrap();
        let gen_config = req.get("generationConfig").unwrap();
        
        // 3. Verify thinkingConfig has includeThoughts: false
        let thinking_config = gen_config.get("thinkingConfig").unwrap();
        assert_eq!(thinking_config["includeThoughts"], false);

        // 4. Reset global mode
        crate::proxy::config::update_image_thinking_mode(Some("enabled".to_string()));
    }

    #[test]
    fn test_user_instruction_preservation() {
        let body = json!({
            "model": "gemini-pro",
            "systemInstruction": {
                "role": "user",
                "parts": [{"text": "User custom prompt"}]
            }
        });

        let result = wrap_request(&body, "test-proj", "gemini-pro", None, None);
        let sys = result
            .get("request")
            .unwrap()
            .get("systemInstruction")
            .unwrap();
        let parts = sys.get("parts").unwrap().as_array().unwrap();

        // Should have 2 parts: Antigravity + User
        assert_eq!(parts.len(), 2);
        assert!(parts[0]
            .get("text")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("You are Antigravity"));
        assert_eq!(
            parts[1].get("text").unwrap().as_str().unwrap(),
            "User custom prompt"
        );
    }

    #[test]
    fn test_duplicate_prevention() {
        let body = json!({
            "model": "gemini-pro",
            "systemInstruction": {
                "parts": [{"text": "You are Antigravity..."}]
            }
        });

        let result = wrap_request(&body, "test-proj", "gemini-pro", None, None);
        let sys = result
            .get("request")
            .unwrap()
            .get("systemInstruction")
            .unwrap();
        let parts = sys.get("parts").unwrap().as_array().unwrap();

        // Should NOT inject duplicate, so only 1 part remains
        assert_eq!(parts.len(), 1);
    }

    #[test]
    fn test_image_generation_with_reference_images() {
        // Create 14 reference images + 1 text prompt
        let mut parts = Vec::new();
        parts.push(json!({"text": "Generate a variation"}));

        for _ in 0..14 {
            parts.push(json!({
                "inlineData": {
                    "mimeType": "image/jpeg",
                    "data": "base64data..."
                }
            }));
        }

        let body = json!({
            "model": "gemini-3-pro-image",
            "contents": [{"parts": parts}]
        });

        let result = wrap_request(&body, "test-proj", "gemini-3-pro-image", None, None);

        let request = result.get("request").unwrap();
        let contents = request.get("contents").unwrap().as_array().unwrap();
        let result_parts = contents[0].get("parts").unwrap().as_array().unwrap();

        // Verify all 15 parts (1 text + 14 images) are preserved
        assert_eq!(result_parts.len(), 15);
    }

    #[test]
    fn test_gemini_pro_thinking_budget_processing() {
        // Update global config to Custom mode to verify logic execution
        use crate::proxy::config::{
            update_thinking_budget_config, ThinkingBudgetConfig, ThinkingBudgetMode,
        };

        // Save old config (optional, but good practice if tests ran in parallel, but here it's fine)
        update_thinking_budget_config(ThinkingBudgetConfig {
            mode: ThinkingBudgetMode::Custom,
            custom_value: 1024, // Distinct value
            effort: None,
        });

        let body = json!({
            "model": "gemini-3-pro-preview",
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingBudget": 32000
                }
            }
        });

        // Test with Pro model
        let result = wrap_request(&body, "test-proj", "gemini-3-pro-preview", None, None);
        let req = result.get("request").unwrap();
        let gen_config = req.get("generationConfig").unwrap();

        let budget = gen_config["thinkingConfig"]["thinkingBudget"]
            .as_u64()
            .unwrap();

        // If logic executes, it sees Custom mode and sets 1024
        // If logic skipped, it keeps 32000
        assert_eq!(
            budget, 1024,
            "Budget should be overridden to 1024 by custom config, proving logic execution"
        );

        // Restore default (Auto 24576)
        update_thinking_budget_config(ThinkingBudgetConfig::default());
    }

    #[cfg(test)]
    mod test_v4_fixes {
        use super::*;
        use serde_json::json;

        #[test]
        fn test_claude_no_root_thinking_injection() {
            // 验证 Claude 模型不会在根目录注入 thinking，而是注入到 generationConfig.thinkingConfig
            // 并且 budget 默认为 16000
            
            // 使用 Auto 模式避免干扰
            crate::proxy::config::update_thinking_budget_config(
                crate::proxy::config::ThinkingBudgetConfig {
                    mode: crate::proxy::config::ThinkingBudgetMode::Auto,
                    custom_value: 0,
                    effort: None,
                },
            );

            let body = json!({
                "model": "claude-3-7-sonnet-thinking", 
                "messages": [{"role": "user", "content": "hi"}]
            });

            let result = wrap_request(&body, "proj", "claude-3-7-sonnet-thinking", None, None);
            let req = result.get("request").unwrap();

            // 1. 确保根目录没有 thinking
            assert!(req.get("thinking").is_none(), "Root level 'thinking' should NOT be present");

            // 2. 确保 generationConfig.thinkingConfig 存在
            let gen_config = req.get("generationConfig").expect("generationConfig should be present");
            let thinking_config = gen_config.get("thinkingConfig").expect("thinkingConfig should be injected");

            // 3. 验证 Claude 默认预算为 16000
            let budget = thinking_config["thinkingBudget"].as_u64().expect("thinkingBudget should be a number");
            assert_eq!(budget, 16000, "Claude default thinking budget should be 16000");
        }

        #[test]
        fn test_gemini_thinking_injection_default() {
            // 验证 Gemini 模型注入默认预算 24576
            let body = json!({
                "model": "gemini-2.0-flash-thinking-exp",
                "contents": [{"role": "user", "parts": [{"text": "hi"}]}]
            });

            let result = wrap_request(&body, "proj", "gemini-2.0-flash-thinking-exp", None, None);
            let req = result.get("request").unwrap();
            let gen_config = req.get("generationConfig").unwrap();
            let thinking_config = gen_config.get("thinkingConfig").unwrap();

            let budget = thinking_config["thinkingBudget"].as_u64().unwrap();
            assert_eq!(budget, 24576, "Gemini default thinking budget should be 24576");
        }
    }
}

    #[test]
    fn test_gemini_pro_auto_inject_thinking() {
        // Reset thinking budget to auto mode at the start to avoid interference from parallel tests
        crate::proxy::config::update_thinking_budget_config(
            crate::proxy::config::ThinkingBudgetConfig {
                mode: crate::proxy::config::ThinkingBudgetMode::Auto,
                custom_value: 24576,
                effort: None,
            },
        );

        // Request WITHOUT thinkingConfig
        let body = json!({
            "model": "gemini-3-pro-preview",
            // No generationConfig or empty one
            "generationConfig": {}
        });

        // Test with Pro-preview model (should NOT auto-inject to avoid 400)
        let result = wrap_request(&body, "test-proj", "gemini-3-pro-preview", None, None);
        let req = result.get("request").unwrap();
        let gen_config = req.get("generationConfig").unwrap();

        // Should NOT have auto-injected thinkingConfig
        assert!(
            gen_config.get("thinkingConfig").is_none(),
            "Should NOT auto-inject thinkingConfig for gemini-3-pro-preview to avoid 400 error"
        );

        // Test with standard gemini-3-pro (non-preview)
        let body_std = json!({
            "model": "gemini-3-pro",
            "generationConfig": {}
        });
        let result_std = wrap_request(&body_std, "test-proj", "gemini-3-pro", None, None);
        let gen_config_std = result_std.get("request").unwrap().get("generationConfig").unwrap();
        
        assert!(
            gen_config_std.get("thinkingConfig").is_some(),
            "Should still auto-inject thinkingConfig for standard gemini-3-pro"
        );
    }

    #[test]
    fn test_openai_image_params_support() {
        // Test Case 1: Standard Size + Quality (HD/4K)
        let body_1 = json!({
            "model": "gemini-3-pro-image",
            "size": "1920x1080",
            "quality": "hd",
            "prompt": "Test"
        });

        let result_1 = wrap_request(&body_1, "test-proj", "gemini-3-pro-image", None, None);
        let req_1 = result_1.get("request").unwrap();
        let gen_config_1 = req_1.get("generationConfig").unwrap();
        let image_config_1 = gen_config_1.get("imageConfig").unwrap();

        assert_eq!(image_config_1["aspectRatio"], "16:9");
        assert_eq!(image_config_1["imageSize"], "4K");

        // Test Case 2: Aspect Ratio String + Standard Quality
        let body_2 = json!({
            "model": "gemini-3-pro-image",
            "size": "1:1",
            "quality": "standard",
             "prompt": "Test"
        });

        let result_2 = wrap_request(&body_2, "test-proj", "gemini-3-pro-image", None, None);
        let req_2 = result_2.get("request").unwrap();
        let image_config_2 = req_2["generationConfig"]["imageConfig"]
            .as_object()
            .unwrap();

        assert_eq!(image_config_2["aspectRatio"], "1:1");
        assert_eq!(image_config_2["imageSize"], "1K");
    }
