# Epic: Claude 4.5 Sonnet (Standard) - 100% Compliance

**Epic ID**: Epic-004
**Status**: Planning
**Priority**: P0 (Critical)
**Target Release**: v3.4.0
**Created**: 2026-01-11
**Owner**: Engineering Team
**Related Epic**: [Epic-003](Epic-003-Claude-4.5-Sonnet-Thinking-Compliance.md) (claude-4.5-sonnet-thinking)

---

## Epic Overview

### Problem Statement

Current API Proxy implementation for **claude-4.5-sonnet** (standard, no extended thinking) has **~75-80% compliance** with Google Antigravity v1.13.3, resulting in:

- ❌ **Detection Risk**: Missing primary anti-detection markers (ideType: ANTIGRAVITY)
- ❌ **Routing Failures**: Missing model provider information (modelId: 333, apiProvider: 26)
- ❌ **Feature Gaps**: Limited tool configuration modes, missing grounding settings

**Key Difference from Epic-003**: No thinking-specific features (no signature validation, no budget constraints, no position enforcement)

### Solution Vision

Achieve **100% compliance** with Google Antigravity v1.13.3 through:

- ✅ **Anti-Detection Compliance**: Add ideType: "ANTIGRAVITY" metadata to all requests
- ✅ **Provider Routing**: Include model provider information (modelId: 333, apiProvider: 26, modelProvider: 3)
- ✅ **Feature Parity**: Support all tool modes and grounding configuration
- ✅ **Extended Context**: Add workspace and project metadata for Cloud AI Companion

**Implementation Strategy**: 90% shared code with Epic-003, model-specific constants only

### Success Metrics

| Metric | Baseline | Target | Method |
|--------|----------|--------|--------|
| **Compliance Score** | 75-80% | 100% | Gap analysis validation |
| **Detection Failures** | Unknown | 0 | Anti-detection testing |
| **Request Format Match** | Partial | Exact | RE spec validation |
| **Code Sharing** | N/A | 90% | Shared implementation |

---

## Business Value

### User Impact

**Primary Benefits**:
- Zero detection as non-Antigravity client
- Stable API access without blocking
- Full feature parity with original Antigravity
- Better integration with Google Cloud AI Companion

**Risk Mitigation**:
- Prevents account blocking from detection
- Reduces API rejection rate
- Ensures long-term compatibility

### Efficiency Gains

**Separate Implementation**: 9 hours (this epic) + 17.5 hours (Epic-003) = 26.5 hours total
**Shared Implementation**: 18-20 hours total (30% time savings)

**Recommendation**: Implement with Epic-003 using shared codebase

---

## Technical Context

### Current Implementation

**Files** (Same as Epic-003):
- `src-tauri/src/proxy/handlers/claude.rs` (1126 lines)
- `src-tauri/src/proxy/mappers/claude/request.rs` (1300+ lines)
- `src-tauri/src/proxy/mappers/claude/response.rs` (510 lines)
- `src-tauri/src/proxy/mappers/claude/models.rs` (400+ lines)

**What Works** (✅):
- User-Agent: "antigravity/1.13.3"
- Request ID format: "agent-{uuid}"
- v1internal endpoints
- Tool use transformation
- Response transformation

**What's Missing** (❌):
- Model provider information (modelId: 333, apiProvider: 26, modelProvider: 3)
- ideType: ANTIGRAVITY metadata
- Flexible tool configuration modes
- Grounding configuration

**Key Differences from Epic-003**:
- ✅ **Simpler**: No thinking block processing
- ✅ **No signature validation**: No JWT validation needed
- ✅ **No budget constraints**: No thinking budget warnings
- ✅ **No position enforcement**: No thinking position validation
- ✅ **No violation metrics**: Standard monitoring only

### Architecture Impact

**Components Affected**:
- Request mapper (claude/request.rs) - Primary changes
- Models (claude/models.rs) - Metadata structure

**Integration Points**:
- Google v1internal API
- Vertex AI routing layer
- Cloud AI Companion (optional)

