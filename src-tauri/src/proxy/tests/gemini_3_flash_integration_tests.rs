//! Integration tests for Gemini 3 Flash auto-injection and 4-level thinking support
//!
//! Tests Flash-specific behavior across both OpenAI and Claude protocols:
//! 1. Auto-injection of thinking config (even when not requested)
//! 2. 4-level thinking mapping (MINIMAL/LOW/MEDIUM/HIGH)
//! 3. Model-specific defaults (Flash: MEDIUM, Pro: HIGH)
//! 4. Backward compatibility with Gemini 2.5 Flash

#[cfg(test)]
mod flash_integration_tests {
    use crate::proxy::mappers::claude::models::{ClaudeRequest, MessageContent, ThinkingConfig};
    use crate::proxy::mappers::claude::request::transform_claude_request_in;
    use crate::proxy::mappers::openai::models::{
        OpenAIContent, OpenAIMessage, OpenAIRequest,
    };
    use crate::proxy::mappers::openai::request::transform_openai_request;

    // ==================================================================================
    // TEST HELPERS
    // ==================================================================================

    /// Create OpenAI request for testing (no thinking parameters)
    fn create_openai_request(model: &str) -> OpenAIRequest {
        OpenAIRequest {
            model: model.to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Hello".to_string())),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }],
            prompt: None,
            stream: false,
            n: None,
            max_tokens: Some(1024),
            temperature: Some(1.0),
            top_p: None,
            stop: None,
            response_format: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            instructions: None,
            input: None,
        }
    }

    /// Create Claude request for testing
    fn create_claude_request(model: &str, budget: Option<u32>) -> ClaudeRequest {
        let thinking = budget.map(|b| ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(b),
        });

