# Developer Guide - Gemini 3 API Integration

**Version**: 3.5.0
**Last Updated**: 2026-01-11
**Purpose**: Developer reference for implementing Gemini 3 thinking support

---

## Overview

This guide provides implementation details for developers working on Gemini 3 API integration, including detection, mapping, validation, and testing.

---

## Architecture Overview

### Module Organization

```
src-tauri/src/proxy/mappers/
├── common/
│   ├── gemini_detection.rs        # Model generation detection
│   ├── thinking_level_mapper.rs   # Budget-to-level conversion
│   ├── gemini_api_validator.rs    # API format validation
│   └── tests/                      # Unit tests
├── openai/
│   └── request.rs                  # OpenAI → Gemini conversion
└── claude/
    └── request.rs                  # Claude → Gemini conversion
```

---

## Core Modules

### 1. Model Detection

**File**: `src-tauri/src/proxy/mappers/common/gemini_detection.rs`

**Purpose**: Centralized model generation detection

**API**:
```rust
/// Detect if model is Gemini 3.x
pub fn is_gemini_3_model(model: &str) -> bool

/// Detect if model is Gemini 2.5
pub fn is_gemini_2_5_model(model: &str) -> bool

/// Detect if model is Gemini 3 Flash variant
pub fn is_gemini_3_flash(model: &str) -> bool

/// Detect if model is Gemini 3 Pro variant
pub fn is_gemini_3_pro(model: &str) -> bool
```

**Usage Example**:
```rust
use crate::proxy::mappers::common::gemini_detection::is_gemini_3_model;

if is_gemini_3_model(&model) {
    // Use thinkingLevel API
    handle_gemini_3_thinking(&model, budget)
} else if is_gemini_2_5_model(&model) {
    // Use thinkingBudget API
    handle_gemini_2_5_thinking(&model, budget)
}
```

**Implementation**:
```rust
pub fn is_gemini_3_model(model: &str) -> bool {
    model.contains("gemini-3")
}

pub fn is_gemini_3_flash(model: &str) -> bool {
    model.contains("gemini-3") && model.contains("-flash")
}

pub fn is_gemini_3_pro(model: &str) -> bool {
    model.contains("gemini-3") && model.contains("-pro")
}
```

---

### 2. Budget-to-Level Mapping

**File**: `src-tauri/src/proxy/mappers/common/thinking_level_mapper.rs`

**Purpose**: Convert integer budgets to enum levels

**API**:
```rust
/// Determine thinking level from budget and model
pub fn determine_thinking_level(
    model: &str,
    budget: Option<i32>
) -> &'static str

/// Get default thinking level for model
pub fn get_default_thinking_level(model: &str) -> &'static str
```

**Usage Example**:
```rust
use crate::proxy::mappers::common::thinking_level_mapper::determine_thinking_level;

let budget = Some(15000);
let level = determine_thinking_level("gemini-3-flash", budget);
assert_eq!(level, "MEDIUM");
```

**Implementation**:
```rust
pub fn determine_thinking_level(model: &str, budget: Option<i32>) -> &'static str {
    let budget = budget.unwrap_or(16000); // Default budget

    if is_gemini_3_flash(model) {
        match budget {
            0..=4000 => "MINIMAL",
            4001..=10000 => "LOW",
            10001..=20000 => "MEDIUM",
            _ => "HIGH",
        }
    } else if is_gemini_3_pro(model) {
        match budget {
            0..=16000 => "LOW",
            _ => "HIGH",
        }
    } else {
        "HIGH" // Default for unknown models
    }
}

pub fn get_default_thinking_level(model: &str) -> &'static str {
    if is_gemini_3_flash(model) {
        "MEDIUM" // Flash default for balance
    } else {
        "HIGH"   // Pro default for quality
    }
}
```

**Mapping Tables**:

Flash (4 levels):
```rust
const FLASH_MAPPING: [(i32, i32, &str); 4] = [
    (0,     4000,  "MINIMAL"),
    (4001,  10000, "LOW"),
    (10001, 20000, "MEDIUM"),
    (20001, 32000, "HIGH"),
];
```

Pro (2 levels):
```rust
const PRO_MAPPING: [(i32, i32, &str); 2] = [
    (0,     16000, "LOW"),
    (16001, 32000, "HIGH"),
];
```

---

### 3. API Validation

**File**: `src-tauri/src/proxy/mappers/common/gemini_api_validator.rs`

**Purpose**: Validate API format compatibility

