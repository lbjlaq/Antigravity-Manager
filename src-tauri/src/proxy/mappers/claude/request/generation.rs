// Generation Config Builder

use serde_json::{json, Value};
use crate::proxy::mappers::claude::models::ClaudeRequest;

/// Build Generation Config for Gemini API
pub fn build_generation_config(
    claude_req: &ClaudeRequest,
    has_web_search: bool,
    is_thinking_enabled: bool,
) -> Value {
    let mut config = json!({});

    // Thinking configuration
    if is_thinking_enabled {
        let mut thinking_config = json!({"includeThoughts": true});
        let budget_tokens = claude_req
            .thinking
            .as_ref()
            .and_then(|t| t.budget_tokens)
            .unwrap_or(16000);

        let model_lower = claude_req.model.to_lowercase();
        let tb_config = crate::proxy::config::get_thinking_budget_config();
        let is_gemini_limited = model_lower.contains("gemini")
            || model_lower.contains("flash")
            || model_lower.ends_with("-thinking")
            || has_web_search;

        let budget = match tb_config.mode {
            crate::proxy::config::ThinkingBudgetMode::Custom => {
                let mut custom_value = tb_config.custom_value as i32;
                if is_gemini_limited && custom_value > 24576 {
                    tracing::warn!(
                        "[Claude-Request] Custom mode: capping thinking_budget from {} to 24576 for Gemini model {}",
                        custom_value,
                        claude_req.model
                    );
                    custom_value = 24576;
                }
                custom_value
            }
            crate::proxy::config::ThinkingBudgetMode::Passthrough => budget_tokens as i32,
            crate::proxy::config::ThinkingBudgetMode::Auto => {
                if is_gemini_limited && budget_tokens > 24576 {
                    tracing::info!(
                        "[Claude-Request] Auto mode: capping thinking_budget from {} to 24576 for Gemini model {}",
                        budget_tokens,
                        claude_req.model
                    );
                    24576
                } else {
                    budget_tokens as i32
                }
            }
        };

        thinking_config["thinkingBudget"] = json!(budget);
        config["thinkingConfig"] = thinking_config;
    }

    if let Some(temp) = claude_req.temperature {
        config["temperature"] = json!(temp);
    }
    if let Some(top_p) = claude_req.top_p {
        config["topP"] = json!(top_p);
    }
    if let Some(top_k) = claude_req.top_k {
        config["topK"] = json!(top_k);
    }

    if let Some(output_config) = &claude_req.output_config {
        if let Some(effort) = &output_config.effort {
            config["effortLevel"] = json!(match effort.to_lowercase().as_str() {
                "high" => "HIGH",
                "medium" => "MEDIUM",
                "low" => "LOW",
                _ => "HIGH",
            });
        }
    }

    let mut final_max_tokens: Option<i64> = claude_req.max_tokens.map(|t| t as i64);

    if let Some(thinking_config) = config.get("thinkingConfig") {
        if let Some(budget) = thinking_config
            .get("thinkingBudget")
            .and_then(|t| t.as_u64())
        {
            let current = final_max_tokens.unwrap_or(0);
            if current <= budget as i64 {
                final_max_tokens = Some((budget + 8192) as i64);
                tracing::info!(
                    "[Generation-Config] Bumping maxOutputTokens to {} due to thinking budget of {}", 
                    final_max_tokens.unwrap(), budget
                );
            }
        }
    }

    if let Some(val) = final_max_tokens {
        config["maxOutputTokens"] = json!(val);
    }

    // Stop sequences - prevent model hallucinating dialogue markers
    // Built via format! to avoid literal sequences that may cause issues
    let user_marker = format!("<|{}|>", "user");
    let end_turn_marker = format!("<|{}|>", "end_of_turn");
    let human_marker = format!("{}{}{}", "\n", "\n", "Human:");
    config["stopSequences"] = json!([user_marker, end_turn_marker, human_marker]);

    config
}
