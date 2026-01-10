# Story CLAUDE-THINKING-001-02: Add API Provider and Model Provider Constants

**Story ID**: CLAUDE-THINKING-001-02
**Epic**: [CLAUDE-THINKING-001 - Claude 4.5 Sonnet Thinking Compliance](../epics/claude-4-5-sonnet-thinking-compliance-epic.md)
**Priority**: P0 (Critical)
**Estimate**: 2 story points (1.5 hours)
**Status**: To Do
**Assignee**: TBD

---

## User Story

**As a** API Proxy developer
**I want** to add API Provider (26) and Model Provider (3) constants with helper functions
**So that** all Claude requests include proper routing information for Vertex AI proxy layer

---

## Context

Requests to v1internal API must include provider routing information to distinguish Claude models (routed through Vertex AI) from Gemini models (direct routing). This is **critical for proper API routing** and anti-detection compliance.

**Current Request** (Missing providers):
```json
{
  "project": "bamboo-precept-lgxtn",
  "model": "claude-4.5-sonnet-thinking",
  "modelId": 334,
  "request": {...}
}
```

**Expected Request** (With providers):
```json
{
  "project": "bamboo-precept-lgxtn",
  "model": "claude-4.5-sonnet-thinking",
  "modelId": 334,
  "apiProvider": 26,
  "modelProvider": 3,
  "request": {...}
}
```

**Detection Risk**: CRITICAL - Missing provider fields may cause routing failures or identify request as non-Antigravity.

---

## Reference Documents

**Primary**:
1. `docs/comparison/claude/claude-4-5-sonnet/current-implementation-thinking.md`
   - Lines 2744-2872: Gap Analysis #1 (Model Provider Information)
   - Lines 2819-2867: Implementation example

2. `docs/technical-specs/antigravity-workflow-compliance-gap-analysis.md`
   - Lines 56-86: Model Name Mapping

**Reverse Engineering Spec**:
- `docs/antigravity/workflows/models/claude/claude-4-5-sonnet-thinking-workflow.md`
   - Lines 161-166: apiProvider: 26 (ANTHROPIC_VERTEX), modelProvider: 3 (ANTHROPIC)

---

## Technical Details

### Implementation Steps

**File**: `src-tauri/src/proxy/mappers/claude/request.rs`

**Step 1 - Add Provider Constants**:
```rust
// Model ID constants (from Story #1)
const CLAUDE_4_5_SONNET_THINKING_MODEL_ID: u32 = 334;
const CLAUDE_4_5_SONNET_MODEL_ID: u32 = 333;

// API Provider constants
// Reference: docs/antigravity/workflows/models/claude/claude-4-5-sonnet-thinking-workflow.md:161-166
const API_PROVIDER_ANTHROPIC_VERTEX: u32 = 26;  // Claude models → Vertex AI
const API_PROVIDER_GEMINI: u32 = 0;  // Gemini models → direct

// Model Provider constants
const MODEL_PROVIDER_ANTHROPIC: u32 = 3;  // Anthropic (Claude)
const MODEL_PROVIDER_GEMINI: u32 = 1;     // Google (Gemini)
const MODEL_PROVIDER_UNKNOWN: u32 = 0;    // Unknown
```

**Step 2 - Add API Provider Helper**:
```rust
/// Get API provider for v1internal routing
/// Claude models route through ANTHROPIC_VERTEX (26)
/// Gemini models use direct routing (0)
fn get_api_provider(model_name: &str) -> u32 {
    if model_name.starts_with("claude-") {
        API_PROVIDER_ANTHROPIC_VERTEX  // 26
    } else if model_name.starts_with("gemini-") {
        API_PROVIDER_GEMINI  // 0
    } else {
        API_PROVIDER_GEMINI  // Default
    }
}
```

**Step 3 - Add Model Provider Helper**:
```rust
/// Get model provider enum for Antigravity
/// Distinguishes Anthropic (3) from Google (1) models
fn get_model_provider(model_name: &str) -> u32 {
    if model_name.starts_with("claude-") {
        MODEL_PROVIDER_ANTHROPIC  // 3
    } else if model_name.starts_with("gemini-") {
        MODEL_PROVIDER_GEMINI  // 1
    } else {
        MODEL_PROVIDER_UNKNOWN  // 0
    }
}
```

