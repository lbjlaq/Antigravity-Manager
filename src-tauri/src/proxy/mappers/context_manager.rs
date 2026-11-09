//! Context Manager Module
//!
//! Responsible for estimating token usage and purifying context (stripping thinking blocks)
//! to prevent "Prompt is too long" errors and avoid invalid signatures.

use super::claude::models::{ClaudeRequest, ContentBlock, MessageContent, SystemPrompt};
use super::openai::models::OpenAIRequest;
use serde_json::Value;

/// Helper to estimate tokens from text with multi-language awareness
///
/// Improved estimation algorithm:
/// - ASCII/English: ~4 characters per token
/// - Unicode/CJK: ~1.5 characters per token (Chinese, Japanese, Korean are tokenized differently)
/// - Adds 15% safety margin to prevent underestimation
fn estimate_tokens_from_str(s: &str) -> u32 {
    if s.is_empty() {
        return 0;
    }

    let mut ascii_chars = 0u32;
    let mut unicode_chars = 0u32;

    for c in s.chars() {
        if c.is_ascii() {
            ascii_chars += 1;
        } else {
            unicode_chars += 1;
        }
    }

    // ASCII: ~4 chars/token, Unicode/CJK: ~1.5 chars/token
    let ascii_tokens = (ascii_chars as f32 / 4.0).ceil() as u32;
    let unicode_tokens = (unicode_chars as f32 / 1.5).ceil() as u32;

    // Add 15% safety margin to account for tokenizer variations
    ((ascii_tokens + unicode_tokens) as f32 * 1.15).ceil() as u32
}

/// Estimate token cost for an image from an OpenAI-format image_url.
/// Handles both base64 data URLs and remote URLs.
/// Gemini counts standard images at ~258 tokens. For very large images
/// (>1MB base64 payload), we scale up since high-res images tokenize higher.
fn estimate_image_tokens_from_url(url: &str) -> u32 {
    const BASE_IMAGE_TOKENS: u32 = 258;

    if url.starts_with("data:") {
        // data:image/png;base64,<data>
        // Extract the base64 portion after the comma
        if let Some(comma_pos) = url.find(',') {
            let base64_len = url.len() - comma_pos - 1;
            // Approximate raw bytes: base64 encodes 3 bytes into 4 chars
            let raw_bytes = (base64_len * 3) / 4;

            // Gemini: standard images = 258 tokens
            // High-res images (>1MB) scale higher, up to ~10k tokens for very large ones
            if raw_bytes > 4_000_000 {
                // >4MB: very high resolution
                10_000
            } else if raw_bytes > 1_000_000 {
                // 1-4MB: high resolution, scale linearly
                let factor = raw_bytes as f32 / 1_000_000.0;
                (BASE_IMAGE_TOKENS as f32 * factor * 4.0).ceil() as u32
            } else {
                BASE_IMAGE_TOKENS
            }
        } else {
            BASE_IMAGE_TOKENS
        }
    } else {
        // Remote URL: assume standard resolution
        BASE_IMAGE_TOKENS
    }
}

/// Estimate token cost for audio from a URL.
/// Audio is tokenized at roughly 32 tokens per second.
/// From base64, we estimate duration from payload size.
fn estimate_media_tokens_from_url(url: &str) -> u32 {
    const BASE_AUDIO_TOKENS: u32 = 500; // ~15 seconds default

    if url.starts_with("data:") {
        if let Some(comma_pos) = url.find(',') {
            let base64_len = url.len() - comma_pos - 1;
            let raw_bytes = (base64_len * 3) / 4;
            // Rough estimate: 16kHz, 16-bit mono audio ≈ 32KB/s
            // 32 tokens/second
            let estimated_seconds = raw_bytes as f32 / 32_000.0;
            let tokens = (estimated_seconds * 32.0).ceil() as u32;
            tokens.max(64) // minimum 64 tokens for any audio
        } else {
            BASE_AUDIO_TOKENS
        }
    } else {
        BASE_AUDIO_TOKENS
    }
}

/// Estimate token cost for Gemini inlineData (base64 images/audio in Gemini format).
/// mime_type determines the estimation strategy. data_len is the length of the base64 string.
fn estimate_inline_data_tokens(mime_type: &str, data_len: usize) -> u32 {
    if mime_type.starts_with("image/") {
        // Same logic as estimate_image_tokens_from_url for base64
        let raw_bytes = (data_len * 3) / 4;
        if raw_bytes > 4_000_000 {
            10_000
        } else if raw_bytes > 1_000_000 {
            let factor = raw_bytes as f32 / 1_000_000.0;
            (258.0 * factor * 4.0).ceil() as u32
        } else {
            258
        }
    } else if mime_type.starts_with("audio/") {
        let raw_bytes = (data_len * 3) / 4;
        let estimated_seconds = raw_bytes as f32 / 32_000.0;
        (estimated_seconds * 32.0).ceil().max(64.0) as u32
    } else {
        // Unknown media: treat as text approximation
        estimate_tokens_from_str(&format!("[binary data: {} bytes]", data_len))
    }
}

