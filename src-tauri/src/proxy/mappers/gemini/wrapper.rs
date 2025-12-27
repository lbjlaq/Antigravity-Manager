// Gemini v1internal 包装/解包
use serde_json::{json, Value};

/// 包装请求体为 v1internal 格式
pub fn wrap_request(body: &Value, project_id: &str, mapped_model: &str) -> Value {
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

    // 强制设置 Gemini v1internal 的最大输出 token 数
    if let Some(obj) = inner_request.as_object_mut() {
        let gen_config = obj.entry("generationConfig").or_insert_with(|| json!({}));
        if let Some(gen_obj) = gen_config.as_object_mut() {
            gen_obj.insert("maxOutputTokens".to_string(), json!(64000)); // Sync with others
        }
    }

    // Use shared grounding/config logic
    let config = crate::proxy::mappers::common_utils::resolve_request_config(original_model, final_model_name);

    // Clean tool declarations (remove forbidden Schema fields like multipleOf)
    if let Some(tools) = inner_request.get_mut("tools") {
        if let Some(tools_arr) = tools.as_array_mut() {
            for tool in tools_arr {
                if let Some(decls) = tool.get_mut("functionDeclarations") {
                    if let Some(decls_arr) = decls.as_array_mut() {
                        for decl in decls_arr {
                            if let Some(params) = decl.get_mut("parameters") {
                                crate::proxy::common::json_schema::clean_json_schema(params);
                            }
                        }
                    }
                }
            }
        }
    }

    // Check if there are functionCall parts without thoughtSignature in history
    // If so, we must disable thinking mode to avoid signature requirement
    let mut has_unsigned_function_calls = false;
    if let Some(contents) = inner_request.get("contents") {
        if let Some(contents_arr) = contents.as_array() {
            for content in contents_arr {
                if let Some(parts) = content.get("parts") {
                    if let Some(parts_arr) = parts.as_array() {
                        for part in parts_arr {
                            if part.get("functionCall").is_some() && part.get("thoughtSignature").is_none() {
                                has_unsigned_function_calls = true;
                                break;
                            }
                        }
                    }
                }
                if has_unsigned_function_calls { break; }
            }
        }
    }

    // CRITICAL: Also check if tools are defined in the current request
    // When tools are present, the model may call functions without thoughtSignature
    let has_tools = inner_request.get("tools").is_some();

    // If there are unsigned function calls OR tools defined, disable thinking mode
    if has_unsigned_function_calls || has_tools {
        if let Some(obj) = inner_request.as_object_mut() {
            let gen_config = obj.entry("generationConfig").or_insert_with(|| json!({}));
            if let Some(gen_obj) = gen_config.as_object_mut() {
                gen_obj.insert("thinkingConfig".to_string(), json!({"includeThoughts": false}));
            }
        }
        tracing::info!("[Gemini] Detected unsigned functionCall in history or tools in request, disabling thinking mode");
    }

    tracing::info!("[Debug] Gemini Wrap: original='{}', mapped='{}', final='{}', type='{}'",
        original_model, final_model_name, config.final_model, config.request_type);
    
    // Inject googleSearch tool if needed
    if config.inject_google_search {
        crate::proxy::mappers::common_utils::inject_google_search_tool(&mut inner_request);
    }

    // Inject imageConfig if present (for image generation models)
    if let Some(image_config) = config.image_config {
         if let Some(obj) = inner_request.as_object_mut() {
             // 1. Remove tools (image generation does not support tools)
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

/// 解包响应（提取 response 字段）
pub fn unwrap_response(response: &Value) -> Value {
    response.get("response").unwrap_or(response).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_request() {
        let body = json!({
            "model": "gemini-2.5-flash",
            "contents": [{"role": "user", "parts": [{"text": "Hi"}]}]
        });

        let result = wrap_request(&body, "test-project", "gemini-2.5-flash");
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
}
