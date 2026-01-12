# Story-024-02: apiProvider Field Completion (Anti-Detection)

**Epic**: Epic-024 - Anti-Detection Hardening
**Priority**: P0 (CRITICAL)
**Effort**: 6-8 hours (3 days)
**Team**: Team 2 (Multi-Protocol Specialists)
**Assignee**: Dev 2B (Mid-Level Protocol Specialist)
**Created**: 2026-01-12
**Status**: ✅ COMPLETE - QA PASSED (10/10) - BLOCKER RESOLVED
**Dependencies**: Story 024-01 (ideType markers must be complete)

---

## 🎯 Objective

Implement complete `apiProvider` field logic across all models to correctly identify upstream API providers (ANTHROPIC_VERTEX, GOOGLE_VERTEX, OPENAI, etc.), ensuring accurate provider routing and detection protection.

---

## 📊 Business Context

### Current State
```yaml
problem:
  apiProvider_coverage: "Partial implementation"
  missing_logic: "Dynamic apiProvider selection missing for some models"
  risk: "Incorrect provider routing = API errors or detection"
  severity: "P0 CRITICAL"

gaps:
  - "Some models have hardcoded apiProvider IDs"
  - "No dynamic selection based on model characteristics"
  - "Claude standard modes missing apiProvider: 26 (ANTHROPIC_VERTEX)"
  - "Gemini models inconsistent apiProvider values"
```

### Target State
```yaml
goal:
  apiProvider_coverage: "100% (all models have correct apiProvider)"
  dynamic_selection: "Provider selected based on model/region/features"
  correctness: "All provider IDs match actual upstream APIs"

success_metrics:
  - "All models have apiProvider field"
  - "apiProvider values validated against constants"
  - "Dynamic selection logic working"
  - "No API routing errors"
```

---

## 🔍 apiProvider ID Reference

### Provider ID Constants
```rust
// src-tauri/src/models/api_provider.rs (NEW or UPDATE)

pub mod ApiProvider {
    pub const ANTHROPIC_VERTEX: u32 = 26;
    pub const GOOGLE_VERTEX: u32 = 32;
    pub const OPENAI: u32 = 1;
    pub const OPENAI_AZURE: u32 = 2;
    // ... other providers
}
```

### Model-to-Provider Mapping
```yaml
claude_models:
  apiProvider: 26  # ANTHROPIC_VERTEX
  models:
    - "claude-4.5-sonnet (thinking + standard)"
    - "claude-opus-4-5 (thinking + standard)"
    - "claude-3-5-sonnet-v2"
    - "claude-3-opus"
    - "claude-3-haiku"

gemini_models:
  apiProvider: 32  # GOOGLE_VERTEX
  models:
    - "gemini-3-flash"
    - "gemini-3-pro-high"
    - "gemini-3-pro-low"
    - "gemini-2.5-pro-thinking"
    - "gemini-2.5-flash"
    - "gemini-2.0-flash-exp"

openai_models:
  apiProvider: 1   # OPENAI (default)
  apiProvider_azure: 2  # OPENAI_AZURE (if configured)
  models:
    - "gpt-4o"
    - "gpt-4o-mini"
    - "gpt-4-turbo"
    - "o1-preview"
```

---

## 📋 Acceptance Criteria

### AC1: apiProvider Constants Defined (CRITICAL)
```gherkin
GIVEN apiProvider IDs need standardization
WHEN constants module is created
THEN all provider IDs MUST be defined as constants
AND constants MUST match actual upstream provider IDs
AND no magic numbers in code
```

**Validation**:
- [ ] `src-tauri/src/models/api_provider.rs` created (NEW file)
- [ ] ANTHROPIC_VERTEX = 26 constant defined
- [ ] GOOGLE_VERTEX = 32 constant defined
- [ ] OPENAI = 1 constant defined
- [ ] OPENAI_AZURE = 2 constant defined (if applicable)
- [ ] All constants documented with provider names

---

### AC2: Claude Models apiProvider (CRITICAL)
```gherkin
GIVEN Claude models in mappers/claude/models.rs
WHEN apiProvider logic is implemented
THEN all Claude models MUST have apiProvider: 26 (ANTHROPIC_VERTEX)
AND apiProvider MUST be included in request payloads
AND apiProvider MUST be validated against constants
```

**Validation**:
- [ ] `src-tauri/src/proxy/mappers/claude/models.rs` updated
- [ ] All Claude models have `api_provider: ApiProvider::ANTHROPIC_VERTEX`
- [ ] Request builder includes apiProvider field
- [ ] Test coverage for apiProvider presence (5+ tests)

