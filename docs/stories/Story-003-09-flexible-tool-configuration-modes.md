# Story-003-09: Flexible Tool Configuration Modes

**Story ID**: Story-003-09
**Epic**: [Epic-003](../epics/Epic-003-Claude-4.5-Sonnet-Thinking-Compliance.md) - Claude 4.5 Sonnet Thinking - 100% Compliance
**Priority**: P2 (Medium)
**Estimated Effort**: 2 hours
**Status**: Pending
**Created**: 2026-01-10
**Owner**: Engineering Team

---

## User Story

**As a** developer using the API proxy for tool-enabled workflows
**I want** flexible tool calling configuration modes (AUTO, ANY, NONE, VALIDATED)
**So that** I can control when and how the model uses tools based on my specific use case requirements

---

## Context

### Current Situation

**Hardcoded Behavior** (`request.rs:438-446`):
```rust
if let Some(tools_val) = tools {
    inner_request["tools"] = tools_val;
    // 显式设置工具配置模式为 VALIDATED
    inner_request["toolConfig"] = json!({
        "functionCallingConfig": {
            "mode": "VALIDATED"  // ❌ Always VALIDATED
        }
    });
}
```

**Issues**:
- ❌ **Always uses VALIDATED mode** regardless of client intent
- ❌ No support for AUTO (model decides when to use tools)
- ❌ No support for ANY (force tool usage)
- ❌ No support for NONE (disable tool calling)
- ❌ No API surface for clients to specify tool calling mode
- ❌ Missing flexibility for different use cases

### Expected Behavior

**Gemini Protocol Specification** (from Gap Analysis lines 3373-3388):
```json
{
  "toolConfig": {
    "functionCallingConfig": {
      "mode": "AUTO" | "ANY" | "NONE" | "VALIDATED"
    }
  }
}
```

**Mode Semantics**:
- **AUTO**: Model autonomously decides when to call tools (balanced approach)
- **ANY**: Model must call at least one tool in response (forced tool usage)
- **NONE**: Disable all tool calling for this request (text-only response)
- **VALIDATED**: Only call tools that pass explicit validation (most restrictive, current default)

### Gap Analysis

**Reference**: Gap Analysis (`current-implementation-thinking.md:3368-3469`) - Gap #6: Tool Configuration Mode

| Mode | Expected Support | Current Support | Gap |
|------|------------------|-----------------|-----|
| **VALIDATED** | ✅ Yes | ✅ Yes | ✅ Compliant |
| **AUTO** | ✅ Yes | ❌ No | ❌ Missing |
| **ANY** | ✅ Yes | ❌ No | ❌ Missing |
| **NONE** | ✅ Yes | ❌ No | ❌ Missing |

**Impact**: LOW to MEDIUM
- VALIDATED mode works for most cases (safe default)
- Missing flexibility reduces use case coverage:
  - **AUTO**: Cannot let model decide tool usage naturally
  - **ANY**: Cannot force tool usage for workflows requiring structured output
  - **NONE**: Cannot disable tools dynamically for specific requests

**Priority**: P2 - Medium priority for feature parity

---

## Reference Documentation

### Primary Analysis
- **Gap Analysis**: `docs/comparison/claude/claude-4-5-sonnet/current-implementation-thinking.md`
  - Lines 3368-3469: Complete Gap #6 analysis
  - Lines 3410-3417: Gap table with mode comparison
  - Lines 3423-3464: Required changes with code examples

### Current Implementation
- **Request Mapper**: `src-tauri/src/proxy/mappers/claude/request.rs`
  - Lines 438-446: Current hardcoded VALIDATED mode
  - Location: Tool config building in `map_claude_to_vertex_ai` function

- **Models**: `src-tauri/src/proxy/mappers/claude/models.rs`
  - Lines 7-31: ClaudeRequest structure (needs tool_choice field)

### Upstream Specification
- **Gemini Protocol**: Vertex AI v1internal API
  - Tool configuration: `toolConfig.functionCallingConfig.mode`
  - Supported modes: AUTO, ANY, NONE, VALIDATED

---

## Technical Details

### Architecture Overview

**Three-Layer Implementation**:
1. **API Layer**: Accept tool_choice from Claude API requests
2. **Model Layer**: Store tool mode preference in ClaudeRequest
3. **Mapping Layer**: Transform to Gemini Protocol toolConfig

```
┌─────────────────────────────────────────────────────────────┐
│ Client Request (Claude API)                                 │
│ {                                                            │
│   "tools": [...],                                            │
│   "tool_choice": {                                           │
│     "type": "auto" | "any" | "none" | "tool"               │
│   }                                                          │
│ }                                                            │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│ ClaudeRequest (Internal Model)                              │
│ pub struct ClaudeRequest {                                   │
│   pub tool_choice: Option<ToolChoice>,  // 🆕 New field     │
│ }                                                            │
│                                                              │
│ pub enum ToolChoice {                                        │
│   Auto,        // Maps to AUTO mode                         │
│   Any,         // Maps to ANY mode                          │
│   None,        // Maps to NONE mode                         │
│   Tool { name: String },  // Maps to VALIDATED + allowedFunctionNames │
│ }                                                            │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│ Gemini Protocol (Vertex AI)                                 │
│ {                                                            │
│   "toolConfig": {                                            │
│     "functionCallingConfig": {                               │
│       "mode": "AUTO" | "ANY" | "NONE" | "VALIDATED",       │
│       "allowedFunctionNames": ["specific_tool"]  // Optional │
│     }                                                        │
│   }                                                          │
│ }                                                            │
└─────────────────────────────────────────────────────────────┘
```

