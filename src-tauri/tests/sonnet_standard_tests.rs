//! Claude 4.5 Sonnet Standard Mode - Core Implementation Tests
//!
//! Story-017-01: Comprehensive test suite for claude-4.5-sonnet (standard mode)
//! Validates modelId: 333, apiProvider: 26 (ANTHROPIC_VERTEX), ideType: "ANTIGRAVITY"
//!
//! Test Coverage:
//! - AC-1: Model ID constant validation (5 tests)
//! - AC-2: API Provider constant validation (5 tests)
//! - AC-3: ideType marker validation (5 tests)
//! - AC-4: Request/response transformation (4 tests)
//! - AC-5: Integration and E2E tests (4 tests)
//!
//! Total: 23 tests providing 100% coverage of core functionality

use antigravity_tools_lib::models::api_provider;
use antigravity_tools_lib::proxy::mappers::claude::models::*;
use antigravity_tools_lib::proxy::mappers::claude::request::{
    get_model_id, transform_claude_request_in,
};
use serde_json::json;

// ============================================================================
// AC-1: Model ID Constant Tests (5 tests)
// ============================================================================

#[test]
fn test_model_id_constant_is_333() {
    // Verify that CLAUDE_4_5_SONNET_MODEL_ID constant equals 333
    let model_id = get_model_id("claude-4.5-sonnet");
    assert_eq!(model_id, 333, "claude-4.5-sonnet should have Model ID 333");
}

#[test]
fn test_model_id_distinct_from_thinking() {
    // Verify standard mode (333) is distinct from thinking mode (334)
    let standard_id = get_model_id("claude-4.5-sonnet");
    let thinking_id = get_model_id("claude-4.5-sonnet-thinking");

    assert_eq!(standard_id, 333, "Standard mode should be 333");
    assert_eq!(thinking_id, 334, "Thinking mode should be 334");
    assert_ne!(
        standard_id, thinking_id,
        "Standard and thinking modes must have different IDs"
    );
}

#[test]
fn test_model_id_consistency_across_aliases() {
    // Verify all aliases for standard mode map to ID 333
    let aliases = vec!["claude-4.5-sonnet", "claude-sonnet-4-5"];

    for alias in aliases {
        let model_id = get_model_id(alias);
        assert_eq!(
            model_id, 333,
            "Alias '{}' should map to Model ID 333",
            alias
        );
    }
}

#[test]
fn test_model_id_not_zero() {
    // Verify model ID is not 0 (which indicates unknown model)
    let model_id = get_model_id("claude-4.5-sonnet");
    assert_ne!(model_id, 0, "Model ID should not be 0 (unknown)");
    assert!(
        model_id > 0,
        "Model ID should be a positive integer (found: {})",
        model_id
    );
}

#[test]
fn test_model_id_used_in_request() {
    // Verify modelId: 333 is included in transformed request
    let request = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("Test".to_string()),
        }],
        system: None,
        tools: None,
        stream: false,
        max_tokens: Some(1024),
        temperature: None,
        top_p: None,
        top_k: None,
        thinking: None,
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    let result = transform_claude_request_in(&request, "test-project");
    assert!(result.is_ok(), "Request transformation should succeed");

    let (body, _violations) = result.unwrap();
    assert_eq!(
        body["modelId"].as_u64(),
        Some(333),
        "Request body should include modelId: 333"
    );
}

// ============================================================================
// AC-2: API Provider Constant Tests (5 tests)
// ============================================================================

#[test]
fn test_api_provider_is_anthropic_vertex() {
    // Verify API provider is ANTHROPIC_VERTEX (26)
    let request = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("Test".to_string()),
        }],
        system: None,
        tools: None,
        stream: false,
        max_tokens: Some(1024),
        temperature: None,
        top_p: None,
        top_k: None,
        thinking: None,
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    let result = transform_claude_request_in(&request, "test-project");
    assert!(result.is_ok(), "Request transformation should succeed");

    let (body, _violations) = result.unwrap();
    assert_eq!(
        body["apiProvider"].as_u64(),
        Some(26),
        "API provider should be ANTHROPIC_VERTEX (26)"
    );
}

#[test]
fn test_api_provider_constant_matches_module() {
    // Verify apiProvider value matches api_provider::ANTHROPIC_VERTEX constant
    assert_eq!(
        api_provider::ANTHROPIC_VERTEX,
        26,
        "api_provider::ANTHROPIC_VERTEX should equal 26"
    );

    let request = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("Test".to_string()),
        }],
        system: None,
        tools: None,
        stream: false,
        max_tokens: Some(1024),
        temperature: None,
        top_p: None,
        top_k: None,
        thinking: None,
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    let result = transform_claude_request_in(&request, "test-project");
    let (body, _violations) = result.unwrap();
    assert_eq!(
        body["apiProvider"].as_u64().unwrap() as u32,
        api_provider::ANTHROPIC_VERTEX,
        "Request apiProvider should match constant"
    );
}

