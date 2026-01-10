# Story-004-05: Grounding Configuration (geminiSettings) for Claude 4.5 Sonnet (Standard)

**Epic**: [Epic-004](../epics/Epic-004-Claude-4.5-Sonnet-Standard-Compliance.md) - Claude 4.5 Sonnet (Standard) 100% Compliance
**Phase**: Phase 2 - Feature Parity (P2)
**Story ID**: Story-004-05
**Status**: ❌ **NOT IMPLEMENTED**
**Priority**: P2 (Medium) - **HIGH for Anti-Detection**
**Effort**: 1 hour
**Tags**: `[SHARED]`, `compliance`, `anti-detection`, `[NOT-IMPLEMENTED]`

---

## ⚠️ CRITICAL: Implementation Status

**This story's functionality is NOT IMPLEMENTED** for either standard (333) or thinking (334) models.

**Current Status**:
- ❌ geminiSettings field: **MISSING ENTIRELY**
- ❌ recitationPolicy configuration: **NOT IMPLEMENTED**
- ❌ No tests exist for grounding configuration
- ❌ **CRITICAL GAP**: Missing field is detectable anti-detection fingerprint

**Gap Confirmed** (`grep geminiSettings src-tauri/...`):
```bash
No matches found  # ❌ Completely absent from codebase
```

**Comparison Doc** (current-implementation.md:1192-1203):
```markdown
#### Current Implementation
**Missing Entirely**:
// ❌ No geminiSettings field
```

**Story Purpose**: FULL IMPLEMENTATION for both models (333 and 334)

**Anti-Detection Impact**: **CRITICAL** - Missing geminiSettings is clear fingerprint that proxy is not genuine Antigravity

---

## Story Overview

### User Story
**As a** developer building API proxy for 100% Antigravity compliance
**I want** geminiSettings configuration with recitationPolicy in all requests
**So that** our requests are indistinguishable from genuine Antigravity and include anti-plagiarism protection

### Business Value
- **Anti-Detection**: **CRITICAL** - geminiSettings always present in Antigravity (100% of requests)
- **Protocol Compliance**: Required field for Gemini Protocol v1internal API
- **Content Quality**: Anti-plagiarism protection prevents copyright issues
- **Attribution Policy**: Proper content attribution for referenced materials
- **Backend Analytics**: Matches Antigravity fingerprint, prevents client detection

### Related Stories
- **Story-003-10**: Grounding Configuration (Epic-003) ❌ NOT IMPLEMENTED
- **Story-004-01/02/03/04**: Phase 1 & 2 stories (various statuses)
- **Relationship**: 100% shared implementation for both models

---

## Problem Statement

### Current State

**Missing Field Entirely** (request.rs:434-498):
```rust
// Current request structure (simplified)
let mut inner_request = json!({
    "contents": contents,
    "safetySettings": safety_settings,
    "systemInstruction": system_instruction,
    "generationConfig": generation_config,
});

if let Some(tools_val) = tools {
    inner_request["tools"] = tools_val;
    inner_request["toolConfig"] = json!({
        "functionCallingConfig": { "mode": mode }
    });
}

// ❌ Missing: geminiSettings field
// ❌ No recitationPolicy configuration
// ❌ Request structure differs from Antigravity
```

**Expected Structure** (from Antigravity RE):
```json
{
  "request": {
    "contents": [...],
    "safetySettings": [...],
    "systemInstruction": {...},
    "generationConfig": {...},
    "toolConfig": {...},
    "geminiSettings": {              // ❌ MISSING
      "recitationPolicy": {
        "action": "BLOCK",           // Anti-plagiarism
        "threshold": "LOW"           // Strict policy
      }
    }
  }
}
```

### Why This is Critical

**Anti-Detection Rationale**:

1. **Always Present in Antigravity**: RE analysis confirms geminiSettings in 100% of requests
2. **Fingerprinting Vector**: Missing field is detectable by backend analytics
3. **Configuration Signature**: Specific values (BLOCK+LOW) match Antigravity profile
4. **Protocol Compliance**: Required for Gemini Protocol v1internal
5. **Backend Pattern Matching**: Google analytics detect non-standard request structures

**Detection Risk Without geminiSettings**:
```
Backend Analytics:
  IF geminiSettings is missing:
    → Flag as non-Antigravity client ⚠️
    → Potential for rate limiting or blocking
    → Analytics show "custom client" pattern
    → Clear fingerprint of proxy/wrapper
```

