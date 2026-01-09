//! Comprehensive test suite for Extended Thinking with different models
//!
//! Tests all variations of model routing and thinking configuration:
//! - Claude models with/without thinking suffix
//! - Gemini models with thinking via parameter
//! - Model routing correctness
//! - Request transformation validation

#[cfg(test)]
mod tests {
    use crate::proxy::common::model_mapping::{map_claude_model_to_gemini, resolve_model_route};
    use crate::proxy::mappers::claude::models::{
        ClaudeRequest, Message, MessageContent, ThinkingConfig
    };
    use crate::proxy::mappers::claude::request::transform_claude_request_in;
    use serde_json::json;
    use std::collections::HashMap;

    // ==================================================================================
    // UNIT TESTS: Model Routing
    // ==================================================================================

    /// Test Claude Opus routing - should route to thinking version
    #[test]
    fn test_claude_opus_routing() {
        assert_eq!(
            map_claude_model_to_gemini("claude-opus-4-5"),
            "claude-opus-4-5-thinking",
            "Opus должен роутиться в thinking версию (Google предоставляет только с thinking)"
        );

        assert_eq!(
            map_claude_model_to_gemini("claude-opus-4"),
            "claude-opus-4-5-thinking"
        );

        assert_eq!(
            map_claude_model_to_gemini("claude-opus-4-5-20251101"),
            "claude-opus-4-5-thinking"
        );
    }

    /// Test Claude Sonnet routing - should preserve thinking/non-thinking versions
    #[test]
    fn test_claude_sonnet_routing() {
        // Sonnet БЕЗ thinking
        assert_eq!(
            map_claude_model_to_gemini("claude-sonnet-4-5"),
            "claude-sonnet-4-5",
            "Sonnet БЕЗ thinking должен сохраняться как есть"
        );

        // Sonnet С thinking
        assert_eq!(
            map_claude_model_to_gemini("claude-sonnet-4-5-thinking"),
            "claude-sonnet-4-5-thinking",
            "Sonnet С thinking должен сохраняться как есть"
        );

        // Legacy versions
        assert_eq!(
            map_claude_model_to_gemini("claude-3-5-sonnet-20241022"),
            "claude-sonnet-4-5"
        );
    }

    /// Test Claude Haiku routing - should route to Gemini Pro High WITHOUT -thinking suffix
    #[test]
    fn test_claude_haiku_routing() {
        assert_eq!(
            map_claude_model_to_gemini("claude-haiku-4-5"),
            "gemini-3-pro-high",
            "Haiku должен роутиться в Gemini Pro High БЕЗ -thinking суффикса"
        );

        assert_eq!(
            map_claude_model_to_gemini("claude-haiku-4"),
            "gemini-3-pro-high"
        );

        assert_eq!(
            map_claude_model_to_gemini("claude-3-haiku-20240307"),
            "gemini-3-pro-high"
        );
    }

    /// Test Gemini routing - should NOT use -thinking suffix
    #[test]
    fn test_gemini_routing_no_thinking_suffix() {
        // Gemini БЕЗ -thinking суффикса (thinking включается через параметр!)
        assert_eq!(
            map_claude_model_to_gemini("gemini-3-pro"),
            "gemini-3-pro-high",
            "Gemini НЕ должен использовать -thinking суффикс"
        );

        assert_eq!(
            map_claude_model_to_gemini("gemini-3-pro-high"),
            "gemini-3-pro-high"
        );

        assert_eq!(
            map_claude_model_to_gemini("gemini-3-pro-low"),
            "gemini-3-pro-low"
        );

        assert_eq!(
            map_claude_model_to_gemini("gemini-3-flash"),
            "gemini-3-flash"
        );
    }

    /// Test fallback routing - should use Gemini Pro High
    #[test]
    fn test_fallback_routing() {
        assert_eq!(
            map_claude_model_to_gemini("unknown-model"),
            "gemini-3-pro-high",
            "Неизвестные модели должны роутиться в Gemini Pro High"
        );

        assert_eq!(
            map_claude_model_to_gemini("some-random-model"),
            "gemini-3-pro-high"
        );
    }

    /// Test pass-through for gemini- prefix
    #[test]
    fn test_gemini_passthrough() {
        assert_eq!(
            map_claude_model_to_gemini("gemini-2.5-flash-mini-test"),
            "gemini-2.5-flash-mini-test",
            "Gemini модели с префиксом должны проходить как есть"
        );
    }

    /// Test custom routing rules
    #[test]
    fn test_custom_routing_exact_match() {
        let mut custom_mapping = HashMap::new();
        custom_mapping.insert("my-model".to_string(), "gemini-3-flash".to_string());

        assert_eq!(
            resolve_model_route("my-model", &custom_mapping),
            "gemini-3-flash",
            "Точное совпадение должно иметь приоритет"
        );
    }

