# Epic-008 QA Strategy - Gemini 2.5 Pro Thinking Optimization

**QA Lead**: Developer C1
**Epic**: Epic-008 (Gemini 2.5 Pro Thinking Optimization)
**Branch**: `epic-008-gemini-2.5-pro-thinking`
**Status**: PLANNED
**Created**: 2026-01-11

---

## 📋 Executive Summary

This document defines the comprehensive QA strategy for Epic-008, supporting Developer A1 (Adaptive Budget Optimization) and Developer B1 (Cache Monitoring) with testing infrastructure, code review, and quality validation.

### QA Objectives

1. **Testing Support**: Provide comprehensive test coverage for both stories (≥90%)
2. **Code Review**: Ensure code quality, performance, and maintainability
3. **Performance Validation**: Verify all performance targets are met
4. **Documentation**: Create testing guides and troubleshooting resources
5. **Integration**: Validate seamless operation of both features together

---

## 🎯 Story-Level Testing Breakdown

### Story-008-01: Adaptive Budget Optimization (Developer A1)

**Module Under Test**: `src-tauri/src/proxy/budget_optimizer.rs` (new module)

#### Testing Focus Areas

**1. Complexity Classifier (AC1)**
- Input: User prompts (string)
- Output: Complexity level enum (Simple/Moderate/Complex/Deep)
- Target: ≥85% classification accuracy, <50ms p95 latency

**Test Data Requirements**:
```yaml
simple_prompts:
  count: 50
  examples:
    - "hello"
    - "what time is it?"
    - "yes"
    - "thanks"
    - "how are you?"
  expected_budget: 2000-4000 tokens

moderate_prompts:
  count: 50
  examples:
    - "explain quantum computing"
    - "summarize this article: [text]"
    - "what's the capital of France?"
  expected_budget: 8000-12000 tokens

complex_prompts:
  count: 50
  examples:
    - "analyze the pros and cons of microservices vs monolithic architecture"
    - "compare React, Vue, and Angular for enterprise applications"
  expected_budget: 16000-24000 tokens

deep_prompts:
  count: 50
  examples:
    - "design a distributed tracing system for a microservices architecture with 100+ services"
    - "research paper analysis: [comprehensive academic paper]"
  expected_budget: 24000-32000 tokens
```

**2. Dynamic Budget Mapper (AC2)**
- Input: ComplexityLevel enum
- Output: Integer budget (1-32000)
- Validation: Budget ranges match complexity level

**3. Historical Usage Analysis (AC3)**
- Input: 30 days request history
- Output: Pattern → optimal_budget mappings
- Validation: Pattern recognition accuracy ≥80%

**4. Quality Feedback Loop (AC4)**
- Input: Response quality score (0.0-1.0)
- Output: Adjusted budget recommendation
- Validation: Convergence within 10 iterations

**5. Cost Savings Validation (AC5)**
- Baseline: Fixed 16000 token budget
- Target: ≥15% savings on simple queries
- Measurement: 1 week production data

**6. Override and Manual Control (AC6)**
- Validation: Manual `thinkingBudget` parameter always takes precedence
- No automatic adjustment when override present

---

### Story-008-02: Cache Monitoring (Developer B1)

**Module Under Test**: `src-tauri/src/proxy/cache_monitor.rs` (new module)

#### Testing Focus Areas

**1. Cache Hit Rate Metrics (AC1)**
- Metric: hits / (hits + misses)
- Target: ≥30% hit rate
- Real-time tracking: WebSocket updates

**2. Signature Reuse Analysis (AC2)**
- Track top 10 most reused signatures
- Identify high-value signatures (≥3 reuses)
- Reuse distribution histogram

**3. Cost Attribution Tracking (AC3)**
- Per-signature cost savings calculation
- Per-account attribution
- Per-user attribution (if applicable)
- Target: ≥20% total cost savings

**4. Performance Benchmarking (AC4)**
- Cache lookup time: <10ms p95
- Cache write time: <5ms p95
- Memory usage tracking
- No >20% performance degradation

**5. Dashboard Integration (AC5)**
- Real-time graph updates
- Top signatures table
- Cost savings chart
- Performance metrics panel
- Data refresh <1s latency

**6. Alerting and Reporting (AC6)**
- Hit rate <20% → Alert
- Lookup time >20ms p95 → Warning
- Daily summary reports
- Weekly optimization recommendations

