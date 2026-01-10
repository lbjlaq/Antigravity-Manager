# Antigravity Workflow Compliance - Gap Analysis & Implementation Plan

**Date**: 2026-01-10
**Version**: 1.0
**Status**: Draft - For Review
**Objective**: Achieve 100% workflow compliance with Google Antigravity v1.13.3

---

## Executive Summary

This document analyzes the current API Proxy implementation against reverse-engineered Google Antigravity v1.13.3 workflows and identifies gaps that need to be addressed for perfect mimicry.

**Current Compliance**: ~75-80%
**Target Compliance**: 100%
**Critical Gaps**: 8 identified
**Medium Gaps**: 12 identified
**Minor Gaps**: 5 identified

---

## 📊 Compliance Overview

### ✅ Currently Compliant (75-80%)

**Identity & Authentication**:
- ✅ User-Agent: `antigravity/1.13.3 darwin/arm64` (client.rs:26)
- ✅ Request ID format: `agent-{uuid}` (request.rs:366)
- ✅ v1internal endpoints with fallback (client.rs:10-15)
- ✅ OAuth 2.0 Bearer token authentication
- ✅ HTTP headers: Content-Type, Authorization, User-Agent

**System Instructions**:
- ✅ Antigravity identity: "You are Antigravity..." (request.rs:496-499)
- ✅ Hybrid injection (user + automatic)

**Safety Settings**:
- ✅ All categories set to OFF/configurable (request.rs:93-99)
- ✅ Configurable via GEMINI_SAFETY_THRESHOLD environment variable

**Thinking Mode**:
- ✅ ThinkingConfig with includeThoughts + thinkingBudget (request.rs:1176-1197)
- ✅ Model-specific budget limits (Gemini: 24576, Claude: 32000)
- ✅ Signature lifecycle management (mappers/signature_store.rs)
- ✅ Thinking block structure with `thought: true` (mappers/claude/models.rs)

**Request Structure**:
- ✅ Project ID selection
- ✅ Request/response transformation (Claude ↔ Gemini v1internal)
- ✅ Tool use support (function calling)

---

## ⚠️ Partial Compliance (Needs Enhancement)

### 1. **Model Name Mapping** (⚠️ PARTIAL - Critical)

**Current State**:
- Has basic mapping via `model_mapping::map_claude_model_to_gemini()`
- Missing comprehensive model ID to model name mapping

**Required (from reverse engineering)**:
```yaml
claude_models:
  - claude-4-sonnet (ID: 281) → claude-4-sonnet
  - claude-4-sonnet-thinking (ID: 282) → claude-4-sonnet-thinking
  - claude-4-opus (ID: 290) → claude-4-opus
  - claude-4-opus-thinking (ID: 291) → claude-4-opus-thinking
  - claude-4.5-sonnet (ID: 333) → claude-4.5-sonnet
  - claude-4.5-sonnet-thinking (ID: 334) → claude-4.5-sonnet-thinking
  - claude-4.5-haiku (ID: 340) → claude-4.5-haiku
  - claude-4.5-haiku-thinking (ID: 341) → claude-4.5-haiku-thinking

gemini_models:
  - gemini-2.5-flash (ID: 312) → gemini-2.5-flash
  - gemini-2.5-flash-thinking (ID: 313) → gemini-2.5-flash-thinking
  - gemini-2.5-flash-thinking-tools (ID: 329) → gemini-2.5-flash-thinking-tools
  - gemini-2.5-flash-lite (ID: 330) → gemini-2.5-flash-lite
  - gemini-2.5-pro (ID: 246) → gemini-2.5-pro
  - gemini-2.5-pro-eval (ID: 331) → gemini-2.5-pro-eval
```

**Gap**: Missing model ID constants and comprehensive validation
**Priority**: P0 (Critical)
**Effort**: 3 hours

---

### 2. **Thinking Block Position Enforcement** (⚠️ PARTIAL - High)

