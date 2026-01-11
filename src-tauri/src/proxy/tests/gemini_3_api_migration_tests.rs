//! Comprehensive test suite for Gemini 3 API Migration (Epic-011 Story-011-01)
//!
//! Tests verify:
//! - Gemini 3.x models use `thinkingLevel` API (NOT `thinkingBudget`)
//! - Gemini 2.5 models continue using `thinkingBudget` API (backward compatibility)
//! - Budget-to-level mapping works correctly
//! - All Gemini 3 variants detected (Flash, Pro High, Pro Low)

#[cfg(test)]
mod tests {
    use crate::proxy::mappers::claude::models::{
        ClaudeRequest, Message, MessageContent, ThinkingConfig,
    };
    use crate::proxy::mappers::claude::request::transform_claude_request_in;

    fn create_test_request(model: &str, enable_thinking: bool) -> ClaudeRequest {
        ClaudeRequest {
            model: model.to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Test message".to_string()),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(8192),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: if enable_thinking {
                Some(ThinkingConfig {
                    type_: "enabled".to_string(),
                    budget_tokens: None,
                })
            } else {
                None
            },
            metadata: None,
            output_config: None,
            tool_choice: None,
        }
    }

    // ==================================================================================
    // GEMINI 3.x API TESTS (thinkingLevel)
    // ==================================================================================

    /// Test 1: Gemini 3 Pro High uses thinkingLevel (NOT thinkingBudget)
    #[test]
    fn test_gemini_3_pro_high_uses_thinking_level() {
        let mut req = create_test_request("gemini-3-pro-high", true);
        req.thinking = Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(20000), // Should map to HIGH
        });

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];

        // CRITICAL: Must use thinkingLevel (NOT thinkingBudget)
        assert!(
            thinking_config["thinkingLevel"].is_string(),
            "Gemini 3 Pro High MUST use thinkingLevel"
        );
        assert!(
            thinking_config["thinkingBudget"].is_null(),
            "Gemini 3 Pro High MUST NOT have thinkingBudget"
        );

        let level = thinking_config["thinkingLevel"].as_str().unwrap();
        assert_eq!(level, "HIGH", "Budget 20000 should map to HIGH");
    }

    /// Test 2: Gemini 3 Pro Low uses thinkingLevel (NOT thinkingBudget)
    #[test]
    fn test_gemini_3_pro_low_uses_thinking_level() {
        let mut req = create_test_request("gemini-3-pro-low", true);
        req.thinking = Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(10000), // Should map to LOW
        });

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];

        // CRITICAL: Must use thinkingLevel (NOT thinkingBudget)
        assert!(
            thinking_config["thinkingLevel"].is_string(),
            "Gemini 3 Pro Low MUST use thinkingLevel"
        );
        assert!(
            thinking_config["thinkingBudget"].is_null(),
            "Gemini 3 Pro Low MUST NOT have thinkingBudget"
        );

        let level = thinking_config["thinkingLevel"].as_str().unwrap();
        assert_eq!(level, "LOW", "Budget 10000 should map to LOW");
    }

    /// Test 3: Gemini 3 Flash uses thinkingLevel with 4-level support
    #[test]
    fn test_gemini_3_flash_uses_thinking_level_4_levels() {
        // Test all 4 Flash levels
        let test_cases = vec![
            (2000, "MINIMAL"),
            (7000, "LOW"),
            (15000, "MEDIUM"),
            (25000, "HIGH"),
        ];

        for (budget, expected_level) in test_cases {
            let mut req = create_test_request("gemini-3-flash", true);
            req.thinking = Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(budget),
            });

            let result = transform_claude_request_in(&req, "test-project");
            assert!(result.is_ok());

            let (body, _) = result.unwrap();
            let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];

            // CRITICAL: Must use thinkingLevel (NOT thinkingBudget)
            assert!(
                thinking_config["thinkingLevel"].is_string(),
                "Gemini 3 Flash MUST use thinkingLevel"
            );
            assert!(
                thinking_config["thinkingBudget"].is_null(),
                "Gemini 3 Flash MUST NOT have thinkingBudget"
            );

            let level = thinking_config["thinkingLevel"].as_str().unwrap();
            assert_eq!(
                level, expected_level,
                "Budget {} should map to {}",
                budget, expected_level
            );
        }
    }

    /// Test 4: Gemini 3 Pro models do NOT support MEDIUM level
    #[test]
    fn test_gemini_3_pro_no_medium_level() {
        // Test Pro High with budget that would be MEDIUM for Flash
        let mut req = create_test_request("gemini-3-pro-high", true);
        req.thinking = Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(15000), // Would be MEDIUM for Flash, but Pro only has LOW/HIGH
        });

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let level = body["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"]
            .as_str()
            .unwrap();

        // CRITICAL: Pro should use LOW (not MEDIUM)
        assert_ne!(level, "MEDIUM", "Pro MUST NOT use MEDIUM level");
        assert_eq!(
            level, "LOW",
            "Budget 15000 should map to LOW for Pro (not MEDIUM)"
        );
    }

    /// Test 5: Budget clamping for Gemini 3 models
    #[test]
    fn test_gemini_3_budget_clamping() {
        let mut req = create_test_request("gemini-3-pro-high", true);
        req.thinking = Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(64000), // Exceeds max 32000
        });

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let level = body["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"]
            .as_str()
            .unwrap();

        // Budget 64000 should clamp to 32000, then map to HIGH
        assert_eq!(level, "HIGH", "Budget >32000 should clamp to 32000, then map to HIGH");
    }

    /// Test 6: Gemini 3 Pro with adaptive budget (typical use case)
    /// Note: When budget_tokens is None, the adaptive budget optimizer calculates
    /// an optimal budget based on prompt complexity. For test messages, this typically
    /// results in a low budget that maps to LOW level.
    #[test]
    fn test_gemini_3_pro_adaptive_budget() {
        let req = create_test_request("gemini-3-pro-high", true);
        // No budget_tokens specified - adaptive optimizer will calculate

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];

        // Should use thinkingLevel (not thinkingBudget)
        assert!(
            thinking_config["thinkingLevel"].is_string(),
            "Pro should use thinkingLevel"
        );
        assert!(
            thinking_config["thinkingBudget"].is_null(),
            "Pro should NOT have thinkingBudget"
        );

        // Adaptive optimizer for test message typically results in LOW level
        let level = thinking_config["thinkingLevel"].as_str().unwrap();
        assert!(
            level == "LOW" || level == "HIGH",
            "Pro should use either LOW or HIGH (got {})",
            level
        );
    }

    /// Test 7: Gemini 3 Flash with adaptive budget (typical use case)
    /// Note: When budget_tokens is None, the adaptive budget optimizer calculates
    /// an optimal budget based on prompt complexity. For test messages, this typically
    /// results in a low budget that maps to MINIMAL level.
    #[test]
    fn test_gemini_3_flash_adaptive_budget() {
        let req = create_test_request("gemini-3-flash", true);
        // No budget_tokens specified - adaptive optimizer will calculate

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];

        // Should use thinkingLevel (not thinkingBudget)
        assert!(
            thinking_config["thinkingLevel"].is_string(),
            "Flash should use thinkingLevel"
        );
        assert!(
            thinking_config["thinkingBudget"].is_null(),
            "Flash should NOT have thinkingBudget"
        );

        // Adaptive optimizer for test message can result in any Flash level
        let level = thinking_config["thinkingLevel"].as_str().unwrap();
        assert!(
            level == "MINIMAL" || level == "LOW" || level == "MEDIUM" || level == "HIGH",
            "Flash should use valid level (got {})",
            level
        );
    }

    // ==================================================================================
    // BACKWARD COMPATIBILITY TESTS (Gemini 2.5)
    // ==================================================================================

    /// Test 8: Gemini 2.5 Flash continues using thinkingBudget
    #[test]
    fn test_gemini_2_5_flash_backward_compatibility() {
        let mut req = create_test_request("gemini-2.5-flash", true);
        req.thinking = Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(16000),
        });

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

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

    /// Test 9: Gemini 2.5 Flash Thinking continues using thinkingBudget
    #[test]
    fn test_gemini_2_5_flash_thinking_backward_compatibility() {
        let mut req = create_test_request("gemini-2.5-flash-thinking", true);
        req.thinking = Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(20000),
        });

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];

        // CRITICAL: Gemini 2.5 MUST use thinkingBudget (NOT thinkingLevel)
        assert!(
            thinking_config["thinkingBudget"].is_number(),
            "Gemini 2.5 Flash Thinking MUST use thinkingBudget"
        );
        assert!(
            thinking_config["thinkingLevel"].is_null(),
            "Gemini 2.5 Flash Thinking MUST NOT have thinkingLevel"
        );
    }

    /// Test 10: Gemini 2.5 Pro Thinking continues using thinkingBudget
    #[test]
    fn test_gemini_2_5_pro_thinking_backward_compatibility() {
        let mut req = create_test_request("gemini-2.5-pro-thinking", true);
        req.thinking = Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(25000),
        });

        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];

        // CRITICAL: Gemini 2.5 MUST use thinkingBudget (NOT thinkingLevel)
        assert!(
            thinking_config["thinkingBudget"].is_number(),
            "Gemini 2.5 Pro Thinking MUST use thinkingBudget"
        );
        assert!(
            thinking_config["thinkingLevel"].is_null(),
            "Gemini 2.5 Pro Thinking MUST NOT have thinkingLevel"
        );
    }

    // ==================================================================================
    // EDGE CASES
    // ==================================================================================

    /// Test 11: Gemini 3 Pro Low and Pro High have same thinking capabilities
    #[test]
    fn test_gemini_3_pro_tiers_equal_thinking() {
        let mut req_low = create_test_request("gemini-3-pro-low", true);
        let mut req_high = create_test_request("gemini-3-pro-high", true);

        req_low.thinking = Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(25000), // Should map to HIGH for both
        });
        req_high.thinking = req_low.thinking.clone();

        let result_low = transform_claude_request_in(&req_low, "test-project");
        let result_high = transform_claude_request_in(&req_high, "test-project");

        assert!(result_low.is_ok());
        assert!(result_high.is_ok());

        let (body_low, _) = result_low.unwrap();
        let (body_high, _) = result_high.unwrap();

        let level_low = body_low["request"]["generationConfig"]["thinkingConfig"]
            ["thinkingLevel"]
            .as_str()
            .unwrap();
        let level_high = body_high["request"]["generationConfig"]["thinkingConfig"]
            ["thinkingLevel"]
            .as_str()
            .unwrap();

        // CRITICAL: Both tiers should have same thinking level
        assert_eq!(
            level_low, level_high,
            "Low and High tiers should have equal thinking capabilities"
        );
        assert_eq!(level_low, "HIGH", "Both should map to HIGH");
    }

    /// Test 12: Zero budget edge case
    #[test]
    fn test_gemini_3_zero_budget() {
        let mut req_flash = create_test_request("gemini-3-flash", true);
        let mut req_pro = create_test_request("gemini-3-pro-high", true);

        req_flash.thinking = Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(0),
        });
        req_pro.thinking = req_flash.thinking.clone();

        let result_flash = transform_claude_request_in(&req_flash, "test-project");
        let result_pro = transform_claude_request_in(&req_pro, "test-project");

        assert!(result_flash.is_ok());
        assert!(result_pro.is_ok());

        let (body_flash, _) = result_flash.unwrap();
        let (body_pro, _) = result_pro.unwrap();

        let level_flash = body_flash["request"]["generationConfig"]["thinkingConfig"]
            ["thinkingLevel"]
            .as_str()
            .unwrap();
        let level_pro = body_pro["request"]["generationConfig"]["thinkingConfig"]
            ["thinkingLevel"]
            .as_str()
            .unwrap();

        assert_eq!(level_flash, "MINIMAL", "Flash with budget 0 should be MINIMAL");
        assert_eq!(level_pro, "LOW", "Pro with budget 0 should be LOW");
    }
}
