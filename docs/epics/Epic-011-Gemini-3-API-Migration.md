# Epic-011: Gemini 3 API Migration (thinkingBudget → thinkingLevel)

**Epic ID**: Epic-011
**Priority**: 🚨 CRITICAL (P0) - BLOCKS Epic-010
**Status**: 📋 PLANNED
**Type**: Infrastructure / Technical Debt Resolution
**Created**: 2026-01-11
**Target**: Q1 2026 (After Epic-007/008/009)

---

## 📋 Executive Summary

```yaml
epic_overview:
  title: "Gemini 3 API Migration"
  type: "Critical Infrastructure Update"
  scope: "Migrate from Gemini 2.5 thinkingBudget API to Gemini 3 thinkingLevel API"

business_impact:
  blocks: "Epic-010 (Gemini 3 Flash Compliance)"
  affects: "Epic-007 (Image), Epic-009 (Low) - improves thinking support"
  enables: "100% Gemini 3.x series compliance"

technical_summary:
  current_state: "Code uses Gemini 2.5 API (thinkingBudget) for Gemini 3 models"
  target_state: "Code uses Gemini 3 API (thinkingLevel) for all Gemini 3.x"
  compatibility: "BREAKING - parameters are mutually exclusive"

timeline:
  estimated_duration: "2 weeks (10 working days)"
  phase_1_critical: "1 week (5 days) - API migration"
  phase_2_validation: "1 week (5 days) - Testing + docs"

resources:
  team_size: "3 developers (1 backend lead + 2 engineers)"
  qa_support: "1 QA engineer (full-time for Phase 2)"
  documentation: "1 tech writer (part-time)"
```

---

## 🎯 Problem Statement

### Current Situation (AS-IS)

```yaml
gemini_2_5_api:
  parameter: "thinkingBudget"
  type: "integer"
  range: "1-32000 tokens"
  example: 16000
  models: ["gemini-2.5-flash", "gemini-2.5-flash-thinking", "gemini-2.5-pro-thinking"]

gemini_3_api:
  parameter: "thinkingLevel"
  type: "enum"
  values: ["MINIMAL", "LOW", "MEDIUM", "HIGH"]
  models: ["gemini-3-flash", "gemini-3-pro-high", "gemini-3-pro-low"]

current_implementation:
  problem: "Code uses thinkingBudget for ALL Gemini models"
  affected: "Gemini 3.x models get WRONG API format"
  result: "API incompatibility, unpredictable behavior"
```

### Impact Analysis

```yaml
epic_010_gemini_3_flash:
  status: "BLOCKED 🚫"
  compliance: "68.8% (due to API issues)"
  thinking_compliance: "25% (2/8 features)"
  impact: "Cannot implement until API fixed"

epic_009_gemini_3_pro_low:
  status: "IN PROGRESS (Команда 2)"
  current_compliance: "82.1%"
  thinking_support: "Affected by wrong API"
  impact: "Thinking mode may not work correctly"

epic_007_gemini_3_pro_image:
  status: "IN PROGRESS (Команда 1)"
  thinking_support: "N/A (image model doesn't use thinking)"
  impact: "No impact (image doesn't have thinking)"

overall_gemini_3_series:
  completion: "67% (2/3 models at 100%)"
  blocked_by: "API incompatibility"
  after_fix: "100% completion possible"
```

---

## 🚨 Critical Issues

### Issue 1: API Incompatibility (P0)

**Severity**: 🚨 CRITICAL
**Priority**: P0
**Blocks**: Epic-010, affects Epic-009

