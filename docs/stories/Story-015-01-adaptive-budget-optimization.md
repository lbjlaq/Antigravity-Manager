# Story-015-01: Adaptive Thinking Budget Optimization

**Epic**: Epic-015 - Gemini 2.5 Pro Thinking Optimization (Team 1, Gemini Specialists)
**Priority**: P2 (MEDIUM - cost optimization)
**Effort**: 5 days
**Assignee**: Dev 1A (Team 1 Lead)
**Status**: ✅ COMPLETE - QA PASSED (10/10) - 72/72 Tests
**Created**: 2026-01-12

---

## Objective

Implement dynamic thinking budget selection based on query complexity analysis to achieve 15-25% cost savings on simple queries while maintaining quality on complex reasoning tasks. This system will automatically classify incoming requests and assign appropriate thinking budgets (4K/16K/32K tokens) to optimize the cost-quality tradeoff for Gemini 2.5 Pro Thinking model.

---

## Business Context

### Problem Statement

Gemini 2.5 Pro Thinking currently uses a fixed thinking budget across all queries, resulting in:
- **Over-provisioning**: Simple queries (e.g., "What is 2+2?") consume expensive 32K thinking budget unnecessarily
- **Cost inefficiency**: 40-60% of queries are simple/moderate complexity but pay premium thinking costs
- **Missed optimization**: No intelligence in budget allocation despite predictable query patterns

### Success Metrics

**Primary KPI**: 15-25% cost reduction on simple queries
**Quality Gate**: <2% quality degradation on complex queries (subjective evaluation)
**Performance Target**: Classifier overhead <50ms per request
**Accuracy Target**: 85%+ correct classification rate

### Business Value

- **Cost savings**: $150-250/month reduction per 1M requests (estimated)
- **Scalability**: Enables handling more queries within budget constraints
- **Intelligence**: Foundation for future ML-based optimization
- **Flexibility**: Per-user budget policies (future enhancement)

---

## Acceptance Criteria

### AC1: Query Complexity Classifier Implemented (3 Tiers)

**GIVEN** an incoming Gemini 2.5 Pro Thinking request
**WHEN** the classifier analyzes the query
**THEN** it must assign exactly one tier: SIMPLE, MODERATE, or COMPLEX

**Classification Logic**:
```yaml
SIMPLE (4K budget):
  - Single-sentence queries
  - Basic arithmetic or factual lookup
  - No multi-step reasoning required
  - Query length: <50 tokens
  - Example: "What is the capital of France?"

MODERATE (16K budget):
  - Multi-sentence queries with context
  - Requires 2-5 reasoning steps
  - Query length: 50-200 tokens
  - Example: "Compare React and Vue for enterprise apps"

COMPLEX (32K budget):
  - Multi-paragraph queries or code analysis
  - Requires >5 reasoning steps or domain expertise
  - Query length: >200 tokens
  - Example: "Design a distributed caching architecture..."
  - Fallback: Default if classification uncertain
```

**Implementation**:
- Heuristic-based classifier (v1): token count, keyword patterns, structural analysis
- Feature extraction: query length, sentence count, technical keywords, code blocks
- Confidence scoring: 0.0-1.0 (default to COMPLEX if confidence <0.7)

---

### AC2: Budget Recommendation Engine (Automatic Selection)

**GIVEN** a classified query tier
**WHEN** mapping to Gemini API request
**THEN** the correct thinking budget MUST be automatically applied

**Budget Mapping**:
```yaml
SIMPLE → thinking_config.thinking_budget: 4000
MODERATE → thinking_config.thinking_budget: 16000
COMPLEX → thinking_config.thinking_budget: 32000
UNCERTAIN → thinking_config.thinking_budget: 32000 (safe default)
```

**Override Support**:
- User-specified budget in request → honors user override (no classification)
- Configuration flag `enable_adaptive_budget: true/false`
- Per-account budget policy support (future enhancement marker)

---