### Implementation Steps

#### Step 1: Define ToolChoice Enum (`models.rs`)

**Location**: `src-tauri/src/proxy/mappers/claude/models.rs` (after line 31)

**Add ToolChoice Enum**:
```rust
/// Tool calling configuration (Claude API)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolChoice {
    /// Let the model decide when to use tools (maps to AUTO)
    Auto,

    /// Force the model to use at least one tool (maps to ANY)
    Any,

    /// Disable all tool calling (maps to NONE)
    None,

    /// Force usage of a specific tool (maps to VALIDATED with allowedFunctionNames)
    Tool {
        name: String,
    },
}

impl Default for ToolChoice {
    fn default() -> Self {
        ToolChoice::Auto
    }
}

impl ToolChoice {
    /// Convert to Gemini Protocol mode string
    pub fn to_gemini_mode(&self) -> &'static str {
        match self {
            ToolChoice::Auto => "AUTO",
            ToolChoice::Any => "ANY",
            ToolChoice::None => "NONE",
            ToolChoice::Tool { .. } => "VALIDATED",
        }
    }

    /// Get specific tool name if Tool variant
    pub fn get_tool_name(&self) -> Option<&str> {
        match self {
            ToolChoice::Tool { name } => Some(name.as_str()),
            _ => None,
        }
    }
}
```

**Why This Design**:
- ✅ **Matches Claude API spec**: Uses `type` tag for JSON deserialization
- ✅ **Type safety**: Enum prevents invalid mode combinations
- ✅ **Conversion helpers**: `to_gemini_mode()` centralizes mapping logic
- ✅ **Tool forcing**: `Tool { name }` variant supports specific tool selection
- ✅ **Serde compatibility**: `#[serde(tag = "type", rename_all = "lowercase")]` for JSON mapping

#### Step 2: Add tool_choice Field to ClaudeRequest (`models.rs`)

**Location**: `src-tauri/src/proxy/mappers/claude/models.rs` (update ClaudeRequest struct)

**Update ClaudeRequest**:
```rust
/// Claude API 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<SystemPrompt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_config: Option<OutputConfig>,

    // 🆕 Tool calling mode configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
}
```

**Backward Compatibility**:
- ✅ Optional field: `Option<ToolChoice>` for backward compatibility
- ✅ Skip if absent: `#[serde(skip_serializing_if = "Option::is_none")]`
- ✅ Default behavior: None → VALIDATED mode (current behavior)

#### Step 3: Update Tool Config Building (`request.rs`)

**Location**: `src-tauri/src/proxy/mappers/claude/request.rs:438-446`

**Current Code**:
```rust
if let Some(tools_val) = tools {
    inner_request["tools"] = tools_val;
    // 显式设置工具配置模式为 VALIDATED
    inner_request["toolConfig"] = json!({
        "functionCallingConfig": {
            "mode": "VALIDATED"
        }
    });
}
```

**Enhanced Code**:
```rust
if let Some(tools_val) = tools {
    inner_request["tools"] = tools_val;

    // 🆕 Flexible tool calling mode configuration
    let tool_choice = claude_req.tool_choice.as_ref();

    // Default to VALIDATED for backward compatibility
    let mode = tool_choice
        .map(|tc| tc.to_gemini_mode())
        .unwrap_or("VALIDATED");

    // Build function calling config
    let mut function_calling_config = json!({
        "mode": mode
    });

    // For specific tool forcing (ToolChoice::Tool), add allowedFunctionNames
    if let Some(tool_name) = tool_choice.and_then(|tc| tc.get_tool_name()) {
        function_calling_config["allowedFunctionNames"] = json!([tool_name]);

        tracing::debug!(
            "[Tool-Config] Forcing specific tool: '{}', mode: {}",
            tool_name, mode
        );
    } else {
        tracing::debug!(
            "[Tool-Config] Using flexible mode: {}",
            mode
        );
    }

    inner_request["toolConfig"] = json!({
        "functionCallingConfig": function_calling_config
    });
}
```

**Key Changes**:
- ✅ **Mode selection**: Uses `tool_choice.to_gemini_mode()` for conversion
- ✅ **Default fallback**: VALIDATED when `tool_choice` is None
- ✅ **Specific tool forcing**: `allowedFunctionNames` for `ToolChoice::Tool { name }`
- ✅ **Debug logging**: Track mode selection for troubleshooting
- ✅ **Backward compatible**: Existing requests without `tool_choice` work unchanged

#### Step 4: Handle NONE Mode Optimization (Optional Enhancement)

**Location**: Same as Step 3, add after tool config building

