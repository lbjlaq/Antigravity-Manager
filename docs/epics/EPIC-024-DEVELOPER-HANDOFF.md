# Epic-024 Developer Handoff - Anti-Detection Hardening

**Date**: 2026-01-12
**From**: Product Manager (Ivan)
**To**: Tech Lead - Team 2 (Multi-Protocol Specialists)
**Status**: ✅ COMPLETE - QA PASSED (10/10) - MERGED TO MAIN (a079136)
**Timeline**: 3 weeks (23-25 hours total effort)
**Priority**: 🚨 P0 CRITICAL

---

## 🎯 Executive Summary

### What We're Building
Complete anti-detection hardening to protect **ALL models (100%)** from detection-based service unavailability.

### Why It's Critical (P0)
- **Current Risk**: Only 60% of models protected → detection = service unavailability
- **Business Impact**: Affects 100% of users, revenue loss, reputation damage
- **Priority**: P0 CRITICAL - blocks all other work if service unavailable

### Scope
**4 stories**, **23-25 hours** (3 weeks with 3 developers), **100% detection protection**

---

## 📊 Current State & Gaps

### Detection Coverage Analysis
```yaml
current_state:
  protected_models: "60% (ideType markers present)"
  unprotected_models: "40% (15+ models missing markers)"
  risk_level: "P0 CRITICAL"

gaps:
  gap_1: "ideType markers missing for 15+ models"
  gap_2: "apiProvider fields incomplete or hardcoded"
  gap_3: "Static user-agent (detection risk)"
  gap_4: "No detection monitoring or alerting"

target_state:
  protected_models: "100% (all models have full anti-detection)"
  detection_monitoring: "Real-time with alerting"
  risk_level: "ELIMINATED"
```

---

## 📋 Story Breakdown

### Story 024-01: ideType Marker Addition
**Priority**: P0 (CRITICAL)
**Effort**: 8-10 hours (4-5 days with pair programming)
**Assignee**: Dev 2A + Dev 2B (PAIR PROGRAMMING - security critical)

**Objective**: Add `ideType: "ANTIGRAVITY"` to 15+ models across all mappers

**Key Deliverables**:
- 5+ Claude models with ideType (claude-4.5-sonnet standard, opus standard, etc.)
- 5+ Gemini models with ideType (gemini-2.0-flash-exp, 2.5-flash, etc.)
- 5+ OpenAI models with ideType (gpt-4o, gpt-4o-mini, etc.)
- 30+ tests (15 unit, 10 integration, 5 E2E)

**Files Modified**:
- `src-tauri/src/proxy/mappers/claude/models.rs`
- `src-tauri/src/proxy/mappers/gemini/models.rs`
- `src-tauri/src/proxy/mappers/openai/models.rs`
- `tests/security/ideType_markers_tests.rs` (NEW)

**Why Pair Programming**: Security critical, real-time review, reduced bugs

**Documentation**: `docs/stories/Story-024-01-ideType-marker-addition.md` (detailed)

---

### Story 024-02: apiProvider Field Completion
**Priority**: P0 (CRITICAL)
**Effort**: 6-8 hours (3 days)
**Assignee**: Dev 2B (Mid-Level Protocol Specialist)
**Dependencies**: Story 024-01 (ideType structure established)

**Objective**: Complete apiProvider logic for correct upstream routing

**Key Deliverables**:
- `api_provider.rs` constants module (ANTHROPIC_VERTEX=26, GOOGLE_VERTEX=32, OPENAI=1, OPENAI_AZURE=2)
- All Claude models: apiProvider = 26
- All Gemini models: apiProvider = 32
- All OpenAI models: apiProvider = 1 or 2 (dynamic Azure support)
- 35+ tests (20 unit, 10 integration, 5 E2E)

**Files Modified**:
- `src-tauri/src/models/api_provider.rs` (NEW)
- `src-tauri/src/proxy/mappers/claude/models.rs` (use constants)
- `src-tauri/src/proxy/mappers/gemini/models.rs` (use constants)
- `src-tauri/src/proxy/mappers/openai/models.rs` (use constants + Azure)
- `src-tauri/src/proxy/mappers/*/request.rs` (include in payloads)
- `tests/security/apiProvider_tests.rs` (NEW)

**Documentation**: `docs/stories/Story-024-02-apiProvider-field-completion.md`

---

### Story 024-03: User-Agent Rotation Strategy
**Priority**: P1 (HIGH)
**Effort**: 5-7 hours (3 days)
**Assignee**: Dev 2A (Senior Security Specialist)
**Dependencies**: None (can run parallel with 024-02)

