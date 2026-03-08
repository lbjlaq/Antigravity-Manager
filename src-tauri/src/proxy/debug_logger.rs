use futures::StreamExt;
use serde_json::Value;
use std::path::PathBuf;
use tokio::fs;

use crate::proxy::config::DebugLoggingConfig;

fn build_filename(prefix: &str, trace_id: Option<&str>) -> String {
    let ts = chrono::Utc::now().format("%Y%m%d_%H%M%S%.3f");
    let tid = trace_id.unwrap_or("unknown");
    format!("{}_{}_{}.json", ts, tid, prefix)
}

fn resolve_output_dir(cfg: &DebugLoggingConfig) -> Option<PathBuf> {
    if let Some(dir) = cfg.output_dir.as_ref() {
        return Some(PathBuf::from(dir));
    }
    if let Ok(data_dir) = crate::modules::account::get_data_dir() {
        return Some(data_dir.join("debug_logs"));
    }
    None
}

pub async fn write_debug_payload(
    cfg: &DebugLoggingConfig,
    trace_id: Option<&str>,
    prefix: &str,
    payload: &Value,
) {
    if !cfg.enabled {
        return;
    }

    let output_dir = match resolve_output_dir(cfg) {
        Some(dir) => dir,
        None => {
            tracing::warn!("[Debug-Log] Enabled but output_dir is not available.");
            return;
        }
    };

    if let Err(e) = fs::create_dir_all(&output_dir).await {
        tracing::warn!("[Debug-Log] Failed to create output dir: {}", e);
        return;
    }

    let filename = build_filename(prefix, trace_id);
    let path = output_dir.join(filename);

    match serde_json::to_vec_pretty(payload) {
        Ok(bytes) => {
            if let Err(e) = fs::write(&path, bytes).await {
                tracing::warn!("[Debug-Log] Failed to write file: {}", e);
            }
        }
        Err(e) => {
            tracing::warn!("[Debug-Log] Failed to serialize payload: {}", e);
        }
    }
}

pub fn is_enabled(cfg: &DebugLoggingConfig) -> bool {
    cfg.enabled
}

/// 解析 SSE 流式数据，提取 thinking 和正文内容
fn parse_sse_stream(raw: &str) -> (String, String) {
    let mut thinking_parts: Vec<String> = Vec::new();
    let mut content_parts: Vec<String> = Vec::new();

    for line in raw.lines() {
        let line = line.trim();
        if !line.starts_with("data: ") {
            continue;
        }
        let json_str = &line[6..]; // 去掉 "data: " 前缀
        if json_str.is_empty() || json_str == "[DONE]" {
            continue;
        }

        // 尝试解析 JSON
        if let Ok(parsed) = serde_json::from_str::<Value>(json_str) {
            // Gemini/v1internal 格式: response.candidates[0].content.parts[0]
            if let Some(candidates) = parsed
                .get("response")
                .and_then(|r| r.get("candidates"))
                .and_then(|c| c.as_array())
            {
                for candidate in candidates {
                    if let Some(parts) = candidate
                        .get("content")
                        .and_then(|c| c.get("parts"))
                        .and_then(|p| p.as_array())
                    {
                        for part in parts {
                            let text = part.get("text").and_then(|t| t.as_str()).unwrap_or("");
                            let is_thought = part
                                .get("thought")
                                .and_then(|t| t.as_bool())
                                .unwrap_or(false);

                            if !text.is_empty() {
                                if is_thought {
                                    thinking_parts.push(text.to_string());
                                } else {
                                    content_parts.push(text.to_string());
                                }
                            }
                        }
                    }
                }
            }
            // OpenAI 格式兼容: choices[0].delta.content
            else if let Some(choices) = parsed.get("choices").and_then(|c| c.as_array()) {
                for choice in choices {
                    if let Some(delta) = choice.get("delta") {
                        if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                            if !content.is_empty() {
                                content_parts.push(content.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    (thinking_parts.join(""), content_parts.join(""))
}

pub fn wrap_stream_with_debug<S, E>(
    stream: std::pin::Pin<Box<S>>,
    cfg: DebugLoggingConfig,
    trace_id: String,
    prefix: &'static str,
    meta: Value,
) -> std::pin::Pin<Box<dyn futures::Stream<Item = Result<bytes::Bytes, E>> + Send>>
where
    S: futures::Stream<Item = Result<bytes::Bytes, E>> + Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    if !is_enabled(&cfg) {
        return stream;
    }

    let wrapped = async_stream::stream! {
        let mut collected: Vec<u8> = Vec::new();
        let mut inner = stream;
        while let Some(item) = inner.next().await {
            if let Ok(bytes) = &item {
                collected.extend_from_slice(bytes);
            }
            yield item;
        }

        let raw_text = String::from_utf8_lossy(&collected).to_string();
        let (thinking_content, response_content) = parse_sse_stream(&raw_text);

        let mut payload = serde_json::json!({
            "kind": "upstream_response",
            "trace_id": trace_id,
            "meta": meta,
        });

        // 只有在有内容时才添加对应字段
        if !thinking_content.is_empty() {
            payload["thinking_content"] = serde_json::Value::String(thinking_content);
        }
        if !response_content.is_empty() {
            payload["response_content"] = serde_json::Value::String(response_content);
        }

        write_debug_payload(&cfg, Some(&payload["trace_id"].as_str().unwrap_or("unknown")), prefix, &payload).await;
    };

    Box::pin(wrapped)
}