**Optimization for NONE Mode**:
```rust
// 🆕 Optimization: If mode is NONE, skip tool building entirely
if let Some(ToolChoice::None) = claude_req.tool_choice.as_ref() {
    tracing::debug!("[Tool-Config] Mode is NONE, skipping tool configuration");
    // Don't add tools or toolConfig to inner_request
} else if let Some(tools_val) = tools {
    // ... (existing tool config building code from Step 3)
}
```

**Why This Optimization**:
- ✅ **Performance**: Avoid unnecessary tool serialization when tools disabled
- ✅ **Correctness**: Aligns with NONE semantics (no tools at all)
- ✅ **Protocol compliance**: Cleaner request structure for NONE mode

**Trade-off**: Slightly more complex control flow vs. marginal performance gain. **Recommendation**: Implement in Phase 3 testing story if needed.

### Mode Behavior Validation

**Each Mode's Expected Behavior**:

| Mode | Gemini String | Tool Array Presence | allowedFunctionNames | Model Behavior |
|------|---------------|---------------------|----------------------|----------------|
| AUTO | `"AUTO"` | Required | Not set | Model autonomously decides when to use tools |
| ANY | `"ANY"` | Required | Not set | Model MUST call at least one tool |
| NONE | `"NONE"` | Optional (can omit) | Not set | Model CANNOT call any tools |
| VALIDATED | `"VALIDATED"` | Required | Optional | Model can only call explicitly validated tools |
| Tool{name} | `"VALIDATED"` | Required | `[name]` | Model can only call the specified tool |

---

## Acceptance Criteria

### AC-1: ToolChoice Enum Implementation ✅

**Given** the models.rs file
**When** I define the ToolChoice enum
**Then** it should:
- ✅ Support 4 variants: Auto, Any, None, Tool{name}
- ✅ Derive Debug, Clone, Serialize, Deserialize, PartialEq
- ✅ Use `#[serde(tag = "type", rename_all = "lowercase")]` for JSON mapping
- ✅ Implement `to_gemini_mode()` helper returning correct mode strings
- ✅ Implement `get_tool_name()` for Tool variant extraction
- ✅ Implement Default trait returning Auto

**Validation**:
```rust
#[test]
fn test_tool_choice_enum_variants() {
    let auto = ToolChoice::Auto;
    assert_eq!(auto.to_gemini_mode(), "AUTO");
    assert_eq!(auto.get_tool_name(), None);

    let any = ToolChoice::Any;
    assert_eq!(any.to_gemini_mode(), "ANY");

    let none = ToolChoice::None;
    assert_eq!(none.to_gemini_mode(), "NONE");

    let tool = ToolChoice::Tool { name: "get_weather".to_string() };
    assert_eq!(tool.to_gemini_mode(), "VALIDATED");
    assert_eq!(tool.get_tool_name(), Some("get_weather"));
}

#[test]
fn test_tool_choice_default() {
    assert_eq!(ToolChoice::default(), ToolChoice::Auto);
}
```

### AC-2: ClaudeRequest Field Addition ✅

**Given** the ClaudeRequest struct
**When** I add the tool_choice field
**Then** it should:
- ✅ Be type `Option<ToolChoice>`
- ✅ Use `#[serde(skip_serializing_if = "Option::is_none")]`
- ✅ Maintain backward compatibility (None = VALIDATED)
- ✅ Not break existing request deserialization

**Validation**:
```rust
#[test]
fn test_claude_request_with_tool_choice() {
    let json = r#"{
        "model": "claude-4.5-sonnet-thinking",
        "messages": [],
        "tools": [{"type": "function", "function": {"name": "test"}}],
        "tool_choice": {"type": "auto"}
    }"#;

    let req: ClaudeRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.tool_choice, Some(ToolChoice::Auto));
}

#[test]
fn test_claude_request_without_tool_choice() {
    let json = r#"{
        "model": "claude-4.5-sonnet-thinking",
        "messages": [],
        "tools": [{"type": "function", "function": {"name": "test"}}]
    }"#;

    let req: ClaudeRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.tool_choice, None);
}
```

### AC-3: AUTO Mode Mapping ✅

**Given** a request with `tool_choice: { "type": "auto" }`
**When** I map to Gemini Protocol
**Then** it should:
- ✅ Set `toolConfig.functionCallingConfig.mode` to `"AUTO"`
- ✅ Not set `allowedFunctionNames`
- ✅ Log debug message with mode selection
- ✅ Include tools array in inner_request

**Validation**:
```rust
#[test]
fn test_auto_mode_mapping() {
    let claude_req = ClaudeRequest {
        model: "claude-4.5-sonnet-thinking".to_string(),
        messages: vec![],
        tools: Some(vec![/* ... */]),
        tool_choice: Some(ToolChoice::Auto),
        // ... other fields
    };

    let inner_request = map_claude_to_vertex_ai(&claude_req, /* ... */);

    assert_eq!(
        inner_request["toolConfig"]["functionCallingConfig"]["mode"],
        "AUTO"
    );
    assert!(inner_request["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"].is_null());
    assert!(inner_request["tools"].is_array());
}
```

### AC-4: ANY Mode Mapping ✅

