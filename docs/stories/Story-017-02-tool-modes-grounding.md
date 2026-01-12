# Story-017-02: Claude 4.5 Sonnet Standard - Tool Modes & Grounding

**Epic**: Epic-017 - Claude 4.5 Sonnet Standard Mode
**Priority**: P1 (HIGH - Feature Parity)
**Effort**: 2-3 days
**Team**: Team 2 (Multi-Protocol Specialists)
**Assignee**: Dev 2B (Mid-Level Protocol Specialist)
**Created**: 2026-01-12
**Status**: 📋 READY FOR DEVELOPMENT
**Dependencies**: Story 017-01 (core implementation complete)

---

## 🎯 Objective

Implement flexible tool configuration modes (AUTO/ANY/NONE) and grounding configuration for **claude-4.5-sonnet** (standard mode) to achieve 95%+ compliance with complete feature parity.

---

## 📊 Business Context

### Current State
```yaml
problem:
  compliance: "90% (after Story 017-01)"
  missing_features: "2 feature gaps"
  risk: "Limited tool flexibility, no grounding support"
  severity: "P1 HIGH"

gaps:
  gap_4: "Flexible tool modes (AUTO/ANY/NONE) missing"
  gap_5: "Grounding config missing"

impact:
  affected_users: "Users requiring advanced tool control"
  use_cases: "Search integration, function calling control"
  business_impact: "Feature parity gap with competitors"
```

### Target State
```yaml
goal:
  compliance: "95%+ (feature parity complete)"
  missing_features: "0 feature gaps"
  tool_flexibility: "COMPLETE (AUTO/ANY/NONE supported)"
  grounding_support: "OPERATIONAL"

success_metrics:
  - "Tool mode AUTO (default) working"
  - "Tool mode ANY (force enable) working"
  - "Tool mode NONE (disable) working"
  - "Grounding config operational"
  - "15+ tests passing (100%)"
```

---

## 🔍 Gap Analysis

### Critical Missing Features

#### Gap 4: Flexible Tool Modes
```yaml
current:
  tool_modes: "NOT SUPPORTED or basic only"
  flexibility: "Limited control over tool usage"

target:
  tool_modes: "AUTO, ANY, NONE"
  flexibility: "Full control over tool behavior"

modes:
  AUTO:
    description: "Model decides when to use tools"
    use_case: "Default behavior, intelligent tool selection"
    default: true

  ANY:
    description: "Force tool usage (model MUST use a tool)"
    use_case: "Enforce function calling"
    validation: "Error if no tools provided"

  NONE:
    description: "Disable tools completely"
    use_case: "Text-only responses"
    behavior: "Ignore tool definitions"
```

#### Gap 5: Grounding Configuration
```yaml
current:
  grounding: "NOT SUPPORTED"
  search: "No Google Search integration"

target:
  grounding: "Google Search integration"
  search_sources: "Web search, code search"

config:
  google_search_retrieval:
    enabled: true
    dynamic_retrieval_config:
      mode: "MODE_DYNAMIC"
      dynamic_threshold: 0.3

  recitation_policy:
    enabled: true
    policy: "CITED_DOCUMENTS_ONLY"
```

---

## 📋 Acceptance Criteria

### AC1: Tool Mode AUTO (Default) (CRITICAL)
```gherkin
GIVEN a claude-4.5-sonnet request with tools
WHEN tool_choice is not specified or set to "auto"
THEN model MUST decide when to use tools
AND tools MUST be available to model
AND model CAN choose to respond without tools
```

**Validation**:
- [ ] Default tool mode is AUTO
- [ ] Tools included in request when provided
- [ ] Model can choose to use or not use tools
- [ ] Test coverage for AUTO mode (3 tests)

---

### AC2: Tool Mode ANY (Force Enable) (CRITICAL)
```gherkin
GIVEN a claude-4.5-sonnet request with tools
WHEN tool_choice is set to "any" or {type: "any"}
THEN model MUST use a tool in response
AND request MUST fail if no tools provided
AND model CANNOT respond without tool usage
```

**Validation**:
- [ ] ANY mode forces tool usage
- [ ] Error if no tools provided with ANY mode
- [ ] Model MUST use a tool (validated)
- [ ] Test coverage for ANY mode (3 tests)

---

### AC3: Tool Mode NONE (Disable) (CRITICAL)
```gherkin
GIVEN a claude-4.5-sonnet request
WHEN tool_choice is set to "none"
THEN tools MUST be completely disabled
AND model MUST respond with text only
AND tool definitions MUST be ignored
```

