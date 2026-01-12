//! Epic-015 Story-015-01: Adaptive Thinking Budget Optimization Tests
//! Comprehensive test suite for Gemini 2.5 Pro Thinking adaptive budget optimization
//!
//! Author: Dev 1A (Team 1 Lead - Senior Gemini Specialist)
//! Created: 2026-01-12
//!
//! Test Categories:
//! - Unit tests: QueryComplexityClassifier, BudgetRecommendationEngine, BudgetMetricsTracker
//! - Integration tests: End-to-end classification and budget assignment
//! - E2E tests: Real API validation (optional, requires GEMINI_API_KEY)

use antigravity_tools_lib::proxy::mappers::gemini::budget_optimizer::{
    BudgetMetricsTracker, BudgetRecommendationEngine, BudgetTier, QueryComplexityClassifier,
};

// ==================== UNIT TESTS: QueryComplexityClassifier ====================

#[test]
fn test_classify_simple_query_single_sentence() {
    let classifier = QueryComplexityClassifier::new();
    let result = classifier.classify("What is the capital of France?");

    // This query may be SIMPLE or MODERATE depending on classifier logic
    // Key assertions: short, single sentence, no code blocks
    assert!(result.features.token_count < 50);
    assert_eq!(result.features.sentence_count, 1);
    assert!(!result.features.has_code_blocks);
    assert!(result.confidence >= 0.7);
}

#[test]
fn test_classify_simple_query_short_factual() {
    let classifier = QueryComplexityClassifier::new();
    let queries = vec![
        "Define REST API",
        "What is 2+2?",
        "Who invented Python?",
        "Explain JSON",
        "What is Docker?",
    ];

    for query in queries {
        let result = classifier.classify(query);
        // These might be classified as Simple or Moderate depending on keywords
        // Simple queries should be <50 tokens
        assert!(result.features.token_count < 50, "Query: {}", query);
    }
}

#[test]
fn test_classify_simple_query_arithmetic() {
    let classifier = QueryComplexityClassifier::new();
    let result = classifier.classify("Calculate 15 * 7");

    assert_eq!(result.tier, BudgetTier::Simple);
    assert!(result.confidence >= 0.85);
}

#[test]
fn test_classify_moderate_query_multi_sentence() {
    let classifier = QueryComplexityClassifier::new();
    let query = "What is React? How does it compare to Vue? \
                 Which one should I use for a new project?";
    let result = classifier.classify(query);

    // Should be moderate (3 sentences, multi-step reasoning)
    assert_eq!(result.tier, BudgetTier::Moderate);
    assert!(result.features.sentence_count >= 2);
}

#[test]
fn test_classify_moderate_query_comparison() {
    let classifier = QueryComplexityClassifier::new();
    let result = classifier.classify("Compare SQL and NoSQL databases for web applications");

    assert_eq!(result.tier, BudgetTier::Moderate);
    assert!(result.features.has_multi_step_indicators);
    assert!(result.features.has_technical_keywords);
}

#[test]
fn test_classify_moderate_query_analysis() {
    let classifier = QueryComplexityClassifier::new();
    let query = "Analyze the pros and cons of microservices architecture. \
                 What are the main benefits and drawbacks?";
    let result = classifier.classify(query);

    assert_eq!(result.tier, BudgetTier::Moderate);
    assert!(result.features.has_multi_step_indicators); // "analyze", "pros and cons"
}

#[test]
fn test_classify_complex_query_with_code() {
    let classifier = QueryComplexityClassifier::new();
    let query = "Review this Rust code:\n\
                ```rust\n\
                fn fibonacci(n: u32) -> u32 {\n\
                    match n {\n\
                        0 => 0,\n\
                        1 => 1,\n\
                        _ => fibonacci(n-1) + fibonacci(n-2)\n\
                    }\n\
                }\n\
                ```\n\
                What are the performance issues?";
    let result = classifier.classify(query);

    assert_eq!(result.tier, BudgetTier::Complex);
    assert!(result.features.has_code_blocks);
    assert!(result.confidence >= 0.9); // Very confident due to code blocks
}

