# Epic-017 Developer Handoff - Claude 4.5 Sonnet Standard Mode

**Date**: 2026-01-12
**From**: Product Manager (Ivan)
**To**: Tech Lead - Team 2 (Multi-Protocol Specialists)
**Status**: 📋 READY FOR EXECUTION
**Timeline**: 1.5 weeks (9-11 hours total effort)
**Priority**: P1 HIGH (Revenue Growth - Premium Model)

---

## 🎯 Executive Summary

### What We're Building
Implement **claude-4.5-sonnet** (standard mode, NO thinking) to achieve 100% compliance with Google Antigravity v1.13.3, unlocking access to high-demand premium model.

### Why It's Critical (P1 HIGH)
- **Revenue Growth**: Premium model, high user demand, competitive differentiation
- **Feature Parity**: Match claude-opus-4-5 thinking mode feature completeness
- **Code Reuse**: 90% shared patterns with thinking mode = fast delivery
- **Foundation Ready**: Epic-024 patterns (ideType, apiProvider) already implemented

### Scope
**3 stories**, **9-11 hours** (1.5 weeks with 3 developers), **75-80% → 100% compliance**

---

## 📊 Current State & Gaps

### Claude 4.5 Sonnet Standard Current State
```yaml
model: "claude-4.5-sonnet"
model_id: 333  # Standard mode
current_compliance: "~75-80%"
priority: "P1 HIGH (revenue growth)"

working_features: "~75%"
  - "User-Agent: antigravity/1.13.3" ✅
  - "Request ID format: agent-{uuid}" ✅
  - "v1internal endpoints" ✅
  - "Tool use transformation" ✅
  - "Response transformation" ✅

gaps: "5 critical features missing (identical to claude-opus-4-5)"
  gap_1: "Missing modelId: 333"
  gap_2: "Missing apiProvider: 26 (ANTHROPIC_VERTEX)"
  gap_3: "Missing ideType: 'ANTIGRAVITY'"
  gap_4: "Flexible tool modes (AUTO/ANY/NONE) missing"
  gap_5: "Grounding config missing"

status: "GOOD - 75-80% working, clear gaps"
```

### Key Differences from Thinking Mode
```yaml
claude-4.5-sonnet-thinking (modelId: 334):
  - "Extended thinking blocks" ✅
  - "Budget constraints (32000 tokens)" ✅
  - "Position enforcement" ✅
  - "Signature validation" ✅

claude-4.5-sonnet-standard (modelId: 333):
  - "NO thinking blocks" ⭕
  - "NO budget constraints" ⭕
  - "NO position enforcement" ⭕
  - "NO signature validation" ⭕
  - "Simpler implementation" ✅
  - "Same API structure otherwise" ✅
```

### Foundation Ready (Epic-024)
```yaml
epic_024_provides:
  ideType_markers: "Story 024-01 ✅ (detection patterns)"
  apiProvider_constants: "Story 024-02 ✅ (provider routing)"
  user_agent_rotation: "Story 024-03 ✅ (anti-detection)"
  detection_monitoring: "Story 024-04 ✅ (alerting)"

code_reuse: "90% from claude-opus-4-5 thinking mode"
  shared_files:
    - "mappers/claude/models.rs" (model constants)
    - "mappers/claude/request.rs" (request builder)
    - "mappers/claude/grounding.rs" (grounding config)

  differences: "Model ID constant only (333 vs 334)"
```

---

## 📋 Story Breakdown

### Story 017-01: Claude Sonnet Standard Core Implementation
**Priority**: P1 (HIGH - revenue growth)
**Effort**: 3-4 days
**Assignee**: Dev 2A (Senior Security, Multi-Protocol team)

**Objective**: Implement modelId 333, apiProvider 26, ideType "ANTIGRAVITY"

**Key Deliverables**:
- modelId: 333 (claude-4.5-sonnet standard)
- apiProvider: 26 (ANTHROPIC_VERTEX from Epic-024)
- ideType: "ANTIGRAVITY" (from Epic-024 patterns)
- Request/response transformation
- 20+ tests (10 unit, 7 integration, 3 E2E)

**Files Modified**:
- `src-tauri/src/proxy/mappers/claude/models.rs` (add model constants)
- `src-tauri/src/proxy/mappers/claude/request.rs` (include metadata)
- `tests/claude/sonnet_standard_tests.rs` (NEW)

**Pattern**: Copy from claude-4.5-sonnet thinking mode, adjust model ID