### AC3: Cost Tracking by Budget Tier

**GIVEN** requests processed with different budgets
**WHEN** monitoring metrics
**THEN** cost breakdown MUST be tracked per tier

**Metrics to Track**:
```rust
struct BudgetMetrics {
    tier: BudgetTier,                    // SIMPLE, MODERATE, COMPLEX
    request_count: u64,                  // Requests in this tier
    total_input_tokens: u64,             // Sum of input tokens
    total_thinking_tokens: u64,          // Sum of thinking tokens consumed
    total_output_tokens: u64,            // Sum of output tokens
    estimated_cost_usd: f64,             // Cost calculation (tokens × rate)
    avg_thinking_utilization: f64,       // actual_thinking / budget_allocated
    classification_confidence_avg: f64,  // Average classifier confidence
}
```

**Cost Calculation**:
- Gemini 2.5 Pro Thinking pricing (as of 2026-01):
  - Input: $0.0525/1M tokens
  - Thinking: $0.35/1M tokens
  - Output: $0.21/1M tokens

**Tracking Location**: Extend `src-tauri/src/proxy/monitor.rs` from Epic-013

---

### AC4: 15-25% Cost Reduction Validated (Simple Queries)

**GIVEN** a benchmark set of 1000 queries (300 simple, 500 moderate, 200 complex)
**WHEN** processed with adaptive budgets vs fixed 32K budget
**THEN** cost reduction MUST be 15-25% overall

**Validation Method**:
```yaml
Baseline (Fixed 32K):
  - All queries use 32K thinking budget
  - Total cost: $X (calculated)

Adaptive Budget:
  - Queries classified and assigned appropriate budgets
  - Total cost: $Y (calculated)
  - Savings: ((X - Y) / X) × 100% ≥ 15%
```

**Test Dataset**:
- Simple queries (30%): "What is X?", "Define Y", "Calculate Z"
- Moderate queries (50%): "Compare A and B", "Explain pros/cons of C"
- Complex queries (20%): Multi-paragraph reasoning, code analysis, system design

**Success Criteria**:
- Cost reduction: 15-25% range
- No false negatives: Complex queries must NOT be classified as SIMPLE/MODERATE
- Acceptable false positives: <10% of SIMPLE queries misclassified as MODERATE/COMPLEX

---

### AC5: No Quality Degradation (Complex Queries)

**GIVEN** complex reasoning tasks requiring full 32K budget
**WHEN** processed with adaptive budget system
**THEN** quality MUST remain equivalent to fixed 32K baseline

**Quality Evaluation**:
```yaml
Method: Human subjective evaluation (spot-check)
Sample Size: 50 complex queries from benchmark set
Evaluators: 2 developers (blind A/B testing)

Criteria:
  - Reasoning depth (1-5 scale)
  - Answer completeness (1-5 scale)
  - Accuracy (1-5 scale)
  - Overall quality (1-5 scale)

Threshold: Average score difference <0.2 points across all criteria
```

**Automated Quality Checks**:
- Response length: Complex queries should produce similar response lengths
- Thinking token usage: Should be within 10% of baseline
- Error rate: No increase in API errors or truncated responses

**Fallback Safety**:
- If classifier confidence <0.7 → default to COMPLEX (32K budget)
- If thinking tokens exhausted → log warning, flag for review

---

### AC6: Comprehensive Test Coverage

**Unit Tests** (20+ tests minimum):
```yaml
Classifier Tests:
  - test_classify_simple_query_single_sentence
  - test_classify_simple_query_short_factual
  - test_classify_moderate_query_multi_step
  - test_classify_moderate_query_comparison
  - test_classify_complex_query_code_analysis
  - test_classify_complex_query_system_design
  - test_classify_uncertain_defaults_to_complex
  - test_classify_edge_case_empty_query
  - test_classify_edge_case_very_long_query
  - test_classify_confidence_scoring

Budget Engine Tests:
  - test_budget_recommendation_simple_4k
  - test_budget_recommendation_moderate_16k
  - test_budget_recommendation_complex_32k
  - test_budget_recommendation_uncertain_32k
  - test_budget_override_user_specified
  - test_budget_disabled_uses_default

Metrics Tests:
  - test_metrics_tracking_per_tier
  - test_cost_calculation_simple
  - test_cost_calculation_moderate
  - test_cost_calculation_complex
```