    /// Test wildcard routing
    #[test]
    fn test_wildcard_routing() {
        let mut custom_mapping = HashMap::new();
        custom_mapping.insert("gpt-4*".to_string(), "gemini-2.5-pro".to_string());

        assert_eq!(
            resolve_model_route("gpt-4-turbo", &custom_mapping),
            "gemini-2.5-pro",
            "Wildcard маппинг должен работать"
        );

        assert_eq!(
            resolve_model_route("gpt-4", &custom_mapping),
            "gemini-2.5-pro"
        );
    }

    // ==================================================================================
    // INTEGRATION TESTS: Request Transformation
    // ==================================================================================

    /// Helper to create basic Claude request
    fn create_basic_request(model: &str, with_thinking: bool) -> ClaudeRequest {
        ClaudeRequest {
            model: model.to_string(),
            messages: vec![
                Message {
                    role: "user".to_string(),
                    content: MessageContent::String("Test message".to_string()),
                }
            ],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(1000),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: if with_thinking {
                Some(ThinkingConfig {
                    type_: "enabled".to_string(),
                    budget_tokens: Some(2000),
                })
            } else {
                None
            },
            metadata: None,
            output_config: None,
        }
    }

    /// Test Claude Opus WITH thinking
    #[test]
    fn test_claude_opus_with_thinking_request() {
        let req = create_basic_request("claude-opus-4-5", true);
        let result = transform_claude_request_in(&req, "test-project");

        assert!(result.is_ok(), "Request should succeed");

        let body = result.unwrap();

        // Проверяем model routing
        assert_eq!(
            body["model"].as_str(),
            Some("claude-opus-4-5-thinking"),
            "Model должна быть claude-opus-4-5-thinking"
        );

        // Проверяем thinkingConfig
        let thinking_config = body["request"]["generationConfig"]["thinkingConfig"].clone();
        assert!(
            !thinking_config.is_null(),
            "thinkingConfig должен присутствовать для Claude thinking models"
        );
        assert_eq!(
            thinking_config["includeThoughts"].as_bool(),
            Some(true)
        );
        assert!(
            thinking_config["thinkingBudget"].as_i64().unwrap() > 0
        );
    }

    /// Test Claude Sonnet WITHOUT thinking
    #[test]
    fn test_claude_sonnet_without_thinking_request() {
        let req = create_basic_request("claude-sonnet-4-5", false);
        let result = transform_claude_request_in(&req, "test-project");

        assert!(result.is_ok(), "Request should succeed");

        let body = result.unwrap();

        // Проверяем model routing
        assert_eq!(
            body["model"].as_str(),
            Some("claude-sonnet-4-5"),
            "Model должна быть claude-sonnet-4-5 БЕЗ thinking"
        );

        // Проверяем отсутствие thinkingConfig
        let thinking_config = body["request"]["generationConfig"]["thinkingConfig"].clone();
        assert!(
            thinking_config.is_null(),
            "thinkingConfig НЕ должен присутствовать когда thinking отключен"
        );
    }

    /// Test Gemini WITH thinking (via parameter, NOT model name!)
    #[test]
    fn test_gemini_with_thinking_request() {
        let req = create_basic_request("gemini-3-pro-high", true);
        let result = transform_claude_request_in(&req, "test-project");

        assert!(result.is_ok(), "Request should succeed");

        let body = result.unwrap();

        // Проверяем model routing - БЕЗ -thinking суффикса!
        assert_eq!(
            body["model"].as_str(),
            Some("gemini-3-pro-high"),
            "Gemini model НЕ должна иметь -thinking суффикс"
        );

        // Проверяем thinkingConfig (thinking включается через параметр)
        let thinking_config = body["request"]["generationConfig"]["thinkingConfig"].clone();
        assert!(
            !thinking_config.is_null(),
            "thinkingConfig должен присутствовать для Gemini с enabled thinking"
        );
        assert_eq!(
            thinking_config["includeThoughts"].as_bool(),
            Some(true)
        );
    }

    /// Test Gemini WITHOUT thinking
    #[test]
    fn test_gemini_without_thinking_request() {
        let req = create_basic_request("gemini-3-flash", false);
        let result = transform_claude_request_in(&req, "test-project");

        assert!(result.is_ok(), "Request should succeed");

        let body = result.unwrap();

        // Проверяем model routing
        assert_eq!(
            body["model"].as_str(),
            Some("gemini-3-flash"),
            "Model должна быть gemini-3-flash"
        );

        // Проверяем отсутствие thinkingConfig
        let thinking_config = body["request"]["generationConfig"]["thinkingConfig"].clone();
        assert!(
            thinking_config.is_null(),
            "thinkingConfig НЕ должен присутствовать когда thinking отключен"
        );
    }