```yaml
problem:
  description: "Code uses Gemini 2.5 API for Gemini 3 models"

  current_code:
    location: "openai/request.rs:263-272"
    api: "thinkingBudget: 16000"
    models_affected: ["gemini-3-flash", "gemini-3-pro-high", "gemini-3-pro-low"]

  required_code:
    api: "thinkingLevel: 'HIGH'"
    values: ["MINIMAL", "LOW", "MEDIUM", "HIGH"]

impact:
  openai_protocol: "Flash excluded from auto-injection (intentional workaround)"
  claude_protocol: "Sends wrong API format"
  gemini_native: "Direct API rejection possible"

evidence:
  file_1: "openai/request.rs:263-272"
  file_2: "claude/request.rs:1517-1522"
  documentation: "gemini-3-flash-COMPARISON.md:104-122"
```

### Issue 2: Flash Auto-Injection Exclusion (P1)

**Severity**: ⚠️ HIGH
**Priority**: P1
**Depends on**: Issue 1 fix

```yaml
problem:
  description: "Flash excluded from OpenAI auto-injection"
  location: "openai/request.rs:247-250"

  current_pattern:
    logic: "ends_with('-high') || ends_with('-low') || contains('-pro')"
    flash_match: false
    reason: "Flash ends with '-flash', not matched"

  is_intentional: true
  rationale: "Safer to exclude until API fixed"

required_fix:
  new_pattern: "model.starts_with('gemini-3') && !model.contains('image')"
  includes: ["gemini-3-flash", "gemini-3-pro-high", "gemini-3-pro-low"]
  excludes: ["gemini-3-pro-image"]
```

### Issue 3: Missing Budget-to-Level Mapping (P1)

**Severity**: ⚠️ HIGH
**Priority**: P1
**Depends on**: Issue 1 fix

```yaml
problem:
  description: "No logic to convert token budgets to thinking levels"

  current_behavior:
    claude_sends: "budget_tokens: 25000"
    code_does: "Clamps to 32000, sends thinkingBudget: 25000"
    should_do: "Map to thinkingLevel: 'HIGH'"

required_mapping:
  flash_4_levels:
    "0-4000": "MINIMAL"
    "4001-10000": "LOW"
    "10001-20000": "MEDIUM"  # Flash exclusive!
    "20001-32000": "HIGH"

  pro_2_levels:
    "0-16000": "LOW"
    "16001-32000": "HIGH"
```

### Issue 4: Missing Test Coverage (P1)

**Severity**: ⚠️ MEDIUM
**Priority**: P1

```yaml
current_tests:
  gemini_3_flash:
    - "test_gemini_3_flash_basic_routing ✅"
    - "test_gemini_3_flash_without_thinking ✅"

missing_tests:
  - "test_gemini_3_flash_thinking_request"
  - "test_gemini_3_flash_budget_limits"
  - "test_gemini_3_flash_level_mapping"
  - "test_gemini_3_flash_medium_level"
  - "test_gemini_3_api_format_validation"

impact:
  regression_risk: "HIGH - changes not validated"
  production_confidence: "LOW - no proof it works"
```

---

## 🎯 Success Criteria

### Phase 1: Critical API Migration (P0)

```yaml
acceptance_criteria:
  1_api_detection:
    criterion: "Code correctly detects Gemini 3.x models"
    validation: "Unit tests for model detection"

  2_thinkingLevel_implementation:
    criterion: "All Gemini 3 models use thinkingLevel API"
    validation: "Request format inspection"

  3_backward_compatibility:
    criterion: "Gemini 2.5 models still use thinkingBudget"
    validation: "No regression in Gemini 2.5 tests"

  4_api_format_validation:
    criterion: "Requests validated before sending"
    validation: "Validation errors caught"

technical_metrics:
  api_correctness: "100% (all Gemini 3 use thinkingLevel)"
  backward_compatibility: "100% (Gemini 2.5 unchanged)"
  validation_coverage: "100% (all API formats validated)"
```

### Phase 2: Feature Parity (P1)

