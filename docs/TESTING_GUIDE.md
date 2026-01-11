# Testing Guide - Gemini 3 Integration

**Version**: 3.5.0
**Last Updated**: 2026-01-11
**Purpose**: Comprehensive testing guide for Gemini 3 thinking features

---

## Overview

This guide covers all testing procedures for Gemini 3 API integration, including unit tests, integration tests, and end-to-end validation.

---

## Test Structure

```
src-tauri/src/proxy/
├── mappers/
│   └── common/
│       └── tests/
│           ├── gemini_detection_tests.rs
│           ├── thinking_level_mapper_tests.rs
│           └── gemini_api_validator_tests.rs
└── tests/
    ├── thinking_models.rs
    ├── gemini_3_flash_integration.rs
    ├── gemini_3_pro_integration.rs
    └── gemini_3_e2e_protocol.rs
```

---

## Running Tests

### All Gemini Tests
```bash
cargo test --lib gemini
```

**Expected Output**:
```
running 298 tests
test gemini_detection::test_gemini_3_detection ... ok
test gemini_detection::test_gemini_3_flash_detection ... ok
test thinking_level_mapper::test_flash_mapping ... ok
test thinking_level_mapper::test_pro_mapping ... ok
test gemini_api_validator::test_api_format_validation ... ok
...
test result: ok. 298 passed; 0 failed; 0 ignored
```

---

### Specific Test Categories

**Detection Tests**:
```bash
cargo test --lib gemini_detection
```

**Mapping Tests**:
```bash
cargo test --lib thinking_level_mapper
```

**Validation Tests**:
```bash
cargo test --lib gemini_api_validator
```

**Integration Tests**:
```bash
cargo test --lib gemini_3_flash_integration
cargo test --lib gemini_3_pro_integration
```

**E2E Protocol Tests**:
```bash
cargo test --lib gemini_3_e2e_protocol
```

---

### Coverage Analysis

**Run Coverage**:
```bash
cargo tarpaulin --lib --tests --out Stdout --exclude-files "tests/*"
```

**Expected Coverage**:
```
|| Tested/Total Lines:
|| src/proxy/mappers/common/gemini_detection.rs: 100.00%
|| src/proxy/mappers/common/thinking_level_mapper.rs: 95.83%
|| src/proxy/mappers/common/gemini_api_validator.rs: 92.31%
|| src/proxy/mappers/openai/request.rs: 87.45%
|| src/proxy/mappers/claude/request.rs: 88.12%
||
|| Total: 90.24%
```

---

## Unit Tests

### Detection Module Tests

**File**: `gemini_detection_tests.rs`

**Test Coverage**:
```rust
#[cfg(test)]
mod detection_tests {
    use super::*;

    #[test]
    fn test_gemini_3_model_detection() {
        // Positive cases
        assert!(is_gemini_3_model("gemini-3-flash"));
        assert!(is_gemini_3_model("gemini-3-pro-high"));
        assert!(is_gemini_3_model("gemini-3-pro-low"));

        // Negative cases
        assert!(!is_gemini_3_model("gemini-2.5-flash"));
        assert!(!is_gemini_3_model("claude-sonnet-4.5"));
    }

    #[test]
    fn test_gemini_3_flash_detection() {
        assert!(is_gemini_3_flash("gemini-3-flash"));
        assert!(!is_gemini_3_flash("gemini-3-pro-high"));
        assert!(!is_gemini_3_flash("gemini-2.5-flash"));
    }

    #[test]
    fn test_gemini_3_pro_detection() {
        assert!(is_gemini_3_pro("gemini-3-pro-high"));
        assert!(is_gemini_3_pro("gemini-3-pro-low"));
        assert!(!is_gemini_3_pro("gemini-3-flash"));
    }

    #[test]
    fn test_gemini_2_5_detection() {
        assert!(is_gemini_2_5_model("gemini-2.5-flash-thinking"));
        assert!(is_gemini_2_5_model("gemini-2.5-pro-thinking"));
        assert!(!is_gemini_2_5_model("gemini-3-flash"));
    }

    #[test]
    fn test_edge_cases() {
        // Empty string
        assert!(!is_gemini_3_model(""));

        // Partial matches
        assert!(!is_gemini_3_model("gemini-3"));  // Incomplete
        assert!(!is_gemini_3_model("3-flash"));   // Missing prefix

        // Case sensitivity
        assert!(!is_gemini_3_model("Gemini-3-flash"));  // Uppercase
    }
}
```

