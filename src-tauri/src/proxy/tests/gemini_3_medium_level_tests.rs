//! Epic-013 Story-013-01: MEDIUM Level Test Coverage
//!
//! Comprehensive test suite for Gemini 3 Flash MEDIUM thinking level validation.
//! MEDIUM level (10001-20000 tokens) is FLASH EXCLUSIVE - Pro models don't support it.
//!
//! Test Coverage:
//! - MEDIUM level works on Flash (gemini-3-flash)
//! - Pro models (gemini-3-pro-high, gemini-3-pro-low) downgrade MEDIUM → LOW
//! - Budget boundaries: 10000, 10001, 15000, 20000, 20001
//! - Cross-protocol consistency (OpenAI, Claude, Gemini native)
//! - All 3 protocols produce identical MEDIUM level mappings

#[cfg(test)]
mod medium_level_tests {
    use crate::proxy::mappers::claude::models::{ClaudeRequest, MessageContent, ThinkingConfig};
    use crate::proxy::mappers::claude::request::transform_claude_request_in;
    use crate::proxy::mappers::openai::models::{OpenAIContent, OpenAIMessage, OpenAIRequest};
    use crate::proxy::mappers::openai::request::transform_openai_request;

    // ==================================================================================
    // TEST HELPERS
    // ==================================================================================

    /// Create Claude request with specific thinking budget
    fn create_claude_request_with_budget(model: &str, budget: u32) -> ClaudeRequest {
        ClaudeRequest {
            model: model.to_string(),
            messages: vec![crate::proxy::mappers::claude::models::Message {
                role: "user".to_string(),
                content: MessageContent::String("Test MEDIUM level".to_string()),
            }],
            max_tokens: Some(1024),
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(budget),
            }),
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

    /// Create OpenAI request with reasoning_effort
    fn create_openai_request_with_effort(
        model: &str,
        reasoning_effort: Option<String>,
    ) -> OpenAIRequest {
        OpenAIRequest {
            model: model.to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Test MEDIUM level".to_string())),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }],
            prompt: None,
            stream: false,
            n: None,
            max_tokens: Some(1024),
            temperature: None,
            top_p: None,
            stop: None,
            response_format: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            reasoning_effort,
            instructions: None,
            input: None,
        }
    }

    /// Extract thinking level from transformed Gemini request
    fn extract_thinking_level(body: &serde_json::Value) -> Option<String> {
        body.get("request")
            .and_then(|req| req.get("generationConfig"))
            .and_then(|gc| gc.get("thinkingConfig"))
            .and_then(|tc| tc.get("thinkingLevel"))
            .and_then(|level| level.as_str())
            .map(|s| s.to_string())
    }

    // ==================================================================================
    // TEST 1: MEDIUM LEVEL EXCLUSIVE TO FLASH
    // ==================================================================================

    #[test]
    fn test_medium_level_flash_exclusive_claude_protocol() {
        // GIVEN: Claude request with MEDIUM-range budget (15000 tokens)
        let claude_request = create_claude_request_with_budget("gemini-3-flash", 15000);

        // WHEN: Mapped to gemini-3-flash
        let flash_result = transform_claude_request_in(&claude_request, "test-project");
        assert!(
            flash_result.is_ok(),
            "Flash should accept MEDIUM budget via Claude protocol"
        );

        let (flash_body, _) = flash_result.unwrap();
        let flash_level = extract_thinking_level(&flash_body);

        // THEN: Flash uses MEDIUM level
        assert_eq!(
            flash_level,
            Some("MEDIUM".to_string()),
            "Flash should support MEDIUM level for budget 15000"
        );

        // WHEN: Same request mapped to gemini-3-pro-high
        let pro_request = create_claude_request_with_budget("gemini-3-pro-high", 15000);
        let pro_result = transform_claude_request_in(&pro_request, "test-project");
        assert!(pro_result.is_ok(), "Pro should accept request");

        let (pro_body, _) = pro_result.unwrap();
        let pro_level = extract_thinking_level(&pro_body);

        // THEN: Pro downgrades MEDIUM → LOW (doesn't support MEDIUM)
        assert_eq!(
            pro_level,
            Some("LOW".to_string()),
            "Pro should downgrade MEDIUM budget to LOW level"
        );
    }

