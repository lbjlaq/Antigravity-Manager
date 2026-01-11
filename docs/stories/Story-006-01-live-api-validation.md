# Story-006-01: Live API Validation - Gemini 2.5 Flash Lite Thinking

**Story ID**: Story-006-01
**Epic**: [Epic-006](../epics/Epic-006-Gemini-2.5-Flash-Lite-Thinking-Optimizations.md) - Gemini 2.5 Flash Lite Thinking - Optimizations & Intelligence
**Priority**: P0 (Critical) - Validation Foundation
**Estimated Effort**: 1 hour
**Type**: VALIDATION + DOCS
**Status**: To Do
**Created**: 2026-01-11
**Owner**: Backend Dev / QA Team

**Sequential Position**: Story #1 of 6 (FIRST - Foundation - BLOCKS ALL)
**Blocks**: Story-006-02, Story-006-03, Story-006-04, Story-006-05, Story-006-06

---

## User Story

**As a** system architect responsible for model compliance confidence
**I want** live API validation of gemini-2.5-flash-lite-thinking model behavior
**So that** we confirm 95% pattern-based inference is correct and achieve 100% confidence before implementing optimizations

---

## Context

### Current Situation

**Pattern-Based Inference** (95% Confidence):

```yaml
# From: gemini-2.5-flash-lite-thinking-COMPARISON.md:469-495
discovery_approach:
  method: "Pattern matching code analysis"
  logic: "'gemini-2.5-flash-lite'.contains('gemini-2.5-flash') → 24576 budget"
  confidence: "95%"

confidence_factors:
  code_logic_clear: "100% (unambiguous pattern match)"
  base_model_confirmed: "100% (model_mapping.rs:49)"
  thinking_pattern_verified: "100% (request.rs:1440-1442)"

uncertainty_sources:
  - "No explicit 'gemini-2.5-flash-lite-thinking' string in code"
  - "Inferred behavior, not direct implementation"
  - "Cannot test without live API access"
```

**Code Evidence** (Pattern Matching):
```rust
// src-tauri/src/proxy/mappers/claude/request.rs:1440-1442
if mapped_model.contains("gemini-2.5-flash") {
    budget.min(24576)  // Flash thinking budget limit
}

// Logic inference:
// "gemini-2.5-flash-lite" contains "gemini-2.5-flash" → TRUE
// Therefore: budget = min(requested, 24576)
```

**Why Validation Needed**:
- ❌ **No Explicit Reference**: No hardcoded "gemini-2.5-flash-lite-thinking" in codebase
- ❌ **Untested Assumption**: Pattern matching never validated with live API
- ❌ **Risk for Epic**: All 5 optimization stories depend on this assumption
- ❌ **Resource Efficiency**: Could waste 11 hours if model doesn't exist or behaves differently

**Current Compliance**: 91.2% (all gaps are P3 optimizations)
**Current Confidence**: 95% (pattern-based inference)

### Expected Behavior After Validation

**Live API Validation** (100% Confidence):

```yaml
validation_goals:
  model_existence: "Confirm gemini-2.5-flash-lite-thinking accepts requests"
  thinking_support: "Verify thinking blocks in response"
  budget_limit: "Validate 24576 budget cap applies"
  pattern_correctness: "Confirm pattern matching logic accurate"

success_criteria:
  - Model accepts thinkingConfig parameter
  - Response includes thinking blocks (if budget > 0)
  - Budget values 1-24576 work correctly
  - Budget > 24576 capped at 24576
  - Response quality within expected range

confidence_boost: "95% → 100%"
risk_mitigation: "Blocks other stories if validation fails"
```

**After Validation**:
- ✅ **100% Confidence**: Live API evidence confirms behavior
- ✅ **Risk Mitigation**: Know model works before building optimizations
- ✅ **Documentation**: Evidence for all future reference
- ✅ **Foundation**: Solid base for Stories 002-006

### Gap Analysis

**Source**: Epic-006 Analysis