/// [FIX #3325] Estimate raw input tokens directly from an incoming JSON payload string (OpenAI, Claude, or Gemini format)
/// Used as a fallback when upstream returns an error status (>=400) without token usage metadata.
/// 接入 Pipeline 协议无关通用估算引擎与全局高并发内容哈希缓存。
pub fn estimate_raw_tokens_from_payload(payload: &str) -> u32 {
    if payload.is_empty() {
        return 0;
    }
    if let Ok(json) = serde_json::from_str::<Value>(payload) {
        return crate::proxy::pipeline::estimate_tokens(&json);
    }
    // Fallback: estimate from raw string
    crate::proxy::pipeline::estimator::estimate_tokens_from_str(payload)
}

/// Context Manager implementation
pub struct ContextManager;

impl ContextManager {
    /// Estimate token usage for a Claude Request
    ///
    /// This is a lightweight estimation, not a precise count.
    /// It iterates through all messages and blocks to sum up estimated tokens.
    pub fn estimate_token_usage(request: &ClaudeRequest) -> u32 {
        let mut total = 0;

        // System prompt
        if let Some(sys) = &request.system {
            match sys {
                SystemPrompt::String(s) => total += estimate_tokens_from_str(s),
                SystemPrompt::Array(blocks) => {
                    for block in blocks {
                        total += estimate_tokens_from_str(&block.text);
                    }
                }
            }
        }

        // Messages
        for msg in &request.messages {
            // Message overhead
            total += 4;

            match &msg.content {
                MessageContent::String(s) => {
                    total += estimate_tokens_from_str(s);
                }
                MessageContent::Array(blocks) => {
                    for block in blocks {
                        match block {
                            ContentBlock::Text { text } => {
                                total += estimate_tokens_from_str(text);
                            }
                            ContentBlock::Thinking { thinking, .. } => {
                                total += estimate_tokens_from_str(thinking);
                                // Signature overhead
                                total += 100;
                            }
                            ContentBlock::RedactedThinking { data } => {
                                total += estimate_tokens_from_str(data);
                            }
                            ContentBlock::ToolUse { name, input, .. } => {
                                total += 20; // Function call overhead
                                total += estimate_tokens_from_str(name);
                                if let Ok(json_str) = serde_json::to_string(input) {
                                    total += estimate_tokens_from_str(&json_str);
                                }
                            }
                            ContentBlock::ToolResult { content, .. } => {
                                total += 10; // Result overhead
                                             // content is serde_json::Value
                                if let Some(s) = content.as_str() {
                                    total += estimate_tokens_from_str(s);
                                } else if let Some(arr) = content.as_array() {
                                    for item in arr {
                                        if let Some(text) =
                                            item.get("text").and_then(|t| t.as_str())
                                        {
                                            total += estimate_tokens_from_str(text);
                                        }
                                    }
                                } else {
                                    // Fallback for objects or other types
                                    if let Ok(s) = serde_json::to_string(content) {
                                        total += estimate_tokens_from_str(&s);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Tools definition overhead (rough estimate)
        if let Some(tools) = &request.tools {
            for tool in tools {
                if let Ok(json_str) = serde_json::to_string(tool) {
                    total += estimate_tokens_from_str(&json_str);
                }
            }
        }

        // Thinking budget overhead if enabled
        if let Some(thinking) = &request.thinking {
            if let Some(budget) = thinking.budget_tokens {
                // Reserve budget in estimation
                total += budget;
            }
        }

        total
    }
}

impl ContextManager {
    /// Estimate token usage for an OpenAI Request
    pub fn estimate_openai_token_usage(request: &OpenAIRequest) -> u32 {
        let mut total = 0;

        // System or developer messages, tools definitions, prompt
        if let Some(prompt) = &request.prompt {
            total += estimate_tokens_from_str(prompt);
        }
        if let Some(instructions) = &request.instructions {
            total += estimate_tokens_from_str(instructions);
        }

        for msg in &request.messages {
            total += 4; // msg overhead

            if let Some(ref content) = msg.content {
                match content {
                    crate::proxy::mappers::openai::models::OpenAIContent::String(s) => {
                        total += estimate_tokens_from_str(s);
                    }
                    crate::proxy::mappers::openai::models::OpenAIContent::Array(blocks) => {
                        for block in blocks {
                            match block {
                                crate::proxy::mappers::openai::models::OpenAIContentBlock::Text { text } => {
                                    total += estimate_tokens_from_str(text);
                                }
                                crate::proxy::mappers::openai::models::OpenAIContentBlock::ImageUrl { image_url } => {
                                    // Gemini counts images at ~258 tokens for standard resolution.
                                    // For base64 data URLs, estimate higher based on payload size
                                    // since very large images can consume significantly more tokens.
                                    total += estimate_image_tokens_from_url(&image_url.url);
                                }
                                crate::proxy::mappers::openai::models::OpenAIContentBlock::AudioUrl { audio_url } => {
                                    // Audio is tokenized at ~32 tokens per second (~25 bytes/token from base64)
                                    total += estimate_media_tokens_from_url(&audio_url.url);
                                }
                                crate::proxy::mappers::openai::models::OpenAIContentBlock::InputAudio { input_audio } => {
                                    // input_audio 携带裸 base64，直接按 inlineData 估算
                                    total += estimate_inline_data_tokens(
                                        &input_audio.mime_type(),
                                        input_audio.data.len(),
                                    );
                                }
                                crate::proxy::mappers::openai::models::OpenAIContentBlock::VideoUrl { video_url } => {
                                    // Video token estimation based on media payload size
                                    total += estimate_media_tokens_from_url(&video_url.url);
                                }
                            }
                        }
                    }
                }
            }

            if let Some(ref reasoning) = msg.reasoning_content {
                total += estimate_tokens_from_str(reasoning);
                total += 100; // signature/thinking overhead
            }

            if let Some(ref tool_calls) = msg.tool_calls {
                for tc in tool_calls {
                    total += 20;
                    if let Some(ref func) = tc.function {
                        total += estimate_tokens_from_str(&func.name);
                        total += estimate_tokens_from_str(&func.arguments);
                    }
                }
            }

            if let Some(ref name) = msg.name {
                total += estimate_tokens_from_str(name);
            }
        }

        if let Some(tools) = &request.tools {
            for tool in tools {
                if let Ok(json_str) = serde_json::to_string(tool) {
                    total += estimate_tokens_from_str(&json_str);
                }
            }
        }

        if let Some(thinking) = &request.thinking {
            if let Some(budget) = thinking.budget_tokens {
                total += budget;
            }
        }

        total
    }

    /// Estimate token usage for a Gemini Request represented as serde_json::Value
    pub fn estimate_gemini_token_usage(body: &Value) -> u32 {
        let body = body.get("request").unwrap_or(body);
        let mut total = 0;

        // systemInstruction
        if let Some(sys_inst) = body.get("systemInstruction") {
            if let Some(parts) = sys_inst.get("parts").and_then(|p| p.as_array()) {
                for part in parts {
                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                        total += estimate_tokens_from_str(text);
                    }
                }
            }
        }

        // contents
        if let Some(contents) = body.get("contents").and_then(|c| c.as_array()) {
            for msg in contents {
                total += 4; // msg overhead
                if let Some(parts) = msg.get("parts").and_then(|p| p.as_array()) {
                    for part in parts {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            total += estimate_tokens_from_str(text);
                        }
                        if let Some(thought) = part.get("thought").and_then(|t| t.as_bool()) {
                            if thought {
                                total += 100; // thinking overhead
                            }
                        }
                        // inlineData: base64-encoded images/audio embedded in the request
                        if let Some(inline_data) = part.get("inlineData") {
                            let mime = inline_data
                                .get("mimeType")
                                .and_then(|m| m.as_str())
                                .unwrap_or("");
                            let data_len = inline_data
                                .get("data")
                                .and_then(|d| d.as_str())
                                .map(|s| s.len())
                                .unwrap_or(0);
                            total += estimate_inline_data_tokens(mime, data_len);
                        }
                        if let Some(fc) = part.get("functionCall") {
                            total += 20;
                            if let Some(name) = fc.get("name").and_then(|n| n.as_str()) {
                                total += estimate_tokens_from_str(name);
                            }
                            if let Some(args) = fc.get("args") {
                                if let Ok(json_str) = serde_json::to_string(args) {
                                    total += estimate_tokens_from_str(&json_str);
                                }
                            }
                        }
                        if let Some(fr) = part.get("functionResponse") {
                            total += 10;
                            if let Some(name) = fr.get("name").and_then(|n| n.as_str()) {
                                total += estimate_tokens_from_str(name);
                            }
                            if let Some(resp) = fr.get("response") {
                                if let Ok(json_str) = serde_json::to_string(resp) {
                                    total += estimate_tokens_from_str(&json_str);
                                }
                            }
                        }
                    }
                }
            }
        }

        // tools
        if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
            for tool in tools {
                if let Ok(json_str) = serde_json::to_string(tool) {
                    total += estimate_tokens_from_str(&json_str);
                }
            }
        }

        total
    }

    /// Re-estimate (and optionally compress) AFTER mapping + thinking restore on the transit body.
    pub fn apply_post_transit_context_mgmt(body: &mut Value, mapped_model: &str) -> u32 {
        let _ = mapped_model;
        // 不改写客户端正文、思考或工具回执。用量只用于观测。
        Self::estimate_gemini_token_usage(body)
    }
}
#[cfg(test)]
mod tests {
    use super::super::claude::models::Message;
    use super::*;

    // Helper to create a request since Default is not implemented
    fn create_test_request() -> ClaudeRequest {
        ClaudeRequest {
            model: "claude-3-5-sonnet".into(),
            messages: vec![],
            system: None,
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        }
    }

    #[test]
    fn test_estimate_tokens() {
        let mut req = create_test_request();
        req.messages = vec![Message {
            role: "user".into(),
            content: MessageContent::String("Hello World".into()),
        }];

        let tokens = ContextManager::estimate_token_usage(&req);
        assert!(tokens > 0);
        assert!(tokens < 50);
    }
}
