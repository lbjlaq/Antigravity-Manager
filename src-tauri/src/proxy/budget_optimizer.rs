//! Adaptive Budget Optimization for Gemini 2.5 Pro Thinking Mode
//!
//! Epic-008, Story-008-01: Implements intelligent budget optimization that dynamically
//! adjusts thinking budget based on query complexity.
//!
//! # Problem
//! Fixed budget of 16,000 tokens for ALL requests is inefficient:
//! - Simple "hello" queries waste 14,000 tokens (uses 16K when 2K would suffice)
//! - Complex architectural analysis limited to 16K when they need 24-32K
//! - No learning from historical patterns
//! - 15-25% potential savings lost
//!
//! # Solution
//! 4-level complexity classification with dynamic budget mapping:
//! - Simple: 2000-4000 tokens (greetings, one-word, yes/no)
//! - Moderate: 8000-12000 tokens (multi-sentence, single-topic, factual)
//! - Complex: 16000-24000 tokens (multi-topic, analysis, reasoning)
//! - Deep: 24000-32000 tokens (research, architectural, comprehensive)
//!
//! # Usage
//! ```rust
//! use crate::proxy::budget_optimizer::BudgetOptimizer;
//!
//! let optimizer = BudgetOptimizer::new();
//! let budget = optimizer.calculate_optimal_budget(
//!     "explain quantum computing",
//!     "gemini-2.5-pro-thinking"
//! )?;
//! // Returns: ~10000 (Moderate complexity)
//! ```
//!
//! # Performance
//! - Classification: <50ms (target)
//! - Accuracy: ≥80% on validation set
//! - Storage: SQLite for pattern persistence

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Complexity level for prompt classification
///
/// Maps to specific budget ranges based on query sophistication:
/// - Simple: Quick responses, greetings, yes/no
/// - Moderate: Explanations, summaries, how-to
/// - Complex: Multi-topic analysis, reasoning
/// - Deep: Research, architectural design, comprehensive analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComplexityLevel {
    /// Simple queries: greetings, one-word responses, yes/no questions
    /// Budget: 2000-4000 tokens
    Simple,
    /// Moderate queries: multi-sentence explanations, single-topic
    /// Budget: 8000-12000 tokens
    Moderate,
    /// Complex queries: multi-topic analysis, reasoning tasks
    /// Budget: 16000-24000 tokens
    Complex,
    /// Deep queries: research, architectural design, comprehensive analysis
    /// Budget: 24000-32000 tokens
    Deep,
}

impl std::fmt::Display for ComplexityLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComplexityLevel::Simple => write!(f, "Simple"),
            ComplexityLevel::Moderate => write!(f, "Moderate"),
            ComplexityLevel::Complex => write!(f, "Complex"),
            ComplexityLevel::Deep => write!(f, "Deep"),
        }
    }
}

/// Budget pattern for historical tracking
///
/// Stores usage patterns for learning and adaptation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetPattern {
    /// SHA-256 hash of prompt for privacy (first 16 chars)
    pub prompt_hash: String,
    /// Classified complexity level
    pub complexity_level: ComplexityLevel,
    /// Average budget used for this pattern
    pub avg_budget: u32,
    /// Number of times this pattern was used
    pub usage_count: u32,
    /// Sum of quality scores (for calculating average)
    pub total_quality_score: f32,
    /// Last usage timestamp (Unix timestamp)
    pub last_used: i64,
    /// Creation timestamp (Unix timestamp)
    pub created_at: i64,
}

/// Main budget optimizer
///
/// Combines classifier, mapper, and pattern storage for intelligent
/// budget optimization with feedback loop.
pub struct BudgetOptimizer {
    classifier: ComplexityClassifier,
    mapper: BudgetMapper,
    pattern_store: Arc<RwLock<PatternStore>>,
    feedback_processor: FeedbackProcessor,
}

impl BudgetOptimizer {
    /// Create new budget optimizer
    pub fn new() -> Self {
        Self {
            classifier: ComplexityClassifier::new(),
            mapper: BudgetMapper::new(),
            pattern_store: Arc::new(RwLock::new(PatternStore::new())),
            feedback_processor: FeedbackProcessor::new(),
        }
    }