**Documentation**: `docs/stories/Story-017-01-claude-sonnet-standard-core.md` (comprehensive)

---

### Story 017-02: Tool Modes & Grounding Configuration
**Priority**: P1 (HIGH - feature parity)
**Effort**: 2-3 days
**Assignee**: Dev 2B (Mid-Level Protocol, Multi-Protocol team)
**Dependencies**: Story 017-01 (core implementation complete)

**Objective**: Flexible tool modes (AUTO/ANY/NONE), grounding config

**Key Deliverables**:
- Tool mode support: AUTO (default), ANY (force), NONE (disable)
- Grounding configuration (search integration)
- Recitation policy settings
- 15+ tests (8 unit, 5 integration, 2 E2E)

**Files Modified**:
- `src-tauri/src/proxy/mappers/claude/request.rs` (tool modes)
- `src-tauri/src/proxy/mappers/claude/grounding.rs` (grounding config - NEW if needed)
- `tests/claude/tool_modes_tests.rs` (NEW)

**Pattern**: Reuse from claude-opus-4-5 thinking mode tool modes

**Documentation**: `docs/stories/Story-017-02-tool-modes-grounding.md` (comprehensive)

---

### Story 017-03: Testing & Documentation
**Priority**: P2 (MEDIUM - quality assurance)
**Effort**: 2 days
**Assignee**: Dev 2C (Junior Monitoring, Multi-Protocol team)
**Dependencies**: Stories 017-01, 017-02 complete

**Objective**: Comprehensive test coverage, documentation updates

**Key Deliverables**:
- Cross-model integration tests (standard vs thinking)
- Regression validation (398/398 baseline tests)
- Performance benchmarks
- Documentation: comparison table, COMPARISON file
- 20+ tests (comprehensive coverage)

**Files Modified**:
- `tests/claude/cross_model_tests.rs` (NEW)
- `tests/claude/regression_tests.rs` (validation)
- `docs/comparison/claude-4-5-sonnet-COMPARISON.md` (NEW)

**Pattern**: Follow Epic-013 QA patterns (comprehensive validation)

**Documentation**: `docs/stories/Story-017-03-testing-documentation.md` (comprehensive)

---

## 🔀 Code Conflict Prevention

### Shared Files with Team 1
**IMPORTANT**: Epic-017 (Team 2) and Epic-015 (Team 1) do NOT share files - no conflicts!

```yaml
team_2_exclusive_files:
  - "mappers/claude/**" ✅ (Team 2 exclusive)
  - "handlers/claude.rs" ✅ (Team 2 exclusive)
  - "tests/claude/**" ✅ (Team 2 exclusive)
  - "models/api_provider.rs" ✅ (Team 2 created in Epic-024)

team_1_exclusive_files:
  - "mappers/gemini/**" ✅ (Team 1 exclusive)
  - "proxy/cache.rs" ✅ (Team 1 from Epic-013)
  - "db/cache_metrics.rs" ✅ (Team 1 from Epic-015)
  - "tests/gemini_3/**" ✅ (Team 1 exclusive)

conflict_risk: "🟢 ZERO (no shared files)"
```

### Team 2 File Ownership
```yaml
epic_017_files:
  core:
    - "mappers/claude/models.rs" (model definitions)
    - "mappers/claude/request.rs" (request builder)
    - "mappers/claude/grounding.rs" (grounding config - NEW if needed)

  testing:
    - "tests/claude/sonnet_standard_tests.rs" (NEW)
    - "tests/claude/tool_modes_tests.rs" (NEW)
    - "tests/claude/cross_model_tests.rs" (NEW)

  documentation:
    - "docs/comparison/claude-4-5-sonnet-COMPARISON.md" (NEW)

ownership: "100% Team 2 (Multi-Protocol Specialists)"
coordination_required: "NONE (independent from Team 1)"
```

---

## 🗓️ Week-by-Week Execution Plan

### Week 4 (Days 1-5) - Core Implementation
**Focus**: Core features + tool modes

**Dev 2A** (Senior Security):
```yaml
days_1_3:
  story: "017-01 Core Implementation"
  effort: "3 days"
  files: "mappers/claude/models.rs, request.rs"
  deliverable: "modelId 333, apiProvider 26, ideType ANTIGRAVITY"
  pattern: "Copy from thinking mode (334), adjust constants"

days_4_5:
  activity: "Code review + integration testing"
  deliverable: "Story 017-01 tests passing (20+ tests)"
```