**Current State**:
- Thinking blocks are created and managed
- No strict enforcement of "thinking block MUST be first part"

**Required (from docs/antigravity/workflows/models/gemini/gemini-2.5-flash-thinking-workflow.md:89-109)**:
```json
// ✅ CORRECT
{
  "role": "model",
  "parts": [
    {"text": "Thinking...", "thought": true, "thoughtSignature": "..."},
    {"text": "Response..."}
  ]
}

// ❌ INCORRECT - Thinking block not first
{
  "role": "model",
  "parts": [
    {"text": "Response..."},
    {"text": "Thinking...", "thought": true}  // WRONG POSITION
  ]
}
```

**Gap**: Need validation function to ensure thinking block is always first
**Priority**: P0 (Critical)
**Effort**: 2 hours

---

### 3. **Budget Constraint Validation** (⚠️ PARTIAL - High)

**Current State**:
- maxOutputTokens calculation exists
- Not strictly enforced that `maxOutputTokens > thinkingBudget`

**Required (from docs/antigravity/workflows/models/gemini/gemini-2.5-flash-thinking-workflow.md:113-131)**:
```json
// ✅ CORRECT
{
  "maxOutputTokens": 40000,
  "thinkingConfig": {
    "thinkingBudget": 24576  // 40000 > 24576 ✓
  }
}

// ❌ INCORRECT - Equal or less
{
  "maxOutputTokens": 24576,
  "thinkingConfig": {
    "thinkingBudget": 24576  // 24576 = 24576 ✗ DETECTED
  }
}
```

**Current Implementation (request.rs:1249-1264)**:
```rust
// Has logic but not strictly enforced
let clamped_budget = if has_web_search || mapped_model.contains("gemini-2.5-flash") {
    budget.min(24576)
} else if mapped_model.contains("claude") {
    budget.min(32000)
} else if mapped_model.contains("gemini") {
    budget.min(32000)
} else {
    budget
};
```

**Gap**: Need explicit validation with error message
**Priority**: P0 (Critical)
**Effort**: 1 hour

---

### 4. **Request Metadata** (⚠️ PARTIAL - Medium)

**Current State**:
- `userAgent`: "antigravity" set in request body (request.rs:374)
- `requestType`: Set via config (request.rs:375)
- Missing `ideType: ANTIGRAVITY` in metadata

**Required (from docs/antigravity/api/claude-integration-analysis.md and workflows)**:
```json
{
  "project": "project-id",
  "requestId": "agent-uuid",
  "model": "claude-4.5-sonnet-thinking",
  "userAgent": "antigravity",
  "requestType": "GENERATE_CONTENT",
  "request": {
    "metadata": {
      "ideType": "ANTIGRAVITY",
      "ideVersion": "1.13.3",
      "platform": "DARWIN"
    },
    "contents": [...],
    ...
  }
}
```

**Gap**: Missing metadata.ideType, ideVersion, platform in request
**Priority**: P1 (High)
**Effort**: 2 hours

---

### 5. **Thinking Signature Validation** (⚠️ PARTIAL - Medium)

**Current State** (claude.rs:40-53):
```rust
fn has_valid_signature(block: &ContentBlock) -> bool {
    match block {
        ContentBlock::Thinking { signature, thinking, .. } => {
            // Empty thinking + any signature = valid (trailing signature case)
            if thinking.is_empty() && signature.is_some() {
                return true;
            }
            // Has content + sufficient length signature = valid
            signature.as_ref().map_or(false, |s| s.len() >= MIN_SIGNATURE_LENGTH)
        }
        _ => true  // Non-thinking blocks default valid
    }
}
```

**Required (from docs/antigravity/workflows/models/gemini/gemini-2.5-flash-thinking-workflow.md:48-84)**:
- Signatures are JWT tokens
- MUST be inherited from previous thinking blocks
- Cannot be forged or modified
- Server validates signature authenticity

