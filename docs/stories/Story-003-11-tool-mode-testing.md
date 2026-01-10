# Story-003-11: Tool Mode Testing and Feature Parity Validation

**Story ID**: Story-003-11
**Epic**: [Epic-003](../epics/Epic-003-Claude-4.5-Sonnet-Thinking-Compliance.md) - Claude 4.5 Sonnet Thinking - 100% Compliance
**Priority**: P2 (High - Testing)
**Estimated Effort**: 2 hours
**Status**: Pending
**Created**: 2026-01-10
**Owner**: Engineering Team

---

## User Story

**As a** developer completing Epic-003 Phase 3 implementation
**I want** comprehensive end-to-end testing of tool modes and grounding configuration
**So that** I can confidently validate 100% feature parity and compliance with Antigravity baseline

---

## Context

### Testing Scope

This story validates **Phase 3 (Feature Parity)** implementation:

**Story-003-09** (Flexible Tool Configuration Modes):
- ✅ ToolChoice enum with 4 variants (AUTO, ANY, NONE, Tool{name})
- ✅ Dynamic mode selection in request mapper
- ✅ allowedFunctionNames for tool forcing

**Story-003-10** (Grounding Configuration):
- ✅ geminiSettings.recitationPolicy always present
- ✅ action="BLOCK", threshold="LOW"

**Compliance Target**: 95% → **100%**

### Why Comprehensive Testing is Critical

**Anti-Detection Validation**:
1. **Tool Modes**: Ensure all 4 modes match Gemini Protocol exactly
2. **geminiSettings**: Verify always present (fingerprinting prevention)
3. **Request Structure**: Confirm indistinguishable from Antigravity
4. **Backend Responses**: Validate upstream API accepts our requests

**Regression Prevention**:
1. **Backward Compatibility**: Existing workflows must work unchanged
2. **No Breaking Changes**: Previous stories' features must remain functional
3. **Performance**: No degradation from new features

**Compliance Verification**:
1. **Gap Closure**: Validate all 8 gaps from Gap Analysis are resolved
2. **100% Compliance**: Confirm matching Antigravity baseline exactly
3. **Manual Validation**: Live endpoint testing against RE specification

### Current Situation

**Implementation Complete** (Stories 009, 010):
- ✅ ToolChoice enum implemented
- ✅ Tool mode mapping implemented
- ✅ geminiSettings added to all requests

**Testing Needed**:
- ❌ No end-to-end validation of tool modes
- ❌ No integration testing with live Vertex AI
- ❌ No compliance verification against RE spec
- ❌ No manual testing scenarios documented

**Risk Without Testing**:
- ⚠️ Tool modes may not work correctly end-to-end
- ⚠️ geminiSettings may have incorrect format
- ⚠️ Backend may reject requests
- ⚠️ Compliance score may not reach 100%

---

## Reference Documentation

### Primary Dependencies
- **Story-003-09**: `docs/stories/Story-003-09-flexible-tool-configuration-modes.md`
  - Lines 751-1022: Testing Strategy (9 unit + 2 integration tests)
  - Lines 200-515: Implementation details for each mode

- **Story-003-10**: `docs/stories/Story-003-10-grounding-configuration.md`
  - Lines 460-640: Testing Strategy (5 unit + 1 integration test)
  - Lines 150-280: geminiSettings implementation

### Gap Analysis Reference
- **Document**: `docs/comparison/claude/claude-4-5-sonnet/current-implementation-thinking.md`
  - Lines 3730-3806: Testing Strategy and Compliance Verification
  - Lines 3797-3805: Compliance checklist

### Epic Success Criteria
- **Epic-003**: `docs/epics/Epic-003-Claude-4.5-Sonnet-Thinking-Compliance.md`
  - Lines 140-143: Phase 3 success criteria
  - Lines 232-260: Epic-level Definition of Done

---

## Technical Details

### Test Architecture