**With geminiSettings**:
```
Backend Analytics:
  IF geminiSettings.recitationPolicy.action == "BLOCK"
    AND geminiSettings.recitationPolicy.threshold == "LOW":
    → Matches Antigravity baseline ✅
    → No fingerprinting signal
    → Analytics show "standard client" pattern
    → Indistinguishable from genuine client
```

### Impact Assessment

| Impact Area | Without geminiSettings | With geminiSettings |
|-------------|----------------------|---------------------|
| **Anti-Detection** | ❌ Clear fingerprint | ✅ Indistinguishable |
| **Protocol Compliance** | ❌ Non-compliant | ✅ 100% compliant |
| **Content Quality** | ⚠️ No anti-plagiarism | ✅ Protected |
| **Backend Analytics** | ❌ "Custom client" | ✅ "Standard client" |
| **Rate Limiting Risk** | ⚠️ Higher risk | ✅ Lower risk |

---

## Acceptance Criteria

### AC-1: geminiSettings Field Implementation
**Verification Method**: Code + Unit test
```rust
// In map_claude_to_vertex_ai(), after toolConfig

// 🆕 Story #10: Add Gemini Settings (Anti-Plagiarism Protection)
// Required for 100% Antigravity compliance and anti-detection
inner_request["geminiSettings"] = json!({
    "recitationPolicy": {
        "action": "BLOCK",      // Block recited content
        "threshold": "LOW"      // Strict matching threshold
    }
});

tracing::debug!(
    "[Gemini-Settings] ✅ Added recitationPolicy: action=BLOCK, threshold=LOW"
);
```

**Test**:
```rust
#[test]
fn test_gemini_settings_present() {
    let req = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),  // Standard model
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("Hello".to_string()),
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

    let (body, _) = transform_claude_request_in(&req, "test-project").unwrap();
    let request = &body["request"];

    // ✅ Verify geminiSettings exists
    assert!(request.get("geminiSettings").is_some());

    // ✅ Verify recitationPolicy structure
    let gemini_settings = &request["geminiSettings"];
    assert_eq!(gemini_settings["recitationPolicy"]["action"], "BLOCK");
    assert_eq!(gemini_settings["recitationPolicy"]["threshold"], "LOW");
}
```

**Success Criteria**:
- geminiSettings field added after toolConfig
- recitationPolicy nested object with correct structure
- action: "BLOCK", threshold: "LOW" values
- Debug logging confirms field addition

---

### AC-2: Always Present (Not Conditional)
**Verification Method**: Integration test
```rust
#[test]
fn test_gemini_settings_always_present() {
    // Test 1: Standard model without tools
    let req1 = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),  // ← Standard (333)
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("Hello".to_string()),
        }],
        system: None,
        tools: None,  // ❌ No tools
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

    let (body1, _) = transform_claude_request_in(&req1, "test-project").unwrap();
    let request1 = &body1["request"];

    // ✅ geminiSettings present even without tools
    assert!(request1.get("geminiSettings").is_some());
    assert_eq!(body1["modelId"], 333);  // Standard model

    // Test 2: Standard model with tools
    let mut req2 = req1.clone();
    req2.tools = Some(vec![Tool {
        type_: None,
        name: Some("get_weather".to_string()),
        description: Some("Get weather".to_string()),
        input_schema: Some(json!({"type": "object"})),
    }]);

    let (body2, _) = transform_claude_request_in(&req2, "test-project").unwrap();
    let request2 = &body2["request"];

    // ✅ geminiSettings present with tools
    assert!(request2.get("geminiSettings").is_some());
    assert_eq!(body2["modelId"], 333);

    // ✅ Same configuration in both cases
    assert_eq!(
        request1["geminiSettings"]["recitationPolicy"]["action"],
        request2["geminiSettings"]["recitationPolicy"]["action"]
    );
}
```

**Success Criteria**:
- geminiSettings present regardless of tools
- geminiSettings present regardless of thinking config
- geminiSettings present in minimal requests
- Same values (BLOCK+LOW) in all cases
- Works for standard model (333)

---