**Gap**:
- Current check is length-based (MIN_SIGNATURE_LENGTH: 10)
- No JWT structure validation
- No signature source tracking (server-generated vs cached)

**Priority**: P1 (High)
**Effort**: 4 hours

---

### 6. **Tool Configuration Mode** (⚠️ PARTIAL - Medium)

**Current State** (request.rs:333-338):
```rust
if let Some(tools_val) = tools {
    inner_request["tools"] = tools_val;
    // Explicit tool mode setting to VALIDATED
    inner_request["toolConfig"] = json!({
        "functionCallingConfig": {
            "mode": "VALIDATED"
        }
    });
}
```

**Required Modes (from reverse engineering)**:
```yaml
tool_config_modes:
  AUTO: "Let model decide when to use tools"
  ANY: "Force model to use a tool"
  NONE: "Disable tool use"
  VALIDATED: "Strict mode with validation (current)"
```

**Gap**: Always using VALIDATED mode, should be configurable per request
**Priority**: P2 (Medium)
**Effort**: 2 hours

---

### 7. **Grounding Configuration** (⚠️ PARTIAL - Medium)

**Current State**:
- Has `inject_google_search` logic (request.rs:341-343)
- Missing full grounding configuration structure

**Required (from docs/antigravity/api/gemini-integration-analysis.md)**:
```json
{
  "geminiSettings": {
    "groundingType": "GROUNDING_WITH_GOOGLE_SEARCH",
    "recitationPolicy": {
      "blockThreshold": "BLOCK_MEDIUM_AND_ABOVE"
    },
    "disableTelemetry": false,
    "disableFeedback": false
  }
}
```

**Gap**: Missing geminiSettings structure with grounding configuration
**Priority**: P2 (Medium)
**Effort**: 3 hours

---

## ❌ Critical Gaps (Missing Implementation)

### 8. **Provider-Specific API Routing** (❌ MISSING - Critical)

**Current State**:
- All requests go to v1internal:generateContent
- No differentiation between Gemini (direct) and Claude (via Vertex AI)

**Required (from docs/antigravity/api/provider-comparison.md)**:

**Gemini (Direct)**:
```
Extension → Language Server → Cloud Code API → Gemini Backend
```

**Claude (via Vertex AI)**:
```
Extension → Language Server → Vertex AI → Claude Backend
```

**Expected Request Structure Differences**:
```json
// Gemini request
{
  "model": "gemini-2.5-flash-thinking",
  "api_provider": "API_PROVIDER_GOOGLE_GEMINI (24)",
  "model_provider": "MODEL_PROVIDER_GOOGLE (4)"
}

// Claude request
{
  "model": "claude-4.5-sonnet-thinking",
  "api_provider": "API_PROVIDER_ANTHROPIC_VERTEX (26)",
  "model_provider": "MODEL_PROVIDER_ANTHROPIC (3)"
}
```

**Gap**:
- Missing api_provider and model_provider fields in request
- No routing logic based on provider
- All requests use same endpoint structure

**Priority**: P0 (Critical)
**Effort**: 6 hours

---

### 9. **Model Feature Matrix Validation** (❌ MISSING - High)

**Current State**:
- Basic model validation exists
- No feature matrix (which models support what)

**Required (from docs/antigravity/workflows/models/README.md)**:
```yaml
model_features:
  claude-4.5-sonnet-thinking:
    thinking: true (32000 tokens)
    tool_use: true
    web_search: false
    multimodal: true (images)

  gemini-2.5-flash-thinking:
    thinking: true (24576 tokens)
    tool_use: true
    web_search: true
    multimodal: false

  gemini-2.5-flash:
    thinking: false
    tool_use: true
    web_search: true
    multimodal: false
```

**Gap**: No validation that requested features are supported by selected model
**Priority**: P1 (High)
**Effort**: 4 hours

---

### 10. **Error Pattern Handling** (❌ MISSING - High)

