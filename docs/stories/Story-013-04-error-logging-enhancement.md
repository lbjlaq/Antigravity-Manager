# Story-013-04: Error Logging Enhancement

**Epic**: Epic-013 (Gemini 3 Flash Optimization)
**Дата создания**: 2026-01-12
**Статус**: 📋 READY FOR DEVELOPMENT
**Приоритет**: P2 (Medium-High)
**Оценка**: 1-2 дня

---

## 🎯 User Story

**As a** DevOps Engineer
**I want** structured error logging for thinking level validation and content filtering
**So that** I can debug issues faster, monitor system health, and analyze failure patterns

**Context**: Current error handling works but lacks structured logging. When level validation fails or content is filtered, we need detailed context for troubleshooting and analytics.

---

## 📋 Background

### Current State

**Error Handling** (Functional but Minimal Logging):
```rust
// Current: Basic error propagation, no structured logging
if let Err(e) = validate_thinking_level(&level, &model) {
    return Err(format!("Invalid level: {}", e));  // ← Minimal context
}
```

**Gaps**:
- ❌ No structured logging (JSON format)
- ❌ Missing context (model, budget, level, user_id)
- ❌ No error categorization (validation vs filter vs API)
- ❌ No metrics integration

### Gap Reference

**From**: `gemini-3-flash-COMPARISON.md:320-350` (Gap 4: OPT-001)

```yaml
optimization_gap:
  category: "Quality Monitoring"
  description: "Structured logging for debugging and analytics"

  current_logging:
    format: "String messages"
    context: "Minimal"
    searchability: "Low"

  desired_logging:
    format: "Structured JSON"
    context: "Rich (model, level, budget, error_type)"
    searchability: "High"
    integration: "Prometheus, ELK, Grafana"
```

---

## 🔧 Technical Details

### Error Categories to Log

#### 1. Thinking Level Validation Errors

**Scenarios**:
- MEDIUM level requested for Pro (should downgrade to LOW)
- Invalid budget range
- Budget-to-level mapping failures

**Log Structure**:
```json
{
  "timestamp": "2026-01-12T10:30:45Z",
  "level": "WARN",
  "category": "thinking_validation",
  "error_type": "medium_downgrade",
  "model": "gemini-3-pro-high",
  "requested_level": "MEDIUM",
  "actual_level": "LOW",
  "reason": "Pro doesn't support MEDIUM, downgraded to LOW",
  "budget_tokens": 15000,
  "request_id": "req_abc123",
  "user_id": "user_xyz789"
}
```

#### 2. Content Filter Errors

**Scenarios**:
- Content blocked by safety filters
- Which harm category triggered
- User can rephrase prompt

**Log Structure**:
```json
{
  "timestamp": "2026-01-12T10:31:20Z",
  "level": "INFO",
  "category": "content_filter",
  "error_type": "safety_block",
  "model": "gemini-3-flash",
  "finish_reason": "content_filter",
  "blocked_category": "HARM_CATEGORY_DANGEROUS_CONTENT",
  "safety_rating": "HIGH",
  "action": "response_blocked",
  "request_id": "req_def456",
  "user_id": "user_xyz789",
  "prompt_preview": "First 50 chars..."  // For debugging, not full prompt
}
```

#### 3. API Errors

**Scenarios**:
- Invalid thinkingLevel value sent to Google
- API rejects request
- Upstream errors

**Log Structure**:
```json
{
  "timestamp": "2026-01-12T10:32:10Z",
  "level": "ERROR",
  "category": "api_error",
  "error_type": "invalid_parameter",
  "model": "gemini-3-flash",
  "parameter": "thinkingConfig.thinkingLevel",
  "invalid_value": "VERY_HIGH",  // Hypothetical invalid value
  "valid_values": ["MINIMAL", "LOW", "MEDIUM", "HIGH"],
  "google_error_code": "INVALID_ARGUMENT",
  "google_error_message": "thinkingLevel must be one of...",
  "request_id": "req_ghi789",
  "user_id": "user_xyz789"
}
```

### Implementation Locations

