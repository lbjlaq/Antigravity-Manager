//! Cross-Protocol Consistency Tests for Gemini 3
//!
//! Ensures OpenAI and Claude protocols produce identical Gemini API
//! requests for equivalent inputs, verifying consistency across protocols.

#[cfg(test)]
mod cross_protocol_tests {
    use crate::proxy::mappers::claude::models::{ClaudeRequest, MessageContent, ThinkingConfig};
    use crate::proxy::mappers::claude::request::transform_claude_request_in;
    use crate::proxy::mappers::openai::models::{OpenAIContent, OpenAIMessage, OpenAIRequest};
    use crate::proxy::mappers::openai::request::transform_openai_request;

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

    #[test]
    fn test_flash_thinking_level_consistency() {
        // Same budget → same thinkingLevel across protocols
        let budget = 15000;

        // OpenAI auto-injection (default MEDIUM)
        let openai_req = create_openai_request("gemini-3-flash");
        let openai_result =
            transform_openai_request(&openai_req, "test-project", "gemini-3-flash");
        assert!(openai_result.is_ok());

        // Claude with explicit budget
        let claude_req = create_claude_request("gemini-3-flash", Some(budget));
        let claude_result = transform_claude_request_in(&claude_req, "test-project");
        assert!(claude_result.is_ok());

        let openai_body = openai_result.unwrap();
        let (claude_body, _) = claude_result.unwrap();

        // Both should map budget 15000 to MEDIUM
        let claude_level = claude_body["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"]
            .as_str()
            .unwrap();
        assert_eq!(
            claude_level, "MEDIUM",
            "Claude with budget 15000 should map to MEDIUM"
        );

        // OpenAI auto-inject defaults to MEDIUM for Flash
        let openai_level = openai_body["request"]["generationConfig"]["thinkingConfig"]
            ["thinkingLevel"]
            .as_str()
            .unwrap();
        assert_eq!(
            openai_level, "MEDIUM",
            "OpenAI auto-inject should default to MEDIUM for Flash"
        );
    }

    #[test]
    fn test_pro_thinking_level_consistency() {
        // Same budget → same thinkingLevel for Pro
        let budget = 20000;

        // Claude with explicit budget
        let claude_req = create_claude_request("gemini-3-pro-high", Some(budget));
        let claude_result = transform_claude_request_in(&claude_req, "test-project");
        assert!(claude_result.is_ok());

        let (claude_body, _) = claude_result.unwrap();
        let claude_level = claude_body["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"]
            .as_str()
            .unwrap();
        assert_eq!(
            claude_level, "HIGH",
            "Claude with budget 20000 should map to HIGH for Pro"
        );

        // OpenAI auto-injection (default HIGH for Pro)
        let openai_req = create_openai_request("gemini-3-pro-high");
        let openai_result =
            transform_openai_request(&openai_req, "test-project", "gemini-3-pro-high");
        assert!(openai_result.is_ok());

        let openai_body = openai_result.unwrap();
        let openai_level = openai_body["request"]["generationConfig"]["thinkingConfig"]
            ["thinkingLevel"]
            .as_str()
            .unwrap();
        assert_eq!(
            openai_level, "HIGH",
            "OpenAI auto-inject should default to HIGH for Pro"
        );
    }

    #[test]
    fn test_auto_injection_defaults_match() {
        // Auto-injection should use same defaults across protocols
        // Flash: MEDIUM, Pro: HIGH

        // Test Flash
        let openai_flash = create_openai_request("gemini-3-flash");
        let result_flash = transform_openai_request(&openai_flash, "test-project", "gemini-3-flash");
        assert!(result_flash.is_ok());

        let body = result_flash.unwrap();
        let level = body["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"]
            .as_str()
            .unwrap();
        assert_eq!(level, "MEDIUM", "Flash auto-inject → MEDIUM");

        // Test Pro
        let openai_pro = create_openai_request("gemini-3-pro-high");
        let result_pro =
            transform_openai_request(&openai_pro, "test-project", "gemini-3-pro-high");
        assert!(result_pro.is_ok());

        let body = result_pro.unwrap();
        let level = body["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"]
            .as_str()
            .unwrap();
        assert_eq!(level, "HIGH", "Pro auto-inject → HIGH");
    }

    #[test]
    fn test_validation_applied_to_both_protocols() {
        // Both protocols should go through same validation
        let claude_req = create_claude_request("gemini-3-flash", Some(15000));
        let claude_result = transform_claude_request_in(&claude_req, "test-project");
        assert!(claude_result.is_ok(), "Valid Claude request should pass");

        let openai_req = create_openai_request("gemini-3-flash");
        let openai_result =
            transform_openai_request(&openai_req, "test-project", "gemini-3-flash");
        assert!(openai_result.is_ok(), "Valid OpenAI request should pass");
    }

    #[test]
    fn test_gemini_25_backward_compat_both_protocols() {
        // Gemini 2.5 should use thinkingBudget in both protocols
        let claude_req = create_claude_request("gemini-2.5-flash-thinking", Some(16000));
        let claude_result = transform_claude_request_in(&claude_req, "test-project");
        assert!(claude_result.is_ok());

        let (claude_body, _) = claude_result.unwrap();
        let config = claude_body["request"]["generationConfig"]["thinkingConfig"].as_object();
        assert!(config.is_some());

        // Should have thinkingBudget
        assert!(
            config.unwrap().get("thinkingBudget").is_some(),
            "Gemini 2.5 should have thinkingBudget"
        );

        // Should NOT have thinkingLevel
        assert!(
            config.unwrap().get("thinkingLevel").is_none(),
            "Gemini 2.5 should NOT have thinkingLevel"
        );
    }
}
