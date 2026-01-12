// 🆕 Story #24-01: ideType Marker Testing
// 🚨 CRITICAL: Validates that ALL models inject ideType="ANTIGRAVITY" for anti-detection
//
// Test Coverage:
// - Gemini models: 5+ models (gemini-2.0-flash-exp, gemini-2.5-flash, gemini-2.5-flash-lite, gemini-2.5-pro, gemini-for-google-2.5-pro)
// - OpenAI models: 5+ models (gpt-4o, gpt-4o-mini, gpt-4-turbo, gpt-3.5-turbo, o1-preview)
// - Metadata validation: ideType, ideVersion, platform, architecture
// - Integration tests: Request payload validation

use serde_json::{json, Value};

// Import wrapper functions from mappers
use antigravity_tools_lib::proxy::mappers::gemini::wrapper::wrap_request;
use antigravity_tools_lib::proxy::mappers::openai::models::*;
use antigravity_tools_lib::proxy::mappers::openai::request::transform_openai_request;

/// Helper function to validate metadata structure
fn validate_metadata(metadata: &Value) {
    // Validate ideType field
    assert!(
        metadata.get("ideType").is_some(),
        "metadata must contain 'ideType' field"
    );
    assert_eq!(
        metadata["ideType"].as_str().unwrap(),
        "ANTIGRAVITY",
        "ideType must be 'ANTIGRAVITY'"
    );

    // Validate ideVersion field
    assert!(
        metadata.get("ideVersion").is_some(),
        "metadata must contain 'ideVersion' field"
    );
    assert_eq!(
        metadata["ideVersion"].as_str().unwrap(),
        "1.13.3",
        "ideVersion must be '1.13.3'"
    );

    // Validate platform field
    assert!(
        metadata.get("platform").is_some(),
        "metadata must contain 'platform' field"
    );
    let platform = metadata["platform"].as_str().unwrap();
    assert!(
        platform == "darwin" || platform == "windows" || platform == "linux",
        "platform must be one of: darwin, windows, linux. Got: {}",
        platform
    );

    // Validate architecture field
    assert!(
        metadata.get("architecture").is_some(),
        "metadata must contain 'architecture' field"
    );
    let arch = metadata["architecture"].as_str().unwrap();
    assert!(
        arch == "arm64" || arch == "x86_64",
        "architecture must be one of: arm64, x86_64. Got: {}",
        arch
    );
}

// ================================
// GEMINI MODEL TESTS (5+ models)
// ================================

#[test]
fn test_gemini_2_0_flash_exp_has_ideType() {
    let body = json!({
        "model": "gemini-2.0-flash-exp",
        "contents": [{"role": "user", "parts": [{"text": "Hello"}]}]
    });

    let result = wrap_request(&body, "test-project", "gemini-2.0-flash-exp");

    // Check that request.metadata exists
    let request = result
        .get("request")
        .expect("result should have 'request' field");
    let metadata = request
        .get("metadata")
        .expect("request should have 'metadata' field");

    validate_metadata(metadata);
}

#[test]
fn test_gemini_2_5_flash_has_ideType() {
    let body = json!({
        "model": "gemini-2.5-flash",
        "contents": [{"role": "user", "parts": [{"text": "Hello"}]}]
    });

    let result = wrap_request(&body, "test-project", "gemini-2.5-flash");

    let request = result
        .get("request")
        .expect("result should have 'request' field");
    let metadata = request
        .get("metadata")
        .expect("request should have 'metadata' field");

    validate_metadata(metadata);
}

#[test]
fn test_gemini_2_5_flash_lite_has_ideType() {
    let body = json!({
        "model": "gemini-2.5-flash-lite",
        "contents": [{"role": "user", "parts": [{"text": "Hello"}]}]
    });

    let result = wrap_request(&body, "test-project", "gemini-2.5-flash-lite");

    let request = result
        .get("request")
        .expect("result should have 'request' field");
    let metadata = request
        .get("metadata")
        .expect("request should have 'metadata' field");

    validate_metadata(metadata);
}

#[test]
fn test_gemini_2_5_pro_has_ideType() {
    let body = json!({
        "model": "gemini-2.5-pro",
        "contents": [{"role": "user", "parts": [{"text": "Hello"}]}]
    });

    let result = wrap_request(&body, "test-project", "gemini-2.5-pro");

    let request = result
        .get("request")
        .expect("result should have 'request' field");
    let metadata = request
        .get("metadata")
        .expect("request should have 'metadata' field");

    validate_metadata(metadata);
}

#[test]
fn test_gemini_for_google_2_5_pro_has_ideType() {
    let body = json!({
        "model": "gemini-for-google-2.5-pro",
        "contents": [{"role": "user", "parts": [{"text": "Hello"}]}]
    });

    let result = wrap_request(&body, "test-project", "gemini-for-google-2.5-pro");

    let request = result
        .get("request")
        .expect("result should have 'request' field");
    let metadata = request
        .get("metadata")
        .expect("request should have 'metadata' field");

    validate_metadata(metadata);
}

