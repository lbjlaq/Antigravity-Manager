# Story-024-01: ideType Marker Addition (Anti-Detection)

**Epic**: Epic-024 - Anti-Detection Hardening
**Priority**: P0 (CRITICAL)
**Effort**: 8-10 hours (4-5 days with pair programming)
**Team**: Team 2 (Multi-Protocol Specialists)
**Assignee**: Dev 2A + Dev 2B (PAIR PROGRAMMING - security critical)
**Created**: 2026-01-12
**Status**: ✅ COMPLETE - QA PASSED (10/10)

---

## 🎯 Objective

Add `ideType: "ANTIGRAVITY"` markers to 15+ models across all three protocol mappers (Claude, Gemini, OpenAI) to achieve 100% model detection protection.

---

## 📊 Business Context

### Current State
```yaml
problem:
  detection_coverage: "Only 60% of models protected"
  missing_markers: "15+ models lack ideType field"
  risk: "Detection = service unavailability"
  severity: "P0 CRITICAL"

impact:
  affected_users: "ALL users (100%)"
  risk_type: "Service unavailability from detection"
  business_impact: "Revenue loss, user churn, reputation damage"
```

### Target State
```yaml
goal:
  detection_coverage: "100% of models protected"
  missing_markers: "0 models (all have ideType)"
  risk: "ELIMINATED"

success_metrics:
  - "All 15+ models have ideType: 'ANTIGRAVITY'"
  - "Zero detection events after deployment"
  - "100% test coverage for ideType presence"
```

---

## 🔍 Gap Analysis

### Models Missing ideType Markers

#### Claude Models (5+ models)
```yaml
mappers/claude/models.rs:
  missing_ideType:
    - "claude-4.5-sonnet (standard mode)"
    - "claude-opus-4-5 (standard mode)"
    - "claude-3-5-sonnet-v2"
    - "claude-3-opus"
    - "claude-3-haiku"

  pattern: "Standard mode variants and legacy models missing markers"
```

#### Gemini Models (5+ models)
```yaml
mappers/gemini/models.rs:
  missing_ideType:
    - "gemini-2.0-flash-exp"
    - "gemini-2.5-flash (base)"
    - "gemini-2.5-flash-lite"
    - "gemini-2.5-pro (base)"
    - "gemini-for-google-2.5-pro"

  pattern: "Non-thinking variants and experimental models"
```

#### OpenAI Models (5+ models)
```yaml
mappers/openai/models.rs:
  missing_ideType:
    - "gpt-4o"
    - "gpt-4o-mini"
    - "gpt-4-turbo"
    - "gpt-3.5-turbo"
    - "o1-preview"

  pattern: "All OpenAI models (no ideType markers yet)"
```

---

## 📋 Acceptance Criteria

### AC1: ideType Markers Added (CRITICAL)
```gherkin
GIVEN a model definition in any mapper (claude/gemini/openai)
WHEN the model is instantiated or serialized
THEN it MUST include ideType: "ANTIGRAVITY" field
AND the field MUST be present in all request payloads
AND the field MUST survive protocol transformations
```

**Validation**:
- [ ] All 15+ identified models have ideType field
- [ ] ideType value is exactly "ANTIGRAVITY" (case-sensitive)
- [ ] Field is present in request JSON payloads
- [ ] Field survives OpenAI → Claude transformation
- [ ] Field survives OpenAI → Gemini transformation

---

### AC2: Protocol-Specific Implementation (CRITICAL)
```gherkin
GIVEN models across three protocol mappers
WHEN ideType markers are added
THEN each mapper MUST implement markers according to protocol spec
AND markers MUST be consistent across mappers
AND existing functionality MUST NOT be broken
```

**Claude Mapper**:
- [ ] `src-tauri/src/proxy/mappers/claude/models.rs` updated
- [ ] ideType added to model struct definitions
- [ ] ideType serialized in request builder
- [ ] Test coverage for all Claude models with ideType