**Objective**: Implement user-agent rotation to prevent static pattern detection

**Key Deliverables**:
- 10+ diverse user agents (Chrome, Firefox, Safari, Edge on various platforms)
- 3 rotation strategies (random, round-robin, weighted)
- Configurable via config file
- Metrics track rotation effectiveness
- 15+ tests (10 unit, 5 integration)

**Files Modified**:
- `src-tauri/src/proxy/upstream/client.rs` (rotation logic)
- `src-tauri/src/proxy/config.rs` (user-agent pool config)
- `tests/security/user_agent_rotation_tests.rs` (NEW)

**Documentation**: `docs/stories/Story-024-03-user-agent-rotation.md`

---

### Story 024-04: Detection Monitoring & Alerting
**Priority**: P1 (HIGH)
**Effort**: 4-5 hours (split across Week 1-2)
**Assignee**: Dev 2C (Junior Monitoring Specialist)
**Dependencies**: None (can run parallel)

**Objective**: Real-time detection monitoring with alerting

**Key Deliverables**:
- 6 detection event categories (ideType missing, rate limits, auth errors, etc.)
- Configurable alert thresholds (count + time window)
- 3 notification channels (email, webhook, dashboard)
- Monitoring dashboard (real-time stats)
- 15+ tests (10 unit, 5 integration)

**Files Modified**:
- `src-tauri/src/proxy/monitor.rs` (detection metrics - **SHARED with Team 1**)
- `src-tauri/src/modules/logger.rs` (detection alerts - **SHARED with Team 1**)
- `tests/security/detection_monitoring_tests.rs` (NEW)

**Documentation**: `docs/stories/Story-024-04-detection-monitoring-alerting.md`

---

## 🔀 Code Conflict Prevention

### Shared Files with Team 1

**CRITICAL**: Team 1 (Epic-015) and Team 2 (Epic-024) both modify `monitor.rs` and `logger.rs`

#### Strategy: Separate Sections

**monitor.rs**:
```yaml
team_1_section: "Lines 1-200 (analytics module from Epic-013)"
team_2_section: "Lines 201-400 (detection module)"
conflict_risk: "🟢 LOW (separate functions)"

coordination:
  - "Team 2 writes to lines 201-400 ONLY"
  - "Do NOT modify Team 1 analytics functions"
  - "Daily sync: Dev 2A + Dev 1A (Team Leads)"
  - "Merge coordination: Week 2 end"
```

**logger.rs**:
```yaml
team_1_section: "Thinking log types (budget, level, cache)"
team_2_section: "Security log types (detection, rotation, alerts)"
conflict_risk: "🟢 LOW (different log categories)"

coordination:
  - "Add detection log types (DetectionEvent, RotationEvent, etc.)"
  - "Do NOT modify thinking log types"
  - "Daily sync via Slack #team-merge-sync"
```

### Team 2 Exclusive Files (Safe)
```yaml
no_conflicts:
  - "mappers/claude/**" ✅
  - "mappers/openai/**" ✅
  - "mappers/gemini/models.rs" ✅ (Team 1 uses gemini/request.rs, budget_optimizer.rs)
  - "upstream/client.rs" ✅
  - "tests/security/**" ✅
  - "models/api_provider.rs" ✅ (NEW)
```

---

## 🗓️ Week-by-Week Execution Plan

### Week 1 (Days 1-5)
**Focus**: ideType markers + detection monitoring setup

**Dev 2A** (Senior Security):
```yaml
days_1_5:
  story: "024-01 ideType markers (PAIR with Dev 2B)"
  effort: "4-5 days"
  files: "claude/gemini/openai models.rs"
  deliverable: "15+ models with ideType: 'ANTIGRAVITY'"
  pairing: "Security critical - real-time code review with Dev 2B"
```

**Dev 2B** (Mid-Level Protocol):
```yaml
days_1_5:
  story: "024-01 ideType markers (PAIR with Dev 2A)"
  effort: "4-5 days"
  files: "Same as Dev 2A"
  deliverable: "Joint delivery - 15+ models complete"
  role: "Protocol implementation, Dev 2A validates security"
```

**Dev 2C** (Junior Monitoring):
```yaml
days_1_3:
  story: "024-04 Detection Monitoring Part 1"
  effort: "2 days"
  files: "monitor.rs (lines 201-300), logger.rs (detection types)"
  deliverable: "Detection event logging, basic metrics"

days_4_5:
  activity: "Support Dev 2A/2B testing"
  deliverable: "Integration tests for ideType markers"
```

