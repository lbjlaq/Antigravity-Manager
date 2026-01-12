// Epic-015 Story-015-01: Adaptive Thinking Budget Optimization
// Implements dynamic thinking budget selection based on query complexity analysis
// to achieve 15-25% cost savings on simple queries while maintaining quality on complex reasoning.
//
// Author: Dev 1A (Team 1 Lead - Senior Gemini Specialist)
// Created: 2026-01-12

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Budget tier classification for Gemini 2.5 Pro Thinking model
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BudgetTier {
    /// Simple queries: <50 tokens, single sentence, basic factual lookup
    /// Budget: 4K tokens
    Simple,

    /// Moderate queries: 50-200 tokens, 2-5 reasoning steps, multi-sentence with context
    /// Budget: 16K tokens
    Moderate,

    /// Complex queries: >200 tokens, >5 reasoning steps, code analysis, system design
    /// Budget: 32K tokens (safe default)
    Complex,
}

impl BudgetTier {
    /// Convert tier to token budget
    pub fn to_token_budget(&self) -> u32 {
        match self {
            BudgetTier::Simple => 4000,
            BudgetTier::Moderate => 16000,
            BudgetTier::Complex => 32000,
        }
    }
}

impl std::fmt::Display for BudgetTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BudgetTier::Simple => write!(f, "SIMPLE"),
            BudgetTier::Moderate => write!(f, "MODERATE"),
            BudgetTier::Complex => write!(f, "COMPLEX"),
        }
    }
}

/// Extracted features from a query for complexity classification
#[derive(Debug, Clone)]
pub struct QueryFeatures {
    /// Total token count (simple whitespace-based tokenization)
    pub token_count: usize,

    /// Number of sentences (split by . ? !)
    pub sentence_count: usize,

    /// Whether query contains code blocks (```)
    pub has_code_blocks: bool,

    /// Whether query contains technical keywords (algorithm, architecture, etc.)
    pub has_technical_keywords: bool,

    /// Whether query contains multi-step reasoning indicators (compare, analyze, etc.)
    pub has_multi_step_indicators: bool,

    /// Average sentence length (tokens per sentence)
    pub avg_sentence_length: f64,
}

/// Classification result with tier, confidence score, and extracted features
#[derive(Debug, Clone)]
pub struct ClassificationResult {
    /// Assigned budget tier
    pub tier: BudgetTier,

    /// Confidence score (0.0-1.0)
    /// If confidence < 0.7, defaults to COMPLEX for safety
    pub confidence: f64,

    /// Extracted query features
    pub features: QueryFeatures,
}

