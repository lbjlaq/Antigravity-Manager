// Claude 请求转换 (Claude → Gemini v1internal)
// 对应 transformClaudeRequestIn

use super::models::*;
// use crate::proxy::common::model_mapping::map_claude_model_to_gemini;
use serde_json::{json, Value};
use std::collections::HashMap;
// use once_cell::sync::Lazy;
// use regex::Regex;

/// 转换 Claude 请求为 Gemini v1internal 格式
pub fn transform_claude_request_in(
    claude_req: &ClaudeRequest,
    project_id: &str,
) -> Result<Value, String> {
    // 检测是否有 web_search 工具
    let has_web_search_tool = claude_req
        .tools
        .as_ref()
        .map(|tools| tools.iter().any(|t| t.name == "web_search"))
        .unwrap_or(false);

    // 用于存储 tool_use id -> name 映射
    let mut tool_id_to_name: HashMap<String, String> = HashMap::new();

    // 1. System Instruction
    let system_instruction = build_system_instruction(&claude_req.system);

    // 4. Generation Config & Thinking
    let generation_config = build_generation_config(claude_req, has_web_search_tool);
    
    // Check if thinking is enabled
    let is_thinking_enabled = claude_req.thinking.as_ref()
        .map(|t| t.type_ == "enabled")
        .unwrap_or(false);

    // Map model name first (used to decide request options)
    let mapped_model = if has_web_search_tool {
        "gemini-2.5-flash".to_string()
    } else {
        crate::proxy::common::model_mapping::map_claude_model_to_gemini(&claude_req.model)
    };

    // Use shared grounding logic
    let config = crate::proxy::mappers::common_utils::resolve_request_config(&claude_req.model, &mapped_model);

    // Only Gemini models support our "dummy thought" workaround.
    // Claude models routed via Google/Vertex can require valid signatures for thinking blocks.
    let allow_dummy_thought = config.final_model.starts_with("gemini-");

    // 2. Contents (Messages)
    let contents = build_contents(
        &claude_req.messages,
        &mut tool_id_to_name,
        is_thinking_enabled,
        allow_dummy_thought,
    )?;

    // 3. Tools
    let tools = build_tools(&claude_req.tools, has_web_search_tool)?;

    // 5. Safety Settings
    let safety_settings = json!([
        { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_CIVIC_INTEGRITY", "threshold": "OFF" },
    ]);

    // Build inner request
    let mut inner_request = json!({
        "contents": contents,
        "safetySettings": safety_settings,
    });

    if let Some(sys_inst) = system_instruction {
        inner_request["systemInstruction"] = sys_inst;
    }

    if !generation_config.is_null() {
        inner_request["generationConfig"] = generation_config;
    }

    if let Some(tools_val) = tools {
        inner_request["tools"] = tools_val;
    }

    // Inject googleSearch tool if needed (and not already done by build_tools)
    if config.inject_google_search && !has_web_search_tool {
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

    // 生成 requestId
    let request_id = format!("agent-{}", uuid::Uuid::new_v4());

    // 构建最终请求体
    let mut body = json!({
        "project": project_id,
        "requestId": request_id,
        "request": inner_request,
        "model": config.final_model,
        "userAgent": "antigravity",
        "requestType": config.request_type,
    });

    // 如果提供了 metadata.user_id，则复用为 sessionId
    if let Some(metadata) = &claude_req.metadata {
        if let Some(user_id) = &metadata.user_id {
            body["request"]["sessionId"] = json!(user_id);
        }
    }

    Ok(body)
}

/// 构建 System Instruction
fn build_system_instruction(system: &Option<SystemPrompt>) -> Option<Value> {
    if let Some(sys) = system {
        let mut parts = Vec::new();

        match sys {
            SystemPrompt::String(text) => {
                parts.push(json!({"text": text}));
            }
            SystemPrompt::Array(blocks) => {
                for block in blocks {
                    if block.block_type == "text" {
                        parts.push(json!({"text": block.text}));
                    }
                }
            }
        }

        if !parts.is_empty() {
            return Some(json!({
                "role": "user",
                "parts": parts
            }));
        }
    }

    None
}

/// 构建 Contents (Messages)
fn build_contents(
    messages: &[Message],
    tool_id_to_name: &mut HashMap<String, String>,
    is_thinking_enabled: bool,
    allow_dummy_thought: bool,
) -> Result<Value, String> {
    let mut contents = Vec::new();

    let msg_count = messages.len();
    for (i, msg) in messages.iter().enumerate() {
        let role = if msg.role == "assistant" {
            "model"
        } else {
            &msg.role
        };

        let mut parts = Vec::new();

        match &msg.content {
            MessageContent::String(text) => {
                if text != "(no content)" {
                    if !text.trim().is_empty() {
                        parts.push(json!({"text": text.trim()}));
                    }
                }
            }
            MessageContent::Array(blocks) => {
                for item in blocks {
                    match item {
                        ContentBlock::Text { text } => {
                            if text != "(no content)" {
                                parts.push(json!({"text": text}));
                            }
                        }
                        ContentBlock::Thinking { thinking, signature } => {
                            let mut part = json!({
                                "text": thinking,
                                "thought": true
                            });
                            if let Some(sig) = signature {
                                part["thoughtSignature"] = json!(sig);
                            }
                            parts.push(part);
                        }
                        ContentBlock::Image { source } => {
                            if source.source_type == "base64" {
                                parts.push(json!({
                                    "inlineData": {
                                        "mimeType": source.media_type,
                                        "data": source.data
                                    }
                                }));
                            }
                        }
                        ContentBlock::ToolUse { id, name, input, signature } => {
                            let mut part = json!({
                                "functionCall": {
                                    "name": name,
                                    "args": input,
                                    "id": id
                                }
                            });

                            // 存储 id -> name 映射
                            tool_id_to_name.insert(id.clone(), name.clone());

                            if let Some(sig) = signature {
                                // Required by some upstreams when thinking/tool signatures are enabled.
                                part["thoughtSignature"] = json!(sig);
                            }
                            parts.push(part);
                        }
                        ContentBlock::ToolResult { tool_use_id, content } => {
                            // 优先使用之前记录的 name，否则用 tool_use_id
                            let func_name = tool_id_to_name
                                .get(tool_use_id)
                                .cloned()
                                .unwrap_or_else(|| tool_use_id.clone());

                            parts.push(json!({
                                "functionResponse": {
                                    "name": func_name,
                                    "response": {"result": content},
                                    "id": tool_use_id
                                }
                            }));
                        }
                    }
                }
            }
        }

        // Fix for "Thinking enabled, assistant message must start with thinking block" 400 error
        // ONLY apply this for the LAST assistant message (Pre-fill scenario)
        // Historical assistant messages MUST NOT have dummy thinking blocks without signatures
        if allow_dummy_thought && role == "model" && is_thinking_enabled && i == msg_count - 1 {
            let has_thought_part = parts.iter().any(|p| {
                p.get("thought").and_then(|v| v.as_bool()).unwrap_or(false)
            });
            
            if !has_thought_part {
                // Prepend a dummy thinking block to satisfy Gemini v1internal requirements
                parts.insert(0, json!({
                    "text": "Thinking...",
                    "thought": true
                }));
            }
        }

        contents.push(json!({
            "role": role,
            "parts": parts
        }));
    }

    Ok(json!(contents))
}

/// 构建 Tools
fn build_tools(
    tools: &Option<Vec<Tool>>,
    has_web_search: bool,
) -> Result<Option<Value>, String> {
    if let Some(tools_list) = tools {
        if has_web_search {
            // Web Search 工具映射
            return Ok(Some(json!([{
                "googleSearch": {
                    "enhancedContent": {
                        "imageSearch": {
                            "maxResultCount": 5
                        }
                    }
                }
            }])));
        }

        // 普通工具
        let mut function_declarations = Vec::new();
        for tool in tools_list {
            let mut input_schema = serde_json::to_value(&tool.input_schema).unwrap_or(json!({}));
            crate::proxy::common::json_schema::clean_json_schema(&mut input_schema);

            let tool_decl = json!({
                "name": tool.name,
                "description": tool.description,
                "parameters": input_schema
            });
            function_declarations.push(tool_decl);
        }

        if !function_declarations.is_empty() {
            return Ok(Some(json!([{
                "functionDeclarations": function_declarations
            }])));
        }
    }

    Ok(None)
}

/// 构建 Generation Config
fn build_generation_config(claude_req: &ClaudeRequest, has_web_search: bool) -> Value {
    let mut config = json!({});

    // Thinking 配置
    if let Some(thinking) = &claude_req.thinking {
        if thinking.type_ == "enabled" {
            let mut thinking_config = json!({"includeThoughts": true});

            if let Some(budget_tokens) = thinking.budget_tokens {
                let mut budget = budget_tokens;
                // gemini-2.5-flash 上限 24576
                let is_flash_model = has_web_search
                    || claude_req.model.contains("gemini-2.5-flash");
                if is_flash_model {
                    budget = budget.min(24576);
                }
                thinking_config["thinkingBudget"] = json!(budget);
            }

            config["thinkingConfig"] = thinking_config;
        }
    }

    // 其他参数
    if let Some(temp) = claude_req.temperature {
        config["temperature"] = json!(temp);
    }
    if let Some(top_p) = claude_req.top_p {
        config["topP"] = json!(top_p);
    }
    if let Some(top_k) = claude_req.top_k {
        config["topK"] = json!(top_k);
    }

    // web_search 强制 candidateCount=1
    /*if has_web_search {
        config["candidateCount"] = json!(1);
    }*/

    // max_tokens 映射为 maxOutputTokens
    config["maxOutputTokens"] = json!(64000);

    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_request() {
        let req = ClaudeRequest {
            model: "claude-sonnet-4-5".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Hello".to_string()),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
        };

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let body = result.unwrap();
        assert_eq!(body["project"], "test-project");
        assert!(body["requestId"].as_str().unwrap().starts_with("agent-"));
    }

    #[test]
    fn test_clean_json_schema() {
        let mut schema = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "location": {
                    "type": "string",
                    "description": "The city and state, e.g. San Francisco, CA",
                    "minLength": 1,
                    "exclusiveMinimum": 0
                },
                "unit": {
                    "type": ["string", "null"],
                    "enum": ["celsius", "fahrenheit"],
                    "default": "celsius"
                },
                "date": {
                    "type": "string",
                    "format": "date"
                }
            },
            "required": ["location"]
        });

        clean_json_schema(&mut schema);

        // Check removed fields
        assert!(schema.get("$schema").is_none());
        assert!(schema.get("additionalProperties").is_none());
        assert!(schema["properties"]["location"].get("minLength").is_none());
        assert!(schema["properties"]["unit"].get("default").is_none());
        assert!(schema["properties"]["date"].get("format").is_none());

        // Check union type handling ["string", "null"] -> "STRING"
        assert_eq!(schema["properties"]["unit"]["type"], "STRING");

        // Check types are uppercased
        assert_eq!(schema["type"], "OBJECT");
        assert_eq!(schema["properties"]["location"]["type"], "STRING");
        assert_eq!(schema["properties"]["date"]["type"], "STRING");
    }
}