**Week 1 Output**:
- ✅ Story 024-01 COMPLETE (ideType markers for 15+ models)
- ✅ Story 024-04 Part 1 (detection logging operational)
- ⚠️ Daily sync with Team 1 Lead on shared files

---

### Week 2 (Days 6-10)
**Focus**: apiProvider completion + user-agent rotation

**Dev 2A** (Senior Security):
```yaml
days_6_8:
  story: "024-03 User-Agent Rotation"
  effort: "3 days"
  files: "upstream/client.rs, config.rs"
  deliverable: "10+ user agents, 3 rotation strategies, metrics"

days_9_10:
  activity: "Code review + Team 2 integration testing"
  deliverable: "All Story 024-03 tests passing"
```

**Dev 2B** (Mid-Level Protocol):
```yaml
days_6_8:
  story: "024-02 apiProvider Field Completion"
  effort: "3 days"
  files: "models/api_provider.rs (NEW), mappers/*/models.rs, mappers/*/request.rs"
  deliverable: "apiProvider constants + all models updated"

days_9_10:
  activity: "Integration testing + regression validation"
  deliverable: "398/398 baseline tests + 35+ new tests passing"
```

**Dev 2C** (Junior Monitoring):
```yaml
days_6_8:
  story: "024-04 Detection Monitoring Part 2"
  effort: "2-3 days"
  files: "monitor.rs (lines 301-400), alerting module"
  deliverable: "Alert thresholds, notification system, dashboard"

days_9_10:
  activity: "Epic-024 QA + documentation"
  deliverable: "All 4 stories validated, monitoring operational"
```

**Week 2 Output**:
- ✅ Story 024-02 COMPLETE (apiProvider for all models)
- ✅ Story 024-03 COMPLETE (user-agent rotation working)
- ✅ Story 024-04 COMPLETE (monitoring dashboard operational)
- ⚠️ Merge coordination with Team 1 (monitor.rs, logger.rs)

---

### Week 3 (Days 11-15)
**Focus**: Epic-024 validation + Epic-017 preparation

**Dev 2A** (Senior Security):
```yaml
days_11_12:
  activity: "Epic-024 Final Security Audit"
  checklist:
    - "All 15+ models have ideType + apiProvider"
    - "User-agent rotation working (10+ agents)"
    - "Detection monitoring operational"
    - "Security validation complete"
  deliverable: "Security sign-off"

days_13_15:
  activity: "Epic-017 Discovery (Claude Sonnet Standard)"
  tasks:
    - "Read claude-4-5-sonnet gaps (5 features missing)"
    - "Plan Epic-017 story breakdown"
    - "Prepare for Week 4 start"
```

**Dev 2B + 2C**:
```yaml
days_11_12:
  activity: "Epic-024 Integration Testing"
  deliverable: "Cross-protocol tests, E2E validation"

days_13_15:
  activity: "Epic-017 Preparation"
  deliverable: "Story files ready for Week 4"
```

**Week 3 Output**:
- ✅ Epic-024 COMPLETE (100% detection protection)
- ✅ Epic-017 discovery done
- ✅ Ready to start Epic-017 Week 4

---

## ✅ Success Criteria

### Detection Coverage
```yaml
before: "60% models protected"
after: "100% models protected ✅"

metrics:
  - "All 15+ models have ideType: 'ANTIGRAVITY'"
  - "All models have correct apiProvider (26/32/1/2)"
  - "User-agent rotation active (10+ agents)"
  - "Detection monitoring operational"
```

### Test Coverage
```yaml
baseline: "398/398 tests passing (Epic-013)"
new_tests: "95+ tests"
  - "Story 024-01: 30 tests"
  - "Story 024-02: 35 tests"
  - "Story 024-03: 15 tests"
  - "Story 024-04: 15 tests"
total: "493+ tests passing (100%)"
```

### Business Impact
```yaml
risk_before: "P0 CRITICAL (service unavailability)"
risk_after: "ELIMINATED ✅"

value:
  - "100% user base protected"
  - "Zero detection-based downtime"
  - "Foundation for Epic-017/019 (Claude models)"
```

---

## 📊 Quality Gates

### Week 1 Checkpoint
- [ ] Story 024-01 complete (15+ models with ideType)
- [ ] Pair programming log (Dev 2A + 2B)
- [ ] Security validation (Dev 2A sign-off)
- [ ] 30+ tests passing