**Integration Tests** (10+ tests minimum):
```yaml
End-to-End Classification:
  - test_e2e_simple_query_classification_and_budget
  - test_e2e_moderate_query_classification_and_budget
  - test_e2e_complex_query_classification_and_budget
  - test_e2e_user_override_bypasses_classification

Metrics Integration:
  - test_metrics_updated_after_request
  - test_cost_tracking_aggregation
  - test_classification_confidence_recording

Configuration:
  - test_adaptive_budget_enabled_flag
  - test_adaptive_budget_disabled_flag
  - test_per_account_budget_policy (stub for future)
```

**E2E Tests** (5+ tests minimum):
```yaml
Real API Tests:
  - test_real_simple_query_4k_budget
  - test_real_moderate_query_16k_budget
  - test_real_complex_query_32k_budget
  - test_cost_savings_calculation
  - test_quality_no_degradation_spot_check
```

---

## Implementation Details

### Module Structure

```
src-tauri/src/proxy/mappers/gemini/
├── budget_optimizer.rs        (NEW - 400 lines)
│   ├── QueryComplexityClassifier
│   │   ├── classify(query: &str) → (BudgetTier, f64)
│   │   ├── extract_features(query: &str) → Features
│   │   ├── calculate_confidence(features: Features) → f64
│   │   └── heuristic_classify(features: Features) → BudgetTier
│   ├── BudgetRecommendationEngine
│   │   ├── recommend_budget(tier: BudgetTier, config: &Config) → u32
│   │   ├── apply_overrides(user_budget: Option<u32>, recommended: u32) → u32
│   │   └── validate_budget(budget: u32) → Result<u32, Error>
│   └── BudgetMetricsTracker
│       ├── record_request(tier: BudgetTier, tokens: TokenUsage, cost: f64)
│       ├── get_metrics_by_tier(tier: BudgetTier) → BudgetMetrics
│       └── calculate_cost_savings(adaptive: f64, baseline: f64) → f64
└── request.rs                 (MODIFY - integrate optimizer)
    └── map_request_with_adaptive_budget(req: OpenAiRequest, config: &Config) → GeminiRequest
```

### Data Structures

