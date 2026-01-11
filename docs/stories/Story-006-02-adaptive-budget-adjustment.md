# Story-006-02: Adaptive Budget Adjustment - Smart Cost Optimization

**Story ID**: Story-006-02
**Epic**: [Epic-006](../epics/Epic-006-Gemini-2.5-Flash-Lite-Thinking-Optimizations.md) - Gemini 2.5 Flash Lite Thinking - Optimizations & Intelligence
**Priority**: P3 (Low) - Cost Optimization
**Estimated Effort**: 3 hours
**Type**: CODE (Backend - Request Mapper)
**Status**: To Do
**Created**: 2026-01-11
**Owner**: Backend Dev

**Sequential Position**: Story #2 of 6 (Smart Features Phase)
**Depends On**: Story-006-01 (Live API Validation) - BLOCKED UNTIL COMPLETE
**Blocks**: Story-006-06 (Documentation) - Needs feature to document

---

## User Story

**As a** cost-conscious API user using gemini-2.5-flash-lite-thinking
**I want** automatic budget optimization based on prompt complexity
**So that** I minimize thinking token costs while maintaining quality for complex tasks

---

## Context

### Current Situation

**Static Budget Allocation** (Cost Inefficient):

```yaml
# Current behavior: User manually specifies thinking budget
current_implementation:
  method: "Manual static budget in request"
  user_controls: "thinkingConfig.budget_tokens parameter"
  system_behavior: "Use exactly what user requests (up to 24576 cap)"

problems:
  over_provisioning:
    scenario: "Simple 'What is 2+2?' with budget=10000"
    waste: "Uses minimal tokens, but budget reserved"
    impact: "Cost inefficiency, quota waste"

  under_provisioning:
    scenario: "Complex algorithm analysis with budget=2000"
    result: "Insufficient thinking depth"
    impact: "Quality degradation"

  no_intelligence:
    issue: "System doesn't adjust budget based on actual task complexity"
    consequence: "User responsible for budget tuning per request"
```

**Example Inefficiency**:
```json
// Simple prompt with excessive budget
{
  "model": "gemini-2.5-flash-lite-thinking",
  "messages": [{"role": "user", "content": "What is the capital of France?"}],
  "thinking": {"type": "enabled", "budget_tokens": 8000}
}
// Reality: Uses ~200 tokens, wasted 7800 tokens of budget reservation
```

**Cost Impact**:
- Users over-provision budgets to be safe
- Simple queries waste 50-80% of allocated budget
- No automatic optimization guidance
- Manual tuning required per request type

### Expected Behavior After Story

**Intelligent Adaptive Budgeting** (Cost Optimized):

```yaml
adaptive_system:
  method: "Automatic complexity-based budget adjustment"
  intelligence: "Classify prompt → Adjust budget automatically"
  system_behavior: "Use optimal budget for task complexity"

complexity_levels:
  SIMPLE:
    examples: ["What is 2+2?", "Define photosynthesis"]
    characteristics:
      - Factual questions
      - Simple calculations
      - Definition queries
    optimal_budget: 500-2000
    typical_usage: "~500 tokens"

  MODERATE:
    examples: ["Explain quantum computing", "Compare sorting algorithms"]
    characteristics:
      - Explanation requests
      - Comparison tasks
      - Multi-step reasoning
    optimal_budget: 2000-8000
    typical_usage: "~3000 tokens"

  COMPLEX:
    examples: ["Design system architecture", "Debug algorithm edge case"]
    characteristics:
      - Design tasks
      - Code review
      - Deep analysis
    optimal_budget: 8000-16000
    typical_usage: "~10000 tokens"

  DEEP:
    examples: ["Prove mathematical theorem", "Distributed consensus algorithm"]
    characteristics:
      - Theoretical proofs
      - Novel algorithm design
      - Research-level reasoning
    optimal_budget: 16000-24576
    typical_usage: "~20000 tokens"

automatic_adjustment:
  user_request: "budget_tokens = null OR unspecified"
  system_action:
    1. "Analyze prompt complexity"
    2. "Classify into SIMPLE/MODERATE/COMPLEX/DEEP"
    3. "Assign optimal budget for that level"
    4. "Apply budget in upstream request"

cost_savings:
  simple_queries: "50-80% reduction (8000 → 1500)"
  moderate_queries: "20-40% reduction (10000 → 6000)"
  complex_queries: "10-20% optimization"
  deep_queries: "Already near maximum, minimal change"
  overall_target: "≥15% cost reduction across all requests"
```

**User Control Preserved**:
```yaml
explicit_budget:
  if_user_specifies: "thinkingConfig.budget_tokens = 5000"
  system_behavior: "Use user's value exactly (no override)"
  adaptive_disabled: true

automatic_budget:
  if_user_omits: "thinkingConfig.budget_tokens = null"
  system_behavior: "Activate adaptive budget classifier"
  adaptive_enabled: true
```

### Gap Analysis