### Week 2 Checkpoint
- [ ] Story 024-02 complete (apiProvider constants + all models)
- [ ] Story 024-03 complete (user-agent rotation working)
- [ ] Story 024-04 complete (monitoring dashboard)
- [ ] 95+ new tests passing
- [ ] 398/398 regression tests still passing

### Week 3 Checkpoint (Epic Complete)
- [ ] All 4 stories delivered
- [ ] Security audit passed
- [ ] 493+ tests passing (100%)
- [ ] Detection monitoring operational
- [ ] Epic-024 merged to main
- [ ] Epic-017 discovery complete

---

## 🔗 Dependencies & Blockers

### Prerequisites
- ✅ Epic-013 COMPLETE (2026-01-12) - no blockers

### Enables
- ✅ Epic-017 (Claude 4.5 Sonnet Standard) - Week 4
- ✅ Epic-019 (Claude 4.5 Opus Standard) - Week 5-6
- ✅ 100% model protection (all future epics)

### Coordination Required
**Daily Sync** (Dev 2A + Dev 1A, Team Leads):
- Time: 9:30 AM (15 minutes)
- Focus: Shared files (monitor.rs, logger.rs)
- Channel: Slack #team-merge-sync

**Weekly Demo** (All Team 2 + PM):
- Time: Friday 3 PM (30 minutes)
- Focus: Sprint progress, checkpoints

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

### Cross-Team Sync (Team Leads)
```yaml
time: "9:30 AM"
duration: "15 minutes"
attendees: "Dev 2A (Team 2 Lead) + Dev 1A (Team 1 Lead)"
focus: "Shared file coordination (monitor.rs, logger.rs)"
```

### Merge Coordination
```yaml
channel: "#team-merge-sync (Slack)"
protocol:
  - "Tag @team-lead before PR merge on shared files"
  - "Cross-team PR review mandatory for monitor.rs, logger.rs"
  - "Merge window: Week 2 end (coordinate with Team 1)"
```

---

## 🚀 Getting Started (Day 1 Actions)

### Immediate (Team Lead - Dev 2A)
1. [ ] Review all 4 story files in `docs/stories/Story-024-*.md`
2. [ ] Assign developers: Dev 2A + 2B (pair), Dev 2C (monitoring)
3. [ ] Setup daily standup (9:00 AM)
4. [ ] Setup cross-team sync (9:30 AM with Dev 1A)
5. [ ] Create Slack channel: #team-merge-sync

### Day 1 Development Start
**Dev 2A + 2B** (Pair Programming):
- [ ] Read Story-024-01 (ideType markers)
- [ ] Setup pair programming environment
- [ ] Begin Claude models (mappers/claude/models.rs)
- [ ] First commit: claude-4.5-sonnet + opus standard ideType

**Dev 2C** (Monitoring):
- [ ] Read Story-024-04 (detection monitoring)
- [ ] Plan monitor.rs extension (lines 201-300)
- [ ] Begin detection event logging
- [ ] First commit: DetectionEvent log types

---

## 📚 Documentation Reference

### Story Files (Detailed Implementation)
1. `docs/stories/Story-024-01-ideType-marker-addition.md` (comprehensive)
2. `docs/stories/Story-024-02-apiProvider-field-completion.md` (comprehensive)
3. `docs/stories/Story-024-03-user-agent-rotation.md` (comprehensive)
4. `docs/stories/Story-024-04-detection-monitoring-alerting.md` (comprehensive)

### Planning Documents
5. `docs/epics/Q2-2026-TEAM-ALLOCATION-PLAN.md` (full Q2 roadmap)
6. `docs/epics/Q2-2026-VISUAL-ROADMAP.md` (timeline visualization)
7. `docs/epics/Q2-2026-STORY-ASSIGNMENT-TABLE.md` (assignment matrix)

### Reference Documents
8. `docs/epics/FUTURE-EPICS-ROADMAP-Q2-2026.md` (Epic-024 context)
9. `docs/comparison/MASTER-MODELS-TABLE.md` (all models status)

---

**Epic Status**: ✅ READY FOR EXECUTION
**Team**: Team 2 (Multi-Protocol Specialists)
**Start Date**: Week 1 Day 1 (after Epic-013 merge)
**Expected Completion**: Week 3 End (3 weeks)
**Next Epic**: Epic-017 (Claude Sonnet Standard, Week 4)

Good luck, Team 2! 🚀 Protect all our users! 🛡️

---

**Document Version**: 1.0 FINAL
**Created**: 2026-01-12
**Last Updated**: 2026-01-12
