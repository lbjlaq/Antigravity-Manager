//! Edge Case & Robustness Tests for Gemini 3
//!
//! Tests boundary conditions, error handling, and robustness of
//! Gemini 3 thinking level mapping and validation.

use crate::proxy::mappers::common::gemini_api_validator::{
    validate_gemini_request, GeminiApiValidationError,
};
use crate::proxy::mappers::common::gemini_detection::is_gemini_3_model;
use crate::proxy::mappers::common::thinking_level_mapper::determine_thinking_level;
use serde_json::json;

#[cfg(test)]
mod edge_case_tests {
    use super::*;

    #[test]
    fn test_zero_budget_flash() {
        // Budget = 0 → MINIMAL for Flash
        let level = determine_thinking_level("gemini-3-flash", Some(0));
        assert_eq!(
            level, "MINIMAL",
            "Zero budget should map to MINIMAL for Flash"
        );

        // Verify this is the lowest level (boundary)
        let level_1 = determine_thinking_level("gemini-3-flash", Some(1));
        assert_eq!(
            level_1, "MINIMAL",
            "Budget 1 should also map to MINIMAL for Flash"
        );
    }

    #[test]
    fn test_zero_budget_pro() {
        // Budget = 0 → LOW for Pro
        let level = determine_thinking_level("gemini-3-pro-high", Some(0));
        assert_eq!(level, "LOW", "Zero budget should map to LOW for Pro");

        let level_low = determine_thinking_level("gemini-3-pro-low", Some(0));
        assert_eq!(
            level_low, "LOW",
            "Zero budget should map to LOW for Pro Low"
        );
    }

    #[test]
    fn test_negative_budget_clamping() {
        // Negative budgets should be handled gracefully
        // Note: Current implementation uses min(32000) which allows negatives through,
        // then pattern matching treats them as not matching any positive range

        // Negative values don't match positive ranges (0..=4000, etc.), so they fall
        // through to the catch-all `_` which returns HIGH
        let level_flash = determine_thinking_level("gemini-3-flash", Some(-1000));
        assert_eq!(
            level_flash, "HIGH",
            "Negative budget doesn't match any positive range, falls to _ => HIGH for Flash"
        );

        // Same behavior for Pro
        let level_pro = determine_thinking_level("gemini-3-pro-high", Some(-1000));
        assert_eq!(
            level_pro, "HIGH",
            "Negative budget doesn't match any positive range, falls to _ => HIGH for Pro"
        );

        // Verify extremely negative values
        let level_extreme = determine_thinking_level("gemini-3-flash", Some(i32::MIN));
        assert_eq!(
            level_extreme, "HIGH",
            "i32::MIN falls through to catch-all HIGH for Flash"
        );
    }

    #[test]
    fn test_max_budget_overflow() {
        // Budget > i32::MAX handled correctly
        // Since we use i32, test with i32::MAX and verify clamping

        let level_flash = determine_thinking_level("gemini-3-flash", Some(i32::MAX));
        assert_eq!(
            level_flash, "HIGH",
            "i32::MAX should clamp to 32000 and map to HIGH for Flash"
        );

        let level_pro = determine_thinking_level("gemini-3-pro-high", Some(i32::MAX));
        assert_eq!(
            level_pro, "HIGH",
            "i32::MAX should clamp to 32000 and map to HIGH for Pro"
        );

        // Test values just above 32000
        let level_above = determine_thinking_level("gemini-3-flash", Some(33000));
        assert_eq!(
            level_above, "HIGH",
            "33000 should clamp to 32000 and map to HIGH"
        );

        let level_large = determine_thinking_level("gemini-3-flash", Some(100000));
        assert_eq!(
            level_large, "HIGH",
            "100000 should clamp to 32000 and map to HIGH"
        );
    }