**Shared Code Strategy**:
```rust
// Shared constant approach
pub const CLAUDE_SONNET_45_MODEL_ID: i32 = 333;           // Standard
pub const CLAUDE_SONNET_45_THINKING_MODEL_ID: i32 = 334;  // Thinking

pub const ANTHROPIC_VERTEX_API_PROVIDER: i32 = 26;        // Shared
pub const ANTHROPIC_MODEL_PROVIDER: i32 = 3;              // Shared
```

---

## Implementation Phases

### Phase 1: Critical Compliance (P0) - 5 hours **[SHARED]**

**Goal**: Achieve anti-detection compliance

**Stories**:
1. Story-004-01: Add Model Provider Constants (1.5h) **[SHARED with Epic-003]**
2. Story-004-02: Add ideType ANTIGRAVITY Metadata (2h) **[SHARED with Epic-003]**
3. Story-004-03: Model-Specific Routing (1.5h) **[STANDARD-SPECIFIC]**

**Success Criteria**:
- All requests include modelId: 333, apiProvider: 26, modelProvider: 3
- ideType: ANTIGRAVITY present in all request metadata
- Compliance score: 75-80% → 90%

**Note**: Stories marked **[SHARED]** reuse 90% of Epic-003 code with model ID constant changes

---

### Phase 2: Feature Parity (P2) - 3 hours **[SHARED]**

**Goal**: Complete feature set

**Stories**:
4. Story-004-04: Flexible Tool Configuration Modes (2h) **[SHARED with Epic-003-09]**
5. Story-004-05: Grounding Configuration (1h) **[SHARED with Epic-003-10]**

**Success Criteria**:
- All tool modes supported (AUTO, ANY, NONE, VALIDATED)
- Recitation policy configured
- Compliance score: 90% → 95%

---

### Phase 3: Enhancement & Context (P3) - 1 hour **[SHARED]**

**Goal**: Extended context support

**Stories**:
6. Story-004-06: Extended Session Metadata (1h) **[SHARED with Epic-003-04]**

**Success Criteria**:
- workspace_id and cloudaicompanion_project fields supported
- 100% compliance validated

---

## Reference Documentation

### Primary Analysis
- `docs/comparison/claude/claude-4-5-sonnet/current-implementation.md` (38KB)
  - Standard model gap analysis
  - Differences from thinking version
  - Implementation patterns

### Quick Reference
- `docs/comparison/claude/claude-4-5-sonnet/EXECUTIVE-SUMMARY.md` (5KB)
  - Quick overview
  - Critical issues (P0)
  - Shared implementation notes

### Technical Specifications
- `docs/technical-specs/antigravity-workflow-compliance-gap-analysis.md` (20KB)
  - Comprehensive gap analysis
  - All models overview

### Reverse Engineering
- `docs/antigravity/workflows/models/claude/claude-4-5-sonnet-workflow.md`
  - Expected request/response structure
  - Model configuration (ID: 333)
  - Anti-detection markers

---

## Story Breakdown

### Total Stories: 6
### Total Effort: 9 hours (~1.1 days)

**P0 Stories** (5h):
- Stories 1-3: Critical compliance (model info, metadata, routing)

**P2 Stories** (3h):
- Stories 4-5: Feature parity (tool modes, grounding)

**P3 Stories** (1h):
- Story 6: Extended session metadata

**Code Sharing**:
- Stories 1-2, 4-6: **90% shared** with Epic-003 (model ID constant changes only)
- Story 3: **Standard-specific** (routing logic for model ID 333)

---

## Comparison with Epic-003

| Aspect | Epic-004 (Standard) | Epic-003 (Thinking) |
|--------|---------------------|---------------------|
| **Model ID** | 333 | 334 |
| **Stories** | 6 | 12 |
| **Effort** | 9 hours | 17.5 hours |
| **Shared Code** | 90% | 90% |
| **Thinking Mode** | ❌ No | ✅ Yes |
| **Signature Validation** | ❌ Not needed | ✅ JWT validation |
| **Budget Constraints** | ❌ Not applicable | ✅ Required |
| **Position Enforcement** | ❌ Not applicable | ✅ Required |
| **Violation Metrics** | ❌ Standard only | ✅ Comprehensive |