**Given** a request with `tool_choice: { "type": "any" }`
**When** I map to Gemini Protocol
**Then** it should:
- ✅ Set `toolConfig.functionCallingConfig.mode` to `"ANY"`
- ✅ Not set `allowedFunctionNames`
- ✅ Force model to call at least one tool
- ✅ Include tools array in inner_request

**Validation**:
```rust
#[test]
fn test_any_mode_mapping() {
    let claude_req = ClaudeRequest {
        model: "claude-4.5-sonnet-thinking".to_string(),
        messages: vec![],
        tools: Some(vec![/* ... */]),
        tool_choice: Some(ToolChoice::Any),
        // ... other fields
    };

    let inner_request = map_claude_to_vertex_ai(&claude_req, /* ... */);

    assert_eq!(
        inner_request["toolConfig"]["functionCallingConfig"]["mode"],
        "ANY"
    );
    assert!(inner_request["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"].is_null());
}
```

### AC-5: NONE Mode Mapping ✅

**Given** a request with `tool_choice: { "type": "none" }`
**When** I map to Gemini Protocol
**Then** it should:
- ✅ Set `toolConfig.functionCallingConfig.mode` to `"NONE"`
- ✅ Disable all tool calling for this request
- ✅ Model returns text-only response

**Validation**:
```rust
#[test]
fn test_none_mode_mapping() {
    let claude_req = ClaudeRequest {
        model: "claude-4.5-sonnet-thinking".to_string(),
        messages: vec![],
        tools: Some(vec![/* ... */]),
        tool_choice: Some(ToolChoice::None),
        // ... other fields
    };

    let inner_request = map_claude_to_vertex_ai(&claude_req, /* ... */);

    assert_eq!(
        inner_request["toolConfig"]["functionCallingConfig"]["mode"],
        "NONE"
    );
}
```

### AC-6: Tool{name} Mode Mapping ✅

**Given** a request with `tool_choice: { "type": "tool", "name": "get_weather" }`
**When** I map to Gemini Protocol
**Then** it should:
- ✅ Set `toolConfig.functionCallingConfig.mode` to `"VALIDATED"`
- ✅ Set `allowedFunctionNames` to `["get_weather"]`
- ✅ Log debug message with specific tool name
- ✅ Force model to use only the specified tool

**Validation**:
```rust
#[test]
fn test_tool_forcing_mapping() {
    let claude_req = ClaudeRequest {
        model: "claude-4.5-sonnet-thinking".to_string(),
        messages: vec![],
        tools: Some(vec![/* ... */]),
        tool_choice: Some(ToolChoice::Tool {
            name: "get_weather".to_string()
        }),
        // ... other fields
    };

    let inner_request = map_claude_to_vertex_ai(&claude_req, /* ... */);

    assert_eq!(
        inner_request["toolConfig"]["functionCallingConfig"]["mode"],
        "VALIDATED"
    );
    assert_eq!(
        inner_request["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"],
        json!(["get_weather"])
    );
}
```

### AC-7: Backward Compatibility (No tool_choice) ✅

**Given** a request without `tool_choice` field
**When** I map to Gemini Protocol
**Then** it should:
- ✅ Default to `"VALIDATED"` mode (current behavior)
- ✅ Not break existing workflows
- ✅ Maintain same request structure as before

**Validation**:
```rust
#[test]
fn test_backward_compatibility_no_tool_choice() {
    let claude_req = ClaudeRequest {
        model: "claude-4.5-sonnet-thinking".to_string(),
        messages: vec![],
        tools: Some(vec![/* ... */]),
        tool_choice: None,  // No tool_choice specified
        // ... other fields
    };

    let inner_request = map_claude_to_vertex_ai(&claude_req, /* ... */);

    // Should default to VALIDATED for backward compatibility
    assert_eq!(
        inner_request["toolConfig"]["functionCallingConfig"]["mode"],
        "VALIDATED"
    );
    assert!(inner_request["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"].is_null());
}
```

### AC-8: JSON Deserialization ✅

**Given** various tool_choice JSON formats
**When** I deserialize into ToolChoice enum
**Then** it should:
- ✅ Parse `{"type": "auto"}` → Auto
- ✅ Parse `{"type": "any"}` → Any
- ✅ Parse `{"type": "none"}` → None
- ✅ Parse `{"type": "tool", "name": "func"}` → Tool{name: "func"}
- ✅ Reject invalid formats with clear error

**Validation**:
```rust
#[test]
fn test_tool_choice_deserialization() {
    // Auto
    let json = r#"{"type": "auto"}"#;
    let tc: ToolChoice = serde_json::from_str(json).unwrap();
    assert_eq!(tc, ToolChoice::Auto);

    // Any
    let json = r#"{"type": "any"}"#;
    let tc: ToolChoice = serde_json::from_str(json).unwrap();
    assert_eq!(tc, ToolChoice::Any);

    // None
    let json = r#"{"type": "none"}"#;
    let tc: ToolChoice = serde_json::from_str(json).unwrap();
    assert_eq!(tc, ToolChoice::None);

    // Tool
    let json = r#"{"type": "tool", "name": "get_weather"}"#;
    let tc: ToolChoice = serde_json::from_str(json).unwrap();
    assert_eq!(tc, ToolChoice::Tool { name: "get_weather".to_string() });

    // Invalid
    let json = r#"{"type": "invalid"}"#;
    assert!(serde_json::from_str::<ToolChoice>(json).is_err());
}
```