**Current State**:
- Basic error handling with retries
- Missing specific error pattern recovery

**Required (from docs/antigravity/reference/error-pattern-catalog.md)**:

**Thinking-Specific Errors**:
```yaml
400_invalid_thinking_budget:
  error: "max_tokens must be greater than thinking_budget"
  recovery: "maxOutputTokens = thinkingBudget + 100"

400_missing_thinking_block:
  error: "Thinking enabled but assistant message lacks thinking block"
  recovery: "Insert thinking block as first part"

400_corrupted_signature:
  error: "Invalid thought signature"
  recovery: "Clear cache and regenerate" (ENABLE RETRY!)
```

**Gap**: Missing specialized error recovery for thinking mode errors
**Priority**: P1 (High)
**Effort**: 5 hours

---

### 11. **Quota Response Parsing** (❌ MISSING - High)

**Current State**:
- Has quota fetching via fetchAvailableModels
- Missing quota info extraction from responses

**Required (from docs/antigravity/api/quota-apis.md)**:
```json
{
  "candidates": [...],
  "usageMetadata": {
    "promptTokenCount": 123,
    "candidatesTokenCount": 3456,
    "totalTokenCount": 3579,
    "thinkingTokenCount": 1234
  },
  "quotaInfo": {
    "remainingFraction": 0.45,
    "resetTime": "2026-01-11T00:00:00Z"
  }
}
```

**Gap**:
- Not extracting quotaInfo from responses
- Not tracking thinkingTokenCount separately

**Priority**: P1 (High - needed for proactive quota monitoring)
**Effort**: 3 hours (integrates with QUOTA-001 epic)

---

### 12. **Session Management** (❌ MISSING - Medium)

**Current State** (request.rs:378-383):
```rust
// If metadata.user_id provided, reuse as sessionId
if let Some(metadata) = &claude_req.metadata {
    if let Some(user_id) = &metadata.user_id {
        body["request"]["sessionId"] = json!(user_id);
    }
}
```

**Required (from reverse engineering)**:
```json
{
  "request": {
    "sessionId": "persistent-session-id",
    "workspace_id": "workspace_abc123",
    "cloudaicompanion_project": "projects/cloudaicompanion-123"
  }
}
```

**Gap**:
- Missing workspace_id management
- Missing cloudaicompanion_project
- sessionId only set if metadata.user_id provided

**Priority**: P2 (Medium)
**Effort**: 2 hours

---

### 13. **Response Format Validation** (❌ MISSING - Medium)

**Current State**:
- Response transformation exists (mappers/claude/response.rs)
- No validation that response matches expected format

**Required**:
- Validate response structure matches Gemini v1internal format
- Validate thinking blocks in responses have correct structure
- Validate signatures are present when thinking enabled

**Gap**: Missing response validation layer
**Priority**: P2 (Medium)
**Effort**: 3 hours

---

### 14. **Streaming Response Compliance** (❌ MISSING - Medium)

**Current State** (mappers/claude/streaming.rs exists):
- Has SSE streaming implementation
- Unknown if format matches exactly

**Required (from docs/antigravity/workflows/models/claude/claude-4-5-sonnet-thinking-workflow.md:474-487)**:
```
data: {"candidates":[{"content":{"role":"model","parts":[{"text":"Let","thought":true}]}}]}

data: {"candidates":[{"content":{"role":"model","parts":[{"text":" me","thought":true}]}}]}

...

data: {"candidates":[{"content":{"role":"model","parts":[{"text":"## Architecture"}]}}]}

...

data: [DONE]
```

**Gap**: Need to verify SSE format matches exactly
**Priority**: P2 (Medium)
**Effort**: 2 hours

---

### 15. **Image Support** (❌ MISSING - Medium)

**Current State**:
- Has basic image handling (ContentBlock::Image)
- Missing complete image workflow