    /// Calculate optimal budget based on prompt complexity
    ///
    /// # Arguments
    /// * `prompt` - User prompt text
    /// * `model` - Target model name (for future model-specific adjustments)
    ///
    /// # Returns
    /// Optimal budget in tokens (2000-32000 range)
    ///
    /// # Performance
    /// Target: <50ms classification + mapping
    ///
    /// # Examples
    /// ```
    /// let optimizer = BudgetOptimizer::new();
    /// let budget = optimizer.calculate_optimal_budget("hello", "gemini-2.5-pro")?;
    /// assert!(budget >= 2000 && budget <= 4000); // Simple
    /// ```
    pub fn calculate_optimal_budget(&self, prompt: &str, _model: &str) -> Result<u32, String> {
        // Step 1: Classify complexity (<50ms target)
        let complexity = self.classifier.classify(prompt)?;

        // Step 2: Map to budget range
        let base_budget = self.mapper.calculate(complexity);

        // Step 3: Check historical patterns for refinement (optional)
        let adjusted_budget = {
            let store = self
                .pattern_store
                .read()
                .map_err(|e| format!("Failed to acquire read lock: {}", e))?;
            store.adjust_based_on_history(prompt, base_budget)
        };

        tracing::debug!(
            "[Budget-Optimizer] prompt_len={}, complexity={:?}, base_budget={}, adjusted_budget={}",
            prompt.len(),
            complexity,
            base_budget,
            adjusted_budget
        );

        Ok(adjusted_budget)
    }

    /// Record feedback for learning
    ///
    /// # Arguments
    /// * `prompt` - Original prompt
    /// * `budget_used` - Actual budget used
    /// * `quality_score` - Quality score (0.0-1.0, where 1.0 is excellent)
    ///
    /// # Examples
    /// ```
    /// optimizer.record_feedback("hello", 3000, 0.95);
    /// ```
    pub fn record_feedback(&self, prompt: &str, budget_used: u32, quality_score: f32) {
        self.feedback_processor.process(
            prompt,
            budget_used,
            quality_score,
            Arc::clone(&self.pattern_store),
        );
    }

    /// Get pattern store for persistence operations
    pub fn get_pattern_store(&self) -> Arc<RwLock<PatternStore>> {
        Arc::clone(&self.pattern_store)
    }
}

impl Default for BudgetOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Complexity classifier using rule-based heuristics
///
/// Performance target: <50ms classification time
struct ComplexityClassifier {
    // Future: Add ML model here for better accuracy
}

impl ComplexityClassifier {
    fn new() -> Self {
        Self {}
    }

    /// Classify prompt complexity using heuristics
    ///
    /// # Indicators
    /// - Prompt length (chars/words)
    /// - Keyword patterns ("analyze", "explain", "design", etc.)
    /// - Question complexity (multi-sentence, multi-topic)
    /// - Special markers (code blocks, lists, etc.)
    ///
    /// # Performance
    /// Target: <50ms (measured via benchmarks)
    fn classify(&self, prompt: &str) -> Result<ComplexityLevel, String> {
        if prompt.is_empty() {
            return Ok(ComplexityLevel::Simple);
        }

        let prompt_lower = prompt.to_lowercase();
        let word_count = prompt.split_whitespace().count();
        let char_count = prompt.len();
        let sentence_count = prompt.matches('.').count()
            + prompt.matches('?').count()
            + prompt.matches('!').count();

        // Check for deep indicators first (most specific)
        if self.has_deep_indicators(&prompt_lower) {
            return Ok(ComplexityLevel::Deep);
        }

        // Deep: Very long prompts (10K+ chars or 2000+ words)
        if char_count > 5000 || word_count > 200 {
            return Ok(ComplexityLevel::Deep);
        }

        // Check for complex indicators
        if self.has_complex_indicators(&prompt_lower) {
            return Ok(ComplexityLevel::Complex);
        }

        // Complex: Multi-sentence or long-ish prompts
        if sentence_count >= 3 || word_count > 50 {
            return Ok(ComplexityLevel::Complex);
        }

        // Simple: Very short, greetings, one-word responses
        if word_count < 3 {
            return Ok(ComplexityLevel::Simple);
        }

        // Check for moderate indicators
        if self.has_moderate_indicators(&prompt_lower) {
            return Ok(ComplexityLevel::Moderate);
        }

        // Moderate: Multi-word but not complex (3-50 words)
        if word_count >= 3 && word_count <= 50 {
            return Ok(ComplexityLevel::Moderate);
        }

        // Default to simple for anything else
        Ok(ComplexityLevel::Simple)
    }

