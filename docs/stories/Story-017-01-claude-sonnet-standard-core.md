# Story-017-01: Claude 4.5 Sonnet Standard - Core Implementation

**Epic**: Epic-017 - Claude 4.5 Sonnet Standard Mode
**Priority**: P1 (HIGH - Revenue Growth)
**Effort**: 3-4 days
**Team**: Team 2 (Multi-Protocol Specialists)
**Assignee**: Dev 2A (Senior Security Specialist)
**Created**: 2026-01-12
**Status**: 📋 READY FOR DEVELOPMENT

---

## 🎯 Objective

Implement core anti-detection and routing compliance for **claude-4.5-sonnet** (standard mode) by adding modelId 333, apiProvider 26, and ideType "ANTIGRAVITY" metadata to achieve 90%+ compliance.

---

## 📊 Business Context

### Current State
```yaml
problem:
  model: "claude-4.5-sonnet (standard, no thinking)"
  compliance: "~75-80%"
  missing_features: "3 critical gaps"
  risk: "Detection + routing failures"
  severity: "P1 HIGH"

gaps:
  gap_1: "Missing modelId: 333"
  gap_2: "Missing apiProvider: 26 (ANTHROPIC_VERTEX)"
  gap_3: "Missing ideType: 'ANTIGRAVITY'"

impact:
  affected_users: "ALL users requesting premium model"
  risk_type: "Service unavailability + detection"
  business_impact: "Revenue loss, competitive disadvantage"
```

### Target State
```yaml
goal:
  compliance: "90%+ (core features complete)"
  missing_features: "0 critical gaps"
  detection_risk: "ELIMINATED"

success_metrics:
  - "modelId: 333 implemented"
  - "apiProvider: 26 (ANTHROPIC_VERTEX) working"
  - "ideType: 'ANTIGRAVITY' present in all requests"
  - "Request/response transformation working"
  - "20+ tests passing (100%)"
```

---

## 🔍 Gap Analysis

### Critical Missing Features

#### Gap 1: Missing modelId: 333
```yaml
current:
  model_id: "NOT SET or incorrect"
  routing: "Fails or routes to wrong model"

target:
  model_id: 333
  purpose: "Identifies claude-4.5-sonnet STANDARD mode"
  distinction: "Different from thinking mode (modelId: 334)"
```

#### Gap 2: Missing apiProvider: 26
```yaml
current:
  api_provider: "NOT SET or hardcoded"
  routing: "Incorrect upstream API selection"

target:
  api_provider: 26
  constant: "ANTHROPIC_VERTEX (from Epic-024)"
  purpose: "Routes to Anthropic via Google Vertex AI"
```

#### Gap 3: Missing ideType: "ANTIGRAVITY"
```yaml
current:
  ide_type: "NOT SET"
  detection_risk: "HIGH (identified as non-Antigravity client)"

target:
  ide_type: "ANTIGRAVITY"
  purpose: "Anti-detection marker"
  pattern: "From Epic-024 Story 024-01"
```

---

## 📋 Acceptance Criteria

### AC1: Model ID Constant Defined (CRITICAL)
```gherkin
GIVEN claude-4.5-sonnet standard model definition
WHEN model constants are created
THEN modelId MUST be 333 (standard mode)
AND modelId MUST be distinct from thinking mode (334)
AND constant MUST be used consistently across codebase
```

**Validation**:
- [ ] `CLAUDE_SONNET_45_STANDARD_MODEL_ID = 333` constant defined
- [ ] Constant used in model struct
- [ ] Constant used in request builder
- [ ] Test coverage for model ID presence
- [ ] No hardcoded "333" magic numbers

---

### AC2: API Provider Constant Used (CRITICAL)
```gherkin
GIVEN apiProvider constants from Epic-024
WHEN claude-4.5-sonnet model is instantiated
THEN apiProvider MUST be 26 (ANTHROPIC_VERTEX)
AND constant MUST be imported from api_provider module
AND apiProvider MUST be included in request payload
```