```yaml
acceptance_criteria:
  1_flash_auto_injection:
    criterion: "Flash included in OpenAI auto-injection"
    validation: "OpenAI protocol test"

  2_budget_mapping:
    criterion: "Token budgets correctly map to levels"
    validation: "All budget ranges tested"

  3_medium_level_support:
    criterion: "MEDIUM level only for Flash"
    validation: "Pro models don't use MEDIUM"

  4_test_coverage:
    criterion: "All 5 missing tests added and passing"
    validation: "Coverage ≥90% for thinking logic"

quality_metrics:
  test_coverage: "≥90% for thinking mode code"
  all_tests_passing: "100%"
  code_review_approved: "Yes"
```

### Epic-Level Success

```yaml
epic_completion:
  epic_010_unblocked: "Flash thinking can be implemented"
  epic_009_improved: "Low thinking works correctly"
  gemini_3_series: "100% API compliance"

compliance_impact:
  gemini_3_flash: "68.8% → 85% (unblocked for Phase 2+3)"
  gemini_3_pro_low: "82.1% → 95% (thinking fixed)"
  gemini_3_pro_high: "96.4% → 98% (thinking improved)"

business_impact:
  blocked_epics: "Epic-010 unblocked ✅"
  technical_debt: "Gemini 3 API debt eliminated ✅"
  future_epics: "Gemini 3 models production-ready ✅"
```

---

## 📦 Story Breakdown

### Phase 1: Critical API Migration (Week 1)

#### Story-011-01: Gemini 3 API Detection & Implementation

**Priority**: 🚨 P0 (CRITICAL)
**Effort**: 5 story points (2-3 days)
**Assignee**: Backend Lead
**Depends on**: None

```yaml
description: >
  Implement correct Gemini 3.x model detection and migrate to thinkingLevel API
  for all affected models (Flash, Pro High, Pro Low).

tasks:
  1_update_detection_logic:
    file: "openai/request.rs:247-250"
    change: "Detect all Gemini 3 models (not just Pro)"
    test: "Unit test for detection"

  2_implement_thinkingLevel_api:
    file: "openai/request.rs:263-272"
    change: "Replace thinkingBudget with thinkingLevel"
    logic: "Use determine_thinking_level() function"

  3_claude_protocol_migration:
    file: "claude/request.rs:1517-1522"
    change: "Map budget to level before sending"

  4_backward_compatibility:
    check: "Gemini 2.5 still uses thinkingBudget"
    validation: "Existing tests pass"

acceptance_criteria:
  - "Gemini 3 models use thinkingLevel API ✅"
  - "Gemini 2.5 models use thinkingBudget API ✅"
  - "Detection logic includes all Gemini 3 variants ✅"
  - "No breaking changes for existing models ✅"
  - "Unit tests pass ✅"

code_changes:
  files_modified:
    - "src-tauri/src/proxy/mappers/openai/request.rs"
    - "src-tauri/src/proxy/mappers/claude/request.rs"

  new_functions:
    - "determine_thinking_level(model: &str, budget: Option<i32>) -> &str"
    - "is_gemini_3_model(model: &str) -> bool"

validation:
  - "Manual API testing with Google"
  - "Format inspection for correctness"
  - "No regression in Gemini 2.5"
```

#### Story-011-02: Budget-to-Level Mapping Logic

**Priority**: 🚨 P0 (CRITICAL)
**Effort**: 3 story points (1-2 days)
**Assignee**: Backend Engineer
**Depends on**: Story-011-01 (parallel OK after architecture defined)