**Step 4 - Update Request Assembly**:
```rust
let mut body = json!({
    "project": project_id,
    "requestId": request_id,
    "model": config.final_model,
    "modelId": get_model_id(&config.final_model),           // Story #1
    "apiProvider": get_api_provider(&config.final_model),   // 🆕 ADD THIS
    "modelProvider": get_model_provider(&config.final_model), // 🆕 ADD THIS
    "userAgent": "antigravity",
    "requestType": config.request_type,
    "request": inner_request,
});
```

---

## Acceptance Criteria

**Given** a request for any Claude model
**When** the request is transformed to v1internal format
**Then** the request body includes `"apiProvider": 26`
**And** the request body includes `"modelProvider": 3`

**And** apiProvider is retrieved via helper function (not hardcoded)
**And** modelProvider is retrieved via helper function (not hardcoded)
**And** Gemini models return different provider values (0 and 1 respectively)

---

## Testing Requirements

### Unit Tests

```rust
// API Provider Tests
#[test]
fn test_get_api_provider_claude() {
    assert_eq!(get_api_provider("claude-4.5-sonnet-thinking"), 26);
    assert_eq!(get_api_provider("claude-4.5-sonnet"), 26);
}

#[test]
fn test_get_api_provider_gemini() {
    assert_eq!(get_api_provider("gemini-2.5-flash"), 0);
}

// Model Provider Tests
#[test]
fn test_get_model_provider_claude() {
    assert_eq!(get_model_provider("claude-4.5-sonnet-thinking"), 3);
}

#[test]
fn test_get_model_provider_gemini() {
    assert_eq!(get_model_provider("gemini-2.5-flash"), 1);
}

// Integration Test
#[test]
fn test_request_includes_providers() {
    let req = build_test_claude_request("claude-4.5-sonnet-thinking");
    let body = transform_claude_request_in(&req, "test-project").unwrap();

    assert_eq!(body["modelId"], 334);
    assert_eq!(body["apiProvider"], 26);
    assert_eq!(body["modelProvider"], 3);
}
```

### Manual Validation

- [ ] Send Claude request → verify apiProvider: 26, modelProvider: 3
- [ ] Send Gemini request → verify different provider values
- [ ] Check API logs for proper Vertex AI routing

---

## Definition of Done

**Code**:
- [ ] Constants added for API_PROVIDER_ANTHROPIC_VERTEX (26) and MODEL_PROVIDER_ANTHROPIC (3)
- [ ] Constants added for Gemini equivalents (0 and 1)
- [ ] `get_api_provider()` and `get_model_provider()` helper functions implemented
- [ ] Request assembly updated to include both provider fields
- [ ] Code reviewed and approved

**Testing**:
- [ ] 10 unit tests written and passing
- [ ] Integration tests passing for Claude and Gemini models
- [ ] Test coverage ≥80% for new code
- [ ] Manual validation against v1internal API

**Compliance**:
- [ ] **Compliance validation**: Provider fields match RE spec (apiProvider: 26, modelProvider: 3)
- [ ] **Performance benchmarking**: <100ms overhead for provider determination
- [ ] **Anti-detection validation**: Provider fields identical to Antigravity 1.13.3
- [ ] **Request format validation**: JSON structure validated
- [ ] **Error handling validation**: Unknown models handled gracefully

**Documentation**:
- [ ] Code comments reference RE spec line numbers (161-166)
- [ ] Implementation notes explain provider routing logic

---

## Dependencies

**Depends On**:
- CLAUDE-THINKING-001-01: Model ID implementation

**Blocks**:
- CLAUDE-THINKING-001-03: Metadata implementation

**Related**:
- Part of FR1: Model Provider Information (completes with Story #1)

---

## Implementation Notes

**Provider Values**:
- **apiProvider: 26** = ANTHROPIC_VERTEX (Claude routing layer)
- **modelProvider: 3** = ANTHROPIC (model provider enum)
- **Gemini**: apiProvider: 0 (direct), modelProvider: 1 (Google)

**Routing Impact**:
- Claude requests with apiProvider: 26 → route through Vertex AI → Anthropic backend
- Missing these fields = routing failures or detection

---

## Change Log

| Date | Change | Author |
|------|--------|--------|
| 2026-01-10 | Story created | BMad Master |