**Validation**:
- [ ] `api_provider::ANTHROPIC_VERTEX` constant imported
- [ ] apiProvider: 26 set in model definition
- [ ] apiProvider included in request JSON
- [ ] Test coverage for apiProvider value
- [ ] Request serialization validated

---

### AC3: ideType Marker Present (CRITICAL)
```gherkin
GIVEN ideType patterns from Epic-024
WHEN claude-4.5-sonnet request is built
THEN ideType MUST be "ANTIGRAVITY"
AND ideType MUST be present in request metadata
AND ideType MUST survive protocol transformations
```

**Validation**:
- [ ] ideType: "ANTIGRAVITY" set in model definition
- [ ] ideType included in request JSON
- [ ] ideType field serialized correctly
- [ ] Test coverage for ideType presence
- [ ] OpenAI → Claude transformation preserves ideType

---

### AC4: Request/Response Transformation (HIGH)
```gherkin
GIVEN claude-4.5-sonnet standard model
WHEN OpenAI-format request is received
THEN request MUST be transformed to Claude format
AND response MUST be transformed back to OpenAI format
AND all metadata MUST be preserved
```

**Validation**:
- [ ] OpenAI → Claude request transformation working
- [ ] Claude → OpenAI response transformation working
- [ ] Metadata preserved (modelId, apiProvider, ideType)
- [ ] Streaming support working
- [ ] Error handling operational

---

### AC5: Test Coverage (HIGH)
```gherkin
GIVEN claude-4.5-sonnet standard implementation
WHEN tests are executed
THEN unit tests MUST validate all constants
AND integration tests MUST validate request/response flow
AND E2E tests MUST validate end-to-end functionality
```

**Test Requirements**:
- [ ] Unit tests: model constants (5 tests)
- [ ] Unit tests: metadata presence (5 tests)
- [ ] Integration tests: request transformation (4 tests)
- [ ] Integration tests: response transformation (3 tests)
- [ ] E2E tests: full flow validation (3 tests)
- [ ] All tests passing (100%)

---

## 🛠️ Implementation Details

### File Changes

#### 1. Model Constants Definition
**File**: `src-tauri/src/proxy/mappers/claude/models.rs`

**Add Constants** (near existing thinking mode constants):
```rust
use crate::models::api_provider;

// Claude 4.5 Sonnet Model IDs
pub const CLAUDE_SONNET_45_STANDARD_MODEL_ID: i32 = 333;   // Standard mode (NEW)
pub const CLAUDE_SONNET_45_THINKING_MODEL_ID: i32 = 334;   // Thinking mode (existing)

// Model Provider Constants (shared)
pub const ANTHROPIC_VERTEX_API_PROVIDER: i32 = api_provider::ANTHROPIC_VERTEX; // 26
pub const ANTHROPIC_MODEL_PROVIDER: i32 = 3;
```

**Add Model Struct**:
```rust
/// Claude 4.5 Sonnet (Standard Mode) - NO thinking
#[derive(Debug, Clone)]
pub struct ClaudeSonnetStandard {
    pub model_id: i32,
    pub api_provider: i32,
    pub model_provider: i32,
    pub ide_type: String,
    pub display_name: String,
}

impl ClaudeSonnetStandard {
    pub fn new() -> Self {
        Self {
            model_id: CLAUDE_SONNET_45_STANDARD_MODEL_ID,
            api_provider: ANTHROPIC_VERTEX_API_PROVIDER,
            model_provider: ANTHROPIC_MODEL_PROVIDER,
            ide_type: "ANTIGRAVITY".to_string(),
            display_name: "Claude 4.5 Sonnet (Standard)".to_string(),
        }
    }
}

impl Default for ClaudeSonnetStandard {
    fn default() -> Self {
        Self::new()
    }
}
```

---