```yaml
description: >
  Implement intelligent mapping from token budgets to thinking levels,
  with Flash-specific MEDIUM level support and Pro-specific LOW/HIGH only.

tasks:
  1_implement_mapping_function:
    function: "determine_thinking_level()"
    flash_logic: "4 levels (MINIMAL, LOW, MEDIUM, HIGH)"
    pro_logic: "2 levels (LOW, HIGH)"

  2_budget_ranges:
    flash:
      minimal: "0-4000 tokens"
      low: "4001-10000 tokens"
      medium: "10001-20000 tokens"  # Exclusive to Flash!
      high: "20001+ tokens"
    pro:
      low: "0-16000 tokens"
      high: "16001+ tokens"

  3_default_levels:
    flash_default: "MEDIUM (balance cost/quality)"
    pro_default: "HIGH (max quality)"

  4_edge_cases:
    - "Budget = 0 → MINIMAL"
    - "Budget > 32000 → clamp to 32000, then HIGH"
    - "Budget not specified → use default"

acceptance_criteria:
  - "Flash supports 4 levels ✅"
  - "Pro supports 2 levels (no MEDIUM) ✅"
  - "All budget ranges map correctly ✅"
  - "MEDIUM level exclusive to Flash ✅"
  - "Default levels appropriate ✅"
  - "Edge cases handled ✅"

code_example:
  rust: |
    fn determine_thinking_level(model: &str, budget: Option<i32>) -> &'static str {
        // Default levels if no budget specified
        if budget.is_none() {
            return if model.contains("-flash") {
                "MEDIUM"  // Flash: balance
            } else {
                "HIGH"    // Pro: quality
            };
        }

        let budget = budget.unwrap().min(32000);  // Clamp to max

        if model.contains("-flash") {
            // Flash: 4 levels
            match budget {
                0..=4000 => "MINIMAL",
                4001..=10000 => "LOW",
                10001..=20000 => "MEDIUM",
                _ => "HIGH"
            }
        } else {
            // Pro (High/Low): 2 levels only
            match budget {
                0..=16000 => "LOW",
                _ => "HIGH"
            }
        }
    }

validation:
  - "Unit tests for all budget ranges"
  - "Flash-specific MEDIUM tests"
  - "Pro exclusion of MEDIUM verified"
```

#### Story-011-03: API Format Validation

**Priority**: 🚨 P0 (CRITICAL)
**Effort**: 2 story points (1 day)
**Assignee**: Backend Engineer
**Depends on**: Story-011-01, Story-011-02

```yaml
description: >
  Implement API format validation to catch errors before sending requests
  to Google, ensuring correct thinkingLevel for Gemini 3 and thinkingBudget
  for Gemini 2.5.

tasks:
  1_create_validator:
    file: "mappers/common/gemini_api_validator.rs" (new)
    function: "validate_gemini_thinking_config()"

  2_validation_rules:
    gemini_3:
      - "Must use thinkingLevel (enum)"
      - "Must NOT use thinkingBudget"
      - "Level must be valid for model type"
    gemini_2_5:
      - "Must use thinkingBudget (integer)"
      - "Must NOT use thinkingLevel"
      - "Budget within 1-32000 range"

  3_error_messages:
    type: "Descriptive validation errors"
    example: "Gemini 3 requires thinkingLevel, not thinkingBudget"

  4_integration:
    location: "Before upstream API call"
    action: "Validate and fail fast if incorrect"

acceptance_criteria:
  - "Gemini 3 validation catches thinkingBudget usage ✅"
  - "Gemini 2.5 validation catches thinkingLevel usage ✅"
  - "Invalid levels detected (e.g., MEDIUM for Pro) ✅"
  - "Clear error messages ✅"
  - "Validation runs before API call ✅"

validation:
  - "Unit tests for validation logic"
  - "Integration tests with malformed requests"
  - "Error message clarity verified"
```

### Phase 2: Feature Parity & Testing (Week 2)

#### Story-011-04: Flash Auto-Injection & Integration

**Priority**: ⚠️ P1 (HIGH)
**Effort**: 2 story points (1 day)
**Assignee**: Backend Engineer
**Depends on**: Story-011-01 (API migration complete)