**Three-Layer Testing Strategy**:
```
┌─────────────────────────────────────────────────────────────┐
│ Layer 1: Unit Tests (Already in Stories 009, 010)           │
│ ✅ ToolChoice enum                                           │
│ ✅ Mode mapping logic                                        │
│ ✅ geminiSettings structure                                  │
└─────────────────────────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│ Layer 2: Integration Tests (This Story)                     │
│ 🆕 End-to-end tool mode workflows                           │
│ 🆕 Live Vertex AI endpoint testing                          │
│ 🆕 Request/response validation                              │
└─────────────────────────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│ Layer 3: Manual Validation (This Story)                     │
│ 🆕 Compliance verification against RE spec                  │
│ 🆕 Backend behavior validation                              │
│ 🆕 Real-world scenario testing                              │
└─────────────────────────────────────────────────────────────┘
```

### Test Scenarios

#### Scenario 1: AUTO Mode End-to-End

**Goal**: Verify model autonomously decides when to use tools

**Test Case**:
```rust
#[tokio::test]
async fn test_e2e_tool_mode_auto() {
    let request = json!({
        "model": "claude-4.5-sonnet-thinking",
        "messages": [{
            "role": "user",
            "content": "What's the weather in Tokyo? Use the weather tool if available."
        }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get current weather for a location",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"}
                    },
                    "required": ["location"]
                }
            }
        }],
        "tool_choice": {"type": "auto"},
        "max_tokens": 1024
    });

    let response = send_claude_request(request).await.unwrap();

    // Verify toolConfig in upstream request
    let captured_request = get_last_upstream_request();
    assert_eq!(
        captured_request["toolConfig"]["functionCallingConfig"]["mode"],
        "AUTO"
    );

    // Verify model could use tool (or not, at its discretion)
    assert!(response.is_ok());
}
```

#### Scenario 2: ANY Mode End-to-End

**Goal**: Verify model MUST call at least one tool

**Test Case**:
```rust
#[tokio::test]
async fn test_e2e_tool_mode_any() {
    let request = json!({
        "model": "claude-4.5-sonnet-thinking",
        "messages": [{
            "role": "user",
            "content": "Get structured data using the tool"
        }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_data",
                "description": "Get structured data",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"}
                    }
                }
            }
        }],
        "tool_choice": {"type": "any"},
        "max_tokens": 1024
    });

    let response = send_claude_request(request).await.unwrap();

    // Verify toolConfig.mode = "ANY"
    let captured_request = get_last_upstream_request();
    assert_eq!(
        captured_request["toolConfig"]["functionCallingConfig"]["mode"],
        "ANY"
    );

    // Verify model called at least one tool
    let response_content = response["content"].as_array().unwrap();
    let has_tool_use = response_content.iter().any(|block| {
        block["type"] == "tool_use"
    });
    assert!(has_tool_use, "Model must call at least one tool with ANY mode");
}
```

#### Scenario 3: NONE Mode End-to-End

**Goal**: Verify tools are completely disabled

**Test Case**:
```rust
#[tokio::test]
async fn test_e2e_tool_mode_none() {
    let request = json!({
        "model": "claude-4.5-sonnet-thinking",
        "messages": [{
            "role": "user",
            "content": "Tell me about the weather (do NOT use tools)"
        }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get weather",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }],
        "tool_choice": {"type": "none"},
        "max_tokens": 1024
    });

    let response = send_claude_request(request).await.unwrap();

    // Verify toolConfig.mode = "NONE"
    let captured_request = get_last_upstream_request();
    assert_eq!(
        captured_request["toolConfig"]["functionCallingConfig"]["mode"],
        "NONE"
    );

    // Verify model did NOT call any tools
    let response_content = response["content"].as_array().unwrap();
    let has_tool_use = response_content.iter().any(|block| {
        block["type"] == "tool_use"
    });
    assert!(!has_tool_use, "Model must NOT call tools with NONE mode");
}
```

#### Scenario 4: Tool Forcing (VALIDATED + allowedFunctionNames)

**Goal**: Verify model calls only the specified tool