**File 1**: `src-tauri/src/proxy/mappers/claude/request.rs`

```rust
// Around line 1450-1475 (budget-to-level mapping)

// BEFORE (no logging):
let thinking_level = match budget_tokens {
    Some(b) if b <= 4000 => "MINIMAL",
    Some(b) if b <= 10000 => "LOW",
    // ...
};

// AFTER (with structured logging):
use tracing::{info, warn};

let thinking_level = match budget_tokens {
    Some(b) if b <= 4000 => {
        info!(
            category = "thinking_mapping",
            model = %mapped_model,
            budget = b,
            level = "MINIMAL",
            "Budget mapped to MINIMAL level"
        );
        "MINIMAL"
    }
    // Handle MEDIUM level downgrade for Pro
    Some(b) if b > 10000 && b <= 20000 && !is_flash => {
        warn!(
            category = "thinking_validation",
            error_type = "medium_downgrade",
            model = %mapped_model,
            requested_level = "MEDIUM",
            actual_level = "LOW",
            budget = b,
            "Pro doesn't support MEDIUM, downgrading to LOW"
        );
        "LOW"  // Downgrade
    }
    // ...
};
```

**File 2**: `src-tauri/src/proxy/mappers/openai/streaming.rs`

```rust
// Content filter detection in streaming response

if finish_reason == "content_filter" {
    warn!(
        category = "content_filter",
        error_type = "safety_block",
        model = %model_name,
        finish_reason = "content_filter",
        request_id = %request_id,
        "Content blocked by safety filters"
    );
}
```

**File 3**: Error response handlers (various locations)

```rust
// Google API error handling

if let Err(api_error) = upstream_response {
    error!(
        category = "api_error",
        error_type = "google_api_failure",
        model = %model_name,
        status_code = api_error.status,
        error_message = %api_error.message,
        request_id = %request_id,
        "Google API request failed"
    );
}
```

---

## ✅ Acceptance Criteria

### AC-1: Thinking Level Validation Logging

```gherkin
GIVEN a Claude request with budget_tokens = 15000 for gemini-3-pro-high
WHEN the request is mapped to Gemini format
THEN a WARN log is emitted with:
  - category: "thinking_validation"
  - error_type: "medium_downgrade"
  - requested_level: "MEDIUM"
  - actual_level: "LOW"
  - model: "gemini-3-pro-high"
  - budget: 15000
```

**Verification**:
- ✅ Log appears in application logs
- ✅ JSON format (structured)
- ✅ Contains all required fields
- ✅ Timestamp is accurate

---

### AC-2: Content Filter Logging

```gherkin
GIVEN a Gemini response with finish_reason = "content_filter"
WHEN the response is processed
THEN an INFO log is emitted with:
  - category: "content_filter"
  - error_type: "safety_block"
  - model: model name
  - finish_reason: "content_filter"
  - request_id: unique identifier
```

**Verification**:
- ✅ Log triggered on content_filter
- ✅ Safety ratings included (if available)
- ✅ NO sensitive content logged (privacy)
- ✅ Actionable information provided

---

### AC-3: API Error Logging

```gherkin
GIVEN a Google API error response
WHEN the error is caught
THEN an ERROR log is emitted with:
  - category: "api_error"
  - error_type: categorized error
  - google_error_code: from API
  - google_error_message: from API
  - request_id: unique identifier
```

**Verification**:
- ✅ All API errors logged
- ✅ Error details preserved
- ✅ Context included (model, parameters)
- ✅ Severity appropriate (ERROR level)

---

### AC-4: Log Format and Structure

```gherkin
GIVEN any error log entry
WHEN examining the log output
THEN it conforms to structured logging format
```

**Verification**:
- ✅ Valid JSON (parseable)
- ✅ Consistent field names across log types
- ✅ Timestamp in ISO 8601 format
- ✅ Log level appropriate (INFO/WARN/ERROR)

---

### AC-5: Performance Impact

```gherkin
GIVEN logging is enabled
WHEN running performance benchmarks
THEN logging overhead is <5% of request latency
```

