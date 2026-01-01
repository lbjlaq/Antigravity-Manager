use base64::Engine;
use serde_json::{json, Value};
use tokio::time::Duration;

use crate::proxy::config::UpstreamProxyConfig;
use crate::proxy::ZaiConfig;

const ZAI_CHAT_COMPLETIONS_GENERAL_URL: &str = "https://api.z.ai/api/paas/v4/chat/completions";
const ZAI_CHAT_COMPLETIONS_CODING_URL: &str = "https://api.z.ai/api/coding/paas/v4/chat/completions";

fn build_client(upstream_proxy: UpstreamProxyConfig, timeout_secs: u64) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.max(5)));

    if upstream_proxy.enabled && !upstream_proxy.url.is_empty() {
        let proxy = reqwest::Proxy::all(&upstream_proxy.url)
            .map_err(|e| format!("Invalid upstream proxy url: {}", e))?;
        builder = builder.proxy(proxy);
    }

    builder.build().map_err(|e| format!("Failed to build HTTP client: {}", e))
}

fn is_http_url(value: &str) -> bool {
    let v = value.trim();
    v.starts_with("http://") || v.starts_with("https://")
}

fn mime_for_image_extension(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        _ => None,
    }
}

fn mime_for_video_extension(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "mp4" => Some("video/mp4"),
        "mov" => Some("video/quicktime"),
        "m4v" => Some("video/x-m4v"),
        _ => None,
    }
}

fn file_ext(path: &std::path::Path) -> Option<String> {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

fn encode_file_as_data_url(path: &std::path::Path, mime: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{};base64,{}", mime, encoded))
}

fn image_source_to_content(image_source: &str, max_size_mb: u64) -> Result<Value, String> {
    if is_http_url(image_source) {
        return Ok(json!({
            "type": "image_url",
            "image_url": { "url": image_source }
        }));
    }

    let path = std::path::Path::new(image_source);
    let meta = std::fs::metadata(path).map_err(|_| "Image file not found".to_string())?;
    let max_size = max_size_mb * 1024 * 1024;
    if meta.len() > max_size {
        return Err(format!(
            "Image file too large ({} bytes), max {} MB",
            meta.len(),
            max_size_mb
        ));
    }

    let ext = file_ext(path).ok_or("Unsupported image format".to_string())?;
    let mime = mime_for_image_extension(&ext).ok_or("Unsupported image format".to_string())?;
    let data_url = encode_file_as_data_url(path, mime)?;
    Ok(json!({
        "type": "image_url",
        "image_url": { "url": data_url }
    }))
}

fn video_source_to_content(video_source: &str, max_size_mb: u64) -> Result<Value, String> {
    if is_http_url(video_source) {
        return Ok(json!({
            "type": "video_url",
            "video_url": { "url": video_source }
        }));
    }

    let path = std::path::Path::new(video_source);
    let meta = std::fs::metadata(path).map_err(|_| "Video file not found".to_string())?;
    let max_size = max_size_mb * 1024 * 1024;
    if meta.len() > max_size {
        return Err(format!(
            "Video file too large ({} bytes), max {} MB",
            meta.len(),
            max_size_mb
        ));
    }

    let ext = file_ext(path).ok_or("Unsupported video format".to_string())?;
    let mime = mime_for_video_extension(&ext).ok_or("Unsupported video format".to_string())?;
    let data_url = encode_file_as_data_url(path, mime)?;
    Ok(json!({
        "type": "video_url",
        "video_url": { "url": data_url }
    }))
}

fn user_message_with_content(mut content: Vec<Value>, prompt: &str) -> Value {
    content.push(json!({ "type": "text", "text": prompt }));
    json!({ "role": "user", "content": content })
}

async fn vision_chat_completion(
    client: &reqwest::Client,
    api_key: &str,
    system_prompt: &str,
    user_content: Vec<Value>,
    prompt: &str,
) -> Result<String, String> {
    let body = json!({
        "model": "glm-4.6v",
        "messages": [
            { "role": "system", "content": system_prompt },
            user_message_with_content(user_content, prompt),
        ],
        "thinking": { "type": "enabled" },
        "stream": false,
        "temperature": 0.8,
        "top_p": 0.6,
        "max_tokens": 32768
    });

    // For GLM Coding Plan users, z.ai requires the dedicated `/coding/paas/v4` endpoint.
    // We try it first, and fall back to the general endpoint only for cases that indicate
    // the coding endpoint is unavailable for this key/environment (401/403/404).
    let candidates = [
        ("coding", ZAI_CHAT_COMPLETIONS_CODING_URL),
        ("general", ZAI_CHAT_COMPLETIONS_GENERAL_URL),
    ];

    let mut last_err: Option<String> = None;
    for (label, url) in candidates {
        let resp = client
            .post(url)
            .bearer_auth(api_key)
            .header("X-Title", "Vision MCP Local")
            .header("Accept-Language", "en-US,en")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Upstream request failed ({}): {}", label, e))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            let err = format!("HTTP {} ({}): {}", status, label, text);
            last_err = Some(err);

            if label == "coding" && matches!(status, 401 | 403 | 404) {
                continue;
            }
            return Err(last_err.unwrap_or_else(|| "Vision request failed".to_string()));
        }

        let v: Value = resp.json().await.map_err(|e| format!("Invalid JSON response ({}): {}", label, e))?;
        let content = v
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| "Invalid API response: missing choices[0].message.content".to_string())?;

        return Ok(content.to_string());
    }

    Err(last_err.unwrap_or_else(|| "Vision request failed".to_string()))
}