**Test Case**:
```rust
#[tokio::test]
async fn test_e2e_tool_forcing() {
    let request = json!({
        "model": "claude-4.5-sonnet-thinking",
        "messages": [{
            "role": "user",
            "content": "Get me some data"
        }],
        "tools": [
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather",
                    "parameters": {"type": "object", "properties": {}}
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "get_stock_price",
                    "description": "Get stock price",
                    "parameters": {"type": "object", "properties": {}}
                }
            }
        ],
        "tool_choice": {
            "type": "tool",
            "name": "get_stock_price"
        },
        "max_tokens": 1024
    });

    let response = send_claude_request(request).await.unwrap();

    // Verify toolConfig
    let captured_request = get_last_upstream_request();
    assert_eq!(
        captured_request["toolConfig"]["functionCallingConfig"]["mode"],
        "VALIDATED"
    );
    assert_eq!(
        captured_request["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"],
        json!(["get_stock_price"])
    );

    // Verify model called ONLY get_stock_price
    let response_content = response["content"].as_array().unwrap();
    let tool_calls: Vec<&str> = response_content.iter()
        .filter_map(|block| {
            if block["type"] == "tool_use" {
                block["name"].as_str()
            } else {
                None
            }
        })
        .collect();

    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0], "get_stock_price");
}
```

#### Scenario 5: geminiSettings Presence Validation

**Goal**: Verify geminiSettings always present regardless of configuration

**Test Cases**:
```rust
#[tokio::test]
async fn test_gemini_settings_always_present() {
    // Test 1: Request with AUTO mode
    let req1 = json!({
        "model": "claude-4.5-sonnet-thinking",
        "messages": [{"role": "user", "content": "Test"}],
        "tools": [/* ... */],
        "tool_choice": {"type": "auto"},
        "max_tokens": 1024
    });
    let _response1 = send_claude_request(req1).await.unwrap();
    let captured1 = get_last_upstream_request();
    assert_eq!(captured1["geminiSettings"]["recitationPolicy"]["action"], "BLOCK");
    assert_eq!(captured1["geminiSettings"]["recitationPolicy"]["threshold"], "LOW");

    // Test 2: Request with NONE mode
    let req2 = json!({
        "model": "claude-4.5-sonnet-thinking",
        "messages": [{"role": "user", "content": "Test"}],
        "tools": [/* ... */],
        "tool_choice": {"type": "none"},
        "max_tokens": 1024
    });
    let _response2 = send_claude_request(req2).await.unwrap();
    let captured2 = get_last_upstream_request();
    assert!(captured2.get("geminiSettings").is_some());

    // Test 3: Request without tools
    let req3 = json!({
        "model": "claude-4.5-sonnet-thinking",
        "messages": [{"role": "user", "content": "Test"}],
        "max_tokens": 1024
    });
    let _response3 = send_claude_request(req3).await.unwrap();
    let captured3 = get_last_upstream_request();
    assert!(captured3.get("geminiSettings").is_some());

    // Test 4: Request without thinking
    let req4 = json!({
        "model": "claude-4.5-sonnet-thinking",
        "messages": [{"role": "user", "content": "Test"}],
        "tools": [/* ... */],
        "max_tokens": 1024
    });
    let _response4 = send_claude_request(req4).await.unwrap();
    let captured4 = get_last_upstream_request();
    assert!(captured4.get("geminiSettings").is_some());
}
```

#### Scenario 6: Backward Compatibility Validation

**Goal**: Verify existing requests without tool_choice still work

**Test Case**:
```rust
#[tokio::test]
async fn test_backward_compatibility_no_tool_choice() {
    let request = json!({
        "model": "claude-4.5-sonnet-thinking",
        "messages": [{
            "role": "user",
            "content": "Hello, world!"
        }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "test_tool",
                "description": "Test",
                "parameters": {"type": "object", "properties": {}}
            }
        }],
        // No tool_choice field
        "max_tokens": 1024
    });

    let response = send_claude_request(request).await.unwrap();

    // Verify defaults to VALIDATED mode
    let captured_request = get_last_upstream_request();
    assert_eq!(
        captured_request["toolConfig"]["functionCallingConfig"]["mode"],
        "VALIDATED"
    );

    // Verify no allowedFunctionNames
    assert!(captured_request["toolConfig"]["functionCallingConfig"]
        .get("allowedFunctionNames")
        .is_none());

    // Verify response successful
    assert!(response.is_ok());
}
```

#### Scenario 7: Error Handling Validation

**Goal**: Verify proper error handling for invalid configurations