### AC-3: Model Differentiation Test (Both Models)
**Verification Method**: Integration test
```rust
#[test]
fn test_gemini_settings_both_models() {
    // Test standard model (333)
    let req_standard = ClaudeRequest {
        model: "claude-4.5-sonnet".to_string(),  // ← Standard
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("Hello".to_string()),
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

    let (body_standard, _) = transform_claude_request_in(&req_standard, "test-project").unwrap();
    let request_standard = &body_standard["request"];

    // Test thinking model (334)
    let mut req_thinking = req_standard.clone();
    req_thinking.model = "claude-4.5-sonnet-thinking".to_string();

    let (body_thinking, _) = transform_claude_request_in(&req_thinking, "test-project").unwrap();
    let request_thinking = &body_thinking["request"];

    // ✅ Verify model IDs are different
    assert_eq!(body_standard["modelId"], 333);
    assert_eq!(body_thinking["modelId"], 334);

    // ✅ Verify geminiSettings IDENTICAL for both models
    assert_eq!(
        request_standard["geminiSettings"],
        request_thinking["geminiSettings"]
    );

    // ✅ Verify recitationPolicy values identical
    assert_eq!(
        request_standard["geminiSettings"]["recitationPolicy"]["action"],
        "BLOCK"
    );
    assert_eq!(
        request_thinking["geminiSettings"]["recitationPolicy"]["action"],
        "BLOCK"
    );
}
```

**Success Criteria**:
- geminiSettings works for both model IDs (333 and 334)
- recitationPolicy values identical for both models
- Only model ID differs, not geminiSettings configuration
- 100% code sharing confirmed

---

## Technical Design

### Implementation Location

**File**: `src-tauri/src/proxy/mappers/claude/request.rs`

**Location**: After toolConfig configuration (after line ~498)

**Code Addition**:
```rust
// Current code (line ~464-498)
if let Some(tools_val) = tools {
    inner_request["tools"] = tools_val;

    // Tool configuration logic...
    let tool_choice = claude_req.tool_choice.as_ref();
    let mode = tool_choice
        .map(|tc| tc.to_gemini_mode())
        .unwrap_or("VALIDATED");

    // Build function calling config
    let mut function_calling_config = json!({
        "mode": mode
    });

    // For specific tool forcing...
    if let Some(tool_name) = tool_choice.and_then(|tc| tc.get_tool_name()) {
        function_calling_config["allowedFunctionNames"] = json!([tool_name]);
    }

    inner_request["toolConfig"] = json!({
        "functionCallingConfig": function_calling_config
    });
}

// 🆕 Story #10: Add Gemini Settings (Anti-Plagiarism Protection)
// CRITICAL: Required for 100% Antigravity compliance and anti-detection
// This field MUST be present in ALL requests to match Antigravity baseline
inner_request["geminiSettings"] = json!({
    "recitationPolicy": {
        "action": "BLOCK",      // Block recited content (strictest policy)
        "threshold": "LOW"      // Block even low-confidence matches
    }
});

tracing::debug!(
    "[Gemini-Settings] ✅ Added recitationPolicy: action=BLOCK, threshold=LOW (anti-plagiarism)"
);

// Continue with inject_google_search, imageConfig, etc.
```

### Implementation Notes

1. **Location Choice**: After toolConfig, before inject_google_search
   - Logical grouping: Gemini-specific settings together
   - Consistent order: Matches Antigravity request structure
   - Not conditional: Always present, not dependent on other fields

2. **Value Justification**:
   - **action: "BLOCK"**: Strictest anti-plagiarism policy
   - **threshold: "LOW"**: Block even low-confidence recitation matches
   - **Matches Antigravity**: Exact values from reverse engineering
   - **Anti-Detection**: Prevents fingerprinting through different values

3. **No Model-Specific Logic**: Works identically for both models (333 and 334)

4. **Performance**: Minimal overhead (~0.1ms for JSON serialization)

---

## Implementation Plan

### Step 1: Add geminiSettings Field (20 min)
- Locate toolConfig code in request.rs (~line 498)
- Add geminiSettings after toolConfig
- Add recitationPolicy with BLOCK+LOW values
- Add debug logging
- Verify code compiles

### Step 2: Add AC-1 Test (15 min)
- Create `test_gemini_settings_present()`
- Use standard model (333)
- Verify geminiSettings structure
- Verify recitationPolicy values

### Step 3: Add AC-2 Test (15 min)
- Create `test_gemini_settings_always_present()`
- Test without tools, with tools
- Test standard model (333)
- Verify unconditional presence