```yaml
gap: "No live API validation of gemini-2.5-flash-lite-thinking"
current_state:
  compliance: "91.2%"
  confidence: "95% (pattern-based)"
  implementation: "Inferred via pattern matching"

target_state:
  compliance: "91.2% (unchanged - validation doesn't add features)"
  confidence: "100% (live API evidence)"
  implementation: "Validated via live API testing"

why_critical:
  blocking_nature: "All 5 optimization stories depend on this model existing"
  resource_protection: "Prevents wasting 11 hours if model doesn't work"
  confidence_requirement: "100% confidence needed for production optimizations"
  risk_mitigation: "Early detection of model issues"

effort: "LOW (1 hour) - Simple API test script"
priority: "P0 - CRITICAL BLOCKER"
```

---

## Reference Documentation

### Primary Sources

1. **COMPARISON Document**: `docs/antigravity/workflows/models/gemini/gemini-2.5-flash-lite-thinking-COMPARISON.md`
   - Lines 469-495: Pattern matching discovery method
   - Lines 32-66: Overall compliance summary (91.2%)
   - Lines 469-495: Confidence assessment (95%)

2. **Epic-006 Document**: `docs/epics/Epic-006-Gemini-2.5-Flash-Lite-Thinking-Optimizations.md`
   - Story-006-01 definition (lines 67-89)
   - Sequential execution dependency (lines 344-354)
   - Validation requirement justification

### Code References

**Pattern Matching Logic**:
- `src-tauri/src/proxy/common/model_mapping.rs:49` - Base model mapping
- `src-tauri/src/proxy/mappers/claude/request.rs:1440-1442` - Budget limit logic
- `src-tauri/src/proxy/mappers/claude/request.rs:1412-1514` - Thinking config transformation

**Test Infrastructure**:
- `src-tauri/tests/` - Integration test location
- Existing model validation patterns from Epic-003, Epic-004, Epic-005

### Related Models

**Similar Patterns**:
- `gemini-2.5-flash-thinking` - Flash thinking base (24576 budget)
- `gemini-2.5-flash-lite` - Lite base (no thinking)
- `gemini-3-pro-high-thinking` - Pro thinking (32000 budget)

---

## Technical Details

### Validation Architecture

**Three-Phase Validation**:

1. **Model Existence Test**: Confirm model accepts requests
2. **Thinking Capability Test**: Verify thinking blocks in response
3. **Budget Limit Test**: Validate 24576 budget cap

### Phase 1: Model Existence Test (15 minutes)

**Objective**: Confirm gemini-2.5-flash-lite-thinking is valid model

**Test Script** (Rust integration test):

```rust
// tests/model_validation/gemini_flash_lite_thinking_test.rs

use antigravity_tools::proxy::handlers::claude::handle_claude_request;
use antigravity_tools::models::ClaudeRequest;

#[tokio::test]
async fn test_gemini_flash_lite_thinking_model_exists() {
    // AC-1: Model accepts requests without error
    let request = ClaudeRequest {
        model: "gemini-2.5-flash-lite-thinking".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String("What is 2+2?".to_string()),
        }],
        max_tokens: Some(100),
        thinking: Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(500),
        }),
        ..Default::default()
    };

    let result = handle_claude_request(request).await;

    assert!(result.is_ok(), "Model should accept requests");

    let response = result.unwrap();
    assert!(response.content.len() > 0, "Should return content");
    assert_ne!(response.model, "", "Should return model name");
}
```

**Expected Outcome**:
- ✅ Request succeeds (no 404 or model not found error)
- ✅ Response contains content
- ✅ Model name in response matches request

**Fallback**: If test fails, document as "Model not available" and BLOCK entire Epic-006

---

### Phase 2: Thinking Capability Test (20 minutes)

**Objective**: Verify thinking blocks present in response

**Test Script**:

```rust
#[tokio::test]
async fn test_gemini_flash_lite_thinking_capability() {
    // AC-2: Thinking blocks appear in response
    let request = ClaudeRequest {
        model: "gemini-2.5-flash-lite-thinking".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String(
                "Solve this step by step: What is the square root of 144?"
            ).to_string(),
        }],
        max_tokens: Some(500),
        thinking: Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(2000),  // Request substantial thinking
        }),
        ..Default::default()
    };

    let result = handle_claude_request(request).await;
    assert!(result.is_ok(), "Request should succeed");

    let response = result.unwrap();

    // Verify thinking block exists
    let has_thinking = response.content.iter().any(|block| {
        matches!(block, ContentBlock::Thinking { .. })
    });

    assert!(has_thinking, "Response should contain thinking blocks");

    // Verify usage includes input_tokens_details
    assert!(response.usage.is_some(), "Should include usage stats");
    let usage = response.usage.unwrap();
    assert!(
        usage.input_tokens_details.is_some(),
        "Should include thinking token details"
    );
}
```

