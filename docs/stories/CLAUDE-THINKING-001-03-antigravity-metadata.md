# Story CLAUDE-THINKING-001-03: Add ideType ANTIGRAVITY Metadata

**Story ID**: CLAUDE-THINKING-001-03
**Epic**: [CLAUDE-THINKING-001 - Claude 4.5 Sonnet Thinking Compliance](../epics/claude-4-5-sonnet-thinking-compliance-epic.md)
**Priority**: P0 (CRITICAL) - 🚨 **PRIMARY ANTI-DETECTION MARKER**
**Estimate**: 3 story points (2 hours)
**Status**: To Do
**Assignee**: TBD

---

## User Story

**As a** API Proxy developer
**I want** to add `ideType: "ANTIGRAVITY"` metadata with IDE version, platform, and architecture
**So that** all requests are identified as coming from Antigravity IDE and avoid detection as third-party client

---

## Context

**🚨 CRITICAL**: `ideType: "ANTIGRAVITY"` is the **PRIMARY anti-detection marker** in Google Antigravity v1.13.3.

**Without this field**:
- ❌ Requests identified as non-Antigravity client
- ❌ High probability of blocking or rate limiting
- ❌ May trigger security flags in Google's systems

**With this field**:
- ✅ Requests appear as legitimate Antigravity IDE
- ✅ Proper routing and rate limit policies applied
- ✅ No detection flags

**Current Request** (No metadata):
```json
{
  "request": {
    "contents": [...],
    "safetySettings": [...]
  }
}
```

**Expected Request** (With Antigravity metadata):
```json
{
  "request": {
    "metadata": {
      "ideType": "ANTIGRAVITY",
      "ideVersion": "1.13.3",
      "platform": "darwin",
      "architecture": "arm64"
    },
    "contents": [...],
    "safetySettings": [...]
  }
}
```

**Detection Risk**: **CRITICAL** - Without ideType, requests are easily identifiable as non-Antigravity.

---

## Reference Documents

**Primary**:
1. `docs/comparison/claude/claude-4-5-sonnet/current-implementation-thinking.md`
   - Lines 2875-2996: Gap Analysis #2 (Request Metadata)
   - Lines 2947-2991: Implementation with platform detection

2. `docs/technical-specs/antigravity-workflow-compliance-gap-analysis.md`
   - Lines 167-197: Request Metadata section

**Reverse Engineering Spec**:
- Expected metadata structure from Antigravity v1.13.3
- Platform values: "darwin", "windows", "linux"
- Architecture values: "arm64", "x86_64"

**Quick Reference**:
- `docs/comparison/claude/claude-4-5-sonnet/EXECUTIVE-SUMMARY.md` (lines 32-38)

---

## Technical Details

### Implementation Steps

**File**: `src-tauri/src/proxy/mappers/claude/request.rs`

**Step 1 - Add Metadata Constants**:
```rust
// IDE Metadata constants
const IDE_TYPE: &str = "ANTIGRAVITY";
const IDE_VERSION: &str = "1.13.3";
```

**Step 2 - Add Platform Detection**:
```rust
/// Detect platform using Rust compile-time cfg macros
fn get_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "linux"  // Fallback
    }
}
```

**Step 3 - Add Architecture Detection**:
```rust
/// Detect CPU architecture using Rust compile-time cfg macros
fn get_architecture() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"  // Apple Silicon, ARM64
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"  // Intel, AMD
    } else {
        "x86_64"  // Fallback
    }
}
```

**Step 4 - Build Metadata Object**:
```rust
// Build metadata with Antigravity identity
let mut metadata = json!({
    "ideType": IDE_TYPE,           // "ANTIGRAVITY"
    "ideVersion": IDE_VERSION,     // "1.13.3"
    "platform": get_platform(),    // "darwin"/"windows"/"linux"
    "architecture": get_architecture()  // "arm64"/"x86_64"
});

// Preserve sessionId if provided
if let Some(claude_metadata) = &claude_req.metadata {
    if let Some(user_id) = &claude_metadata.user_id {
        metadata["sessionId"] = json!(user_id);
    }
}

// Add metadata to inner request
inner_request["metadata"] = metadata;
```

---

## Acceptance Criteria

**Given** any Claude request
**When** the request is transformed to v1internal format
**Then** the request.metadata includes `"ideType": "ANTIGRAVITY"`
**And** the request.metadata includes `"ideVersion": "1.13.3"`
**And** the request.metadata includes correct `"platform"` based on OS
**And** the request.metadata includes correct `"architecture"` based on CPU

**Platform Detection**:
- macOS → `"platform": "darwin"`
- Windows → `"platform": "windows"`
- Linux → `"platform": "linux"`