**Required (from workflows/models/README.md:433-448)**:
```json
{
  "contents": [{
    "role": "user",
    "parts": [
      {"text": "Analyze this image"},
      {
        "inlineData": {
          "mimeType": "image/png",
          "data": "base64..."
        }
      }
    ]
  }]
}
```

**Gap**:
- Need to verify image data format matches
- Missing mimeType validation
- Missing Claude multimodal support validation

**Priority**: P3 (Low)
**Effort**: 2 hours

---

## 🎯 Implementation Priority Matrix

### P0 - Critical (Must Fix Immediately)

| # | Gap | Effort | Impact |
|---|-----|--------|--------|
| 1 | Model Name Mapping | 3h | Detection risk - HIGH |
| 2 | Thinking Block Position | 2h | Detection risk - HIGH |
| 3 | Budget Constraint Validation | 1h | API rejection - HIGH |
| 8 | Provider-Specific Routing | 6h | Core architecture - CRITICAL |

**Total P0 Effort**: 12 hours

---

### P1 - High (Fix Within Sprint)

| # | Gap | Effort | Impact |
|---|-----|--------|--------|
| 4 | Request Metadata (ideType) | 2h | Detection risk - MEDIUM |
| 5 | Signature Validation | 4h | Thinking mode reliability - HIGH |
| 9 | Model Feature Matrix | 4h | User experience - MEDIUM |
| 10 | Error Pattern Handling | 5h | Reliability - HIGH |
| 11 | Quota Response Parsing | 3h | Proactive monitoring - HIGH |

**Total P1 Effort**: 18 hours

---

### P2 - Medium (Fix Next Sprint)

| # | Gap | Effort | Impact |
|---|-----|--------|--------|
| 6 | Tool Configuration Mode | 2h | Flexibility - LOW |
| 7 | Grounding Configuration | 3h | Feature completeness - MEDIUM |
| 12 | Session Management | 2h | Feature completeness - LOW |
| 13 | Response Validation | 3h | Quality assurance - MEDIUM |
| 14 | Streaming Compliance | 2h | Format accuracy - MEDIUM |

**Total P2 Effort**: 12 hours

---

### P3 - Low (Nice to Have)

| # | Gap | Effort | Impact |
|---|-----|--------|--------|
| 15 | Image Support | 2h | Feature completeness - LOW |

**Total P3 Effort**: 2 hours

---

## 📋 Implementation Roadmap

### Phase 1: Critical Compliance (Week 1 - 12 hours)

**Objective**: Fix detection-critical gaps

**Tasks**:
1. ✅ Implement comprehensive model name mapping with ID constants
2. ✅ Add thinking block position enforcement
3. ✅ Add strict budget constraint validation with error handling
4. ✅ Implement provider-specific API routing (api_provider, model_provider fields)

**Deliverables**:
- `src-tauri/src/proxy/common/model_mapping.rs` - Enhanced with ID constants
- `src-tauri/src/proxy/mappers/claude/validation.rs` - New validation module
- `src-tauri/src/proxy/providers/` - New provider routing module

**Success Criteria**:
- All P0 gaps resolved
- No detection-critical issues remaining
- Pass provider routing tests

---

### Phase 2: High-Priority Enhancements (Week 2 - 18 hours)

**Objective**: Improve reliability and feature completeness

**Tasks**:
1. ✅ Add request metadata (ideType, ideVersion, platform)
2. ✅ Enhance signature validation with JWT structure check
3. ✅ Implement model feature matrix validation
4. ✅ Add thinking-mode-specific error recovery patterns
5. ✅ Implement quota response parsing and tracking

**Deliverables**:
- `src-tauri/src/proxy/mappers/common/metadata.rs` - Metadata builder
- `src-tauri/src/proxy/mappers/signature_store.rs` - Enhanced validation
- `src-tauri/src/proxy/common/model_features.rs` - Feature matrix
- `src-tauri/src/proxy/handlers/error_recovery.rs` - Error patterns
- `src-tauri/src/proxy/quota/response_parser.rs` - Quota extraction