/// Budget metrics for tracking cost and performance by tier
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetMetrics {
    /// Budget tier
    pub tier: BudgetTier,

    /// Total number of requests in this tier
    pub request_count: u64,

    /// Total input tokens consumed
    pub total_input_tokens: u64,

    /// Total thinking tokens consumed
    pub total_thinking_tokens: u64,

    /// Total output tokens consumed
    pub total_output_tokens: u64,

    /// Estimated cost in USD (based on Gemini 2.5 Pro Thinking pricing)
    pub estimated_cost_usd: f64,

    /// Average thinking token utilization (actual_thinking / budget_allocated)
    pub avg_thinking_utilization: f64,

    /// Average classification confidence
    pub classification_confidence_avg: f64,

    /// Last updated timestamp
    #[serde(with = "chrono::serde::ts_seconds")]
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

impl BudgetMetrics {
    /// Create new empty metrics for a tier
    pub fn new(tier: BudgetTier) -> Self {
        Self {
            tier,
            request_count: 0,
            total_input_tokens: 0,
            total_thinking_tokens: 0,
            total_output_tokens: 0,
            estimated_cost_usd: 0.0,
            avg_thinking_utilization: 0.0,
            classification_confidence_avg: 0.0,
            last_updated: chrono::Utc::now(),
        }
    }
}

/// Query complexity classifier
/// Heuristic-based classifier (v1) using token count, keyword patterns, and structural analysis
pub struct QueryComplexityClassifier {
    /// Confidence threshold for classification (default: 0.7)
    /// If confidence < threshold, defaults to COMPLEX for safety
    confidence_threshold: f64,

    /// Technical keywords for domain detection
    technical_keywords: Vec<&'static str>,

    /// Multi-step reasoning indicators
    multi_step_indicators: Vec<&'static str>,
}

impl QueryComplexityClassifier {
    /// Create new classifier with default threshold (0.7)
    pub fn new() -> Self {
        Self {
            confidence_threshold: 0.7,
            technical_keywords: vec![
                "algorithm", "architecture", "optimize", "implement",
                "debug", "refactor", "design pattern", "scalability",
                "performance", "security", "database", "api", "endpoint",
                "authentication", "authorization", "encryption", "cache",
                "distributed", "microservice", "kubernetes", "docker",
            ],
            multi_step_indicators: vec![
                "compare", "analyze", "evaluate", "pros and cons",
                "step by step", "first", "then", "finally", "next",
                "explain", "describe", "list", "enumerate",
            ],
        }
    }

    /// Create classifier with custom confidence threshold
    pub fn with_threshold(threshold: f64) -> Self {
        let mut classifier = Self::new();
        classifier.confidence_threshold = threshold;
        classifier
    }

    /// Classify a query and return classification result
    pub fn classify(&self, query: &str) -> ClassificationResult {
        let features = self.extract_features(query);
        let tier = self.heuristic_classify(&features);
        let confidence = self.calculate_confidence(&features, tier);

        // Safe fallback: low confidence → COMPLEX
        let final_tier = if confidence < self.confidence_threshold {
            tracing::debug!(
                "[BudgetOptimizer] Low confidence ({:.2}), defaulting to COMPLEX for safety",
                confidence
            );
            BudgetTier::Complex
        } else {
            tier
        };

        tracing::debug!(
            "[BudgetOptimizer] Classification: tier={}, confidence={:.2}, tokens={}, sentences={}",
            final_tier,
            confidence,
            features.token_count,
            features.sentence_count
        );

        ClassificationResult {
            tier: final_tier,
            confidence,
            features,
        }
    }

    /// Extract features from query text
    fn extract_features(&self, query: &str) -> QueryFeatures {
        let tokens: Vec<&str> = query.split_whitespace().collect();
        let sentences: Vec<&str> = query
            .split(&['.', '?', '!'][..])
            .filter(|s| !s.trim().is_empty())
            .collect();

        let token_count = tokens.len();
        let sentence_count = sentences.len().max(1); // Avoid division by zero

        QueryFeatures {
            token_count,
            sentence_count,
            has_code_blocks: query.contains("```"),
            has_technical_keywords: self.detect_technical_keywords(query),
            has_multi_step_indicators: self.detect_multi_step_indicators(query),
            avg_sentence_length: token_count as f64 / sentence_count as f64,
        }
    }

    /// Heuristic-based classification using rule-based logic
    fn heuristic_classify(&self, features: &QueryFeatures) -> BudgetTier {
        // COMPLEX indicators (highest priority)
        if features.has_code_blocks
            || features.token_count > 200
            || features.sentence_count > 10
        {
            return BudgetTier::Complex;
        }

        // SIMPLE indicators (must have NO complexity indicators)
        // Simple: short, single-sentence, no technical keywords, no multi-step reasoning
        if features.token_count < 50
            && features.sentence_count <= 2
            && !features.has_multi_step_indicators
        {
            // Technical keywords alone don't make it complex if query is very short
            // e.g., "What is Docker?" is still simple even with technical keyword
            if features.token_count < 20 || !features.has_technical_keywords {
                return BudgetTier::Simple;
            }
        }

        // Default to MODERATE
        BudgetTier::Moderate
    }

    /// Calculate confidence score based on feature clarity
    fn calculate_confidence(&self, features: &QueryFeatures, tier: BudgetTier) -> f64 {
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

    /// Detect technical keywords in query
    fn detect_technical_keywords(&self, query: &str) -> bool {
        let query_lower = query.to_lowercase();
        self.technical_keywords
            .iter()
            .any(|k| query_lower.contains(k))
    }

    /// Detect multi-step reasoning indicators
    fn detect_multi_step_indicators(&self, query: &str) -> bool {
        let query_lower = query.to_lowercase();
        self.multi_step_indicators
            .iter()
            .any(|i| query_lower.contains(i))
    }
}

impl Default for QueryComplexityClassifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Budget recommendation engine
/// Maps classified tiers to token budgets with override support
pub struct BudgetRecommendationEngine {
    /// Whether adaptive budget optimization is enabled
    adaptive_enabled: bool,
}

impl BudgetRecommendationEngine {
    /// Create new recommendation engine
    pub fn new(adaptive_enabled: bool) -> Self {
        Self { adaptive_enabled }
    }

    /// Recommend budget based on tier, user override, and configuration
    ///
    /// Priority order:
    /// 1. User override (if provided)
    /// 2. Feature flag check (if disabled, return default 32K)
    /// 3. Tier-based budget (if enabled)
    pub fn recommend_budget(
        &self,
        tier: BudgetTier,
        user_override: Option<u32>,
    ) -> u32 {
        // User override takes precedence
        if let Some(budget) = user_override {
            tracing::debug!(
                "[BudgetOptimizer] Using user override: {} tokens",
                budget
            );
            return budget;
        }

        // Feature flag check
        if !self.adaptive_enabled {
            tracing::debug!(
                "[BudgetOptimizer] Adaptive budget disabled, using default 32K"
            );
            return 32000; // Default fixed budget
        }

        // Return tier-based budget
        let budget = tier.to_token_budget();
        tracing::debug!(
            "[BudgetOptimizer] Recommending {} tokens for {} tier",
            budget,
            tier
        );
        budget
    }

    /// Validate budget is within acceptable range
    pub fn validate_budget(&self, budget: u32) -> Result<u32, String> {
        if budget == 0 {
            return Err("Budget cannot be zero".to_string());
        }

        if budget > 32000 {
            return Err(format!(
                "Budget {} exceeds maximum 32K tokens",
                budget
            ));
        }

        Ok(budget)
    }
}

/// Budget metrics tracker
/// Tracks request counts, token usage, and costs by budget tier
pub struct BudgetMetricsTracker {
    /// Metrics by tier
    metrics: Arc<Mutex<HashMap<BudgetTier, BudgetMetrics>>>,
}

impl BudgetMetricsTracker {
    /// Create new metrics tracker
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Record a request with its token usage and confidence
    pub fn record_request(
        &self,
        tier: BudgetTier,
        input_tokens: u64,
        thinking_tokens: u64,
        output_tokens: u64,
        confidence: f64,
    ) {
        let mut metrics = self.metrics.lock().unwrap();
        let entry = metrics
            .entry(tier)
            .or_insert_with(|| BudgetMetrics::new(tier));

        entry.request_count += 1;
        entry.total_input_tokens += input_tokens;
        entry.total_thinking_tokens += thinking_tokens;
        entry.total_output_tokens += output_tokens;

        // Cost calculation (Gemini 2.5 Pro Thinking rates as of 2026-01)
        // Input: $0.0525/1M tokens, Thinking: $0.35/1M tokens, Output: $0.21/1M tokens
        let cost = (input_tokens as f64 * 0.0525 / 1_000_000.0)
            + (thinking_tokens as f64 * 0.35 / 1_000_000.0)
            + (output_tokens as f64 * 0.21 / 1_000_000.0);
        entry.estimated_cost_usd += cost;

        // Update rolling average confidence
        let prev_total = entry.classification_confidence_avg * (entry.request_count - 1) as f64;
        entry.classification_confidence_avg = (prev_total + confidence) / entry.request_count as f64;

        // Update thinking utilization (average of actual_thinking / budget_allocated)
        let budget = tier.to_token_budget() as f64;
        let utilization = thinking_tokens as f64 / budget;
        let prev_util_total = entry.avg_thinking_utilization * (entry.request_count - 1) as f64;
        entry.avg_thinking_utilization = (prev_util_total + utilization) / entry.request_count as f64;

        entry.last_updated = chrono::Utc::now();

        tracing::debug!(
            "[BudgetOptimizer] Recorded request: tier={}, cost=${:.6}, utilization={:.2}%",
            tier,
            cost,
            utilization * 100.0
        );
    }

    /// Get all metrics by tier
    pub fn get_metrics(&self) -> HashMap<BudgetTier, BudgetMetrics> {
        self.metrics.lock().unwrap().clone()
    }

    /// Get metrics for a specific tier
    pub fn get_metrics_for_tier(&self, tier: BudgetTier) -> Option<BudgetMetrics> {
        self.metrics.lock().unwrap().get(&tier).cloned()
    }

    /// Calculate cost savings percentage compared to baseline (all 32K budget)
    ///
    /// Formula: ((baseline_cost - actual_cost) / baseline_cost) × 100%
    pub fn calculate_cost_savings(&self) -> f64 {
        let metrics = self.metrics.lock().unwrap();

        // Calculate actual cost (adaptive)
        let actual_cost: f64 = metrics.values().map(|m| m.estimated_cost_usd).sum();

        // Calculate baseline cost (all requests using 32K budget)
        let baseline_cost: f64 = metrics
            .values()
            .map(|m| {
                // Baseline: same input/output tokens, but all thinking uses 32K budget
                let input_cost = m.total_input_tokens as f64 * 0.0525 / 1_000_000.0;
                let thinking_cost = 32000.0 * m.request_count as f64 * 0.35 / 1_000_000.0;
                let output_cost = m.total_output_tokens as f64 * 0.21 / 1_000_000.0;
                input_cost + thinking_cost + output_cost
            })
            .sum();

        if baseline_cost > 0.0 {
            let savings = ((baseline_cost - actual_cost) / baseline_cost) * 100.0;
            tracing::info!(
                "[BudgetOptimizer] Cost savings: {:.2}% (baseline=${:.6}, actual=${:.6})",
                savings,
                baseline_cost,
                actual_cost
            );
            savings
        } else {
            0.0
        }
    }

    /// Reset all metrics (useful for testing)
    #[cfg(test)]
    pub fn reset(&self) {
        self.metrics.lock().unwrap().clear();
    }
}

impl Default for BudgetMetricsTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === QueryComplexityClassifier Tests ===

    #[test]
    fn test_classify_simple_query_single_sentence() {
        let classifier = QueryComplexityClassifier::new();
        let result = classifier.classify("What is the capital of France?");

        assert_eq!(result.tier, BudgetTier::Simple);
        assert!(result.confidence >= 0.7);
        assert!(result.features.token_count < 50);
        assert_eq!(result.features.sentence_count, 1);
    }

    #[test]
    fn test_classify_simple_query_short_factual() {
        let classifier = QueryComplexityClassifier::new();
        let result = classifier.classify("Define REST API");

        assert_eq!(result.tier, BudgetTier::Simple);
        assert!(result.confidence >= 0.7);
    }

    #[test]
    fn test_classify_moderate_query_multi_step() {
        let classifier = QueryComplexityClassifier::new();
        let query = "Compare React and Vue for enterprise applications. \
                     What are the pros and cons of each framework?";
        let result = classifier.classify(query);

        assert_eq!(result.tier, BudgetTier::Moderate);
        assert!(result.features.has_multi_step_indicators);
    }

    #[test]
    fn test_classify_moderate_query_comparison() {
        let classifier = QueryComplexityClassifier::new();
        let result = classifier.classify("Analyze the differences between SQL and NoSQL databases");

        assert_eq!(result.tier, BudgetTier::Moderate);
        assert!(result.features.has_multi_step_indicators);
    }

    #[test]
    fn test_classify_complex_query_code_analysis() {
        let classifier = QueryComplexityClassifier::new();
        let query = "Review this code:\n```rust\nfn main() { println!(\"hello\"); }\n```";
        let result = classifier.classify(query);

        assert_eq!(result.tier, BudgetTier::Complex);
        assert!(result.features.has_code_blocks);
    }

    #[test]
    fn test_classify_complex_query_system_design() {
        let classifier = QueryComplexityClassifier::new();
        let query = "Design a distributed caching architecture for a high-traffic e-commerce platform. \
                     Consider scalability, fault tolerance, and data consistency. \
                     Explain the trade-offs between different caching strategies.";
        let result = classifier.classify(query);

        // This query has technical keywords and moderate length → likely MODERATE or COMPLEX
        assert!(result.features.has_technical_keywords);
        // Should be at least MODERATE
        assert!(result.tier == BudgetTier::Moderate || result.tier == BudgetTier::Complex);
    }

    #[test]
    fn test_classify_uncertain_defaults_to_complex() {
        let classifier = QueryComplexityClassifier::with_threshold(0.95); // Very high threshold
        // Use query that's borderline but would be MODERATE with normal threshold
        // 45 tokens, 3 sentences → right at the SIMPLE boundary
        let result = classifier.classify("Explain how authentication works in web applications. \
                                          What are the main security considerations? \
                                          Compare session-based vs token-based approaches.");

        // With high threshold (0.95), borderline queries default to COMPLEX for safety
        // This query would be MODERATE with standard threshold but has confidence < 0.95
        assert_eq!(result.tier, BudgetTier::Complex);
        assert!(result.confidence < 0.95, "Expected confidence < 0.95, got {}", result.confidence);
    }

    #[test]
    fn test_classify_edge_case_empty_query() {
        let classifier = QueryComplexityClassifier::new();
        let result = classifier.classify("");

        // Empty query should be classified as SIMPLE (0 tokens < 50)
        assert_eq!(result.tier, BudgetTier::Simple);
    }

    #[test]
    fn test_classify_edge_case_very_long_query() {
        let classifier = QueryComplexityClassifier::new();
        let long_query = "word ".repeat(250); // 250+ tokens
        let result = classifier.classify(&long_query);

        assert_eq!(result.tier, BudgetTier::Complex);
        assert!(result.features.token_count > 200);
    }

    #[test]
    fn test_classify_confidence_scoring() {
        let classifier = QueryComplexityClassifier::new();

        // Very simple query -> high confidence
        let simple_result = classifier.classify("Hi");
        assert!(simple_result.confidence >= 0.9);

        // Code block query -> high confidence for COMPLEX
        let complex_result = classifier.classify("```python\nprint('hello')\n```");
        assert!(complex_result.confidence >= 0.9);
    }

    // === BudgetRecommendationEngine Tests ===

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
        let budget = engine.recommend_budget(BudgetTier::Simple, Some(8000));

        assert_eq!(budget, 8000);
    }

    #[test]
    fn test_budget_disabled_uses_default() {
        let engine = BudgetRecommendationEngine::new(false);
        let budget = engine.recommend_budget(BudgetTier::Simple, None);

        assert_eq!(budget, 32000); // Default when disabled
    }

    #[test]
    fn test_budget_validation() {
        let engine = BudgetRecommendationEngine::new(true);

        assert!(engine.validate_budget(4000).is_ok());
        assert!(engine.validate_budget(32000).is_ok());
        assert!(engine.validate_budget(0).is_err());
        assert!(engine.validate_budget(40000).is_err());
    }

    // === BudgetMetricsTracker Tests ===

    #[test]
    fn test_metrics_tracking_per_tier() {
        let tracker = BudgetMetricsTracker::new();

        tracker.record_request(BudgetTier::Simple, 100, 1000, 200, 0.95);
        tracker.record_request(BudgetTier::Moderate, 150, 8000, 300, 0.85);

        let metrics = tracker.get_metrics();
        assert_eq!(metrics.len(), 2);

        let simple_metrics = metrics.get(&BudgetTier::Simple).unwrap();
        assert_eq!(simple_metrics.request_count, 1);
        assert_eq!(simple_metrics.total_input_tokens, 100);

        let moderate_metrics = metrics.get(&BudgetTier::Moderate).unwrap();
        assert_eq!(moderate_metrics.request_count, 1);
        assert_eq!(moderate_metrics.total_thinking_tokens, 8000);
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
        assert!(metrics.estimated_cost_usd > 0.0);
    }

    #[test]
    fn test_cost_calculation_complex() {
        let tracker = BudgetMetricsTracker::new();

        tracker.record_request(BudgetTier::Complex, 500, 30000, 1000, 0.95);

        let metrics = tracker.get_metrics_for_tier(BudgetTier::Complex).unwrap();
        assert!(metrics.estimated_cost_usd > 0.0);
    }

    #[test]
    fn test_cost_savings_calculation() {
        let tracker = BudgetMetricsTracker::new();

        // Record diverse requests
        tracker.record_request(BudgetTier::Simple, 100, 1000, 200, 0.95);
        tracker.record_request(BudgetTier::Simple, 100, 1500, 200, 0.90);
        tracker.record_request(BudgetTier::Moderate, 200, 10000, 500, 0.85);
        tracker.record_request(BudgetTier::Complex, 500, 30000, 1000, 0.95);

        let savings = tracker.calculate_cost_savings();

        // Should have positive savings (using less than 32K budget for simple/moderate)
        assert!(savings > 0.0);
        assert!(savings < 100.0); // Sanity check
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
    fn test_confidence_averaging() {
        let tracker = BudgetMetricsTracker::new();

        tracker.record_request(BudgetTier::Simple, 100, 1000, 200, 0.90);
        tracker.record_request(BudgetTier::Simple, 100, 1000, 200, 0.80);

        let metrics = tracker.get_metrics_for_tier(BudgetTier::Simple).unwrap();
        let expected_avg = (0.90 + 0.80) / 2.0;

        assert!((metrics.classification_confidence_avg - expected_avg).abs() < 0.01);
    }
}