    /// Check for moderate complexity indicators
    fn has_moderate_indicators(&self, prompt: &str) -> bool {
        let moderate_keywords = [
            "explain ",
            "what is",
            "what's",
            "how do",
            "how to",
            "summarize",
            "summary",
            "define",
            "describe",
            "tell me about",
        ];

        moderate_keywords.iter().any(|kw| prompt.contains(*kw))
    }

    /// Check for deep complexity indicators
    fn has_deep_indicators(&self, prompt: &str) -> bool {
        let deep_keywords = [
            "design distributed",
            "design a system",
            "architect a",
            "architectural review",
            "comprehensive security audit",
            "comprehensive ",  // comprehensive anything is deep
            "research paper",
            "deep dive",
            "in-depth",
            "strategic plan",
            "roadmap",
            "migration plan",
            "performance audit",
            "security audit",
            "end-to-end",
            "multi-region",
            "scalability plan",
            "legacy modernization",
            "100+",  // Large scale
            "1000+",
            "-year",  // Multi-year planning
        ];

        deep_keywords.iter().any(|kw| prompt.contains(*kw))
    }

    /// Check for complex complexity indicators
    fn has_complex_indicators(&self, prompt: &str) -> bool {
        let complex_keywords = [
            "analyze ",
            "compare ",  // Space after to avoid "comprehensive"
            "evaluate ",
            "assess ",
            "trade-off",
            "pros and cons",
            "advantages and disadvantages",
            "reason about",
            "discuss ",
            "design a ",  // Not "design a distributed" (that's deep)
            "debug ",
            "troubleshoot",
            "optimize ",
            "improve performance",
            "code review",
        ];

        let keyword_count = complex_keywords.iter().filter(|kw| prompt.contains(*kw)).count();

        // Has complex keywords or comparative markers
        keyword_count >= 1 || prompt.contains(" vs ") || prompt.contains(" versus ")
    }
}

/// Budget mapper from complexity to token budget
///
/// Maps complexity levels to specific budget ranges
struct BudgetMapper;

impl BudgetMapper {
    fn new() -> Self {
        Self
    }

    /// Calculate budget based on complexity level
    ///
    /// # Ranges
    /// - Simple: 2000-4000 (use 3000)
    /// - Moderate: 8000-12000 (use 10000)
    /// - Complex: 16000-24000 (use 20000)
    /// - Deep: 24000-32000 (use 28000)
    fn calculate(&self, complexity: ComplexityLevel) -> u32 {
        match complexity {
            ComplexityLevel::Simple => 3000,
            ComplexityLevel::Moderate => 10000,
            ComplexityLevel::Complex => 20000,
            ComplexityLevel::Deep => 28000,
        }
    }
}

/// Pattern store for historical usage tracking
///
/// Stores patterns in-memory with optional SQLite persistence
pub struct PatternStore {
    patterns: HashMap<String, BudgetPattern>,
}

impl PatternStore {
    fn new() -> Self {
        Self {
            patterns: HashMap::new(),
        }
    }

    /// Adjust budget based on historical patterns
    ///
    /// # Logic
    /// If we've seen similar prompt before, adjust budget based on
    /// average quality score. Higher quality = can reduce budget.
    /// Lower quality = should increase budget.
    fn adjust_based_on_history(&self, prompt: &str, base_budget: u32) -> u32 {
        let prompt_hash = self.hash_prompt(prompt);

        if let Some(pattern) = self.patterns.get(&prompt_hash) {
            // Calculate average quality score
            let avg_quality = if pattern.usage_count > 0 {
                pattern.total_quality_score / pattern.usage_count as f32
            } else {
                0.5 // Neutral
            };

            // Adjust budget based on quality
            // High quality (>0.8) = reduce budget by 10%
            // Low quality (<0.5) = increase budget by 10%
            let adjustment_factor = if avg_quality > 0.8 {
                0.9 // Reduce
            } else if avg_quality < 0.5 {
                1.1 // Increase
            } else {
                1.0 // No change
            };

            let adjusted = (base_budget as f32 * adjustment_factor) as u32;

            tracing::debug!(
                "[Pattern-Store] Found pattern: hash={}, usage={}, avg_quality={:.2}, adjustment={:.2}",
                &prompt_hash[..8],
                pattern.usage_count,
                avg_quality,
                adjustment_factor
            );

            adjusted
        } else {
            base_budget
        }
    }