**Expected Outcome**:
- ✅ Response contains ContentBlock::Thinking
- ✅ Usage stats include input_tokens_details (thinking tokens)
- ✅ Thinking content is non-empty

---

### Phase 3: Budget Limit Test (25 minutes)

**Objective**: Validate 24576 budget cap applies correctly

**Test Script**:

```rust
#[tokio::test]
async fn test_gemini_flash_lite_thinking_budget_limits() {
    // AC-3: Budget capped at 24576

    // Test 1: Budget below limit (should use requested)
    let request_low = create_test_request(2000);
    let response_low = handle_claude_request(request_low).await.unwrap();
    let thinking_tokens_low = extract_thinking_tokens(&response_low);
    assert!(
        thinking_tokens_low <= 2000,
        "Should respect budget below limit"
    );

    // Test 2: Budget at limit (should work)
    let request_max = create_test_request(24576);
    let response_max = handle_claude_request(request_max).await.unwrap();
    let thinking_tokens_max = extract_thinking_tokens(&response_max);
    assert!(
        thinking_tokens_max <= 24576,
        "Should respect 24576 limit"
    );

    // Test 3: Budget above limit (should cap at 24576)
    let request_high = create_test_request(32000);  // Request Pro limit
    let response_high = handle_claude_request(request_high).await.unwrap();
    let thinking_tokens_high = extract_thinking_tokens(&response_high);
    assert!(
        thinking_tokens_high <= 24576,
        "Should cap at 24576, not allow 32000"
    );
}

fn create_test_request(budget: u32) -> ClaudeRequest {
    ClaudeRequest {
        model: "gemini-2.5-flash-lite-thinking".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: MessageContent::String(
                "Analyze the complexity of merge sort algorithm."
            ).to_string(),
        }],
        max_tokens: Some(1000),
        thinking: Some(ThinkingConfig {
            type_: "enabled".to_string(),
            budget_tokens: Some(budget),
        }),
        ..Default::default()
    }
}

fn extract_thinking_tokens(response: &ClaudeResponse) -> u32 {
    response
        .usage
        .as_ref()
        .and_then(|u| u.input_tokens_details.as_ref())
        .and_then(|d| d.thinking_tokens)
        .unwrap_or(0)
}
```