**Test Cases**:
```rust
#[tokio::test]
async fn test_invalid_tool_choice_format() {
    let request = json!({
        "model": "claude-4.5-sonnet-thinking",
        "messages": [{"role": "user", "content": "Test"}],
        "tools": [/* ... */],
        "tool_choice": {"type": "invalid_mode"},  // Invalid
        "max_tokens": 1024
    });

    let result = send_claude_request(request).await;

    // Should return 400 Bad Request
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert_eq!(error.status(), 400);
    assert!(error.body().contains("invalid tool_choice"));
}

#[tokio::test]
async fn test_tool_forcing_nonexistent_tool() {
    let request = json!({
        "model": "claude-4.5-sonnet-thinking",
        "messages": [{"role": "user", "content": "Test"}],
        "tools": [{
            "type": "function",
            "function": {"name": "existing_tool", "description": "Test"}
        }],
        "tool_choice": {
            "type": "tool",
            "name": "nonexistent_tool"  // Not in tools array
        },
        "max_tokens": 1024
    });

    // Should pass-through to upstream (let backend validate)
    let _response = send_claude_request(request).await;

    let captured_request = get_last_upstream_request();
    assert_eq!(
        captured_request["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"],
        json!(["nonexistent_tool"])
    );

    // Expect upstream error or empty tool response
}
```

---

## Acceptance Criteria

### AC-1: AUTO Mode End-to-End ✅

**Given** a request with `tool_choice: {type: "auto"}`
**When** I send to Vertex AI
**Then** it should:
- ✅ Set `toolConfig.mode` to "AUTO" in upstream request
- ✅ Receive valid response from backend
- ✅ Model autonomously decides tool usage
- ✅ No errors or rejections

**Validation**: Scenario 1 test passes

### AC-2: ANY Mode End-to-End ✅

**Given** a request with `tool_choice: {type: "any"}`
**When** I send to Vertex AI
**Then** it should:
- ✅ Set `toolConfig.mode` to "ANY" in upstream request
- ✅ Model calls at least one tool in response
- ✅ Receive valid response with tool_use block
- ✅ No errors or rejections

**Validation**: Scenario 2 test passes

### AC-3: NONE Mode End-to-End ✅

**Given** a request with `tool_choice: {type: "none"}`
**When** I send to Vertex AI
**Then** it should:
- ✅ Set `toolConfig.mode` to "NONE" in upstream request
- ✅ Model does NOT call any tools
- ✅ Receive text-only response
- ✅ No errors or rejections

**Validation**: Scenario 3 test passes

### AC-4: Tool Forcing End-to-End ✅

**Given** a request with `tool_choice: {type: "tool", name: "specific_tool"}`
**When** I send to Vertex AI
**Then** it should:
- ✅ Set `toolConfig.mode` to "VALIDATED"
- ✅ Set `allowedFunctionNames` to ["specific_tool"]
- ✅ Model calls ONLY the specified tool
- ✅ Receive valid response

**Validation**: Scenario 4 test passes

### AC-5: geminiSettings Always Present ✅

**Given** any request configuration
**When** I send to Vertex AI
**Then** geminiSettings should:
- ✅ Always be present in upstream request
- ✅ Have action="BLOCK", threshold="LOW"
- ✅ Work with AUTO, ANY, NONE, VALIDATED modes
- ✅ Work with and without tools
- ✅ Work with and without thinking

**Validation**: Scenario 5 test passes

### AC-6: Backward Compatibility ✅

**Given** a request without tool_choice field
**When** I send to Vertex AI
**Then** it should:
- ✅ Default to VALIDATED mode
- ✅ Not set allowedFunctionNames
- ✅ Work exactly as before
- ✅ No regressions

**Validation**: Scenario 6 test passes

### AC-7: Error Handling ✅

**Given** invalid tool_choice configurations
**When** I send requests
**Then** it should:
- ✅ Return 400 for invalid tool_choice format
- ✅ Pass-through nonexistent tool names to upstream
- ✅ Handle upstream errors gracefully
- ✅ Provide clear error messages

**Validation**: Scenario 7 tests pass

### AC-8: Compliance Score 100% ✅

**Given** all Phase 3 implementations complete
**When** I validate against Gap Analysis checklist
**Then** it should:
- ✅ Close all 8 gaps (from Gap Analysis)
- ✅ Match Antigravity baseline exactly
- ✅ Pass manual validation against RE spec
- ✅ Achieve 100% compliance score