    #[test]
    fn test_medium_level_flash_exclusive_openai_protocol() {
        // GIVEN: OpenAI request with reasoning_effort="medium"
        let flash_request =
            create_openai_request_with_effort("gemini-3-flash", Some("medium".to_string()));

        // WHEN: Mapped to Flash
        let flash_result =
            transform_openai_request(&flash_request, "test-project", "gemini-3-flash");
        assert!(flash_result.is_ok());

        let flash_body = flash_result.unwrap();
        let flash_level = extract_thinking_level(&flash_body);

        // THEN: Flash supports MEDIUM
        assert_eq!(
            flash_level,
            Some("MEDIUM".to_string()),
            "Flash should support reasoning_effort='medium'"
        );

        // WHEN: Same request mapped to Pro
        let pro_request =
            create_openai_request_with_effort("gemini-3-pro-high", Some("medium".to_string()));
        let pro_result =
            transform_openai_request(&pro_request, "test-project", "gemini-3-pro-high");
        assert!(pro_result.is_ok());

        let pro_body = pro_result.unwrap();
        let pro_level = extract_thinking_level(&pro_body);

        // THEN: Pro downgrades to LOW
        assert_eq!(
            pro_level,
            Some("LOW".to_string()),
            "Pro should downgrade reasoning_effort='medium' to LOW"
        );
    }

