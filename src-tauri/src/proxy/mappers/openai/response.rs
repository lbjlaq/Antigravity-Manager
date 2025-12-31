use super::models::*;
use serde_json::Value;

pub fn transform_openai_response(gemini_response: &Value, session_id: Option<&str>) -> OpenAIResponse {
    // 解包 response 字段
    let raw = gemini_response.get("response").unwrap_or(gemini_response);

    // 提取 content 和 tool_calls
    let mut content_out = String::new();
    let mut tool_calls = Vec::new();

    if let Some(parts) = raw
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|cand| cand.get("content"))
        .and_then(|content| content.get("parts"))
        .and_then(|p| p.as_array())
    {
        for part in parts {
            /* 暂时禁用：思维链/推理部分 (Gemini 2.0+) 避免干扰 Codex CLI 等非推理客户端
            if let Some(thought) = part.get("thought").and_then(|t| t.as_str()) {
                if !thought.is_empty() {
                    content_out.push_str("<thought>\n");
                    content_out.push_str(thought);
                    content_out.push_str("\n</thought>\n\n");
                }
            }
            */

            // 捕获 thoughtSignature (Gemini 3 工具调用必需)
            if let Some(sig) = part
                .get("thoughtSignature")
                .or(part.get("thought_signature"))
                .and_then(|s| s.as_str())
            {
                crate::proxy::mappers::signature_store::store_thought_signature_for_session(
                    session_id.unwrap_or("global"),
                    sig,
                );
            }

            // 文本部分
            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                content_out.push_str(text);
            }

            // 工具调用部分
            if let Some(fc) = part.get("functionCall") {
                let name = fc.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                let args = fc
                    .get("args")
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "{}".to_string());
                let id = fc
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("{}-{}", name, uuid::Uuid::new_v4()));

                tool_calls.push(ToolCall {
                    id,
                    r#type: "function".to_string(),
                    function: ToolFunction {
                        name: name.to_string(),
                        arguments: args,
                    },
                });
            }

            // 图片处理
            if let Some(img) = part.get("inlineData") {
                let mime_type = img
                    .get("mimeType")
                    .and_then(|v| v.as_str())
                    .unwrap_or("image/png");
                let data = img.get("data").and_then(|v| v.as_str()).unwrap_or("");
                if !data.is_empty() {
                    content_out.push_str(&format!("![image](data:{};base64,{})", mime_type, data));
                }
            }
        }
    }

    // 提取并处理联网搜索引文 (Grounding Metadata)
    if let Some(grounding) = raw
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|cand| cand.get("groundingMetadata"))
    {
        let mut grounding_text = String::new();

        // 1. 处理搜索词
        if let Some(queries) = grounding.get("webSearchQueries").and_then(|q| q.as_array()) {
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

    // 提取 finish_reason
    let finish_reason = raw
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|cand| cand.get("finishReason"))
        .and_then(|f| f.as_str())
        .map(|f| match f {
            "STOP" => "stop",
            "MAX_TOKENS" => "length",
            "SAFETY" => "content_filter",
            "RECITATION" => "content_filter",
            _ => "stop",
        })
        .unwrap_or("stop");

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
        choices: vec![Choice {
            index: 0,
            message: OpenAIMessage {
                role: "assistant".to_string(),
                content: if content_out.is_empty() {
                    None
                } else {
                    Some(OpenAIContent::String(content_out))
                },
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls)
                },
                tool_call_id: None,
                name: None,
            },
            finish_reason: Some(finish_reason.to_string()),
        }],
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
            "modelVersion": "gemini-2.5-pro",
            "responseId": "resp_123"
        });

        let result = transform_openai_response(&gemini_resp, None);
        assert_eq!(result.object, "chat.completion");

        let content = match result.choices[0].message.content.as_ref().unwrap() {
            OpenAIContent::String(s) => s,
            _ => panic!("Expected string content"),
        };
        assert_eq!(content, "Hello!");
        assert_eq!(result.choices[0].finish_reason, Some("stop".to_string()));
    }
}