**Validation**: Manual compliance checklist completion

### AC-9: Performance Validation ✅

**Given** all new features implemented
**When** I measure request/response latency
**Then** it should:
- ✅ Add <100ms overhead for tool mode mapping
- ✅ Add <10ms for geminiSettings addition
- ✅ No degradation in existing request performance
- ✅ Maintain upstream API performance

**Validation**: Performance benchmarking

### AC-10: Integration Test Suite Complete ✅

**Given** this story's test implementation
**When** I run the full integration test suite
**Then** it should:
- ✅ All 7 scenarios pass (Scenarios 1-7)
- ✅ All tests use live Vertex AI endpoint
- ✅ Tests cover all tool modes
- ✅ Tests cover all edge cases
- ✅ Tests run reliably (no flaky tests)

**Validation**: All integration tests passing

---

## Testing Strategy

### Integration Tests (7 scenarios)

**File**: `src-tauri/tests/tool_mode_integration_tests.rs`

```rust
//! Integration tests for tool mode functionality
//!
//! These tests use live Vertex AI endpoints and validate end-to-end behavior.
//!
//! Prerequisites:
//! - Valid Google OAuth token
//! - Vertex AI API enabled
//! - Test project configured

#[cfg(test)]
mod tool_mode_tests {
    use super::*;

    // Scenario 1: AUTO mode
    #[tokio::test]
    #[ignore] // Run with: cargo test --test tool_mode_integration_tests -- --ignored
    async fn test_e2e_tool_mode_auto() {
        // Implementation from Scenario 1 above
    }

    // Scenario 2: ANY mode
    #[tokio::test]
    #[ignore]
    async fn test_e2e_tool_mode_any() {
        // Implementation from Scenario 2 above
    }

    // Scenario 3: NONE mode
    #[tokio::test]
    #[ignore]
    async fn test_e2e_tool_mode_none() {
        // Implementation from Scenario 3 above
    }

    // Scenario 4: Tool forcing
    #[tokio::test]
    #[ignore]
    async fn test_e2e_tool_forcing() {
        // Implementation from Scenario 4 above
    }

    // Scenario 5: geminiSettings presence
    #[tokio::test]
    #[ignore]
    async fn test_gemini_settings_always_present() {
        // Implementation from Scenario 5 above
    }

    // Scenario 6: Backward compatibility
    #[tokio::test]
    #[ignore]
    async fn test_backward_compatibility_no_tool_choice() {
        // Implementation from Scenario 6 above
    }

    // Scenario 7: Error handling
    #[tokio::test]
    #[ignore]
    async fn test_invalid_tool_choice_format() {
        // Implementation from Scenario 7a above
    }

    #[tokio::test]
    #[ignore]
    async fn test_tool_forcing_nonexistent_tool() {
        // Implementation from Scenario 7b above
    }

    // Test helpers
    async fn send_claude_request(request: serde_json::Value) -> Result<serde_json::Value, Error> {
        // Send request through proxy to Vertex AI
        // ...
    }

    fn get_last_upstream_request() -> serde_json::Value {
        // Capture and return last upstream request
        // (requires request logging/capture infrastructure)
        // ...
    }
}
```

### Manual Testing Checklist

#### Tool Mode Validation
- [ ] **AUTO Mode**:
  - [ ] Send request with AUTO mode
  - [ ] Verify model decides when to use tools
  - [ ] Test with single tool and multiple tools
  - [ ] Verify toolConfig.mode = "AUTO" in logs

- [ ] **ANY Mode**:
  - [ ] Send request with ANY mode
  - [ ] Verify model calls at least one tool
  - [ ] Test with single tool (must call it)
  - [ ] Verify toolConfig.mode = "ANY" in logs

- [ ] **NONE Mode**:
  - [ ] Send request with NONE mode
  - [ ] Verify model returns text-only response
  - [ ] Verify no tool calls in response
  - [ ] Verify toolConfig.mode = "NONE" in logs

- [ ] **Tool Forcing**:
  - [ ] Send request forcing specific tool
  - [ ] Verify model calls ONLY that tool
  - [ ] Test with multiple tools available
  - [ ] Verify allowedFunctionNames in logs

