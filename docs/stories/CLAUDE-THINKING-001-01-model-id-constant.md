# Story CLAUDE-THINKING-001-01: Add Model ID Constant and Helper Function

**Story ID**: CLAUDE-THINKING-001-01
**Epic**: [CLAUDE-THINKING-001 - Claude 4.5 Sonnet Thinking Compliance](../epics/claude-4-5-sonnet-thinking-compliance-epic.md)
**Priority**: P0 (Critical)
**Estimate**: 2 story points (1.5 hours)
**Status**: In Progress
**Assignee**: Development Team

---

## User Story

**As a** API Proxy developer
**I want** to add model ID constant (334) and helper function to retrieve model ID by name
**So that** all claude-4.5-sonnet-thinking requests include the correct model identifier for Vertex AI routing

---

## Context

Current implementation is missing the `modelId` field in requests to Google's v1internal API. According to reverse engineering of Antigravity v1.13.3, **all Claude model requests must include modelId** for proper routing through Vertex AI.

**Current Request** (Missing modelId):
```json
{
  "project": "bamboo-precept-lgxtn",
  "requestId": "agent-uuid",
  "model": "claude-4.5-sonnet-thinking",
  "userAgent": "antigravity",
  "request": {...}
}
```

**Expected Request** (With modelId):
```json
{
  "project": "bamboo-precept-lgxtn",
  "requestId": "agent-uuid",
  "model": "claude-4.5-sonnet-thinking",
  "modelId": 334,
  "userAgent": "antigravity",
  "request": {...}
}
```

**Detection Risk**: HIGH - Missing modelId may cause routing failures or detection as non-Antigravity client.

---

## Reference Documents

**Primary**:
1. `docs/comparison/claude/claude-4-5-sonnet/current-implementation-thinking.md`
   - Lines 2744-2872: Gap Analysis #1 (Model Provider Information)
   - Lines 2819-2867: Implementation example with helper functions

2. `docs/technical-specs/antigravity-workflow-compliance-gap-analysis.md`
   - Lines 56-86: Model Name Mapping section

**Reverse Engineering Spec**:
- `docs/antigravity/workflows/models/claude/claude-4-5-sonnet-thinking-workflow.md`
  - Lines 161-166: Model configuration (modelId: 334)

**Quick Reference**:
- `docs/comparison/claude/claude-4-5-sonnet/EXECUTIVE-SUMMARY.md`

---

## Technical Details

### Implementation Steps

**File**: `src-tauri/src/proxy/mappers/claude/request.rs`

**Step 1 - Add Constants** (top of file):
```rust
// Model ID constants from Google Antigravity v1.13.3
// Reference: docs/antigravity/workflows/models/claude/claude-4-5-sonnet-thinking-workflow.md:161-166
const CLAUDE_4_5_SONNET_THINKING_MODEL_ID: u32 = 334;
const CLAUDE_4_5_SONNET_MODEL_ID: u32 = 333;
// Add other models as needed
```

**Step 2 - Add Helper Function**:
```rust
/// Get model ID for Antigravity v1internal API
/// Returns model ID from reverse-engineered specification
/// Reference: docs/antigravity/workflows/models/claude/claude-4-5-sonnet-thinking-workflow.md:161-166
fn get_model_id(model_name: &str) -> u32 {
    match model_name {
        "claude-4.5-sonnet-thinking" => CLAUDE_4_5_SONNET_THINKING_MODEL_ID,
        "claude-4.5-sonnet" => CLAUDE_4_5_SONNET_MODEL_ID,
        // Add mappings for other models
        _ => 0  // Unknown model
    }
}
```

**Step 3 - Update Request Assembly** (around line 314-398):
```rust
let mut body = json!({
    "project": project_id,
    "requestId": request_id,
    "model": config.final_model,
    "modelId": get_model_id(&config.final_model),  // 🆕 ADD THIS LINE
    "userAgent": "antigravity",
    "requestType": config.request_type,
    "request": inner_request,
});
```

---

## Acceptance Criteria

**Given** a request for claude-4.5-sonnet-thinking model
**When** the request is transformed to v1internal format
**Then** the request body includes `"modelId": 334`

