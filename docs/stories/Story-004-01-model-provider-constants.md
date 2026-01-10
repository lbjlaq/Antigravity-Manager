# Story-004-01: Add Model Provider Constants (Standard Model)

**Story ID**: Story-004-01
**Epic**: [Epic-004](../epics/Epic-004-Claude-4.5-Sonnet-Standard-Compliance.md) - Claude 4.5 Sonnet (Standard) Compliance
**Related Stories**: [Story-003-01](Story-003-01-model-id-constant.md), [Story-003-02](Story-003-02-api-model-providers.md)
**Priority**: P0 (Critical)
**Phase**: 1 (Critical Compliance)
**Estimated Effort**: 1.5 hours
**Status**: ✅ **IMPLEMENTED** (Shared with Epic-003)
**Tags**: `[SHARED]`, `compliance`, `anti-detection`, `[ALREADY-DONE]`
**Created**: 2026-01-11
**Owner**: Engineering Team

---

## ⚠️ IMPORTANT: Implementation Status

**This story is ALREADY IMPLEMENTED** as part of Epic-003 (Stories 003-01 and 003-02).

**Current Implementation**:
- ✅ Constants defined: `request.rs:13-24`
- ✅ Helper functions: `request.rs:177-211`
- ✅ Request assembly: `request.rs:518-520`
- ✅ Model ID 333 supported: `"claude-4.5-sonnet"` mapping exists
- ✅ API Provider 26: `API_PROVIDER_ANTHROPIC_VERTEX`
- ✅ Model Provider 3: `MODEL_PROVIDER_ANTHROPIC`

**Verification Required**:
This story serves as **VALIDATION** that standard model (ID: 333) is correctly integrated with the shared constants implementation from Epic-003.

**No Additional Work Needed** unless validation reveals issues.

---

## User Story

**As a** API Proxy developer
**I want** to add complete model provider constants for claude-4.5-sonnet (modelId: 333, apiProvider: 26, modelProvider: 3)
**So that** all standard Claude requests include proper routing information for Vertex AI and anti-detection compliance

---

## Context

### Current Situation

**Missing Critical Fields** in v1internal API requests:
```json
{
  "project": "bamboo-precept-lgxtn",
  "requestId": "agent-uuid",
  "model": "claude-4.5-sonnet",
  // ❌ Missing: modelId
  // ❌ Missing: apiProvider
  // ❌ Missing: modelProvider
  "userAgent": "antigravity",
  "request": {...}
}
```

**Issues**:
- ❌ **Routing Failures**: Vertex AI proxy layer cannot identify model
- ❌ **Detection Risk**: CRITICAL - Missing provider fields identify non-Antigravity client
- ❌ **Protocol Violation**: v1internal API expects all three fields

### Expected Behavior

**Complete Request with Provider Information**:
```json
{
  "project": "bamboo-precept-lgxtn",
  "requestId": "agent-uuid",
  "model": "claude-4.5-sonnet",
  "modelId": 333,               // ← Standard model ID
  "apiProvider": 26,            // ← ANTHROPIC_VERTEX
  "modelProvider": 3,           // ← ANTHROPIC
  "userAgent": "antigravity",
  "request": {...}
}
```

**Benefits**:
- ✅ Proper routing through Vertex AI proxy layer
- ✅ Anti-detection compliance (primary marker)
- ✅ Consistent with Antigravity v1.13.3 behavior

---

## Shared Implementation Notes

**90% Code Sharing with Epic-003**:
- ✅ Constants structure identical (apiProvider, modelProvider)
- ✅ Helper functions identical
- ✅ Request assembly logic identical
- ⚠️ **Only difference**: Model ID constant (333 vs 334)

**Recommended Approach**:
Implement together with [Story-003-01](Story-003-01-model-id-constant.md) and [Story-003-02](Story-003-02-api-model-providers.md) using shared constants file.

---

## Reference Documentation

### Primary Analysis
1. **Epic-004 Gap Analysis**:
   - `docs/comparison/claude/claude-4-5-sonnet/current-implementation.md`
   - Lines 787-795: Critical Issue #1 (No Model Provider Information)
   - Lines 945-958: Gap Analysis table

2. **Epic-003 Shared Implementation**:
   - `docs/comparison/claude/claude-4-5-sonnet/current-implementation-thinking.md`
   - Lines 2744-2872: Identical implementation pattern
   - Lines 2819-2867: Helper functions (reusable)