**Source**: gemini-2.5-flash-lite-thinking-COMPARISON.md (P3 Gap #1)

```yaml
gap: "No automatic budget optimization based on task complexity"
priority: P3
current_state:
  budget_control: "Manual - user specifies or system default"
  intelligence: "None - no complexity analysis"
  optimization: "Static budgets regardless of task"

target_state:
  budget_control: "Intelligent - auto-adjust when unspecified"
  intelligence: "Complexity classifier (4 levels)"
  optimization: "Dynamic budgets matching task requirements"

why_valuable:
  cost_savings: "15-25% reduction in thinking token costs"
  user_experience: "Eliminates manual budget tuning"
  quality_preservation: "Sufficient budget for complex tasks"
  resource_efficiency: "Optimal quota utilization"

effort: "MEDIUM (3 hours)"
priority_rationale: "P3 (optimization, not critical), but high ROI"
```

**Cost Savings Calculation**:
```yaml
scenario_analysis:
  baseline_usage:
    simple_queries_40%:
      current_budget: 8000
      actual_usage: 500
      waste: 7500  # 93.75% waste

    moderate_queries_35%:
      current_budget: 10000
      actual_usage: 3000
      waste: 7000  # 70% waste

    complex_queries_20%:
      current_budget: 12000
      actual_usage: 10000
      waste: 2000  # 16.67% waste

    deep_queries_5%:
      current_budget: 20000
      actual_usage: 18000
      waste: 2000  # 10% waste

  with_adaptive_budgeting:
    simple_queries_40%:
      adaptive_budget: 1500
      actual_usage: 500
      waste: 1000  # 66.67% waste (still overhead, but acceptable)

    moderate_queries_35%:
      adaptive_budget: 6000
      actual_usage: 3000
      waste: 3000  # 50% waste (reduced from 70%)

    complex_queries_20%:
      adaptive_budget: 12000
      actual_usage: 10000
      waste: 2000  # 16.67% waste (unchanged, already optimal)

    deep_queries_5%:
      adaptive_budget: 22000
      actual_usage: 18000
      waste: 4000  # 18.18% waste (slight increase for safety margin)

  cost_savings:
    simple: "6500 tokens saved per query (81.25% reduction)"
    moderate: "4000 tokens saved per query (40% reduction)"
    complex: "0 tokens saved (already optimal)"
    deep: "-2000 tokens (small increase for headroom)"
    weighted_average: "~20% cost reduction overall"
```

---

## Reference Documentation

### Primary Sources

1. **Epic-006 Document**: `docs/epics/Epic-006-Gemini-2.5-Flash-Lite-Thinking-Optimizations.md`
   - Story-006-02 definition (lines 91-108)
   - Adaptive budget logic examples (lines 415-438)

2. **COMPARISON Document**: `docs/antigravity/workflows/models/gemini/gemini-2.5-flash-lite-thinking-COMPARISON.md`
   - P3 Gap #1: Adaptive Budget Adjustment (lines TBD)

3. **Budget Limit Reference**: Story-006-01 validation results
   - Confirmed budget cap: 24576
   - Thinking token behavior validated

### Code References

**Existing Implementation**:
- `src-tauri/src/proxy/mappers/claude/request.rs:1412-1514` - Thinking config transformation
- `src-tauri/src/proxy/mappers/claude/request.rs:1440-1442` - Budget limit logic (24576 cap)
- `src-tauri/src/proxy/mappers/claude/request.rs:578` - Request body construction

**Integration Points**:
- Thinking config parsing (existing)
- Budget application (existing - extend with classifier)
- Model-specific budget limits (existing - reuse 24576 cap)

### Related Patterns

**Similar Optimizations**:
- Auto-stream conversion (converts non-streaming → streaming for optimization)
- Rate limit retry logic (intelligent account rotation)
- Token manager quota weighting (complexity-aware account selection)

---

## Technical Details

### Implementation Architecture

**Four-Component System**:

1. **Complexity Classifier**: Analyze prompt → Assign complexity level
2. **Budget Mapper**: Map complexity level → Optimal budget
3. **Request Interceptor**: Apply adaptive budget when user doesn't specify
4. **Telemetry Tracker**: Measure actual vs. allocated budget for tuning

### Component 1: Complexity Classifier (1 hour)

**Objective**: Classify prompts into SIMPLE/MODERATE/COMPLEX/DEEP

**Classification Algorithm** (Heuristic-Based):

```rust
// src-tauri/src/proxy/mappers/claude/complexity_classifier.rs (NEW FILE)

use crate::models::Message;

/// Complexity levels for adaptive budget allocation
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ComplexityLevel {
    Simple,    // Factual questions, simple calculations
    Moderate,  // Explanations, comparisons, multi-step reasoning
    Complex,   // Design, code review, deep analysis
    Deep,      // Theoretical proofs, novel algorithms, research
}

impl ComplexityLevel {
    /// Get optimal budget range for this complexity level
    pub fn optimal_budget(&self) -> (u32, u32) {
        match self {
            Self::Simple => (500, 2000),
            Self::Moderate => (2000, 8000),
            Self::Complex => (8000, 16000),
            Self::Deep => (16000, 24576),
        }
    }

    /// Get recommended budget (midpoint of range)
    pub fn recommended_budget(&self) -> u32 {
        let (min, max) = self.optimal_budget();
        (min + max) / 2
    }
}

/// Complexity classifier using heuristic rules
pub struct ComplexityClassifier;

impl ComplexityClassifier {
    /// Classify a prompt's complexity level
    pub fn classify(messages: &[Message]) -> ComplexityLevel {
        // Extract full prompt text
        let prompt = Self::extract_prompt_text(messages);

        // Score various complexity indicators
        let scores = ComplexityScores {
            prompt_length: Self::score_length(&prompt),
            keyword_complexity: Self::score_keywords(&prompt),
            structural_complexity: Self::score_structure(&prompt),
            semantic_complexity: Self::score_semantics(&prompt),
        };

        // Aggregate scores into final complexity level
        Self::aggregate_scores(scores)
    }

    /// Extract full prompt text from messages
    fn extract_prompt_text(messages: &[Message]) -> String {
        messages
            .iter()
            .filter(|m| m.role == "user")
            .map(|m| match &m.content {
                MessageContent::String(s) => s.as_str(),
                MessageContent::Array(blocks) => {
                    blocks
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Score based on prompt length (tokens estimated)
    fn score_length(prompt: &str) -> f32 {
        let estimated_tokens = prompt.split_whitespace().count() as f32 * 1.3;
        match estimated_tokens as u32 {
            0..=50 => 0.2,      // Very short - likely simple
            51..=150 => 0.4,    // Short - moderate
            151..=400 => 0.6,   // Medium - complex
            401..=1000 => 0.8,  // Long - deep
            _ => 1.0,           // Very long - deep
        }
    }

    /// Score based on complexity keywords
    fn score_keywords(prompt: &str) -> f32 {
        let prompt_lower = prompt.to_lowercase();

        // Simple indicators (0.2)
        let simple_keywords = ["what is", "define", "who is", "when did", "capital of"];
        if simple_keywords.iter().any(|&kw| prompt_lower.contains(kw)) {
            return 0.2;
        }

        // Moderate indicators (0.4-0.6)
        let moderate_keywords = [
            "explain", "compare", "difference between", "how does",
            "summarize", "list", "steps to",
        ];
        if moderate_keywords.iter().any(|&kw| prompt_lower.contains(kw)) {
            return 0.5;
        }

        // Complex indicators (0.6-0.8)
        let complex_keywords = [
            "design", "implement", "analyze", "debug", "optimize",
            "architecture", "review", "refactor",
        ];
        if complex_keywords.iter().any(|&kw| prompt_lower.contains(kw)) {
            return 0.7;
        }

        // Deep indicators (0.8-1.0)
        let deep_keywords = [
            "prove", "theorem", "algorithm", "distributed", "consensus",
            "formal", "mathematical", "research", "novel",
        ];
        if deep_keywords.iter().any(|&kw| prompt_lower.contains(kw)) {
            return 0.9;
        }

        // Default: moderate
        0.5
    }

    /// Score based on structural complexity
    fn score_structure(prompt: &str) -> f32 {
        let mut score = 0.0;

        // Code blocks indicate complexity
        if prompt.contains("```") {
            score += 0.3;
        }

        // Multiple questions indicate complexity
        let question_count = prompt.matches('?').count();
        score += (question_count as f32 * 0.1).min(0.3);

        // Lists/steps indicate moderate-complex
        if prompt.contains("1.") || prompt.contains("- ") {
            score += 0.2;
        }

        score.min(1.0)
    }

    /// Score based on semantic complexity
    fn score_semantics(prompt: &str) -> f32 {
        let prompt_lower = prompt.to_lowercase();

        // Technical domains (higher complexity)
        let technical_domains = [
            "algorithm", "architecture", "distributed", "concurrent",
            "optimization", "cryptography", "machine learning",
        ];
        let domain_matches = technical_domains
            .iter()
            .filter(|&&term| prompt_lower.contains(term))
            .count();

        (domain_matches as f32 * 0.2).min(0.6)
    }

    /// Aggregate individual scores into final complexity level
    fn aggregate_scores(scores: ComplexityScores) -> ComplexityLevel {
        // Weighted average of scores
        let final_score = scores.prompt_length * 0.3
            + scores.keyword_complexity * 0.4
            + scores.structural_complexity * 0.2
            + scores.semantic_complexity * 0.1;

        // Map score to complexity level
        match final_score {
            x if x < 0.35 => ComplexityLevel::Simple,
            x if x < 0.65 => ComplexityLevel::Moderate,
            x if x < 0.85 => ComplexityLevel::Complex,
            _ => ComplexityLevel::Deep,
        }
    }
}