---

## 🧪 Test Categories and Coverage

### 1. Unit Tests (Target: ≥90% code coverage)

**Budget Optimizer Tests** (`src-tauri/src/proxy/tests/budget_optimizer.rs`):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complexity_classifier_simple() {
        // Verify simple prompts → Simple classification
        let classifier = ComplexityClassifier::new();
        assert_eq!(
            classifier.classify("hello"),
            ComplexityLevel::Simple
        );
    }

    #[test]
    fn test_complexity_classifier_accuracy() {
        // Test classification accuracy across 200 samples
        // Target: ≥85% accuracy
    }

    #[test]
    fn test_budget_mapper_ranges() {
        // Verify budget ranges for each complexity level
        let mapper = BudgetMapper::new();
        let budget = mapper.calculate(ComplexityLevel::Simple);
        assert!(budget >= 2000 && budget <= 4000);
    }

    #[test]
    fn test_manual_override_precedence() {
        // Verify manual budget always takes precedence
    }

    #[test]
    fn test_feedback_loop_convergence() {
        // Verify feedback loop converges within 10 iterations
    }

    #[test]
    fn test_pattern_learning_persistence() {
        // Verify learned patterns persist across restarts
    }

    #[tokio::test]
    async fn test_classification_performance() {
        // Verify <50ms p95 latency
        let start = Instant::now();
        classifier.classify(prompt);
        assert!(start.elapsed() < Duration::from_millis(50));
    }
}
```

**Cache Monitor Tests** (`src-tauri/src/proxy/tests/cache_monitor.rs`):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_hit_rate_calculation() {
        let monitor = CacheMonitor::new();
        monitor.record_hit("sig1");
        monitor.record_miss("sig2");
        assert_eq!(monitor.get_hit_rate(), 0.5);
    }

    #[tokio::test]
    async fn test_signature_reuse_tracking() {
        // Track reuse count per signature
    }

    #[tokio::test]
    async fn test_cost_attribution_accuracy() {
        // Verify cost savings calculations
    }

    #[tokio::test]
    async fn test_performance_metric_collection() {
        // Verify metrics collected accurately
    }

    #[tokio::test]
    async fn test_metrics_persistence() {
        // Verify metrics persist across restarts
    }

    #[tokio::test]
    async fn test_cache_lookup_performance() {
        // Verify <10ms p95 latency
    }
}
```

---

### 2. Integration Tests (100% critical path coverage)

**Adaptive Budget Integration** (`src-tauri/tests/adaptive_budget_integration.rs`):
```rust
#[tokio::test]
async fn test_adaptive_budget_simple_query() {
    // GIVEN: Simple query "hello"
    // WHEN: Request processed with adaptive budget
    // THEN: Budget is 2000-4000 tokens
    // VERIFY: Response quality unchanged
}

#[tokio::test]
async fn test_adaptive_budget_complex_query() {
    // GIVEN: Complex multi-paragraph analysis request
    // WHEN: Request processed with adaptive budget
    // THEN: Budget is 16000-24000 tokens
    // VERIFY: Response quality meets standards
}

#[tokio::test]
async fn test_cost_savings_validation() {
    // GIVEN: 100 requests (50 simple, 50 complex)
    // WHEN: Processed with adaptive budget
    // THEN: ≥15% cost savings vs baseline
    // VERIFY: Quality metrics unchanged
}

#[tokio::test]
async fn test_quality_preservation() {
    // GIVEN: Baseline quality scores
    // WHEN: Adaptive budget enabled
    // THEN: No degradation in quality metrics
}
```

**Cache Monitoring Integration** (`src-tauri/tests/cache_monitor_integration.rs`):
```rust
#[tokio::test]
async fn test_cache_monitor_integration() {
    // GIVEN: Cache monitor active
    // WHEN: Requests processed
    // THEN: Metrics collected accurately
    // VERIFY: Dashboard API returns correct data
}

#[tokio::test]
async fn test_dashboard_metrics_api() {
    // GIVEN: Metrics API endpoints
    // WHEN: Dashboard queries metrics
    // THEN: Response time <100ms
    // VERIFY: Data accuracy ≥95%
}

#[tokio::test]
async fn test_real_time_updates() {
    // GIVEN: WebSocket connection
    // WHEN: Cache metrics change
    // THEN: Updates delivered <1s latency
}

#[tokio::test]
async fn test_cost_savings_validation() {
    // GIVEN: Cached requests
    // WHEN: Cost calculator runs
    // THEN: Savings ≥20% of total cost
}
```