### AC-9: Debug Logging ✅

**Given** any tool_choice mode selection
**When** I build tool config
**Then** it should:
- ✅ Log mode selection with `tracing::debug!()`
- ✅ Include mode name in log message
- ✅ For Tool variant, log specific tool name
- ✅ Use `[Tool-Config]` prefix for filtering

**Validation**:
```rust
#[test]
fn test_tool_config_logging() {
    // Initialize test logger
    let _guard = init_test_logging();

    let claude_req = ClaudeRequest {
        model: "claude-4.5-sonnet-thinking".to_string(),
        messages: vec![],
        tools: Some(vec![/* ... */]),
        tool_choice: Some(ToolChoice::Tool {
            name: "get_weather".to_string()
        }),
        // ... other fields
    };

    map_claude_to_vertex_ai(&claude_req, /* ... */);

    // Check logs contain expected messages
    // (Implementation depends on test logging infrastructure)
}
```

### AC-10: No Tools with NONE Mode ✅

**Given** a request with `tool_choice: None` but no tools array
**When** I map to Gemini Protocol
**Then** it should:
- ✅ Not create toolConfig at all
- ✅ Handle gracefully without errors
- ✅ Log debug message about skipped tool config

**Validation**:
```rust
#[test]
fn test_none_mode_without_tools() {
    let claude_req = ClaudeRequest {
        model: "claude-4.5-sonnet-thinking".to_string(),
        messages: vec![],
        tools: None,  // No tools provided
        tool_choice: Some(ToolChoice::None),
        // ... other fields
    };

    let inner_request = map_claude_to_vertex_ai(&claude_req, /* ... */);

    // Should not have toolConfig
    assert!(inner_request.get("toolConfig").is_none());
}
```

### AC-11: Invalid Tool Name Handling ✅

**Given** a request with `tool_choice: Tool { name: "nonexistent" }`
**When** the tool name is not in tools array
**Then** it should:
- ✅ Still set allowedFunctionNames to specified name
- ✅ Let upstream API validate tool existence
- ✅ Log warning about potential mismatch (optional)

**Validation**:
```rust
#[test]
fn test_invalid_tool_name_passthrough() {
    let claude_req = ClaudeRequest {
        model: "claude-4.5-sonnet-thinking".to_string(),
        messages: vec![],
        tools: Some(vec![
            // Tool with different name
            serde_json::json!({
                "type": "function",
                "function": {"name": "other_tool"}
            })
        ]),
        tool_choice: Some(ToolChoice::Tool {
            name: "nonexistent".to_string()
        }),
        // ... other fields
    };

    let inner_request = map_claude_to_vertex_ai(&claude_req, /* ... */);

    // Should still set the requested name, upstream will validate
    assert_eq!(
        inner_request["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"],
        json!(["nonexistent"])
    );
}
```

### AC-12: Compliance Score Improvement ✅

**Given** this story's implementation
**When** I complete all mode support
**Then** it should:
- ✅ Close Gap #6: Tool Configuration Mode
- ✅ Increase compliance score from 95% → 96.67%
- ✅ Pass manual validation against RE spec
- ✅ Support all 4 Gemini Protocol modes

**Validation**: Manual testing with all 4 modes against live Vertex AI endpoint

---

## Testing Strategy

### Unit Tests (9 tests)

**File**: `src-tauri/src/proxy/mappers/claude/request_tests.rs`