#### 2. Request Builder Integration
**File**: `src-tauri/src/proxy/mappers/claude/request.rs`

**Import Model**:
```rust
use super::models::{
    ClaudeSonnetStandard,
    CLAUDE_SONNET_45_STANDARD_MODEL_ID,
};
```

**Add Model Detection**:
```rust
/// Detect if request is for claude-4.5-sonnet (standard mode)
fn is_sonnet_standard(model: &str) -> bool {
    model == "claude-4.5-sonnet" ||
    model == "claude-sonnet-4.5" ||
    model == "claude-3-5-sonnet-20241022" ||  // Alias
    model == "claude-3-5-sonnet-20240620"     // Alias
}
```

**Build Request with Metadata**:
```rust
pub fn build_claude_request(
    openai_request: &OpenAIRequest,
    model_name: &str,
) -> Result<ClaudeRequest, String> {
    // Detect model type
    let is_standard = is_sonnet_standard(model_name);

    // Get model metadata
    let (model_id, api_provider, model_provider, ide_type) = if is_standard {
        let model = ClaudeSonnetStandard::new();
        (
            model.model_id,
            model.api_provider,
            model.model_provider,
            model.ide_type,
        )
    } else {
        // Handle other models (thinking mode, opus, etc.)
        // ... existing code ...
    };

    // Build request with metadata
    let request = ClaudeRequest {
        model: model_name.to_string(),
        messages: transform_messages(&openai_request.messages)?,
        tools: transform_tools(&openai_request.tools)?,

        // Anti-detection metadata (NEW)
        model_id: Some(model_id),
        api_provider: Some(api_provider),
        model_provider: Some(model_provider),
        ide_type: Some(ide_type),

        // Standard fields
        max_tokens: openai_request.max_tokens,
        temperature: openai_request.temperature,
        top_p: openai_request.top_p,
        stream: openai_request.stream,

        // ... other fields ...
    };

    Ok(request)
}
```

**Request Struct Metadata Fields**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeRequest {
    pub model: String,
    pub messages: Vec<ClaudeMessage>,

    // Anti-detection metadata (ADD THESE)
    #[serde(rename = "modelId", skip_serializing_if = "Option::is_none")]
    pub model_id: Option<i32>,

    #[serde(rename = "apiProvider", skip_serializing_if = "Option::is_none")]
    pub api_provider: Option<i32>,

    #[serde(rename = "modelProvider", skip_serializing_if = "Option::is_none")]
    pub model_provider: Option<i32>,

    #[serde(rename = "ideType", skip_serializing_if = "Option::is_none")]
    pub ide_type: Option<String>,

    // ... existing fields ...
}
```

---

#### 3. Response Transformation
**File**: `src-tauri/src/proxy/mappers/claude/response.rs`

**No Changes Required**: Response transformation already handles standard mode (no thinking blocks to process).

**Validation**: Ensure existing response transformation works for standard mode:
```rust
pub fn transform_claude_response(
    claude_response: ClaudeResponse,
) -> Result<OpenAIResponse, String> {
    // Standard transformation (no thinking block processing)
    // This already works for standard mode

    Ok(OpenAIResponse {
        id: claude_response.id,
        object: "chat.completion".to_string(),
        created: claude_response.created,
        model: claude_response.model,
        choices: transform_choices(claude_response.content)?,
        usage: transform_usage(claude_response.usage),
        // ... other fields ...
    })
}
```

---

### Test Strategy

#### Unit Tests (10 tests)
**File**: `tests/claude/sonnet_standard_tests.rs` (NEW)

```rust
#[cfg(test)]
mod sonnet_standard_tests {
    use super::*;
    use crate::proxy::mappers::claude::models::{
        ClaudeSonnetStandard,
        CLAUDE_SONNET_45_STANDARD_MODEL_ID,
    };
    use crate::models::api_provider;

    #[test]
    fn test_model_id_constant() {
        assert_eq!(CLAUDE_SONNET_45_STANDARD_MODEL_ID, 333);
    }