        ClaudeRequest {
            model: model.to_string(),
            messages: vec![crate::proxy::mappers::claude::models::Message {
                role: "user".to_string(),
                content: MessageContent::String("Hello".to_string()),
            }],
            max_tokens: Some(1024),
            thinking,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: false,
            system: None,
            tools: None,
            tool_choice: None,
            metadata: None,
            output_config: None,
        }
    }

    // ==================================================================================
    // OPENAI PROTOCOL FLASH AUTO-INJECTION TESTS
    // ==================================================================================

    /// Test 1: OpenAI protocol auto-injects thinking for Flash (default MEDIUM)
    #[test]
    fn test_flash_auto_injection_openai_protocol_default() {
        let req = create_openai_request("gemini-3-flash");

        let result = transform_openai_request(&req, "test-project", "gemini-3-flash");
        assert!(result.is_ok(), "Transform should succeed");

        let body = result.unwrap();

        // ASSERT: thinkingConfig must be present even though not requested
        assert!(
            body["request"]["generationConfig"]["thinkingConfig"].is_object(),
            "OpenAI protocol should auto-inject thinkingConfig for Flash"
        );
        assert_eq!(
            body["request"]["generationConfig"]["thinkingConfig"]["includeThoughts"], true,
            "includeThoughts should be true"
        );

        // Flash default should be MEDIUM (balance cost/quality)
        let level = body["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"]
            .as_str()
            .unwrap();
        assert_eq!(
            level, "MEDIUM",
            "Flash default should be MEDIUM for OpenAI protocol"
        );
    }

    /// Test 2: OpenAI protocol Flash uses thinkingLevel (NOT thinkingBudget)
    #[test]
    fn test_flash_openai_uses_thinking_level_not_budget() {
        let req = create_openai_request("gemini-3-flash");

        let result = transform_openai_request(&req, "test-project", "gemini-3-flash");
        assert!(result.is_ok(), "Transform should succeed");

        let body = result.unwrap();
        let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];

        // CRITICAL: Flash should use thinkingLevel (NOT thinkingBudget)
        assert!(
            thinking_config["thinkingLevel"].is_string(),
            "Flash should use thinkingLevel (enum)"
        );
        assert!(
            thinking_config["thinkingBudget"].is_null(),
            "Flash should NOT have thinkingBudget"
        );
    }

    // ==================================================================================
    // CLAUDE PROTOCOL FLASH 4-LEVEL MAPPING TESTS
    // ==================================================================================

    /// Test 3: Flash uses 4-level mapping for Claude protocol
    #[test]
    fn test_flash_4_level_mapping_claude_protocol() {
        let test_cases = vec![
            (2000, "MINIMAL"),  // 0-4000
            (4000, "MINIMAL"),  // boundary
            (4001, "LOW"),      // 4001-10000
            (7000, "LOW"),      // middle
            (10000, "LOW"),     // boundary
            (10001, "MEDIUM"),  // 10001-20000 (Flash exclusive!)
            (15000, "MEDIUM"),  // middle
            (20000, "MEDIUM"),  // boundary
            (20001, "HIGH"),    // 20001+
            (25000, "HIGH"),    // high
            (32000, "HIGH"),    // max
        ];

        for (budget, expected_level) in test_cases {
            let req = create_claude_request("gemini-3-flash", Some(budget));
            let result = transform_claude_request_in(&req, "test-project");

            assert!(result.is_ok(), "Transform should succeed for budget {}", budget);

            let (body, _) = result.unwrap();
            let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];

            assert!(
                thinking_config["thinkingLevel"].is_string(),
                "Flash should use thinkingLevel for budget {}",
                budget
            );

            let actual_level = thinking_config["thinkingLevel"].as_str().unwrap();
            assert_eq!(
                actual_level, expected_level,
                "Budget {} should map to {} (got {})",
                budget, expected_level, actual_level
            );

            // Verify thinkingBudget is NOT present
            assert!(
                thinking_config["thinkingBudget"].is_null(),
                "Flash should NOT have thinkingBudget for budget {}",
                budget
            );
        }
    }

    /// Test 4: Flash MEDIUM level is exclusive (Pro models don't support it)
    #[test]
    fn test_flash_medium_level_exclusive() {
        // Flash should support MEDIUM
        let flash_req = create_claude_request("gemini-3-flash", Some(15000));
        let flash_result = transform_claude_request_in(&flash_req, "test-project");
        assert!(flash_result.is_ok());

        let (flash_body, _) = flash_result.unwrap();
        let flash_level = flash_body["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"]
            .as_str()
            .unwrap();
        assert_eq!(
            flash_level, "MEDIUM",
            "Flash should support MEDIUM level for budget 15000"
        );

        // Pro should use LOW for same budget (Pro doesn't support MEDIUM)
        let pro_req = create_claude_request("gemini-3-pro-high", Some(15000));
        let pro_result = transform_claude_request_in(&pro_req, "test-project");
        assert!(pro_result.is_ok());

        let (pro_body, _) = pro_result.unwrap();
        let pro_level = pro_body["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"]
            .as_str()
            .unwrap();
        assert_eq!(
            pro_level, "LOW",
            "Pro should use LOW (not MEDIUM) for budget 15000"
        );
    }

    /// Test 5: Flash adaptive budget uses thinkingLevel (when thinking enabled)
    /// Note: Claude protocol requires explicit thinking config, unlike OpenAI auto-injection
    #[test]
    fn test_flash_adaptive_budget_claude_protocol() {
        // Create request WITH thinking enabled but NO explicit budget
        // This triggers adaptive budget optimizer which calculates optimal budget
        let mut req = create_claude_request("gemini-3-flash", None);
        req.thinking = Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: None, // No explicit budget - triggers adaptive calculation
        });

        let result = transform_claude_request_in(&req, "test-project");

        assert!(result.is_ok(), "Transform should succeed for adaptive budget");

        let (body, _) = result.unwrap();
        let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];

        // Should use thinkingLevel (not thinkingBudget)
        assert!(
            thinking_config["thinkingLevel"].is_string(),
            "Flash should use thinkingLevel for adaptive budget"
        );

        // Level should be one of the 4 valid Flash levels
        let level = thinking_config["thinkingLevel"].as_str().unwrap();
        assert!(
            level == "MINIMAL" || level == "LOW" || level == "MEDIUM" || level == "HIGH",
            "Flash adaptive should use valid level (got {})",
            level
        );

        // Verify thinkingBudget is NOT present
        assert!(
            thinking_config["thinkingBudget"].is_null(),
            "Flash should NOT have thinkingBudget even with adaptive budget"
        );
    }

    // ==================================================================================
    // PRO MODEL COMPARISON TESTS (2-level vs 4-level)
    // ==================================================================================

    /// Test 6: Pro models use 2-level mapping (LOW/HIGH only)
    #[test]
    fn test_pro_2_level_mapping_comparison() {
        let test_cases = vec![
            ("gemini-3-pro-high", vec![(8000, "LOW"), (16000, "LOW"), (16001, "HIGH"), (25000, "HIGH")]),
            ("gemini-3-pro-low", vec![(8000, "LOW"), (16000, "LOW"), (16001, "HIGH"), (25000, "HIGH")]),
        ];

        for (model, budgets) in test_cases {
            for (budget, expected_level) in budgets {
                let req = create_claude_request(model, Some(budget));
                let result = transform_claude_request_in(&req, "test-project");

                assert!(result.is_ok(), "Transform should succeed for {} budget {}", model, budget);

                let (body, _) = result.unwrap();
                let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];

                let actual_level = thinking_config["thinkingLevel"].as_str().unwrap();
                assert_eq!(
                    actual_level, expected_level,
                    "{} budget {} should map to {} (got {})",
                    model, budget, expected_level, actual_level
                );

                // Verify Pro NEVER returns MEDIUM or MINIMAL
                assert_ne!(
                    actual_level, "MEDIUM",
                    "{} should NEVER return MEDIUM",
                    model
                );
                assert_ne!(
                    actual_level, "MINIMAL",
                    "{} should NEVER return MINIMAL",
                    model
                );
            }
        }
    }

    /// Test 7: OpenAI protocol Pro models use default HIGH
    #[test]
    fn test_pro_openai_default_high() {
        let pro_models = vec!["gemini-3-pro-high", "gemini-3-pro-low"];

        for model in pro_models {
            let req = create_openai_request(model);
            let result = transform_openai_request(&req, "test-project", model);
            assert!(result.is_ok(), "Transform should succeed for {}", model);

            let body = result.unwrap();
            let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];

            // Pro default should be HIGH (maximize quality)
            let level = thinking_config["thinkingLevel"].as_str().unwrap();
            assert_eq!(
                level, "HIGH",
                "{} default should be HIGH for OpenAI protocol",
                model
            );
        }
    }

    // ==================================================================================
    // BACKWARD COMPATIBILITY TESTS (Gemini 2.5 Flash)
    // ==================================================================================

    /// Test 8: Gemini 2.5 Flash does NOT use thinkingLevel
    #[test]
    fn test_gemini_2_5_flash_backward_compatibility() {
        let req = create_claude_request("gemini-2.5-flash", Some(16000));
        let result = transform_claude_request_in(&req, "test-project");

        assert!(result.is_ok(), "Transform should succeed for Gemini 2.5 Flash");

        let (body, _) = result.unwrap();
        let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];

        // CRITICAL: Gemini 2.5 MUST use thinkingBudget (NOT thinkingLevel)
        assert!(
            thinking_config["thinkingBudget"].is_number(),
            "Gemini 2.5 Flash MUST use thinkingBudget"
        );
        assert!(
            thinking_config["thinkingLevel"].is_null(),
            "Gemini 2.5 Flash MUST NOT have thinkingLevel"
        );

        let budget = thinking_config["thinkingBudget"].as_i64().unwrap();
        assert_eq!(budget, 16000, "Gemini 2.5 should preserve budget value");
    }

    /// Test 9: Gemini 2.5 Flash-Thinking uses thinkingBudget
    #[test]
    fn test_gemini_2_5_flash_thinking_backward_compatibility() {
        let req = create_claude_request("gemini-2.5-flash-thinking", Some(20000));
        let result = transform_claude_request_in(&req, "test-project");

        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];

        // Gemini 2.5 Flash-Thinking uses thinkingBudget
        assert!(
            thinking_config["thinkingBudget"].is_number(),
            "Gemini 2.5 Flash-Thinking should use thinkingBudget"
        );
        assert!(
            thinking_config["thinkingLevel"].is_null(),
            "Gemini 2.5 Flash-Thinking should NOT use thinkingLevel"
        );
    }

    // ==================================================================================
    // EDGE CASE TESTS
    // ==================================================================================

    /// Test 10: Budget clamping to 32000 max
    #[test]
    fn test_flash_budget_clamping() {
        let test_cases = vec![
            (35000, "HIGH"), // Over max, clamps to 32000 → HIGH
            (50000, "HIGH"), // Way over max → HIGH
            (100000, "HIGH"), // Extreme → HIGH
        ];

        for (budget, expected_level) in test_cases {
            let req = create_claude_request("gemini-3-flash", Some(budget));
            let result = transform_claude_request_in(&req, "test-project");

            assert!(result.is_ok(), "Transform should succeed for budget {}", budget);

            let (body, _) = result.unwrap();
            let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];

            let actual_level = thinking_config["thinkingLevel"].as_str().unwrap();
            assert_eq!(
                actual_level, expected_level,
                "Budget {} (clamped to 32000) should map to {}",
                budget, expected_level
            );
        }
    }

    /// Test 11: Zero budget handling
    #[test]
    fn test_flash_zero_budget() {
        let req = create_claude_request("gemini-3-flash", Some(0));
        let result = transform_claude_request_in(&req, "test-project");

        assert!(result.is_ok(), "Transform should succeed for zero budget");

        let (body, _) = result.unwrap();
        let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];

        let level = thinking_config["thinkingLevel"].as_str().unwrap();
        assert_eq!(
            level, "MINIMAL",
            "Zero budget should map to MINIMAL for Flash"
        );
    }

    /// Test 12: OpenAI protocol doesn't accidentally use thinkingBudget
    #[test]
    fn test_openai_flash_no_thinking_budget() {
        let req = create_openai_request("gemini-3-flash");
        let result = transform_openai_request(&req, "test-project", "gemini-3-flash");
        assert!(result.is_ok(), "Transform should succeed");

        let body = result.unwrap();
        let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];

        // CRITICAL: OpenAI protocol should NEVER produce thinkingBudget for Gemini 3
        assert!(
            thinking_config["thinkingBudget"].is_null(),
            "OpenAI protocol should NOT produce thinkingBudget for Gemini 3 Flash"
        );
        assert!(
            thinking_config["thinkingLevel"].is_string(),
            "OpenAI protocol should use thinkingLevel for Gemini 3 Flash"
        );
    }
}