### Reverse Engineering Spec
- `docs/antigravity/workflows/models/claude/claude-4-5-sonnet-workflow.md`
  - Model configuration: modelId: 333 (standard)
  - apiProvider: 26 (ANTHROPIC_VERTEX)
  - modelProvider: 3 (ANTHROPIC)

### Technical Specs
- `docs/technical-specs/antigravity-workflow-compliance-gap-analysis.md`
  - Lines 56-86: Model Name Mapping
  - Provider routing architecture

---

## Technical Implementation

### Step 1: Add Shared Constants Structure

**File**: `src-tauri/src/proxy/mappers/claude/constants.rs` (NEW file for shared constants)

**Create Shared Constants File**:
```rust
//! Claude model constants for Antigravity v1.13.3 compliance
//!
//! References:
//! - docs/antigravity/workflows/models/claude/claude-4-5-sonnet-workflow.md
//! - docs/antigravity/workflows/models/claude/claude-4-5-sonnet-thinking-workflow.md

/// Model IDs from Google Antigravity v1.13.3
pub const CLAUDE_SONNET_45_STANDARD_MODEL_ID: i32 = 333;   // Epic-004
pub const CLAUDE_SONNET_45_THINKING_MODEL_ID: i32 = 334;   // Epic-003

/// API Provider IDs (shared across all Claude models)
/// ANTHROPIC_VERTEX indicates routing through Vertex AI proxy layer
pub const ANTHROPIC_VERTEX_API_PROVIDER: i32 = 26;

/// Model Provider IDs (shared across all Claude models)
/// ANTHROPIC indicates Anthropic as the underlying model provider
pub const ANTHROPIC_MODEL_PROVIDER: i32 = 3;

/// Check if model name indicates thinking mode
pub fn is_thinking_model(model_name: &str) -> bool {
    model_name.contains("-thinking") || model_name.contains("_thinking")
}

/// Get model ID for given model name
/// Returns appropriate model ID based on thinking mode detection
pub fn get_model_id(model_name: &str) -> i32 {
    if is_thinking_model(model_name) {
        CLAUDE_SONNET_45_THINKING_MODEL_ID
    } else {
        CLAUDE_SONNET_45_STANDARD_MODEL_ID
    }
}

/// Get API provider ID (same for all Claude models)
pub fn get_api_provider() -> i32 {
    ANTHROPIC_VERTEX_API_PROVIDER
}

/// Get model provider ID (same for all Claude models)
pub fn get_model_provider() -> i32 {
    ANTHROPIC_MODEL_PROVIDER
}
```

**Rationale**:
- ✅ Single source of truth for all model constants
- ✅ Shared by both Epic-003 and Epic-004
- ✅ Easy to extend for future models
- ✅ Clear documentation and references

---

### Step 2: Update Request Mapper Module

**File**: `src-tauri/src/proxy/mappers/claude/mod.rs`

**Add Module Declaration**:
```rust
pub mod constants;  // 🆕 Shared constants module
pub mod request;
pub mod response;
pub mod models;
pub mod thinking_utils;
```

---

### Step 3: Update Request Assembly

**File**: `src-tauri/src/proxy/mappers/claude/request.rs`

**Import Constants**:
```rust
use super::constants::{
    get_model_id,
    get_api_provider,
    get_model_provider,
};
```

**Update Request Creation** (around line 314-398):
```rust
pub fn map_claude_to_vertex_ai(
    claude_req: &ClaudeRequest,
    config: &ProxyConfig,
    trace_id: &str,
) -> serde_json::Value {
    // ... existing code ...

    // Detect model name
    let model_name = &claude_req.model;

    // 🆕 Get provider information from shared constants
    let model_id = get_model_id(model_name);          // 333 for standard, 334 for thinking
    let api_provider = get_api_provider();            // 26 (ANTHROPIC_VERTEX)
    let model_provider = get_model_provider();        // 3 (ANTHROPIC)

    // Create outer request wrapper
    let outer_request = json!({
        "project": project_id,
        "requestId": request_id,
        "model": model_name,
        "modelId": model_id,           // 🆕 Add model ID
        "apiProvider": api_provider,   // 🆕 Add API provider
        "modelProvider": model_provider, // 🆕 Add model provider
        "userAgent": "antigravity",
        "requestType": "GENERATE_CONTENT",
        "request": inner_request
    });

    tracing::debug!(
        "[Request-Mapper] Added provider info: modelId={}, apiProvider={}, modelProvider={}",
        model_id, api_provider, model_provider
    );

    outer_request
}
```