```rust
// src-tauri/src/proxy/mappers/gemini/budget_optimizer.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BudgetTier {
    Simple,      // 4K budget
    Moderate,    // 16K budget
    Complex,     // 32K budget
}

impl BudgetTier {
    pub fn to_token_budget(&self) -> u32 {
        match self {
            BudgetTier::Simple => 4000,
            BudgetTier::Moderate => 16000,
            BudgetTier::Complex => 32000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueryFeatures {
    pub token_count: usize,
    pub sentence_count: usize,
    pub has_code_blocks: bool,
    pub has_technical_keywords: bool,
    pub has_multi_step_indicators: bool,
    pub avg_sentence_length: f64,
}

#[derive(Debug, Clone)]
pub struct ClassificationResult {
    pub tier: BudgetTier,
    pub confidence: f64,        // 0.0-1.0
    pub features: QueryFeatures,
}

#[derive(Debug, Clone, Serialize)]
pub struct BudgetMetrics {
    pub tier: BudgetTier,
    pub request_count: u64,
    pub total_input_tokens: u64,
    pub total_thinking_tokens: u64,
    pub total_output_tokens: u64,
    pub estimated_cost_usd: f64,
    pub avg_thinking_utilization: f64,
    pub classification_confidence_avg: f64,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

pub struct QueryComplexityClassifier {
    confidence_threshold: f64,  // Default: 0.7
}

impl QueryComplexityClassifier {
    pub fn new() -> Self {
        Self {
            confidence_threshold: 0.7,
        }
    }

    pub fn classify(&self, query: &str) -> ClassificationResult {
        let features = self.extract_features(query);
        let tier = self.heuristic_classify(&features);
        let confidence = self.calculate_confidence(&features, tier);

        // Safe fallback: low confidence → COMPLEX
        let final_tier = if confidence < self.confidence_threshold {
            BudgetTier::Complex
        } else {
            tier
        };

        ClassificationResult {
            tier: final_tier,
            confidence,
            features,
        }
    }

    fn extract_features(&self, query: &str) -> QueryFeatures {
        let tokens: Vec<&str> = query.split_whitespace().collect();
        let sentences: Vec<&str> = query.split(&['.', '?', '!'][..]).collect();

        QueryFeatures {
            token_count: tokens.len(),
            sentence_count: sentences.len(),
            has_code_blocks: query.contains("```") || query.contains("```"),
            has_technical_keywords: self.detect_technical_keywords(query),
            has_multi_step_indicators: self.detect_multi_step(query),
            avg_sentence_length: tokens.len() as f64 / sentences.len().max(1) as f64,
        }
    }

    fn heuristic_classify(&self, features: &QueryFeatures) -> BudgetTier {
        // Rule-based classification (v1)

        // COMPLEX indicators (highest priority)
        if features.has_code_blocks
            || features.token_count > 200
            || features.sentence_count > 10 {
            return BudgetTier::Complex;
        }

        // SIMPLE indicators
        if features.token_count < 50
            && features.sentence_count <= 2
            && !features.has_multi_step_indicators
            && !features.has_technical_keywords {
            return BudgetTier::Simple;
        }

        // Default to MODERATE
        BudgetTier::Moderate
    }

    fn calculate_confidence(&self, features: &QueryFeatures, tier: BudgetTier) -> f64 {
        // Confidence scoring based on feature clarity
        match tier {
            BudgetTier::Simple => {
                if features.token_count < 30 && features.sentence_count == 1 {
                    0.95 // Very confident
                } else if features.token_count < 50 {
                    0.85 // Confident
                } else {
                    0.70 // Threshold
                }
            }
            BudgetTier::Complex => {
                if features.has_code_blocks || features.token_count > 300 {
                    0.95 // Very confident
                } else if features.token_count > 200 {
                    0.85 // Confident
                } else {
                    0.75 // Moderate confidence
                }
            }
            BudgetTier::Moderate => 0.80, // Default moderate confidence
        }
    }

    fn detect_technical_keywords(&self, query: &str) -> bool {
        let keywords = [
            "algorithm", "architecture", "optimize", "implement",
            "debug", "refactor", "design pattern", "scalability",
            "performance", "security", "database", "API"
        ];
        keywords.iter().any(|k| query.to_lowercase().contains(k))
    }

    fn detect_multi_step(&self, query: &str) -> bool {
        let indicators = [
            "compare", "analyze", "evaluate", "pros and cons",
            "step by step", "first", "then", "finally"
        ];
        indicators.iter().any(|i| query.to_lowercase().contains(i))
    }
}

pub struct BudgetRecommendationEngine {
    adaptive_enabled: bool,
}

impl BudgetRecommendationEngine {
    pub fn recommend_budget(
        &self,
        tier: BudgetTier,
        user_override: Option<u32>,
        config: &ProxyConfig,
    ) -> u32 {
        // User override takes precedence
        if let Some(budget) = user_override {
            return budget;
        }

        // Feature flag check
        if !config.adaptive_budget_enabled {
            return 32000; // Default fixed budget
        }

        // Return tier-based budget
        tier.to_token_budget()
    }
}

pub struct BudgetMetricsTracker {
    metrics: Arc<Mutex<HashMap<BudgetTier, BudgetMetrics>>>,
}

impl BudgetMetricsTracker {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn record_request(
        &self,
        tier: BudgetTier,
        input_tokens: u64,
        thinking_tokens: u64,
        output_tokens: u64,
        confidence: f64,
    ) {
        let mut metrics = self.metrics.lock().unwrap();
        let entry = metrics.entry(tier).or_insert_with(|| BudgetMetrics {
            tier,
            request_count: 0,
            total_input_tokens: 0,
            total_thinking_tokens: 0,
            total_output_tokens: 0,
            estimated_cost_usd: 0.0,
            avg_thinking_utilization: 0.0,
            classification_confidence_avg: 0.0,
            last_updated: chrono::Utc::now(),
        });

        entry.request_count += 1;
        entry.total_input_tokens += input_tokens;
        entry.total_thinking_tokens += thinking_tokens;
        entry.total_output_tokens += output_tokens;

        // Cost calculation (Gemini 2.5 Pro Thinking rates)
        let cost = (input_tokens as f64 * 0.0525 / 1_000_000.0)
                 + (thinking_tokens as f64 * 0.35 / 1_000_000.0)
                 + (output_tokens as f64 * 0.21 / 1_000_000.0);
        entry.estimated_cost_usd += cost;

        // Update rolling average confidence
        entry.classification_confidence_avg =
            (entry.classification_confidence_avg * (entry.request_count - 1) as f64 + confidence)
            / entry.request_count as f64;

        entry.last_updated = chrono::Utc::now();
    }

    pub fn get_metrics(&self) -> HashMap<BudgetTier, BudgetMetrics> {
        self.metrics.lock().unwrap().clone()
    }

    pub fn calculate_cost_savings(&self) -> f64 {
        let metrics = self.metrics.lock().unwrap();

        // Calculate actual cost (adaptive)
        let actual_cost: f64 = metrics.values()
            .map(|m| m.estimated_cost_usd)
            .sum();

        // Calculate baseline cost (all 32K budget)
        let baseline_cost: f64 = metrics.values()
            .map(|m| {
                let tokens = m.total_input_tokens + m.total_thinking_tokens + m.total_output_tokens;
                // Assume same distribution with 32K budget
                (m.total_input_tokens as f64 * 0.0525 / 1_000_000.0)
                + (32000 as f64 * m.request_count as f64 * 0.35 / 1_000_000.0)
                + (m.total_output_tokens as f64 * 0.21 / 1_000_000.0)
            })
            .sum();

        if baseline_cost > 0.0 {
            ((baseline_cost - actual_cost) / baseline_cost) * 100.0
        } else {
            0.0
        }
    }
}
```