**Validation**:
- [ ] NONE mode disables all tools
- [ ] Model responds with text only
- [ ] Tool definitions ignored
- [ ] Test coverage for NONE mode (3 tests)

---

### AC4: Grounding Configuration (HIGH)
```gherkin
GIVEN a claude-4.5-sonnet request
WHEN grounding is enabled
THEN Google Search integration MUST be active
AND recitation policy MUST be enforced
AND search results CAN be incorporated into response
```

**Validation**:
- [ ] Google Search retrieval config working
- [ ] Dynamic retrieval mode operational
- [ ] Recitation policy enforced
- [ ] Test coverage for grounding (3 tests)

---

### AC5: Test Coverage (HIGH)
```gherkin
GIVEN tool modes and grounding implementation
WHEN tests are executed
THEN all tool mode variations MUST be tested
AND grounding config MUST be validated
AND integration tests MUST cover all scenarios
```

**Test Requirements**:
- [ ] Unit tests: tool mode detection (5 tests)
- [ ] Unit tests: grounding config (3 tests)
- [ ] Integration tests: tool mode behavior (5 tests)
- [ ] Integration tests: grounding integration (2 tests)
- [ ] All tests passing (100%)

---

## 🛠️ Implementation Details

### File Changes

#### 1. Tool Mode Configuration
**File**: `src-tauri/src/proxy/mappers/claude/request.rs`

**Add Tool Mode Enum**:
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ToolMode {
    Auto,   // Default: model decides
    Any,    // Force: must use a tool
    None,   // Disable: no tools allowed
}

impl Default for ToolMode {
    fn default() -> Self {
        ToolMode::Auto
    }
}

impl ToolMode {
    /// Parse from OpenAI tool_choice field
    pub fn from_openai_tool_choice(tool_choice: &Option<Value>) -> Self {
        match tool_choice {
            None => ToolMode::Auto,  // Default
            Some(Value::String(s)) => match s.as_str() {
                "auto" => ToolMode::Auto,
                "any" | "required" => ToolMode::Any,
                "none" => ToolMode::None,
                _ => ToolMode::Auto,  // Fallback
            },
            Some(Value::Object(obj)) => {
                if let Some(Value::String(type_val)) = obj.get("type") {
                    match type_val.as_str() {
                        "auto" => ToolMode::Auto,
                        "any" | "required" => ToolMode::Any,
                        "none" => ToolMode::None,
                        _ => ToolMode::Auto,
                    }
                } else {
                    ToolMode::Auto
                }
            },
            _ => ToolMode::Auto,
        }
    }
}
```

**Add Tool Configuration Struct**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfiguration {
    #[serde(rename = "functionCallingConfig")]
    pub function_calling_config: FunctionCallingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallingConfig {
    pub mode: ToolMode,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_function_names: Option<Vec<String>>,
}

impl ToolConfiguration {
    pub fn new(mode: ToolMode) -> Self {
        Self {
            function_calling_config: FunctionCallingConfig {
                mode,
                allowed_function_names: None,
            },
        }
    }

    pub fn with_allowed_functions(mut self, functions: Vec<String>) -> Self {
        self.function_calling_config.allowed_function_names = Some(functions);
        self
    }
}
```

**Update Request Builder**:
```rust
pub fn build_claude_request(
    openai_request: &OpenAIRequest,
    model_name: &str,
) -> Result<ClaudeRequest, String> {
    // ... existing code ...

    // Parse tool mode
    let tool_mode = ToolMode::from_openai_tool_choice(&openai_request.tool_choice);

    // Validate tool mode
    if tool_mode == ToolMode::Any && openai_request.tools.is_none() {
        return Err("Tool mode 'any' requires tools to be provided".to_string());
    }

    // Build tool configuration
    let tool_config = if openai_request.tools.is_some() {
        Some(ToolConfiguration::new(tool_mode))
    } else {
        None
    };

    // Build request
    let request = ClaudeRequest {
        model: model_name.to_string(),
        messages: transform_messages(&openai_request.messages)?,

        // Tool configuration (NEW)
        tool_config,
        tools: if tool_mode == ToolMode::None {
            None  // Ignore tools if mode is NONE
        } else {
            transform_tools(&openai_request.tools)?
        },

        // ... other fields ...
    };

    Ok(request)
}
```