### Step 4: Add AC-3 Test (10 min)
- Create `test_gemini_settings_both_models()`
- Test both 333 and 334
- Verify identical configuration
- Confirm 100% code sharing

**Total Effort**: ~1 hour

---

## Testing Strategy

### Unit Tests (3 new tests)
- **AC-1**: geminiSettings structure validation
- **AC-2**: Unconditional presence verification
- **AC-3**: Model differentiation (333 vs 334)

### Test Execution
```bash
# Run all gemini settings tests
cargo test --package antigravity_tools_lib test_gemini_settings

# Run with output
cargo test --package antigravity_tools_lib test_gemini_settings -- --nocapture

# Run all request mapper tests
cargo test --package antigravity_tools_lib mappers::claude::request
```

### Success Criteria
- All 3 new tests pass ✅
- geminiSettings present in all requests ✅
- Values correct for both models (333 and 334) ✅
- No regressions in existing tests ✅

---

## Risk Assessment

### Technical Risks
- **Risk**: Backend might reject requests with geminiSettings
- **Mitigation**: Field is standard Gemini Protocol, matches Antigravity
- **Probability**: Very Low

- **Risk**: Values might need adjustment
- **Mitigation**: BLOCK+LOW matches reverse engineering exactly
- **Probability**: Very Low

### Implementation Risks
- **Risk**: Incorrect field placement
- **Mitigation**: Place after toolConfig, matches Antigravity structure
- **Probability**: Low

---

## Compliance Impact

### Before Story-004-05
- **geminiSettings**: ❌ Missing entirely
- **recitationPolicy**: ❌ Not configured
- **Anti-Detection**: ❌ Clear fingerprint (missing field)
- **Protocol Compliance**: ❌ Non-compliant with Gemini Protocol
- **Compliance Score**: 95% (missing critical anti-detection field)

### After Story-004-05
- **geminiSettings**: ✅ Present in all requests
- **recitationPolicy**: ✅ Configured (BLOCK+LOW)
- **Anti-Detection**: ✅ Indistinguishable from Antigravity
- **Protocol Compliance**: ✅ 100% compliant
- **Compliance Score**: 100% (**COMPLETE**)

**Compliance Progression**: 95% → **100%** ✅

---

## Dependencies

### Prerequisites
- ✅ Story-004-01/02/03/04: Phase 1 & 2 stories (various statuses)
- ✅ Request mapping infrastructure exists

### Blocking Issues
None - straightforward field addition

### Parallel Work
Can be developed in parallel with Story-004-06 (Session Metadata)

---

## Definition of Done

### Code
- [ ] geminiSettings field added to request.rs
- [ ] recitationPolicy configured (BLOCK+LOW)
- [ ] Field placed after toolConfig
- [ ] Debug logging added
- [ ] Code formatted with `cargo fmt`
- [ ] No clippy warnings

### Testing
- [ ] AC-1 test passes (geminiSettings structure)
- [ ] AC-2 test passes (always present)
- [ ] AC-3 test passes (both models)
- [ ] No regressions in existing tests
- [ ] Test coverage complete

### Documentation
- [ ] Story-004-05 marked complete
- [ ] Epic-004 progress updated (6/6 stories DONE)
- [ ] Implementation notes documented
- [ ] Code comments reference story

### Validation
- [ ] geminiSettings present in all requests
- [ ] Values correct (BLOCK+LOW)
- [ ] Works for both models (333 and 334)
- [ ] Anti-detection compliance validated
- [ ] **Compliance score: 100%** ✅

---

## Success Metrics

| Metric | Target | Validation |
|--------|--------|------------|
| Field implementation | Complete | Code + tests |
| Test pass rate | 100% | `cargo test` output |
| Both models support | Yes | AC-3 test |
| Compliance score | 100% | Gap analysis complete |
| No regressions | 0 failures | All tests pass |
| Anti-detection | Indistinguishable | Field matches Antigravity |

---

## File Impact Analysis

### Files Modified
**`src-tauri/src/proxy/mappers/claude/request.rs`** (+15 lines code, +80 lines tests)
- **Section**: After toolConfig (~line 498)
- **Code Changes**: Add geminiSettings with recitationPolicy
- **Test Changes**: Add 3 new tests (AC-1, AC-2, AC-3)
- **Impact**: Low - simple field addition