#### Grounding Configuration Validation
- [ ] **geminiSettings Presence**:
  - [ ] Verify in AUTO mode request
  - [ ] Verify in ANY mode request
  - [ ] Verify in NONE mode request
  - [ ] Verify in VALIDATED mode request
  - [ ] Verify in request without tools
  - [ ] Verify in request without thinking

- [ ] **Recitation Policy Values**:
  - [ ] Verify action = "BLOCK"
  - [ ] Verify threshold = "LOW"
  - [ ] Verify format matches RE spec exactly

#### Compliance Validation
- [ ] **Gap Analysis Checklist** (from lines 3797-3805):
  - [x] Model provider info present (Stories 1-2)
  - [x] ideType: ANTIGRAVITY present (Story 3)
  - [x] JWT signatures validated (Story 5)
  - [x] Budget constraints logged (Story 6)
  - [x] Position violations logged (Story 7)
  - [ ] Tool modes configurable (Story 9) ← **This Story**
  - [ ] Grounding config present (Story 10) ← **This Story**
  - [x] Session metadata complete (Story 4)

- [ ] **Request Structure Comparison**:
  - [ ] Compare captured request with Antigravity baseline
  - [ ] Verify identical field ordering
  - [ ] Verify identical value formats
  - [ ] Verify no extra or missing fields

#### Performance Validation
- [ ] **Latency Measurement**:
  - [ ] Measure request mapping time (<100ms target)
  - [ ] Measure end-to-end latency (no degradation)
  - [ ] Test with various request sizes

- [ ] **Error Handling**:
  - [ ] Test invalid tool_choice format
  - [ ] Test nonexistent tool forcing
  - [ ] Test upstream API errors
  - [ ] Verify error messages are clear

#### Backward Compatibility
- [ ] **Existing Workflows**:
  - [ ] Test request without tool_choice
  - [ ] Test request without tools
  - [ ] Test request without thinking
  - [ ] Test minimal request
  - [ ] Verify all existing features work

---

## Definition of Done

### Code Quality
- [ ] ✅ All 7 integration test scenarios implemented
- [ ] ✅ All tests using live Vertex AI endpoint
- [ ] ✅ Test helper functions implemented
- [ ] ✅ Request capture infrastructure working
- [ ] ✅ Code follows Rust best practices
- [ ] ✅ No clippy warnings
- [ ] ✅ Code formatted with rustfmt

### Testing
- [ ] ✅ All 7 integration scenarios passing
- [ ] ✅ Manual testing checklist completed
- [ ] ✅ Performance benchmarking completed
- [ ] ✅ Error handling validated
- [ ] ✅ Backward compatibility confirmed

### Validation
- [ ] ✅ All 10 AC validated
- [ ] ✅ Gap Analysis checklist complete (8/8 gaps closed)
- [ ] ✅ Compliance score: **100%**
- [ ] ✅ Manual validation against RE spec successful
- [ ] ✅ Live Vertex AI endpoint testing successful

### Documentation
- [ ] ✅ Test scenarios documented
- [ ] ✅ Manual testing checklist completed
- [ ] ✅ Performance results documented
- [ ] ✅ Known issues/limitations documented (if any)
- [ ] ✅ Story marked as completed

### Epic Completion
- [ ] ✅ Phase 3 (Feature Parity) complete
- [ ] ✅ Epic-003 updated with Phase 3 completion
- [ ] ✅ Compliance score verified at 100%
- [ ] ✅ Ready for Phase 4 (Enhancement & Monitoring)

---

## Dependencies

### Upstream Dependencies (Must Be Complete)
- ✅ **Story-003-09**: Flexible Tool Configuration Modes (COMPLETE)
- ✅ **Story-003-10**: Grounding Configuration (COMPLETE)
- ✅ **Story-003-01 to 003-08**: All previous stories (COMPLETE)

**Why**: This story validates implementations from Stories 009 and 010.

### Downstream Dependencies
- **Story-003-12**: Compliance Monitoring Dashboard (will display test results)

**Impact**: Dashboard will show 100% compliance after this story.

### Infrastructure Dependencies
- **Live Vertex AI Access**: Valid OAuth token and enabled API
- **Request Logging**: Capture infrastructure for upstream requests
- **Test Environment**: Configured test project

---

## Risk Analysis