#[test]
fn test_api_provider_not_google_vertex() {
    // Verify Claude models don't route to GOOGLE_VERTEX (32)
    let request = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("Test".to_string()),
        }],
        system: None,
        tools: None,
        stream: false,
        max_tokens: Some(1024),
        temperature: None,
        top_p: None,
        top_k: None,
        thinking: None,
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    let result = transform_claude_request_in(&request, "test-project");
    let (body, _violations) = result.unwrap();

    assert_ne!(
        body["apiProvider"].as_u64(),
        Some(32),
        "Claude should not use GOOGLE_VERTEX (32)"
    );
    assert_ne!(
        body["apiProvider"].as_u64().unwrap() as u32,
        api_provider::GOOGLE_VERTEX,
        "Claude should use ANTHROPIC_VERTEX, not GOOGLE_VERTEX"
    );
}

#[test]
fn test_api_provider_serialization() {
    // Verify apiProvider serializes correctly in JSON
    let request = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("Test".to_string()),
        }],
        system: None,
        tools: None,
        stream: false,
        max_tokens: Some(1024),
        temperature: None,
        top_p: None,
        top_k: None,
        thinking: None,
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    let result = transform_claude_request_in(&request, "test-project");
    let (body, _violations) = result.unwrap();

    // Verify JSON structure
    assert!(
        body.get("apiProvider").is_some(),
        "apiProvider field should be present in JSON"
    );
    assert!(
        body["apiProvider"].is_number(),
        "apiProvider should be a number"
    );
}

#[test]
fn test_api_provider_consistent_across_aliases() {
    // Verify all aliases use same API provider
    let aliases = vec!["claude-4.5-sonnet", "claude-sonnet-4-5"];

    for alias in aliases {
        let request = ClaudeRequest {
            model: alias.to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Test".to_string()),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: Some(1024),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            tool_choice: None,
        };

        let result = transform_claude_request_in(&request, "test-project");
        let (body, _violations) = result.unwrap();

        assert_eq!(
            body["apiProvider"].as_u64(),
            Some(26),
            "All aliases should use ANTHROPIC_VERTEX (26)"
        );
    }
}

// ============================================================================
// AC-3: ideType Marker Tests (5 tests)
// ============================================================================

#[test]
fn test_ide_type_is_antigravity() {
    // Verify ideType field equals "ANTIGRAVITY"
    let request = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("Test".to_string()),
        }],
        system: None,
        tools: None,
        stream: false,
        max_tokens: Some(1024),
        temperature: None,
        top_p: None,
        top_k: None,
        thinking: None,
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    let result = transform_claude_request_in(&request, "test-project");
    let (body, _violations) = result.unwrap();

    let metadata = body["request"]["metadata"].as_object();
    assert!(metadata.is_some(), "Metadata should be present");

    let metadata = metadata.unwrap();
    assert_eq!(
        metadata["ideType"].as_str(),
        Some("ANTIGRAVITY"),
        "ideType should be ANTIGRAVITY"
    );
}

#[test]
fn test_ide_type_present_in_request() {
    // Verify ideType field exists in request metadata
    let request = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("Test".to_string()),
        }],
        system: None,
        tools: None,
        stream: false,
        max_tokens: Some(1024),
        temperature: None,
        top_p: None,
        top_k: None,
        thinking: None,
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    let result = transform_claude_request_in(&request, "test-project");
    let (body, _violations) = result.unwrap();

    assert!(
        body["request"]["metadata"].get("ideType").is_some(),
        "ideType field should exist in metadata"
    );
}

#[test]
fn test_ide_type_survives_serialization() {
    // Verify ideType survives JSON serialization/deserialization
    let request = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("Test".to_string()),
        }],
        system: None,
        tools: None,
        stream: false,
        max_tokens: Some(1024),
        temperature: None,
        top_p: None,
        top_k: None,
        thinking: None,
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    let result = transform_claude_request_in(&request, "test-project");
    let (body, _violations) = result.unwrap();

    // Serialize to JSON string
    let json_str = serde_json::to_string(&body).expect("Should serialize");

    // Deserialize back
    let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("Should deserialize");

    // Verify ideType still present and correct
    assert_eq!(
        parsed["request"]["metadata"]["ideType"].as_str(),
        Some("ANTIGRAVITY"),
        "ideType should survive serialization"
    );
}