**Dev 2B** (Mid-Level Protocol):
```yaml
days_1_2:
  activity: "Story 017-01 support + test development"
  deliverable: "Unit tests for core implementation"

days_3_5:
  story: "017-02 Tool Modes & Grounding"
  effort: "2-3 days"
  files: "request.rs (tool modes), grounding.rs (NEW)"
  deliverable: "AUTO/ANY/NONE modes, grounding config"
```

**Dev 2C** (Junior Monitoring):
```yaml
days_1_5:
  activity: "Test planning + documentation prep"
  deliverable: "Test strategy, documentation templates"
```

**Week 4 Output**:
- ✅ Story 017-01 COMPLETE (core implementation)
- ✅ Story 017-02 80% complete (tool modes working)

---

### Week 5 (Days 6-7.5) - Testing & Documentation
**Focus**: Quality assurance + documentation

**Dev 2A** (Senior Security):
```yaml
days_6_7:
  activity: "Epic-017 Final Validation"
  checklist:
    - "All 5 gaps resolved (modelId, apiProvider, ideType, tools, grounding)"
    - "100% compliance validated"
    - "Security audit passed"
  deliverable: "Security sign-off"

day_7_5:
  activity: "Epic-018 Preparation (if time permits)"
```

**Dev 2B** (Mid-Level Protocol):
```yaml
days_6_7:
  activity: "Story 017-02 completion + integration testing"
  deliverable: "All tool modes working, grounding config operational"

day_7_5:
  activity: "Cross-story validation"
```

**Dev 2C** (Junior Monitoring):
```yaml
days_6_7_5:
  story: "017-03 Testing & Documentation"
  effort: "2 days"
  deliverable:
    - "Cross-model tests (standard vs thinking)"
    - "Regression validation (398/398 baseline)"
    - "Documentation complete (COMPARISON file)"
```

**Week 5 Output**:
- ✅ Story 017-02 COMPLETE (tool modes + grounding)
- ✅ Story 017-03 COMPLETE (testing + docs)
- ✅ Epic-017 COMPLETE (100% compliance)

---

## ✅ Success Criteria

### Compliance Score
```yaml
before: "75-80% (5 gaps)"
after: "100% (all gaps resolved) ✅"

gaps_resolved:
  - "modelId: 333 implemented" ✅
  - "apiProvider: 26 implemented" ✅
  - "ideType: 'ANTIGRAVITY' implemented" ✅
  - "Tool modes (AUTO/ANY/NONE) working" ✅
  - "Grounding config operational" ✅
```

### Test Coverage
```yaml
baseline: "398/398 tests passing (Epic-013)"
new_tests: "55+ tests"
  - "Story 017-01: 20 tests (core)"
  - "Story 017-02: 15 tests (tools + grounding)"
  - "Story 017-03: 20 tests (integration + regression)"
total: "453+ tests passing (100%)"
```

### Business Impact
```yaml
revenue_impact: "HIGH (premium model access)"
user_satisfaction: "HIGH (requested feature)"
competitive_advantage: "MEDIUM (feature parity with competitors)"

value:
  - "Access to high-demand premium model"
  - "100% feature parity with thinking mode"
  - "Foundation for Epic-019 (Claude Opus standard)"
```

---

## 📊 Quality Gates

### Week 4 Checkpoint (Core Complete)
- [ ] Story 017-01 complete (core implementation)
- [ ] modelId 333, apiProvider 26, ideType ANTIGRAVITY validated
- [ ] Story 017-02 80% complete (tool modes working)
- [ ] 35+ tests passing (Story 017-01 + 017-02 partial)

### Week 5 Checkpoint (Epic Complete)
- [ ] All 3 stories delivered
- [ ] 100% compliance validated
- [ ] 55+ new tests passing (100%)
- [ ] 398/398 regression tests still passing
- [ ] Documentation complete (COMPARISON file)
- [ ] Epic-017 merged to main

---

## 🔗 Dependencies & Blockers

### Prerequisites
- ✅ Epic-024 COMPLETE (2026-01-12) - provides ideType, apiProvider patterns
- ✅ Epic-013 COMPLETE (2026-01-12) - 398/398 test baseline

### Parallel Work (No Conflicts)
- ⚪ Epic-015 (Gemini Pro Optimization) - Team 1, no shared files
- ⚪ Epic-007/009 (Gemini Image/Low) - Team 1, no shared files