```rust
#[cfg(test)]
mod tool_choice_tests {
    use super::*;

    #[test]
    fn test_tool_choice_enum_variants() {
        // AC-1: Test all enum variants and helper methods
        let auto = ToolChoice::Auto;
        assert_eq!(auto.to_gemini_mode(), "AUTO");
        assert_eq!(auto.get_tool_name(), None);

        let any = ToolChoice::Any;
        assert_eq!(any.to_gemini_mode(), "ANY");
        assert_eq!(any.get_tool_name(), None);

        let none = ToolChoice::None;
        assert_eq!(none.to_gemini_mode(), "NONE");
        assert_eq!(none.get_tool_name(), None);

        let tool = ToolChoice::Tool { name: "get_weather".to_string() };
        assert_eq!(tool.to_gemini_mode(), "VALIDATED");
        assert_eq!(tool.get_tool_name(), Some("get_weather"));
    }

    #[test]
    fn test_tool_choice_default() {
        // AC-1: Test Default trait
        assert_eq!(ToolChoice::default(), ToolChoice::Auto);
    }

    #[test]
    fn test_tool_choice_deserialization() {
        // AC-8: Test JSON deserialization for all variants

        // Auto
        let json = r#"{"type": "auto"}"#;
        let tc: ToolChoice = serde_json::from_str(json).unwrap();
        assert_eq!(tc, ToolChoice::Auto);

        // Any
        let json = r#"{"type": "any"}"#;
        let tc: ToolChoice = serde_json::from_str(json).unwrap();
        assert_eq!(tc, ToolChoice::Any);

        // None
        let json = r#"{"type": "none"}"#;
        let tc: ToolChoice = serde_json::from_str(json).unwrap();
        assert_eq!(tc, ToolChoice::None);

        // Tool
        let json = r#"{"type": "tool", "name": "get_weather"}"#;
        let tc: ToolChoice = serde_json::from_str(json).unwrap();
        assert_eq!(tc, ToolChoice::Tool { name: "get_weather".to_string() });

        // Invalid type
        let json = r#"{"type": "invalid"}"#;
        assert!(serde_json::from_str::<ToolChoice>(json).is_err());

        // Missing name for tool type
        let json = r#"{"type": "tool"}"#;
        assert!(serde_json::from_str::<ToolChoice>(json).is_err());
    }

    #[test]
    fn test_claude_request_with_tool_choice() {
        // AC-2: Test ClaudeRequest deserialization with tool_choice
        let json = r#"{
            "model": "claude-4.5-sonnet-thinking",
            "messages": [],
            "stream": false,
            "tools": [{"type": "function", "function": {"name": "test"}}],
            "tool_choice": {"type": "auto"}
        }"#;

        let req: ClaudeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.tool_choice, Some(ToolChoice::Auto));
    }

    #[test]
    fn test_claude_request_without_tool_choice() {
        // AC-2: Test backward compatibility
        let json = r#"{
            "model": "claude-4.5-sonnet-thinking",
            "messages": [],
            "stream": false,
            "tools": [{"type": "function", "function": {"name": "test"}}]
        }"#;

        let req: ClaudeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.tool_choice, None);
    }

    #[test]
    fn test_auto_mode_mapping() {
        // AC-3: Test AUTO mode conversion
        let mut claude_req = create_test_claude_request();
        claude_req.tool_choice = Some(ToolChoice::Auto);
        claude_req.tools = Some(vec![create_test_tool("get_weather")]);

        let config = create_test_config();
        let inner_request = map_claude_to_vertex_ai(&claude_req, &config, "test-trace");

        assert_eq!(
            inner_request["toolConfig"]["functionCallingConfig"]["mode"],
            "AUTO"
        );
        assert!(inner_request["toolConfig"]["functionCallingConfig"]
            .get("allowedFunctionNames")
            .is_none());
        assert!(inner_request["tools"].is_array());
    }

    #[test]
    fn test_any_mode_mapping() {
        // AC-4: Test ANY mode conversion
        let mut claude_req = create_test_claude_request();
        claude_req.tool_choice = Some(ToolChoice::Any);
        claude_req.tools = Some(vec![create_test_tool("get_weather")]);

        let config = create_test_config();
        let inner_request = map_claude_to_vertex_ai(&claude_req, &config, "test-trace");

        assert_eq!(
            inner_request["toolConfig"]["functionCallingConfig"]["mode"],
            "ANY"
        );
        assert!(inner_request["toolConfig"]["functionCallingConfig"]
            .get("allowedFunctionNames")
            .is_none());
    }

    #[test]
    fn test_none_mode_mapping() {
        // AC-5: Test NONE mode conversion
        let mut claude_req = create_test_claude_request();
        claude_req.tool_choice = Some(ToolChoice::None);
        claude_req.tools = Some(vec![create_test_tool("get_weather")]);

        let config = create_test_config();
        let inner_request = map_claude_to_vertex_ai(&claude_req, &config, "test-trace");

        assert_eq!(
            inner_request["toolConfig"]["functionCallingConfig"]["mode"],
            "NONE"
        );
    }

    #[test]
    fn test_tool_forcing_mapping() {
        // AC-6: Test Tool{name} mode conversion
        let mut claude_req = create_test_claude_request();
        claude_req.tool_choice = Some(ToolChoice::Tool {
            name: "get_weather".to_string()
        });
        claude_req.tools = Some(vec![
            create_test_tool("get_weather"),
            create_test_tool("get_forecast"),
        ]);

        let config = create_test_config();
        let inner_request = map_claude_to_vertex_ai(&claude_req, &config, "test-trace");

        assert_eq!(
            inner_request["toolConfig"]["functionCallingConfig"]["mode"],
            "VALIDATED"
        );
        assert_eq!(
            inner_request["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"],
            json!(["get_weather"])
        );
    }

    #[test]
    fn test_backward_compatibility_no_tool_choice() {
        // AC-7: Test default VALIDATED mode for backward compatibility
        let mut claude_req = create_test_claude_request();
        claude_req.tool_choice = None;
        claude_req.tools = Some(vec![create_test_tool("get_weather")]);

        let config = create_test_config();
        let inner_request = map_claude_to_vertex_ai(&claude_req, &config, "test-trace");

        // Should default to VALIDATED
        assert_eq!(
            inner_request["toolConfig"]["functionCallingConfig"]["mode"],
            "VALIDATED"
        );
        assert!(inner_request["toolConfig"]["functionCallingConfig"]
            .get("allowedFunctionNames")
            .is_none());
    }

    // Test helpers
    fn create_test_claude_request() -> ClaudeRequest {
        ClaudeRequest {
            model: "claude-4.5-sonnet-thinking".to_string(),
            messages: vec![],
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
        }
    }

    fn create_test_tool(name: &str) -> serde_json::Value {
        json!({
            "type": "function",
            "function": {
                "name": name,
                "description": "Test tool",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        })
    }

    fn create_test_config() -> ProxyConfig {
        // Create minimal config for testing
        // ...
    }
}
```