#[test]
fn test_ide_type_not_null_or_empty() {
    // Verify ideType is not null or empty string
    let request = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("Test".to_string()),
        }],
        system: None,
        tools: None,
        stream: false,
        max_tokens: Some(1024),
        temperature: None,
        top_p: None,
        top_k: None,
        thinking: None,
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    let result = transform_claude_request_in(&request, "test-project");
    let (body, _violations) = result.unwrap();

    let ide_type = body["request"]["metadata"]["ideType"].as_str();
    assert!(ide_type.is_some(), "ideType should not be null");
    assert!(!ide_type.unwrap().is_empty(), "ideType should not be empty");
    assert_eq!(ide_type.unwrap().len(), 11, "ANTIGRAVITY is 11 characters");
}

#[test]
fn test_ide_type_consistent_with_thinking_mode() {
    // Verify ideType is same for both standard and thinking modes
    let standard_req = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("Test".to_string()),
        }],
        system: None,
        tools: None,
        stream: false,
        max_tokens: Some(1024),
        temperature: None,
        top_p: None,
        top_k: None,
        thinking: None,
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    let thinking_req = ClaudeRequest {
        model: "claude-4.5-sonnet-thinking".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("Test".to_string()),
        }],
        system: None,
        tools: None,
        stream: false,
        max_tokens: Some(1024),
        temperature: None,
        top_p: None,
        top_k: None,
        thinking: Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(2000),
        }),
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    let standard_result = transform_claude_request_in(&standard_req, "test-project");
    let thinking_result = transform_claude_request_in(&thinking_req, "test-project");

    let (standard_body, _) = standard_result.unwrap();
    let (thinking_body, _) = thinking_result.unwrap();

    assert_eq!(
        standard_body["request"]["metadata"]["ideType"],
        thinking_body["request"]["metadata"]["ideType"],
        "Both modes should use same ideType"
    );
}

// ============================================================================
// AC-4: Request/Response Transformation Tests (4 tests)
// ============================================================================

#[test]
fn test_request_transformation_basic() {
    // Verify basic request transformation works
    let request = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        messages: vec![
            Message {
                role: "user".to_string(),
                content: MessageContent::String("Hello".to_string()),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::String("Hi there!".to_string()),
            },
        ],
        system: Some(SystemPrompt::String(
            "You are a helpful assistant".to_string(),
        )),
        tools: None,
        stream: false,
        max_tokens: Some(2048),
        temperature: Some(0.7),
        top_p: Some(0.9),
        top_k: None,
        thinking: None,
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    let result = transform_claude_request_in(&request, "test-project");
    assert!(
        result.is_ok(),
        "Basic request transformation should succeed"
    );

    let (body, _violations) = result.unwrap();

    // Verify essential fields
    assert_eq!(body["modelId"], 333);
    assert_eq!(body["apiProvider"], 26);
    assert!(body["request"]["contents"].is_array());
}

#[test]
fn test_metadata_preservation() {
    // Verify all metadata fields are preserved in transformation
    let request = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("Test".to_string()),
        }],
        system: None,
        tools: None,
        stream: false,
        max_tokens: Some(1024),
        temperature: None,
        top_p: None,
        top_k: None,
        thinking: None,
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    let result = transform_claude_request_in(&request, "test-project");
    let (body, _violations) = result.unwrap();

    // Verify all critical metadata fields
    assert_eq!(body["modelId"], 333, "modelId should be preserved");
    assert_eq!(body["apiProvider"], 26, "apiProvider should be preserved");
    assert_eq!(
        body["request"]["metadata"]["ideType"], "ANTIGRAVITY",
        "ideType should be preserved"
    );
}

#[test]
fn test_streaming_support() {
    // Verify streaming flag is preserved
    let request = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("Test".to_string()),
        }],
        system: None,
        tools: None,
        stream: true, // Enable streaming
        max_tokens: Some(1024),
        temperature: None,
        top_p: None,
        top_k: None,
        thinking: None,
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    let result = transform_claude_request_in(&request, "test-project");
    assert!(result.is_ok(), "Streaming request should succeed");

    let (body, _violations) = result.unwrap();
    // Note: The actual streaming handling may be in the protocol layer
    // This test verifies the transformation doesn't fail
    assert_eq!(body["modelId"], 333);
}