    /// Test Haiku routing to Gemini Pro High
    #[test]
    fn test_haiku_to_gemini_routing() {
        let req = create_basic_request("claude-haiku-4-5", true);
        let result = transform_claude_request_in(&req, "test-project");

        assert!(result.is_ok(), "Request should succeed");

        let body = result.unwrap();

        // Haiku должен роутиться в Gemini Pro High БЕЗ -thinking
        assert_eq!(
            body["model"].as_str(),
            Some("gemini-3-pro-high"),
            "Haiku должен роутиться в gemini-3-pro-high"
        );

        // thinkingConfig должен быть для Gemini (через параметр)
        let thinking_config = body["request"]["generationConfig"]["thinkingConfig"].clone();
        assert!(
            !thinking_config.is_null(),
            "thinkingConfig должен присутствовать для Haiku->Gemini с thinking"
        );
    }

    // ==================================================================================
    // VALIDATION TESTS: Thinking Budget Limits
    // ==================================================================================

    /// Test thinking budget clamping for Claude models
    #[test]
    fn test_claude_thinking_budget_limits() {
        let mut req = create_basic_request("claude-opus-4-5", true);
        req.thinking = Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(64000), // Превышает максимум 32000
        });

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let body = result.unwrap();
        let budget = body["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"]
            .as_i64()
            .unwrap();

        assert!(
            budget <= 32000,
            "Thinking budget для Claude должен быть ограничен 32000, получено: {}",
            budget
        );
    }

    /// Test thinking budget clamping for Gemini Flash
    #[test]
    fn test_gemini_flash_thinking_budget_limits() {
        let mut req = create_basic_request("gemini-2.5-flash", true);
        req.thinking = Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(64000), // Превышает максимум 24576 для Flash
        });

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let body = result.unwrap();
        let budget = body["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"]
            .as_i64()
            .unwrap();

        assert!(
            budget <= 24576,
            "Thinking budget для Gemini Flash должен быть ограничен 24576, получено: {}",
            budget
        );
    }

    /// Test thinking budget clamping for Gemini Pro
    #[test]
    fn test_gemini_pro_thinking_budget_limits() {
        let mut req = create_basic_request("gemini-3-pro-high", true);
        req.thinking = Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(64000), // Превышает максимум 32000
        });

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let body = result.unwrap();
        let budget = body["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"]
            .as_i64()
            .unwrap();

        assert!(
            budget <= 32000,
            "Thinking budget для Gemini Pro должен быть ограничен 32000, получено: {}",
            budget
        );
    }

    // ==================================================================================
    // EDGE CASES
    // ==================================================================================

    /// Test model with thinking suffix but thinking disabled in request
    #[test]
    fn test_thinking_model_with_disabled_thinking() {
        let req = create_basic_request("claude-sonnet-4-5-thinking", false);
        let result = transform_claude_request_in(&req, "test-project");

        assert!(result.is_ok());

        let body = result.unwrap();

        // Model сохраняется с -thinking суффиксом
        assert_eq!(
            body["model"].as_str(),
            Some("claude-sonnet-4-5-thinking")
        );

        // Но thinkingConfig НЕ должен быть установлен
        let thinking_config = body["request"]["generationConfig"]["thinkingConfig"].clone();
        assert!(
            thinking_config.is_null(),
            "thinkingConfig НЕ должен быть если thinking.type != 'enabled'"
        );
    }

    /// Test empty thinking config
    #[test]
    fn test_empty_thinking_config() {
        let mut req = create_basic_request("claude-opus-4-5", true);
        req.thinking = Some(ThinkingConfig {
            type_: "disabled".to_string(),
            budget_tokens: None,
        });

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let body = result.unwrap();
        let thinking_config = body["request"]["generationConfig"]["thinkingConfig"].clone();

        assert!(
            thinking_config.is_null(),
            "thinkingConfig НЕ должен быть когда type='disabled'"
        );
    }

    /// Test max_tokens validation with thinking
    #[test]
    fn test_max_tokens_with_thinking() {
        let mut req = create_basic_request("claude-opus-4-5", true);
        req.max_tokens = Some(100); // Меньше чем budget
        req.thinking = Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(2000),
        });

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let body = result.unwrap();
        let max_tokens = body["request"]["generationConfig"]["maxOutputTokens"]
            .as_i64()
            .unwrap();
        let budget = body["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"]
            .as_i64()
            .unwrap();

        assert!(
            max_tokens > budget,
            "maxOutputTokens ({}) должен быть > thinkingBudget ({})",
            max_tokens,
            budget
        );
    }
}