struct ComplexityScores {
    prompt_length: f32,
    keyword_complexity: f32,
    structural_complexity: f32,
    semantic_complexity: f32,
}
```

**Classification Examples**:
```rust
// SIMPLE (score ~0.2)
"What is 2+2?"
"Define photosynthesis"
"Who is the president of France?"

// MODERATE (score ~0.5)
"Explain how neural networks learn"
"Compare merge sort and quick sort algorithms"
"List the steps to deploy a Docker container"

// COMPLEX (score ~0.7)
"Design a scalable microservices architecture for e-commerce"
"Review this Rust code and suggest optimizations: ```rust..."
"Debug why this algorithm has O(n²) complexity instead of O(n log n)"

// DEEP (score ~0.9)
"Prove the correctness of the Paxos consensus algorithm"
"Design a novel algorithm for distributed leader election with Byzantine fault tolerance"
"Formally verify the memory safety of this lock-free data structure"
```

**Accuracy Target**: ≥70% classification accuracy (acceptable for cost optimization)

---

### Component 2: Budget Mapper (30 minutes)

**Objective**: Map complexity level to optimal budget value

**Implementation**:

```rust
// Add to complexity_classifier.rs

impl ComplexityClassifier {
    /// Get adaptive budget for messages (if user didn't specify)
    pub fn get_adaptive_budget(messages: &[Message]) -> u32 {
        let complexity = Self::classify(messages);
        complexity.recommended_budget()
    }
}
```

**Budget Mapping Table**:
```yaml
Simple:
  range: 500-2000
  recommended: 1250
  rationale: "Factual queries need minimal thinking"

Moderate:
  range: 2000-8000
  recommended: 5000
  rationale: "Explanations need moderate reasoning"

Complex:
  range: 8000-16000
  recommended: 12000
  rationale: "Design/analysis needs deep thinking"