---

### Step 4: Validation and Logging

**Add Debug Logging**:
```rust
tracing::info!(
    "[Claude-Request] Model: {}, ID: {}, Provider: API={} Model={}",
    model_name, model_id, api_provider, model_provider
);
```

**Add Validation Check**:
```rust
// Validate model ID is set
if model_id == 0 {
    tracing::warn!("[Claude-Request] Unknown model '{}', using default ID", model_name);
}
```

---

## Testing Strategy

### Unit Tests

**File**: `src-tauri/src/proxy/mappers/claude/constants.rs`

**Test 1: Model ID Detection**:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_model_id() {
        assert_eq!(get_model_id("claude-4.5-sonnet"), 333);
        assert_eq!(get_model_id("claude-sonnet-4-5"), 333);
    }

    #[test]
    fn test_thinking_model_id() {
        assert_eq!(get_model_id("claude-4.5-sonnet-thinking"), 334);
        assert_eq!(get_model_id("claude-sonnet-4-5-thinking"), 334);
    }

    #[test]
    fn test_thinking_detection() {
        assert!(!is_thinking_model("claude-4.5-sonnet"));
        assert!(is_thinking_model("claude-4.5-sonnet-thinking"));
        assert!(is_thinking_model("claude-4.5-sonnet_thinking"));
    }

    #[test]
    fn test_provider_constants() {
        assert_eq!(get_api_provider(), 26);
        assert_eq!(get_model_provider(), 3);
    }
}
```

**Test 2: Request Assembly with Provider Info**:
```rust
#[test]
fn test_request_includes_provider_info() {
    let claude_req = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),
        // ... other fields
    };

    let config = ProxyConfig::default();
    let result = map_claude_to_vertex_ai(&claude_req, &config, "test-trace");

    // Verify provider fields
    assert_eq!(result["modelId"].as_i64().unwrap(), 333);
    assert_eq!(result["apiProvider"].as_i64().unwrap(), 26);
    assert_eq!(result["modelProvider"].as_i64().unwrap(), 3);
}
```

### Integration Tests

**File**: `src-tauri/tests/integration/claude_request_test.rs`

**Test: End-to-End Request Transformation**:
```rust
#[tokio::test]
async fn test_claude_standard_request_with_providers() {
    // Setup mock Claude request
    let claude_req = json!({
        "model": "claude-4.5-sonnet",
        "messages": [{"role": "user", "content": "test"}],
        "max_tokens": 1024
    });

    // Transform to v1internal format
    let result = transform_claude_request(&claude_req).await.unwrap();

    // Verify all provider fields present
    assert_eq!(result["modelId"], 333);
    assert_eq!(result["apiProvider"], 26);
    assert_eq!(result["modelProvider"], 3);
    assert_eq!(result["model"], "claude-4.5-sonnet");
}
```

**Test: Thinking vs Standard Model Detection**:
```rust
#[tokio::test]
async fn test_model_id_detection() {
    let standard_req = json!({"model": "claude-4.5-sonnet"});
    let thinking_req = json!({"model": "claude-4.5-sonnet-thinking"});

    let standard_result = transform_claude_request(&standard_req).await.unwrap();
    let thinking_result = transform_claude_request(&thinking_req).await.unwrap();

    assert_eq!(standard_result["modelId"], 333);  // Standard
    assert_eq!(thinking_result["modelId"], 334);  // Thinking
}
```

### Manual Testing

**Test Case 1: Standard Model Request**:
```bash
# Send request to proxy
curl -X POST http://localhost:8045/v1/messages \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-4.5-sonnet",
    "messages": [{"role": "user", "content": "test"}],
    "max_tokens": 1024
  }'

# Check logs for provider info
# Expected: "Model: claude-4.5-sonnet, ID: 333, Provider: API=26 Model=3"
```

**Test Case 2: Thinking Model Request**:
```bash
# Send thinking model request
curl -X POST http://localhost:8045/v1/messages \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-4.5-sonnet-thinking",
    "messages": [{"role": "user", "content": "test"}],
    "max_tokens": 1024,
    "thinking": {"type": "enabled", "budget_tokens": 10000}
  }'

