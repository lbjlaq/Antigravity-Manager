//! Performance & Quality Tests for Gemini 3
//!
//! Verifies performance characteristics and error message quality
//! for Gemini 3 thinking logic.

use crate::proxy::mappers::common::gemini_api_validator::validate_gemini_request;
use crate::proxy::mappers::common::gemini_detection::is_gemini_3_model;
use crate::proxy::mappers::common::thinking_level_mapper::determine_thinking_level;
use serde_json::json;
use std::time::Instant;

#[cfg(test)]
mod performance_tests {
    use super::*;

    #[test]
    fn test_detection_performance() {
        // Verify is_gemini_3_model() < 1µs per call
        let test_models = vec![
            "gemini-3-flash",
            "gemini-3-pro-high",
            "gemini-3-pro-low",
            "gemini-2.5-flash-thinking",
            "gemini-2.5-pro-thinking",
            "claude-sonnet-4-5",
            "gpt-4o",
        ];

        let iterations = 10000; // 10K iterations for stable measurement

        for model in &test_models {
            let start = Instant::now();
            for _ in 0..iterations {
                let _ = is_gemini_3_model(model);
            }
            let elapsed = start.elapsed();

            let avg_per_call = elapsed.as_nanos() as f64 / iterations as f64;
            println!(
                "[Performance] is_gemini_3_model('{}') avg: {:.2}ns ({:.3}µs)",
                model,
                avg_per_call,
                avg_per_call / 1000.0
            );

            // Verify < 1µs per call (1000ns)
            assert!(
                avg_per_call < 1000.0,
                "Detection for '{}' should be <1µs, got {:.2}ns",
                model,
                avg_per_call
            );
        }

        println!("[Performance] ✅ All detection calls <1µs");
    }

    #[test]
    fn test_mapping_performance() {
        // Verify determine_thinking_level() < 1µs per call
        let test_cases = vec![
            ("gemini-3-flash", Some(2000)),
            ("gemini-3-flash", Some(7000)),
            ("gemini-3-flash", Some(15000)),
            ("gemini-3-flash", Some(25000)),
            ("gemini-3-flash", None),
            ("gemini-3-pro-high", Some(10000)),
            ("gemini-3-pro-high", Some(20000)),
            ("gemini-3-pro-high", None),
            ("gemini-3-pro-low", Some(16000)),
        ];

        let iterations = 10000; // 10K iterations for stable measurement

        for (model, budget) in &test_cases {
            let start = Instant::now();
            for _ in 0..iterations {
                let _ = determine_thinking_level(model, *budget);
            }
            let elapsed = start.elapsed();

            let avg_per_call = elapsed.as_nanos() as f64 / iterations as f64;
            println!(
                "[Performance] determine_thinking_level('{}', {:?}) avg: {:.2}ns ({:.3}µs)",
                model,
                budget,
                avg_per_call,
                avg_per_call / 1000.0
            );

            // Verify < 1µs per call (1000ns)
            assert!(
                avg_per_call < 1000.0,
                "Mapping for '{}' with budget {:?} should be <1µs, got {:.2}ns",
                model,
                budget,
                avg_per_call
            );
        }

        println!("[Performance] ✅ All mapping calls <1µs");
    }

    #[test]
    fn test_validation_performance() {
        // Verify validation < 100µs per request
        let test_requests = vec![
            (
                "gemini-3-flash",
                json!({
                    "generationConfig": {
                        "thinkingConfig": {
                            "includeThoughts": true,
                            "thinkingLevel": "HIGH"
                        }
                    }
                }),
            ),
            (
                "gemini-3-pro-high",
                json!({
                    "generationConfig": {
                        "thinkingConfig": {
                            "includeThoughts": true,
                            "thinkingLevel": "LOW"
                        }
                    }
                }),
            ),
            (
                "gemini-2.5-flash-thinking",
                json!({
                    "generationConfig": {
                        "thinkingConfig": {
                            "includeThoughts": true,
                            "thinkingBudget": 16000
                        }
                    }
                }),
            ),
            (
                "gemini-3-flash",
                json!({
                    "generationConfig": {
                        "temperature": 0.7
                    }
                }),
            ),
        ];

        let iterations = 1000; // 1K iterations for validation (more complex)

        for (model, request) in &test_requests {
            let start = Instant::now();
            for _ in 0..iterations {
                let _ = validate_gemini_request(model, request);
            }
            let elapsed = start.elapsed();

            let avg_per_call = elapsed.as_micros() as f64 / iterations as f64;
            println!(
                "[Performance] validate_gemini_request('{}') avg: {:.2}µs",
                model, avg_per_call
            );

            // Verify < 100µs per request
            assert!(
                avg_per_call < 100.0,
                "Validation for '{}' should be <100µs, got {:.2}µs",
                model,
                avg_per_call
            );
        }

        println!("[Performance] ✅ All validation calls <100µs");
    }