**Models to Update**:
- `claude-4.5-sonnet` (standard) - ADD apiProvider: 26
- `claude-opus-4-5` (standard) - ADD apiProvider: 26
- `claude-3-5-sonnet-v2` - VERIFY apiProvider: 26
- `claude-3-opus` - VERIFY apiProvider: 26
- `claude-3-haiku` - VERIFY apiProvider: 26

---

### AC3: Gemini Models apiProvider (CRITICAL)
```gherkin
GIVEN Gemini models in mappers/gemini/models.rs
WHEN apiProvider logic is implemented
THEN all Gemini models MUST have apiProvider: 32 (GOOGLE_VERTEX)
AND apiProvider MUST be included in request payloads
AND apiProvider MUST be consistent across all Gemini variants
```

**Validation**:
- [ ] `src-tauri/src/proxy/mappers/gemini/models.rs` updated
- [ ] All Gemini models have `api_provider: ApiProvider::GOOGLE_VERTEX`
- [ ] Request builder includes apiProvider field
- [ ] Test coverage for apiProvider presence (5+ tests)

**Models to Verify/Update**:
- `gemini-3-flash` - VERIFY apiProvider: 32
- `gemini-3-pro-high` - VERIFY apiProvider: 32
- `gemini-3-pro-low` - VERIFY apiProvider: 32
- `gemini-2.5-pro-thinking` - ADD apiProvider: 32
- `gemini-2.5-flash` - ADD apiProvider: 32
- `gemini-2.0-flash-exp` - ADD apiProvider: 32

---

### AC4: OpenAI Models apiProvider (CRITICAL)
```gherkin
GIVEN OpenAI models in mappers/openai/models.rs
WHEN apiProvider logic is implemented
THEN all OpenAI models MUST have apiProvider: 1 (OPENAI)
AND apiProvider MUST support OPENAI_AZURE (2) when configured
AND apiProvider selection MUST be dynamic based on configuration
```

**Validation**:
- [ ] `src-tauri/src/proxy/mappers/openai/models.rs` updated
- [ ] All OpenAI models have `api_provider: ApiProvider::OPENAI` (default)
- [ ] Azure support: `api_provider: ApiProvider::OPENAI_AZURE` (if configured)
- [ ] Dynamic selection logic implemented
- [ ] Test coverage for both OPENAI and OPENAI_AZURE (5+ tests)

**Models to Update**:
- `gpt-4o` - ADD apiProvider: 1 (or 2 if Azure)
- `gpt-4o-mini` - ADD apiProvider: 1 (or 2 if Azure)
- `gpt-4-turbo` - ADD apiProvider: 1 (or 2 if Azure)
- `o1-preview` - ADD apiProvider: 1 (or 2 if Azure)

---

### AC5: Request Serialization (CRITICAL)
```gherkin
GIVEN apiProvider fields in model definitions
WHEN requests are built and serialized
THEN apiProvider MUST be included in request payload JSON
AND field name MUST be "apiProvider" (camelCase)
AND value MUST be integer (not string)
```

**Validation**:
- [ ] Claude request builder includes apiProvider
- [ ] Gemini request builder includes apiProvider
- [ ] OpenAI request builder includes apiProvider
- [ ] Serialization test coverage (10+ tests)
- [ ] JSON payload validation tests

---

### AC6: Test Coverage (HIGH)
```gherkin
GIVEN apiProvider implementation complete
WHEN tests are executed
THEN tests MUST validate apiProvider presence in all models
AND tests MUST validate apiProvider values correctness
AND tests MUST validate apiProvider in request payloads
```

**Test Requirements**:
- [ ] Unit tests: apiProvider constants (5 tests)
- [ ] Unit tests: apiProvider in models (15+ tests, 1 per model)
- [ ] Integration tests: apiProvider in requests (10+ tests)
- [ ] E2E tests: apiProvider routing validation (5+ tests)
- [ ] All tests passing (100%)

---

## 🛠️ Implementation Details

### Step 1: Define apiProvider Constants

**File**: `src-tauri/src/models/api_provider.rs` (NEW)