# Check logs for provider info
# Expected: "Model: claude-4.5-sonnet-thinking, ID: 334, Provider: API=26 Model=3"
```

---

## Acceptance Criteria

**AC1: Constants File Created**
- [ ] `constants.rs` module created with all provider constants
- [ ] Model ID constants: 333 (standard), 334 (thinking)
- [ ] API Provider constant: 26
- [ ] Model Provider constant: 3
- [ ] Helper functions: `get_model_id()`, `get_api_provider()`, `get_model_provider()`

**AC2: Request Assembly Updated**
- [ ] `map_claude_to_vertex_ai()` imports constants
- [ ] `modelId` field added to outer request
- [ ] `apiProvider` field added to outer request
- [ ] `modelProvider` field added to outer request
- [ ] Correct model ID selected based on model name (333 vs 334)

**AC3: Model Detection Logic**
- [ ] `is_thinking_model()` correctly detects thinking mode
- [ ] `-thinking` suffix detected
- [ ] `_thinking` suffix detected
- [ ] Standard models return ID 333
- [ ] Thinking models return ID 334

**AC4: Testing Complete**
- [ ] Unit tests: 4 tests passing (model ID, thinking detection, providers)
- [ ] Integration tests: 2 tests passing (end-to-end, detection)
- [ ] Manual testing: Both standard and thinking models verified
- [ ] Logs show correct provider information

**AC5: Code Sharing Validated**
- [ ] Constants shared with Epic-003 implementation
- [ ] Helper functions reusable across both epics
- [ ] No code duplication between standard and thinking versions
- [ ] Single source of truth for all model constants

---

## Dependencies

**Blocks**:
- Story-004-02: ideType ANTIGRAVITY Metadata (needs constants)
- Story-004-03: Model-Specific Routing (uses model ID detection)

**Blocked By**: None (first story in Epic-004)

**Related**:
- ✅ Story-003-01: Model ID Constant (thinking version)
- ✅ Story-003-02: API/Model Provider Constants (thinking version)

---

## Implementation Notes

### Shared Code Strategy

**DO** ✅:
- Create single `constants.rs` file for both epics
- Use model detection to branch between IDs (333 vs 334)
- Share all helper functions (`get_model_id()`, etc.)
- Implement Epic-003 and Epic-004 together for efficiency

**DON'T** ❌:
- Duplicate constants in separate files
- Hardcode model IDs (333, 334) in request mapper
- Create separate helper functions for standard vs thinking
- Implement epics separately (loses 30% efficiency)

### Performance Considerations

- Model ID lookup: O(1) constant time
- No regex matching (simple string contains check)
- No heap allocations in hot path
- Estimated overhead: <0.1ms per request

---

## Risks & Mitigation

**Risk**: Model ID mismatch between standard (333) and thinking (334)
**Mitigation**: Use model name detection with unit tests

**Risk**: Code divergence if implemented separately
**Mitigation**: Shared constants file, single implementation

**Risk**: Breaking existing requests
**Mitigation**: Additive change only (adds fields, doesn't modify existing)

---

## Success Metrics

**Before Story-004-01**:
- ❌ Compliance Score: 75-80%
- ❌ Missing: modelId, apiProvider, modelProvider
- ❌ Detection Risk: CRITICAL

**After Story-004-01**:
- ✅ Compliance Score: 80-85% (+5-10%)
- ✅ All provider fields present
- ✅ Detection Risk: MEDIUM (still missing ideType)

**Verification**:
```bash
# Check request includes all provider fields
curl -X POST http://localhost:8045/v1/messages \
  -H "Content-Type: application/json" \
  -d '{"model": "claude-4.5-sonnet", "messages": [{"role": "user", "content": "test"}]}' \
  --trace-ascii - 2>&1 | grep -E "(modelId|apiProvider|modelProvider)"

# Expected output:
# "modelId": 333
# "apiProvider": 26
# "modelProvider": 3
```

---

## Related Documentation

**Epic-003 Stories** (90% Shared):
- [Story-003-01](Story-003-01-model-id-constant.md): Model ID Constant (thinking: 334)
- [Story-003-02](Story-003-02-api-model-providers.md): API/Model Provider Constants

**Epic-004 Stories** (Next):
- [Story-004-02](Story-004-02-antigravity-metadata.md): ideType ANTIGRAVITY Metadata
- [Story-004-03](Story-004-03-model-specific-routing.md): Model-Specific Routing

**Gap Analysis**:
- `docs/comparison/claude/claude-4-5-sonnet/current-implementation.md` (Lines 787-795)

---

## Change Log

| Date | Change | Author |
|------|--------|--------|
| 2026-01-11 | Story-004-01 created | BMad Master |
| 2026-01-11 | Shared constants strategy documented | BMad Master |
| 2026-01-11 | Model detection logic specified | BMad Master |
