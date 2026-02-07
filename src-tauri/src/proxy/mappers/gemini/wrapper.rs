// Gemini v1internal 包装/解包
use serde_json::{json, Value};

/// 包装请求体为 v1internal 格式
pub fn wrap_request(body: &Value, project_id: &str, mapped_model: &str, session_id: Option<&str>) -> Value {
    // 优先使用传入的 mapped_model，其次尝试从 body 获取
    let original_model = body.get("model").and_then(|v| v.as_str()).unwrap_or(mapped_model);
    
    // 如果 mapped_model 是空的，则使用 original_model
    let final_model_name = if !mapped_model.is_empty() {
        mapped_model
    } else {
        original_model
    };

    // 复制 body 以便修改
    let mut inner_request = body.clone();

    // 深度清理 [undefined] 字符串 (Cherry Studio 等客户端常见注入)
    crate::proxy::mappers::common_utils::deep_clean_undefined(&mut inner_request);

    // [FIX #765] Inject thought_signature into functionCall parts
    if let Some(s_id) = session_id {
        if let Some(contents) = inner_request.get_mut("contents").and_then(|c| c.as_array_mut()) {
            for content in contents {
                if let Some(parts) = content.get_mut("parts").and_then(|p| p.as_array_mut()) {
                    for part in parts {
                        if part.get("functionCall").is_some() {
                            // Only inject if it doesn't already have one
                            if part.get("thoughtSignature").is_none() {
                                if let Some(sig) = crate::proxy::SignatureCache::global().get_session_signature(s_id) {
                                    if let Some(obj) = part.as_object_mut() {
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
    }

    // [FIX #1592/#1602] Gemini thinking budget capping
    // Force cap thinking_budget to 24576 for ALL Gemini models to prevent 400 Bad Request
    // This applies to both Flash and Pro models
    {
        let tb_config = crate::proxy::config::get_thinking_budget_config();
        let lower_model = final_model_name.to_lowercase();
        let should_process_thinking = lower_model.contains("flash")
            || lower_model.contains("pro")
            || lower_model.contains("thinking");

        if should_process_thinking {
            if let Some(req_obj) = inner_request.as_object_mut() {
                let gen_cfg = req_obj
                    .entry("generationConfig".to_string())
                    .or_insert_with(|| json!({}));

                if let Some(gen_obj) = gen_cfg.as_object_mut() {
                    let should_inject_default = gen_obj.get("thinkingConfig").is_none()
                        && (lower_model.contains("thinking")
                            || lower_model.contains("gemini-2.0-pro")
                            || lower_model.contains("gemini-3-pro"));

                    if should_inject_default {
                        gen_obj.insert(
                            "thinkingConfig".to_string(),
                            json!({
                                "includeThoughts": true,
                                "thinkingBudget": 16000
                            }),
                        );
                    }

                    if let Some(thinking_config) = gen_obj.get_mut("thinkingConfig") {
                        if let Some(budget_val) = thinking_config.get("thinkingBudget") {
                            if let Some(budget) = budget_val.as_u64() {
                                let final_budget = match tb_config.mode {
                                    crate::proxy::config::ThinkingBudgetMode::Custom => {
                                        let custom = tb_config.custom_value as u64;
                                        if custom > 24576 {
                                            tracing::warn!(
                                                "[Gemini-Wrap] Custom mode: capping thinking_budget from {} to 24576 for model {}",
                                                custom,
                                                final_model_name
                                            );
                                            24576
                                        } else {
                                            custom
                                        }
                                    }
                                    crate::proxy::config::ThinkingBudgetMode::Passthrough => budget,
                                    crate::proxy::config::ThinkingBudgetMode::Auto => {
                                        if budget > 24576 {
                                            tracing::info!(
                                                "[Gemini-Wrap] Auto mode: capping thinking_budget from {} to 24576 for model {}",
                                                budget,
                                                final_model_name
                                            );
                                            24576
                                        } else {
                                            budget
                                        }
                                    }
                                };

                                if final_budget != budget {
                                    thinking_config["thinkingBudget"] = json!(final_budget);
                                }
                            }
                        } else if thinking_config
                            .get("includeThoughts")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                        {
                            thinking_config["thinkingBudget"] = json!(24576);
                            tracing::debug!(
                                "[Gemini-Wrap] Injected default thinkingBudget=24576 for model {}",
                                final_model_name
                            );
                        }
                    }
                }
            }
        }
    }

    // [FIX] Removed forced maxOutputTokens (64000) as it exceeds limits for Gemini 1.5 Flash/Pro standard models (8192).
    // This caused upstream to return empty/invalid responses, leading to 'NoneType' object has no attribute 'strip' in Python clients.
    // relying on upstream defaults or user provided values is safer.

    // 提取 tools 列表以进行联网探测 (Gemini 风格可能是嵌套的)
    let tools_val: Option<Vec<Value>> = inner_request.get("tools").and_then(|t| t.as_array()).map(|arr| {
        arr.clone()
    });

    // Use shared grounding/config logic
    let config = crate::proxy::mappers::common_utils::resolve_request_config(original_model, final_model_name, &tools_val, None, None);

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
                                if let Some(params_json_schema) = decl_obj.remove("parametersJsonSchema") {
                                    let mut params = params_json_schema;
                                    crate::proxy::common::json_schema::clean_json_schema(&mut params);
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

    tracing::debug!("[Debug] Gemini Wrap: original='{}', mapped='{}', final='{}', type='{}'", 
        original_model, final_model_name, config.final_model, config.request_type);
    
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

             // 3. Clean generationConfig (remove thinkingConfig, responseMimeType, responseModalities etc.)
             let gen_config = obj.entry("generationConfig").or_insert_with(|| json!({}));
             if let Some(gen_obj) = gen_config.as_object_mut() {
                 gen_obj.remove("thinkingConfig");
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
                    let has_antigravity = parts_array.get(0)
                        .and_then(|p| p.get("text"))
                        .and_then(|t| t.as_str())
                        .map(|s| s.contains("You are Antigravity"))
                        .unwrap_or(false);
                    
                    if !has_antigravity {
                        // 在前面插入 Antigravity 身份
                        parts_array.insert(0, json!({"text": antigravity_identity}));
                    }
                }
            }
        } else {
            // 没有 systemInstruction,创建一个新的
            inner_request["systemInstruction"] = json!({
                "role": "user",
                "parts": [{"text": antigravity_identity}]
            });
        }
    }

    let final_request = json!({
        "project": project_id,
        "requestId": format!("agent-{}", uuid::Uuid::new_v4()), // 修正为 agent- 前缀
        "request": inner_request,
        "model": config.final_model,
        "userAgent": "antigravity",
        "requestType": config.request_type
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
        crate::proxy::SignatureCache::global().cache_session_signature(session_id, signature.to_string(), 1);

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

        let result = wrap_request(&body, "proj", "gemini-pro", Some(session_id));
        let injected_sig = result["request"]["contents"][0]["parts"][0]["thoughtSignature"].as_str().unwrap();
        assert_eq!(injected_sig, signature);
    }
}

/// 解包响应（提取 response 字段）
pub fn unwrap_response(response: &Value) -> Value {
    response.get("response").unwrap_or(response).clone()
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

        let result = wrap_request(&body, "test-project", "gemini-2.5-flash", None);
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
        
        let result = wrap_request(&body, "test-proj", "gemini-pro", None);
        
        // 验证 systemInstruction
        let sys = result.get("request").unwrap().get("systemInstruction").unwrap();
        
        // 1. 验证 role: "user"
        assert_eq!(sys.get("role").unwrap(), "user");
        
        // 2. 验证 Antigravity 身份注入
        let parts = sys.get("parts").unwrap().as_array().unwrap();
        assert!(!parts.is_empty());
        let first_text = parts[0].get("text").unwrap().as_str().unwrap();
        assert!(first_text.contains("You are Antigravity"));
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

        let result = wrap_request(&body, "test-proj", "gemini-pro", None);
        let sys = result.get("request").unwrap().get("systemInstruction").unwrap();
        let parts = sys.get("parts").unwrap().as_array().unwrap();

        // Should have 2 parts: Antigravity + User
        assert_eq!(parts.len(), 2);
        assert!(parts[0].get("text").unwrap().as_str().unwrap().contains("You are Antigravity"));
        assert_eq!(parts[1].get("text").unwrap().as_str().unwrap(), "User custom prompt");
    }

    #[test]
    fn test_duplicate_prevention() {
        let body = json!({
            "model": "gemini-pro",
            "systemInstruction": {
                "parts": [{"text": "You are Antigravity..."}]
            }
        });

        let result = wrap_request(&body, "test-proj", "gemini-pro", None);
        let sys = result.get("request").unwrap().get("systemInstruction").unwrap();
        let parts = sys.get("parts").unwrap().as_array().unwrap();

        // Should NOT inject duplicate, so only 1 part remains
        assert_eq!(parts.len(), 1);
    }

    #[test]
    fn test_gemini_flash_thinking_budget_capping() {
        let body = json!({
            "model": "gemini-2.0-flash-thinking-exp",
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingBudget": 32000
                }
            }
        });

        // Test with Flash model - should cap at 24576
        let result = wrap_request(&body, "test-proj", "gemini-2.0-flash-thinking-exp", None);
        let req = result.get("request").unwrap();
        let gen_config = req.get("generationConfig").unwrap();
        let budget = gen_config["thinkingConfig"]["thinkingBudget"].as_u64().unwrap();
        assert_eq!(budget, 24576);

        // Test with Pro model - should cap at 24576 in Auto mode
        let body_pro = json!({
            "model": "gemini-2.0-pro-exp",
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingBudget": 32000
                }
            }
        });
        let result_pro = wrap_request(&body_pro, "test-proj", "gemini-2.0-pro-exp", None);
        let budget_pro = result_pro["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"].as_u64().unwrap();
        assert_eq!(budget_pro, 24576);
    }

    #[test]
    fn test_gemini_pro_thinking_budget_processing() {
        use crate::proxy::config::{
            update_thinking_budget_config, ThinkingBudgetConfig, ThinkingBudgetMode,
        };

        update_thinking_budget_config(ThinkingBudgetConfig {
            mode: ThinkingBudgetMode::Custom,
            custom_value: 1024,
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

        let result = wrap_request(&body, "test-proj", "gemini-3-pro-preview", None);
        let req = result.get("request").unwrap();
        let gen_config = req.get("generationConfig").unwrap();

        let budget = gen_config["thinkingConfig"]["thinkingBudget"]
            .as_u64()
            .unwrap();
        assert_eq!(budget, 1024);

        update_thinking_budget_config(ThinkingBudgetConfig::default());
    }

    #[test]
    fn test_gemini_pro_auto_inject_thinking() {
        let body = json!({
            "model": "gemini-3-pro-preview",
            "generationConfig": {}
        });

        let result = wrap_request(&body, "test-proj", "gemini-3-pro-preview", None);
        let req = result.get("request").unwrap();
        let gen_config = req.get("generationConfig").unwrap();

        assert!(gen_config.get("thinkingConfig").is_some());
        let budget = gen_config["thinkingConfig"]["thinkingBudget"]
            .as_u64()
            .unwrap();
        assert_eq!(budget, 16000);
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

        let result = wrap_request(&body, "test-proj", "gemini-3-pro-image", None);

        let request = result.get("request").unwrap();
        let contents = request.get("contents").unwrap().as_array().unwrap();
        let result_parts = contents[0].get("parts").unwrap().as_array().unwrap();

        // Verify all 15 parts (1 text + 14 images) are preserved
        assert_eq!(result_parts.len(), 15);
    }
}