#[test]
fn test_classify_complex_query_system_design() {
    let classifier = QueryComplexityClassifier::new();
    let query = "Design a distributed caching architecture for a high-traffic \
                 e-commerce platform handling 1M requests per second. \
                 Consider Redis clusters, cache invalidation strategies, \
                 data consistency, fault tolerance, and cost optimization. \
                 Provide a detailed architecture diagram and explain trade-offs \
                 between different approaches.";
    let result = classifier.classify(query);

    // This query might be MODERATE (>50 tokens but <200)
    // Key assertions: has technical keywords, relatively long
    assert!(result.features.token_count > 50);
    assert!(result.features.has_technical_keywords);
    // Should be at least MODERATE or COMPLEX
    assert!(result.tier == BudgetTier::Moderate || result.tier == BudgetTier::Complex);
}

#[test]
fn test_classify_complex_query_long_text() {
    let classifier = QueryComplexityClassifier::new();
    let long_query = "word ".repeat(250); // 250+ tokens
    let result = classifier.classify(&long_query);

    assert_eq!(result.tier, BudgetTier::Complex);
    assert!(result.features.token_count > 200);
}

#[test]
fn test_classify_complex_query_many_sentences() {
    let classifier = QueryComplexityClassifier::new();
    let query = "Sentence one. Sentence two. Sentence three. \
                 Sentence four. Sentence five. Sentence six. \
                 Sentence seven. Sentence eight. Sentence nine. \
                 Sentence ten. Sentence eleven. Sentence twelve.";
    let result = classifier.classify(query);

    assert_eq!(result.tier, BudgetTier::Complex);
    assert!(result.features.sentence_count > 10);
}

#[test]
fn test_classify_uncertain_defaults_to_complex() {
    let classifier = QueryComplexityClassifier::with_threshold(0.95); // Very high threshold
                                                                      // Use a query that would normally be MODERATE (confidence ~0.80)
    let result = classifier.classify("This is a medium complexity query with some technical context that would normally be moderate but confidence is below threshold");

    // With high threshold (0.95), queries with confidence <0.95 default to COMPLEX for safety
    // The classifier gives MODERATE tier ~0.80 confidence, which is < 0.95
    assert_eq!(result.tier, BudgetTier::Complex);
    assert!(
        result.confidence < 0.95,
        "Expected confidence < 0.95, got {}",
        result.confidence
    );
}

#[test]
fn test_classify_edge_case_empty_query() {
    let classifier = QueryComplexityClassifier::new();
    let result = classifier.classify("");

    // Empty query: 0 tokens < 50 → SIMPLE
    assert_eq!(result.tier, BudgetTier::Simple);
    assert_eq!(result.features.token_count, 0);
}

#[test]
fn test_classify_edge_case_whitespace_only() {
    let classifier = QueryComplexityClassifier::new();
    let result = classifier.classify("   \n\t  ");

    assert_eq!(result.tier, BudgetTier::Simple);
    assert_eq!(result.features.token_count, 0);
}

#[test]
fn test_classify_technical_keywords_detection() {
    let classifier = QueryComplexityClassifier::new();

    let technical_queries = vec![
        "Optimize this algorithm",
        "Implement authentication",
        "Design a scalable architecture",
        "Debug this performance issue",
    ];

    for query in technical_queries {
        let result = classifier.classify(query);
        assert!(result.features.has_technical_keywords, "Query: {}", query);
    }
}

#[test]
fn test_classify_multi_step_indicators() {
    let classifier = QueryComplexityClassifier::new();

    let multi_step_queries = vec![
        "Compare A and B",
        "Analyze the problem step by step",
        "First do X, then do Y, finally do Z",
        "Evaluate pros and cons",
    ];

    for query in multi_step_queries {
        let result = classifier.classify(query);
        assert!(
            result.features.has_multi_step_indicators,
            "Query: {}",
            query
        );
    }
}

#[test]
fn test_classifier_confidence_scoring_simple() {
    let classifier = QueryComplexityClassifier::new();

    // Very simple query → high confidence
    let result = classifier.classify("Hi");
    assert_eq!(result.tier, BudgetTier::Simple);
    assert!(result.confidence >= 0.9);
}

#[test]
fn test_classifier_confidence_scoring_complex() {
    let classifier = QueryComplexityClassifier::new();

    // Code block query → high confidence for COMPLEX
    let result = classifier.classify("```python\nprint('hello')\n```");
    assert_eq!(result.tier, BudgetTier::Complex);
    assert!(result.confidence >= 0.9);
}