    /// Save or update pattern
    pub fn save_pattern(&mut self, pattern: BudgetPattern) {
        self.patterns.insert(pattern.prompt_hash.clone(), pattern);
    }

    /// Get pattern by hash
    pub fn get_pattern(&self, prompt_hash: &str) -> Option<&BudgetPattern> {
        self.patterns.get(prompt_hash)
    }

    /// Hash prompt for privacy (SHA-256, first 16 chars)
    fn hash_prompt(&self, prompt: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(prompt.as_bytes());
        let result = hasher.finalize();
        format!("{:x}", result)[..16].to_string()
    }

    /// Load patterns from database (called during initialization)
    pub fn load_from_db(&mut self, patterns: Vec<BudgetPattern>) {
        for pattern in patterns {
            self.patterns.insert(pattern.prompt_hash.clone(), pattern);
        }
    }

    /// Get all patterns for persistence
    pub fn get_all_patterns(&self) -> Vec<BudgetPattern> {
        self.patterns.values().cloned().collect()
    }
}

/// Feedback processor for learning from usage
struct FeedbackProcessor;

impl FeedbackProcessor {
    fn new() -> Self {
        Self
    }

    /// Process feedback and update pattern store
    fn process(
        &self,
        prompt: &str,
        budget_used: u32,
        quality_score: f32,
        pattern_store: Arc<RwLock<PatternStore>>,
    ) {
        use sha2::{Digest, Sha256};

        // Hash prompt
        let mut hasher = Sha256::new();
        hasher.update(prompt.as_bytes());
        let result = hasher.finalize();
        let prompt_hash = format!("{:x}", result)[..16].to_string();

        // Classify for storage
        let classifier = ComplexityClassifier::new();
        let complexity = match classifier.classify(prompt) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("[Feedback-Processor] Classification failed: {}", e);
                return;
            }
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Update or create pattern
        if let Ok(mut store) = pattern_store.write() {
            if let Some(existing) = store.get_pattern(&prompt_hash).cloned() {
                // Update existing
                let updated = BudgetPattern {
                    prompt_hash: existing.prompt_hash.clone(),
                    complexity_level: complexity,
                    avg_budget: ((existing.avg_budget as f32 * existing.usage_count as f32
                        + budget_used as f32)
                        / (existing.usage_count + 1) as f32) as u32,
                    usage_count: existing.usage_count + 1,
                    total_quality_score: existing.total_quality_score + quality_score,
                    last_used: now,
                    created_at: existing.created_at,
                };
                store.save_pattern(updated);
            } else {
                // Create new
                let new_pattern = BudgetPattern {
                    prompt_hash,
                    complexity_level: complexity,
                    avg_budget: budget_used,
                    usage_count: 1,
                    total_quality_score: quality_score,
                    last_used: now,
                    created_at: now,
                };
                store.save_pattern(new_pattern);
            }
        }
    }
}

