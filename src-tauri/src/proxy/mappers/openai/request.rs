// OpenAI → Gemini 请求转换
use super::models::*;
use serde_json::{json, Value};

pub fn transform_openai_request(request: &OpenAIRequest, project_id: &str, mapped_model: &str) -> Value {
    // Resolve grounding config
    let config = crate::proxy::mappers::common_utils::resolve_request_config(&request.model, mapped_model);

    tracing::info!("[Debug] OpenAI Request: original='{}', mapped='{}', type='{}', has_image_config={}", 
        request.model, mapped_model, config.request_type, config.image_config.is_some());
    
    // 1. 提取所有 System Message
    let system_instructions: Vec<String> = request.messages.iter()
        .filter(|msg| msg.role == "system")
        .filter_map(|msg| msg.content.clone())
        .collect();

    // 2. 构建 Gemini contents (过滤掉 system)
    let contents: Vec<Value> = request
        .messages
        .iter()
        .filter(|msg| msg.role != "system")
        .map(|msg| {
            let role = match msg.role.as_str() {
                "assistant" => "model",
                "tool" => "user", // OpenAI 'tool' role maps to user side in Gemini function response
                _ => &msg.role,
            };

            let mut parts = Vec::new();
            
            // Handle text content
            if let Some(content) = &msg.content {
                parts.push(json!({"text": content}));
            }

            // Handle tool calls (assistant message)
            if let Some(tool_calls) = &msg.tool_calls {
                for tc in tool_calls {
                    parts.push(json!({
                        "functionCall": {
                            "name": tc.function.name,
                            "args": serde_json::from_str::<Value>(&tc.function.arguments).unwrap_or(json!({})),
                            "id": tc.id
                        }
                    }));
                }
            }

            // Handle tool response
            if msg.role == "tool" {
                if let (Some(id), Some(content)) = (&msg.tool_call_id, &msg.content) {
                    parts.push(json!({
                        "functionResponse": {
                           "name": "unknown", // Need to find name if possible, or just use id
                           "id": id,
                           "response": { "result": content }
                        }
                    }));
                }
            }

            json!({
                "role": role,
                "parts": parts
            })
        })
        .collect();

    // 3. 构建请求体
    let mut gen_config = json!({
        "maxOutputTokens": request.max_tokens.unwrap_or(64000), // Adjusted to 64k to match Claude
        "temperature": request.temperature.unwrap_or(1.0),
        "topP": request.top_p.unwrap_or(1.0), 
    });

    // Handle stop sequences
    if let Some(stop) = &request.stop {
        if stop.is_string() {
            gen_config["stopSequences"] = json!([stop]);
        } else if stop.is_array() {
            gen_config["stopSequences"] = stop.clone();
        }
    }

    // Handle response_format (JSON mode)
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

    // 4. Handle Tools - Convert OpenAI format to Gemini functionDeclarations format
    if let Some(tools) = &request.tools {
        let mut function_declarations: Vec<Value> = Vec::new();
        
        for tool in tools.iter() {
            // OpenAI format: { "type": "function", "function": { "name": "...", "description": "...", "parameters": {...} } }
            // Gemini format: { "name": "...", "description": "...", "parameters": {...} }
            if let Some(func) = tool.get("function") {
                let mut gemini_func = func.clone();
                
                // Clean the JSON schema in parameters
                if let Some(params) = gemini_func.get_mut("parameters") {
                    crate::proxy::common::json_schema::clean_json_schema(params);
                }
                
                function_declarations.push(gemini_func);
            }
        }
        
        if !function_declarations.is_empty() {
            // Gemini expects: { "tools": [{ "functionDeclarations": [...] }] }
            inner_request["tools"] = json!([{
                "functionDeclarations": function_declarations
            }]);
        }
    }
    
    // 5. 注入 systemInstruction (如果有)
    if !system_instructions.is_empty() {
        let combined_instruction = system_instructions.join("\n\n");
        inner_request["systemInstruction"] = json!({
            "parts": [{"text": combined_instruction}]
        });
    }
    
    // Inject googleSearch tool if needed (overrides or adds to existing tools)
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
                 gen_obj.remove("responseModalities");
                 gen_obj.insert("imageConfig".to_string(), image_config);
             }
         }
    }

    json!({
        "project": project_id,
        "requestId": format!("openai-{}", uuid::Uuid::new_v4()),
        "request": inner_request,
        "model": config.final_model,
        "userAgent": "antigravity", // Changed from "antigravity-openai" to match Claude
        "requestType": config.request_type
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_openai_request() {
        let req = OpenAIRequest {
            model: "gpt-4".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some("Hello".to_string()),
                tool_calls: None,
                tool_call_id: None,
            }],
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            stop: None,
            response_format: None,
            tools: None,
            tool_choice: None,
        };

        let result = transform_openai_request(&req, "test-project", "gemini-1.5-pro-latest");
        assert_eq!(result["project"], "test-project");
        assert!(result["requestId"].as_str().unwrap().starts_with("openai-"));
        
        // Ensure contents are present
        let contents = result["request"]["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "user");
    }

    #[test]
    fn test_transform_openai_request_system_instruction() {
        let req = OpenAIRequest {
            model: "gpt-4".to_string(),
            messages: vec![
                OpenAIMessage {
                    role: "system".to_string(),
                    content: Some("System Prompt 1".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                OpenAIMessage {
                    role: "system".to_string(),
                    content: Some("System Prompt 2".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                OpenAIMessage {
                    role: "user".to_string(),
                    content: Some("User Message".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                }
            ],
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            stop: None,
            response_format: None,
            tools: None,
            tool_choice: None,
        };

        let result = transform_openai_request(&req, "test-project", "gemini-1.5-pro-latest");
        let inner_request = &result["request"];

        // 1. Verify systemInstruction is present
        let system_instruction = &inner_request["systemInstruction"];
        assert!(system_instruction.is_object());
        
        let parts = system_instruction["parts"].as_array().unwrap();
        let text = parts[0]["text"].as_str().unwrap();
        assert!(text.contains("System Prompt 1"));
        assert!(text.contains("System Prompt 2"));

        // 2. Verify contents do NOT contain system messages
        let contents = inner_request["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1); // Only user message remains
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "User Message");
    }
}