**Gemini Mapper**:
- [ ] `src-tauri/src/proxy/mappers/gemini/models.rs` updated
- [ ] ideType added to model struct definitions
- [ ] ideType serialized in request builder
- [ ] Test coverage for all Gemini models with ideType

**OpenAI Mapper**:
- [ ] `src-tauri/src/proxy/mappers/openai/models.rs` updated
- [ ] ideType added to model struct definitions
- [ ] ideType included in OpenAI-formatted requests
- [ ] Test coverage for all OpenAI models with ideType

---

### AC3: Test Coverage (CRITICAL)
```gherkin
GIVEN new ideType markers in model definitions
WHEN tests are executed
THEN tests MUST validate ideType presence in all models
AND tests MUST validate ideType value correctness
AND tests MUST validate ideType in request payloads
```

**Test Requirements**:
- [ ] Unit tests: ideType present in model structs (15+ tests)
- [ ] Integration tests: ideType in request payloads (10+ tests)
- [ ] E2E tests: ideType survives protocol transformations (5+ tests)
- [ ] All tests passing (100%)

---

### AC4: Backward Compatibility (HIGH)
```gherkin
GIVEN existing models and requests
WHEN ideType markers are added
THEN existing functionality MUST NOT break
AND existing tests MUST continue passing
AND no regressions in API behavior
```

**Validation**:
- [ ] All existing tests still pass (398/398 from Epic-013)
- [ ] No breaking changes to request format
- [ ] No breaking changes to response handling
- [ ] Regression test suite passes

---

## 🛠️ Implementation Details

### File Changes

#### 1. Claude Mapper Models
**File**: `src-tauri/src/proxy/mappers/claude/models.rs`

**Current** (example - claude-4.5-sonnet thinking mode):
```rust
pub struct ClaudeModel {
    pub id: String,
    pub display_name: String,
    pub model_id: u32,
    pub api_provider: u32,
    // ideType missing for standard mode variants
}
```

**Target**:
```rust
pub struct ClaudeModel {
    pub id: String,
    pub display_name: String,
    pub model_id: u32,
    pub api_provider: u32,
    pub ide_type: String,  // NEW: Add this field
}

// Implementation
impl ClaudeModel {
    pub fn claude_4_5_sonnet_standard() -> Self {
        Self {
            id: "claude-4.5-sonnet".to_string(),
            display_name: "Claude 4.5 Sonnet (Standard)".to_string(),
            model_id: 333,
            api_provider: 26, // ANTHROPIC_VERTEX
            ide_type: "ANTIGRAVITY".to_string(),  // NEW
        }
    }
}
```

**Models to Update** (5+):
- `claude-4.5-sonnet` (standard mode) - NEW ideType
- `claude-opus-4-5` (standard mode) - NEW ideType
- `claude-3-5-sonnet-v2` - ADD ideType
- `claude-3-opus` - ADD ideType
- `claude-3-haiku` - ADD ideType

---

#### 2. Gemini Mapper Models
**File**: `src-tauri/src/proxy/mappers/gemini/models.rs`

**Pattern**:
```rust
pub struct GeminiModel {
    pub id: String,
    pub display_name: String,
    pub ide_type: String,  // ADD this field to all models
    // ... other fields
}

impl GeminiModel {
    pub fn gemini_2_5_flash() -> Self {
        Self {
            id: "gemini-2.5-flash".to_string(),
            display_name: "Gemini 2.5 Flash".to_string(),
            ide_type: "ANTIGRAVITY".to_string(),  // NEW
            // ... other fields
        }
    }
}
```

**Models to Update** (5+):
- `gemini-2.0-flash-exp` - ADD ideType
- `gemini-2.5-flash` (base) - ADD ideType
- `gemini-2.5-flash-lite` - ADD ideType
- `gemini-2.5-pro` (base) - ADD ideType
- `gemini-for-google-2.5-pro` - ADD ideType

---

#### 3. OpenAI Mapper Models
**File**: `src-tauri/src/proxy/mappers/openai/models.rs`