**Run**:
```bash
cargo test --lib gemini_detection
```

---

### Mapping Module Tests

**File**: `thinking_level_mapper_tests.rs`

**Test Coverage**:
```rust
#[cfg(test)]
mod mapper_tests {
    use super::*;

    #[test]
    fn test_flash_minimal_mapping() {
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(0)),
            "MINIMAL"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(4000)),
            "MINIMAL"
        );
    }

    #[test]
    fn test_flash_low_mapping() {
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(4001)),
            "LOW"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(10000)),
            "LOW"
        );
    }

    #[test]
    fn test_flash_medium_mapping() {
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(10001)),
            "MEDIUM"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(20000)),
            "MEDIUM"
        );
    }

    #[test]
    fn test_flash_high_mapping() {
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(20001)),
            "HIGH"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(32000)),
            "HIGH"
        );
    }

    #[test]
    fn test_pro_low_mapping() {
        assert_eq!(
            determine_thinking_level("gemini-3-pro-high", Some(0)),
            "LOW"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-pro-high", Some(16000)),
            "LOW"
        );
    }

    #[test]
    fn test_pro_high_mapping() {
        assert_eq!(
            determine_thinking_level("gemini-3-pro-high", Some(16001)),
            "HIGH"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-pro-high", Some(32000)),
            "HIGH"
        );
    }

    #[test]
    fn test_default_levels() {
        assert_eq!(
            get_default_thinking_level("gemini-3-flash"),
            "MEDIUM"
        );
        assert_eq!(
            get_default_thinking_level("gemini-3-pro-high"),
            "HIGH"
        );
    }

    #[test]
    fn test_none_budget() {
        // Should use default levels when budget is None
        assert_eq!(
            determine_thinking_level("gemini-3-flash", None),
            "MEDIUM"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-pro-high", None),
            "HIGH"
        );
    }

    #[test]
    fn test_boundary_values() {
        // Test exact boundary values
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(4000)),
            "MINIMAL"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(4001)),
            "LOW"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(10001)),
            "MEDIUM"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(20001)),
            "HIGH"
        );
    }
}
```

**Run**:
```bash
cargo test --lib thinking_level_mapper
```

---

### Validation Module Tests

**File**: `gemini_api_validator_tests.rs`

**Test Coverage**:
```rust
#[cfg(test)]
mod validator_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_gemini_3_with_thinking_level() {
        let request = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingLevel": "HIGH"
                }
            }
        });

        assert!(validate_gemini_request("gemini-3-flash", &request).is_ok());
    }

    #[test]
    fn test_gemini_3_with_thinking_budget_fails() {
        let request = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingBudget": 16000
                }
            }
        });

        let result = validate_gemini_request("gemini-3-flash", &request);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must use thinkingLevel"));
    }

    #[test]
    fn test_gemini_2_5_with_thinking_budget() {
        let request = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingBudget": 16000
                }
            }
        });

        assert!(validate_gemini_request("gemini-2.5-flash-thinking", &request).is_ok());
    }

    #[test]
    fn test_gemini_2_5_with_thinking_level_fails() {
        let request = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingLevel": "HIGH"
                }
            }
        });

        let result = validate_gemini_request("gemini-2.5-flash-thinking", &request);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must use thinkingBudget"));
    }

    #[test]
    fn test_flash_medium_level_valid() {
        let result = validate_thinking_level("gemini-3-flash", "MEDIUM");
        assert!(result.is_ok());
    }

    #[test]
    fn test_pro_medium_level_invalid() {
        let result = validate_thinking_level("gemini-3-pro-high", "MEDIUM");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid thinkingLevel: 'MEDIUM'"));
    }

    #[test]
    fn test_all_flash_levels_valid() {
        for level in &["MINIMAL", "LOW", "MEDIUM", "HIGH"] {
            assert!(validate_thinking_level("gemini-3-flash", level).is_ok());
        }
    }

    #[test]
    fn test_pro_valid_levels() {
        assert!(validate_thinking_level("gemini-3-pro-high", "LOW").is_ok());
        assert!(validate_thinking_level("gemini-3-pro-high", "HIGH").is_ok());
    }

    #[test]
    fn test_invalid_level_values() {
        assert!(validate_thinking_level("gemini-3-flash", "INVALID").is_err());
        assert!(validate_thinking_level("gemini-3-flash", "low").is_err());  // Wrong case
        assert!(validate_thinking_level("gemini-3-flash", "").is_err());
    }

    #[test]
    fn test_no_thinking_config_passes() {
        let request = json!({
            "generationConfig": {
                "temperature": 0.7
            }
        });

        // Should pass validation when no thinking config present
        assert!(validate_gemini_request("gemini-3-flash", &request).is_ok());
    }
}
```