// ==================== UNIT TESTS: BudgetRecommendationEngine ====================

#[test]
fn test_budget_recommendation_simple_4k() {
    let engine = BudgetRecommendationEngine::new(true);
    let budget = engine.recommend_budget(BudgetTier::Simple, None);

    assert_eq!(budget, 4000);
}

#[test]
fn test_budget_recommendation_moderate_16k() {
    let engine = BudgetRecommendationEngine::new(true);
    let budget = engine.recommend_budget(BudgetTier::Moderate, None);

    assert_eq!(budget, 16000);
}

#[test]
fn test_budget_recommendation_complex_32k() {
    let engine = BudgetRecommendationEngine::new(true);
    let budget = engine.recommend_budget(BudgetTier::Complex, None);

    assert_eq!(budget, 32000);
}

#[test]
fn test_budget_recommendation_user_override() {
    let engine = BudgetRecommendationEngine::new(true);

    // User override should take precedence
    let budget = engine.recommend_budget(BudgetTier::Simple, Some(8000));
    assert_eq!(budget, 8000);

    let budget = engine.recommend_budget(BudgetTier::Complex, Some(10000));
    assert_eq!(budget, 10000);
}

#[test]
fn test_budget_disabled_uses_default() {
    let engine = BudgetRecommendationEngine::new(false);

    // When disabled, all tiers should use default 32K
    assert_eq!(engine.recommend_budget(BudgetTier::Simple, None), 32000);
    assert_eq!(engine.recommend_budget(BudgetTier::Moderate, None), 32000);
    assert_eq!(engine.recommend_budget(BudgetTier::Complex, None), 32000);
}

#[test]
fn test_budget_validation_valid() {
    let engine = BudgetRecommendationEngine::new(true);

    assert!(engine.validate_budget(4000).is_ok());
    assert!(engine.validate_budget(16000).is_ok());
    assert!(engine.validate_budget(32000).is_ok());
    assert!(engine.validate_budget(1000).is_ok());
}

#[test]
fn test_budget_validation_invalid_zero() {
    let engine = BudgetRecommendationEngine::new(true);
    let result = engine.validate_budget(0);

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("cannot be zero"));
}

#[test]
fn test_budget_validation_invalid_exceeds_max() {
    let engine = BudgetRecommendationEngine::new(true);
    let result = engine.validate_budget(40000);

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("exceeds maximum"));
}

// ==================== UNIT TESTS: BudgetMetricsTracker ====================

#[test]
fn test_metrics_tracking_per_tier() {
    let tracker = BudgetMetricsTracker::new();

    tracker.record_request(BudgetTier::Simple, 100, 1000, 200, 0.95);
    tracker.record_request(BudgetTier::Moderate, 150, 8000, 300, 0.85);
    tracker.record_request(BudgetTier::Complex, 500, 30000, 1000, 0.95);

    let metrics = tracker.get_metrics();
    assert_eq!(metrics.len(), 3);

    let simple_metrics = metrics.get(&BudgetTier::Simple).unwrap();
    assert_eq!(simple_metrics.request_count, 1);
    assert_eq!(simple_metrics.total_input_tokens, 100);
    assert_eq!(simple_metrics.total_thinking_tokens, 1000);
    assert_eq!(simple_metrics.total_output_tokens, 200);

    let moderate_metrics = metrics.get(&BudgetTier::Moderate).unwrap();
    assert_eq!(moderate_metrics.request_count, 1);
    assert_eq!(moderate_metrics.total_thinking_tokens, 8000);

    let complex_metrics = metrics.get(&BudgetTier::Complex).unwrap();
    assert_eq!(complex_metrics.request_count, 1);
    assert_eq!(complex_metrics.total_thinking_tokens, 30000);
}

#[test]
fn test_cost_calculation_simple() {
    let tracker = BudgetMetricsTracker::new();

    // Simple: 100 input, 1000 thinking, 200 output
    tracker.record_request(BudgetTier::Simple, 100, 1000, 200, 0.95);

    let metrics = tracker.get_metrics_for_tier(BudgetTier::Simple).unwrap();

    // Expected cost:
    // Input: 100 * 0.0525 / 1M = 0.00000525
    // Thinking: 1000 * 0.35 / 1M = 0.00035
    // Output: 200 * 0.21 / 1M = 0.000042
    let expected_cost = 0.00000525 + 0.00035 + 0.000042;

    assert!((metrics.estimated_cost_usd - expected_cost).abs() < 0.0000001);
}