**Success Criteria**:
- All P1 gaps resolved
- Thinking mode highly reliable
- Quota tracking functional
- Feature validation prevents unsupported combinations

---

### Phase 3: Medium-Priority Polish (Week 3 - 12 hours)

**Objective**: Complete feature set and improve flexibility

**Tasks**:
1. ✅ Add configurable tool calling mode (AUTO, ANY, NONE, VALIDATED)
2. ✅ Implement full grounding configuration (geminiSettings)
3. ✅ Add session management (workspace_id, cloudaicompanion_project)
4. ✅ Add response format validation
5. ✅ Verify streaming response format compliance

**Deliverables**:
- `src-tauri/src/proxy/mappers/claude/tools.rs` - Enhanced tool config
- `src-tauri/src/proxy/mappers/gemini/grounding.rs` - Grounding config
- `src-tauri/src/proxy/common/session.rs` - Session management
- `src-tauri/src/proxy/mappers/validation/response.rs` - Response validator

**Success Criteria**:
- All P2 gaps resolved
- Full feature parity with Antigravity v1.13.3
- Streaming format verified

---

### Phase 4: Final Polish (Week 4 - 2 hours)

**Objective**: Complete remaining nice-to-have features

**Tasks**:
1. ✅ Verify image support completeness
2. ✅ Final compliance testing
3. ✅ Documentation updates

**Deliverables**:
- Updated workflow documentation
- Compliance test suite
- Migration guide

**Success Criteria**:
- 100% compliance achieved
- All tests passing
- Documentation complete

---

## 🧪 Testing Strategy

### Compliance Test Suite

**Unit Tests**:
- Model name mapping validation
- Thinking block position enforcement
- Budget constraint validation
- Signature validation
- Feature matrix validation

**Integration Tests**:
- End-to-end Claude → Gemini v1internal → Response flow
- Thinking mode with signature lifecycle
- Provider routing (Gemini direct vs Claude via Vertex)
- Error recovery patterns
- Quota extraction and tracking

**Compliance Tests**:
- Compare requests against reverse-engineered examples
- Validate all required fields present
- Validate field formats match exactly
- Validate error responses match expected patterns

**Regression Tests**:
- Existing functionality continues to work
- No performance degradation
- No increased error rates

---

## 📊 Success Metrics

**Compliance Score**: (Compliant Items / Total Items) * 100%

**Target Metrics**:
- Phase 1: ≥90% compliance (critical items fixed)
- Phase 2: ≥95% compliance (high-priority fixed)
- Phase 3: ≥98% compliance (medium-priority fixed)
- Phase 4: 100% compliance (all items fixed)

**Quality Metrics**:
- API error rate: <1% (target: <0.5%)
- Thinking mode success rate: >95% (target: >98%)
- Detection incidents: 0 (must be zero)
- User-reported issues: <5% regression

---

## 🔗 Related Documents

- [Reverse Engineering Master Summary](../antigravity/MASTER-SUMMARY.md)
- [Provider Comparison](../antigravity/api/provider-comparison.md)
- [Claude Workflow](../antigravity/workflows/models/claude/claude-4-5-sonnet-thinking-workflow.md)
- [Gemini Workflow](../antigravity/workflows/models/gemini/gemini-2.5-flash-thinking-workflow.md)
- [Error Pattern Catalog](../antigravity/reference/error-pattern-catalog.md)
- [Quota APIs](../antigravity/api/quota-apis.md)
- [Proactive Quota Monitoring Epic](./proactive-quota-monitoring-spec.md)

---

**Document History**:
- 2026-01-10: Initial draft created from comprehensive analysis
- 2026-01-10: Ready for review and discussion

---

**Next Steps**:
1. Review this gap analysis with team
2. Prioritize any additional gaps identified
3. Approve implementation roadmap
4. Begin Phase 1 implementation