```rust
//! API Provider constants for upstream API routing
//!
//! These constants map to specific upstream API providers and are used
//! for correct API routing and detection protection.

/// Anthropic API via Google Vertex AI
pub const ANTHROPIC_VERTEX: u32 = 26;

/// Google Gemini API via Google Vertex AI
pub const GOOGLE_VERTEX: u32 = 32;

/// OpenAI API (direct)
pub const OPENAI: u32 = 1;

/// OpenAI API via Azure
pub const OPENAI_AZURE: u32 = 2;

/// Helper function to get provider name for logging
pub fn provider_name(provider_id: u32) -> &'static str {
    match provider_id {
        ANTHROPIC_VERTEX => "ANTHROPIC_VERTEX",
        GOOGLE_VERTEX => "GOOGLE_VERTEX",
        OPENAI => "OPENAI",
        OPENAI_AZURE => "OPENAI_AZURE",
        _ => "UNKNOWN",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_constants() {
        assert_eq!(ANTHROPIC_VERTEX, 26);
        assert_eq!(GOOGLE_VERTEX, 32);
        assert_eq!(OPENAI, 1);
        assert_eq!(OPENAI_AZURE, 2);
    }

    #[test]
    fn test_provider_name() {
        assert_eq!(provider_name(26), "ANTHROPIC_VERTEX");
        assert_eq!(provider_name(32), "GOOGLE_VERTEX");
        assert_eq!(provider_name(1), "OPENAI");
    }
}
```

---

### Step 2: Update Claude Models

**File**: `src-tauri/src/proxy/mappers/claude/models.rs`

```rust
use crate::models::api_provider;

pub struct ClaudeModel {
    pub id: String,
    pub display_name: String,
    pub model_id: u32,
    pub api_provider: u32,  // ALREADY EXISTS (from Story 024-01)
    pub ide_type: String,   // ALREADY EXISTS (from Story 024-01)
}

impl ClaudeModel {
    /// Claude 4.5 Sonnet (Standard Mode)
    pub fn claude_4_5_sonnet_standard() -> Self {
        Self {
            id: "claude-4.5-sonnet".to_string(),
            display_name: "Claude 4.5 Sonnet (Standard)".to_string(),
            model_id: 333,
            api_provider: api_provider::ANTHROPIC_VERTEX,  // USE CONSTANT
            ide_type: "ANTIGRAVITY".to_string(),
        }
    }

    /// Claude Opus 4.5 (Standard Mode)
    pub fn claude_opus_4_5_standard() -> Self {
        Self {
            id: "claude-opus-4-5".to_string(),
            display_name: "Claude Opus 4.5 (Standard)".to_string(),
            model_id: 334,  // TBD: Verify actual model ID
            api_provider: api_provider::ANTHROPIC_VERTEX,  // USE CONSTANT
            ide_type: "ANTIGRAVITY".to_string(),
        }
    }

    // ... update all other Claude models similarly
}
```

---

### Step 3: Update Gemini Models

**File**: `src-tauri/src/proxy/mappers/gemini/models.rs`

```rust
use crate::models::api_provider;

pub struct GeminiModel {
    pub id: String,
    pub display_name: String,
    pub api_provider: u32,  // ADD THIS FIELD
    pub ide_type: String,   // ALREADY EXISTS (from Story 024-01)
    // ... other fields
}

impl GeminiModel {
    pub fn gemini_3_flash() -> Self {
        Self {
            id: "gemini-3-flash".to_string(),
            display_name: "Gemini 3 Flash".to_string(),
            api_provider: api_provider::GOOGLE_VERTEX,  // USE CONSTANT
            ide_type: "ANTIGRAVITY".to_string(),
            // ... other fields
        }
    }

    pub fn gemini_2_5_pro_thinking() -> Self {
        Self {
            id: "gemini-2.5-pro-thinking".to_string(),
            display_name: "Gemini 2.5 Pro (Thinking)".to_string(),
            api_provider: api_provider::GOOGLE_VERTEX,  // USE CONSTANT
            ide_type: "ANTIGRAVITY".to_string(),
            // ... other fields
        }
    }

    // ... update all other Gemini models similarly
}
```

---

### Step 4: Update OpenAI Models

**File**: `src-tauri/src/proxy/mappers/openai/models.rs`