### Integration Tests (2 tests)

**File**: `src-tauri/src/proxy/handlers/claude_tests.rs`

```rust
#[cfg(test)]
mod tool_choice_integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_end_to_end_tool_choice_auto() {
        // AC-3: Test full request/response flow with AUTO mode
        let request_body = json!({
            "model": "claude-4.5-sonnet-thinking",
            "messages": [{
                "role": "user",
                "content": "What's the weather in Tokyo?"
            }],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather for a location",
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

        // Send request through handler
        // Verify toolConfig in upstream request
        // Verify response format
    }

    #[tokio::test]
    async fn test_end_to_end_tool_choice_force_specific() {
        // AC-6: Test forced tool calling
        let request_body = json!({
            "model": "claude-4.5-sonnet-thinking",
            "messages": [{
                "role": "user",
                "content": "Tell me about the weather"
            }],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather for a location",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "location": {"type": "string"}
                        },
                        "required": ["location"]
                    }
                }
            }, {
                "type": "function",
                "function": {
                    "name": "get_forecast",
                    "description": "Get weather forecast",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "location": {"type": "string"}
                        },
                        "required": ["location"]
                    }
                }
            }],
            "tool_choice": {
                "type": "tool",
                "name": "get_weather"
            },
            "max_tokens": 1024
        });

        // Send request through handler
        // Verify allowedFunctionNames = ["get_weather"]
        // Verify model only calls get_weather, not get_forecast
    }
}
```

### Manual Testing Checklist

- [ ] **AUTO Mode**:
  - [ ] Send request with AUTO mode
  - [ ] Verify model decides when to use tools
  - [ ] Test with multiple tools available
  - [ ] Verify toolConfig.mode = "AUTO" in logs

- [ ] **ANY Mode**:
  - [ ] Send request with ANY mode
  - [ ] Verify model calls at least one tool
  - [ ] Test with single tool and multiple tools
  - [ ] Verify toolConfig.mode = "ANY" in logs

- [ ] **NONE Mode**:
  - [ ] Send request with NONE mode
  - [ ] Verify model returns text-only response
  - [ ] Verify no tool calls in response
  - [ ] Verify toolConfig.mode = "NONE" in logs

- [ ] **Tool{name} Mode**:
  - [ ] Send request forcing specific tool
  - [ ] Verify model only calls specified tool
  - [ ] Test with non-matching tool name (expect upstream error)
  - [ ] Verify allowedFunctionNames in logs

- [ ] **Backward Compatibility**:
  - [ ] Send request without tool_choice
  - [ ] Verify VALIDATED mode default
  - [ ] Verify existing workflows unchanged

- [ ] **Error Handling**:
  - [ ] Invalid tool_choice type → 400 error
  - [ ] Tool{name} with empty name → 400 error
  - [ ] NONE mode with required tool → upstream handles

---

## Definition of Done

### Code Quality
- [ ] ✅ ToolChoice enum implemented with all 4 variants
- [ ] ✅ ClaudeRequest updated with tool_choice field
- [ ] ✅ Tool config building updated in request.rs
- [ ] ✅ All helper methods implemented (to_gemini_mode, get_tool_name)
- [ ] ✅ Code follows Rust best practices
- [ ] ✅ No clippy warnings
- [ ] ✅ Code formatted with rustfmt

### Testing
- [ ] ✅ All 9 unit tests passing
- [ ] ✅ All 2 integration tests passing
- [ ] ✅ Manual testing completed for all modes
- [ ] ✅ Backward compatibility validated
- [ ] ✅ Edge cases tested (no tools, invalid names)

### Documentation
- [ ] ✅ Code comments added for ToolChoice enum
- [ ] ✅ Doc comments for helper methods
- [ ] ✅ Mode semantics documented
- [ ] ✅ Debug logging added with clear messages
- [ ] ✅ Story marked as completed

### Validation
- [ ] ✅ Gap #6 closed in Gap Analysis
- [ ] ✅ Compliance score updated: 95% → 96.67%
- [ ] ✅ All AC validated
- [ ] ✅ Manual testing against Vertex AI successful
- [ ] ✅ No regressions in existing functionality

### Integration
- [ ] ✅ Changes committed to feature branch
- [ ] ✅ Code review completed
- [ ] ✅ Merged to main branch
- [ ] ✅ Epic-003 updated with completion status

---

## Dependencies

### Upstream Dependencies
- ✅ **Story-003-01**: Model ID constant (completed)
- ✅ **Story-003-02**: API/Model Provider constants (completed)
- ✅ **Story-003-03**: Antigravity metadata (completed)
- ✅ **Story-003-05**: JWT signature validation (completed)

**Why**: These stories establish the foundation for request mapping and validation.