**Verification**:
- ✅ Logging is async (non-blocking)
- ✅ No significant latency increase
- ✅ No memory leaks from logging
- ✅ Log rotation configured (prevent disk fill)

---

## 🔍 Implementation Guide

### Step 1: Add `tracing` Dependency (if not present)

**File**: `src-tauri/Cargo.toml`

```toml
[dependencies]
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
```

### Step 2: Initialize Structured Logging

**File**: `src-tauri/src/modules/logger.rs` (or similar)

```rust
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_logging() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .json()  // JSON format for structured logs
                .with_target(true)
                .with_level(true)
        )
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();
}
```

### Step 3: Add Logging to Level Validation

**File**: `src-tauri/src/proxy/mappers/claude/request.rs`

**Location**: Budget-to-level mapping logic (~line 1450-1475)

```rust
use tracing::{info, warn, error};

// Inside the budget-to-level mapping function
fn map_budget_to_level(budget: Option<u32>, model: &str, is_flash: bool) -> &'static str {
    match budget {
        Some(b) if b <= 4000 => {
            info!(
                category = "thinking_mapping",
                model = %model,
                budget = b,
                level = "MINIMAL",
                "Budget mapped to MINIMAL"
            );
            "MINIMAL"
        }

        Some(b) if b <= 10000 => {
            info!(
                category = "thinking_mapping",
                model = %model,
                budget = b,
                level = "LOW",
                "Budget mapped to LOW"
            );
            "LOW"
        }

        Some(b) if b > 10000 && b <= 20000 => {
            if is_flash {
                info!(
                    category = "thinking_mapping",
                    model = %model,
                    budget = b,
                    level = "MEDIUM",
                    "Budget mapped to MEDIUM (Flash exclusive)"
                );
                "MEDIUM"
            } else {
                warn!(
                    category = "thinking_validation",
                    error_type = "medium_downgrade",
                    model = %model,
                    requested_level = "MEDIUM",
                    actual_level = "LOW",
                    budget = b,
                    reason = "Pro doesn't support MEDIUM level",
                    "Downgrading MEDIUM to LOW for Pro model"
                );
                "LOW"
            }
        }

        Some(b) if b > 20000 => {
            info!(
                category = "thinking_mapping",
                model = %model,
                budget = b,
                level = "HIGH",
                "Budget mapped to HIGH"
            );
            "HIGH"
        }

        None => {
            info!(
                category = "thinking_mapping",
                model = %model,
                budget = "default",
                level = "HIGH",
                "No budget specified, using default HIGH"
            );
            "HIGH"
        }
    }
}
```

### Step 4: Add Logging to Content Filter Handling

**File**: `src-tauri/src/proxy/mappers/openai/streaming.rs`

**Location**: Response processing, finish_reason detection

```rust
use tracing::{info, warn};

// When processing streaming chunk
if let Some(finish_reason) = chunk.get("finish_reason").and_then(|v| v.as_str()) {
    if finish_reason == "content_filter" {
        warn!(
            category = "content_filter",
            error_type = "safety_block",
            model = %model_name,
            finish_reason = "content_filter",
            request_id = %request_id,
            "Response blocked by safety filters"
        );

        // Extract safety ratings if available
        if let Some(safety_ratings) = chunk.get("safety_ratings") {
            info!(
                category = "content_filter",
                safety_ratings = ?safety_ratings,
                "Safety ratings for blocked content"
            );
        }
    }
}
```

### Step 5: Add Logging to API Error Handling

**File**: Multiple error handling locations

```rust
use tracing::error;

// Example: Google API error
if let Err(api_error) = google_response {
    error!(
        category = "api_error",
        error_type = "google_api_failure",
        model = %model_name,
        status_code = api_error.status_code,
        error_code = %api_error.code,
        error_message = %api_error.message,
        request_id = %request_id,
        "Google API request failed"
    );

    // Return error to client
    return Err(api_error);
}
```

### Step 6: Test Logging

**Manual Testing**:

