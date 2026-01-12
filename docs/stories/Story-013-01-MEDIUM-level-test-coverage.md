# Story-013-01: MEDIUM Level Test Coverage

**Epic**: Epic-013 (Gemini 3 Flash Optimization)
**Дата создания**: 2026-01-12
**Статус**: 📋 READY FOR DEVELOPMENT
**Приоритет**: P1 (High)
**Оценка**: 1-2 дня

---

## 🎯 User Story

**As a** Quality Engineer
**I want** comprehensive test coverage for Flash MEDIUM thinking level
**So that** we validate Flash's unique 4-level thinking capability and achieve 95%+ compliance

**Context**: Gemini 3 Flash is the ONLY model supporting MEDIUM thinking level (Pro has only LOW/HIGH). Current test coverage: 3/5 thinking tests done. Need 2 more tests for MEDIUM level validation.

---

## 📋 Background

### Current Implementation Status (Epic-011 Complete)

**File**: `src-tauri/src/proxy/tests/thinking_models.rs`

**Existing Tests** (3/5):
```yaml
test_1:
  name: "test_gemini_3_flash_thinking_basic"
  status: "✅ PASSING"
  coverage: "Basic thinking mode activation"

test_2:
  name: "test_gemini_3_flash_auto_injection"
  status: "✅ PASSING"
  coverage: "OpenAI protocol auto-injection"

test_3:
  name: "test_gemini_3_flash_level_mapping"
  status: "✅ PASSING"
  coverage: "Budget-to-level mapping (basic)"
```

**Missing Tests** (2/5):
```yaml
test_4_medium_level:
  name: "test_gemini_3_flash_medium_level_exclusive"
  status: "❌ NOT IMPLEMENTED"
  purpose: "Verify MEDIUM works on Flash, fails on Pro"

test_5_medium_mapping:
  name: "test_gemini_3_flash_medium_budget_mapping"
  status: "❌ NOT IMPLEMENTED"
  purpose: "Verify 10001-20000 token range maps to MEDIUM"
```

### Gap Reference

**From**: `gemini-3-flash-COMPARISON.md:320-350` (Gap 3: TEST-001)

```yaml
test_coverage_gap:
  total_tests_needed: 5
  implemented: 3
  missing: 2
  compliance_impact: "88% → 95%"

missing_test_scenarios:
  1_medium_level_exclusive:
    description: "MEDIUM level works on Flash but NOT on Pro"
    importance: "CRITICAL - validates Flash unique feature"
    current_status: "NOT TESTED"

  2_medium_budget_mapping:
    description: "Budget range 10001-20000 → MEDIUM level"
    importance: "HIGH - ensures correct mapping logic"
    current_status: "NOT TESTED"
```

---

## 🔧 Technical Details

### Test Architecture

**Location**: `src-tauri/src/proxy/tests/thinking_models.rs`

**Test Framework**: Rust standard `#[test]` + custom fixtures

**Key Code References**:
- Budget-to-level mapping: `claude/request.rs:1450-1475`
- Level validation: `claude/request.rs:700-730`
- Flash detection: `openai/request.rs:247-250`

### Test 4: MEDIUM Level Exclusive Feature

**Test Name**: `test_gemini_3_flash_medium_level_exclusive`

**Purpose**: Validate MEDIUM level works ONLY on Flash

```rust
#[test]
fn test_gemini_3_flash_medium_level_exclusive() {
    // GIVEN: Claude request with MEDIUM-level budget (15000 tokens)
    let claude_request = create_claude_request_with_thinking(15000);

    // WHEN: Mapped to gemini-3-flash
    let flash_request = map_to_gemini("gemini-3-flash", claude_request.clone());
    let flash_level = extract_thinking_level(&flash_request);

    // THEN: Flash uses MEDIUM level
    assert_eq!(flash_level, "MEDIUM", "Flash should support MEDIUM level");

    // WHEN: Mapped to gemini-3-pro-high
    let pro_request = map_to_gemini("gemini-3-pro-high", claude_request.clone());
    let pro_level = extract_thinking_level(&pro_request);

    // THEN: Pro downgrades MEDIUM → LOW (doesn't support MEDIUM)
    assert_eq!(pro_level, "LOW", "Pro should downgrade MEDIUM to LOW");
}
```

**Expected Behavior**:
```yaml
flash_behavior:
  input_budget: 15000
  expected_level: "MEDIUM"
  reason: "Flash supports all 4 levels"

pro_behavior:
  input_budget: 15000
  expected_level: "LOW"  # NOT MEDIUM!
  reason: "Pro only supports LOW/HIGH, MEDIUM is downgraded"
```

### Test 5: MEDIUM Budget Mapping

**Test Name**: `test_gemini_3_flash_medium_budget_mapping`

**Purpose**: Verify budget range 10001-20000 maps to MEDIUM