**Pattern**:
```rust
pub struct OpenAIModel {
    pub id: String,
    pub display_name: String,
    pub ide_type: String,  // NEW field for all OpenAI models
    // ... other fields
}

impl OpenAIModel {
    pub fn gpt_4o() -> Self {
        Self {
            id: "gpt-4o".to_string(),
            display_name: "GPT-4o".to_string(),
            ide_type: "ANTIGRAVITY".to_string(),  // NEW
            // ... other fields
        }
    }
}
```

**Models to Update** (5+):
- `gpt-4o` - ADD ideType
- `gpt-4o-mini` - ADD ideType
- `gpt-4-turbo` - ADD ideType
- `gpt-3.5-turbo` - ADD ideType
- `o1-preview` - ADD ideType

---

#### 4. Request Serialization
**Files**:
- `src-tauri/src/proxy/mappers/claude/request.rs`
- `src-tauri/src/proxy/mappers/gemini/request.rs`
- `src-tauri/src/proxy/mappers/openai/request.rs`

**Pattern** (example for Claude):
```rust
// In request builder
pub fn build_claude_request(model: &ClaudeModel, ...) -> ClaudeRequest {
    ClaudeRequest {
        model: model.id.clone(),
        // ... other fields
        ide_type: model.ide_type.clone(),  // NEW: Include in request
    }
}

// Serialization
#[derive(Serialize)]
pub struct ClaudeRequest {
    pub model: String,
    // ... other fields
    #[serde(rename = "ideType")]
    pub ide_type: String,  // NEW: Serialize as "ideType"
}
```

---

### Test Strategy

#### Unit Tests (15+ tests)
**File**: `tests/security/ideType_markers_tests.rs` (NEW)

```rust
#[cfg(test)]
mod ideType_marker_tests {
    use super::*;

    #[test]
    fn test_claude_sonnet_standard_has_ideType() {
        let model = ClaudeModel::claude_4_5_sonnet_standard();
        assert_eq!(model.ide_type, "ANTIGRAVITY");
    }

    #[test]
    fn test_gemini_2_5_flash_has_ideType() {
        let model = GeminiModel::gemini_2_5_flash();
        assert_eq!(model.ide_type, "ANTIGRAVITY");
    }

    #[test]
    fn test_openai_gpt_4o_has_ideType() {
        let model = OpenAIModel::gpt_4o();
        assert_eq!(model.ide_type, "ANTIGRAVITY");
    }

    // ... 12+ more tests for all models
}
```

#### Integration Tests (10+ tests)
**File**: `tests/security/ideType_integration_tests.rs` (NEW)

```rust
#[tokio::test]
async fn test_claude_request_includes_ideType() {
    let request = build_test_claude_request();
    let json = serde_json::to_value(&request).unwrap();

    assert!(json.get("ideType").is_some());
    assert_eq!(json["ideType"], "ANTIGRAVITY");
}

#[tokio::test]
async fn test_gemini_request_includes_ideType() {
    let request = build_test_gemini_request();
    let json = serde_json::to_value(&request).unwrap();

    assert!(json.get("ideType").is_some());
    assert_eq!(json["ideType"], "ANTIGRAVITY");
}

// ... 8+ more integration tests
```

#### E2E Tests (5+ tests)
**File**: `tests/security/ideType_e2e_tests.rs` (NEW)

```rust
#[tokio::test]
async fn test_openai_to_claude_preserves_ideType() {
    // OpenAI format request → Claude transformation
    let openai_req = create_openai_request_with_ideType();
    let claude_req = transform_to_claude(openai_req);

    assert!(claude_req.contains_field("ideType"));
    assert_eq!(claude_req.get_ideType(), "ANTIGRAVITY");
}

// ... 4+ more E2E tests
```

---

## 🚨 Pair Programming Rationale

**Why Pair Programming?**