```yaml
description: >
  Enable Flash in OpenAI auto-injection now that API is fixed,
  and ensure all Gemini 3 models get correct thinking behavior.

tasks:
  1_update_detection_pattern:
    current: "ends_with('-high') || ends_with('-low') || contains('-pro')"
    new: "model.starts_with('gemini-3') && !model.contains('image')"
    includes: ["flash", "pro-high", "pro-low"]
    excludes: ["pro-image"]

  2_verify_flash_injection:
    test: "Flash gets thinking with OpenAI protocol"
    default_level: "MEDIUM"

  3_integration_testing:
    protocols: ["OpenAI", "Claude", "Gemini Native"]
    models: ["flash", "pro-high", "pro-low"]

acceptance_criteria:
  - "Flash included in auto-injection ✅"
  - "Image excluded (no thinking support) ✅"
  - "All 3 thinking models get injection ✅"
  - "Default levels appropriate ✅"
  - "All protocols tested ✅"

validation:
  - "OpenAI protocol integration test"
  - "Claude protocol integration test"
  - "Gemini native protocol test"
```

#### Story-011-05: Comprehensive Test Coverage

**Priority**: ⚠️ P1 (HIGH)
**Effort**: 5 story points (2-3 days)
**Assignee**: Backend Engineer + QA
**Depends on**: All Phase 1 stories

```yaml
description: >
  Add comprehensive test coverage for Gemini 3 thinking mode,
  including all 5 missing critical tests.

tasks:
  1_test_thinking_request:
    name: "test_gemini_3_flash_thinking_request"
    validates:
      - "Thinking config injection"
      - "API format correctness"
      - "thinkingLevel present, thinkingBudget absent"

  2_test_budget_limits:
    name: "test_gemini_3_flash_budget_limits"
    validates:
      - "Budget clamping to 32000"
      - "Correct level after clamping"

  3_test_level_mapping:
    name: "test_gemini_3_flash_level_mapping"
    validates:
      - "All budget ranges → correct levels"
      - "Edge cases (0, 32000, >32000)"

  4_test_medium_level:
    name: "test_gemini_3_flash_medium_level"
    validates:
      - "Flash supports MEDIUM"
      - "Pro does NOT support MEDIUM"

  5_test_api_format:
    name: "test_gemini_3_api_format_validation"
    validates:
      - "Validation catches format errors"
      - "Gemini 3 uses thinkingLevel"
      - "Gemini 2.5 uses thinkingBudget"

acceptance_criteria:
  - "All 5 new tests implemented ✅"
  - "All tests passing ✅"
  - "Coverage ≥90% for thinking logic ✅"
  - "No regression in existing tests ✅"
  - "CI/CD integration complete ✅"

test_coverage_target:
  thinking_mode_code: "≥90%"
  api_mapping_code: "≥95%"
  validation_code: "100%"

deliverables:
  - "5 new test files"
  - "Test documentation"
  - "Coverage report"
```

#### Story-011-06: Documentation Update

**Priority**: ⚠️ P1 (HIGH)
**Effort**: 3 story points (1-2 days)
**Assignee**: Tech Writer + Backend Lead
**Depends on**: All Phase 1+2 stories

```yaml
description: >
  Update all documentation to reflect API migration,
  remove critical warnings, update compliance metrics.

tasks:
  1_update_workflow_docs:
    files:
      - "gemini-3-flash-thinking-workflow.md"
      - "gemini-3-pro-high-thinking-workflow.md"
      - "gemini-3-pro-low-thinking-workflow.md"
    changes:
      - "Remove ⚠️ CRITICAL API COMPATIBILITY NOTICE"
      - "Update examples to use thinkingLevel"
      - "Remove (API Update Required ⚠️) markers"

  2_update_comparison_docs:
    file: "gemini-3-flash-COMPARISON.md"
    changes:
      - "Update compliance: 68.8% → 85%"
      - "Thinking compliance: 25% → 85%"
      - "Remove critical API issue"
      - "Update implementation status"

  3_create_migration_guide:
    file: "GEMINI-3-API-MIGRATION-GUIDE.md" (new)
    content:
      - "API changes explanation"
      - "Before/after code examples"
      - "Migration timeline"
      - "Client impact assessment"

  4_update_reverse_engineering:
    file: "gemini-3-flash-reverse-engineering.md"
    changes:
      - "Mark API issue as RESOLVED"
      - "Document final implementation"
      - "Update gap analysis"

acceptance_criteria:
  - "All critical warnings removed ✅"
  - "Compliance metrics updated ✅"
  - "Migration guide complete ✅"
  - "Code examples use thinkingLevel ✅"
  - "Before/after comparison clear ✅"

deliverables:
  - "Updated workflow docs (3 files)"
  - "Updated COMPARISON docs"
  - "New migration guide"
  - "Updated reverse-engineering doc"
```