**And** the model ID is retrieved via `get_model_id()` helper function (not hardcoded)
**And** the helper function supports multiple Claude models (thinking and non-thinking)
**And** the function returns `0` for unknown/unsupported models

---

## Testing Requirements

### Unit Tests

```rust
#[test]
fn test_get_model_id_sonnet_thinking() {
    assert_eq!(get_model_id("claude-4.5-sonnet-thinking"), 334);
}

#[test]
fn test_get_model_id_sonnet() {
    assert_eq!(get_model_id("claude-4.5-sonnet"), 333);
}

#[test]
fn test_get_model_id_unknown() {
    assert_eq!(get_model_id("unknown-model"), 0);
}

#[test]
fn test_request_includes_model_id() {
    let req = build_test_claude_request("claude-4.5-sonnet-thinking");
    let body = transform_claude_request_in(&req, "test-project").unwrap();

    assert_eq!(body["modelId"], 334);
}
```

### Integration Tests

- Create full request transformation test
- Verify modelId is present in final JSON
- Validate against RE specification format

### Manual Validation

- [ ] Send test request to v1internal API
- [ ] Verify request is accepted (not rejected for missing modelId)
- [ ] Check logs for proper model ID (334) in request

---

## Definition of Done

**Code**:
- [ ] Constants added for CLAUDE_4_5_SONNET_THINKING_MODEL_ID (334) and CLAUDE_4_5_SONNET_MODEL_ID (333)
- [ ] `get_model_id()` helper function implemented with match statement
- [ ] Request assembly updated to include `modelId` field
- [ ] Code reviewed and approved

**Testing**:
- [ ] 4 unit tests written and passing
- [ ] Integration test passing
- [ ] Test coverage ≥80% for new code
- [ ] Manual validation completed against v1internal API

**Compliance**:
- [ ] **Compliance validation**: Request format matches RE spec (docs/antigravity/workflows/models/claude/claude-4-5-sonnet-thinking-workflow.md:161-166)
- [ ] **Performance benchmarking**: get_model_id() function adds <1ms overhead (compile-time match)
- [ ] **Anti-detection validation**: modelId field identical to Antigravity 1.13.3 format
- [ ] **Request format validation**: JSON structure validated against spec lines 2757-2767
- [ ] **Error handling validation**: Unknown models return 0 (handled gracefully)

**Documentation**:
- [ ] Code comments reference RE spec line numbers
- [ ] Implementation notes added explaining model ID purpose
- [ ] Helper function documentation includes return values

---

## Dependencies

**Depends On**: None (first story in epic)

**Blocks**:
- CLAUDE-THINKING-001-02: API/Model Provider constants (builds on this foundation)

**Related**:
- Part of FR1: Model Provider Information (along with Stories #2)

---

## Implementation Notes

**Why Model ID 334?**
- Reverse engineering of Antigravity v1.13.3 shows `claude-4.5-sonnet-thinking` has Model ID: 334
- Different from non-thinking version (Model ID: 333)
- Required for proper routing through Vertex AI → Anthropic backend

**Helper Function Approach**:
- Uses Rust `match` statement for compile-time optimization
- Returns `0` for unknown models (safe fallback)
- Extensible for future Claude models (opus, haiku, etc.)

**Detection Risk**:
- **Without modelId**: Requests may be rejected or flagged as non-Antigravity
- **With modelId**: Proper routing and identification as legitimate Antigravity request

---

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| v1internal API rejects requests without modelId | Medium | High | Test with real API, implement in Story #1 |
| Wrong model ID causes routing failure | Low | High | Validate against RE spec, extensive testing |
| Performance degradation | Low | Low | Compile-time match is instant |

---

## Related Stories

**Next in Sequence**:
- CLAUDE-THINKING-001-02: Add API Provider and Model Provider Constants

**Same FR Group** (FR1: Model Provider Information):
- CLAUDE-THINKING-001-01: Model ID (this story)
- CLAUDE-THINKING-001-02: API/Model Provider

---

## Change Log

| Date | Change | Author |
|------|--------|--------|
| 2026-01-10 | Story created | BMad Master |
| 2026-01-10 | Assigned to Development Team | BMad Master |