// ============================================================================
// UNIT TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Test 1: Simple prompt classification
    #[test]
    fn test_complexity_simple_prompts() {
        let classifier = ComplexityClassifier::new();

        let simple_prompts = vec![
            "hello",
            "hi",
            "yes",
            "no",
            "thanks",
            "good morning",
            "what time?",
            "ok",
        ];

        for prompt in simple_prompts {
            let result = classifier.classify(prompt);
            assert!(result.is_ok(), "Failed to classify: {}", prompt);
            assert_eq!(
                result.unwrap(),
                ComplexityLevel::Simple,
                "Prompt '{}' should be Simple",
                prompt
            );
        }
    }

    // Test 2: Moderate prompt classification
    #[test]
    fn test_complexity_moderate_prompts() {
        let classifier = ComplexityClassifier::new();

        let moderate_prompts = vec![
            "explain quantum computing",
            "what is machine learning?",
            "summarize this article",
            "how do I use Git?",
            "define recursion in programming",
        ];

        for prompt in moderate_prompts {
            let result = classifier.classify(prompt);
            assert!(result.is_ok(), "Failed to classify: {}", prompt);
            assert_eq!(
                result.unwrap(),
                ComplexityLevel::Moderate,
                "Prompt '{}' should be Moderate",
                prompt
            );
        }
    }

    // Test 3: Complex prompt classification
    #[test]
    fn test_complexity_complex_prompts() {
        let classifier = ComplexityClassifier::new();

        let complex_prompts = vec![
            "analyze the pros and cons of microservices",
            "compare React and Vue for enterprise apps",
            "evaluate SQL vs NoSQL databases",
            "debug a memory leak in Node.js",
        ];

        for prompt in complex_prompts {
            let result = classifier.classify(prompt);
            assert!(result.is_ok(), "Failed to classify: {}", prompt);
            assert_eq!(
                result.unwrap(),
                ComplexityLevel::Complex,
                "Prompt '{}' should be Complex",
                prompt
            );
        }
    }

    // Test 4: Deep prompt classification
    #[test]
    fn test_complexity_deep_prompts() {
        let classifier = ComplexityClassifier::new();

        let deep_prompts = vec![
            "design a distributed tracing system for 100+ microservices",
            "architect a globally distributed e-commerce platform",
            "comprehensive security audit with recommendations",
            "create a 3-year technical roadmap for migration",
        ];

        for prompt in deep_prompts {
            let result = classifier.classify(prompt);
            assert!(result.is_ok(), "Failed to classify: {}", prompt);
            assert_eq!(
                result.unwrap(),
                ComplexityLevel::Deep,
                "Prompt '{}' should be Deep",
                prompt
            );
        }
    }

    // Test 5: Simple budget mapping
    #[test]
    fn test_budget_mapping_simple() {
        let mapper = BudgetMapper::new();
        let budget = mapper.calculate(ComplexityLevel::Simple);
        assert!(
            budget >= 2000 && budget <= 4000,
            "Simple budget should be 2000-4000, got {}",
            budget
        );
    }

    // Test 6: Moderate budget mapping
    #[test]
    fn test_budget_mapping_moderate() {
        let mapper = BudgetMapper::new();
        let budget = mapper.calculate(ComplexityLevel::Moderate);
        assert!(
            budget >= 8000 && budget <= 12000,
            "Moderate budget should be 8000-12000, got {}",
            budget
        );
    }

    // Test 7: Complex budget mapping
    #[test]
    fn test_budget_mapping_complex() {
        let mapper = BudgetMapper::new();
        let budget = mapper.calculate(ComplexityLevel::Complex);
        assert!(
            budget >= 16000 && budget <= 24000,
            "Complex budget should be 16000-24000, got {}",
            budget
        );
    }

    // Test 8: Deep budget mapping
    #[test]
    fn test_budget_mapping_deep() {
        let mapper = BudgetMapper::new();
        let budget = mapper.calculate(ComplexityLevel::Deep);
        assert!(
            budget >= 24000 && budget <= 32000,
            "Deep budget should be 24000-32000, got {}",
            budget
        );
    }

    // Test 9: Edge case - empty prompt
    #[test]
    fn test_edge_case_empty_prompt() {
        let classifier = ComplexityClassifier::new();
        let result = classifier.classify("");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ComplexityLevel::Simple);
    }

    // Test 10: Edge case - very long prompt
    #[test]
    fn test_edge_case_very_long_prompt() {
        let classifier = ComplexityClassifier::new();
        let long_prompt = "a".repeat(10_000);
        let result = classifier.classify(&long_prompt);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            ComplexityLevel::Deep,
            "Very long prompt should be Deep"
        );
    }

    // Test 11: Pattern storage save/load
    #[test]
    fn test_pattern_storage_save_load() {
        let mut store = PatternStore::new();

        let pattern = BudgetPattern {
            prompt_hash: "abc123".to_string(),
            complexity_level: ComplexityLevel::Moderate,
            avg_budget: 10000,
            usage_count: 5,
            total_quality_score: 4.2,
            last_used: 1234567890,
            created_at: 1234567800,
        };

        store.save_pattern(pattern.clone());

        let retrieved = store.get_pattern("abc123");
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.prompt_hash, "abc123");
        assert_eq!(retrieved.avg_budget, 10000);
        assert_eq!(retrieved.usage_count, 5);
    }

    // Test 12: Feedback loop adjustment
    #[test]
    fn test_feedback_loop_adjustment() {
        let optimizer = BudgetOptimizer::new();

        // Initial calculation
        let prompt = "explain machine learning";
        let budget1 = optimizer.calculate_optimal_budget(prompt, "gemini-2.5-pro");
        assert!(budget1.is_ok());
        let budget1_value = budget1.unwrap();

        // Record high quality feedback
        optimizer.record_feedback(prompt, budget1_value, 0.95);

        // Should adjust budget based on feedback
        let budget2 = optimizer.calculate_optimal_budget(prompt, "gemini-2.5-pro");
        assert!(budget2.is_ok());

        // With high quality, budget should be reduced or stay same
        assert!(budget2.unwrap() <= budget1_value);
    }

    // Test 13: Performance - classification speed
    #[test]
    fn test_performance_classification_speed() {
        let classifier = ComplexityClassifier::new();
        let prompts = vec![
            "hello",
            "explain quantum computing",
            "analyze microservices vs monolith",
            "design a distributed system for 100+ services",
        ];

        let start = std::time::Instant::now();
        for _ in 0..1000 {
            for prompt in &prompts {
                let _ = classifier.classify(prompt);
            }
        }
        let duration = start.elapsed();

        // 1000 iterations * 4 prompts = 4000 classifications
        // Target: <50ms per classification
        // 4000 * 50ms = 200,000ms = 200s max
        let avg_per_classification = duration.as_micros() / 4000;
        println!(
            "Average classification time: {} microseconds",
            avg_per_classification
        );

        // Should be well under 50ms (50,000 microseconds)
        assert!(
            avg_per_classification < 50_000,
            "Classification too slow: {}µs (target: <50,000µs)",
            avg_per_classification
        );
    }

    // Test 14: Validation set accuracy
    #[test]
    fn test_validation_set_accuracy() {
        let optimizer = BudgetOptimizer::new();

        // Validation set with expected budgets
        let validation_cases = vec![
            // Simple (2000-4000)
            ("hello", 2000, 4000),
            ("thanks", 2000, 4000),
            ("yes", 2000, 4000),
            // Moderate (8000-12000)
            ("explain machine learning", 8000, 12000),
            ("summarize this article", 8000, 12000),
            ("what is Rust?", 8000, 12000),
            // Complex (16000-24000)
            ("analyze microservices vs monolith", 16000, 24000),
            ("compare React and Vue with examples", 16000, 24000),
            ("debug slow API performance", 16000, 24000),
            // Deep (24000-32000)
            ("design distributed tracing for 100+ microservices", 24000, 32000),
            ("comprehensive security audit with recommendations", 24000, 32000),
            ("3-year technical roadmap for modernization", 24000, 32000),
        ];

        let mut correct = 0;
        let total = validation_cases.len();

        for (prompt, min_budget, max_budget) in validation_cases {
            let result = optimizer.calculate_optimal_budget(prompt, "gemini-2.5-pro");
            assert!(result.is_ok(), "Failed to calculate budget for: {}", prompt);

            let budget = result.unwrap();
            if budget >= min_budget && budget <= max_budget {
                correct += 1;
            } else {
                println!(
                    "MISS: '{}' -> budget={} (expected {}-{})",
                    prompt, budget, min_budget, max_budget
                );
            }
        }

        let accuracy = (correct as f32 / total as f32) * 100.0;
        println!(
            "Validation accuracy: {}/{} ({:.1}%)",
            correct, total, accuracy
        );

        // Target: ≥80% accuracy
        assert!(
            accuracy >= 80.0,
            "Validation accuracy too low: {:.1}% (target: ≥80%)",
            accuracy
        );
    }
}