### Risk #1: Live Endpoint Testing Reliability
**Severity**: MEDIUM
**Probability**: MEDIUM
**Impact**: Flaky tests, unreliable validation

**Mitigation**:
- ✅ Use `#[ignore]` for integration tests (run separately)
- ✅ Implement retry logic for network issues
- ✅ Clear test isolation and cleanup
- ✅ Document prerequisites clearly

**Contingency**: If live testing unreliable, use request capture validation only.

### Risk #2: Backend Rejection of New Configurations
**Severity**: HIGH
**Probability**: LOW
**Impact**: Requests rejected, features don't work

**Mitigation**:
- ✅ Follow Gap Analysis exact specifications
- ✅ Manual validation before automated testing
- ✅ Start with VALIDATED mode (known working)
- ✅ Test one mode at a time

**Contingency**: If mode rejected, investigate error and adjust implementation.

### Risk #3: Performance Degradation
**Severity**: LOW
**Probability**: LOW
**Impact**: Increased latency from new features

**Mitigation**:
- ✅ Benchmark before and after implementation
- ✅ Target <100ms overhead
- ✅ Optimize hot paths
- ✅ Profile if needed

**Measurement**: AC-9 performance validation.

### Risk #4: Incomplete Test Coverage
**Severity**: MEDIUM
**Probability**: LOW
**Impact**: Bugs in production, compliance issues

**Mitigation**:
- ✅ Comprehensive test scenarios (7 scenarios)
- ✅ Manual testing checklist
- ✅ Gap Analysis checklist validation
- ✅ Multiple validation layers

**Contingency**: Add additional tests if gaps found during manual testing.

---

## Implementation Notes

### Testing Infrastructure Requirements

**Request Capture**:
- Need infrastructure to capture upstream requests
- Options: Logging middleware, test spy, mock upstream
- Requirement: Access to full JSON request sent to Vertex AI

**Test Environment**:
- Separate test project/account recommended
- Valid OAuth token with sufficient quota
- Vertex AI API enabled

**Test Isolation**:
- Each test should be independent
- No shared state between tests
- Clean setup/teardown

### Manual Testing Approach

**Validation Sequence**:
1. Start with VALIDATED mode (baseline)
2. Test AUTO mode next (least restrictive)
3. Test ANY mode (forced tool usage)
4. Test NONE mode (disabled tools)
5. Test tool forcing last (most specific)

**For Each Mode**:
1. Capture and inspect upstream request
2. Verify mode and allowedFunctionNames
3. Verify geminiSettings present
4. Check backend response
5. Validate response format
6. Document any issues

### Performance Benchmarking

**Metrics to Collect**:
- Request mapping time (target: <100ms)
- End-to-end latency (no degradation)
- Memory usage (minimal increase)
- CPU usage (minimal increase)

**Baseline**: Measure before implementing Stories 009-010
**Target**: <10% overhead from new features

---

## File Impact Analysis

### New Files

| File | Lines | Description |
|------|-------|-------------|
| `tool_mode_integration_tests.rs` | ~500 | 7 integration test scenarios |

### Modified Files

| File | Lines Changed | Change Type | Description |
|------|---------------|-------------|-------------|
| `Cargo.toml` | +3 | Addition | Test dependencies if needed |

**Total Changes**:
- **Test Code**: ~500 lines
- **Infrastructure**: Minimal (test helpers)

---

## Change Log

| Date | Change | Author |
|------|--------|--------|
| 2026-01-10 | Story created with comprehensive test scenarios | BMad Master |

---

## References

### Dependency Stories
- **Story-003-09**: `docs/stories/Story-003-09-flexible-tool-configuration-modes.md`
  - Tool mode implementation and unit tests
- **Story-003-10**: `docs/stories/Story-003-10-grounding-configuration.md`
  - geminiSettings implementation and unit tests

### Gap Analysis
- **Document**: `docs/comparison/claude/claude-4-5-sonnet/current-implementation-thinking.md`
  - Lines 3730-3806: Testing Strategy
  - Lines 3797-3805: Compliance checklist

### Epic Definition
- **Epic-003**: `docs/epics/Epic-003-Claude-4.5-Sonnet-Thinking-Compliance.md`
  - Phase 3 Success Criteria
  - Epic-level Definition of Done