Deep:
  range: 16000-24576
  recommended: 20288
  rationale: "Theoretical work needs maximum thinking"
```

---

### Component 3: Request Interceptor (1 hour)

**Objective**: Apply adaptive budget when user doesn't specify explicit budget

**File**: `src-tauri/src/proxy/mappers/claude/request.rs`

**Location**: In `transform_claude_request_in()` function, around line 1412 (thinking config processing)

**Code Changes**:

```rust
// BEFORE (existing code):
// src-tauri/src/proxy/mappers/claude/request.rs:1412-1514

if let Some(thinking) = &req.thinking {
    // ... existing thinking config processing ...
    if let Some(budget) = thinking.budget_tokens {
        // User specified budget - use it
        thinking_budget = budget.min(max_budget);
    }
}

// AFTER (with adaptive budgeting):
use crate::proxy::mappers::claude::complexity_classifier::{ComplexityClassifier, ComplexityLevel};

if let Some(thinking) = &req.thinking {
    // ... existing thinking config processing ...

    if let Some(budget) = thinking.budget_tokens {
        // ✅ User explicitly specified budget - respect it (no override)
        thinking_budget = budget.min(max_budget);

        tracing::debug!(
            "Using user-specified thinking budget: {} (capped at {})",
            budget,
            max_budget
        );
    } else {
        // 🆕 User didn't specify budget - use adaptive budget
        let adaptive_budget = ComplexityClassifier::get_adaptive_budget(&req.messages);
        thinking_budget = adaptive_budget.min(max_budget);

        let complexity = ComplexityClassifier::classify(&req.messages);

        tracing::info!(
            "Applied adaptive thinking budget: {} (complexity: {:?}, max: {})",
            thinking_budget,
            complexity,
            max_budget
        );

        // 🆕 Telemetry: Track adaptive budget usage
        if let Some(project_name) = project {
            crate::proxy::telemetry::record_adaptive_budget(
                project_name,
                &config.final_model,
                complexity,
                thinking_budget,
            );
        }
    }
}
```

**Control Flow Logic**:
```yaml
scenario_1_explicit_budget:
  user_request: "thinking.budget_tokens = 5000"
  system_behavior: "Use 5000 (user's explicit value)"
  adaptive_disabled: true
  log_message: "Using user-specified budget: 5000"

scenario_2_null_budget:
  user_request: "thinking.budget_tokens = null"
  system_behavior:
    1. "Classify prompt complexity (SIMPLE/MODERATE/COMPLEX/DEEP)"
    2. "Get recommended budget for that level"
    3. "Apply budget, cap at 24576"
  adaptive_enabled: true
  log_message: "Applied adaptive budget: 5000 (complexity: Moderate)"

scenario_3_no_thinking_config:
  user_request: "thinking = null"
  system_behavior: "No thinking budget applied (thinking disabled)"
  adaptive_disabled: true
  log_message: "Thinking not requested"
```

---

### Component 4: Telemetry Tracker (30 minutes)

**Objective**: Measure actual vs. allocated budget to validate savings

**New File**: `src-tauri/src/proxy/telemetry.rs`

```rust
// src-tauri/src/proxy/telemetry.rs (NEW FILE or add to existing monitor.rs)

use crate::proxy::mappers::claude::complexity_classifier::ComplexityLevel;
use std::sync::Mutex;
use lazy_static::lazy_static;

lazy_static! {
    static ref ADAPTIVE_BUDGET_STATS: Mutex<AdaptiveBudgetStats> =
        Mutex::new(AdaptiveBudgetStats::default());
}

#[derive(Default)]
struct AdaptiveBudgetStats {
    requests_by_complexity: HashMap<ComplexityLevel, ComplexityStats>,
}

struct ComplexityStats {
    request_count: u32,
    total_allocated_budget: u64,
    total_actual_usage: u64,
}

/// Record adaptive budget usage for telemetry
pub fn record_adaptive_budget(
    project: &str,
    model: &str,
    complexity: ComplexityLevel,
    allocated_budget: u32,
) {
    let mut stats = ADAPTIVE_BUDGET_STATS.lock().unwrap();

    let entry = stats
        .requests_by_complexity
        .entry(complexity)
        .or_insert(ComplexityStats {
            request_count: 0,
            total_allocated_budget: 0,
            total_actual_usage: 0,
        });

    entry.request_count += 1;
    entry.total_allocated_budget += allocated_budget as u64;

    tracing::debug!(
        "Adaptive budget allocated: project={}, model={}, complexity={:?}, budget={}",
        project,
        model,
        complexity,
        allocated_budget
    );
}

/// Record actual thinking token usage (called after response received)
pub fn record_actual_usage(
    complexity: ComplexityLevel,
    actual_tokens: u32,
) {
    let mut stats = ADAPTIVE_BUDGET_STATS.lock().unwrap();

    if let Some(entry) = stats.requests_by_complexity.get_mut(&complexity) {
        entry.total_actual_usage += actual_tokens as u64;
    }
}