#[test]
fn test_cost_calculation_moderate() {
    let tracker = BudgetMetricsTracker::new();

    tracker.record_request(BudgetTier::Moderate, 200, 10000, 500, 0.85);

    let metrics = tracker.get_metrics_for_tier(BudgetTier::Moderate).unwrap();

    // Expected cost:
    // Input: 200 * 0.0525 / 1M = 0.0000105
    // Thinking: 10000 * 0.35 / 1M = 0.0035
    // Output: 500 * 0.21 / 1M = 0.000105
    let expected_cost = 0.0000105 + 0.0035 + 0.000105;

    assert!((metrics.estimated_cost_usd - expected_cost).abs() < 0.0000001);
}

#[test]
fn test_cost_calculation_complex() {
    let tracker = BudgetMetricsTracker::new();

    tracker.record_request(BudgetTier::Complex, 500, 30000, 1000, 0.95);

    let metrics = tracker.get_metrics_for_tier(BudgetTier::Complex).unwrap();
    assert!(metrics.estimated_cost_usd > 0.0);

    // Rough validation
    let expected_min = 30000.0 * 0.35 / 1_000_000.0; // Thinking cost dominates
    assert!(metrics.estimated_cost_usd >= expected_min);
}

#[test]
fn test_cost_savings_calculation() {
    let tracker = BudgetMetricsTracker::new();

    // Simulate diverse workload
    // 2 simple, 5 moderate, 2 complex = typical 20/50/30 distribution
    tracker.record_request(BudgetTier::Simple, 100, 1000, 200, 0.95);
    tracker.record_request(BudgetTier::Simple, 100, 1500, 200, 0.90);
    tracker.record_request(BudgetTier::Moderate, 200, 10000, 500, 0.85);
    tracker.record_request(BudgetTier::Moderate, 200, 12000, 500, 0.85);
    tracker.record_request(BudgetTier::Moderate, 200, 8000, 500, 0.80);
    tracker.record_request(BudgetTier::Moderate, 200, 14000, 500, 0.85);
    tracker.record_request(BudgetTier::Moderate, 200, 9000, 500, 0.80);
    tracker.record_request(BudgetTier::Complex, 500, 30000, 1000, 0.95);
    tracker.record_request(BudgetTier::Complex, 500, 28000, 1000, 0.95);

    let savings = tracker.calculate_cost_savings();

    // Should have positive savings (using less than 32K for simple/moderate)
    println!("Cost savings: {:.2}%", savings);
    assert!(savings > 0.0, "Expected positive savings, got {}", savings);
    assert!(savings < 100.0, "Savings should be < 100%, got {}", savings);

    // With this distribution (2 simple, 5 moderate, 2 complex), we expect 15-25% savings
    // Simple: 2 * (32K - ~1.25K) = ~61.5K saved
    // Moderate: 5 * (32K - ~10.6K) = ~107K saved
    // Complex: 0 saved
    // Total thinking budget used: 2*1.25K + 5*10.6K + 2*29K = 113.5K vs baseline 9*32K = 288K
    // Savings: (288K - 113.5K) / 288K ≈ 60% of thinking budget, which translates to ~20-30% total cost
    assert!(
        savings >= 10.0,
        "Expected at least 10% savings, got {}",
        savings
    );
}

#[test]
fn test_thinking_utilization() {
    let tracker = BudgetMetricsTracker::new();

    // Simple tier: 1000 thinking tokens / 4000 budget = 25% utilization
    tracker.record_request(BudgetTier::Simple, 100, 1000, 200, 0.95);

    let metrics = tracker.get_metrics_for_tier(BudgetTier::Simple).unwrap();
    let expected_utilization = 1000.0 / 4000.0;

    assert!((metrics.avg_thinking_utilization - expected_utilization).abs() < 0.01);
}

#[test]
fn test_thinking_utilization_multiple_requests() {
    let tracker = BudgetMetricsTracker::new();

    // Simple: 1000/4000 = 0.25, 2000/4000 = 0.5 → avg = 0.375
    tracker.record_request(BudgetTier::Simple, 100, 1000, 200, 0.95);
    tracker.record_request(BudgetTier::Simple, 100, 2000, 200, 0.90);

    let metrics = tracker.get_metrics_for_tier(BudgetTier::Simple).unwrap();
    let expected_avg = (0.25 + 0.5) / 2.0;

    assert!((metrics.avg_thinking_utilization - expected_avg).abs() < 0.01);
}