### Integration Points

**1. Modify `src-tauri/src/proxy/mappers/gemini/request.rs`**:
```rust
use super::budget_optimizer::{
    QueryComplexityClassifier,
    BudgetRecommendationEngine,
    BudgetMetricsTracker
};

pub fn map_request(
    req: OpenAiRequest,
    config: &ProxyConfig,
    metrics_tracker: Arc<BudgetMetricsTracker>,
) -> Result<GeminiRequest, MapperError> {
    // Extract query text from request
    let query_text = extract_query_text(&req)?;

    // Classify query complexity
    let classifier = QueryComplexityClassifier::new();
    let classification = classifier.classify(&query_text);

    // Recommend budget
    let engine = BudgetRecommendationEngine::new();
    let user_override = extract_user_budget_override(&req);
    let thinking_budget = engine.recommend_budget(
        classification.tier,
        user_override,
        config,
    );

    // Build Gemini request with adaptive budget
    let mut gemini_req = build_base_gemini_request(req)?;
    gemini_req.generation_config.thinking_config = Some(ThinkingConfig {
        thinking_budget,
        include_thoughts: true,
    });

    // Track classification metadata
    gemini_req.metadata = Some(RequestMetadata {
        budget_tier: classification.tier,
        classification_confidence: classification.confidence,
    });

    Ok(gemini_req)
}
```