pub fn tool_specs() -> Vec<Value> {
    vec![
        json!({
            "name": "ui_to_artifact",
            "description": "Convert UI screenshots into artifacts (code/prompt/spec/description).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "image_source": { "type": "string", "description": "Local file path or remote URL to the image" },
                    "output_type": { "type": "string", "enum": ["code","prompt","spec","description"] },
                    "prompt": { "type": "string" }
                },
                "required": ["image_source","output_type","prompt"]
            }
        }),
        json!({
            "name": "extract_text_from_screenshot",
            "description": "Extract text/code from screenshots (OCR-like).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "image_source": { "type": "string" },
                    "prompt": { "type": "string" },
                    "language_hint": { "type": "string" }
                },
                "required": ["image_source","prompt"]
            }
        }),
        json!({
            "name": "diagnose_error_screenshot",
            "description": "Diagnose error screenshots (stack traces, logs, runtime errors).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "image_source": { "type": "string" },
                    "prompt": { "type": "string" },
                    "context": { "type": "string" }
                },
                "required": ["image_source","prompt"]
            }
        }),
        json!({
            "name": "understand_technical_diagram",
            "description": "Analyze architecture/flow/UML/ER diagrams.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "image_source": { "type": "string" },
                    "prompt": { "type": "string" },
                    "diagram_type": { "type": "string" }
                },
                "required": ["image_source","prompt"]
            }
        }),
        json!({
            "name": "analyze_data_visualization",
            "description": "Analyze charts/dashboards to extract insights and trends.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "image_source": { "type": "string" },
                    "prompt": { "type": "string" },
                    "analysis_focus": { "type": "string" }
                },
                "required": ["image_source","prompt"]
            }
        }),
        json!({
            "name": "ui_diff_check",
            "description": "Compare two UI screenshots and report visual differences.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "expected_image_source": { "type": "string" },
                    "actual_image_source": { "type": "string" },
                    "prompt": { "type": "string" }
                },
                "required": ["expected_image_source","actual_image_source","prompt"]
            }
        }),
        json!({
            "name": "image_analysis",
            "description": "General-purpose image analysis.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "image_source": { "type": "string" },
                    "prompt": { "type": "string" }
                },
                "required": ["image_source","prompt"]
            }
        }),
        json!({
            "name": "video_analysis",
            "description": "Analyze video content.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "video_source": { "type": "string" },
                    "prompt": { "type": "string" }
                },
                "required": ["video_source","prompt"]
            }
        }),
    ]
}