**Current File Stats**:
- Total lines: ~3500 lines
- Test lines: ~1068 lines (31%)
- After changes: +95 lines → ~3595 lines total

---

## Related Documentation

### Epic-004 Documents
- [Epic-004](../epics/Epic-004-Claude-4.5-Sonnet-Standard-Compliance.md) - Master epic
- [Story-004-01](Story-004-01-model-provider-constants.md) - Constants (IMPLEMENTED)
- [Story-004-02](Story-004-02-antigravity-metadata.md) - Metadata (IMPLEMENTED)
- [Story-004-03](Story-004-03-model-specific-routing.md) - Routing (IN PROGRESS)
- [Story-004-04](Story-004-04-flexible-tool-configuration-modes.md) - Tool Config (IMPLEMENTED + TEST-GAP)

### Epic-003 Reference (Shared Spec)
- [Story-003-10](Story-003-10-grounding-configuration.md) - Grounding Configuration ❌ NOT IMPLEMENTED

### Gap Analysis
- `docs/comparison/claude/claude-4-5-sonnet/current-implementation.md` - Lines 1168-1221 (geminiSettings gap)
- `docs/comparison/claude/claude-4-5-sonnet/EXECUTIVE-SUMMARY.md` - P2-2: Grounding Config

### Reverse Engineering Spec
- `docs/antigravity/workflows/models/claude/claude-4-5-sonnet-workflow.md` - geminiSettings specification

---

## Implementation Notes

### Why [SHARED] Tag?
- geminiSettings configuration: 100% identical for both models
- recitationPolicy values: Same for 333 and 334
- No model-specific logic needed
- Only model ID differs (in routing, not in geminiSettings)

### Why [NOT-IMPLEMENTED] Tag?
- Code completely absent from codebase
- No tests exist for grounding configuration
- Full implementation required (not just validation)
- Different from stories 004-01 through 004-04

### Relationship to Story-003-10
- **Spec**: Story-003-10 documents HOW to implement
- **Status**: Story-003-10 also NOT IMPLEMENTED (Pending)
- **Code**: 100% shared implementation between stories
- **Timeline**: Both stories implemented together

### Key Insights
1. **Critical for Anti-Detection**: Missing geminiSettings is clear fingerprint
2. **Always Present**: Not conditional, added to ALL requests
3. **Identical for Both Models**: No model-specific logic
4. **Simple Implementation**: ~15 lines of code, 3 tests
5. **High Impact**: Completes Epic-004 (100% compliance)

---

## Change Log

| Date | Change | Author |
|------|--------|--------|
| 2026-01-11 | Story-004-05 created | BMad Master |
| 2026-01-11 | CRITICAL: Identified geminiSettings NOT IMPLEMENTED | BMad Master |
| 2026-01-11 | Verified gap in comparison docs and codebase | BMad Master |
| 2026-01-11 | 3 AC defined for full implementation | BMad Master |
| 2026-01-11 | Anti-detection impact documented | BMad Master |

---

## Notes

### Critical Discovery
During deep analysis, discovered that:
1. ❌ geminiSettings completely absent from codebase
2. ❌ Story-003-10 also NOT IMPLEMENTED (both epics affected)
3. ❌ **HIGH RISK**: Missing field is anti-detection fingerprint
4. ✅ Simple fix: ~15 lines of code, but critical impact

### Why This Matters
- **Anti-Detection**: Backend analytics can detect missing geminiSettings
- **Protocol Compliance**: Required field for Gemini Protocol v1internal
- **Content Quality**: Missing anti-plagiarism protection
- **Fingerprinting**: Clear signal that proxy is not genuine Antigravity

### Anti-Detection Context
```
Antigravity Requests: geminiSettings ALWAYS present (100%)
Our Proxy Requests: geminiSettings MISSING (0%)
→ Clear fingerprint detectable by backend analytics
→ Risk of rate limiting, blocking, or special treatment
```

### Next Steps After Completion
1. Update Epic-004 progress (Story 5/6 complete → Phase 2 DONE)
2. Proceed to Story-004-06 (Session Metadata - Phase 3)
3. Validate compliance score reached 100%
4. **Document Epic-004 as COMPLETE** after Story-004-06

### Implementation Priority
**MEDIUM Priority (P2)** for feature parity
**HIGH Priority** for anti-detection compliance

Recommend implementing **together with Story-003-10** (same code, shared implementation).
