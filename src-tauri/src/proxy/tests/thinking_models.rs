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
        ClaudeRequest, Message, MessageContent, ThinkingConfig,
    };
    use crate::proxy::mappers::claude::request::transform_claude_request_in;
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
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Test message".to_string()),
            }],
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
            tool_choice: None,
        }
    }

    /// Test Claude Opus WITH thinking
    #[test]
    fn test_claude_opus_with_thinking_request() {
        let req = create_basic_request("claude-opus-4-5", true);
        let result = transform_claude_request_in(&req, "test-project");

        assert!(result.is_ok(), "Request should succeed");

        let (body, _violations) = result.unwrap();

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
        assert_eq!(thinking_config["includeThoughts"].as_bool(), Some(true));
        assert!(thinking_config["thinkingBudget"].as_i64().unwrap() > 0);
    }

    /// Test Claude Sonnet WITHOUT thinking
    #[test]
    fn test_claude_sonnet_without_thinking_request() {
        let req = create_basic_request("claude-sonnet-4-5", false);
        let result = transform_claude_request_in(&req, "test-project");

        assert!(result.is_ok(), "Request should succeed");

        let (body, _violations) = result.unwrap();

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

        let (body, _violations) = result.unwrap();

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
        assert_eq!(thinking_config["includeThoughts"].as_bool(), Some(true));
    }

    /// Test Gemini WITHOUT thinking
    #[test]
    fn test_gemini_without_thinking_request() {
        let req = create_basic_request("gemini-3-flash", false);
        let result = transform_claude_request_in(&req, "test-project");

        assert!(result.is_ok(), "Request should succeed");

        let (body, _violations) = result.unwrap();

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

        let (body, _violations) = result.unwrap();

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

        let (body, _violations) = result.unwrap();
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

        let (body, _violations) = result.unwrap();
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
    /// NOTE (EPIC-011): Updated to test thinkingLevel instead of thinkingBudget for Gemini 3.x
    #[test]
    fn test_gemini_pro_thinking_budget_limits() {
        let mut req = create_basic_request("gemini-3-pro-high", true);
        req.thinking = Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(64000), // Превышает максимум 32000
        });

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _violations) = result.unwrap();
        let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];

        // EPIC-011: Gemini 3.x uses thinkingLevel (NOT thinkingBudget)
        assert!(
            thinking_config["thinkingLevel"].is_string(),
            "Gemini 3 must use thinkingLevel"
        );
        assert!(
            thinking_config["thinkingBudget"].is_null(),
            "Gemini 3 must NOT have thinkingBudget"
        );

        // Budget 64000 clamps to 32000, which maps to HIGH
        let level = thinking_config["thinkingLevel"].as_str().unwrap();
        assert_eq!(
            level, "HIGH",
            "Budget >32000 should clamp to 32000, then map to HIGH"
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

        let (body, _violations) = result.unwrap();

        // Model сохраняется с -thinking суффиксом
        assert_eq!(body["model"].as_str(), Some("claude-sonnet-4-5-thinking"));

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

        let (body, _violations) = result.unwrap();
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

        let (body, _violations) = result.unwrap();
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

    // ==================================================================================
    // LOW TIER SPECIFIC TESTS (Story-009-05)
    // ==================================================================================

    /// Test 1: Direct routing for gemini-3-pro-low
    /// Validates that gemini-3-pro-low routes correctly without fallback to High tier
    #[test]
    fn test_gemini_3_pro_low_routing() {
        let req = create_basic_request("gemini-3-pro-low", false);
        let result = transform_claude_request_in(&req, "test-project");

        assert!(result.is_ok(), "Request should succeed");

        let (body, _violations) = result.unwrap();

        // Verify model routes to gemini-3-pro-low (not High tier fallback)
        assert_eq!(
            body["model"].as_str(),
            Some("gemini-3-pro-low"),
            "Model should be gemini-3-pro-low"
        );
    }

    /// Test 2: Budget equality between Low and High tiers
    /// CRITICAL: Validates that Low tier has SAME 32000 thinking budget as High tier
    /// This is the key value proposition of Low tier!
    #[test]
    /// NOTE (EPIC-011): Updated to test thinkingLevel instead of thinkingBudget for Gemini 3.x
    fn test_gemini_3_pro_low_thinking_budget_same_as_high() {
        // Create requests for both tiers with excessive budget (64000 > 32000 max)
        let mut req_low = create_basic_request("gemini-3-pro-low", true);
        let mut req_high = create_basic_request("gemini-3-pro-high", true);

        req_low.thinking = Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(64000), // Exceeds 32000 limit
        });
        req_high.thinking = req_low.thinking.clone();

        let result_low = transform_claude_request_in(&req_low, "test-project");
        let result_high = transform_claude_request_in(&req_high, "test-project");

        assert!(result_low.is_ok());
        assert!(result_high.is_ok());

        let (body_low, _) = result_low.unwrap();
        let (body_high, _) = result_high.unwrap();

        // EPIC-011: Gemini 3.x uses thinkingLevel (NOT thinkingBudget)
        let level_low = body_low["request"]["generationConfig"]["thinkingConfig"]
            ["thinkingLevel"]
            .as_str()
            .unwrap();
        let level_high = body_high["request"]["generationConfig"]["thinkingConfig"]
            ["thinkingLevel"]
            .as_str()
            .unwrap();

        // CRITICAL: Both tiers have SAME thinking level
        assert_eq!(
            level_low, level_high,
            "Low tier level ({}) should equal High tier level ({})",
            level_low, level_high
        );
        assert_eq!(level_low, "HIGH", "Both tiers should map to HIGH (budget 64000 → 32000 → HIGH)");
    }

    /// Test 3: Thinking configuration with 16000 budget for Low tier
    /// Validates that Low tier correctly handles 16000 thinking budget
    /// (This is the default budget used by OpenAI auto-injection for ends_with("-low") models)
    /// NOTE (EPIC-011): Updated to test thinkingLevel instead of thinkingBudget for Gemini 3.x
    #[test]
    fn test_gemini_3_pro_low_thinking_budget_16000() {
        // Create request with 16000 thinking budget (OpenAI auto-injection default)
        let mut req = create_basic_request("gemini-3-pro-low", true);
        req.thinking = Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(16000), // Default budget from OpenAI auto-injection
        });

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok(), "Request should succeed");

        let (body, _violations) = result.unwrap();

        // Check that thinkingConfig is present
        let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];
        assert!(
            !thinking_config.is_null(),
            "thinkingConfig should be present for Low tier with thinking enabled"
        );

        // EPIC-011: Gemini 3.x uses thinkingLevel (NOT thinkingBudget)
        assert!(
            thinking_config["thinkingLevel"].is_string(),
            "Gemini 3 Pro Low must use thinkingLevel"
        );
        assert!(
            thinking_config["thinkingBudget"].is_null(),
            "Gemini 3 Pro Low must NOT have thinkingBudget"
        );

        // Budget 16000 maps to LOW for Pro models (threshold is 16000)
        let level = thinking_config["thinkingLevel"].as_str().unwrap();
        assert_eq!(
            level, "LOW",
            "Budget 16000 should map to LOW for Pro (threshold is 16000)"
        );

        // Verify includeThoughts is true
        assert_eq!(
            thinking_config["includeThoughts"].as_bool(),
            Some(true),
            "includeThoughts should be true"
        );
    }

    /// Test 4: Alias routing for Low tier
    /// Validates that both "gemini-low" and "gemini-3-low" aliases route correctly
    #[test]
    fn test_gemini_low_aliases() {
        let aliases = vec!["gemini-low", "gemini-3-low"];

        for model_alias in aliases {
            let req = create_basic_request(model_alias, false);
            let result = transform_claude_request_in(&req, "test-project");

            assert!(
                result.is_ok(),
                "Request with alias '{}' should succeed",
                model_alias
            );

            let (body, _violations) = result.unwrap();
            let model = body["model"].as_str().unwrap();

            // Both aliases should resolve to gemini-3-pro-low
            assert_eq!(
                model, "gemini-3-pro-low",
                "Alias '{}' should resolve to gemini-3-pro-low",
                model_alias
            );
        }
    }

    /// Test 5: Model ID mapping for Low tier
    /// Validates that gemini-3-pro-low uses Model ID = 0 (name-based routing)
    /// This is the architectural decision from Story-009-02
    #[test]
    fn test_gemini_3_pro_low_model_id_mapping() {
        use crate::proxy::mappers::claude::request::get_model_id;

        // Test base model
        let model_id = get_model_id("gemini-3-pro-low");
        assert_eq!(
            model_id, 0,
            "gemini-3-pro-low should use Model ID 0 (name-based routing)"
        );

        // Test thinking variant (should also use 0)
        let thinking_id = get_model_id("gemini-3-pro-low-thinking");
        assert_eq!(
            thinking_id, 0,
            "gemini-3-pro-low-thinking should use Model ID 0 (name-based routing)"
        );
    }
}