**Run**:
```bash
cargo test --lib gemini_api_validator
```

---

## Integration Tests

### Flash Integration Tests

**File**: `gemini_3_flash_integration.rs`

**Test Coverage**:
```rust
#[cfg(test)]
mod flash_integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_flash_auto_injection_openai() {
        let client = TestClient::new();

        let response = client
            .post("/v1/chat/completions")
            .json(&json!({
                "model": "gemini-3-flash",
                "messages": [
                    {"role": "user", "content": "Test"}
                ]
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);

        // Verify thinking content present
        let body: Value = response.json().await.unwrap();
        assert!(body["choices"][0]["message"]["reasoning_content"].is_string());
    }

    #[tokio::test]
    async fn test_flash_with_budget_openai() {
        let client = TestClient::new();

        let response = client
            .post("/v1/chat/completions")
            .json(&json!({
                "model": "gemini-3-flash",
                "messages": [
                    {"role": "user", "content": "Test"}
                ],
                "thinking_budget": 5000
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_flash_minimal_level() {
        let client = TestClient::new();

        let response = client
            .post("/v1/chat/completions")
            .json(&json!({
                "model": "gemini-3-flash",
                "messages": [{"role": "user", "content": "Quick test"}],
                "thinking_budget": 2000  // Should map to MINIMAL
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_flash_medium_level_claude() {
        let client = TestClient::new();

        let response = client
            .post("/v1/messages")
            .header("x-api-key", "sk-antigravity")
            .json(&json!({
                "model": "gemini-3-flash",
                "messages": [
                    {"role": "user", "content": "Test"}
                ],
                "thinking": {
                    "type": "enabled",
                    "budget": 15000
                }
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);

        // Verify thinking blocks present
        let body: Value = response.json().await.unwrap();
        let has_thinking = body["content"]
            .as_array()
            .unwrap()
            .iter()
            .any(|block| block["type"] == "thinking");
        assert!(has_thinking);
    }
}
```

**Run**:
```bash
cargo test --lib gemini_3_flash_integration
```

---

### Pro Integration Tests

**File**: `gemini_3_pro_integration.rs`

**Test Coverage**:
```rust
#[cfg(test)]
mod pro_integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_pro_high_auto_injection() {
        let client = TestClient::new();

        let response = client
            .post("/v1/chat/completions")
            .json(&json!({
                "model": "gemini-3-pro-high",
                "messages": [
                    {"role": "user", "content": "Analyze"}
                ]
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_pro_low_level() {
        let client = TestClient::new();

        let response = client
            .post("/v1/chat/completions")
            .json(&json!({
                "model": "gemini-3-pro-high",
                "messages": [{"role": "user", "content": "Test"}],
                "thinking_budget": 10000  // Should map to LOW
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_pro_high_level() {
        let client = TestClient::new();

        let response = client
            .post("/v1/chat/completions")
            .json(&json!({
                "model": "gemini-3-pro-high",
                "messages": [{"role": "user", "content": "Deep analysis"}],
                "thinking_budget": 25000  // Should map to HIGH
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_pro_medium_level_fails() {
        let client = TestClient::new();

        // Pro doesn't support MEDIUM, should fail validation
        let response = client
            .post("/v1/native/gemini")
            .json(&json!({
                "model": "gemini-3-pro-high",
                "generationConfig": {
                    "thinkingConfig": {
                        "thinkingLevel": "MEDIUM"
                    }
                }
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 400);
        let body: Value = response.json().await.unwrap();
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("invalid thinkingLevel"));
    }
}
```

**Run**:
```bash
cargo test --lib gemini_3_pro_integration
```

---

## E2E Protocol Tests

**File**: `gemini_3_e2e_protocol.rs`