### Downstream Dependencies
- **Story-003-10**: Grounding Configuration (next)
- **Story-003-11**: Tool Mode Testing (validation)

**Impact**: Story-003-11 will test all 4 modes end-to-end to ensure correct behavior.

---

## Risk Analysis

### Risk #1: Breaking Existing Tool Workflows
**Severity**: MEDIUM
**Probability**: LOW
**Impact**: Existing tool-enabled requests fail or behave differently

**Mitigation**:
- ✅ Use `Option<ToolChoice>` for backward compatibility
- ✅ Default to VALIDATED when tool_choice is None
- ✅ Comprehensive testing of existing workflows
- ✅ Gradual rollout with monitoring

**Contingency**: If issues arise, add feature flag to disable tool_choice and revert to hardcoded VALIDATED.

### Risk #2: Upstream API Mode Rejection
**Severity**: MEDIUM
**Probability**: LOW
**Impact**: Vertex AI rejects certain mode combinations

**Mitigation**:
- ✅ Follow Gap Analysis spec exactly (tested against RE)
- ✅ Manual testing with all 4 modes
- ✅ Add upstream error handling and logging
- ✅ Document any mode-specific limitations

**Contingency**: If mode rejected, log warning and fallback to VALIDATED with client notification.

### Risk #3: Invalid Tool Name Handling
**Severity**: LOW
**Probability**: MEDIUM
**Impact**: Client specifies non-existent tool in Tool{name} mode

**Mitigation**:
- ✅ Pass-through approach: let upstream validate
- ✅ Optional validation: check tool name exists in tools array
- ✅ Clear error messages from upstream
- ✅ Document expected behavior

**Contingency**: Add optional client-side validation if needed based on user feedback.

---

## Implementation Notes

### Design Decisions

**1. Why ToolChoice Instead of ToolMode?**
- ✅ **Matches Claude API**: Official Claude API uses `tool_choice`
- ✅ **Industry standard**: OpenAI uses `tool_choice`
- ✅ **Clear semantics**: "choice" implies selection, "mode" is ambiguous

**2. Why Tagged Enum for Serde?**
- ✅ **Type safety**: Prevents invalid combinations
- ✅ **JSON mapping**: `#[serde(tag = "type")]` matches Claude API format
- ✅ **Extensibility**: Easy to add new modes without breaking changes

**3. Why Default to VALIDATED?**
- ✅ **Backward compatibility**: Existing behavior preserved
- ✅ **Conservative default**: Most restrictive mode prevents unexpected tool calls
- ✅ **Explicit opt-in**: Clients must explicitly request other modes

**4. Why Pass-through Invalid Tool Names?**
- ✅ **Separation of concerns**: Let upstream API validate tool existence
- ✅ **Simpler implementation**: No need to maintain tool registry
- ✅ **Clear error source**: Upstream errors more specific than client-side validation

### Performance Considerations

**Impact**: NEGLIGIBLE
- ✅ Enum comparison: O(1)
- ✅ String conversion: Compile-time for mode strings
- ✅ JSON serialization: Minimal overhead for optional field
- ✅ No additional network calls or database queries

**Measurement**: <1ms additional latency in request mapping.

### Future Enhancements

**Phase 4 Optimization**:
- Add client-side tool name validation (optional)
- Add metrics for mode usage distribution
- Add dashboard visualization of mode selection
- Support OpenAI tool_choice format conversion (if needed)

---

## File Impact Analysis

### Modified Files

| File | Lines Changed | Change Type | Description |
|------|---------------|-------------|-------------|
| `models.rs` | +60 | Addition | ToolChoice enum, ClaudeRequest field |
| `request.rs` | +25 | Modification | Tool config building logic |
| `request_tests.rs` | +220 | Addition | 9 unit tests |
| `claude_tests.rs` | +90 | Addition | 2 integration tests |

**Total Changes**:
- **Production Code**: ~85 lines
- **Test Code**: ~310 lines
- **Test/Code Ratio**: 3.6:1 (high test coverage)

### New Files
None (all changes to existing files)

---

## Change Log

| Date | Change | Author |
|------|--------|--------|
| 2026-01-10 | Story created with comprehensive analysis | BMad Master |

---

## References

### Gap Analysis
- **Document**: `docs/comparison/claude/claude-4-5-sonnet/current-implementation-thinking.md`
- **Section**: Lines 3368-3469 (Gap #6: Tool Configuration Mode)
- **Mode Table**: Lines 3410-3417
- **Required Changes**: Lines 3423-3464

### Current Implementation
- **Request Mapper**: `src-tauri/src/proxy/mappers/claude/request.rs:438-446`
- **Models**: `src-tauri/src/proxy/mappers/claude/models.rs:7-31`

### Gemini Protocol
- **Tool Config**: `toolConfig.functionCallingConfig.mode`
- **Modes**: AUTO, ANY, NONE, VALIDATED
- **Optional**: `allowedFunctionNames` for tool forcing

### Claude API
- **Specification**: Claude API uses `tool_choice` parameter
- **Format**: `{"type": "auto"|"any"|"none"|"tool", "name": "..."}`