    #[test]
    fn test_boundary_values() {
        // Test exact boundary values (4000, 10000, 16000, 20000, 32000)

        // Flash boundaries
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(4000)),
            "MINIMAL",
            "Boundary 4000 should be MINIMAL (inclusive)"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(4001)),
            "LOW",
            "Boundary 4001 should be LOW"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(10000)),
            "LOW",
            "Boundary 10000 should be LOW (inclusive)"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(10001)),
            "MEDIUM",
            "Boundary 10001 should be MEDIUM"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(20000)),
            "MEDIUM",
            "Boundary 20000 should be MEDIUM (inclusive)"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(20001)),
            "HIGH",
            "Boundary 20001 should be HIGH"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(32000)),
            "HIGH",
            "Boundary 32000 should be HIGH"
        );

        // Pro boundaries (only one: 16000)
        assert_eq!(
            determine_thinking_level("gemini-3-pro-high", Some(16000)),
            "LOW",
            "Boundary 16000 should be LOW for Pro (inclusive)"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-pro-high", Some(16001)),
            "HIGH",
            "Boundary 16001 should be HIGH for Pro"
        );

        // Verify Pro at Flash boundaries doesn't return MEDIUM
        assert_ne!(
            determine_thinking_level("gemini-3-pro-high", Some(10000)),
            "MEDIUM",
            "Pro should never return MEDIUM at Flash boundaries"
        );
        assert_ne!(
            determine_thinking_level("gemini-3-pro-high", Some(20000)),
            "MEDIUM",
            "Pro should never return MEDIUM at Flash boundaries"
        );
    }

    #[test]
    fn test_malformed_thinking_config() {
        // Invalid JSON structure handled gracefully
        let invalid_configs = vec![
            // Gemini 3.x with thinkingBudget instead of thinkingLevel
            json!({
                "generationConfig": {
                    "thinkingConfig": {
                        "includeThoughts": true,
                        "thinkingBudget": 16000  // Wrong for Gemini 3.x
                    }
                }
            }),
            // Gemini 2.5 with thinkingLevel instead of thinkingBudget
            json!({
                "generationConfig": {
                    "thinkingConfig": {
                        "includeThoughts": true,
                        "thinkingLevel": "HIGH"  // Wrong for Gemini 2.5
                    }
                }
            }),
            // Invalid thinking level value for Flash
            json!({
                "generationConfig": {
                    "thinkingConfig": {
                        "includeThoughts": true,
                        "thinkingLevel": "ULTRA"  // Invalid
                    }
                }
            }),
            // Pro with MEDIUM level (not supported)
            json!({
                "generationConfig": {
                    "thinkingConfig": {
                        "includeThoughts": true,
                        "thinkingLevel": "MEDIUM"  // Pro doesn't support MEDIUM
                    }
                }
            }),
        ];

        // Test 1: Gemini 3.x with thinkingBudget
        let result = validate_gemini_request("gemini-3-pro-high", &invalid_configs[0]);
        assert!(
            result.is_err(),
            "Gemini 3.x with thinkingBudget should fail validation"
        );
        assert!(matches!(
            result.unwrap_err(),
            GeminiApiValidationError::Gemini3WithBudget { .. }
        ));

        // Test 2: Gemini 2.5 with thinkingLevel
        let result = validate_gemini_request("gemini-2.5-pro-thinking", &invalid_configs[1]);
        assert!(
            result.is_err(),
            "Gemini 2.5 with thinkingLevel should fail validation"
        );
        assert!(matches!(
            result.unwrap_err(),
            GeminiApiValidationError::Gemini25WithLevel { .. }
        ));

        // Test 3: Invalid level value
        let result = validate_gemini_request("gemini-3-flash", &invalid_configs[2]);
        assert!(
            result.is_err(),
            "Invalid thinkingLevel value should fail validation"
        );
        assert!(matches!(
            result.unwrap_err(),
            GeminiApiValidationError::InvalidThinkingLevel { .. }
        ));

        // Test 4: Pro with MEDIUM level
        let result = validate_gemini_request("gemini-3-pro-high", &invalid_configs[3]);
        assert!(
            result.is_err(),
            "Pro with MEDIUM level should fail validation"
        );
    }

    #[test]
    fn test_missing_generation_config() {
        // Request without generationConfig handled correctly
        let request_no_config = json!({
            "contents": [{
                "role": "user",
                "parts": [{"text": "Hello"}]
            }]
            // No generationConfig at all
        });

        // Should pass validation (non-thinking request)
        let result = validate_gemini_request("gemini-3-flash", &request_no_config);
        assert!(
            result.is_ok(),
            "Request without generationConfig should pass (non-thinking)"
        );

        // Request with generationConfig but no thinkingConfig
        let request_no_thinking = json!({
            "generationConfig": {
                "temperature": 0.7,
                "topP": 0.9
            }
        });

        let result = validate_gemini_request("gemini-3-flash", &request_no_thinking);
        assert!(
            result.is_ok(),
            "Request without thinkingConfig should pass (non-thinking)"
        );
    }

    #[test]
    fn test_concurrent_requests() {
        // Thread safety and concurrent mapping
        use std::sync::Arc;
        use std::thread;

        let test_cases = Arc::new(vec![
            ("gemini-3-flash", Some(5000), "LOW"),
            ("gemini-3-flash", Some(15000), "MEDIUM"),
            ("gemini-3-flash", Some(25000), "HIGH"),
            ("gemini-3-pro-high", Some(10000), "LOW"),
            ("gemini-3-pro-high", Some(20000), "HIGH"),
            ("gemini-3-flash", Some(0), "MINIMAL"),
            ("gemini-3-flash", Some(32000), "HIGH"),
            ("gemini-3-pro-low", Some(16000), "LOW"),
        ]);

        let mut handles = vec![];

        // Spawn 8 threads, each processing all test cases
        for thread_id in 0..8 {
            let cases = Arc::clone(&test_cases);
            let handle = thread::spawn(move || {
                for (model, budget, expected) in cases.iter() {
                    let result = determine_thinking_level(model, *budget);
                    assert_eq!(
                        result, *expected,
                        "Thread {} failed: model={}, budget={:?}, expected={}",
                        thread_id, model, budget, expected
                    );

                    // Also test detection
                    let is_gemini_3 = is_gemini_3_model(model);
                    assert!(is_gemini_3, "Model {} should be detected as Gemini 3", model);
                }
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread panicked");
        }

        // If we got here, all concurrent executions succeeded
        assert!(true, "Concurrent requests handled correctly");
    }
}