**API**:
```rust
/// Validate Gemini request against model requirements
pub fn validate_gemini_request(
    model: &str,
    request_body: &serde_json::Value
) -> Result<(), String>

/// Validate thinking level for model
pub fn validate_thinking_level(
    model: &str,
    level: &str
) -> Result<(), String>
```

**Usage Example**:
```rust
use crate::proxy::mappers::common::gemini_api_validator::validate_gemini_request;

// Validate before sending to upstream
validate_gemini_request(&model, &request_body)?;

// Send to Google API
send_to_google_api(request_body).await
```

**Implementation**:
```rust
pub fn validate_gemini_request(
    model: &str,
    request_body: &serde_json::Value
) -> Result<(), String> {
    let thinking_config = request_body
        .get("generationConfig")
        .and_then(|c| c.get("thinkingConfig"));

    if let Some(config) = thinking_config {
        if is_gemini_3_model(model) {
            // Gemini 3.x must use thinkingLevel
            if config.get("thinkingBudget").is_some() {
                return Err(format!(
                    "Gemini 3.x model '{}' must use thinkingLevel API, not thinkingBudget",
                    model
                ));
            }

            // Validate level if present
            if let Some(level) = config.get("thinkingLevel").and_then(|l| l.as_str()) {
                validate_thinking_level(model, level)?;
            }
        } else if is_gemini_2_5_model(model) {
            // Gemini 2.5 must use thinkingBudget
            if config.get("thinkingLevel").is_some() {
                return Err(format!(
                    "Gemini 2.5 model '{}' must use thinkingBudget API, not thinkingLevel",
                    model
                ));
            }
        }
    }

    Ok(())
}

pub fn validate_thinking_level(model: &str, level: &str) -> Result<(), String> {
    let valid_levels = if is_gemini_3_flash(model) {
        vec!["MINIMAL", "LOW", "MEDIUM", "HIGH"]
    } else if is_gemini_3_pro(model) {
        vec!["LOW", "HIGH"]
    } else {
        return Ok(()); // Unknown model, skip validation
    };

    if !valid_levels.contains(&level) {
        return Err(format!(
            "Model '{}' has invalid thinkingLevel: '{}'. Valid levels: {}",
            model,
            level,
            valid_levels.join(", ")
        ));
    }

    Ok(())
}
```

---

## Protocol Handlers

### OpenAI Request Handler

**File**: `src-tauri/src/proxy/mappers/openai/request.rs`

**Integration Points**:

**1. Auto-Injection Logic**:
```rust
// Location: ~line 247
let is_gemini_3_thinking = is_gemini_3_model(&mapped_model);

if is_gemini_3_thinking && !has_explicit_thinking_config {
    // Auto-inject default thinking level
    let default_level = get_default_thinking_level(&mapped_model);

    gen_config["thinkingConfig"] = json!({
        "includeThoughts": true,
        "thinkingLevel": default_level
    });
}
```

**2. Budget Conversion**:
```rust
// Extract thinking budget from OpenAI request
let thinking_budget = req_body
    .get("thinking_budget")
    .and_then(|b| b.as_i64())
    .map(|b| b as i32);

if let Some(budget) = thinking_budget {
    if is_gemini_3_model(&mapped_model) {
        // Convert budget to level
        let level = determine_thinking_level(&mapped_model, Some(budget));

        gen_config["thinkingConfig"] = json!({
            "includeThoughts": true,
            "thinkingLevel": level
        });
    } else {
        // Use budget directly for Gemini 2.5
        gen_config["thinkingConfig"] = json!({
            "includeThoughts": true,
            "thinkingBudget": budget
        });
    }
}
```

**3. Validation**:
```rust
// Validate before sending to upstream
validate_gemini_request(&mapped_model, &upstream_body)?;
```

---

### Claude Request Handler

**File**: `src-tauri/src/proxy/mappers/claude/request.rs`

**Integration Points**:

**1. Extract Thinking Config**:
```rust
// Location: ~line 1350
let thinking_config = req_body.get("thinking");
let thinking_budget = thinking_config
    .and_then(|t| t.get("budget"))
    .and_then(|b| b.as_i64())
    .map(|b| b as i32);
```

**2. Model-Specific Handling**:
```rust
if is_gemini_3_model(&mapped_model) {
    // Gemini 3: Use thinkingLevel
    let level = determine_thinking_level(&mapped_model, thinking_budget);

    gen_config["thinkingConfig"] = json!({
        "includeThoughts": true,
        "thinkingLevel": level
    });
} else if is_gemini_2_5_model(&mapped_model) {
    // Gemini 2.5: Use thinkingBudget
    gen_config["thinkingConfig"] = json!({
        "includeThoughts": true,
        "thinkingBudget": thinking_budget.unwrap_or(16000)
    });
}
```