// ================================
// OPENAI MODEL TESTS (5+ models)
// ================================

#[test]
fn test_gpt_4o_has_ideType() {
    let req = OpenAIRequest {
        model: "gpt-4o".to_string(),
        messages: vec![OpenAIMessage {
            role: "user".to_string(),
            content: Some(OpenAIContent::String("Hello".to_string())),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }],
        stream: false,
        n: None,
        max_tokens: None,
        temperature: None,
        top_p: None,
        stop: None,
        response_format: None,
        tools: None,
        tool_choice: None,
        parallel_tool_calls: None,
        reasoning_effort: None,
        instructions: None,
        input: None,
        prompt: None,
    };

    let result = transform_openai_request(&req, "test-project", "gemini-2.5-flash")
        .expect("transform should succeed");

    let request = result
        .get("request")
        .expect("result should have 'request' field");
    let metadata = request
        .get("metadata")
        .expect("request should have 'metadata' field");

    validate_metadata(metadata);
}

#[test]
fn test_gpt_4o_mini_has_ideType() {
    let req = OpenAIRequest {
        model: "gpt-4o-mini".to_string(),
        messages: vec![OpenAIMessage {
            role: "user".to_string(),
            content: Some(OpenAIContent::String("Hello".to_string())),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }],
        stream: false,
        n: None,
        max_tokens: None,
        temperature: None,
        top_p: None,
        stop: None,
        response_format: None,
        tools: None,
        tool_choice: None,
        parallel_tool_calls: None,
        reasoning_effort: None,
        instructions: None,
        input: None,
        prompt: None,
    };

    let result = transform_openai_request(&req, "test-project", "gemini-2.5-flash")
        .expect("transform should succeed");

    let request = result
        .get("request")
        .expect("result should have 'request' field");
    let metadata = request
        .get("metadata")
        .expect("request should have 'metadata' field");

    validate_metadata(metadata);
}

#[test]
fn test_gpt_4_turbo_has_ideType() {
    let req = OpenAIRequest {
        model: "gpt-4-turbo".to_string(),
        messages: vec![OpenAIMessage {
            role: "user".to_string(),
            content: Some(OpenAIContent::String("Hello".to_string())),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }],
        stream: false,
        n: None,
        max_tokens: None,
        temperature: None,
        top_p: None,
        stop: None,
        response_format: None,
        tools: None,
        tool_choice: None,
        parallel_tool_calls: None,
        reasoning_effort: None,
        instructions: None,
        input: None,
        prompt: None,
    };

    let result = transform_openai_request(&req, "test-project", "gemini-2.5-flash")
        .expect("transform should succeed");

    let request = result
        .get("request")
        .expect("result should have 'request' field");
    let metadata = request
        .get("metadata")
        .expect("request should have 'metadata' field");

    validate_metadata(metadata);
}

#[test]
fn test_gpt_3_5_turbo_has_ideType() {
    let req = OpenAIRequest {
        model: "gpt-3.5-turbo".to_string(),
        messages: vec![OpenAIMessage {
            role: "user".to_string(),
            content: Some(OpenAIContent::String("Hello".to_string())),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }],
        stream: false,
        n: None,
        max_tokens: None,
        temperature: None,
        top_p: None,
        stop: None,
        response_format: None,
        tools: None,
        tool_choice: None,
        parallel_tool_calls: None,
        reasoning_effort: None,
        instructions: None,
        input: None,
        prompt: None,
    };

    let result = transform_openai_request(&req, "test-project", "gemini-2.5-flash")
        .expect("transform should succeed");

    let request = result
        .get("request")
        .expect("result should have 'request' field");
    let metadata = request
        .get("metadata")
        .expect("request should have 'metadata' field");

    validate_metadata(metadata);
}

#[test]
fn test_o1_preview_has_ideType() {
    let req = OpenAIRequest {
        model: "o1-preview".to_string(),
        messages: vec![OpenAIMessage {
            role: "user".to_string(),
            content: Some(OpenAIContent::String("Hello".to_string())),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }],
        stream: false,
        n: None,
        max_tokens: None,
        temperature: None,
        top_p: None,
        stop: None,
        response_format: None,
        tools: None,
        tool_choice: None,
        parallel_tool_calls: None,
        reasoning_effort: None,
        instructions: None,
        input: None,
        prompt: None,
    };

    let result = transform_openai_request(&req, "test-project", "gemini-2.5-flash")
        .expect("transform should succeed");

    let request = result
        .get("request")
        .expect("result should have 'request' field");
    let metadata = request
        .get("metadata")
        .expect("request should have 'metadata' field");

    validate_metadata(metadata);
}

