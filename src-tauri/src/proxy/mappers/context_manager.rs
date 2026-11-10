//! Context Manager Module
//!
//! Responsible for estimating token usage and purifying context (stripping thinking blocks)
//! to prevent "Prompt is too long" errors and avoid invalid signatures.

use super::claude::models::{ClaudeRequest, ContentBlock, MessageContent, SystemPrompt};
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