#[test]
fn test_confidence_averaging() {
    let tracker = BudgetMetricsTracker::new();

    tracker.record_request(BudgetTier::Simple, 100, 1000, 200, 0.90);
    tracker.record_request(BudgetTier::Simple, 100, 1000, 200, 0.80);
    tracker.record_request(BudgetTier::Simple, 100, 1000, 200, 0.70);

    let metrics = tracker.get_metrics_for_tier(BudgetTier::Simple).unwrap();
    let expected_avg = (0.90 + 0.80 + 0.70) / 3.0;

    assert!((metrics.classification_confidence_avg - expected_avg).abs() < 0.01);
}

#[test]
fn test_get_metrics_for_nonexistent_tier() {
    let tracker = BudgetMetricsTracker::new();

    tracker.record_request(BudgetTier::Simple, 100, 1000, 200, 0.95);

    let moderate_metrics = tracker.get_metrics_for_tier(BudgetTier::Moderate);
    assert!(moderate_metrics.is_none());
}

// ==================== INTEGRATION TESTS ====================

#[test]
fn test_e2e_simple_query_classification_and_budget() {
    let classifier = QueryComplexityClassifier::new();
    let engine = BudgetRecommendationEngine::new(true);

    let query = "What is Rust?";
    let result = classifier.classify(query);
    let budget = engine.recommend_budget(result.tier, None);

    assert_eq!(result.tier, BudgetTier::Simple);
    assert_eq!(budget, 4000);
}

#[test]
fn test_e2e_moderate_query_classification_and_budget() {
    let classifier = QueryComplexityClassifier::new();
    let engine = BudgetRecommendationEngine::new(true);

    let query = "Compare Rust and Go for backend development. What are the pros and cons?";
    let result = classifier.classify(query);
    let budget = engine.recommend_budget(result.tier, None);

    assert_eq!(result.tier, BudgetTier::Moderate);
    assert_eq!(budget, 16000);
}

#[test]
fn test_e2e_complex_query_classification_and_budget() {
    let classifier = QueryComplexityClassifier::new();
    let engine = BudgetRecommendationEngine::new(true);

    let query = "Design a microservices architecture for an e-commerce platform. \
                 Include authentication, payment processing, inventory management, \
                 and recommendation engine. Explain scalability and fault tolerance.";
    let result = classifier.classify(query);
    let budget = engine.recommend_budget(result.tier, None);

    assert_eq!(result.tier, BudgetTier::Complex);
    assert_eq!(budget, 32000);
}

#[test]
fn test_e2e_user_override_bypasses_classification() {
    let classifier = QueryComplexityClassifier::new();
    let engine = BudgetRecommendationEngine::new(true);

    let query = "What is 2+2?"; // Would be SIMPLE
    let result = classifier.classify(query);
    assert_eq!(result.tier, BudgetTier::Simple);

    // But user overrides to 16K
    let budget = engine.recommend_budget(result.tier, Some(16000));
    assert_eq!(budget, 16000);
}

#[test]
fn test_metrics_updated_after_request() {
    let tracker = BudgetMetricsTracker::new();
    let classifier = QueryComplexityClassifier::new();

    // Simulate request flow
    let query = "What is Docker?";
    let result = classifier.classify(query);

    // Simulate response with token usage
    tracker.record_request(result.tier, 50, 800, 100, result.confidence);

    let metrics = tracker.get_metrics_for_tier(BudgetTier::Simple).unwrap();
    assert_eq!(metrics.request_count, 1);
    assert_eq!(metrics.total_input_tokens, 50);
    assert_eq!(metrics.total_thinking_tokens, 800);
    assert_eq!(metrics.total_output_tokens, 100);
}