**Expected Outcome**:
- ✅ Budget 2000: Uses ~2000 thinking tokens
- ✅ Budget 24576: Uses up to 24576 (works at limit)
- ✅ Budget 32000: Capped at 24576 (not Pro's 32000)

**Critical Validation**: Budget 32000 test proves pattern matching correct (lite uses 24576, not Pro's 32000)

---

## Acceptance Criteria

### AC-1: Model Existence Confirmed

**Requirement**: gemini-2.5-flash-lite-thinking accepts API requests without model-not-found errors

**Verification**:
```yaml
test_method: "Integration test - Phase 1"
request:
  model: "gemini-2.5-flash-lite-thinking"
  messages: [{"role": "user", "content": "What is 2+2?"}]
  thinking: {"type": "enabled", "budget_tokens": 500}

expected_result:
  http_status: 200
  response_content: "Non-empty content array"
  model_name: "gemini-2.5-flash-lite-thinking"
  no_errors: true

failure_handling:
  if_404: "BLOCK entire Epic-006, document model unavailable"
  if_other_error: "Investigate error, determine if blocking"
```

**Pass Criteria**:
- ✅ HTTP 200 response
- ✅ Response contains content blocks
- ✅ No "model not found" or "invalid model" errors
- ✅ Model name in response matches request

---

### AC-2: Thinking Capability Validated

**Requirement**: Model returns thinking blocks when thinkingConfig provided

**Verification**:
```yaml
test_method: "Integration test - Phase 2"
request:
  model: "gemini-2.5-flash-lite-thinking"
  prompt: "Solve step by step: square root of 144"
  thinking_budget: 2000

expected_result:
  thinking_blocks: "Present in content array"
  thinking_tokens: ">0 in usage.input_tokens_details.thinking_tokens"
  content_quality: "Shows step-by-step reasoning"

validation_points:
  - ContentBlock::Thinking exists in response.content
  - usage.input_tokens_details.thinking_tokens > 0
  - Thinking content is coherent and non-empty
```

**Pass Criteria**:
- ✅ Response contains ContentBlock::Thinking
- ✅ thinking_tokens > 0 in usage stats
- ✅ Thinking content demonstrates reasoning process
- ✅ Quality comparable to gemini-2.5-flash-thinking (base)

---

### AC-3: Budget Limit Confirmed as 24576

**Requirement**: Pattern matching prediction confirmed - budget capped at 24576 (Flash limit), not 32000 (Pro limit)

**Verification**:
```yaml
test_method: "Integration test - Phase 3 (3 subtests)"

subtest_1_below_limit:
  budget_requested: 2000
  expected_behavior: "Uses ~2000 tokens"
  validates: "Respects budget when below limit"

subtest_2_at_limit:
  budget_requested: 24576
  expected_behavior: "Uses up to 24576 tokens"
  validates: "Limit is 24576, not lower"

subtest_3_above_limit:
  budget_requested: 32000  # Request Pro limit
  expected_behavior: "Capped at 24576, not 32000"
  validates: "Pattern matching correct - uses Flash limit"

critical_validation:
  key_test: "subtest_3_above_limit"
  proves: "gemini-2.5-flash-lite-thinking uses Flash budget (24576)"
  disproves: "NOT Pro budget (32000)"
  confirms: "Pattern matching logic accurate"
```

**Pass Criteria**:
- ✅ Subtest 1: thinking_tokens ≤ 2000
- ✅ Subtest 2: thinking_tokens ≤ 24576 (and potentially close to limit)
- ✅ Subtest 3: thinking_tokens ≤ 24576 (NOT close to 32000)
- ✅ Pattern matching prediction confirmed

**Critical**: Subtest 3 is THE validation - if it allows 32000, pattern matching is wrong

---

### AC-4: Validation Report Documented

**Requirement**: Comprehensive validation report with evidence and confidence update

**Report Structure**:
```yaml
# docs/validation/gemini-2.5-flash-lite-thinking-validation-report.md

validation_date: "YYYY-MM-DD"
model: "gemini-2.5-flash-lite-thinking"

test_results:
  model_existence:
    status: "PASS | FAIL"
    evidence: "Screenshot or API response JSON"

  thinking_capability:
    status: "PASS | FAIL"
    thinking_blocks_present: true | false
    thinking_tokens_used: "Numeric value"

  budget_limit:
    budget_2000: "PASS - Uses ~2000 tokens"
    budget_24576: "PASS - Works at limit"
    budget_32000: "PASS - Capped at 24576 (NOT 32000)"

conclusion:
  pattern_matching_accurate: true | false
  confidence_before: "95%"
  confidence_after: "100%"
  epic_006_decision: "PROCEED | BLOCK"

evidence_attachments:
  - API request/response logs
  - Usage statistics screenshots
  - Budget capping proof
```

**Pass Criteria**:
- ✅ Report created in docs/validation/
- ✅ All 3 test phases documented
- ✅ Evidence included (API responses, screenshots)
- ✅ Confidence updated: 95% → 100%
- ✅ Clear GO/NO-GO decision for Epic-006

---

### AC-5: COMPARISON Document Updated

**Requirement**: Update confidence and validation status in COMPARISON document

**File**: `docs/antigravity/workflows/models/gemini/gemini-2.5-flash-lite-thinking-COMPARISON.md`

**Updates Required**:

**Section 1: Overall Compliance** (lines 32-66)
```yaml
# BEFORE:
overall_confidence_level: "95% (Pattern-based inference, not explicit code reference)"

# AFTER:
overall_confidence_level: "100% (Live API validation confirms pattern matching accurate)"
validation_date: "YYYY-MM-DD"
validation_evidence: "docs/validation/gemini-2.5-flash-lite-thinking-validation-report.md"
```

**Section 2: Discovery Method** (lines 469-495)
```yaml
# ADD:
live_validation:
  date: "YYYY-MM-DD"
  method: "Integration tests - Story-006-01"
  results:
    model_exists: true
    thinking_supported: true
    budget_limit: 24576  # Confirmed, not inferred
  confidence_boost: "95% → 100%"
  evidence: "docs/validation/gemini-2.5-flash-lite-thinking-validation-report.md"
```

**Pass Criteria**:
- ✅ Confidence level updated to 100%
- ✅ Validation date added
- ✅ Live validation section added
- ✅ Reference to validation report included

---

## Implementation Steps

### Step 1: Test Infrastructure Setup (10 minutes)

**Create Test Directory**:
```bash
mkdir -p src-tauri/tests/model_validation
```

**Create Test File**:
```bash
touch src-tauri/tests/model_validation/gemini_flash_lite_thinking_test.rs
```

**Update Cargo.toml** (if needed):
```toml
[[test]]
name = "model_validation"
path = "tests/model_validation/mod.rs"
```

---

### Step 2: Write Integration Tests (20 minutes)

**Implement Three Test Functions**:
1. `test_gemini_flash_lite_thinking_model_exists()` - AC-1
2. `test_gemini_flash_lite_thinking_capability()` - AC-2
3. `test_gemini_flash_lite_thinking_budget_limits()` - AC-3

**Helper Functions**:
- `create_test_request(budget: u32) -> ClaudeRequest`
- `extract_thinking_tokens(response: &ClaudeResponse) -> u32`

**Error Handling**:
```rust
#[tokio::test]
async fn test_model_validation_with_fallback() {
    match handle_claude_request(request).await {
        Ok(response) => {
            // Validation logic
        }
        Err(e) if e.contains("model not found") => {
            // CRITICAL: Model doesn't exist
            panic!("BLOCKING: gemini-2.5-flash-lite-thinking not available");
        }
        Err(e) => {
            // Other errors - investigate
            eprintln!("Validation error: {}", e);
        }
    }
}
```

---

### Step 3: Execute Tests (15 minutes)

**Run Tests**:
```bash
# Run validation tests
cargo test --test model_validation -- --test-threads=1

# Expected output:
# test test_gemini_flash_lite_thinking_model_exists ... ok
# test test_gemini_flash_lite_thinking_capability ... ok
# test test_gemini_flash_lite_thinking_budget_limits ... ok
```

**Capture Evidence**:
- Screenshot test output showing "ok" status
- Save API request/response logs
- Record thinking token usage for each test

**Decision Point**:
- ✅ **All Pass**: Proceed to Step 4 (documentation)
- ❌ **Any Fail**: BLOCK Epic-006, document failure, escalate

---

### Step 4: Create Validation Report (10 minutes)

**Report Template**:
```bash
mkdir -p docs/validation
touch docs/validation/gemini-2.5-flash-lite-thinking-validation-report.md
```

**Report Content** (from AC-4 structure):
- Validation date and model name
- Test results for all 3 phases
- Evidence (logs, screenshots)
- Confidence assessment (95% → 100%)
- GO/NO-GO decision for Epic-006

---

### Step 5: Update COMPARISON Document (5 minutes)

**Update Two Sections**:
1. Overall compliance (lines 32-66) - Add confidence boost
2. Discovery method (lines 469-495) - Add live validation details

**Commit Message**:
```bash
git commit -m "docs(story-006-01): validate gemini-2.5-flash-lite-thinking via live API

- Confirm model existence (AC-1)
- Validate thinking capability (AC-2)
- Verify 24576 budget limit (AC-3)
- Boost confidence 95% → 100%
- Documentation: validation report + COMPARISON update

Ref: docs/stories/Story-006-01-live-api-validation.md"
```

---

## Definition of Done

Story-006-01 is **COMPLETE** when:

### Test Requirements
- ✅ Integration test file created: `tests/model_validation/gemini_flash_lite_thinking_test.rs`
- ✅ 3 test functions implemented (model existence, thinking capability, budget limits)
- ✅ All tests passing (`cargo test --test model_validation`)
- ✅ Test evidence captured (screenshots, logs)

### Validation Requirements
- ✅ Model existence confirmed (AC-1)
- ✅ Thinking capability validated (AC-2)
- ✅ Budget limit verified as 24576 (AC-3)
- ✅ Pattern matching accuracy confirmed

### Documentation Requirements
- ✅ Validation report created: `docs/validation/gemini-2.5-flash-lite-thinking-validation-report.md`
- ✅ COMPARISON document updated (confidence 95% → 100%)
- ✅ Evidence included (API responses, usage stats)
- ✅ Story status updated to "✅ IMPLEMENTED"

### Decision Requirements
- ✅ GO/NO-GO decision documented for Epic-006
- ✅ If GO: Unblock Stories 002-006
- ✅ If NO-GO: Block Epic-006, document reason

---

## Dependencies

### Upstream Dependencies (Must Exist)
- ✅ Epic-006: Epic document defines validation requirement
- ✅ COMPARISON: 91.2% compliance, 95% confidence documented
- ✅ Pattern matching code: request.rs:1440-1442 contains logic
- ✅ Live API access: Requires valid account with quota

### Downstream Dependencies (Blocked Until Complete)
- ⏳ Story-006-02: Adaptive Budget Adjustment (BLOCKED)
- ⏳ Story-006-03: Quality Ceiling Detection (BLOCKED)
- ⏳ Story-006-04: Budget Analytics Dashboard (BLOCKED)
- ⏳ Story-006-05: Quality Metrics Dashboard (BLOCKED)
- ⏳ Story-006-06: Documentation Consolidation (BLOCKED)

### Critical Path
- **MUST BE FIRST**: Foundation for all Epic-006 optimizations
- **BLOCKS ALL**: No optimization work until validation passes
- **Epic Success Factor**: If validation fails, entire Epic-006 blocked

---

## Risks & Mitigations

### Risk 1: Model Does Not Exist 🔴

**Risk**: gemini-2.5-flash-lite-thinking may not be valid model ID

**Probability**: LOW (15%) - Pattern matching logic seems sound

**Impact**: CRITICAL - Blocks entire Epic-006 (11 hours wasted)

**Mitigation Strategy**:
```yaml
detection:
  error_type: "404 or 'model not found'"
  test_phase: "Phase 1 - Model Existence Test"

immediate_action:
  1. "STOP - Do not proceed with Epic-006"
  2. "Document model unavailable in validation report"
  3. "Update Epic-006 status to BLOCKED"
  4. "Notify stakeholders"

investigation:
  check_model_catalog: "Verify model exists in Gemini catalog"
  check_naming: "Try variations (gemini-2.5-flash-lite-exp-thinking?)"
  check_availability: "Confirm model released (not preview-only)"

fallback_options:
  option_1: "Use gemini-2.5-flash-thinking (base) instead"
  option_2: "Wait for model release"
  option_3: "Cancel Epic-006 as not applicable"
```

---

### Risk 2: Budget Limit Different Than 24576 🟡

**Risk**: Pattern matching wrong - budget may be different (e.g., 32000, 16384, or 8192)

**Probability**: LOW (10%) - Pattern matching logic clear, but untested

**Impact**: MEDIUM - Stories 002-003 need adjustment, not blocking

**Mitigation Strategy**:
```yaml
detection:
  test: "Phase 3 - Subtest 3 (budget 32000)"
  expected: "Capped at 24576"
  actual_if_wrong: "Different limit (e.g., 32000, 16384, 8192)"

if_budget_32000:
  finding: "Model uses Pro budget, not Flash budget"
  implication: "Pattern matching assumption incorrect"
  action:
    - Update COMPARISON: budget_limit = 32000
    - Adjust Story-006-02: Use 32000 limit
    - Proceed with Epic-006 (not blocking)

if_budget_16384:
  finding: "Model has lower limit than Flash"
  implication: "Lite variant has reduced budget"
  action:
    - Update COMPARISON: budget_limit = 16384
    - Adjust Story-006-02: Use 16384 limit
    - Story-006-03 more valuable (earlier ceiling)

if_budget_8192:
  finding: "Model has very low limit"
  implication: "Lite variant severely constrained"
  action:
    - Update COMPARISON: budget_limit = 8192
    - Re-evaluate Epic-006 value proposition
    - Consider whether optimizations worth effort
```

---

### Risk 3: Thinking Quality Unexpectedly Poor 🟡

**Risk**: Model produces low-quality thinking output despite correct implementation

**Probability**: LOW (5%) - Lite should match Flash quality

**Impact**: MEDIUM - Affects Story-006-03 value, not blocking

**Mitigation Strategy**:
```yaml
detection:
  method: "Manual review of thinking blocks in Phase 2"
  indicators:
    - Incoherent reasoning
    - Repetitive content
    - Lower quality than Flash base

action_if_detected:
  1. "Document quality concerns in validation report"
  2. "Proceed with Epic-006 (not blocking)"
  3. "Adjust Story-006-03 expectations"
  4. "Consider quality ceiling lower than expected"

impact_on_epic:
  story_002: "No impact - budget optimization independent"
  story_003: "Adjust quality ceiling threshold"
  story_004_005: "Analytics still valuable"
  story_006: "Document quality limitations"
```

---

### Risk 4: API Rate Limits During Testing 🟢

**Risk**: Testing consumes quota and hits rate limits

**Probability**: MEDIUM (30%) - 3 tests with substantial thinking budgets

**Impact**: LOW - Delays validation by hours, not days

**Mitigation Strategy**:
```yaml
prevention:
  - Use low budgets where possible (Phase 1: 500 tokens)
  - Implement test throttling (wait between tests)
  - Use test account with quota

if_rate_limited:
  immediate:
    - Wait for rate limit reset (typically 1 minute)
    - Use different account if available
    - Reduce test thinking budgets

  fallback:
    - Run tests over longer period (spread across hours)
    - Use cached responses if available
    - Document rate limit impact in report
```

---

## Testing Strategy

### Test Pyramid

```yaml
integration_tests:
  count: 3
  focus: "Live API behavior validation"
  coverage:
    - Model existence (AC-1)
    - Thinking capability (AC-2)
    - Budget limits (AC-3)

manual_validation:
  count: 1
  focus: "Evidence review and quality assessment"
  coverage: "Validation report accuracy"
```

### Test Execution Plan

**Pre-Validation**:
```bash
# Ensure test infrastructure ready
cargo build --tests

# Verify test account has quota
# Check API credentials configured
```

**Validation Execution**:
```bash
# Run all validation tests
cargo test --test model_validation -- --test-threads=1 --nocapture

# Expected duration: 3-5 minutes
# Token usage: ~5000 thinking tokens + responses
```

**Post-Validation**:
```bash
# Review test output
# Capture evidence (screenshots, logs)
# Create validation report
# Update COMPARISON document
```

---

## Success Metrics

### Confidence Metrics

```yaml
before_story:
  confidence: "95%"
  basis: "Pattern matching code analysis"
  uncertainty: "No live API validation"
  risk: "Entire Epic-006 based on assumption"

after_story:
  confidence: "100%"
  basis: "Live API validation with evidence"
  certainty: "Model confirmed working as expected"
  risk: "Zero - validated foundation"

improvement:
  confidence_boost: "+5% (95% → 100%)"
  risk_reduction: "100% (eliminated assumption risk)"
  epic_security: "Solid foundation for 11 hours of work"
```

### Time Metrics

```yaml
validation_investment:
  story_effort: "1 hour"

risk_mitigation_value:
  potential_waste_prevented: "11 hours (if model doesn't exist)"
  roi: "1100% (11h saved / 1h invested)"

blocking_relationship:
  stories_unblocked: 5
  total_unblocked_effort: "11 hours"
```

### Quality Metrics

```yaml
epic_foundation:
  before: "Risky - 95% confidence assumption"
  after: "Solid - 100% validated foundation"

development_confidence:
  before: "Uncertain - 'Is this model real?'"
  after: "Certain - 'Model works exactly as expected'"
```

---

## Documentation Updates

### Files to Create

**1. Validation Report**: `docs/validation/gemini-2.5-flash-lite-thinking-validation-report.md`
- Test results for all 3 phases
- Evidence (API logs, screenshots)
- Confidence assessment
- GO/NO-GO decision

**2. Test File**: `src-tauri/tests/model_validation/gemini_flash_lite_thinking_test.rs`
- Integration tests (3 functions)
- Helper functions (2 functions)

### Files to Update

**1. COMPARISON Document**: `docs/antigravity/workflows/models/gemini/gemini-2.5-flash-lite-thinking-COMPARISON.md`
- Overall confidence: 95% → 100%
- Live validation section added
- Evidence reference added

**2. This Story**: `docs/stories/Story-006-01-live-api-validation.md`
- Add validation evidence section
- Update status to ✅ IMPLEMENTED
- Document actual results

**3. Epic-006**: `docs/epics/Epic-006-Gemini-2.5-Flash-Lite-Thinking-Optimizations.md`
- Update Story-006-01 status
- Update progress tracker (0/6 → 1/6)

---

## Cross-References

### Related Stories

**Epic-003** (Pattern Reference):
- Story-003-01: Model validation pattern

**Epic-005** (Validation Pattern):
- Story-005-01: Model ID discovery and validation

**Epic-006** (This Epic - ALL STORIES DEPEND):
- Story-006-02: Adaptive Budget (BLOCKED until validated)
- Story-006-03: Quality Ceiling (BLOCKED until validated)
- Story-006-04: Budget Analytics (BLOCKED until validated)
- Story-006-05: Quality Metrics (BLOCKED until validated)
- Story-006-06: Documentation (BLOCKED until validated)

---

## Implementation Notes

### Test Environment Setup

**Required Resources**:
- Live API access to Gemini 2.5 models
- Test account with available quota (≥10K tokens)
- Integration test infrastructure

**Environment Variables**:
```bash
# API credentials (already configured in Antigravity)
# No additional setup needed beyond standard dev environment
```

### Test Execution Tips

**Tip 1: Throttle Tests**
```rust
// Add delays between tests to avoid rate limits
tokio::time::sleep(Duration::from_secs(5)).await;
```

**Tip 2: Capture Rich Evidence**
```rust
// Log full request/response for evidence
println!("Request: {}", serde_json::to_string_pretty(&request)?);
println!("Response: {}", serde_json::to_string_pretty(&response)?);
```

**Tip 3: Use Descriptive Assertions**
```rust
assert!(
    thinking_tokens <= 24576,
    "Budget should cap at 24576 (Flash limit), got {} tokens",
    thinking_tokens
);
```

### Common Pitfalls

**Avoid**:
- ❌ Running tests in parallel (may hit rate limits)
- ❌ Using production account (consume real quota)
- ❌ Skipping evidence capture (can't prove validation)
- ❌ Hardcoding expected token counts (vary by model/prompt)

**Ensure**:
- ✅ Use test account with quota buffer
- ✅ Run tests sequentially with delays
- ✅ Capture full request/response logs
- ✅ Use threshold checks, not exact matches

---

## Questions & Answers

### Q1: What if validation takes longer than 1 hour?

**A**: Extend timeline if needed - validation is CRITICAL:
- Validation more important than timeline
- Better to spend 2 hours validating than waste 11 hours on wrong assumption
- Update Epic-006 timeline accordingly

### Q2: Should we validate ALL Gemini 2.5 Flash Lite variants?

**A**: NO - Only gemini-2.5-flash-lite-thinking:
- This story focuses on ONE model (thinking variant)
- Base variant (gemini-2.5-flash-lite) already validated in previous epics
- Other variants out of scope for Epic-006

### Q3: What if budget limit is different than expected?

**A**: Document actual limit, adjust Epic-006 plans:
- Update COMPARISON document with actual limit
- Adjust Story-006-02 (Adaptive Budget) to use correct limit
- Adjust Story-006-03 (Quality Ceiling) threshold if needed
- NOT BLOCKING - proceed with Epic-006

### Q4: How much quota will validation consume?

**A**: Approximately 5K-8K tokens:
- Phase 1: ~500 tokens (simple prompt)
- Phase 2: ~2000 tokens (reasoning prompt)
- Phase 3: ~2000 tokens × 3 subtests = ~6000 tokens
- **Total**: ~8500 tokens input + output

### Q5: Can we use cached responses instead of live API?

**A**: NO - Live API required for true validation:
- Cached responses don't prove model exists TODAY
- API may change behavior between cache and validation
- 100% confidence requires real-time evidence
- Test account quota should handle 8K tokens

---

## Story Status

**Current Status**: To Do
**Next Story**: Story-006-02 (Adaptive Budget Adjustment) - BLOCKED UNTIL THIS COMPLETES
**Epic Progress**: 0/6 stories complete (0%)

**Critical Path**: This story is THE foundation - all 5 other stories depend on it.

---

**Created**: 2026-01-11
**Last Updated**: 2026-01-11
**Story Points**: 1
**Estimated Hours**: 1 (15m + 20m + 25m setup/execution + documentation)
**Token Budget**: ~8,500 tokens for validation tests