    #[test]
    fn test_model_id_distinct_from_thinking() {
        use crate::proxy::mappers::claude::models::CLAUDE_SONNET_45_THINKING_MODEL_ID;
        assert_ne!(
            CLAUDE_SONNET_45_STANDARD_MODEL_ID,
            CLAUDE_SONNET_45_THINKING_MODEL_ID
        );
    }

    #[test]
    fn test_api_provider_is_anthropic_vertex() {
        let model = ClaudeSonnetStandard::new();
        assert_eq!(model.api_provider, 26);
        assert_eq!(model.api_provider, api_provider::ANTHROPIC_VERTEX);
    }

    #[test]
    fn test_ide_type_is_antigravity() {
        let model = ClaudeSonnetStandard::new();
        assert_eq!(model.ide_type, "ANTIGRAVITY");
    }

    #[test]
    fn test_model_detection_sonnet_standard() {
        assert!(is_sonnet_standard("claude-4.5-sonnet"));
        assert!(is_sonnet_standard("claude-sonnet-4.5"));
        assert!(is_sonnet_standard("claude-3-5-sonnet-20241022"));
        assert!(is_sonnet_standard("claude-3-5-sonnet-20240620"));
    }

    #[test]
    fn test_model_detection_not_thinking() {
        assert!(!is_sonnet_standard("claude-4.5-sonnet-thinking"));
        assert!(!is_sonnet_standard("claude-sonnet-4-5-20250929"));
    }

    #[test]
    fn test_model_provider_constant() {
        let model = ClaudeSonnetStandard::new();
        assert_eq!(model.model_provider, 3);
    }

    #[test]
    fn test_model_display_name() {
        let model = ClaudeSonnetStandard::new();
        assert_eq!(model.display_name, "Claude 4.5 Sonnet (Standard)");
    }

    #[test]
    fn test_model_default_trait() {
        let model = ClaudeSonnetStandard::default();
        assert_eq!(model.model_id, 333);
    }

    #[test]
    fn test_model_clone_trait() {
        let model = ClaudeSonnetStandard::new();
        let cloned = model.clone();
        assert_eq!(model.model_id, cloned.model_id);
        assert_eq!(model.api_provider, cloned.api_provider);
    }
}
```

---

#### Integration Tests (7 tests)
**File**: `tests/claude/request_integration_tests.rs`

```rust
#[tokio::test]
async fn test_request_includes_model_id() {
    let openai_req = create_test_openai_request("claude-4.5-sonnet");
    let claude_req = build_claude_request(&openai_req, "claude-4.5-sonnet").unwrap();

    assert!(claude_req.model_id.is_some());
    assert_eq!(claude_req.model_id.unwrap(), 333);
}

#[tokio::test]
async fn test_request_includes_api_provider() {
    let openai_req = create_test_openai_request("claude-4.5-sonnet");
    let claude_req = build_claude_request(&openai_req, "claude-4.5-sonnet").unwrap();

    assert!(claude_req.api_provider.is_some());
    assert_eq!(claude_req.api_provider.unwrap(), 26);
}

#[tokio::test]
async fn test_request_includes_ide_type() {
    let openai_req = create_test_openai_request("claude-4.5-sonnet");
    let claude_req = build_claude_request(&openai_req, "claude-4.5-sonnet").unwrap();

    assert!(claude_req.ide_type.is_some());
    assert_eq!(claude_req.ide_type.unwrap(), "ANTIGRAVITY");
}

#[tokio::test]
async fn test_request_serialization() {
    let openai_req = create_test_openai_request("claude-4.5-sonnet");
    let claude_req = build_claude_request(&openai_req, "claude-4.5-sonnet").unwrap();
    let json = serde_json::to_value(&claude_req).unwrap();

    assert!(json.get("modelId").is_some());
    assert_eq!(json["modelId"], 333);
    assert!(json.get("apiProvider").is_some());
    assert_eq!(json["apiProvider"], 26);
    assert!(json.get("ideType").is_some());
    assert_eq!(json["ideType"], "ANTIGRAVITY");
}