**Architecture Detection**:
- ARM64/Apple Silicon → `"architecture": "arm64"`
- x86_64/Intel → `"architecture": "x86_64"`

---

## Testing Requirements

### Unit Tests

```rust
#[test]
fn test_metadata_includes_ide_type() {
    let req = build_test_claude_request("claude-4.5-sonnet-thinking");
    let body = transform_claude_request_in(&req, "test-project").unwrap();

    assert_eq!(body["request"]["metadata"]["ideType"], "ANTIGRAVITY");
}

#[test]
fn test_metadata_includes_ide_version() {
    let req = build_test_claude_request("claude-4.5-sonnet-thinking");
    let body = transform_claude_request_in(&req, "test-project").unwrap();

    assert_eq!(body["request"]["metadata"]["ideVersion"], "1.13.3");
}

#[test]
fn test_metadata_complete() {
    let req = build_test_claude_request("claude-4.5-sonnet-thinking");
    let body = transform_claude_request_in(&req, "test-project").unwrap();

    let metadata = &body["request"]["metadata"];

    // All four fields must be present
    assert_eq!(metadata["ideType"], "ANTIGRAVITY");
    assert_eq!(metadata["ideVersion"], "1.13.3");
    assert!(metadata.get("platform").is_some());
    assert!(metadata.get("architecture").is_some());
}

#[test]
fn test_metadata_preserves_session_id() {
    let mut req = build_test_claude_request("claude-4.5-sonnet-thinking");
    req.metadata = Some(ClaudeMetadata {
        user_id: Some("test-session-123".to_string()),
    });

    let body = transform_claude_request_in(&req, "test-project").unwrap();

    assert_eq!(body["request"]["metadata"]["sessionId"], "test-session-123");
    assert_eq!(body["request"]["metadata"]["ideType"], "ANTIGRAVITY");
}
```

### Manual Validation

- [ ] Build on macOS → verify platform: "darwin"
- [ ] Build on Windows → verify platform: "windows"
- [ ] Build on Linux → verify platform: "linux"
- [ ] Send request to v1internal API → verify accepted without detection
- [ ] Check API logs for "ANTIGRAVITY" ideType presence

---

## Definition of Done

**Code**:
- [ ] Constants added for IDE_TYPE and IDE_VERSION
- [ ] `get_platform()` function implemented with cfg! macros
- [ ] `get_architecture()` function implemented with cfg! macros
- [ ] Metadata object built with all 4 required fields
- [ ] Metadata added to inner_request["metadata"]
- [ ] SessionId preservation logic implemented
- [ ] Code reviewed and approved

**Testing**:
- [ ] 11 unit tests written and passing
- [ ] Integration test validates complete metadata structure
- [ ] Test coverage ≥80% for new code
- [ ] Manual validation on target OS
- [ ] Cross-platform validation (if possible)

**Compliance**:
- [ ] **Compliance validation**: Metadata structure matches RE spec exactly
- [ ] **Performance benchmarking**: Platform/architecture detection is compile-time (0ms overhead)
- [ ] **Anti-detection validation**: ideType: "ANTIGRAVITY" present in ALL requests
- [ ] **Request format validation**: Metadata object structure validated
- [ ] **Error handling validation**: Missing sessionId handled gracefully

**Documentation**:
- [ ] Code comments explain anti-detection purpose
- [ ] Reference to RE spec line numbers
- [ ] Implementation notes about platform detection
- [ ] Warning about CRITICAL importance of ideType

---

## Dependencies

**Depends On**:
- CLAUDE-THINKING-001-01: Model ID constants
- CLAUDE-THINKING-001-02: Provider constants

**Blocks**:
- CLAUDE-THINKING-001-04: Extended session metadata

**Related**:
- Part of FR2: Request Metadata

---

## Implementation Notes

**Why This Is Critical**:
```
ideType: "ANTIGRAVITY" = Primary client identification marker
```

**Without this**:
- Google's API sees request as "unknown client"
- May trigger detection algorithms
- Higher probability of rate limiting/blocking

**With this**:
- Request appears as legitimate Antigravity IDE
- Normal rate limits and routing applied
- No security flags

**Platform/Architecture Detection**:
- Uses Rust's **compile-time** `cfg!()` macros
- **Zero runtime overhead** - values determined at compile time
- Automatically correct for target platform

**Detection Risk Assessment**:
- **Before Story #3**: Detection risk = **HIGH**
- **After Story #3**: Detection risk = **LOW**

---

## Change Log

| Date | Change | Author |
|------|--------|--------|
| 2026-01-10 | Story created | BMad Master |