```rust
use crate::models::api_provider;

pub struct OpenAIModel {
    pub id: String,
    pub display_name: String,
    pub api_provider: u32,  // ADD THIS FIELD
    pub ide_type: String,   // ALREADY EXISTS (from Story 024-01)
    // ... other fields
}

impl OpenAIModel {
    /// Get apiProvider based on configuration
    fn get_api_provider() -> u32 {
        // Check if Azure is configured
        if is_azure_configured() {
            api_provider::OPENAI_AZURE
        } else {
            api_provider::OPENAI
        }
    }

    pub fn gpt_4o() -> Self {
        Self {
            id: "gpt-4o".to_string(),
            display_name: "GPT-4o".to_string(),
            api_provider: Self::get_api_provider(),  // DYNAMIC
            ide_type: "ANTIGRAVITY".to_string(),
            // ... other fields
        }
    }

    // ... update all other OpenAI models similarly
}

// Helper function to check Azure configuration
fn is_azure_configured() -> bool {
    // Check environment variables or config file
    std::env::var("AZURE_OPENAI_ENDPOINT").is_ok()
}
```

---

### Step 5: Update Request Builders

#### Claude Request Builder
**File**: `src-tauri/src/proxy/mappers/claude/request.rs`

```rust
use crate::models::api_provider;

#[derive(Serialize)]
pub struct ClaudeRequest {
    pub model: String,
    // ... other fields
    #[serde(rename = "apiProvider")]
    pub api_provider: u32,  // ADD THIS FIELD
    #[serde(rename = "ideType")]
    pub ide_type: String,   // ALREADY EXISTS (from Story 024-01)
}

pub fn build_claude_request(model: &ClaudeModel, ...) -> ClaudeRequest {
    ClaudeRequest {
        model: model.id.clone(),
        // ... other fields
        api_provider: model.api_provider,  // INCLUDE apiProvider
        ide_type: model.ide_type.clone(),
    }
}
```

#### Gemini Request Builder
**File**: `src-tauri/src/proxy/mappers/gemini/request.rs`

```rust
#[derive(Serialize)]
pub struct GeminiRequest {
    pub model: String,
    // ... other fields
    #[serde(rename = "apiProvider")]
    pub api_provider: u32,  // ADD THIS FIELD
    #[serde(rename = "ideType")]
    pub ide_type: String,   // ALREADY EXISTS (from Story 024-01)
}

pub fn build_gemini_request(model: &GeminiModel, ...) -> GeminiRequest {
    GeminiRequest {
        model: model.id.clone(),
        // ... other fields
        api_provider: model.api_provider,  // INCLUDE apiProvider
        ide_type: model.ide_type.clone(),
    }
}
```

#### OpenAI Request Builder
**File**: `src-tauri/src/proxy/mappers/openai/request.rs`

```rust
#[derive(Serialize)]
pub struct OpenAIRequest {
    pub model: String,
    // ... other fields
    #[serde(rename = "apiProvider")]
    pub api_provider: u32,  // ADD THIS FIELD
    #[serde(rename = "ideType")]
    pub ide_type: String,   // ALREADY EXISTS (from Story 024-01)
}

pub fn build_openai_request(model: &OpenAIModel, ...) -> OpenAIRequest {
    OpenAIRequest {
        model: model.id.clone(),
        // ... other fields
        api_provider: model.api_provider,  // INCLUDE apiProvider
        ide_type: model.ide_type.clone(),
    }
}
```

---

### Test Strategy

#### Unit Tests (20+ tests)
**File**: `tests/security/apiProvider_tests.rs` (NEW)

```rust
#[cfg(test)]
mod apiProvider_tests {
    use super::*;
    use crate::models::api_provider;

    // Constants tests
    #[test]
    fn test_anthropic_vertex_constant() {
        assert_eq!(api_provider::ANTHROPIC_VERTEX, 26);
    }

    #[test]
    fn test_google_vertex_constant() {
        assert_eq!(api_provider::GOOGLE_VERTEX, 32);
    }

    // Claude models tests
    #[test]
    fn test_claude_sonnet_standard_apiProvider() {
        let model = ClaudeModel::claude_4_5_sonnet_standard();
        assert_eq!(model.api_provider, 26);
        assert_eq!(model.api_provider, api_provider::ANTHROPIC_VERTEX);
    }

    #[test]
    fn test_claude_opus_standard_apiProvider() {
        let model = ClaudeModel::claude_opus_4_5_standard();
        assert_eq!(model.api_provider, 26);
    }

    // Gemini models tests
    #[test]
    fn test_gemini_3_flash_apiProvider() {
        let model = GeminiModel::gemini_3_flash();
        assert_eq!(model.api_provider, 32);
        assert_eq!(model.api_provider, api_provider::GOOGLE_VERTEX);
    }

    #[test]
    fn test_gemini_2_5_pro_thinking_apiProvider() {
        let model = GeminiModel::gemini_2_5_pro_thinking();
        assert_eq!(model.api_provider, 32);
    }

    // OpenAI models tests
    #[test]
    fn test_gpt_4o_apiProvider_direct() {
        // Without Azure configuration
        std::env::remove_var("AZURE_OPENAI_ENDPOINT");
        let model = OpenAIModel::gpt_4o();
        assert_eq!(model.api_provider, 1);
        assert_eq!(model.api_provider, api_provider::OPENAI);
    }

    #[test]
    fn test_gpt_4o_apiProvider_azure() {
        // With Azure configuration
        std::env::set_var("AZURE_OPENAI_ENDPOINT", "https://test.openai.azure.com");
        let model = OpenAIModel::gpt_4o();
        assert_eq!(model.api_provider, 2);
        assert_eq!(model.api_provider, api_provider::OPENAI_AZURE);
    }

    // ... 10+ more tests for all models
}
```