**Add to Request Struct**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeRequest {
    pub model: String,
    pub messages: Vec<ClaudeMessage>,

    // Tool configuration (ADD THIS)
    #[serde(rename = "toolConfig", skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<ToolConfiguration>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ClaudeTool>>,

    // ... existing fields ...
}
```

---

#### 2. Grounding Configuration
**File**: `src-tauri/src/proxy/mappers/claude/grounding.rs` (NEW)

```rust
//! Grounding configuration for Claude models
//!
//! Provides Google Search integration and recitation policies

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundingConfig {
    #[serde(rename = "googleSearchRetrieval")]
    pub google_search_retrieval: GoogleSearchRetrieval,

    #[serde(rename = "recitationPolicy")]
    pub recitation_policy: RecitationPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleSearchRetrieval {
    pub enabled: bool,

    #[serde(rename = "dynamicRetrievalConfig")]
    pub dynamic_retrieval_config: DynamicRetrievalConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicRetrievalConfig {
    pub mode: String,  // "MODE_DYNAMIC"

    #[serde(rename = "dynamicThreshold")]
    pub dynamic_threshold: f64,  // 0.0 - 1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecitationPolicy {
    pub enabled: bool,
    pub policy: String,  // "CITED_DOCUMENTS_ONLY"
}

impl Default for GroundingConfig {
    fn default() -> Self {
        Self {
            google_search_retrieval: GoogleSearchRetrieval {
                enabled: true,
                dynamic_retrieval_config: DynamicRetrievalConfig {
                    mode: "MODE_DYNAMIC".to_string(),
                    dynamic_threshold: 0.3,
                },
            },
            recitation_policy: RecitationPolicy {
                enabled: true,
                policy: "CITED_DOCUMENTS_ONLY".to_string(),
            },
        }
    }
}

impl GroundingConfig {
    /// Create new grounding config with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Disable grounding
    pub fn disabled() -> Self {
        Self {
            google_search_retrieval: GoogleSearchRetrieval {
                enabled: false,
                dynamic_retrieval_config: DynamicRetrievalConfig {
                    mode: "MODE_DYNAMIC".to_string(),
                    dynamic_threshold: 0.3,
                },
            },
            recitation_policy: RecitationPolicy {
                enabled: false,
                policy: "CITED_DOCUMENTS_ONLY".to_string(),
            },
        }
    }

    /// Enable grounding with custom threshold
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.google_search_retrieval.dynamic_retrieval_config.dynamic_threshold = threshold;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_grounding_config() {
        let config = GroundingConfig::default();
        assert!(config.google_search_retrieval.enabled);
        assert_eq!(config.google_search_retrieval.dynamic_retrieval_config.mode, "MODE_DYNAMIC");
        assert_eq!(config.google_search_retrieval.dynamic_retrieval_config.dynamic_threshold, 0.3);
    }

    #[test]
    fn test_disabled_grounding_config() {
        let config = GroundingConfig::disabled();
        assert!(!config.google_search_retrieval.enabled);
        assert!(!config.recitation_policy.enabled);
    }

    #[test]
    fn test_custom_threshold() {
        let config = GroundingConfig::new().with_threshold(0.5);
        assert_eq!(config.google_search_retrieval.dynamic_retrieval_config.dynamic_threshold, 0.5);
    }
}
```

**Integrate with Request Builder**:
```rust
// In request.rs
use super::grounding::GroundingConfig;

pub fn build_claude_request(
    openai_request: &OpenAIRequest,
    model_name: &str,
) -> Result<ClaudeRequest, String> {
    // ... existing code ...

    // Add grounding config (optional, can be enabled via env var or config)
    let grounding_enabled = std::env::var("CLAUDE_GROUNDING_ENABLED")
        .map(|v| v == "true")
        .unwrap_or(false);

    let grounding = if grounding_enabled {
        Some(GroundingConfig::new())
    } else {
        None
    };

    let request = ClaudeRequest {
        // ... existing fields ...

        // Grounding configuration (NEW)
        #[serde(skip_serializing_if = "Option::is_none")]
        grounding,

        // ... other fields ...
    };

    Ok(request)
}
```

---

### Test Strategy

#### Unit Tests (8 tests)
**File**: `tests/claude/tool_modes_tests.rs` (NEW)

```rust
#[cfg(test)]
mod tool_mode_tests {
    use super::*;
    use crate::proxy::mappers::claude::request::ToolMode;
    use serde_json::json;

    #[test]
    fn test_tool_mode_default_is_auto() {
        let mode = ToolMode::default();
        assert_eq!(mode, ToolMode::Auto);
    }

    #[test]
    fn test_parse_tool_choice_none() {
        let mode = ToolMode::from_openai_tool_choice(&None);
        assert_eq!(mode, ToolMode::Auto);
    }

    #[test]
    fn test_parse_tool_choice_auto_string() {
        let mode = ToolMode::from_openai_tool_choice(&Some(json!("auto")));
        assert_eq!(mode, ToolMode::Auto);
    }

    #[test]
    fn test_parse_tool_choice_any_string() {
        let mode = ToolMode::from_openai_tool_choice(&Some(json!("any")));
        assert_eq!(mode, ToolMode::Any);
    }

    #[test]
    fn test_parse_tool_choice_none_string() {
        let mode = ToolMode::from_openai_tool_choice(&Some(json!("none")));
        assert_eq!(mode, ToolMode::None);
    }

    #[test]
    fn test_parse_tool_choice_object_any() {
        let mode = ToolMode::from_openai_tool_choice(&Some(json!({"type": "any"})));
        assert_eq!(mode, ToolMode::Any);
    }

    #[test]
    fn test_tool_configuration_serialization() {
        let config = ToolConfiguration::new(ToolMode::Auto);
        let json = serde_json::to_value(&config).unwrap();

        assert!(json.get("functionCallingConfig").is_some());
        assert_eq!(json["functionCallingConfig"]["mode"], "AUTO");
    }

    #[test]
    fn test_grounding_config_default() {
        let config = GroundingConfig::default();
        assert!(config.google_search_retrieval.enabled);
        assert_eq!(config.google_search_retrieval.dynamic_retrieval_config.dynamic_threshold, 0.3);
    }
}
```

---

#### Integration Tests (7 tests)
**File**: `tests/claude/tool_integration_tests.rs`

```rust
#[tokio::test]
async fn test_tool_mode_auto_includes_tools() {
    let mut openai_req = create_test_openai_request("claude-4.5-sonnet");
    openai_req.tools = Some(vec![create_test_tool()]);
    openai_req.tool_choice = Some(json!("auto"));

    let claude_req = build_claude_request(&openai_req, "claude-4.5-sonnet").unwrap();

    assert!(claude_req.tool_config.is_some());
    assert_eq!(claude_req.tool_config.unwrap().function_calling_config.mode, ToolMode::Auto);
    assert!(claude_req.tools.is_some());
}

#[tokio::test]
async fn test_tool_mode_any_requires_tools() {
    let mut openai_req = create_test_openai_request("claude-4.5-sonnet");
    openai_req.tool_choice = Some(json!("any"));
    // No tools provided

    let result = build_claude_request(&openai_req, "claude-4.5-sonnet");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Tool mode 'any' requires tools"));
}

#[tokio::test]
async fn test_tool_mode_any_with_tools() {
    let mut openai_req = create_test_openai_request("claude-4.5-sonnet");
    openai_req.tools = Some(vec![create_test_tool()]);
    openai_req.tool_choice = Some(json!("any"));

    let claude_req = build_claude_request(&openai_req, "claude-4.5-sonnet").unwrap();

    assert!(claude_req.tool_config.is_some());
    assert_eq!(claude_req.tool_config.unwrap().function_calling_config.mode, ToolMode::Any);
}

#[tokio::test]
async fn test_tool_mode_none_disables_tools() {
    let mut openai_req = create_test_openai_request("claude-4.5-sonnet");
    openai_req.tools = Some(vec![create_test_tool()]);
    openai_req.tool_choice = Some(json!("none"));

    let claude_req = build_claude_request(&openai_req, "claude-4.5-sonnet").unwrap();

    assert!(claude_req.tools.is_none());  // Tools ignored
}

#[tokio::test]
async fn test_grounding_config_serialization() {
    let grounding = GroundingConfig::new();
    let json = serde_json::to_value(&grounding).unwrap();

    assert!(json.get("googleSearchRetrieval").is_some());
    assert_eq!(json["googleSearchRetrieval"]["enabled"], true);
    assert!(json.get("recitationPolicy").is_some());
}

#[tokio::test]
async fn test_grounding_in_request() {
    std::env::set_var("CLAUDE_GROUNDING_ENABLED", "true");

    let openai_req = create_test_openai_request("claude-4.5-sonnet");
    let claude_req = build_claude_request(&openai_req, "claude-4.5-sonnet").unwrap();

    assert!(claude_req.grounding.is_some());
    let grounding = claude_req.grounding.unwrap();
    assert!(grounding.google_search_retrieval.enabled);

    std::env::remove_var("CLAUDE_GROUNDING_ENABLED");
}

#[tokio::test]
async fn test_tool_mode_request_serialization() {
    let mut openai_req = create_test_openai_request("claude-4.5-sonnet");
    openai_req.tools = Some(vec![create_test_tool()]);
    openai_req.tool_choice = Some(json!("any"));

    let claude_req = build_claude_request(&openai_req, "claude-4.5-sonnet").unwrap();
    let json = serde_json::to_value(&claude_req).unwrap();

    assert!(json.get("toolConfig").is_some());
    assert_eq!(json["toolConfig"]["functionCallingConfig"]["mode"], "ANY");
}
```

---

## 🔗 Dependencies

### Prerequisite
- ✅ Story 017-01 COMPLETE (core implementation with metadata)

### Blocks
- 🔴 Story 017-03 (Testing & Documentation) - needs complete feature set

### Enables
- ✅ 95%+ compliance (all features complete)
- ✅ Feature parity with thinking mode
- ✅ Advanced tool control for users

---

## 📊 Success Metrics

### Code Metrics
```yaml
files_created: "2 (grounding.rs, tool_modes_tests.rs)"
files_modified: "1 (request.rs)"
enums_added: "1 (ToolMode)"
structs_added: "5 (ToolConfiguration, FunctionCallingConfig, GroundingConfig, etc.)"
test_coverage:
  unit_tests: "8 tests"
  integration_tests: "7 tests"
  total: "15 tests"
```

### Quality Metrics
```yaml
regression_tests: "398/398 passing (Epic-013 baseline)"
story_017_01_tests: "20 tests passing (from Story 017-01)"
new_tests: "15 tests passing (100%)"
code_review: "Mid-Level Protocol (Dev 2B) + Team Lead"
linting: "cargo clippy clean"
```

### Business Metrics
```yaml
compliance_before: "90% (after Story 017-01)"
compliance_after: "95%+ (feature parity complete)"
gaps_resolved: "5/5 (all gaps closed)"
tool_flexibility: "COMPLETE (AUTO/ANY/NONE)"
grounding_support: "OPERATIONAL"
```

---

## 🎯 Definition of Done

- [ ] **Code Complete**:
  - [ ] ToolMode enum (AUTO/ANY/NONE) implemented
  - [ ] ToolConfiguration struct working
  - [ ] Grounding config module created
  - [ ] Request builder integrates tool modes
  - [ ] Request builder integrates grounding
  - [ ] Validation for tool mode ANY (requires tools)

- [ ] **Tests Passing**:
  - [ ] 398/398 regression tests passing
  - [ ] 20 tests from Story 017-01 passing
  - [ ] 8 new unit tests passing (100%)
  - [ ] 7 new integration tests passing (100%)

- [ ] **Quality Gates**:
  - [ ] Code review approved (Dev 2B + Team Lead)
  - [ ] Linting clean (cargo clippy)
  - [ ] Formatted correctly (cargo fmt)
  - [ ] No regressions detected

- [ ] **Documentation**:
  - [ ] grounding.rs module documented
  - [ ] Tool mode enum documented
  - [ ] Test documentation complete

- [ ] **Deployment Ready**:
  - [ ] Builds successfully
  - [ ] Integration tested with Story 017-01
  - [ ] Ready for Story 017-03 (testing)

---

## 📝 Implementation Notes

### Code Reuse from Thinking Mode
```yaml
reusable_patterns: "90%"
  - "Tool mode structure" ✅
  - "Grounding config structure" ✅
  - "Serialization patterns" ✅

differences: "None (identical to thinking mode)"
```

### Performance Considerations
```yaml
overhead: "Minimal (~0.5ms for tool mode parsing)"
grounding_overhead: "Variable (depends on search results)"
serialization: "Standard serde (no custom logic)"
```

### Security Considerations
```yaml
validation: "Tool mode ANY requires tools (error if missing)"
grounding: "Recitation policy enforced"
search_safety: "Google Search integration (safe)"
```

---

**Story Status**: 📋 READY FOR DEVELOPMENT
**Assigned To**: Dev 2B (Mid-Level Protocol Specialist)
**Start Date**: Week 4 Day 3 (after Story 017-01 complete)
**Estimated Completion**: Week 5 Day 2 (2-3 days)
