# Story CLAUDE-THINKING-001-04: Add Extended Session Metadata Fields

**Story ID**: CLAUDE-THINKING-001-04
**Epic**: [CLAUDE-THINKING-001 - Claude 4.5 Sonnet Thinking Compliance](../epics/claude-4-5-sonnet-thinking-compliance-epic.md)
**Priority**: P3 (Low) - 📌 **Completes FR2 (Request Metadata)**
**Estimate**: 1 story point (1 hour)
**Status**: To Do
**Assignee**: TBD

---

## User Story

**As a** API Proxy developer
**I want** to add optional workspace_id and cloudaicompanion_project fields to request metadata
**So that** advanced session context is preserved for Google Cloud AI Companion integration and workspace tracking

---

## Context

This story **completes FR2 (Request Metadata)** by adding optional extended metadata fields for Google Cloud AI Companion integration.

**These fields are OPTIONAL** - not all requests need them, only those with workspace/project tracking.

**Current Metadata** (from Story #3):
```json
{
  "metadata": {
    "ideType": "ANTIGRAVITY",
    "ideVersion": "1.13.3",
    "platform": "darwin",
    "architecture": "arm64"
  }
}
```

**Extended Metadata** (this story):
```json
{
  "metadata": {
    "ideType": "ANTIGRAVITY",
    "ideVersion": "1.13.3",
    "platform": "darwin",
    "architecture": "arm64",
    "workspace_id": "workspace-abc",           // Optional
    "cloudaicompanion_project": "project-xyz"  // Optional
  }
}
```

**Impact**: LOW - Nice to have for Cloud AI Companion, not critical for anti-detection.

---

## Reference Documents

**Primary**:
1. `docs/comparison/claude/claude-4-5-sonnet/current-implementation-thinking.md`
   - Lines 3542-3635: Gap Analysis #8 (Session Management)
   - Lines 3589-3630: Implementation example

---

## Technical Details

### Implementation Steps

**File**: `src-tauri/src/proxy/mappers/claude/models.rs`

**Step 1 - Extend Metadata Model**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloudaicompanion_project: Option<String>,
}
```

**File**: `src-tauri/src/proxy/mappers/claude/request.rs`

**Step 2 - Update Metadata Building**:
```rust
// Build base metadata (from Story #3)
let mut metadata = json!({
    "ideType": IDE_TYPE,
    "ideVersion": IDE_VERSION,
    "platform": get_platform(),
    "architecture": get_architecture()
});

// Add optional extended metadata
if let Some(claude_metadata) = &claude_req.metadata {
    // SessionId (from Story #3)
    if let Some(user_id) = &claude_metadata.user_id {
        metadata["sessionId"] = json!(user_id);
    }

    // 🆕 Workspace ID (optional)
    if let Some(workspace_id) = &claude_metadata.workspace_id {
        metadata["workspace_id"] = json!(workspace_id);
    }

    // 🆕 Cloud AI Companion Project (optional)
    if let Some(project) = &claude_metadata.cloudaicompanion_project {
        metadata["cloudaicompanion_project"] = json!(project);
    }
}

inner_request["metadata"] = metadata;
```

---

## Acceptance Criteria

**Given** a Claude request with extended metadata
**When** the request is transformed to v1internal format
**Then** the request.metadata includes workspace_id if provided
**And** the request.metadata includes cloudaicompanion_project if provided

**And** these fields are OPTIONAL (not required)
**And** existing metadata (ideType, ideVersion, platform, architecture) is preserved
**And** sessionId is still preserved if provided

---

## Testing Requirements

### Unit Tests

```rust
#[test]
fn test_metadata_without_extended_fields() {
    let req = build_test_claude_request("claude-4.5-sonnet-thinking");
    let body = transform_claude_request_in(&req, "test-project").unwrap();

    let metadata = &body["request"]["metadata"];

    // Required fields present
    assert_eq!(metadata["ideType"], "ANTIGRAVITY");

    // Extended fields NOT present
    assert!(metadata.get("workspace_id").is_none());
    assert!(metadata.get("cloudaicompanion_project").is_none());
}

#[test]
fn test_metadata_with_all_fields() {
    let mut req = build_test_claude_request("claude-4.5-sonnet-thinking");
    req.metadata = Some(ClaudeMetadata {
        user_id: Some("session-456".to_string()),
        workspace_id: Some("workspace-abc".to_string()),
        cloudaicompanion_project: Some("project-def".to_string()),
    });

    let body = transform_claude_request_in(&req, "test-project").unwrap();
    let metadata = &body["request"]["metadata"];

    // All fields present
    assert_eq!(metadata["sessionId"], "session-456");
    assert_eq!(metadata["workspace_id"], "workspace-abc");
    assert_eq!(metadata["cloudaicompanion_project"], "project-def");
    assert_eq!(metadata["ideType"], "ANTIGRAVITY");
}
```

### Manual Validation

- [ ] Send request WITHOUT extended metadata → verify transformation succeeds
- [ ] Send request WITH workspace_id → verify field included
- [ ] Send request WITH all metadata → verify all preserved

---

## Definition of Done

**Code**:
- [ ] ClaudeMetadata struct extended with workspace_id and cloudaicompanion_project
- [ ] Both fields marked as Option<String>
- [ ] Metadata building logic updated to conditionally add extended fields
- [ ] Required metadata fields still always present
- [ ] Code reviewed and approved

**Testing**:
- [ ] 6 unit tests written and passing
- [ ] Integration test validates complete metadata
- [ ] Test coverage ≥80%
- [ ] Negative test: missing extended fields doesn't break transformation

**Compliance**:
- [ ] **Compliance validation**: Extended metadata structure matches RE spec
- [ ] **Performance benchmarking**: Optional field checking adds <10ms overhead
- [ ] **Anti-detection validation**: Extended fields only present when provided
- [ ] **Request format validation**: Metadata JSON structure validated
- [ ] **Error handling validation**: Missing extended fields handled gracefully

**Documentation**:
- [ ] Code comments explain optional nature
- [ ] Reference to RE spec line numbers
- [ ] Updated ClaudeMetadata struct documentation

---

## Dependencies

**Depends On**:
- CLAUDE-THINKING-001-03: ideType metadata (base structure)

**Completes**:
- 🎯 FR2: Request Metadata (all metadata fields implemented)
- 🎯 FR8: Session Management (extended session context)

**Enables**:
- ✅ P0 Phase completion (Stories #1-4 complete metadata requirements)
- ⏭️ Ready for Story #5 (JWT validation - last P0 story)

---

## Implementation Notes

**Why These Fields Are Optional**:
- Not all Antigravity requests include workspace tracking
- Cloud AI Companion integration is optional feature
- Basic requests only need ideType/ideVersion/platform/architecture

**Field Purpose**:
- **workspace_id**: Links request to specific workspace in Cloud AI Companion
- **cloudaicompanion_project**: Associates request with Cloud AI Companion project

**Priority**: P3 (Low) - Nice to have, not critical for anti-detection

---

## Change Log

| Date | Change | Author |
|------|--------|--------|
| 2026-01-10 | Story created | BMad Master |