/// Get adaptive budget efficiency metrics
pub fn get_efficiency_metrics() -> AdaptiveEfficiencyMetrics {
    let stats = ADAPTIVE_BUDGET_STATS.lock().unwrap();

    let mut metrics = AdaptiveEfficiencyMetrics::default();

    for (complexity, stats) in &stats.requests_by_complexity {
        let allocated = stats.total_allocated_budget as f32;
        let actual = stats.total_actual_usage as f32;
        let efficiency = if allocated > 0.0 {
            (actual / allocated) * 100.0
        } else {
            0.0
        };

        metrics.by_complexity.insert(
            *complexity,
            EfficiencyReport {
                request_count: stats.request_count,
                total_allocated: stats.total_allocated_budget,
                total_actual: stats.total_actual_usage,
                efficiency_percent: efficiency,
            },
        );
    }

    metrics
}
```

**Telemetry Usage**:
```rust
// In response handling code (after receiving response with usage stats)
if let Some(usage) = &response.usage {
    if let Some(details) = &usage.input_tokens_details {
        if let Some(thinking_tokens) = details.thinking_tokens {
            // Record actual usage for efficiency tracking
            telemetry::record_actual_usage(complexity, thinking_tokens);
        }
    }
}
```

---

## Acceptance Criteria

### AC-1: Complexity Classifier Implemented

**Requirement**: Four-level complexity classifier (SIMPLE/MODERATE/COMPLEX/DEEP) with ≥70% accuracy

**Verification**:
```yaml
test_method: "Unit tests with example prompts"

test_cases:
  simple_prompts:
    examples:
      - "What is 2+2?"
      - "Define photosynthesis"
      - "Capital of France?"
    expected_classification: ComplexityLevel::Simple
    expected_budget_range: 500-2000

  moderate_prompts:
    examples:
      - "Explain neural networks"
      - "Compare merge sort vs quick sort"
      - "How does React useState work?"
    expected_classification: ComplexityLevel::Moderate
    expected_budget_range: 2000-8000

  complex_prompts:
    examples:
      - "Design microservices architecture"
      - "Review Rust code: ```rust fn example() {...}```"
      - "Debug O(n²) complexity issue"
    expected_classification: ComplexityLevel::Complex
    expected_budget_range: 8000-16000

  deep_prompts:
    examples:
      - "Prove Paxos consensus correctness"
      - "Design Byzantine fault-tolerant algorithm"
      - "Formal verification of lock-free data structure"
    expected_classification: ComplexityLevel::Deep
    expected_budget_range: 16000-24576

accuracy_target: "≥70% (14/20 correct classifications minimum)"
```

**Pass Criteria**:
- ✅ Classifier returns correct ComplexityLevel for ≥70% of test cases
- ✅ Budget ranges match expected values
- ✅ No crashes or panics on edge cases (empty prompts, very long prompts)

---

### AC-2: Adaptive Budget Applied When Unspecified

**Requirement**: System automatically applies adaptive budget when user doesn't specify explicit budget

**Verification**:
```yaml
test_method: "Integration tests"

test_1_explicit_budget:
  request:
    model: "gemini-2.5-flash-lite-thinking"
    messages: [{"role": "user", "content": "Simple question"}]
    thinking: {"type": "enabled", "budget_tokens": 10000}
  expected_behavior:
    budget_used: 10000  # User's explicit value
    adaptive_disabled: true
  verification: "Check request logs show 'Using user-specified budget: 10000'"

test_2_null_budget_simple:
  request:
    model: "gemini-2.5-flash-lite-thinking"
    messages: [{"role": "user", "content": "What is 2+2?"}]
    thinking: {"type": "enabled", "budget_tokens": null}
  expected_behavior:
    complexity_detected: "Simple"
    budget_applied: 1250  # Adaptive budget
    adaptive_enabled: true
  verification: "Check request logs show 'Applied adaptive budget: 1250 (complexity: Simple)'"

test_3_null_budget_complex:
  request:
    model: "gemini-2.5-flash-lite-thinking"
    messages: [{"role": "user", "content": "Design scalable microservices..."}]
    thinking: {"type": "enabled", "budget_tokens": null}
  expected_behavior:
    complexity_detected: "Complex"
    budget_applied: 12000  # Adaptive budget
    adaptive_enabled: true
  verification: "Check request logs show 'Applied adaptive budget: 12000 (complexity: Complex)'"
```

**Pass Criteria**:
- ✅ Explicit budget preserved (no override)
- ✅ Null budget triggers adaptive classification
- ✅ Adaptive budget matches complexity level
- ✅ Logs clearly indicate adaptive vs explicit budget

---

### AC-3: Cost Savings ≥15% Measured

**Requirement**: Telemetry shows ≥15% cost reduction compared to baseline static budgets

**Verification**:
```yaml
test_method: "Production telemetry analysis (1 week sample)"

baseline_scenario:
  # Previous behavior: Users manually specify budgets
  simple_queries_40%: "Average budget 8000, actual usage 500"
  moderate_queries_35%: "Average budget 10000, actual usage 3000"
  complex_queries_20%: "Average budget 12000, actual usage 10000"
  deep_queries_5%: "Average budget 20000, actual usage 18000"

  total_allocated: "100 requests × avg 9400 = 940,000 tokens"
  total_actual: "100 requests × avg 3950 = 395,000 tokens"
  efficiency: "42% (58% waste)"

adaptive_scenario:
  simple_queries_40%: "Adaptive budget 1250, actual usage 500"
  moderate_queries_35%: "Adaptive budget 5000, actual usage 3000"
  complex_queries_20%: "Adaptive budget 12000, actual usage 10000"
  deep_queries_5%: "Adaptive budget 20288, actual usage 18000"

  total_allocated: "100 requests × avg 5214 = 521,400 tokens"
  total_actual: "100 requests × avg 3950 = 395,000 tokens"
  efficiency: "76% (24% waste)"

savings_calculation:
  baseline_allocated: 940000
  adaptive_allocated: 521400
  absolute_savings: 418600  # Tokens saved
  percentage_savings: "44.5%"  # Exceeds 15% target