---

## Testing

### Unit Tests

**Location**: `src-tauri/src/proxy/mappers/common/tests/`

**Run Tests**:
```bash
# Run all Gemini tests
cargo test --lib gemini

# Run specific module tests
cargo test --lib gemini_detection
cargo test --lib thinking_level_mapper
cargo test --lib gemini_api_validator
```

**Example Test**:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gemini_3_flash_detection() {
        assert!(is_gemini_3_model("gemini-3-flash"));
        assert!(is_gemini_3_flash("gemini-3-flash"));
        assert!(!is_gemini_3_pro("gemini-3-flash"));
    }

    #[test]
    fn test_flash_budget_mapping() {
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(3000)),
            "MINIMAL"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(8000)),
            "LOW"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(15000)),
            "MEDIUM"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(25000)),
            "HIGH"
        );
    }

    #[test]
    fn test_pro_budget_mapping() {
        assert_eq!(
            determine_thinking_level("gemini-3-pro-high", Some(10000)),
            "LOW"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-pro-high", Some(20000)),
            "HIGH"
        );
    }

    #[test]
    fn test_api_validation() {
        let request = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "thinkingLevel": "HIGH"
                }
            }
        });

        // Should succeed for Gemini 3
        assert!(validate_gemini_request("gemini-3-flash", &request).is_ok());

        // Should fail for Gemini 2.5
        assert!(validate_gemini_request("gemini-2.5-flash-thinking", &request).is_err());
    }

    #[test]
    fn test_invalid_level_validation() {
        // MEDIUM not valid for Pro
        let result = validate_thinking_level("gemini-3-pro-high", "MEDIUM");
        assert!(result.is_err());

        // MEDIUM valid for Flash
        let result = validate_thinking_level("gemini-3-flash", "MEDIUM");
        assert!(result.is_ok());
    }
}
```

---

### Integration Tests

**Run Integration Tests**:
```bash
cargo test --lib gemini_3_flash_integration
cargo test --lib gemini_3_pro_integration
cargo test --lib gemini_3_e2e_protocol
```

**Example Integration Test**:
```rust
#[tokio::test]
async fn test_gemini_3_flash_openai_protocol() {
    let client = TestClient::new();

    let response = client
        .post("/v1/chat/completions")
        .json(&json!({
            "model": "gemini-3-flash",
            "messages": [
                {"role": "user", "content": "Test thinking"}
            ],
            "thinking_budget": 15000
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.unwrap();

    // Verify thinking content present
    let reasoning = body["choices"][0]["message"]["reasoning_content"]
        .as_str()
        .unwrap();
    assert!(!reasoning.is_empty());
}
```

---

### Coverage Analysis

**Run Coverage**:
```bash
cargo tarpaulin --lib --tests --out Stdout --exclude-files "tests/*"
```

**Expected Coverage**:
- Detection module: 100%
- Mapping module: 95%+
- Validation module: 90%+
- Integration: 85%+

---

## Common Implementation Patterns

### Pattern 1: Detect Model Generation

```rust
use crate::proxy::mappers::common::gemini_detection::*;

fn handle_model(model: &str) {
    if is_gemini_3_model(model) {
        // Gemini 3.x: Use thinkingLevel
        use_thinking_level_api();
    } else if is_gemini_2_5_model(model) {
        // Gemini 2.5: Use thinkingBudget
        use_thinking_budget_api();
    } else {
        // Other models
        use_default_api();
    }
}
```

---

### Pattern 2: Convert Budget to Level

```rust
use crate::proxy::mappers::common::thinking_level_mapper::determine_thinking_level;

fn convert_thinking_config(model: &str, budget: Option<i32>) -> serde_json::Value {
    if is_gemini_3_model(model) {
        let level = determine_thinking_level(model, budget);
        json!({
            "includeThoughts": true,
            "thinkingLevel": level
        })
    } else {
        json!({
            "includeThoughts": true,
            "thinkingBudget": budget.unwrap_or(16000)
        })
    }
}
```

---

### Pattern 3: Validate Before Sending

```rust
use crate::proxy::mappers::common::gemini_api_validator::validate_gemini_request;

async fn send_to_upstream(model: &str, body: &serde_json::Value) -> Result<Response, Error> {
    // Validate API format
    validate_gemini_request(model, body)?;

    // Send to Google API
    let response = http_client
        .post(&upstream_url)
        .json(body)
        .send()
        .await?;

    Ok(response)
}
```

---

### Pattern 4: Handle Auto-Injection

```rust
use crate::proxy::mappers::common::thinking_level_mapper::get_default_thinking_level;

fn maybe_inject_thinking(model: &str, gen_config: &mut serde_json::Value) {
    if !is_gemini_3_model(model) {
        return; // Only Gemini 3.x has auto-injection
    }

    // Check if thinking already configured
    if gen_config.get("thinkingConfig").is_some() {
        return; // User provided explicit config
    }

    // Auto-inject default level
    let default_level = get_default_thinking_level(model);
    gen_config["thinkingConfig"] = json!({
        "includeThoughts": true,
        "thinkingLevel": default_level
    });
}
```

---

## Debugging

### Enable Debug Logging

```rust
tracing::debug!("Detected Gemini 3 model: {}", model);
tracing::debug!("Converting budget {} to level {}", budget, level);
tracing::debug!("Validating thinking config: {:?}", thinking_config);
```

**Run with Debug Logs**:
```bash
RUST_LOG=debug cargo run
```

---

### Common Issues

**Issue 1**: Wrong API format sent to upstream

**Debug**:
```rust
tracing::info!("Final request body: {}", serde_json::to_string_pretty(&body));
```

**Check**: Verify `thinkingLevel` (not `thinkingBudget`) in request

---

**Issue 2**: Validation fails unexpectedly

**Debug**:
```rust
match validate_gemini_request(model, body) {
    Ok(_) => tracing::debug!("Validation passed"),
    Err(e) => tracing::error!("Validation failed: {}", e),
}
```

**Check**: Model name detection and level values

---

**Issue 3**: Budget not converting to expected level

**Debug**:
```rust
tracing::debug!("Model: {}, Budget: {:?}, Level: {}",
    model, budget, determine_thinking_level(model, budget));
```

**Check**: Budget ranges and model variant detection

---

## Best Practices

### 1. Always Use Centralized Detection
```rust
// ✅ Good
use crate::proxy::mappers::common::gemini_detection::is_gemini_3_model;
if is_gemini_3_model(model) { ... }

// ❌ Bad
if model.contains("gemini-3") { ... }  // Duplicated logic
```

---

### 2. Validate Early
```rust
// ✅ Good
validate_gemini_request(model, body)?;
send_to_upstream(body).await

// ❌ Bad
send_to_upstream(body).await  // May fail at Google API
```

---

### 3. Use Type-Safe Enums
```rust
// ✅ Good
enum ThinkingLevel {
    Minimal,
    Low,
    Medium,
    High,
}

// ❌ Bad
let level = "MEDIUM";  // String literals error-prone
```

---

### 4. Provide Clear Error Messages
```rust
// ✅ Good
return Err(format!(
    "Gemini 3.x model '{}' must use thinkingLevel API. Found thinkingBudget instead.",
    model
));

// ❌ Bad
return Err("Invalid API format".to_string());
```

---

## Future Enhancements

### Planned Features

**1. Dynamic Level Adjustment**:
```rust
// Adjust level based on request complexity
fn auto_adjust_level(model: &str, message_count: usize) -> &'static str {
    match message_count {
        0..=2 => "LOW",
        3..=10 => "MEDIUM",
        _ => "HIGH",
    }
}
```

**2. Cost Optimization**:
```rust
// Track budget usage per session
fn optimize_budget(session_id: &str, remaining_quota: i32) -> &'static str {
    if remaining_quota < 10000 {
        "LOW"  // Conserve quota
    } else {
        "MEDIUM"
    }
}
```

**3. A/B Testing Support**:
```rust
// Test different levels for same request
fn ab_test_thinking_level(user_id: &str) -> &'static str {
    if user_id.ends_with("0") {
        "LOW"
    } else {
        "MEDIUM"
    }
}
```

---

## References

### Internal Documentation
- `docs/technical-specs/GEMINI_API_ANALYSIS.md` - Technical specification
- `docs/comparison/ANTHROPIC_VS_GOOGLE_THINKING.md` - Protocol comparison
- `docs/API_REFERENCE.md` - User-facing API documentation

### Google Documentation
- [Gemini Thinking API](https://ai.google.dev/gemini-api/docs/thinking)
- [Gemini 3 Flash Announcement](https://blog.google/products/gemini/gemini-3-flash/)
- [Vertex AI Thinking](https://docs.cloud.google.com/vertex-ai/generative-ai/docs/thinking)

---

**Document Status**: ✅ Complete
**Last Updated**: 2026-01-11 (Epic-011 Story-006)
**Next Review**: Code implementation changes