---

## 📊 Risk Assessment

### Technical Risks

```yaml
risk_1_breaking_changes:
  probability: "MEDIUM"
  impact: "HIGH"
  mitigation:
    - "Comprehensive testing before deployment"
    - "Backward compatibility validation"
    - "Feature flag for gradual rollout"

risk_2_api_rejection:
  probability: "LOW"
  impact: "HIGH"
  mitigation:
    - "Validate against Google API specs"
    - "Manual testing with real API"
    - "Error handling and fallback"

risk_3_regression:
  probability: "MEDIUM"
  impact: "MEDIUM"
  mitigation:
    - "Full regression test suite"
    - "No changes to Gemini 2.5 logic"
    - "Gradual rollout per model"
```

### Schedule Risks

```yaml
risk_1_dependency_delays:
  probability: "LOW"
  impact: "MEDIUM"
  mitigation:
    - "Stories designed for parallel work"
    - "Clear dependencies documented"
    - "Buffer time in estimates"

risk_2_testing_bottleneck:
  probability: "MEDIUM"
  impact: "LOW"
  mitigation:
    - "QA involved from start"
    - "Automated tests priority"
    - "Test writing parallel with dev"
```

---

## 🗺️ Timeline & Dependencies

### Week 1: Critical API Migration (P0)

```yaml
days_1_3:
  story: "Story-011-01 (API Detection & Implementation)"
  team: "Backend Lead"
  deliverable: "thinkingLevel API working"

days_2_4:
  story: "Story-011-02 (Budget-to-Level Mapping)"
  team: "Backend Engineer"
  deliverable: "Mapping logic complete"
  parallel_with: "Story-011-01 (after architecture defined)"

days_4_5:
  story: "Story-011-03 (API Validation)"
  team: "Backend Engineer"
  deliverable: "Validation in place"
  depends_on: "Story-011-01, Story-011-02"

milestone_week_1:
  achievement: "All Gemini 3 models use correct API"
  confidence: "HIGH"
```

### Week 2: Feature Parity & Testing (P1)

```yaml
days_6:
  story: "Story-011-04 (Flash Auto-Injection)"
  team: "Backend Engineer"
  deliverable: "Flash included in OpenAI protocol"

days_7_9:
  story: "Story-011-05 (Test Coverage)"
  team: "Backend Engineer + QA"
  deliverable: "5 tests added, ≥90% coverage"

days_9_10:
  story: "Story-011-06 (Documentation)"
  team: "Tech Writer + Backend Lead"
  deliverable: "All docs updated, warnings removed"

milestone_week_2:
  achievement: "Production-ready, fully tested, documented"
  confidence: "HIGH"
```

### Dependencies Graph

```
Story-011-01 (API Migration)
    ├─> Story-011-03 (Validation)
    │       └─> Story-011-04 (Flash Injection)
    │               └─> Story-011-05 (Tests)
    │                       └─> Story-011-06 (Docs)
    │
    └─> Story-011-02 (Mapping)
            └─> Story-011-03 (Validation)
```

---

## 📈 Success Metrics

### Technical Metrics

```yaml
code_quality:
  test_coverage: "≥90% for thinking mode"
  code_review: "100% approved"
  linting: "0 warnings"

api_correctness:
  gemini_3_format: "100% use thinkingLevel"
  gemini_2_5_format: "100% use thinkingBudget"
  validation_rate: "100% before API calls"

performance:
  no_latency_regression: "Response time unchanged"
  no_error_rate_increase: "Error rate ≤ baseline"
```