#[tokio::test]
async fn test_response_transformation() {
    let claude_resp = create_test_claude_response();
    let openai_resp = transform_claude_response(claude_resp).unwrap();

    assert_eq!(openai_resp.object, "chat.completion");
    assert!(!openai_resp.choices.is_empty());
}

#[tokio::test]
async fn test_standard_vs_thinking_distinction() {
    let standard_req = build_claude_request(
        &create_test_openai_request("claude-4.5-sonnet"),
        "claude-4.5-sonnet"
    ).unwrap();

    let thinking_req = build_claude_request(
        &create_test_openai_request("claude-4.5-sonnet-thinking"),
        "claude-4.5-sonnet-thinking"
    ).unwrap();

    assert_eq!(standard_req.model_id.unwrap(), 333);
    assert_eq!(thinking_req.model_id.unwrap(), 334);
}

#[tokio::test]
async fn test_alias_routing() {
    let aliases = vec![
        "claude-sonnet-4.5",
        "claude-3-5-sonnet-20241022",
        "claude-3-5-sonnet-20240620",
    ];

    for alias in aliases {
        let req = build_claude_request(
            &create_test_openai_request(alias),
            alias
        ).unwrap();
        assert_eq!(req.model_id.unwrap(), 333);
    }
}
```

---

#### E2E Tests (3 tests)
**File**: `tests/claude/e2e_tests.rs`

```rust
#[tokio::test]
async fn test_e2e_standard_request_flow() {
    // OpenAI format → Claude transformation → API call (mocked) → OpenAI response
    let openai_req = create_test_openai_request("claude-4.5-sonnet");
    let claude_req = build_claude_request(&openai_req, "claude-4.5-sonnet").unwrap();

    // Mock API call
    let claude_resp = mock_claude_api_call(&claude_req).await.unwrap();

    // Transform response back
    let openai_resp = transform_claude_response(claude_resp).unwrap();

    // Validate end-to-end
    assert_eq!(openai_resp.object, "chat.completion");
    assert!(!openai_resp.choices.is_empty());
    assert_eq!(openai_resp.model, "claude-4.5-sonnet");
}

#[tokio::test]
async fn test_e2e_metadata_preservation() {
    let openai_req = create_test_openai_request("claude-4.5-sonnet");
    let claude_req = build_claude_request(&openai_req, "claude-4.5-sonnet").unwrap();

    // Serialize to JSON (simulates network transmission)
    let json = serde_json::to_string(&claude_req).unwrap();
    let deserialized: ClaudeRequest = serde_json::from_str(&json).unwrap();

    // Validate metadata preserved
    assert_eq!(deserialized.model_id.unwrap(), 333);
    assert_eq!(deserialized.api_provider.unwrap(), 26);
    assert_eq!(deserialized.ide_type.unwrap(), "ANTIGRAVITY");
}

#[tokio::test]
async fn test_e2e_streaming_support() {
    let mut openai_req = create_test_openai_request("claude-4.5-sonnet");
    openai_req.stream = Some(true);

    let claude_req = build_claude_request(&openai_req, "claude-4.5-sonnet").unwrap();
    assert_eq!(claude_req.stream, Some(true));

    // Mock streaming API call
    let stream_chunks = mock_claude_streaming_call(&claude_req).await.unwrap();

    // Validate streaming response
    assert!(!stream_chunks.is_empty());
    for chunk in stream_chunks {
        assert!(chunk.choices.is_some());
    }
}
```

---

## 🔗 Dependencies

### Prerequisite
- ✅ Epic-024 COMPLETE (2026-01-12) - provides ideType, apiProvider patterns
- ✅ `api_provider.rs` module exists (from Story 024-02)

### Blocks
- 🔴 Story 017-02 (Tool Modes & Grounding) - depends on core implementation

### Enables
- ✅ Story 017-02 (tool modes can build on this foundation)
- ✅ Story 017-03 (testing can validate core functionality)
- ✅ 90%+ compliance achieved (core features complete)

---

## 📊 Success Metrics

### Code Metrics
```yaml
files_created: "1 (sonnet_standard_tests.rs)"
files_modified: "2 (models.rs, request.rs)"
constants_added: "1 (CLAUDE_SONNET_45_STANDARD_MODEL_ID = 333)"
lines_added: "~150-200 lines"
test_coverage:
  unit_tests: "10 tests"
  integration_tests: "7 tests"
  e2e_tests: "3 tests"
  total: "20 tests"