**2. Extend `src-tauri/src/proxy/monitor.rs`**:
```rust
pub struct RequestMonitor {
    // Existing fields...
    budget_metrics_tracker: Arc<BudgetMetricsTracker>,
}

impl RequestMonitor {
    pub fn record_gemini_response(
        &self,
        metadata: RequestMetadata,
        usage: TokenUsage,
    ) {
        self.budget_metrics_tracker.record_request(
            metadata.budget_tier,
            usage.input_tokens,
            usage.thinking_tokens,
            usage.output_tokens,
            metadata.classification_confidence,
        );
    }

    pub fn get_budget_metrics(&self) -> HashMap<BudgetTier, BudgetMetrics> {
        self.budget_metrics_tracker.get_metrics()
    }

    pub fn get_cost_savings(&self) -> f64 {
        self.budget_metrics_tracker.calculate_cost_savings()
    }
}
```

**3. Configuration Support** (`src-tauri/src/proxy/config.rs`):
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    // Existing fields...

    /// Enable adaptive thinking budget optimization (default: false)
    #[serde(default)]
    pub adaptive_budget_enabled: bool,
}
```

---

## Test Strategy

### Phase 1: Unit Testing (Days 1-2)
**Focus**: Classifier logic, budget recommendation, metrics calculation

```bash
cargo test --package antigravity_tools_lib budget_optimizer
```

**Key Tests**:
- Classifier accuracy on edge cases (empty, very long, code-heavy)
- Budget recommendation with/without overrides
- Metrics aggregation and cost calculation
- Confidence scoring validation

---

### Phase 2: Integration Testing (Days 3-4)
**Focus**: End-to-end request flow with classification

```bash
cargo test --package antigravity_tools_lib mappers::gemini::integration
```

**Key Tests**:
- Request mapping with adaptive budget
- Metrics tracking after response
- Configuration flag toggling (enable/disable adaptive)
- User override bypassing classification

---

### Phase 3: E2E Validation (Day 5)
**Focus**: Real API calls, cost validation, quality assessment

**Test Dataset**: `tests/data/budget_optimization_queries.json`
```json
[
  {"query": "What is 2+2?", "expected_tier": "Simple"},
  {"query": "Explain the benefits of microservices", "expected_tier": "Moderate"},
  {"query": "Design a distributed caching system...", "expected_tier": "Complex"}
]
```

**Run E2E Tests**:
```bash
GEMINI_API_KEY=xxx cargo test --package antigravity_tools_lib e2e_budget
```

**Manual Quality Evaluation**:
- Sample 50 complex queries
- Compare responses: fixed 32K vs adaptive
- Score quality (blind A/B test)
- Threshold: <2% degradation

---

## Dependencies

### Internal Dependencies
- **Epic-013 Story 013-05**: Cache module (`src-tauri/src/proxy/cache.rs`) - COMPLETED
- **Epic-013 Story 013-06**: Monitor module (`src-tauri/src/proxy/monitor.rs`) - COMPLETED
- **Gemini request mapper**: `src-tauri/src/proxy/mappers/gemini/request.rs` - STABLE

### External Dependencies
- No new crates required (uses existing tokenizers and string processing)

### Configuration Requirements
- Add `adaptive_budget_enabled: bool` to `ProxyConfig`
- No breaking changes (default: false)

---

## Success Metrics

### Quantitative Metrics
| Metric | Target | Measurement Method |
|--------|--------|-------------------|
| Cost reduction (simple queries) | 15-25% | Compare adaptive vs fixed 32K on benchmark set |
| Classifier overhead | <50ms | Log classification time per request |
| Classification accuracy | 85%+ | Manual validation on 200-query test set |
| Quality degradation (complex) | <2% | Human evaluation (A/B test) |
| Test coverage | 90%+ | `cargo tarpaulin` |

### Qualitative Metrics
- **Classifier transparency**: Clear logging of tier assignments and confidence
- **Operational visibility**: Metrics dashboard shows budget distribution
- **Configuration simplicity**: Single flag to enable/disable
- **Extensibility**: Easy to add ML-based classifier in future

---

## Definition of Done

### Code Complete
- ✅ `budget_optimizer.rs` implemented (400 lines)
- ✅ `request.rs` integrated with optimizer
- ✅ `monitor.rs` extended with budget metrics tracking
- ✅ Configuration flag added to `config.rs`

### Testing Complete
- ✅ 20+ unit tests passing (`cargo test budget_optimizer`)
- ✅ 10+ integration tests passing (`cargo test mappers::gemini::integration`)
- ✅ 5+ E2E tests passing with real API (`cargo test e2e_budget`)
- ✅ Test coverage ≥90% (`cargo tarpaulin`)

### Quality Gates Passed
- ✅ Cost reduction: 15-25% validated on benchmark set (300 simple, 500 moderate, 200 complex queries)
- ✅ Quality: <2% degradation on complex queries (manual evaluation)
- ✅ Performance: Classifier overhead <50ms (P95)
- ✅ Accuracy: 85%+ classification accuracy (manual validation)

### Documentation Complete
- ✅ Inline code documentation (rustdoc)
- ✅ Architecture decision: Why heuristic classifier (v1) vs ML
- ✅ Usage guide: How to enable adaptive budget
- ✅ Metrics guide: How to interpret budget metrics

### Deployment Ready
- ✅ Feature flag defaulting to `false` (safe rollout)
- ✅ No breaking changes to existing API
- ✅ Logging and monitoring integrated
- ✅ Rollback plan: Disable flag if issues detected

---

## Risk Assessment

### Technical Risks

**Risk 1**: Classifier accuracy <85% (misclassification)
- **Impact**: HIGH (cost overspending or quality degradation)
- **Probability**: MEDIUM
- **Mitigation**: Conservative confidence threshold (0.7), default to COMPLEX on uncertainty
- **Fallback**: Disable adaptive budget via configuration flag

**Risk 2**: Quality degradation on edge cases
- **Impact**: HIGH (user-facing quality issues)
- **Probability**: LOW
- **Mitigation**: Comprehensive test set with edge cases, manual quality evaluation
- **Fallback**: Increase confidence threshold or revert to fixed 32K

**Risk 3**: Classifier overhead >50ms
- **Impact**: MEDIUM (request latency)
- **Probability**: LOW
- **Mitigation**: Heuristic-based classifier (fast), async processing if needed
- **Fallback**: Cache classification results for repeated queries

### Operational Risks

**Risk 4**: Unexpected API behavior with different budgets
- **Impact**: MEDIUM (API errors or throttling)
- **Probability**: LOW
- **Mitigation**: Gradual rollout, monitor API error rates by budget tier
- **Fallback**: Disable adaptive budget immediately

---

## Future Enhancements

### ML-Based Classifier (v2)
- Train lightweight ML model on labeled query dataset
- Features: embeddings, structural analysis, domain classification
- Target: 95%+ accuracy, <100ms inference time

### Per-User Budget Policies
- Allow users to configure budget preferences (cost vs quality tradeoff)
- Custom tier mappings (e.g., user prefers 8K for moderate queries)

### Dynamic Budget Adjustment
- Real-time feedback loop: if thinking tokens exhausted, upgrade budget for next similar query
- Learn from user feedback (thumbs up/down on responses)

### Cost Attribution
- Track cost savings per user/account
- Show cost breakdown in dashboard (by tier, by user)

---

## Notes

- **Conservative design**: Defaults to COMPLEX (32K) when uncertain to avoid quality risks
- **Extensibility**: Architecture supports plugging in ML-based classifier without breaking changes
- **Metrics foundation**: Budget metrics tracking enables data-driven optimization in future iterations
- **Team coordination**: Does NOT conflict with Team 2 Epic-024 work (separate model/features)
- **Builds on Epic-013**: Leverages existing cache and monitor infrastructure