#### Integration Tests (10+ tests)
**File**: `tests/security/apiProvider_integration_tests.rs` (NEW)

```rust
#[tokio::test]
async fn test_claude_request_includes_apiProvider() {
    let request = build_test_claude_request();
    let json = serde_json::to_value(&request).unwrap();

    assert!(json.get("apiProvider").is_some());
    assert_eq!(json["apiProvider"], 26);
}

#[tokio::test]
async fn test_gemini_request_includes_apiProvider() {
    let request = build_test_gemini_request();
    let json = serde_json::to_value(&request).unwrap();

    assert!(json.get("apiProvider").is_some());
    assert_eq!(json["apiProvider"], 32);
}

#[tokio::test]
async fn test_openai_request_includes_apiProvider() {
    let request = build_test_openai_request();
    let json = serde_json::to_value(&request).unwrap();

    assert!(json.get("apiProvider").is_some());
    assert!(json["apiProvider"] == 1 || json["apiProvider"] == 2); // OPENAI or AZURE
}

// ... 7+ more integration tests
```

---

## 🔗 Dependencies

### Prerequisite
- ✅ Story 024-01 COMPLETE (ideType markers implemented)

### Blocks
- 🟡 Story 024-03 (User-Agent Rotation) - can run in parallel
- 🟡 Story 024-04 (Detection Monitoring) - can run in parallel
- 🟡 Epic-017/019 (Claude) - will use apiProvider patterns

### Enables
- ✅ Correct upstream API routing
- ✅ Detection protection via provider identification
- ✅ Azure OpenAI support (dynamic provider selection)

---

## 📊 Success Metrics

### Code Metrics
```yaml
files_created: "1 (api_provider.rs constants)"
files_modified: "6 (3 models.rs + 3 request.rs)"
constants_defined: "4+ provider constants"
models_updated: "15+ models"
test_coverage:
  unit_tests: "20+ tests"
  integration_tests: "10+ tests"
  e2e_tests: "5+ tests"
```

### Quality Metrics
```yaml
regression_tests: "398/398 passing (Epic-013 baseline)"
new_tests: "35+ tests passing (100%)"
code_review: "Dev 2A validation + Team Lead approval"
linting: "cargo clippy clean"
```

### Business Metrics
```yaml
apiProvider_coverage: "100% (all models)"
routing_correctness: "100% (no routing errors)"
Azure_support: "✅ (dynamic provider selection)"
```

---

## 🎯 Definition of Done

- [ ] **Code Complete**:
  - [ ] api_provider.rs constants module created
  - [ ] All 15+ models have apiProvider field
  - [ ] apiProvider serialized in all three mappers
  - [ ] Dynamic Azure selection implemented (OpenAI)

- [ ] **Tests Passing**:
  - [ ] 398/398 regression tests passing
  - [ ] 20+ new unit tests passing (100%)
  - [ ] 10+ new integration tests passing (100%)
  - [ ] 5+ new E2E tests passing (100%)

- [ ] **Quality Gates**:
  - [ ] Code review approved (Dev 2A + Team Lead)
  - [ ] Linting clean (cargo clippy)
  - [ ] Formatted correctly (cargo fmt)
  - [ ] No regressions detected

- [ ] **Documentation**:
  - [ ] api_provider constants documented
  - [ ] Code comments explain provider IDs
  - [ ] Test documentation complete

- [ ] **Deployment Ready**:
  - [ ] Builds successfully
  - [ ] Ready for merge to main

---

**Story Status**: ✅ READY FOR DEVELOPMENT
**Assigned To**: Dev 2B (Mid-Level Protocol Specialist)
**Start Date**: Week 2 (after Story 024-01 complete)
**Estimated Completion**: Week 2 End (3 days)