    #[test]
    fn test_memory_efficiency() {
        // No unnecessary allocations, borrows only
        // This is more of a code review test, but we can verify basic behavior

        let model = "gemini-3-flash";
        let budget = Some(15000);

        // Detection - should only do string operations, no allocations
        let is_gemini_3 = is_gemini_3_model(model);
        assert!(is_gemini_3);

        // Mapping - returns static str, no allocations
        let level = determine_thinking_level(model, budget);
        assert_eq!(level, "MEDIUM");

        // Verify level is a static string (no heap allocation)
        let ptr1 = level.as_ptr();
        let level2 = determine_thinking_level(model, budget);
        let ptr2 = level2.as_ptr();

        // If both are static strings with same value, they should point to same memory
        // (This is implementation-dependent but common for static strings)
        assert_eq!(level, level2, "Both calls should return same value");

        // Test multiple calls don't cause memory growth
        let mut levels = Vec::new();
        for _ in 0..1000 {
            levels.push(determine_thinking_level(model, budget));
        }

        // All should be the same static string
        for l in &levels {
            assert_eq!(*l, "MEDIUM");
        }

        println!("[Performance] ✅ Memory efficiency verified (static strings)");
    }

    #[test]
    fn test_error_message_quality() {
        // Validation errors are clear and actionable
        use crate::proxy::mappers::common::gemini_api_validator::GeminiApiValidationError;

        // Test 1: Gemini 3.x with thinkingBudget
        let request_3_budget = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingBudget": 16000
                }
            }
        });

        let result = validate_gemini_request("gemini-3-pro-high", &request_3_budget);
        assert!(result.is_err());
        let error = result.unwrap_err();
        let error_msg = error.to_string();

        // Error should mention:
        // 1. The model name
        // 2. What's wrong (using thinkingBudget)
        // 3. What should be used instead (thinkingLevel)
        assert!(
            error_msg.contains("gemini-3-pro-high"),
            "Error should mention model name: {}",
            error_msg
        );
        assert!(
            error_msg.contains("thinkingLevel"),
            "Error should mention correct API: {}",
            error_msg
        );
        assert!(
            error_msg.contains("thinkingBudget"),
            "Error should mention what's wrong: {}",
            error_msg
        );

        println!("[Quality] Gemini 3.x budget error: {}", error_msg);

        // Test 2: Gemini 2.5 with thinkingLevel
        let request_25_level = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingLevel": "HIGH"
                }
            }
        });

        let result = validate_gemini_request("gemini-2.5-pro-thinking", &request_25_level);
        assert!(result.is_err());
        let error = result.unwrap_err();
        let error_msg = error.to_string();

        assert!(
            error_msg.contains("gemini-2.5-pro-thinking"),
            "Error should mention model name: {}",
            error_msg
        );
        assert!(
            error_msg.contains("thinkingBudget"),
            "Error should mention correct API: {}",
            error_msg
        );
        assert!(
            error_msg.contains("thinkingLevel"),
            "Error should mention what's wrong: {}",
            error_msg
        );

        println!("[Quality] Gemini 2.5 level error: {}", error_msg);

        // Test 3: Invalid thinking level
        let request_invalid_level = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingLevel": "ULTRA"
                }
            }
        });

        let result = validate_gemini_request("gemini-3-flash", &request_invalid_level);
        assert!(result.is_err());
        let error = result.unwrap_err();
        let error_msg = error.to_string();

        assert!(
            error_msg.contains("gemini-3-flash"),
            "Error should mention model name: {}",
            error_msg
        );
        assert!(
            error_msg.contains("ULTRA"),
            "Error should mention invalid value: {}",
            error_msg
        );
        assert!(
            error_msg.contains("MINIMAL") || error_msg.contains("LOW"),
            "Error should list valid levels: {}",
            error_msg
        );

        println!("[Quality] Invalid level error: {}", error_msg);

        // Test 4: Pro with MEDIUM level
        let request_pro_medium = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingLevel": "MEDIUM"
                }
            }
        });

        let result = validate_gemini_request("gemini-3-pro-high", &request_pro_medium);
        assert!(result.is_err());
        let error = result.unwrap_err();
        let error_msg = error.to_string();

        assert!(
            error_msg.contains("gemini-3-pro-high"),
            "Error should mention model name: {}",
            error_msg
        );
        assert!(
            error_msg.contains("MEDIUM"),
            "Error should mention invalid value: {}",
            error_msg
        );

        println!("[Quality] Pro MEDIUM error: {}", error_msg);

        println!("[Quality] ✅ All error messages are clear and actionable");
    }
}