```

### Quality Metrics
```yaml
regression_tests: "398/398 passing (Epic-013 baseline)"
new_tests: "20 tests passing (100%)"
code_review: "Senior Security (Dev 2A) + Team Lead"
linting: "cargo clippy clean"
formatting: "cargo fmt check passing"
```

### Business Metrics
```yaml
compliance_before: "75-80%"
compliance_after: "90%+ (core features complete)"
gaps_resolved: "3/5 (modelId, apiProvider, ideType)"
detection_risk: "ELIMINATED (ideType present)"
revenue_impact: "HIGH (premium model access enabled)"
```

---

## 🎯 Definition of Done

- [ ] **Code Complete**:
  - [ ] CLAUDE_SONNET_45_STANDARD_MODEL_ID = 333 constant defined
  - [ ] ClaudeSonnetStandard model struct implemented
  - [ ] apiProvider: 26 (ANTHROPIC_VERTEX) integrated
  - [ ] ideType: "ANTIGRAVITY" present in all requests
  - [ ] Request builder includes all metadata
  - [ ] Model detection function working (standard vs thinking)

- [ ] **Tests Passing**:
  - [ ] 398/398 regression tests passing
  - [ ] 10 unit tests passing (100%)
  - [ ] 7 integration tests passing (100%)
  - [ ] 3 E2E tests passing (100%)

- [ ] **Quality Gates**:
  - [ ] Code review approved (Dev 2A + Team Lead)
  - [ ] Security validation passed
  - [ ] Linting clean (cargo clippy)
  - [ ] Formatted correctly (cargo fmt)
  - [ ] No regressions detected

- [ ] **Documentation**:
  - [ ] Code comments explain model ID distinction
  - [ ] Test documentation complete
  - [ ] Implementation notes documented

- [ ] **Deployment Ready**:
  - [ ] Builds successfully (cargo build)
  - [ ] Integration tested with existing code
  - [ ] Ready for Story 017-02 (tool modes)

---

## 📝 Implementation Notes

### Code Reuse from Thinking Mode
```yaml
reusable_patterns: "90%"
  - "Request transformation structure" ✅
  - "Response transformation structure" ✅
  - "Metadata field definitions" ✅
  - "Serialization patterns" ✅

differences: "Model ID constant only"
  - "333 (standard) vs 334 (thinking)"
  - "No thinking block processing"
  - "No budget constraints"
  - "No signature validation"
```

### Performance Considerations
```yaml
overhead: "Negligible (<1ms per request)"
metadata_size: "~100 bytes (modelId, apiProvider, ideType)"
serialization: "Standard serde (no custom logic)"
memory: "~50 bytes per model instance"
```

### Security Considerations
```yaml
anti_detection: "100% (ideType: ANTIGRAVITY)"
provider_routing: "Correct (apiProvider: 26)"
model_identification: "Accurate (modelId: 333)"
validation: "Model ID validated against constants"
```

---

**Story Status**: 📋 READY FOR DEVELOPMENT
**Assigned To**: Dev 2A (Senior Security Specialist)
**Start Date**: Week 4 Day 1
**Estimated Completion**: Week 4 Day 3 (3 days)
