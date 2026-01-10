# Story-003-08 ADDENDUM: Mapper → Monitor Access Pattern (Gap #10)

**Addendum ID**: Story-003-08-ADDENDUM
**Parent Story**: [Story-003-08](Story-003-08-enhanced-violation-metrics.md) - Enhanced Violation Metrics
**Issue**: Critical Gap #10 - Mapper → Monitor Access Pattern not described
**Priority**: P1 (CRITICAL - Blocks Story-003-08 implementation)
**Estimated Effort**: +0.5 hours (additional to Story-003-08's 2h)
**Created**: 2026-01-10
**Owner**: Engineering Team

---

## Problem Statement

### Critical Gap Identified

**Story-003-08** defines integration points with Stories #6 and #7 via TODO comments:

```rust
// Story-003-06 TODO at request.rs:~1182 (Budget Constraint)
if max_tokens <= clamped_budget {
    tracing::warn!(...);
    // TODO (Story #8): Increment metrics counter
    // metrics::increment_counter!("thinking_budget_violations");
}

// Story-003-07 TODO at request.rs:~751 (Position Enforcement)
if !parts.is_empty() {
    tracing::error!(...);
    // TODO (Story #8): Increment metrics counters
    // metrics::increment_counter!("thinking_position_violations");
}
```

**However**, Story-003-08 does NOT describe **HOW** these TODOs access `ProxyMonitor` from mapper functions.

### Architecture Analysis

**Current Access Pattern**:
```rust
// server.rs - AppState definition
pub struct AppState {
    pub monitor: Arc<ProxyMonitor>,  // Monitor stored in AppState
    // ...
}

// handlers/claude.rs - Handler has access
pub async fn handle_messages(
    State(state): State<AppState>,  // ✅ Handler receives state
    // ...
) -> Response {
    // state.monitor is accessible here ✅
}

// mappers/claude/request.rs - Mapper has NO access
pub fn map_claude_to_vertex_ai(
    claude_req: &ClaudeRequest,
    config: &ProxyConfig,
    trace_id: &str,
) -> serde_json::Value {
    // ❌ NO access to AppState or monitor here!
    // How to call monitor.record_budget_violation()?
}
```

**Problem**: Mapper functions are pure transformation functions with NO access to AppState.

---

## Solution: Context via Handler (Option D)

### Design Decision

**Selected Approach**: **Option D - Context via Handler**

**Rationale**:
- ✅ **Minimal changes**: Mapper remains pure function
- ✅ **Clear separation**: Handler orchestrates, mapper transforms
- ✅ **Testable**: Mapper can be unit tested without monitor
- ✅ **Flexible**: Handler can choose when/how to update monitor
- ✅ **Thread-safe**: No global state, explicit ownership

**Rejected Options**:
- ❌ **Option A (Global static)**: Requires lazy_static, harder to test, implicit dependency
- ❌ **Option B (Pass monitor)**: Clutters mapper signature, violates separation of concerns
- ❌ **Option C (Return violations)**: Complex return type, double processing

### Architecture Overview

```
┌────────────────────────────────────────────────────────────┐
│ Handler (handlers/claude.rs)                               │
│                                                            │
│ pub async fn handle_messages(                             │
│     State(state): State<AppState>,  // Has monitor access │
│     ...                                                    │
│ ) -> Response {                                           │
│     // 1. Call mapper                                     │
│     let inner_request = map_claude_to_vertex_ai(...);     │
│                                                            │
│     // 2. Extract violation flags from inner_request      │
│     let had_budget_violation =                            │
│         inner_request["__meta"]["budget_violation"]       │
│             .as_bool().unwrap_or(false);                  │
│                                                            │
│     // 3. Update monitor if violations occurred           │
│     if had_budget_violation {                             │
│         let monitor = state.monitor.clone();              │
│         tokio::spawn(async move {                         │
│             monitor.record_budget_violation().await;      │
│         });                                               │
│     }                                                      │
│                                                            │
│     // 4. Remove __meta before upstream send             │
│     inner_request.as_object_mut()                         │
│         .unwrap().remove("__meta");                       │
│                                                            │
│     // 5. Send request to upstream                        │
│     // ...                                                 │
│ }                                                          │
└────────────────────────────────────────────────────────────┘
                         │
                         ▼
┌────────────────────────────────────────────────────────────┐
│ Mapper (mappers/claude/request.rs)                        │
│                                                            │
│ pub fn map_claude_to_vertex_ai(                           │
│     claude_req: &ClaudeRequest,                           │
│     config: &ProxyConfig,                                 │
│     trace_id: &str,                                       │
│ ) -> serde_json::Value {                                  │
│     let mut inner_request = json!({ /* ... */ });        │
│                                                            │
│     // Budget constraint check                            │
│     if max_tokens <= clamped_budget {                     │
│         tracing::warn!("[Thinking-Budget] ⚠️ ...");       │
│                                                            │
│         // 🆕 Set violation flag in __meta               │
│         inner_request["__meta"]["budget_violation"] =    │
│             json!(true);                                  │
│                                                            │
│         clamped_budget + 100                              │
│     }                                                      │
│                                                            │
│     // Position enforcement check                         │
│     if !parts.is_empty() {                                │
│         tracing::error!("[Thinking-Position] ❌ ...");    │
│                                                            │
│         // 🆕 Set violation flags in __meta              │
│         inner_request["__meta"]["position_violation"] =  │
│             json!(true);                                  │
│         inner_request["__meta"]["position_index"] =      │
│             json!(parts.len());                           │
│         inner_request["__meta"]["position_role"] =       │
│             json!(role);                                  │
│     }                                                      │
│                                                            │
│     inner_request                                         │
│ }                                                          │
└────────────────────────────────────────────────────────────┘
```

---

## Technical Implementation

### Step 1: Define __meta Structure in Mapper

**Location**: `mappers/claude/request.rs` (within `map_claude_to_vertex_ai`)

**Add __meta Object**:
```rust
// At the beginning of map_claude_to_vertex_ai function
let mut inner_request = json!({
    "contents": contents,
    "safetySettings": safety_settings,
    // ... existing fields

    // 🆕 Metadata for violation tracking (removed before upstream send)
    "__meta": {
        "budget_violation": false,
        "position_violation": false,
        "position_index": null,
        "position_role": null
    }
});
```

**Update Budget Violation Check** (request.rs:~1182):
```rust
if max_tokens <= clamped_budget {
    tracing::warn!(
        "[Thinking-Budget] ⚠️ Constraint violation: maxOutputTokens ({}) <= thinkingBudget ({}). \
         Auto-fixing to {} to maintain compatibility. \
         Client should fix configuration to prevent this warning.",
        max_tokens, clamped_budget, clamped_budget + 100
    );

    // 🆕 Set violation flag for handler
    if let Some(meta) = inner_request.get_mut("__meta") {
        meta["budget_violation"] = json!(true);
    }

    clamped_budget + 100
}
```

**Update Position Violation Check** (request.rs:~751):
```rust
if !parts.is_empty() {
    tracing::error!(
        "[Thinking-Position] ❌ PROTOCOL VIOLATION: Thinking block at index {} (must be first). \
         Message role: '{}', total parts before: {}, thinking length: {}. \
         Downgrading to text block to maintain compatibility. \
         Client MUST fix message structure to prevent this error.",
        parts.len(), role, parts.len(), thinking.len()
    );

    // 🆕 Set violation flags for handler
    if let Some(meta) = inner_request.get_mut("__meta") {
        meta["position_violation"] = json!(true);
        meta["position_index"] = json!(parts.len());
        meta["position_role"] = json!(role);
    }

    if !thinking.is_empty() {
        parts.push(json!({"text": thinking}));
    }
    continue;
}
```

### Step 2: Extract Violations in Handler

**Location**: `handlers/claude.rs` (within `handle_messages`)

**Add Helper Function**:
```rust
/// Extract violation metadata from mapped request
fn extract_violations(request: &serde_json::Value) -> ViolationFlags {
    ViolationFlags {
        budget_violation: request["__meta"]["budget_violation"]
            .as_bool()
            .unwrap_or(false),
        position_violation: request["__meta"]["position_violation"]
            .as_bool()
            .unwrap_or(false),
        position_index: request["__meta"]["position_index"]
            .as_u64()
            .map(|v| v as usize),
        position_role: request["__meta"]["position_role"]
            .as_str()
            .map(|s| s.to_string()),
    }
}

#[derive(Debug)]
struct ViolationFlags {
    budget_violation: bool,
    position_violation: bool,
    position_index: Option<usize>,
    position_role: Option<String>,
}
```

**Update Handler After Mapping**:
```rust
pub async fn handle_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    // ... existing code ...

    // Map Claude request to Gemini format
    let mut inner_request = map_claude_to_vertex_ai(&claude_req, &config, &trace_id);

    // 🆕 Extract violation metadata
    let violations = extract_violations(&inner_request);

    // 🆕 Remove __meta before sending to upstream
    if let Some(obj) = inner_request.as_object_mut() {
        obj.remove("__meta");
    }

    // 🆕 Update monitor with violations (non-blocking)
    if violations.budget_violation || violations.position_violation {
        let monitor = state.monitor.clone();
        let v = violations; // Move into spawn

        tokio::spawn(async move {
            if v.budget_violation {
                monitor.record_budget_violation().await;
            }

            if v.position_violation {
                if let (Some(index), Some(role)) = (v.position_index, v.position_role) {
                    monitor.record_position_violation(index, &role).await;
                }
            }
        });
    }

    // Continue with upstream request...
    // ...
}
```

### Step 3: Clean Up __meta

**Why Remove __meta?**
- ✅ **Not part of protocol**: Gemini Protocol does not expect __meta field
- ✅ **Internal only**: Used only for handler ↔ mapper communication
- ✅ **Clean request**: Upstream receives standard-compliant request

**Implementation**:
```rust
// After extracting violations, before upstream send
if let Some(obj) = inner_request.as_object_mut() {
    obj.remove("__meta");
}
```

---

## Alternative Implementation: Return Tuple

**If __meta approach is not preferred**, use explicit return:

```rust
// Mapper returns violations explicitly
pub fn map_claude_to_vertex_ai(
    claude_req: &ClaudeRequest,
    config: &ProxyConfig,
    trace_id: &str,
) -> (serde_json::Value, ViolationFlags) {
    let mut inner_request = json!({ /* ... */ });
    let mut violations = ViolationFlags::default();

    // Budget check
    if max_tokens <= clamped_budget {
        tracing::warn!(...);
        violations.budget_violation = true;
        clamped_budget + 100
    }

    // Position check
    if !parts.is_empty() {
        tracing::error!(...);
        violations.position_violation = true;
        violations.position_index = Some(parts.len());
        violations.position_role = Some(role.to_string());
        // ...
    }

    (inner_request, violations)
}

// Handler receives both
let (mut inner_request, violations) = map_claude_to_vertex_ai(&claude_req, &config, &trace_id);
```

**Trade-offs**:
- ✅ **Explicit**: Clear API, violations are return value
- ✅ **Type-safe**: Compiler enforces handling
- ❌ **Breaking change**: All callers must update
- ❌ **Signature clutter**: Makes function more complex

**Recommendation**: Use `__meta` approach for backward compatibility.

---

## Testing Strategy

### Unit Tests for __meta

**File**: `mappers/claude/request_tests.rs`

```rust
#[test]
fn test_budget_violation_sets_meta_flag() {
    let mut claude_req = create_test_request();
    claude_req.max_tokens = Some(32000);  // Equal to budget (violation)
    claude_req.thinking = Some(ThinkingConfig {
        type_: "enabled".to_string(),
        budget_tokens: Some(32000),
    });

    let result = map_claude_to_vertex_ai(&claude_req, &create_test_config(), "test");

    // Verify __meta flag set
    assert_eq!(result["__meta"]["budget_violation"], true);
}

#[test]
fn test_position_violation_sets_meta_flags() {
    let mut claude_req = create_test_request();
    claude_req.messages = vec![
        Message {
            role: "user".to_string(),
            content: MessageContent::Array(vec![
                ContentBlock::Text { text: "Before thinking".to_string() },
                ContentBlock::Thinking {
                    thinking: "Think".to_string(),
                    signature: Some("sig".to_string()),
                    cache_control: None,
                },
            ]),
        }
    ];

    let result = map_claude_to_vertex_ai(&claude_req, &create_test_config(), "test");

    // Verify __meta flags set
    assert_eq!(result["__meta"]["position_violation"], true);
    assert_eq!(result["__meta"]["position_index"], 1);
    assert_eq!(result["__meta"]["position_role"], "user");
}

#[test]
fn test_no_violations_meta_flags_false() {
    let claude_req = create_valid_thinking_request();

    let result = map_claude_to_vertex_ai(&claude_req, &create_test_config(), "test");

    // Verify __meta flags are false
    assert_eq!(result["__meta"]["budget_violation"], false);
    assert_eq!(result["__meta"]["position_violation"], false);
}
```

### Integration Tests for Handler

**File**: `handlers/claude_tests.rs`

```rust
#[tokio::test]
async fn test_handler_extracts_and_records_budget_violation() {
    let (state, monitor) = create_test_state_with_monitor();

    // Create request with budget violation
    let request = json!({
        "model": "claude-4.5-sonnet-thinking",
        "messages": [{"role": "user", "content": "Test"}],
        "max_tokens": 32000,
        "thinking": {"type": "enabled", "budget_tokens": 32000}
    });

    let response = handle_messages(
        State(state),
        HeaderMap::new(),
        Json(request)
    ).await;

    // Verify response successful
    assert!(response.status().is_success());

    // Verify monitor recorded violation
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;  // Wait for spawn
    let stats = monitor.get_stats().await;
    assert_eq!(stats.thinking_budget_violations, 1);
}

#[tokio::test]
async fn test_handler_removes_meta_before_upstream() {
    // Use request capture to verify __meta not sent to upstream
    let captured_request = capture_upstream_request(|| async {
        // Send request with violation
        // ...
    }).await;

    // Verify __meta removed
    assert!(captured_request.get("__meta").is_none());
}
```

---

## Acceptance Criteria

### AC-1: __meta Structure Added ✅

**Given** the map_claude_to_vertex_ai function
**When** I call it with any request
**Then** the result should:
- ✅ Include `__meta` object
- ✅ Have `budget_violation` field (boolean)
- ✅ Have `position_violation` field (boolean)
- ✅ Have `position_index` field (number or null)
- ✅ Have `position_role` field (string or null)

### AC-2: Budget Violation Flag Set ✅

**Given** a request with budget constraint violation
**When** mapper processes it
**Then** it should:
- ✅ Set `__meta.budget_violation` to true
- ✅ Log warning message
- ✅ Auto-fix the constraint

### AC-3: Position Violation Flags Set ✅

**Given** a request with position enforcement violation
**When** mapper processes it
**Then** it should:
- ✅ Set `__meta.position_violation` to true
- ✅ Set `__meta.position_index` to violation index
- ✅ Set `__meta.position_role` to message role
- ✅ Log error message

### AC-4: Handler Extracts Violations ✅

**Given** mapped request with violations
**When** handler processes it
**Then** it should:
- ✅ Extract violation flags from `__meta`
- ✅ Create ViolationFlags struct
- ✅ Handle missing __meta gracefully

### AC-5: Monitor Updated Non-Blocking ✅

**Given** extracted violations
**When** handler updates monitor
**Then** it should:
- ✅ Use `tokio::spawn` for non-blocking update
- ✅ Call `monitor.record_budget_violation()` if budget violation
- ✅ Call `monitor.record_position_violation(index, role)` if position violation
- ✅ Not block request processing

### AC-6: __meta Removed Before Upstream ✅

**Given** mapped request with __meta
**When** handler sends to upstream
**Then** it should:
- ✅ Remove __meta object from request
- ✅ Send clean request to upstream
- ✅ Upstream never sees __meta field

### AC-7: Backward Compatibility ✅

**Given** requests without violations
**When** processed through handler
**Then** it should:
- ✅ Work exactly as before
- ✅ No performance impact
- ✅ No monitor updates triggered

### AC-8: Tests Pass ✅

**Given** new unit and integration tests
**When** I run test suite
**Then** all tests should:
- ✅ Unit tests pass (3 tests)
- ✅ Integration tests pass (2 tests)
- ✅ No regressions

---

## Definition of Done

### Code Changes
- [ ] ✅ __meta structure added to mapper
- [ ] ✅ Budget violation sets __meta.budget_violation
- [ ] ✅ Position violation sets __meta flags
- [ ] ✅ Handler extracts violations
- [ ] ✅ Handler updates monitor (non-blocking)
- [ ] ✅ Handler removes __meta before upstream

### Testing
- [ ] ✅ 3 unit tests for __meta (mapper)
- [ ] ✅ 2 integration tests for handler
- [ ] ✅ All tests passing
- [ ] ✅ No regressions

### Documentation
- [ ] ✅ Story-003-08 updated with reference to addendum
- [ ] ✅ Architecture diagram showing data flow
- [ ] ✅ Code comments explaining __meta usage

### Validation
- [ ] ✅ Gap #10 closed
- [ ] ✅ Story-003-08 implementation unblocked
- [ ] ✅ Integration with Stories #6, #7 complete
- [ ] ✅ Monitor correctly receives violations

---

## Impact on Story-003-08

### Updated Effort Estimate

**Original Story-003-08**: 2 hours
**Addendum (Gap #10)**: +0.5 hours
**Total**: **2.5 hours**

### Updated Implementation Steps

Story-003-08 implementation now follows this sequence:

1. **Step 1**: Extend ProxyStats (as planned)
2. **Step 2**: Create ViolationMetrics (as planned)
3. **Step 3**: Integrate with ProxyMonitor (as planned)
4. **Step 4** (🆕): Add __meta structure to mapper
5. **Step 5** (🆕): Update budget violation to set __meta flag
6. **Step 6** (🆕): Update position violation to set __meta flags
7. **Step 7** (🆕): Add violation extraction in handler
8. **Step 8** (🆕): Update monitor from handler (non-blocking)
9. **Step 9** (🆕): Remove __meta before upstream send
10. **Step 10**: Add Tauri command (as planned)
11. **Step 11**: Database migration (as planned)

---

## References

### Parent Story
- **Story-003-08**: `docs/stories/Story-003-08-enhanced-violation-metrics.md`
  - Lines 660-740: Integration points (Step 4, Step 5)
  - Lines 260-320: ProxyMonitor structure

### Current Architecture
- **AppState**: `src-tauri/src/proxy/server.rs:18-33`
- **Handler**: `src-tauri/src/proxy/handlers/claude.rs:375-377`
- **Mapper**: `src-tauri/src/proxy/mappers/claude/request.rs`
  - Line ~1182: Budget constraint violation
  - Line ~751: Position enforcement violation

### Similar Patterns
- **Middleware**: `src-tauri/src/proxy/middleware/monitor.rs:97`
  - Shows how to access `state.monitor`
  - Example of non-blocking tokio::spawn

---

## Change Log

| Date | Change | Author |
|------|--------|--------|
| 2026-01-10 | Addendum created to address Gap #10 | BMad Master |