```rust
#[test]
fn test_gemini_3_flash_medium_budget_mapping() {
    // Test budget range boundaries for MEDIUM level

    // GIVEN: Budget = 10000 (upper limit of LOW)
    let request_10k = create_claude_request_with_thinking(10000);
    let result_10k = map_to_gemini("gemini-3-flash", request_10k);
    assert_eq!(extract_thinking_level(&result_10k), "LOW",
        "Budget 10000 should map to LOW (not MEDIUM)");

    // GIVEN: Budget = 10001 (lower limit of MEDIUM)
    let request_10k1 = create_claude_request_with_thinking(10001);
    let result_10k1 = map_to_gemini("gemini-3-flash", request_10k1);
    assert_eq!(extract_thinking_level(&result_10k1), "MEDIUM",
        "Budget 10001 should map to MEDIUM");

    // GIVEN: Budget = 15000 (middle of MEDIUM range)
    let request_15k = create_claude_request_with_thinking(15000);
    let result_15k = map_to_gemini("gemini-3-flash", request_15k);
    assert_eq!(extract_thinking_level(&result_15k), "MEDIUM",
        "Budget 15000 should map to MEDIUM");

    // GIVEN: Budget = 20000 (upper limit of MEDIUM)
    let request_20k = create_claude_request_with_thinking(20000);
    let result_20k = map_to_gemini("gemini-3-flash", request_20k);
    assert_eq!(extract_thinking_level(&result_20k), "MEDIUM",
        "Budget 20000 should map to MEDIUM");

    // GIVEN: Budget = 20001 (lower limit of HIGH)
    let request_20k1 = create_claude_request_with_thinking(20001);
    let result_20k1 = map_to_gemini("gemini-3-flash", request_20k1);
    assert_eq!(extract_thinking_level(&result_20k1), "HIGH",
        "Budget 20001 should map to HIGH (not MEDIUM)");
}
```

**Budget Mapping Logic**:
```yaml
budget_ranges:
  0_to_4000:
    level: "MINIMAL"
    note: "Fastest, minimal thinking"

  4001_to_10000:
    level: "LOW"
    note: "Quick reasoning"

  10001_to_20000:
    level: "MEDIUM"  # ← Flash exclusive!
    note: "Balanced reasoning (Flash only)"

  20001_plus:
    level: "HIGH"
    note: "Maximum reasoning depth"
```

---

## ✅ Acceptance Criteria

### AC-1: MEDIUM Level Exclusive Test

```gherkin
GIVEN a Claude request with budget_tokens = 15000
WHEN mapped to "gemini-3-flash"
THEN thinkingConfig.thinkingLevel = "MEDIUM"

GIVEN the same request with budget_tokens = 15000
WHEN mapped to "gemini-3-pro-high"
THEN thinkingConfig.thinkingLevel = "LOW" (downgraded from MEDIUM)
```

**Verification**:
- ✅ Test passes for Flash (MEDIUM supported)
- ✅ Test passes for Pro (MEDIUM downgraded to LOW)
- ✅ No compilation errors
- ✅ Clippy warnings addressed

---

### AC-2: MEDIUM Budget Mapping Test

```gherkin
GIVEN budgets [10000, 10001, 15000, 20000, 20001]
WHEN each is mapped to "gemini-3-flash"
THEN levels are ["LOW", "MEDIUM", "MEDIUM", "MEDIUM", "HIGH"]
```

**Verification**:
- ✅ Boundary test: 10000 → LOW (not MEDIUM)
- ✅ Boundary test: 10001 → MEDIUM (not LOW)
- ✅ Mid-range test: 15000 → MEDIUM
- ✅ Boundary test: 20000 → MEDIUM (not HIGH)
- ✅ Boundary test: 20001 → HIGH (not MEDIUM)

---

### AC-3: Test Suite Integration

```gherkin
GIVEN all 5 thinking tests (3 existing + 2 new)
WHEN running "cargo test thinking_models"
THEN all 5 tests pass with 0 failures
```

**Verification**:
- ✅ Total tests: 5/5 passing (was 3/5)
- ✅ Test coverage: Complete for Flash thinking modes
- ✅ CI/CD integration: Tests run automatically
- ✅ Documentation: Test descriptions updated

---

### AC-4: Code Quality

```gherkin
GIVEN the new test code
WHEN running quality checks
THEN all checks pass
```

**Verification**:
- ✅ `cargo test` → All tests passing
- ✅ `cargo clippy` → No new warnings
- ✅ `cargo fmt --check` → Code formatted
- ✅ Code review → Tests follow existing patterns

---

## 🔍 Implementation Guide

### Step 1: Create Test Helper Functions

```rust
// Helper to create Claude request with specific thinking budget
fn create_claude_request_with_thinking(budget: u32) -> ClaudeRequest {
    ClaudeRequest {
        model: "claude-sonnet".to_string(),
        messages: vec![...],
        thinking: Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(budget),
        }),
        ..Default::default()
    }
}

// Helper to extract thinking level from Gemini request
fn extract_thinking_level(gemini_request: &Value) -> &str {
    gemini_request
        .get("generationConfig")
        .and_then(|gc| gc.get("thinkingConfig"))
        .and_then(|tc| tc.get("thinkingLevel"))
        .and_then(|level| level.as_str())
        .unwrap_or("NONE")
}
```

### Step 2: Implement Test 4