### Enables
- ✅ Epic-019 (Claude Opus 4.5 Standard) - Week 7-8
- ✅ Complete Claude 4.5 offering (thinking + standard modes)
- ✅ Premium model revenue growth

---

## 📝 Communication Protocol

### Daily Standup (Team 2 Internal)
```yaml
time: "9:00 AM"
duration: "10 minutes"
attendees: "Dev 2A, 2B, 2C"
format:
  - "Yesterday's progress"
  - "Today's focus"
  - "Blockers"
```

### Weekly Demo (Team 2 + PM)
```yaml
time: "Friday 3 PM"
duration: "30 minutes"
attendees: "All Team 2 + PM (Ivan)"
focus: "Sprint progress, demos, Epic-017 status"
```

### Cross-Team Coordination
```yaml
requirement: "NONE (no file conflicts with Team 1)"
independence: "Team 2 can work independently"
```

---

## 🚀 Getting Started (Week 4 Day 1 Actions)

### Immediate (Team Lead - Dev 2A)
1. [ ] Review all 3 story files in `docs/stories/Story-017-*.md`
2. [ ] Assign developers: Dev 2A (core), Dev 2B (tools), Dev 2C (QA)
3. [ ] Setup daily standup (9:00 AM)
4. [ ] Confirm Epic-024 patterns available (ideType, apiProvider)
5. [ ] Review claude-opus-4-5 thinking mode code (90% reusable)

### Day 1 Development Start
**Dev 2A** (Core Implementation):
- [ ] Read Story-017-01 (core implementation)
- [ ] Review claude-opus-4-5 thinking mode code (mappers/claude/)
- [ ] Create claude-4.5-sonnet standard constants (modelId: 333)
- [ ] Begin request builder modifications
- [ ] First commit: model constants + basic structure

**Dev 2B** (Tool Modes):
- [ ] Read Story-017-02 (tool modes + grounding)
- [ ] Review thinking mode tool mode implementation
- [ ] Plan grounding config structure
- [ ] Prepare test data

**Dev 2C** (QA):
- [ ] Read Story-017-03 (testing + docs)
- [ ] Plan test strategy (cross-model, regression)
- [ ] Prepare documentation templates
- [ ] Setup test environments

---

## 📚 Documentation Reference

### Story Files (Detailed Implementation)
1. `docs/stories/Story-017-01-claude-sonnet-standard-core.md` (comprehensive)
2. `docs/stories/Story-017-02-tool-modes-grounding.md` (comprehensive)
3. `docs/stories/Story-017-03-testing-documentation.md` (comprehensive)

### Planning Documents
4. `docs/epics/Q2-2026-TEAM-ALLOCATION-PLAN.md` (full Q2 roadmap)
5. `docs/epics/Q2-2026-VISUAL-ROADMAP.md` (timeline visualization)
6. `docs/epics/Q2-2026-STORY-ASSIGNMENT-TABLE.md` (assignment matrix)

### Reference Documents (Epic-024 Foundation)
7. `docs/epics/EPIC-024-DEVELOPER-HANDOFF.md` (ideType, apiProvider patterns)
8. `docs/stories/Story-024-01-ideType-marker-addition.md` (ideType implementation)
9. `docs/stories/Story-024-02-apiProvider-field-completion.md` (apiProvider constants)

### Reference Documents (Epic-004 Analysis)
10. `docs/epics/Epic-004-Claude-4.5-Sonnet-Standard-Compliance.md` (gap analysis)
11. `docs/epics/Epic-004-GAPS-AND-RECOMMENDATIONS.md` (detailed gaps)
12. `docs/comparison/claude/claude-4-5-sonnet/current-implementation.md` (RE analysis)

### Reference Documents (Master Table)
13. `docs/comparison/MASTER-MODELS-TABLE.md` (all models status)

---

**Epic Status**: 📋 READY FOR EXECUTION
**Team**: Team 2 (Multi-Protocol Specialists)
**Start Date**: Week 4 Day 1 (after Epic-024 merge)
**Expected Completion**: Week 5 Day 7.5 (1.5 weeks)
**Next Epic**: Epic-019 (Claude Opus Standard, Week 7-8)

Good luck, Team 2! 🚀 Let's unlock premium model access! 💎

---

**Document Version**: 1.0 FINAL
**Created**: 2026-01-12
**Last Updated**: 2026-01-12
