//! Epic-008 Story-012-02: Feedback extraction utilities for budget optimization
//!
//! Helper functions to extract prompt, budget, and quality metrics from
//! OpenAI and Claude requests/responses for the budget optimizer.

use crate::proxy::mappers::openai::{OpenAIContent, OpenAIRequest};
use serde_json::Value;

/// Extract first user prompt from OpenAI request
///
/// # Arguments
/// * `request` - OpenAI request containing messages
///
/// # Returns
/// First user message content, or empty string if not found
pub fn extract_openai_prompt(request: &OpenAIRequest) -> String {
    for msg in &request.messages {
        if msg.role == "user" {
            if let Some(content) = &msg.content {
                return match content {
                    OpenAIContent::String(s) => s.clone(),
                    OpenAIContent::Array(parts) => {
                        // For multi-part content, join text parts
                        parts
                            .iter()
                            .filter_map(|part| {
                                if let crate::proxy::mappers::openai::OpenAIContentBlock::Text { text } = part {
                                    Some(text.clone())
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(" ")
                    }
                };
            }
        }
    }
    String::new()
}

/// Extract thinking budget from OpenAI response
///
/// # Arguments
/// * `response_json` - OpenAI response as JSON Value
///
/// # Returns
/// Thinking token count, or 0 if not found
///
/// # Logic
/// Looks for reasoning_content length or usage.completion_tokens
pub fn extract_openai_budget(response_json: &serde_json::Value) -> u32 {
    // Try to get thinking tokens from reasoning_content
    if let Some(choices) = response_json.get("choices").and_then(|v| v.as_array()) {
        for choice in choices {
            if let Some(message) = choice.get("message") {
                // Check for reasoning_content
                if let Some(reasoning) = message
                    .get("reasoning_content")
                    .and_then(|v| v.as_str())
                {
                    // Rough estimate: 1 token ≈ 4 characters for English text
                    return (reasoning.len() / 4) as u32;
                }
            }
        }
    }

    // Fallback: Try usage.completion_tokens
    if let Some(usage) = response_json.get("usage") {
        if let Some(completion_tokens) = usage.get("completion_tokens").and_then(|v| v.as_u64()) {
            return completion_tokens as u32;
        }
    }

    0
}

/// Calculate quality score based on response characteristics
///
/// # Arguments
/// * `response_json` - OpenAI response as JSON Value
/// * `budget_used` - Actual thinking budget used
///
/// # Returns
/// Quality score between 0.0 and 1.0
///
/// # Heuristics
/// - More thinking tokens → higher quality (up to a point)
/// - Presence of reasoning_content → bonus
/// - finish_reason="stop" → higher quality
/// - Too few tokens (<1000) → lower quality
///
/// # Formula
/// Base: 0.7 (neutral)
/// +0.1 if has reasoning_content
/// +0.1 if finish_reason="stop"
/// +0.05 if budget > 5000 (moderate thinking)
/// +0.05 if budget > 10000 (deep thinking)
/// -0.1 if budget < 1000 (too shallow)
pub fn calculate_openai_quality(response_json: &serde_json::Value, budget_used: u32) -> f32 {
    let mut score: f32 = 0.7; // Neutral base

    // Check for reasoning_content (indicates thinking was used)
    let has_reasoning = response_json
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|msg| msg.get("reasoning_content"))
        .is_some();

    if has_reasoning {
        score += 0.1;
    }

    // Check finish_reason
    let finish_reason = response_json
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(|v| v.as_str());

    if finish_reason == Some("stop") {
        score += 0.1;
    }

    // Adjust based on budget usage
    if budget_used > 10000 {
        score += 0.05; // Deep thinking bonus
    } else if budget_used > 5000 {
        score += 0.05; // Moderate thinking bonus
    } else if budget_used < 1000 {
        score -= 0.1; // Too shallow penalty
    }

    // Clamp to [0.0, 1.0]
    score.max(0.0).min(1.0)
}

// ===== Claude-specific Functions =====

/// Extract first user prompt from Claude request
///
/// # Arguments
/// * `request_json` - Claude request as JSON Value
///
/// # Returns
/// First user message content, or empty string if not found
pub fn extract_claude_prompt(request_json: &Value) -> String {
    if let Some(messages) = request_json.get("messages").and_then(|v| v.as_array()) {
        for msg in messages {
            if msg.get("role").and_then(|r| r.as_str()) == Some("user") {
                if let Some(content) = msg.get("content") {
                    // Content can be string or array
                    return match content {
                        Value::String(s) => s.clone(),
                        Value::Array(parts) => {
                            // For multi-part content, join text parts
                            parts
                                .iter()
                                .filter_map(|part| {
                                    part.get("text").and_then(|v| v.as_str()).map(|s| s.to_string())
                                })
                                .collect::<Vec<_>>()
                                .join(" ")
                        }
                        _ => String::new(),
                    };
                }
            }
        }
    }
    String::new()
}

/// Extract thinking budget from Claude response
///
/// # Arguments
/// * `response_json` - Claude response as JSON Value (ClaudeResponse)
///
/// # Returns
/// Thinking token count from usage.output_tokens
pub fn extract_claude_budget(response_json: &Value) -> u32 {
    // Claude response has usage.output_tokens
    if let Some(usage) = response_json.get("usage") {
        if let Some(output_tokens) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
            return output_tokens as u32;
        }
    }

    0
}

/// Calculate quality score for Claude response
///
/// # Arguments
/// * `response_json` - Claude response as JSON Value
/// * `budget_used` - Actual thinking budget used
///
/// # Returns
/// Quality score between 0.0 and 1.0
///
/// # Heuristics
/// - More output tokens → higher quality (up to a point)
/// - stop_reason="end_turn" → higher quality
/// - Cache hits → bonus (efficient)
/// - Too few tokens (<500) → lower quality
pub fn calculate_claude_quality(response_json: &Value, budget_used: u32) -> f32 {
    let mut score: f32 = 0.7; // Neutral base

    // Check stop_reason
    let stop_reason = response_json
        .get("stop_reason")
        .and_then(|v| v.as_str());

    if stop_reason == Some("end_turn") {
        score += 0.1;
    }

    // Check for cache usage (indicates efficiency)
    if let Some(usage) = response_json.get("usage") {
        if usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) > 0 {
            score += 0.05; // Cache hit bonus
        }
    }

    // Adjust based on budget usage
    if budget_used > 10000 {
        score += 0.05; // Deep thinking bonus
    } else if budget_used > 5000 {
        score += 0.05; // Moderate thinking bonus
    } else if budget_used < 500 {
        score -= 0.1; // Too shallow penalty
    }

    // Clamp to [0.0, 1.0]
    score.max(0.0).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::mappers::openai::OpenAIMessage;
    use serde_json::json;

    #[test]
    fn test_extract_openai_prompt_simple() {
        let request = OpenAIRequest {
            model: "gpt-4".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("hello world".to_string())),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }],
            prompt: None,
            stream: false,
            n: None,
            max_tokens: None,
            temperature: None,
            top_p: None,
            stop: None,
            response_format: None,
            tools: None,
            tool_choice: None,
            input: None,
            instructions: None,
            parallel_tool_calls: None,
            reasoning_effort: None,
        };

        assert_eq!(extract_openai_prompt(&request), "hello world");
    }

    #[test]
    fn test_extract_openai_budget_from_reasoning() {
        let response = json!({
            "choices": [{
                "message": {
                    "reasoning_content": "a".repeat(1000) // 1000 chars ≈ 250 tokens
                }
            }]
        });

        let budget = extract_openai_budget(&response);
        assert!(budget >= 200 && budget <= 300);
    }

    #[test]
    fn test_calculate_openai_quality_high() {
        let response = json!({
            "choices": [{
                "message": {
                    "reasoning_content": "detailed analysis..."
                },
                "finish_reason": "stop"
            }]
        });

        let quality = calculate_openai_quality(&response, 12000);
        assert!(quality >= 0.9); // Has reasoning + stop + deep thinking
    }

    #[test]
    fn test_calculate_openai_quality_low() {
        let response = json!({
            "choices": [{
                "message": {},
                "finish_reason": "length"
            }]
        });

        let quality = calculate_openai_quality(&response, 500);
        assert!(quality < 0.7); // No reasoning + shallow
    }

    #[test]
    fn test_extract_claude_prompt() {
        let request = json!({
            "messages": [{
                "role": "user",
                "content": "explain AI"
            }]
        });

        assert_eq!(extract_claude_prompt(&request), "explain AI");
    }

    #[test]
    fn test_extract_claude_budget() {
        let response = json!({
            "usage": {
                "output_tokens": 1500
            }
        });

        assert_eq!(extract_claude_budget(&response), 1500);
    }

    #[test]
    fn test_calculate_claude_quality_high() {
        let response = json!({
            "stop_reason": "end_turn",
            "usage": {
                "output_tokens": 12000,
                "cache_read_input_tokens": 5000
            }
        });

        let quality = calculate_claude_quality(&response, 12000);
        assert!(quality >= 0.85); // end_turn + cache + deep thinking
    }
}