### Business Metrics

```yaml
epic_unblocking:
  epic_010_status: "UNBLOCKED ✅"
  epic_009_improvement: "Thinking mode working correctly"

compliance_improvement:
  gemini_3_flash: "68.8% → 85%"
  gemini_3_pro_low: "82.1% → 95%"
  gemini_3_series: "85.7% → 95%"

technical_debt:
  api_debt_eliminated: "YES ✅"
  future_maintenance: "REDUCED"
```

---

## 🔄 Post-Epic Activities

### Immediate (Week 3)

```yaml
epic_010_unblocking:
  action: "Begin Epic-010 planning"
  readiness: "Code ready for Flash thinking implementation"
  timeline: "Can start immediately"

epic_009_validation:
  action: "Validate Low thinking improvements"
  test: "Confirm thinking mode working correctly"

production_monitoring:
  action: "Monitor Gemini 3 API behavior"
  metrics: "Error rates, latency, thinking quality"
  duration: "2 weeks observation"
```

### Strategic Review Integration

```yaml
q1_retrospective:
  include: "Epic-011 as infrastructure win"
  lesson: "Critical API fixes enable multiple epics"

q2_planning:
  unblocked: "Epic-010 ready for Q2"
  pipeline: "Gemini 3 series 100% achievable"
```

---

## 📚 Related Documentation

### Implementation References

- **API Analysis**: `docs/antigravity/workflows/models/gemini/gemini-3-api-breaking-change-analysis.md`
- **Flash COMPARISON**: `docs/antigravity/workflows/models/gemini/gemini-3-flash-COMPARISON.md`
- **Technical Issues**: `docs/analysis/EPIC-010-TECHNICAL-ISSUES-ANALYSIS.md`
- **RE Quality Report**: `docs/analysis/GEMINI-3-FLASH-DOCUMENTATION-QUALITY-REPORT.md`

### Code Locations

- **OpenAI Mapper**: `src-tauri/src/proxy/mappers/openai/request.rs:247-272`
- **Claude Mapper**: `src-tauri/src/proxy/mappers/claude/request.rs:1517-1522`
- **Tests**: `src-tauri/src/proxy/tests/thinking_models.rs`

### Affected Epics

- **Epic-007** (gemini-3-pro-image): IN PROGRESS, no thinking impact
- **Epic-009** (gemini-3-pro-low): IN PROGRESS, thinking improved
- **Epic-010** (gemini-3-flash): BLOCKED → UNBLOCKED after this epic

---

## ✅ Definition of Done

### Phase 1 (P0) - Critical

- [ ] Gemini 3 models use `thinkingLevel` API
- [ ] Gemini 2.5 models continue using `thinkingBudget` API
- [ ] Budget-to-level mapping implemented (Flash 4 levels, Pro 2 levels)
- [ ] API format validation in place
- [ ] No regression in Gemini 2.5 functionality
- [ ] Unit tests passing

### Phase 2 (P1) - Parity

- [ ] Flash included in OpenAI auto-injection
- [ ] All 5 missing tests added and passing
- [ ] Test coverage ≥90% for thinking logic
- [ ] All documentation updated
- [ ] Critical warnings removed from docs
- [ ] Migration guide created

### Epic Complete

- [ ] All stories completed
- [ ] Code review approved
- [ ] QA sign-off
- [ ] Production deployment successful
- [ ] Epic-010 unblocked for planning
- [ ] No critical issues in production monitoring (1 week)

---

**Epic Status**: 📋 PLANNED
**Next Step**: Team assignment and sprint planning
**Estimated Start**: After Epic-007/008/009 completion (2026-02-17)
**Estimated Completion**: 2026-03-03 (2 weeks)
**Blocks**: Epic-010 (Gemini 3 Flash Compliance)
**Improves**: Epic-009 (Gemini 3 Pro Low thinking support)