**Test Coverage**:
```rust
#[cfg(test)]
mod e2e_tests {
    use super::*;

    #[tokio::test]
    async fn test_openai_to_gemini_conversion() {
        let client = TestClient::new();

        // Send OpenAI format
        let response = client
            .post("/v1/chat/completions")
            .json(&json!({
                "model": "gemini-3-flash",
                "messages": [{"role": "user", "content": "Test"}],
                "thinking_budget": 15000
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);

        // Verify OpenAI format response
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["object"], "chat.completion");
        assert!(body["choices"][0]["message"]["reasoning_content"].is_string());
    }

    #[tokio::test]
    async fn test_claude_to_gemini_conversion() {
        let client = TestClient::new();

        // Send Claude format
        let response = client
            .post("/v1/messages")
            .header("x-api-key", "sk-antigravity")
            .json(&json!({
                "model": "gemini-3-flash",
                "messages": [{"role": "user", "content": "Test"}],
                "thinking": {
                    "type": "enabled",
                    "budget": 15000
                }
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);

        // Verify Claude format response
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["type"], "message");
        let has_thinking = body["content"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["type"] == "thinking");
        assert!(has_thinking);
    }

    #[tokio::test]
    async fn test_budget_to_level_conversion_accuracy() {
        let test_cases = vec![
            (3000, "MINIMAL"),
            (8000, "LOW"),
            (15000, "MEDIUM"),
            (25000, "HIGH"),
        ];

        let client = TestClient::new();

        for (budget, expected_level) in test_cases {
            // Capture upstream request
            let upstream_request = capture_upstream_request(&client, budget).await;

            let actual_level = upstream_request["generationConfig"]["thinkingConfig"]["thinkingLevel"]
                .as_str()
                .unwrap();

            assert_eq!(actual_level, expected_level,
                "Budget {} should map to {}", budget, expected_level);
        }
    }
}
```

**Run**:
```bash
cargo test --lib gemini_3_e2e_protocol
```

---

## Test Execution Matrix

### Complete Test Run

```bash
#!/bin/bash

echo "=== Running Gemini 3 Test Suite ==="

echo "
[1/6] Detection Tests..."
cargo test --lib gemini_detection

echo "\n[2/6] Mapping Tests..."
cargo test --lib thinking_level_mapper

echo "\n[3/6] Validation Tests..."
cargo test --lib gemini_api_validator

echo "\n[4/6] Flash Integration Tests..."
cargo test --lib gemini_3_flash_integration

echo "\n[5/6] Pro Integration Tests..."
cargo test --lib gemini_3_pro_integration

echo "\n[6/6] E2E Protocol Tests..."
cargo test --lib gemini_3_e2e_protocol

echo "\n=== Test Suite Complete ==="
```

**Save as**: `test-gemini-3.sh`

**Run**:
```bash
chmod +x test-gemini-3.sh
./test-gemini-3.sh
```

---

## Test Results Validation

### Expected Test Metrics

**Total Tests**: 298
**Pass Rate**: 100% (0 failures)
**Coverage**: 90%+
**Execution Time**: <30 seconds

### Regression Test Checklist

- [ ] All detection tests pass
- [ ] All mapping tests pass
- [ ] All validation tests pass
- [ ] Flash integration tests pass
- [ ] Pro integration tests pass
- [ ] E2E protocol tests pass
- [ ] Backward compatibility maintained (Gemini 2.5 tests still pass)
- [ ] No new warnings or clippy issues

---

## Troubleshooting Test Failures

### Common Issues

**Issue 1**: Detection tests fail

**Cause**: Model name pattern changed

**Fix**: Update detection logic in `gemini_detection.rs`

---

**Issue 2**: Mapping tests fail

**Cause**: Budget ranges modified

**Fix**: Update mapping ranges in `thinking_level_mapper.rs` and tests

---

**Issue 3**: Integration tests timeout

**Cause**: Network issues or upstream API down

**Fix**: Use mock server for testing, avoid live API calls

---

**Issue 4**: E2E tests fail intermittently

**Cause**: Race conditions or shared state

**Fix**: Use isolated test instances, reset state between tests

---

## Continuous Integration

### CI Configuration

**GitHub Actions**:
```yaml
name: Gemini 3 Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - name: Run Gemini 3 Tests
        run: |
          cargo test --lib gemini
      - name: Run Coverage
        run: |
          cargo tarpaulin --lib --out Stdout --exclude-files "tests/*"
```

---

## References

- **Developer Guide**: `docs/DEVELOPER_GUIDE.md`
- **API Reference**: `docs/API_REFERENCE.md`
- **Technical Specification**: `docs/technical-specs/GEMINI_API_ANALYSIS.md`

---

**Document Status**: ✅ Complete
**Last Updated**: 2026-01-11 (Epic-011 Story-006)
**Next Review**: Test suite expansion for Gemini 4.x
