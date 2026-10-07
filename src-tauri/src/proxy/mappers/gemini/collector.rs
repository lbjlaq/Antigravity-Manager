// Gemini Stream Collector
// Used for auto-converting streaming responses to JSON for non-streaming requests

use bytes::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};
use tracing::debug;

use crate::proxy::SignatureCache; // Assuming this is available at crate root or re-exported

/// Collects a Gemini SSE stream into a complete Gemini Response Value
/// ALSO performs signature caching side-effect
pub async fn collect_stream_to_json<S, E>(stream: S, session_id: &str) -> Result<Value, String>
where
    S: futures::Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    collect_stream_to_json_with_anchor(stream, session_id, None).await
}

pub async fn collect_stream_to_json_with_anchor<S, E>(
    mut stream: S,
    session_id: &str,
    anchor: Option<&str>,
) -> Result<Value, String>
where
    S: futures::Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    let mut collected_response = json!({
        "candidates": [
            {
                "content": {
                    "parts": [],
                    "role": "model"
                },
                "finishReason": "STOP",
                "index": 0
            }
        ]
    });

    let mut content_parts: Vec<Value> = Vec::new(); // To accumulate parts
    let mut usage_metadata: Option<Value> = None;
    let mut finish_reason: Option<String> = None;
    let mut stream_error: Option<Value> = None;
    let mut line_buffer = bytes::BytesMut::new();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| {
            crate::proxy::mappers::error_classifier::report_stream_error(
                "gemini-collector",
                "collect_stream_to_json_with_anchor",
                &e,
                format!("session={}", session_id),
            )
            .client_message()
        })?;

        line_buffer.extend_from_slice(&chunk);

        while let Some(pos) = line_buffer.iter().position(|&b| b == b'\n') {
            let line_raw = line_buffer.split_to(pos + 1);
            let line_str = String::from_utf8_lossy(&line_raw);
            let line = line_str.trim();
            if line.starts_with("data: ") {
                let json_part = line.trim_start_matches("data: ").trim();
                if json_part == "[DONE]" {
                    continue;
                }

                if let Ok(mut json) = serde_json::from_str::<Value>(json_part) {
                    // Unwrap v1internal response wrapper similar to handler
                    let actual_data =
                        if let Some(inner) = json.get_mut("response").map(|v| v.take()) {
                            inner
                        } else {
                            json
                        };

                    // Check for error payload (e.g. 504 Deadline Exceeded, 503 Overloaded, 429)
                    if let Some(err) = actual_data.get("error") {
                        let err_val = if actual_data.as_object().is_some_and(|m| m.len() == 1) {
                            actual_data
                        } else {
                            json!({ "error": err })
                        };
                        stream_error = Some(err_val);
                        break;
                    }

                    // 1. Capture Usage
                    if let Some(usage) = actual_data.get("usageMetadata") {
                        usage_metadata = Some(usage.clone());
                    }

                    // 2. Capture Content & Signature
                    if let Some(candidates) =
                        actual_data.get("candidates").and_then(|c| c.as_array())
                    {
                        if let Some(candidate) = candidates.first() {
                            // Update finish reason if present
                            if let Some(fr) = candidate.get("finishReason").and_then(|v| v.as_str())
                            {
                                finish_reason = Some(fr.to_string());
                            }

                            if let Some(parts) = candidate
                                .get("content")
                                .and_then(|c| c.get("parts"))
                                .and_then(|p| p.as_array())
                            {
                                for part in parts {
                                    // Signature Caching
                                    if let Some(sig) =
                                        part.get("thoughtSignature").and_then(|s| s.as_str())
                                    {
                                        // Cache it!
                                        SignatureCache::global().cache_session_signature(
                                            session_id,
                                            sig.to_string(),
                                            1,
                                        );
                                        debug!(
                                            "[Gemini-AutoConverter] Cached signature (len: {}) for session: {}",
                                            sig.len(),
                                            session_id
                                        );
                                    }

                                    // Collect part
                                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                        if let Some(last) = content_parts.last_mut() {
                                            if last.get("text").is_some()
                                                && part.get("thought").is_none()
                                                && last.get("thought").is_none()
                                            {
                                                // Merge text
                                                if let Some(last_text) =
                                                    last.get_mut("text").and_then(|v| v.as_str())
                                                {
                                                    let new_text = format!("{}{}", last_text, text);
                                                    *last = json!({ "text": new_text });
                                                    continue;
                                                }
                                            }
                                        }
                                        content_parts.push(part.clone());
                                    } else {
                                        // Other parts (images, thoughts, function calls), just push
                                        content_parts.push(part.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if stream_error.is_some() {
            break;
        }
    }

    // Flush leftover buffer if any
    if stream_error.is_none() && !line_buffer.is_empty() {
        let line_str = String::from_utf8_lossy(&line_buffer);
        let line = line_str.trim();
        if line.starts_with("data: ") {
            let json_part = line.trim_start_matches("data: ").trim();
            if json_part != "[DONE]" {
                if let Ok(mut json) = serde_json::from_str::<Value>(json_part) {
                    let actual_data =
                        if let Some(inner) = json.get_mut("response").map(|v| v.take()) {
                            inner
                        } else {
                            json
                        };
                    if let Some(err) = actual_data.get("error") {
                        let err_val = if actual_data.as_object().is_some_and(|m| m.len() == 1) {
                            actual_data
                        } else {
                            json!({ "error": err })
                        };
                        stream_error = Some(err_val);
                    }
                }
            }
        }
    }

    // If stream contained an error event/payload, return it directly without polluting state
    if let Some(err_val) = stream_error {
        return Ok(err_val);
    }

    // Stream finished without content parts or finish reason -> premature interruption
    if content_parts.is_empty() && finish_reason.is_none() {
        return Err(
            crate::proxy::mappers::error_classifier::report_stream_error(
                "gemini-collector",
                "collect_stream_to_json_with_anchor",
                &"stream terminated prematurely without content or finish reason",
                format!("session={}", session_id),
            )
            .client_message(),
        );
    }

    if !content_parts.is_empty() {
        let anchor_str = anchor.unwrap_or("root");
        crate::proxy::thinking_store::capture_gemini_parts_with_anchor(
            session_id,
            &content_parts,
            anchor_str,
        );
    }

    // Construct final response
    collected_response["candidates"][0]["content"]["parts"] = json!(content_parts);
    if let Some(fr) = finish_reason {
        collected_response["candidates"][0]["finishReason"] = json!(fr);
    }
    if let Some(usage) = usage_metadata {
        collected_response["usageMetadata"] = usage;
    }

    Ok(collected_response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use std::io;

    #[tokio::test]
    async fn test_collect_simple_text_response() {
        let sse_data = vec![
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hello\"}],\"role\":\"model\"},\"index\":0}]}\n\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" world!\"}],\"role\":\"model\"},\"finishReason\":\"STOP\",\"index\":0}],\"usageMetadata\":{\"totalTokenCount\":10}}\n\n",
            "data: [DONE]\n\n",
        ];

        let byte_stream = stream::iter(
            sse_data
                .into_iter()
                .map(|s| Ok::<Bytes, io::Error>(Bytes::from(s))),
        );

        let result = collect_stream_to_json(byte_stream, "test_session").await;
        assert!(result.is_ok());

        let resp = result.unwrap();
        let parts = resp["candidates"][0]["content"]["parts"]
            .as_array()
            .unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], "Hello world!");
        assert_eq!(resp["candidates"][0]["finishReason"], "STOP");
        assert_eq!(resp["usageMetadata"]["totalTokenCount"], 10);
    }

    #[tokio::test]
    async fn test_collect_error_payload_propagation_504() {
        let sse_data = vec![
            "data: {\"error\":{\"code\":504,\"message\":\"stream idle timeout\",\"status\":\"DEADLINE_EXCEEDED\"}}\n\n",
            "data: [DONE]\n\n",
        ];

        let byte_stream = stream::iter(
            sse_data
                .into_iter()
                .map(|s| Ok::<Bytes, io::Error>(Bytes::from(s))),
        );

        let result = collect_stream_to_json(byte_stream, "test_session_504").await;
        assert!(result.is_ok());

        let resp = result.unwrap();
        assert!(resp.get("error").is_some());
        assert_eq!(resp["error"]["code"], 504);
        assert_eq!(resp["error"]["status"], "DEADLINE_EXCEEDED");
    }

    #[tokio::test]
    async fn test_collect_error_payload_with_v1internal_wrapper() {
        let sse_data = vec![
            "data: {\"response\":{\"error\":{\"code\":503,\"message\":\"model overloaded\",\"status\":\"UNAVAILABLE\"}}}\n\n",
            "data: [DONE]\n\n",
        ];

        let byte_stream = stream::iter(
            sse_data
                .into_iter()
                .map(|s| Ok::<Bytes, io::Error>(Bytes::from(s))),
        );

        let result = collect_stream_to_json(byte_stream, "test_session_503").await;
        assert!(result.is_ok());

        let resp = result.unwrap();
        assert!(resp.get("error").is_some());
        assert_eq!(resp["error"]["code"], 503);
        assert_eq!(resp["error"]["status"], "UNAVAILABLE");
    }

    #[tokio::test]
    async fn test_collect_premature_empty_stream_fails() {
        let sse_data = vec!["data: [DONE]\n\n"];

        let byte_stream = stream::iter(
            sse_data
                .into_iter()
                .map(|s| Ok::<Bytes, io::Error>(Bytes::from(s))),
        );

        let result = collect_stream_to_json(byte_stream, "test_session_empty").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("stream terminated prematurely"));
    }

    #[tokio::test]
    async fn test_collect_multibyte_chunk_split() {
        let cyrillic_word = "Привет, мир! 🚀";
        let chunk_data = format!(
            "data: {{\"candidates\":[{{\"content\":{{\"parts\":[{{\"text\":\"{}\"}}],\"role\":\"model\"}},\"finishReason\":\"STOP\",\"index\":0}}]}}\n\ndata: [DONE]\n\n",
            cyrillic_word
        );

        let bytes = chunk_data.into_bytes();
        let chunk_size = 13;
        let mut chunks = Vec::new();
        for chunk in bytes.chunks(chunk_size) {
            chunks.push(Ok::<Bytes, io::Error>(Bytes::copy_from_slice(chunk)));
        }

        let byte_stream = stream::iter(chunks);
        let result = collect_stream_to_json(byte_stream, "test_session_multibyte").await;
        assert!(result.is_ok());

        let resp = result.unwrap();
        let parts = resp["candidates"][0]["content"]["parts"]
            .as_array()
            .unwrap();
        assert_eq!(parts[0]["text"], cyrillic_word);
    }
}