---

### 3. Performance Tests

**Budget Optimizer Performance** (`src-tauri/benches/budget_optimizer_bench.rs`):
```rust
#[bench]
fn bench_classification_latency(b: &mut Bencher) {
    // Target: <50ms p95
    b.iter(|| {
        classifier.classify(test_prompt);
    });
}

#[bench]
fn bench_budget_calculation(b: &mut Bencher) {
    // Target: <10ms p99
    b.iter(|| {
        mapper.calculate(ComplexityLevel::Complex);
    });
}

#[bench]
fn bench_pattern_lookup(b: &mut Bencher) {
    // Target: <5ms p95
    b.iter(|| {
        pattern_store.lookup(signature);
    });
}
```

**Cache Monitor Performance** (`src-tauri/benches/cache_monitor_bench.rs`):
```rust
#[bench]
fn bench_cache_lookup_time(b: &mut Bencher) {
    // Target: <10ms p95
    b.iter(|| {
        monitor.get_hit_rate();
    });
}

#[bench]
fn bench_metrics_collection_overhead(b: &mut Bencher) {
    // Target: <1% of request time
    b.iter(|| {
        monitor.record_hit(signature);
    });
}
```

---

### 4. End-to-End Tests

**Combined Feature E2E** (`src-tauri/tests/epic_008_e2e.rs`):
```rust
#[tokio::test]
async fn test_adaptive_budget_with_cache_monitoring() {
    // GIVEN: Both features enabled
    // WHEN: Mix of simple and complex queries
    // THEN:
    //   - Adaptive budgets applied correctly
    //   - Cache metrics collected accurately
    //   - No feature conflicts
    //   - Dashboard shows combined insights
    // VERIFY: Cost savings ≥15%, hit rate ≥30%
}

#[tokio::test]
async fn test_production_workflow_simulation() {
    // GIVEN: Production-like configuration
    // WHEN: 1000 requests over 1 hour
    // THEN:
    //   - Response time unchanged (<5% regression)
    //   - Memory usage <+50MB
    //   - CPU usage <+10%
    //   - No resource leaks
}
```

---

## 📊 Performance Benchmarking Framework

### Setup

**Tools**:
- `criterion` - Rust benchmarking library
- `hyperfine` - Command-line benchmarking
- Custom latency tracker for production monitoring

**Metrics to Track**:
```yaml
budget_optimizer:
  classification_latency:
    target: "<50ms p95"
    measurement: "Histogram with percentiles"

  budget_calculation:
    target: "<10ms p99"
    measurement: "Histogram with percentiles"

  pattern_lookup:
    target: "<5ms p95"
    measurement: "Histogram with percentiles"

cache_monitor:
  cache_lookup_time:
    target: "<10ms p95"
    measurement: "Histogram with percentiles"

  metrics_collection_overhead:
    target: "<1% of request time"
    measurement: "Relative percentage"

  dashboard_update_latency:
    target: "<1s"
    measurement: "Real-time WebSocket latency"

integration:
  response_time_regression:
    target: "<5%"
    baseline: "Current production average"

  memory_overhead:
    target: "<50MB"
    baseline: "Current production baseline"

  cpu_overhead:
    target: "<10%"
    baseline: "Current production baseline"
```

---

## 🔍 Code Review Checklist

### Budget Optimizer Review (`budget_optimizer.rs`)

**Architecture & Design**:
- [ ] Follows Rust best practices (ownership, borrowing, lifetimes)
- [ ] Proper error handling (Result types, meaningful errors)
- [ ] Thread-safe (Arc, Mutex, RwLock used correctly)
- [ ] Async/await usage correct (no blocking in async context)

**Performance**:
- [ ] No unnecessary cloning or allocations
- [ ] Efficient data structures (HashMap, Vec usage)
- [ ] Classification algorithm <50ms p95
- [ ] Budget calculation <10ms p99
- [ ] Pattern lookup <5ms p95

**Edge Cases**:
- [ ] Empty prompts handled
- [ ] Very long prompts (>10K chars) handled
- [ ] Special characters, emoji, non-English text
- [ ] Concurrent access to pattern store
- [ ] Database unavailability handled gracefully