pub async fn call_tool(
    zai: &ZaiConfig,
    upstream_proxy: UpstreamProxyConfig,
    timeout_secs: u64,
    tool_name: &str,
    arguments: &Value,
) -> Result<Value, String> {
    let api_key = zai.api_key.trim();
    if api_key.is_empty() {
        return Err("z.ai api_key is missing".to_string());
    }

    let client = build_client(upstream_proxy, timeout_secs)?;

    let tool_result = match tool_name {
        "ui_to_artifact" => {
            let image_source = arguments
                .get("image_source")
                .and_then(|v| v.as_str())
                .ok_or("Missing image_source")?;
            let output_type = arguments
                .get("output_type")
                .and_then(|v| v.as_str())
                .ok_or("Missing output_type")?;
            let prompt = arguments.get("prompt").and_then(|v| v.as_str()).ok_or("Missing prompt")?;

            let system_prompt = match output_type {
                "code" => "You are a frontend engineer. Generate clean, accessible, responsive frontend code from the UI screenshot.",
                "prompt" => "You generate precise prompts to recreate UI screenshots.",
                "spec" => "You are a design systems architect. Produce a detailed UI specification from the screenshot.",
                "description" => "You describe UI screenshots clearly and completely in natural language.",
                _ => return Err("Invalid output_type".to_string()),
            };

            let image = image_source_to_content(image_source, 5)?;
            vision_chat_completion(&client, api_key, system_prompt, vec![image], prompt).await?
        }
        "extract_text_from_screenshot" => {
            let image_source = arguments
                .get("image_source")
                .and_then(|v| v.as_str())
                .ok_or("Missing image_source")?;
            let mut prompt = arguments.get("prompt").and_then(|v| v.as_str()).ok_or("Missing prompt")?.to_string();
            if let Some(lang) = arguments.get("language_hint").and_then(|v| v.as_str()) {
                if !lang.trim().is_empty() {
                    prompt.push_str(&format!("\n\nLanguage hint: {}", lang.trim()));
                }
            }
            let image = image_source_to_content(image_source, 5)?;
            let system_prompt = "Extract text from the screenshot accurately. Preserve code formatting. If unsure, say what is uncertain.";
            vision_chat_completion(&client, api_key, system_prompt, vec![image], &prompt).await?
        }
        "diagnose_error_screenshot" => {
            let image_source = arguments
                .get("image_source")
                .and_then(|v| v.as_str())
                .ok_or("Missing image_source")?;
            let mut prompt = arguments.get("prompt").and_then(|v| v.as_str()).ok_or("Missing prompt")?.to_string();
            if let Some(ctx) = arguments.get("context").and_then(|v| v.as_str()) {
                if !ctx.trim().is_empty() {
                    prompt.push_str(&format!("\n\nContext: {}", ctx.trim()));
                }
            }
            let image = image_source_to_content(image_source, 5)?;
            let system_prompt = "Diagnose the error shown in the screenshot. Identify root cause, propose fixes and verification steps.";
            vision_chat_completion(&client, api_key, system_prompt, vec![image], &prompt).await?
        }
        "understand_technical_diagram" => {
            let image_source = arguments
                .get("image_source")
                .and_then(|v| v.as_str())
                .ok_or("Missing image_source")?;
            let mut prompt = arguments.get("prompt").and_then(|v| v.as_str()).ok_or("Missing prompt")?.to_string();
            if let Some(diagram_type) = arguments.get("diagram_type").and_then(|v| v.as_str()) {
                if !diagram_type.trim().is_empty() {
                    prompt.push_str(&format!("\n\nDiagram type: {}", diagram_type.trim()));
                }
            }
            let image = image_source_to_content(image_source, 5)?;
            let system_prompt = "Explain the technical diagram. Describe components, relationships, data flows, and key assumptions.";
            vision_chat_completion(&client, api_key, system_prompt, vec![image], &prompt).await?
        }
        "analyze_data_visualization" => {
            let image_source = arguments
                .get("image_source")
                .and_then(|v| v.as_str())
                .ok_or("Missing image_source")?;
            let mut prompt = arguments.get("prompt").and_then(|v| v.as_str()).ok_or("Missing prompt")?.to_string();
            if let Some(focus) = arguments.get("analysis_focus").and_then(|v| v.as_str()) {
                if !focus.trim().is_empty() {
                    prompt.push_str(&format!("\n\nFocus: {}", focus.trim()));
                }
            }
            let image = image_source_to_content(image_source, 5)?;
            let system_prompt = "Analyze the chart/dashboard and extract insights, trends, anomalies, and recommendations.";
            vision_chat_completion(&client, api_key, system_prompt, vec![image], &prompt).await?
        }
        "ui_diff_check" => {
            let expected = arguments
                .get("expected_image_source")
                .and_then(|v| v.as_str())
                .ok_or("Missing expected_image_source")?;
            let actual = arguments
                .get("actual_image_source")
                .and_then(|v| v.as_str())
                .ok_or("Missing actual_image_source")?;
            let prompt = arguments.get("prompt").and_then(|v| v.as_str()).ok_or("Missing prompt")?;

            let expected_img = image_source_to_content(expected, 5)?;
            let actual_img = image_source_to_content(actual, 5)?;
            let system_prompt = "Compare the two UI screenshots and report differences grouped by severity. Include actionable fix suggestions.";
            vision_chat_completion(
                &client,
                api_key,
                system_prompt,
                vec![expected_img, actual_img],
                prompt,
            )
            .await?
        }
        "analyze_image" | "image_analysis" => {
            let image_source = arguments
                .get("image_source")
                .and_then(|v| v.as_str())
                .ok_or("Missing image_source")?;
            let prompt = arguments.get("prompt").and_then(|v| v.as_str()).ok_or("Missing prompt")?;
            let image = image_source_to_content(image_source, 5)?;
            let system_prompt = "Analyze the image. Be precise and include relevant details.";
            vision_chat_completion(&client, api_key, system_prompt, vec![image], prompt).await?
        }
        "analyze_video" | "video_analysis" => {
            let video_source = arguments
                .get("video_source")
                .and_then(|v| v.as_str())
                .ok_or("Missing video_source")?;
            let prompt = arguments.get("prompt").and_then(|v| v.as_str()).ok_or("Missing prompt")?;
            let video = video_source_to_content(video_source, 8)?;
            let system_prompt = "Analyze the video content according to the user's request.";
            vision_chat_completion(&client, api_key, system_prompt, vec![video], prompt).await?
        }
        _ => return Err("Unknown tool".to_string()),
    };

    Ok(json!({
        "content": [
            { "type": "text", "text": tool_result }
        ]
    }))
}