**Key Insight**: Epic-004 is significantly simpler due to no thinking-specific features

---

## Risks & Mitigation

### Technical Risks

**Risk**: Code divergence between standard and thinking versions
**Mitigation**: Use shared helper functions with model-specific constants

**Risk**: Breaking changes to existing functionality
**Mitigation**: Comprehensive unit/integration tests, phased rollout

**Risk**: Detection by Google despite compliance
**Mitigation**: Exact RE spec matching, anti-detection validation

### Dependencies

**External**:
- Google v1internal API stability
- Vertex AI routing layer availability

**Internal**:
- Epic-003 shared codebase (recommended to implement together)
- Existing request/response transformation logic

---

## Definition of Done (Epic Level)

**Code**:
- [ ] All 6 stories completed and merged
- [ ] Code review approved for all changes
- [ ] No regression in existing functionality
- [ ] 90% code sharing with Epic-003 validated

**Testing**:
- [ ] All unit tests passing (≥80% coverage)
- [ ] All integration tests passing
- [ ] Manual validation against RE spec
- [ ] Anti-detection testing completed

**Compliance**:
- [ ] Compliance score: 100%
- [ ] All 5 gaps addressed (from gap analysis)
- [ ] Request format matches RE spec exactly
- [ ] Zero detection failures in testing

**Documentation**:
- [ ] All code changes documented
- [ ] Implementation notes complete
- [ ] Compliance validation report
- [ ] Shared code patterns documented

**Deployment**:
- [ ] Deployed to staging environment
- [ ] Production deployment approved
- [ ] Rollback plan tested

---

## Related Epics

**Prerequisites**: None (can be implemented independently)

**Recommended**: [Epic-003](Epic-003-Claude-4.5-Sonnet-Thinking-Compliance.md) (implement together for 30% efficiency gain)

**Future Epics**:
- GEMINI-THINKING-001: Gemini 2.5 Flash Thinking compliance
- CLAUDE-HAIKU-001: Claude 4.5 Haiku compliance

---

## Implementation Strategy

### Recommended Approach: Combined Implementation

**Implement Epic-003 and Epic-004 together**:
1. Create shared constants file:
   ```rust
   // Model IDs
   pub const CLAUDE_SONNET_45_STANDARD_MODEL_ID: i32 = 333;
   pub const CLAUDE_SONNET_45_THINKING_MODEL_ID: i32 = 334;

   // Shared constants
   pub const ANTHROPIC_VERTEX_API_PROVIDER: i32 = 26;
   pub const ANTHROPIC_MODEL_PROVIDER: i32 = 3;
   ```

2. Use model detection to branch logic:
   ```rust
   let model_id = if is_thinking_mode {
       CLAUDE_SONNET_45_THINKING_MODEL_ID
   } else {
       CLAUDE_SONNET_45_STANDARD_MODEL_ID
   };
   ```

3. Share all metadata, tool config, grounding logic

**Benefits**:
- ✅ 30% time savings (18-20 hours vs 26.5 hours)
- ✅ Consistent implementation patterns
- ✅ Single code review and testing cycle
- ✅ Easier maintenance

**Alternative: Sequential Implementation**
1. Complete Epic-003 first (17.5 hours)
2. Adapt for Epic-004 (5-6 hours instead of 9 hours due to learned patterns)
3. Total: 22.5-23.5 hours (still 10% savings from experience)

---

## Change Log

| Date | Change | Author |
|------|--------|--------|
| 2026-01-11 | Epic-004 created | BMad Master |
| 2026-01-11 | Initial structure and 6 stories planned | BMad Master |
| 2026-01-11 | Shared implementation strategy documented | BMad Master |

---

## Next Steps

1. **Review** Epic-004 structure and story breakdown
2. **Decision**: Implement separately or combined with Epic-003?
3. **Create** Story-004-01 through Story-004-06
4. **Validate** shared code strategy with development team
5. **Begin** implementation (Phase 1: Critical Compliance)