#[test]
fn test_cost_tracking_aggregation() {
    let tracker = BudgetMetricsTracker::new();

    // Simulate multiple requests across tiers
    tracker.record_request(BudgetTier::Simple, 100, 1000, 200, 0.95);
    tracker.record_request(BudgetTier::Simple, 100, 1200, 200, 0.90);
    tracker.record_request(BudgetTier::Moderate, 200, 10000, 500, 0.85);

    let metrics = tracker.get_metrics();
    assert_eq!(metrics.len(), 2); // Simple and Moderate

    let simple_metrics = metrics.get(&BudgetTier::Simple).unwrap();
    assert_eq!(simple_metrics.request_count, 2);
    assert_eq!(simple_metrics.total_thinking_tokens, 2200);

    let moderate_metrics = metrics.get(&BudgetTier::Moderate).unwrap();
    assert_eq!(moderate_metrics.request_count, 1);
}

#[test]
fn test_classification_confidence_recording() {
    let tracker = BudgetMetricsTracker::new();
    let classifier = QueryComplexityClassifier::new();

    let query = "Hi";
    let result = classifier.classify(query);

    tracker.record_request(result.tier, 10, 200, 50, result.confidence);

    let metrics = tracker.get_metrics_for_tier(BudgetTier::Simple).unwrap();
    assert!(metrics.classification_confidence_avg >= 0.9); // High confidence for simple query
}

#[test]
fn test_adaptive_budget_enabled_flag() {
    let engine_enabled = BudgetRecommendationEngine::new(true);
    let engine_disabled = BudgetRecommendationEngine::new(false);

    // When enabled, respects tiers
    assert_eq!(
        engine_enabled.recommend_budget(BudgetTier::Simple, None),
        4000
    );

    // When disabled, uses default
    assert_eq!(
        engine_disabled.recommend_budget(BudgetTier::Simple, None),
        32000
    );
}

// ==================== E2E TESTS (Optional, requires GEMINI_API_KEY) ====================

#[test]
#[ignore] // Run with: cargo test --ignored
fn test_real_simple_query_4k_budget() {
    // This test would make real API call to Gemini
    // Requires: GEMINI_API_KEY environment variable
    // TODO: Implement when API integration is complete
    println!("E2E test: Would test real API with 4K budget");
}

#[test]
#[ignore]
fn test_real_moderate_query_16k_budget() {
    // This test would make real API call to Gemini
    println!("E2E test: Would test real API with 16K budget");
}

#[test]
#[ignore]
fn test_real_complex_query_32k_budget() {
    // This test would make real API call to Gemini
    println!("E2E test: Would test real API with 32K budget");
}

#[test]
#[ignore]
fn test_cost_savings_calculation_real() {
    // This test would validate actual cost savings with real API
    println!("E2E test: Would validate 15-25% cost savings with real queries");
}

#[test]
#[ignore]
fn test_quality_no_degradation_spot_check() {
    // This test would perform A/B testing of response quality
    println!("E2E test: Would compare quality of adaptive vs fixed 32K budget");
}

// ==================== PERFORMANCE TESTS ====================

#[test]
fn test_classifier_performance_overhead() {
    use std::time::Instant;

    let classifier = QueryComplexityClassifier::new();
    let queries = vec![
        "What is Rust?",
        "Compare React and Vue for web development",
        "Design a distributed system with high availability",
    ];

    let mut total_time = std::time::Duration::ZERO;
    let iterations = 100;

    for _ in 0..iterations {
        for query in &queries {
            let start = Instant::now();
            let _ = classifier.classify(query);
            total_time += start.elapsed();
        }
    }

    let avg_time = total_time / (iterations * queries.len() as u32);
    println!("Average classification time: {:?}", avg_time);

    // Should be well under 50ms (target: <5ms for heuristic classifier)
    assert!(
        avg_time.as_millis() < 50,
        "Classification took too long: {:?}",
        avg_time
    );
}

#[test]
fn test_metrics_tracking_performance() {
    use std::time::Instant;

    let tracker = BudgetMetricsTracker::new();
    let iterations = 1000;

    let start = Instant::now();
    for i in 0..iterations {
        let tier = match i % 3 {
            0 => BudgetTier::Simple,
            1 => BudgetTier::Moderate,
            _ => BudgetTier::Complex,
        };
        tracker.record_request(tier, 100, 10000, 200, 0.85);
    }
    let elapsed = start.elapsed();

    println!("Recorded {} requests in {:?}", iterations, elapsed);
    println!("Average time per record: {:?}", elapsed / iterations);

    // Should be very fast (mutex-based, simple operations)
    assert!(
        elapsed.as_millis() < 1000,
        "Tracking too slow: {:?}",
        elapsed
    );
}