verification_query:
  # Run telemetry API to get efficiency metrics
  # GET /api/proxy/telemetry/adaptive-budget-efficiency
  expected_response:
    overall_efficiency: "≥60%"  # Up from 42%
    cost_reduction: "≥15%"  # Minimum target
    requests_analyzed: "≥100"  # Sufficient sample size
```

**Pass Criteria**:
- ✅ Overall cost reduction ≥15% vs. baseline
- ✅ Efficiency improves from ~42% to ≥60%
- ✅ All complexity levels show appropriate budget allocation
- ✅ No quality degradation (separate validation in Story-006-03)

---

### AC-4: User Control Preserved

**Requirement**: Explicit user budgets always respected, never overridden by adaptive system

**Verification**:
```yaml
test_method: "Unit and integration tests"

test_scenarios:
  explicit_low_budget:
    user_specifies: 500
    adaptive_suggests: 12000  # Complex prompt
    system_uses: 500  # User's value
    rationale: "User wants to save costs"

  explicit_high_budget:
    user_specifies: 24000
    adaptive_suggests: 1250  # Simple prompt
    system_uses: 24000  # User's value (capped at 24576)
    rationale: "User wants maximum quality"

  explicit_zero_budget:
    user_specifies: 0
    adaptive_suggests: 5000
    system_uses: 0  # Effectively disables thinking
    rationale: "User explicitly disables thinking"

  null_budget:
    user_specifies: null
    adaptive_suggests: 8000
    system_uses: 8000  # Adaptive value
    rationale: "User delegates to system"
```

**Pass Criteria**:
- ✅ Explicit budgets never overridden (0, low, high, or at cap)
- ✅ Only null budgets trigger adaptive classification
- ✅ Documentation clearly explains user control
- ✅ Logs distinguish explicit vs adaptive budgets

---

### AC-5: Telemetry Dashboard Integration

**Requirement**: Adaptive budget metrics visible in monitoring dashboard

**Dashboard Widget**:
```typescript
// src/components/proxy/AdaptiveBudgetMetrics.tsx (NEW COMPONENT)

interface AdaptiveBudgetMetrics {
  by_complexity: {
    Simple: EfficiencyReport;
    Moderate: EfficiencyReport;
    Complex: EfficiencyReport;
    Deep: EfficiencyReport;
  };
  overall_efficiency: number;
  cost_reduction_percent: number;
}

interface EfficiencyReport {
  request_count: number;
  total_allocated: number;
  total_actual: number;
  efficiency_percent: number;
}

// Display:
// - Bar chart: Budget allocated vs actual usage by complexity
// - Efficiency percentage by complexity level
// - Overall cost savings metric (% reduction)
// - Request distribution pie chart
```

**API Endpoint**:
```rust
// src-tauri/src/proxy/handlers/telemetry.rs
#[get("/api/proxy/telemetry/adaptive-budget-efficiency")]
async fn get_adaptive_budget_efficiency() -> Json<AdaptiveEfficiencyMetrics> {
    Json(telemetry::get_efficiency_metrics())
}
```

**Pass Criteria**:
- ✅ Dashboard widget displays adaptive budget metrics
- ✅ Efficiency percentages calculated correctly
- ✅ Cost reduction metric shows ≥15% savings
- ✅ Real-time updates as requests processed

---

## Implementation Steps

### Phase 1: Complexity Classifier (1.5 hours)

**Step 1.1**: Create new module
```bash
touch src-tauri/src/proxy/mappers/claude/complexity_classifier.rs
```

**Step 1.2**: Implement ComplexityLevel enum
- Define 4 levels: Simple, Moderate, Complex, Deep
- Add optimal_budget() and recommended_budget() methods

**Step 1.3**: Implement ComplexityClassifier struct
- Extract prompt text from messages
- Implement 4 scoring functions (length, keywords, structure, semantics)
- Aggregate scores into final complexity level

**Step 1.4**: Write unit tests
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_classify_simple_prompts() {
        let messages = vec![/* simple prompt */];
        let complexity = ComplexityClassifier::classify(&messages);
        assert_eq!(complexity, ComplexityLevel::Simple);
    }

    // Test all 4 levels × 3 examples each = 12 tests
}
```

**Step 1.5**: Validate accuracy
- Run tests, ensure ≥70% accuracy
- Tune thresholds if needed

---

### Phase 2: Request Integration (1 hour)

**Step 2.1**: Import classifier in request.rs
```rust
use crate::proxy::mappers::claude::complexity_classifier::{ComplexityClassifier, ComplexityLevel};
```

**Step 2.2**: Modify thinking config logic
- Add condition: if budget_tokens is None → use adaptive
- Apply ComplexityClassifier::get_adaptive_budget()
- Log adaptive budget application

**Step 2.3**: Add telemetry call
- Record adaptive budget usage
- Pass complexity level, budget value

**Step 2.4**: Test integration
```bash
cargo test test_adaptive_budget_integration
```

---

### Phase 3: Telemetry & Dashboard (30 minutes)

**Step 3.1**: Create telemetry module
- Implement AdaptiveBudgetStats struct
- Add record_adaptive_budget() function
- Add get_efficiency_metrics() function

**Step 3.2**: Create API endpoint
```rust
#[get("/api/proxy/telemetry/adaptive-budget-efficiency")]
async fn get_adaptive_budget_efficiency() -> Json<AdaptiveEfficiencyMetrics>
```

**Step 3.3**: Create dashboard widget (optional - can defer to Story-006-04)
- Bar chart component
- Efficiency metrics display

---

## Definition of Done

Story-006-02 is **COMPLETE** when:

### Code Requirements
- ✅ ComplexityClassifier implemented (complexity_classifier.rs)
- ✅ Four complexity levels defined with budget mappings
- ✅ Adaptive budget integration in request.rs
- ✅ Telemetry tracking implemented
- ✅ Code formatted (`cargo fmt`) and linted (`cargo clippy`)

### Testing Requirements
- ✅ Unit tests: ≥12 tests (3 per complexity level)
- ✅ Integration tests: 3 tests (explicit, null simple, null complex)
- ✅ Accuracy validation: ≥70% classification accuracy
- ✅ All tests passing (`cargo test`)

### Performance Requirements
- ✅ Cost savings ≥15% measured via telemetry
- ✅ Efficiency improves from ~42% to ≥60%
- ✅ No performance degradation (classification <5ms overhead)

### Documentation Requirements
- ✅ Code comments explain classification logic
- ✅ Story status updated to "✅ IMPLEMENTED"
- ✅ Telemetry documentation added
- ✅ User guide section (optional - can defer to Story-006-06)

---

## Dependencies

### Upstream Dependencies (Must Complete)
- ✅ Story-006-01: Live API Validation (BLOCKING - confirms 24576 limit)

### Downstream Dependencies (Will Benefit)
- ⏳ Story-006-03: Quality Ceiling Detection (uses complexity classifier)
- ⏳ Story-006-04: Budget Analytics Dashboard (displays efficiency metrics)
- ⏳ Story-006-06: Documentation (documents adaptive budget feature)

### Parallel Work (Can Run Concurrently)
- ⏳ Story-006-03: Quality Ceiling Detection (independent feature)

---

## Risks & Mitigations

### Risk 1: Classification Accuracy <70% 🟡

**Risk**: Heuristic classifier may not achieve 70% accuracy target

**Probability**: MEDIUM (30%)

**Impact**: MEDIUM - Suboptimal budgets, but not breaking

**Mitigation**:
```yaml
if_accuracy_60_70%:
  action: "Accept as acceptable for cost optimization"
  rationale: "Even 60% accuracy provides cost savings"

if_accuracy_50_60%:
  action: "Tune thresholds and keyword lists"
  approach:
    - Analyze misclassifications
    - Adjust score weights
    - Add more keywords

if_accuracy_<50%:
  action: "Simplify to 2-level classifier (Simple vs Complex)"
  fallback:
    - Simple: ≤5000 tokens budget
    - Complex: ≥10000 tokens budget
  reduced_savings: "~10% instead of 15%"
```

---

### Risk 2: Performance Overhead 🟢

**Risk**: Classification adds latency to request processing

**Probability**: LOW (10%)

**Impact**: LOW - Acceptable if <10ms overhead

**Mitigation**:
```yaml
target_latency: "<5ms per classification"

if_exceeds_5ms:
  optimization_1: "Cache classification for repeated prompts"
  optimization_2: "Simplify scoring functions"
  optimization_3: "Precompute keyword sets"

if_exceeds_10ms:
  fallback: "Make adaptive budget opt-in via config flag"
  default: "Disabled for performance-sensitive users"
```

---

### Risk 3: User Confusion About Adaptive Behavior 🟡

**Risk**: Users don't understand when adaptive budget is used vs explicit budget

**Probability**: MEDIUM (40%)

**Impact**: LOW - Confusion, not breaking

**Mitigation**:
```yaml
clarity_measures:
  1. "Clear logging: 'Applied adaptive budget' vs 'Using user-specified budget'"
  2. "Documentation: Explain null budget triggers adaptive"
  3. "API response header: X-Adaptive-Budget-Applied: true"
  4. "Dashboard: Show adaptive vs explicit budget requests"

if_confusion_persists:
  option_1: "Add config flag: enable_adaptive_budget (default: true)"
  option_2: "Add request field: auto_optimize_budget: true"
  option_3: "Document prominently in API docs"
```

---

## Testing Strategy

### Test Pyramid

```yaml
unit_tests:
  count: 16
  focus:
    - Complexity classification (12 tests - 3 per level)
    - Budget mapping (4 tests - 1 per level)
  coverage: "ComplexityClassifier, ComplexityLevel"

integration_tests:
  count: 3
  focus:
    - Explicit budget preserved
    - Null budget triggers adaptive
    - Budget cap applied correctly
  coverage: "Request transformation with adaptive budget"

telemetry_tests:
  count: 2
  focus:
    - Adaptive budget recorded
    - Efficiency metrics calculated
  coverage: "Telemetry module"

performance_tests:
  count: 1
  focus: "Classification latency <5ms"
  coverage: "ComplexityClassifier performance"
```

### Test Execution Plan

**Pre-Implementation**:
```bash
# Baseline: No adaptive budget tests exist
cargo test | grep adaptive  # Should find 0 tests
```

**Post-Implementation**:
```bash
# Run all adaptive budget tests
cargo test adaptive_budget -- --nocapture

# Expected: 22 tests passing
# - 16 unit tests (classification + budget mapping)
# - 3 integration tests (explicit/null/cap)
# - 2 telemetry tests
# - 1 performance test
```

**Accuracy Validation**:
```bash
# Run classification accuracy test
cargo test test_classification_accuracy -- --nocapture

# Expected output:
# Correct: 17/20 (85% accuracy)
# Target: ≥14/20 (70% accuracy)
# Status: PASS
```

---

## Success Metrics

### Cost Metrics