    #[test]
    fn test_medium_level_pro_low_model_downgrade() {
        // Test that gemini-3-pro-low also downgrades MEDIUM → LOW
        let pro_low_request = create_claude_request_with_budget("gemini-3-pro-low", 15000);
        let result = transform_claude_request_in(&pro_low_request, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let level = extract_thinking_level(&body);

        assert_eq!(
            level,
            Some("LOW".to_string()),
            "gemini-3-pro-low should also downgrade MEDIUM to LOW"
        );
    }

    // ==================================================================================
    // TEST 2: MEDIUM BUDGET BOUNDARY MAPPING
    // ==================================================================================

    #[test]
    fn test_medium_budget_lower_boundary_10001() {
        // Budget 10001 is the LOWER boundary of MEDIUM range
        let request = create_claude_request_with_budget("gemini-3-flash", 10001);
        let result = transform_claude_request_in(&request, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let level = extract_thinking_level(&body);

        assert_eq!(
            level,
            Some("MEDIUM".to_string()),
            "Budget 10001 should map to MEDIUM (lower boundary)"
        );
    }

    #[test]
    fn test_medium_budget_upper_boundary_10000() {
        // Budget 10000 is the UPPER boundary of LOW range (NOT MEDIUM)
        let request = create_claude_request_with_budget("gemini-3-flash", 10000);
        let result = transform_claude_request_in(&request, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let level = extract_thinking_level(&body);

        assert_eq!(
            level,
            Some("LOW".to_string()),
            "Budget 10000 should map to LOW, not MEDIUM"
        );
    }

    #[test]
    fn test_medium_budget_mid_range_15000() {
        // Budget 15000 is in the MIDDLE of MEDIUM range (10001-20000)
        let request = create_claude_request_with_budget("gemini-3-flash", 15000);
        let result = transform_claude_request_in(&request, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let level = extract_thinking_level(&body);

        assert_eq!(
            level,
            Some("MEDIUM".to_string()),
            "Budget 15000 should map to MEDIUM (mid-range)"
        );
    }

    #[test]
    fn test_medium_budget_upper_boundary_20000() {
        // Budget 20000 is the UPPER boundary of MEDIUM range
        let request = create_claude_request_with_budget("gemini-3-flash", 20000);
        let result = transform_claude_request_in(&request, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let level = extract_thinking_level(&body);

        assert_eq!(
            level,
            Some("MEDIUM".to_string()),
            "Budget 20000 should map to MEDIUM (upper boundary)"
        );
    }

    #[test]
    fn test_medium_budget_lower_boundary_20001() {
        // Budget 20001 is the LOWER boundary of HIGH range (NOT MEDIUM)
        let request = create_claude_request_with_budget("gemini-3-flash", 20001);
        let result = transform_claude_request_in(&request, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let level = extract_thinking_level(&body);

        assert_eq!(
            level,
            Some("HIGH".to_string()),
            "Budget 20001 should map to HIGH, not MEDIUM"
        );
    }

    #[test]
    fn test_medium_budget_boundaries_comprehensive() {
        // Comprehensive boundary test: all 5 boundary points
        let test_cases = vec![
            (10000, "LOW", "upper boundary of LOW range"),
            (10001, "MEDIUM", "lower boundary of MEDIUM range"),
            (15000, "MEDIUM", "mid-point of MEDIUM range"),
            (20000, "MEDIUM", "upper boundary of MEDIUM range"),
            (20001, "HIGH", "lower boundary of HIGH range"),
        ];

        for (budget, expected_level, description) in test_cases {
            let request = create_claude_request_with_budget("gemini-3-flash", budget);
            let result = transform_claude_request_in(&request, "test-project");
            assert!(
                result.is_ok(),
                "Transform should succeed for {}",
                description
            );

            let (body, _) = result.unwrap();
            let actual_level = extract_thinking_level(&body);

            assert_eq!(
                actual_level,
                Some(expected_level.to_string()),
                "Budget {} should map to {} ({})",
                budget,
                expected_level,
                description
            );
        }
    }

    // ==================================================================================
    // TEST 3: CROSS-PROTOCOL CONSISTENCY FOR MEDIUM LEVEL
    // ==================================================================================

    #[test]
    fn test_medium_level_cross_protocol_consistency_flash() {
        // Same budget should produce same MEDIUM level across protocols

        // Claude protocol with budget 15000
        let claude_req = create_claude_request_with_budget("gemini-3-flash", 15000);
        let claude_result = transform_claude_request_in(&claude_req, "test-project");
        assert!(claude_result.is_ok());
        let (claude_body, _) = claude_result.unwrap();
        let claude_level = extract_thinking_level(&claude_body);

        // OpenAI protocol auto-injects MEDIUM for Flash (default)
        let openai_req = create_openai_request_with_effort("gemini-3-flash", None);
        let openai_result = transform_openai_request(&openai_req, "test-project", "gemini-3-flash");
        assert!(openai_result.is_ok());
        let openai_body = openai_result.unwrap();
        let openai_level = extract_thinking_level(&openai_body);

        // ASSERT: Both protocols should produce MEDIUM for Flash
        assert_eq!(
            claude_level,
            Some("MEDIUM".to_string()),
            "Claude protocol should produce MEDIUM for budget 15000"
        );
        assert_eq!(
            openai_level,
            Some("MEDIUM".to_string()),
            "OpenAI protocol should auto-inject MEDIUM for Flash"
        );
        assert_eq!(
            claude_level, openai_level,
            "Both protocols should produce identical MEDIUM level"
        );
    }

    #[test]
    fn test_medium_level_cross_protocol_consistency_pro_downgrade() {
        // Pro models should consistently downgrade MEDIUM → LOW across protocols

        // Claude protocol with MEDIUM-range budget
        let claude_req = create_claude_request_with_budget("gemini-3-pro-high", 15000);
        let claude_result = transform_claude_request_in(&claude_req, "test-project");
        assert!(claude_result.is_ok());
        let (claude_body, _) = claude_result.unwrap();
        let claude_level = extract_thinking_level(&claude_body);

        // OpenAI protocol with reasoning_effort="medium"
        let openai_req =
            create_openai_request_with_effort("gemini-3-pro-high", Some("medium".to_string()));
        let openai_result =
            transform_openai_request(&openai_req, "test-project", "gemini-3-pro-high");
        assert!(openai_result.is_ok());
        let openai_body = openai_result.unwrap();
        let openai_level = extract_thinking_level(&openai_body);

        // ASSERT: Both protocols should downgrade to LOW for Pro
        assert_eq!(
            claude_level,
            Some("LOW".to_string()),
            "Claude protocol should downgrade MEDIUM to LOW for Pro"
        );
        assert_eq!(
            openai_level,
            Some("LOW".to_string()),
            "OpenAI protocol should downgrade MEDIUM to LOW for Pro"
        );
        assert_eq!(
            claude_level, openai_level,
            "Both protocols should consistently downgrade to LOW"
        );
    }

    #[test]
    fn test_medium_level_openai_reasoning_effort_mapping() {
        // OpenAI reasoning_effort="medium" should map to MEDIUM for Flash
        let request =
            create_openai_request_with_effort("gemini-3-flash", Some("medium".to_string()));
        let result = transform_openai_request(&request, "test-project", "gemini-3-flash");
        assert!(result.is_ok());

        let body = result.unwrap();
        let level = extract_thinking_level(&body);

        assert_eq!(
            level,
            Some("MEDIUM".to_string()),
            "reasoning_effort='medium' should map to MEDIUM for Flash"
        );
    }

    // ==================================================================================
    // TEST 4: MEDIUM LEVEL VALIDATION ACROSS ALL FLASH MODELS
    // ==================================================================================

    #[test]
    fn test_medium_level_all_flash_variants() {
        // Test that Flash model supports MEDIUM level
        // Note: Only "gemini-3-flash" is the valid Gemini 3 Flash model name
        let request = create_claude_request_with_budget("gemini-3-flash", 15000);
        let result = transform_claude_request_in(&request, "test-project");
        assert!(
            result.is_ok(),
            "Transform should succeed for gemini-3-flash"
        );

        let (body, _) = result.unwrap();
        let level = extract_thinking_level(&body);

        assert_eq!(
            level,
            Some("MEDIUM".to_string()),
            "gemini-3-flash should support MEDIUM level for budget 15000"
        );
    }

    #[test]
    fn test_medium_level_all_pro_variants_downgrade() {
        // Test that all Pro model variants downgrade MEDIUM → LOW
        let pro_models = vec!["gemini-3-pro-high", "gemini-3-pro-low"];

        for model in pro_models {
            let request = create_claude_request_with_budget(model, 15000);
            let result = transform_claude_request_in(&request, "test-project");
            assert!(result.is_ok(), "Transform should succeed for {}", model);

            let (body, _) = result.unwrap();
            let level = extract_thinking_level(&body);

            assert_eq!(
                level,
                Some("LOW".to_string()),
                "{} should downgrade MEDIUM to LOW for budget 15000",
                model
            );
        }
    }

    // ==================================================================================
    // TEST 5: EDGE CASES FOR MEDIUM LEVEL
    // ==================================================================================

    #[test]
    fn test_medium_level_exact_boundaries() {
        // Test exact boundary transitions
        let boundaries = vec![
            (9999, "LOW", "just below MEDIUM range"),
            (10000, "LOW", "exactly at LOW upper boundary"),
            (10001, "MEDIUM", "exactly at MEDIUM lower boundary"),
            (20000, "MEDIUM", "exactly at MEDIUM upper boundary"),
            (20001, "HIGH", "exactly at HIGH lower boundary"),
            (20002, "HIGH", "just above MEDIUM range"),
        ];

        for (budget, expected, description) in boundaries {
            let request = create_claude_request_with_budget("gemini-3-flash", budget);
            let result = transform_claude_request_in(&request, "test-project");
            assert!(
                result.is_ok(),
                "Transform should succeed for budget {}",
                budget
            );

            let (body, _) = result.unwrap();
            let level = extract_thinking_level(&body);

            assert_eq!(
                level,
                Some(expected.to_string()),
                "Budget {} ({}) should map to {}",
                budget,
                description,
                expected
            );
        }
    }

    #[test]
    fn test_medium_level_pro_never_returns_medium() {
        // CRITICAL: Pro models should NEVER return MEDIUM for any budget
        let pro_budgets = vec![5000, 10000, 10001, 15000, 20000];

        for budget in pro_budgets {
            let pro_high_request = create_claude_request_with_budget("gemini-3-pro-high", budget);
            let pro_high_result = transform_claude_request_in(&pro_high_request, "test-project");
            assert!(pro_high_result.is_ok());
            let (pro_high_body, _) = pro_high_result.unwrap();
            let pro_high_level = extract_thinking_level(&pro_high_body);

            assert_ne!(
                pro_high_level,
                Some("MEDIUM".to_string()),
                "gemini-3-pro-high should NEVER return MEDIUM for budget {}",
                budget
            );

            let pro_low_request = create_claude_request_with_budget("gemini-3-pro-low", budget);
            let pro_low_result = transform_claude_request_in(&pro_low_request, "test-project");
            assert!(pro_low_result.is_ok());
            let (pro_low_body, _) = pro_low_result.unwrap();
            let pro_low_level = extract_thinking_level(&pro_low_body);

            assert_ne!(
                pro_low_level,
                Some("MEDIUM".to_string()),
                "gemini-3-pro-low should NEVER return MEDIUM for budget {}",
                budget
            );
        }
    }
}