```bash
# 1. Enable debug logs
export RUST_LOG=info

# 2. Start application
npm run tauri dev

# 3. Trigger scenarios
# - Send request with budget 15000 to Pro (expect MEDIUM downgrade log)
# - Send request with unsafe content (expect content_filter log)
# - Send invalid request (expect API error log)

# 4. Check logs
# Look for JSON-formatted log entries
```

**Automated Testing**:

```rust
#[test]
fn test_medium_downgrade_logging() {
    // Capture logs during test
    let logs = capture_logs(|| {
        let request = create_request_with_budget(15000);
        map_to_gemini("gemini-3-pro-high", request);
    });

    // Verify WARN log was emitted
    assert!(logs.iter().any(|log| {
        log.level == "WARN" &&
        log.category == "thinking_validation" &&
        log.error_type == "medium_downgrade"
    }));
}
```

---

## 📊 Quality Gates

### QG-1: Log Format Validation

```bash
# Verify logs are valid JSON
tail -f logs/app.log | jq '.'
```

**Expected**:
- ✅ All log entries are valid JSON
- ✅ Consistent field names
- ✅ Proper data types

---

### QG-2: Coverage of Error Scenarios

**Test Matrix**:
| Scenario | Log Expected | Verified |
|----------|--------------|----------|
| MEDIUM on Pro | WARN (downgrade) | ✅ |
| Content filtered | INFO (safety_block) | ✅ |
| API error | ERROR (api_failure) | ✅ |
| Invalid budget | WARN (validation) | ✅ |
| Normal operation | INFO (mapping) | ✅ |

---

### QG-3: Performance Impact

```bash
# Benchmark with/without logging
cargo bench --bench thinking_performance
```

**Expected**:
- ✅ Logging overhead <5% of total latency
- ✅ No blocking operations
- ✅ Memory usage stable

---

### QG-4: Privacy Compliance

**Check**:
- ✅ NO full prompts logged (only previews if needed)
- ✅ User IDs hashed or anonymized
- ✅ Sensitive data redacted
- ✅ GDPR compliance maintained

---

## 🎯 Success Metrics

```yaml
logging_coverage:
  before: "Minimal error messages"
  after: "Structured logs for all error categories"
  improvement: "100% error scenarios logged"

debugging_efficiency:
  before: "Hours to debug issues (manual log search)"
  after: "Minutes (structured query)"
  improvement: "10x faster troubleshooting"

analytics_capability:
  before: "No error analytics"
  after: "Queryable error database"
  new_insights:
    - "MEDIUM downgrade frequency (Pro usage)"
    - "Content filter trigger rates by category"
    - "API error patterns"
```

---

## 🔗 Related Work

**Dependencies**:
- ✅ Epic-011 Complete (thinkingLevel API implemented)
- ✅ Story-013-01 (MEDIUM level tests provide scenarios to log)

**Follow-up Stories**:
- Story-013-06: Cost Analytics (uses logs for analysis)
- Future: ELK/Grafana integration (visualize logs)

**References**:
- `claude/request.rs:1450-1475` (level validation logic)
- `openai/streaming.rs` (content filter detection)
- `tracing` crate documentation

---

## 📝 Notes

### Why Structured Logging Matters

```yaml
benefits:
  debugging:
    - "Quickly find all MEDIUM downgrade cases"
    - "Identify which safety categories block most often"
    - "Trace request flow by request_id"

  analytics:
    - "Aggregate error rates by model"
    - "Monitor level distribution (input to Cost Analytics)"
    - "Detect anomalies and patterns"

  operations:
    - "Alert on error rate spikes"
    - "SLA monitoring (error budgets)"
    - "Incident response (root cause analysis)"
```

### Future Enhancements (Not in This Story)

- **Metrics Export**: Prometheus metrics from logs
- **Dashboards**: Grafana visualizations
- **Alerts**: PagerDuty integration for critical errors
- **Log Aggregation**: ELK stack or Splunk

---

**Story Owner**: Backend Developer + DevOps
**Reviewers**: Tech Lead, SRE
**Estimated Effort**: 1-2 days (8-16 hours)
**Actual Effort**: _TBD after completion_

**Status**: 📋 READY FOR DEVELOPMENT
**Next Step**: Assign to developer, start implementation