// ================================
// INTEGRATION TESTS (5 tests)
// ================================

#[test]
fn test_gemini_metadata_persists_through_wrapper() {
    // Test that metadata survives the full wrap_request pipeline
    let body = json!({
        "model": "gemini-2.5-flash",
        "contents": [{"role": "user", "parts": [{"text": "Test"}]}],
        "generationConfig": {"temperature": 0.7}
    });

    let result = wrap_request(&body, "test-project", "gemini-2.5-flash");

    // Verify metadata exists at correct location
    assert!(result.get("request").is_some());
    assert!(result["request"].get("metadata").is_some());

    let metadata = &result["request"]["metadata"];
    validate_metadata(metadata);

    // Verify other fields still present
    assert_eq!(result["model"], "gemini-2.5-flash");
    assert_eq!(result["project"], "test-project");
}

#[test]
fn test_openai_metadata_persists_through_transform() {
    // Test that metadata survives the full transform_openai_request pipeline
    let req = OpenAIRequest {
        model: "gpt-4o".to_string(),
        messages: vec![OpenAIMessage {
            role: "user".to_string(),
            content: Some(OpenAIContent::String("Test".to_string())),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }],
        stream: false,
        n: None,
        max_tokens: Some(1000),
        temperature: Some(0.7),
        top_p: None,
        stop: None,
        response_format: None,
        tools: None,
        tool_choice: None,
        parallel_tool_calls: None,
        reasoning_effort: None,
        instructions: None,
        input: None,
        prompt: None,
    };

    let result = transform_openai_request(&req, "test-project", "gemini-2.5-flash")
        .expect("transform should succeed");

    // Verify metadata exists at correct location
    assert!(result.get("request").is_some());
    assert!(result["request"].get("metadata").is_some());

    let metadata = &result["request"]["metadata"];
    validate_metadata(metadata);

    // Verify other fields still present
    assert_eq!(result["model"], "gemini-2.5-flash");
    assert_eq!(result["project"], "test-project");
}

#[test]
fn test_metadata_includes_all_required_fields() {
    // Comprehensive check that all metadata fields are present and valid
    let body = json!({
        "model": "gemini-2.5-flash",
        "contents": [{"role": "user", "parts": [{"text": "Test"}]}]
    });

    let result = wrap_request(&body, "test-project", "gemini-2.5-flash");
    let metadata = &result["request"]["metadata"];

    // Check all required fields
    let required_fields = vec!["ideType", "ideVersion", "platform", "architecture"];
    for field in required_fields {
        assert!(
            metadata.get(field).is_some(),
            "metadata must contain '{}' field",
            field
        );
        assert!(
            metadata[field].is_string(),
            "'{}' field must be a string",
            field
        );
        assert!(
            !metadata[field].as_str().unwrap().is_empty(),
            "'{}' field must not be empty",
            field
        );
    }
}

#[test]
fn test_metadata_format_matches_claude_implementation() {
    // Ensure Gemini/OpenAI metadata matches Claude's format exactly
    let body = json!({
        "model": "gemini-2.5-flash",
        "contents": [{"role": "user", "parts": [{"text": "Test"}]}]
    });

    let result = wrap_request(&body, "test-project", "gemini-2.5-flash");
    let metadata = &result["request"]["metadata"];

    // Verify field names (camelCase, not snake_case)
    assert!(
        metadata.get("ideType").is_some(),
        "field must be 'ideType' (camelCase)"
    );
    assert!(
        metadata.get("ideVersion").is_some(),
        "field must be 'ideVersion' (camelCase)"
    );
    assert!(
        metadata.get("architecture").is_some(),
        "field must be 'architecture' (not 'arch')"
    );

    // Verify values match constants
    assert_eq!(metadata["ideType"], "ANTIGRAVITY");
    assert_eq!(metadata["ideVersion"], "1.13.3");
}

#[test]
fn test_metadata_injection_does_not_break_existing_functionality() {
    // Regression test: Ensure metadata addition doesn't break existing features
    let body = json!({
        "model": "gemini-2.5-flash",
        "contents": [{"role": "user", "parts": [{"text": "Test"}]}],
        "generationConfig": {
            "temperature": 0.9,
            "topP": 0.95,
            "maxOutputTokens": 2048
        },
        "safetySettings": []
    });

    let result = wrap_request(&body, "test-project", "gemini-2.5-flash");

    // Verify metadata was added
    assert!(result["request"].get("metadata").is_some());

    // Verify existing fields still present and unchanged
    let request = &result["request"];
    assert!(request.get("contents").is_some());
    assert!(request.get("generationConfig").is_some());
    assert_eq!(request["generationConfig"]["temperature"], 0.9);
    assert_eq!(request["generationConfig"]["topP"], 0.95);
}