```yaml
before_story:
  average_budget_allocation: "~9400 tokens per request"
  average_actual_usage: "~3950 tokens per request"
  efficiency: "42%"
  waste: "58%"

after_story:
  average_budget_allocation: "~5214 tokens per request"
  average_actual_usage: "~3950 tokens per request"
  efficiency: "76%"
  waste: "24%"

improvement:
  cost_reduction: "44.5% (exceeds 15% target)"
  efficiency_gain: "+34 percentage points (42% → 76%)"
  tokens_saved: "~4200 tokens per request"
```

### Accuracy Metrics

```yaml
classification_accuracy:
  target: "≥70%"
  actual: "TBD after testing"
  acceptable_range: "60-90%"

budget_appropriateness:
  simple_queries: "90% use ≤2000 tokens"
  moderate_queries: "80% use ≤8000 tokens"
  complex_queries: "70% use ≤16000 tokens"
  deep_queries: "90% use ≤24576 tokens"
```

### Performance Metrics

```yaml
latency:
  classification_overhead: "<5ms"
  request_processing_impact: "<2%"

throughput:
  no_degradation: "Requests per second unchanged"
```

---

## Documentation Updates

### Files to Create

**1. Complexity Classifier Module**: `src-tauri/src/proxy/mappers/claude/complexity_classifier.rs`
- ComplexityLevel enum
- ComplexityClassifier struct
- Classification algorithms

**2. Telemetry Module**: `src-tauri/src/proxy/telemetry.rs` (or extend monitor.rs)
- Adaptive budget tracking
- Efficiency metrics

**3. Unit Tests**: Add to complexity_classifier.rs
- 16 classification and budget mapping tests

**4. Integration Tests**: Add to request.rs tests
- 3 adaptive budget integration tests

### Files to Update

**1. Request Mapper**: `src-tauri/src/proxy/mappers/claude/request.rs`
- Import ComplexityClassifier
- Add adaptive budget logic (lines ~1412-1514)
- Add telemetry calls

**2. This Story**: `docs/stories/Story-006-02-adaptive-budget-adjustment.md`
- Add classification accuracy results
- Add telemetry data
- Update status to ✅ IMPLEMENTED

**3. Epic-006**: `docs/epics/Epic-006-Gemini-2.5-Flash-Lite-Thinking-Optimizations.md`
- Update Story-006-02 status
- Update progress (1/6 → 2/6)

---

## Cross-References

### Related Stories

**Epic-006** (This Epic):
- Story-006-01: Live API Validation (BLOCKS this story)
- Story-006-03: Quality Ceiling Detection (uses complexity classifier)
- Story-006-04: Budget Analytics Dashboard (displays metrics)
- Story-006-06: Documentation (documents feature)

### Related Features

**Existing Optimizations**:
- Auto-stream conversion (similar intelligent optimization)
- Token manager quota weighting (complexity-aware)
- Rate limit retry logic (intelligent recovery)

---

## Implementation Notes

### Classifier Tuning Tips

**Tip 1: Keyword Lists**
- Start conservative, expand based on telemetry
- Monitor misclassifications, add missing keywords

**Tip 2: Score Weights**
- Keyword score most important (0.4 weight)
- Length score secondary (0.3 weight)
- Structure and semantics supportive (0.2 + 0.1)

**Tip 3: Threshold Tuning**
- Start with 0.35/0.65/0.85 thresholds
- Adjust based on accuracy validation
- Bias toward overestimating complexity (safer for quality)

### Common Pitfalls

**Avoid**:
- ❌ Overriding explicit user budgets (violates user control)
- ❌ Classification taking >10ms (performance impact)
- ❌ Hardcoding prompts for testing (use realistic examples)
- ❌ Ignoring telemetry (can't measure savings)

**Ensure**:
- ✅ User control always preserved
- ✅ Classification fast (<5ms)
- ✅ Telemetry comprehensive
- ✅ Logging clear and informative

---

## Questions & Answers

### Q1: What if users prefer manual budget control?

**A**: Adaptive budget is opt-out via explicit budget:
- Default: If user specifies budget → use it (no override)
- Adaptive: Only activated when budget_tokens = null
- Future: Could add config flag to disable adaptive entirely

### Q2: Should we support budget_tokens = "auto"?

**A**: Not in this story - explicit opt-in would require API change:
- Current: null triggers adaptive (implicit)
- Proposed: "auto" string triggers adaptive (explicit)
- Decision: Defer to future enhancement if needed

### Q3: How do we handle multi-turn conversations?

**A**: Classify only user messages, ignore assistant messages:
- extract_prompt_text() filters for role == "user"
- Complexity based on current user request
- Future: Could consider conversation context

### Q4: What about streaming requests?

**A**: Adaptive budget works identically:
- Streaming vs non-streaming doesn't affect classification
- Budget applied before streaming begins
- Telemetry recorded same way

### Q5: Should adaptive budget consider user history?

**A**: Not in this story - single-request classification:
- Future enhancement: Per-user complexity patterns
- Machine learning model could improve accuracy
- Current: Stateless heuristic classifier sufficient

---

## Story Status

**Current Status**: To Do
**Next Story**: Story-006-03 (Quality Ceiling Detection) - Can run in parallel
**Epic Progress**: 1/6 stories complete (16.7%) - Story-006-01 complete

**Depends On**: Story-006-01 (Live API Validation) - MUST COMPLETE FIRST

---

**Created**: 2026-01-11
**Last Updated**: 2026-01-11
**Story Points**: 3
**Estimated Hours**: 3 (1.5h classifier + 1h integration + 0.5h telemetry)
**Cost Savings Target**: ≥15% reduction in thinking token costs