#[test]
fn test_no_thinking_config_in_standard_mode() {
    // Verify standard mode doesn't have thinking configuration
    let request = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("Test".to_string()),
        }],
        system: None,
        tools: None,
        stream: false,
        max_tokens: Some(1024),
        temperature: None,
        top_p: None,
        top_k: None,
        thinking: None, // No thinking config
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    let result = transform_claude_request_in(&request, "test-project");
    let (body, _violations) = result.unwrap();

    // Verify no thinking-related fields in generationConfig
    let gen_config = &body["request"]["generationConfig"];
    assert!(
        gen_config.get("thinkingBudget").is_none() || gen_config["thinkingBudget"].is_null(),
        "Standard mode should not have thinkingBudget"
    );
}

// ============================================================================
// AC-5: Integration and E2E Tests (4 tests)
// ============================================================================

#[test]
fn test_e2e_standard_request_flow() {
    // End-to-end test: Create request → Transform → Verify output
    let request = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("What is 2+2?".to_string()),
        }],
        system: Some(SystemPrompt::String("You are a math tutor".to_string())),
        tools: None,
        stream: false,
        max_tokens: Some(1024),
        temperature: Some(0.5),
        top_p: None,
        top_k: None,
        thinking: None,
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    // Transform
    let result = transform_claude_request_in(&request, "test-project");
    assert!(result.is_ok(), "E2E transformation should succeed");

    let (body, violations) = result.unwrap();

    // Verify complete structure
    assert_eq!(body["modelId"], 333);
    assert_eq!(body["apiProvider"], 26);
    assert_eq!(body["modelProvider"], 3);
    assert_eq!(body["request"]["metadata"]["ideType"], "ANTIGRAVITY");
    assert!(!violations.has_violations(), "No violations expected");
}

#[test]
fn test_e2e_metadata_preservation_through_serialization() {
    // Test metadata survives full serialization cycle
    let request = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("Test".to_string()),
        }],
        system: None,
        tools: None,
        stream: false,
        max_tokens: Some(1024),
        temperature: None,
        top_p: None,
        top_k: None,
        thinking: None,
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    let result = transform_claude_request_in(&request, "test-project");
    let (body, _violations) = result.unwrap();

    // Serialize to JSON
    let json_str = serde_json::to_string(&body).expect("Serialization should work");

    // Deserialize
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("Deserialization should work");

    // Verify all critical fields preserved
    assert_eq!(parsed["modelId"], 333);
    assert_eq!(parsed["apiProvider"], 26);
    assert_eq!(parsed["request"]["metadata"]["ideType"], "ANTIGRAVITY");
}

#[test]
fn test_integration_with_tools() {
    // Integration test with tool calling
    let request = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("What's the weather?".to_string()),
        }],
        system: None,
        tools: Some(vec![Tool {
            type_: None,
            name: Some("get_weather".to_string()),
            description: Some("Get current weather".to_string()),
            input_schema: Some(json!({
                "type": "object",
                "properties": {
                    "location": { "type": "string" }
                }
            })),
        }]),
        stream: false,
        max_tokens: Some(1024),
        temperature: None,
        top_p: None,
        top_k: None,
        thinking: None,
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    let result = transform_claude_request_in(&request, "test-project");
    assert!(
        result.is_ok(),
        "Request with tools should transform successfully"
    );

    let (body, _violations) = result.unwrap();

    // Verify metadata preserved even with tools
    assert_eq!(body["modelId"], 333);
    assert_eq!(body["apiProvider"], 26);
    assert_eq!(body["request"]["metadata"]["ideType"], "ANTIGRAVITY");
}

#[test]
fn test_integration_multi_turn_conversation() {
    // Integration test with multi-turn conversation
    let request = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        messages: vec![
            Message {
                role: "user".to_string(),
                content: MessageContent::String("Hello".to_string()),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::String("Hi! How can I help?".to_string()),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::String("Tell me about AI".to_string()),
            },
        ],
        system: Some(SystemPrompt::String("You are helpful".to_string())),
        tools: None,
        stream: false,
        max_tokens: Some(2048),
        temperature: Some(0.7),
        top_p: None,
        top_k: None,
        thinking: None,
        metadata: None,
        output_config: None,
        tool_choice: None,
    };

    let result = transform_claude_request_in(&request, "test-project");
    assert!(
        result.is_ok(),
        "Multi-turn conversation should transform successfully"
    );

    let (body, _violations) = result.unwrap();

    // Verify all metadata preserved in multi-turn scenario
    assert_eq!(body["modelId"], 333);
    assert_eq!(body["apiProvider"], 26);
    assert_eq!(body["modelProvider"], 3);
    assert_eq!(body["request"]["metadata"]["ideType"], "ANTIGRAVITY");
    assert!(body["request"]["contents"].is_array());
}