**Code Quality**:
- [ ] Clear function names and documentation
- [ ] Meaningful variable names
- [ ] No magic numbers (use constants)
- [ ] DRY principle followed
- [ ] SOLID principles applied

---

### Cache Monitor Review (`cache_monitor.rs`)

**Architecture & Design**:
- [ ] Metrics collection non-blocking
- [ ] Thread-safe metrics storage
- [ ] Efficient aggregation algorithms
- [ ] Proper WebSocket event handling

**Performance**:
- [ ] Metrics lookup <10ms p95
- [ ] Metrics write <5ms p95
- [ ] Memory usage bounded (no unbounded growth)
- [ ] Database writes batched for efficiency

**Edge Cases**:
- [ ] High request volume (>1000 req/s)
- [ ] Cache size limits exceeded
- [ ] Database write failures
- [ ] WebSocket connection loss
- [ ] Concurrent metrics updates

**Code Quality**:
- [ ] Clear separation of concerns
- [ ] Testable design (dependency injection)
- [ ] Comprehensive error handling
- [ ] Logging at appropriate levels

---

## 📝 Test Data Generation

### Complexity Classifier Test Data

**Generator**: `src-tauri/tests/fixtures/prompt_generator.rs`

```rust
pub struct PromptGenerator;

impl PromptGenerator {
    pub fn generate_simple_prompts(count: usize) -> Vec<String> {
        vec![
            "hello",
            "hi",
            "thanks",
            "yes",
            "no",
            "ok",
            "bye",
            "what time?",
            "how are you?",
            // ... 50 total
        ]
    }

    pub fn generate_moderate_prompts(count: usize) -> Vec<String> {
        vec![
            "explain quantum computing",
            "what is machine learning?",
            "summarize this article: ...",
            "how do I use Rust?",
            // ... 50 total
        ]
    }

    pub fn generate_complex_prompts(count: usize) -> Vec<String> {
        vec![
            "analyze the pros and cons of microservices vs monolithic architecture",
            "compare React, Vue, and Angular for enterprise applications",
            // ... 50 total
        ]
    }

    pub fn generate_deep_prompts(count: usize) -> Vec<String> {
        vec![
            "design a distributed tracing system for 100+ microservices",
            "research paper analysis: [10-page academic paper]",
            // ... 50 total
        ]
    }
}
```

### Cache Monitoring Test Data

**Mock Data**: `src-tauri/tests/fixtures/cache_mock_data.rs`

```rust
pub struct CacheMockData;

impl CacheMockData {
    pub fn generate_signature_stats(count: usize) -> Vec<SignatureStats> {
        // Generate realistic signature stats with varying reuse counts
    }

    pub fn generate_cache_metrics() -> CacheMetrics {
        // Generate realistic cache metrics for dashboard testing
    }

    pub fn simulate_request_pattern(duration: Duration) -> Vec<CacheEvent> {
        // Simulate realistic request patterns over time
    }
}
```

---

## ✅ Quality Gates

### Pre-Merge Checklist

**Story-008-01 (Adaptive Budget)**:
- [ ] All 6 acceptance criteria met (AC1-AC6)
- [ ] Unit tests ≥90% coverage
- [ ] Integration tests passing (4 tests minimum)
- [ ] Performance benchmarks met (<50ms classification)
- [ ] Code review approved
- [ ] Documentation complete
- [ ] No regressions in existing functionality

**Story-008-02 (Cache Monitoring)**:
- [ ] All 6 acceptance criteria met (AC1-AC6)
- [ ] Unit tests ≥90% coverage
- [ ] Integration tests passing (4 tests minimum)
- [ ] Performance benchmarks met (<10ms lookup)
- [ ] Code review approved
- [ ] Dashboard integration validated
- [ ] No regressions in existing functionality

**Story-008-03 (Integration)**:
- [ ] Both features work together seamlessly
- [ ] End-to-end tests passing
- [ ] Performance regression <5%
- [ ] Memory overhead <50MB
- [ ] CPU overhead <10%
- [ ] Documentation comprehensive
- [ ] Deployment guide validated

---

## 📚 Documentation Deliverables

### Testing Guides

