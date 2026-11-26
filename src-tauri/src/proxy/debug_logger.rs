use futures::StreamExt;
use serde_json::Value;
use std::path::PathBuf;
use tokio::fs;

use crate::proxy::config::DebugLoggingConfig;

const DEBUG_STREAM_HEAD_BYTES: usize = 256 * 1024;
const DEBUG_STREAM_TAIL_BYTES: usize = 256 * 1024;

struct BoundedStreamCapture {
    head: Vec<u8>,
    tail: Vec<u8>,
    total_bytes: usize,
}

impl BoundedStreamCapture {
    fn new() -> Self {
        Self {
            head: Vec::with_capacity(DEBUG_STREAM_HEAD_BYTES),
            tail: Vec::with_capacity(DEBUG_STREAM_TAIL_BYTES),
            total_bytes: 0,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        self.total_bytes = self.total_bytes.saturating_add(bytes.len());
        let head_len = (DEBUG_STREAM_HEAD_BYTES - self.head.len()).min(bytes.len());
        self.head.extend_from_slice(&bytes[..head_len]);
        let remaining = &bytes[head_len..];
        if remaining.len() >= DEBUG_STREAM_TAIL_BYTES {
            self.tail.clear();
            self.tail
                .extend_from_slice(&remaining[remaining.len() - DEBUG_STREAM_TAIL_BYTES..]);
        } else if !remaining.is_empty() {
            let overflow = self
                .tail
                .len()
                .saturating_add(remaining.len())
                .saturating_sub(DEBUG_STREAM_TAIL_BYTES);
            if overflow > 0 {
                self.tail.drain(..overflow);
            }
            self.tail.extend_from_slice(remaining);
        }
    }

    fn truncated(&self) -> bool {
        self.total_bytes > self.head.len() + self.tail.len()
    }

    fn bounded_bytes(&self) -> Vec<u8> {
        let marker = b"\n...[debug stream truncated]...\n";
        let mut bytes = Vec::with_capacity(
            self.head.len() + self.tail.len() + usize::from(self.truncated()) * marker.len(),
        );
        bytes.extend_from_slice(&self.head);
        if self.truncated() {
            bytes.extend_from_slice(marker);
        }
        bytes.extend_from_slice(&self.tail);
        bytes
    }
}

fn sanitize_trace_id_for_filename(raw: &str) -> String {
    let sanitized: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(96)
        .collect();
    let trimmed = sanitized.trim_matches('_');
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}

fn build_filename(prefix: &str, trace_id: Option<&str>) -> String {
    let ts = chrono::Utc::now().format("%Y%m%d_%H%M%S%.3f");
    let tid = trace_id
        .map(sanitize_trace_id_for_filename)
        .unwrap_or_else(|| "unknown".to_string());
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

fn resolve_exchange_output_dir(cfg: &DebugLoggingConfig) -> Option<PathBuf> {
    resolve_output_dir(cfg).map(|dir| dir.join("debug_exchanges"))
}

pub async fn write_debug_payload(
    cfg: &DebugLoggingConfig,
    trace_id: Option<&str>,
    prefix: &str,
    payload: &Value,
) {
    if !is_enabled(cfg) {
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

pub async fn write_exchange_payload(
    cfg: &DebugLoggingConfig,
    trace_id: Option<&str>,
    prefix: &str,
    payload: &Value,
) {
    if !is_enabled(cfg) {
        return;
    }

    let output_dir = match resolve_exchange_output_dir(cfg) {
        Some(dir) => dir,
        None => {
            tracing::warn!("[Debug-Exchange] Enabled but output_dir is not available.");
            return;
        }
    };

    if let Err(e) = fs::create_dir_all(&output_dir).await {
        tracing::warn!("[Debug-Exchange] Failed to create output dir: {}", e);
        return;
    }

    let filename = build_filename(prefix, trace_id);
    let path = output_dir.join(filename);

    match serde_json::to_vec_pretty(payload) {
        Ok(bytes) => {
            if let Err(e) = fs::write(&path, bytes).await {
                tracing::warn!("[Debug-Exchange] Failed to write file: {}", e);
            }
        }
        Err(e) => {
            tracing::warn!("[Debug-Exchange] Failed to serialize payload: {}", e);
        }
    }
}

pub fn is_enabled(cfg: &DebugLoggingConfig) -> bool {
    cfg.enabled || crate::modules::log_bridge::is_log_bridge_enabled()
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
        let mut capture = BoundedStreamCapture::new();
        let mut inner = stream;
        while let Some(item) = inner.next().await {
            if let Ok(bytes) = &item {
                capture.push(bytes);
            }
            yield item;
        }

        let raw_text = String::from_utf8_lossy(&capture.bounded_bytes()).to_string();
        let (thinking_content, response_content) = parse_sse_stream(&raw_text);

        let mut payload = serde_json::json!({
            "kind": prefix,
            "trace_id": trace_id,
            "meta": meta,
            "raw_stream": raw_text,
            "raw_stream_bytes": capture.total_bytes,
            "truncated": capture.truncated(),
        });

        // 只有在有内容时才添加对应字段
        if !thinking_content.is_empty() {
            payload["thinking_content"] = serde_json::Value::String(thinking_content);
        }
        if !response_content.is_empty() {
            payload["response_content"] = serde_json::Value::String(response_content);
        }

        write_exchange_payload(&cfg, Some(&payload["trace_id"].as_str().unwrap_or("unknown")), prefix, &payload).await;
    };

    Box::pin(wrapped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{stream, StreamExt};

    #[tokio::test]
    async fn debug_stream_capture_is_bounded_and_forwarding_is_unchanged() {
        let data = bytes::Bytes::from(vec![b'A'; 1024 * 1024]);
        let output_dir = std::env::temp_dir().join(format!(
            "antigravity-debug-stream-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let cfg = DebugLoggingConfig {
            enabled: true,
            output_dir: Some(output_dir.to_string_lossy().into_owned()),
        };
        let wrapped = wrap_stream_with_debug(
            Box::pin(stream::iter(vec![Ok::<_, String>(data.clone())])),
            cfg,
            "bounded-stream".to_string(),
            "test_stream",
            serde_json::json!({}),
        );
        let forwarded = wrapped
            .map(|item| item.expect("forwarded chunk"))
            .collect::<Vec<_>>()
            .await;
        assert_eq!(forwarded, vec![data]);

        let exchange_dir = output_dir.join("debug_exchanges");
        let mut entries = tokio::fs::read_dir(&exchange_dir)
            .await
            .expect("debug log dir");
        let path = entries
            .next_entry()
            .await
            .expect("read debug entry")
            .expect("debug entry")
            .path();
        let payload: Value =
            serde_json::from_slice(&tokio::fs::read(path).await.expect("read debug payload"))
                .expect("debug json");
        assert_eq!(payload["raw_stream_bytes"], 1024 * 1024);
        assert_eq!(payload["truncated"], true);
        assert!(
            payload["raw_stream"].as_str().unwrap().len()
                <= DEBUG_STREAM_HEAD_BYTES + DEBUG_STREAM_TAIL_BYTES + 64
        );
        tokio::fs::remove_dir_all(output_dir)
            .await
            .expect("remove test debug dir");
    }

    #[test]
    fn build_filename_sanitizes_trace_id() {
        let name = build_filename("req", Some("../../etc/passwd:evil\\id"));
        assert!(!name.contains('/'));
        assert!(!name.contains('\\'));
        assert!(!name.contains(':'));
        assert!(!name.contains(".."));
        assert!(name.ends_with("_req.json"));

        let empty = build_filename("req", Some("///"));
        assert!(empty.ends_with("_unknown_req.json"));
    }
}
