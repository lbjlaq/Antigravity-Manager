//! End-to-End Protocol Tests for Gemini 3 API Migration
//!
//! Tests full request transformation pipelines from OpenAI/Claude protocols
//! to Gemini API format, verifying correct thinking level mapping and validation.

#[cfg(test)]
mod e2e_protocol_tests {
    use crate::proxy::mappers::claude::models::{ClaudeRequest, MessageContent, ThinkingConfig};
    use crate::proxy::mappers::claude::request::transform_claude_request_in;
    use crate::proxy::mappers::openai::models::{OpenAIContent, OpenAIMessage, OpenAIRequest};
    use crate::proxy::mappers::openai::request::transform_openai_request;

    // ===== TEST HELPERS =====

    fn create_openai_request(model: &str) -> OpenAIRequest {
        OpenAIRequest {
            model: model.to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(OpenAIContent::String("Test message".to_string())),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }],
            prompt: None,
            stream: false,
            n: None,
            max_tokens: Some(1024),
            temperature: Some(0.7),
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

    fn create_claude_request(model: &str, budget: Option<u32>) -> ClaudeRequest {
        let thinking = budget.map(|b| ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(b),
        });

        ClaudeRequest {
            model: model.to_string(),
            messages: vec![crate::proxy::mappers::claude::models::Message {
                role: "user".to_string(),
                content: MessageContent::String("Test message".to_string()),
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

    // ===== E2E Flash Tests =====

    #[test]
    fn test_e2e_flash_minimal() {
        let req = create_claude_request("gemini-3-flash", Some(2000));
        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok(), "Transform should succeed");

        let (body, _violations) = result.unwrap();
        let thinking_config = &body["request"]["generationConfig"]["thinkingConfig"];
        assert!(thinking_config.is_object(), "thinkingConfig should exist");

        let thinking_level = thinking_config["thinkingLevel"].as_str().unwrap();
        assert_eq!(thinking_level, "MINIMAL", "Budget 2000 → MINIMAL for Flash");
    }

    #[test]
    fn test_e2e_flash_low() {
        let req = create_claude_request("gemini-3-flash", Some(7000));
        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let level = body["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"]
            .as_str()
            .unwrap();
        assert_eq!(level, "LOW", "Budget 7000 → LOW for Flash");
    }

    #[test]
    fn test_e2e_flash_medium() {
        let req = create_claude_request("gemini-3-flash", Some(15000));
        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let level = body["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"]
            .as_str()
            .unwrap();
        assert_eq!(level, "MEDIUM", "Budget 15000 → MEDIUM for Flash");
    }

    #[test]
    fn test_e2e_flash_high() {
        let req = create_claude_request("gemini-3-flash", Some(25000));
        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let level = body["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"]
            .as_str()
            .unwrap();
        assert_eq!(level, "HIGH", "Budget 25000 → HIGH for Flash");
    }

    // ===== E2E Pro Tests =====

    #[test]
    fn test_e2e_pro_low() {
        let req = create_claude_request("gemini-3-pro-low", Some(10000));
        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let level = body["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"]
            .as_str()
            .unwrap();
        assert_eq!(level, "LOW", "Budget 10000 → LOW for Pro");
    }

    #[test]
    fn test_e2e_pro_high() {
        let req = create_claude_request("gemini-3-pro-high", Some(20000));
        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let level = body["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"]
            .as_str()
            .unwrap();
        assert_eq!(level, "HIGH", "Budget 20000 → HIGH for Pro");
    }

    // ===== Default Budget Tests =====

    #[test]
    fn test_e2e_flash_default() {
        let req = create_openai_request("gemini-3-flash");
        let result = transform_openai_request(&req, "test-project", "gemini-3-flash");
        assert!(result.is_ok());

        let body = result.unwrap();
        let config = body["request"]["generationConfig"]["thinkingConfig"].as_object();
        assert!(config.is_some(), "Auto-inject should add thinkingConfig");

        let level = config.unwrap()["thinkingLevel"].as_str();
        assert_eq!(level, Some("MEDIUM"), "Flash default → MEDIUM");
    }

    #[test]
    fn test_e2e_pro_default() {
        let req = create_openai_request("gemini-3-pro-high");
        let result = transform_openai_request(&req, "test-project", "gemini-3-pro-high");
        assert!(result.is_ok());

        let body = result.unwrap();
        let config = body["request"]["generationConfig"]["thinkingConfig"].as_object();
        assert!(config.is_some());

        let level = config.unwrap()["thinkingLevel"].as_str();
        assert_eq!(level, Some("HIGH"), "Pro default → HIGH");
    }

    // ===== Validation Test =====

    #[test]
    fn test_e2e_validation_passes() {
        let req = create_claude_request("gemini-3-flash", Some(15000));
        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok(), "Valid request should pass validation");
    }

    // ===== Backward Compatibility Test =====

    #[test]
    fn test_e2e_gemini_25_backward_compat() {
        let req = create_claude_request("gemini-2.5-flash-thinking", Some(16000));
        let result = transform_claude_request_in(&req, "test-project");
        assert!(result.is_ok());

        let (body, _) = result.unwrap();
        let config = body["request"]["generationConfig"]["thinkingConfig"].as_object();
        assert!(config.is_some());

        // Gemini 2.5 should have thinkingBudget
        let budget = config.unwrap().get("thinkingBudget");
        assert!(budget.is_some(), "Gemini 2.5 should have thinkingBudget");

        // Should NOT have thinkingLevel
        assert!(
            config.unwrap().get("thinkingLevel").is_none(),
            "Gemini 2.5 should NOT have thinkingLevel"
        );
    }
}