```yaml
security_critical: true
risk_level: "P0 CRITICAL"
impact: "100% user base"

pair_benefits:
  - "Real-time code review (security focus)"
  - "Reduced bug probability (2 pairs of eyes)"
  - "Knowledge sharing (Dev 2A security + Dev 2B protocol)"
  - "Faster debugging (immediate feedback)"

team_composition:
  dev_2a: "Senior Security Specialist (lead)"
  dev_2b: "Mid-Level Protocol Specialist (support)"

  collaboration:
    - "Dev 2A: Security validation, architecture decisions"
    - "Dev 2B: Protocol implementation, testing"
    - "Joint: Code review, edge case discussion"
```

---

## 🔗 Dependencies

### Prerequisite
- ✅ Epic-013 COMPLETE (2026-01-12) - no blockers

### Blocks
- 🔴 Story 024-02 (apiProvider fields) - depends on ideType structure
- 🟡 Epic-017 (Claude Sonnet) - will use ideType patterns from this story

### Enables
- ✅ 100% detection protection across all models
- ✅ Foundation for Epic-017/019 (Claude standard modes)
- ✅ Detection monitoring (Story 024-04)

---

## 📊 Success Metrics

### Code Metrics
```yaml
files_modified: "3 (claude/gemini/openai models.rs)"
models_updated: "15+ models"
lines_added: "~150-200 lines (ideType fields + serialization)"
test_coverage:
  unit_tests: "15+ tests (1 per model)"
  integration_tests: "10+ tests (protocol validation)"
  e2e_tests: "5+ tests (transformation validation)"
```

### Quality Metrics
```yaml
regression_tests: "398/398 passing (Epic-013 baseline)"
new_tests: "30+ tests passing (100%)"
code_review: "Pair programming (2 reviewers built-in)"
security_audit: "Dev 2A security validation"
```

### Business Metrics
```yaml
detection_coverage: "100% (60% → 100%)"
risk_eliminated: "P0 CRITICAL risk resolved"
service_availability: "100% (no detection-based downtime)"
```

---

## 🎯 Definition of Done

- [ ] **Code Complete**:
  - [ ] All 15+ models have ideType: "ANTIGRAVITY" field
  - [ ] ideType serialized in all three mappers
  - [ ] Request builders include ideType in payloads

- [ ] **Tests Passing**:
  - [ ] 398/398 regression tests passing (Epic-013 baseline)
  - [ ] 15+ new unit tests passing (100%)
  - [ ] 10+ new integration tests passing (100%)
  - [ ] 5+ new E2E tests passing (100%)

- [ ] **Quality Gates**:
  - [ ] Pair programming completed (Dev 2A + Dev 2B)
  - [ ] Security audit passed (Dev 2A validation)
  - [ ] Code review approved
  - [ ] No regressions detected

- [ ] **Documentation**:
  - [ ] Code comments explain ideType purpose
  - [ ] Test documentation complete
  - [ ] Security validation documented

- [ ] **Deployment Ready**:
  - [ ] Builds successfully (cargo build)
  - [ ] Lints clean (cargo clippy)
  - [ ] Formatted correctly (cargo fmt)
  - [ ] Ready for merge to main

---

## 📝 Implementation Notes

### Security Considerations
```yaml
marker_value: "ANTIGRAVITY"
case_sensitive: true
immutable: true
required: true

validation:
  - "Never allow empty ideType"
  - "Never allow null ideType"
  - "Never allow incorrect spelling/case"
  - "Fail fast if ideType missing"
```

### Performance Considerations
```yaml
overhead: "Negligible (<1ms per request)"
serialization: "Standard serde (no custom logic)"
memory: "~20 bytes per model instance"
```

### Code Review Checklist
- [ ] ideType spelled correctly in all locations
- [ ] ideType value is exactly "ANTIGRAVITY" (case-sensitive)
- [ ] Serialization uses correct field name ("ideType", not "ide_type")
- [ ] All models updated (no models skipped)
- [ ] Tests cover all updated models
- [ ] No hardcoded strings (use constants where appropriate)

---

**Story Status**: ✅ READY FOR DEVELOPMENT
**Assigned To**: Dev 2A + Dev 2B (Pair Programming)
**Start Date**: Week 1 Day 1 (after Epic-013 merge)
**Estimated Completion**: Week 1 End (4-5 days with pair programming)