1. Create test function `test_gemini_3_flash_medium_level_exclusive`
2. Test Flash with budget 15000 → expect "MEDIUM"
3. Test Pro with budget 15000 → expect "LOW" (downgraded)
4. Add assertions and error messages
5. Run test: `cargo test test_gemini_3_flash_medium_level_exclusive`

### Step 3: Implement Test 5

1. Create test function `test_gemini_3_flash_medium_budget_mapping`
2. Test boundary cases: 10000, 10001, 20000, 20001
3. Test mid-range: 15000
4. Add descriptive assertions
5. Run test: `cargo test test_gemini_3_flash_medium_budget_mapping`

### Step 4: Verify Full Test Suite

```bash
# Run all thinking model tests
cargo test thinking_models

# Expected output:
# test test_gemini_3_flash_thinking_basic ... ok
# test test_gemini_3_flash_auto_injection ... ok
# test test_gemini_3_flash_level_mapping ... ok
# test test_gemini_3_flash_medium_level_exclusive ... ok
# test test_gemini_3_flash_medium_budget_mapping ... ok
#
# test result: ok. 5 passed; 0 failed; 0 ignored
```

### Step 5: Update Documentation

**File**: `src-tauri/src/proxy/tests/thinking_models.rs` (doc comments)

```rust
/// Flash Thinking Mode Test Suite (5/5 tests)
///
/// Epic-013 Story-013-01: Complete MEDIUM level test coverage
///
/// Tests:
/// 1. Basic thinking activation (Epic-011)
/// 2. OpenAI auto-injection (Epic-011)
/// 3. Level mapping basics (Epic-011)
/// 4. MEDIUM level exclusive to Flash (Epic-013) ← NEW
/// 5. MEDIUM budget range mapping (Epic-013) ← NEW
///
/// Coverage: 100% for Flash thinking capabilities
```

---

## 📊 Quality Gates

### QG-1: Test Execution

```bash
cargo test thinking_models --verbose
```

**Expected**:
- ✅ 5/5 tests passing
- ✅ 0 failures
- ✅ 0 ignored tests
- ✅ Execution time <5 seconds

---

### QG-2: Code Quality

```bash
cargo clippy --all-targets
cargo fmt --check
```

**Expected**:
- ✅ No clippy warnings for new code
- ✅ Code properly formatted
- ✅ No unused imports
- ✅ Proper error handling

---

### QG-3: Epic-011 Regression

```bash
cargo test --all
```

**Expected**:
- ✅ All 75 Epic-011 tests still passing
- ✅ No new test failures introduced
- ✅ Total test count: 75 → 77 tests

---

### QG-4: Documentation

**Check**:
- ✅ Test doc comments explain purpose
- ✅ Assertion messages are descriptive
- ✅ Test follows existing patterns
- ✅ Budget mapping ranges documented

---

## 🎯 Success Metrics

```yaml
test_coverage:
  before: "3/5 thinking tests (60%)"
  after: "5/5 thinking tests (100%)"
  improvement: "+40% coverage"

compliance_impact:
  flash_compliance: "85% → 88%"
  epic_013_progress: "Phase 2 - Story 1 of 4"

quality_assurance:
  regression_tests: "75/75 still passing"
  new_tests: "2/2 passing"
  total_tests: "77/77 passing (100%)"
```

---

## 🔗 Related Work

**Dependencies**:
- ✅ Epic-011 Complete (thinkingLevel API implemented)
- ✅ 75/75 tests passing (no blockers)

**Follow-up Stories**:
- Story-013-04: Error Logging (uses test insights)
- Story-013-06: Cost Analytics (monitors MEDIUM level usage)

**References**:
- Epic-011 Implementation: `docs/epics/EPIC-011-gemini-3-api-migration.md`
- COMPARISON Doc: `gemini-3-flash-COMPARISON.md:320-350`
- Code: `claude/request.rs:1450-1475` (budget mapping)

---

## 📝 Notes

### Why MEDIUM Level Matters

```yaml
flash_advantage:
  levels: 4 (MINIMAL, LOW, MEDIUM, HIGH)
  pro_levels: 2 (LOW, HIGH)
  unique_feature: "MEDIUM is Flash-exclusive"

business_value:
  cost_optimization: "MEDIUM cheaper than HIGH, better than LOW"
  use_cases:
    - "Balanced reasoning for moderate complexity tasks"
    - "Cost-effective alternative to HIGH for budget-conscious users"
    - "Flash differentiation from Pro models"

testing_importance:
  - "Validates Flash unique capability"
  - "Ensures Pro correctly downgrades MEDIUM → LOW"
  - "Confirms budget mapping accuracy (10001-20000 range)"
```

### Test Maintenance

- Tests are **deterministic** (no network calls, no randomness)
- Tests are **fast** (<5 seconds total)
- Tests are **isolated** (no shared state)
- Tests follow **existing patterns** in thinking_models.rs

---

**Story Owner**: QA Team + Backend Developer
**Reviewers**: Tech Lead, Test Engineer
**Estimated Effort**: 1-2 days (8-16 hours)
**Actual Effort**: _TBD after completion_

**Status**: 📋 READY FOR DEVELOPMENT
**Next Step**: Assign to developer, start implementation