1. **Adaptive Budget Testing Guide** (`docs/qa/adaptive-budget-testing-guide.md`)
   - How to run unit tests
   - How to generate test data
   - How to validate classification accuracy
   - Performance benchmarking procedures

2. **Cache Monitoring Testing Guide** (`docs/qa/cache-monitoring-testing-guide.md`)
   - How to test metrics collection
   - How to validate dashboard integration
   - How to simulate production load
   - Performance validation procedures

3. **Integration Testing Guide** (`docs/qa/epic-008-integration-testing.md`)
   - How to test both features together
   - End-to-end workflow validation
   - Performance regression testing
   - Production simulation procedures

### Troubleshooting Documentation

4. **Troubleshooting Guide** (`docs/qa/epic-008-troubleshooting.md`)
   - Common issues and solutions
   - Performance tuning recommendations
   - Debugging tips for developers
   - Error message reference

---

## 🚀 QA Timeline

### Week 1: Testing Support

**Days 1-2**: Test Infrastructure Setup
- Create test data generator
- Set up performance benchmarking framework
- Create mock data for cache monitoring
- Coordinate with Developer A1 and B1

**Days 3-5**: Developer A1 Support (Adaptive Budget)
- Review classifier implementation
- Create unit tests for complexity classifier
- Validate budget mapper logic
- Test edge cases (empty prompts, very long prompts)

**Days 6-7**: Developer B1 Support (Cache Monitoring)
- Review metrics collection implementation
- Create dashboard endpoint tests
- Validate real-time update mechanisms
- Test performance (<10ms lookup)

### Week 2: Integration & Quality

**Days 8-9**: Code Review
- Review `budget_optimizer.rs` implementation
- Review `cache_monitor.rs` implementation
- Check for race conditions, memory leaks
- Validate error handling

**Days 10-11**: Integration Testing
- Cross-module integration tests
- End-to-end workflow validation
- Performance regression testing
- Load testing (1000 req/s)

**Days 12-14**: Documentation & Completion
- Write testing guides
- Create troubleshooting documentation
- Validate deployment readiness
- Complete QA completion report

---

## 📊 Success Metrics

### Quantitative Metrics

```yaml
test_coverage:
  unit_tests: "≥90%"
  integration_tests: "100% critical paths"
  e2e_tests: "All user workflows"

performance:
  classification_latency: "<50ms p95"
  cache_lookup_time: "<10ms p95"
  response_time_regression: "<5%"
  memory_overhead: "<50MB"
  cpu_overhead: "<10%"

quality:
  code_review_issues: "0 critical"
  test_pass_rate: "100%"
  documentation_completeness: "100%"
```

### Qualitative Metrics

- **Developer Feedback**: "Tests are clear and comprehensive"
- **Code Quality**: "No major refactoring needed"
- **Documentation**: "Easy to understand and follow"
- **Integration**: "Features work seamlessly together"

---

## 🔗 References

**Epic Documents**:
- [Epic-008 Main Document](../epics/Epic-008-Gemini-2.5-Pro-Thinking-Optimization.md)
- Story-008-01: Adaptive Budget Optimization (to be created by Developer A1)
- Story-008-02: Cache Monitoring (to be created by Developer B1)

**Code References**:
- `src-tauri/src/proxy/handlers/openai.rs`
- `src-tauri/src/proxy/handlers/claude.rs`
- `src-tauri/src/proxy/mappers/common_utils.rs`
- `src-tauri/src/proxy/cache.rs` (existing cache implementation)
- `src-tauri/src/proxy/tests/` (existing test structure)

**Similar Testing Patterns**:
- `src-tauri/tests/image_generation_e2e.rs` (Epic-007 E2E tests)
- `src-tauri/src/proxy/tests/thinking_models.rs` (Thinking model tests)

---

## ✅ Next Steps

1. **Wait for Developer A1 and B1**: Stories not yet started
2. **Coordinate Testing Requirements**: Align on testing approach
3. **Set Up Test Infrastructure**: Create generators and frameworks
4. **Begin Code Review Support**: Review implementations as they progress
5. **Execute Integration Testing**: Once both stories are complete
6. **Complete Documentation**: Finalize all testing guides

---

**Document Status**: ✅ READY FOR EPIC-008 KICKOFF
**Next Action**: Coordinate with Developer A1 and B1 when Epic-008 starts
**Created By**: Developer C1 (QA Specialist)
**Date**: 2026-01-11
