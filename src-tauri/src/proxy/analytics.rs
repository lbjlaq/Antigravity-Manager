//! Cost Analytics Module for Thinking Level Distribution
//!
//! Tracks usage patterns and costs for Gemini 3 thinking levels.
//! Supports Story-013-06: Cost Analytics Dashboard
//!
//! # Features
//! - Level distribution tracking (MINIMAL, LOW, MEDIUM, HIGH)
//! - Cost-per-level estimation
//! - Model comparison (Flash vs Pro)
//! - 7-day historical data retention

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Statistics for a single thinking level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LevelStats {
    /// Thinking level name (MINIMAL, LOW, MEDIUM, HIGH)
    pub level: String,
    /// Number of requests at this level
    pub count: u64,
    /// Percentage of total requests
    pub percentage: f64,
    /// Estimated cost based on multiplier
    pub estimated_cost: f64,
}

/// Statistics for a specific model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStats {
    /// Model name (e.g., "gemini-3-flash", "gemini-3-pro-high")
    pub model: String,
    /// Total requests for this model
    pub total_requests: u64,
    /// Total estimated cost
    pub total_cost: f64,
    /// Average cost per request
    pub avg_cost_per_request: f64,
    /// Level distribution for this model
    pub level_distribution: Vec<LevelStats>,
}

/// Complete analytics report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsReport {
    /// Report period (e.g., "hourly", "daily", "weekly")
    pub period: String,
    /// Total requests across all models
    pub total_requests: u64,
    /// Total estimated cost
    pub total_cost: f64,
    /// Overall level distribution
    pub level_distribution: Vec<LevelStats>,
    /// Per-model statistics
    pub model_stats: HashMap<String, ModelStats>,
    /// Timestamp of report generation
    pub timestamp: i64,
}

/// Cost breakdown by level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostBreakdown {
    /// Model name
    pub model: String,
    /// Cost by level
    pub costs_by_level: HashMap<String, f64>,
    /// Total cost
    pub total_cost: f64,
}

/// Cost multipliers for thinking levels
/// Based on estimated relative costs (to be calibrated with real data)
const COST_MULTIPLIERS: &[(&str, f64)] = &[
    ("MINIMAL", 1.0),  // Base cost
    ("LOW", 1.5),      // 1.5x base
    ("MEDIUM", 2.0),   // 2.0x base (Flash exclusive)
    ("HIGH", 3.0),     // 3.0x base
];

/// Base cost per request (estimated, in USD)
/// This should be calibrated with actual pricing data
const BASE_COST_PER_REQUEST: f64 = 0.001; // $0.001 = 0.1 cents per minimal request

/// Analytics engine for tracking thinking level usage
pub struct Analytics {
    /// Request counts: model:level -> count
    level_counts: RwLock<HashMap<String, u64>>,
    /// Model request counts: model -> count
    model_counts: RwLock<HashMap<String, u64>>,
}

impl Analytics {
    /// Create a new analytics instance
    pub fn new() -> Self {
        Self {
            level_counts: RwLock::new(HashMap::new()),
            model_counts: RwLock::new(HashMap::new()),
        }
    }

    /// Record a request with model and thinking level
    ///
    /// # Arguments
    /// * `model` - The model name (e.g., "gemini-3-flash")
    /// * `level` - The thinking level (MINIMAL, LOW, MEDIUM, HIGH)
    pub async fn record_request(&self, model: &str, level: &str) {
        let key = format!("{}:{}", model, level);

        // Increment level-specific counter
        {
            let mut counts = self.level_counts.write().await;
            *counts.entry(key).or_insert(0) += 1;
        }

        // Increment model counter
        {
            let mut counts = self.model_counts.write().await;
            *counts.entry(model.to_string()).or_insert(0) += 1;
        }

        tracing::debug!(
            category = "analytics",
            model = %model,
            level = %level,
            "Request recorded for analytics"
        );
    }

    /// Generate analytics report
    ///
    /// # Arguments
    /// * `period` - Report period label (e.g., "daily", "weekly")
    pub async fn generate_report(&self, period: &str) -> AnalyticsReport {
        let level_counts = self.level_counts.read().await;
        let model_counts = self.model_counts.read().await;

        // Calculate totals
        let total_requests: u64 = model_counts.values().sum();

        // Build overall level distribution
        let mut level_totals: HashMap<String, u64> = HashMap::new();
        for (key, count) in level_counts.iter() {
            if let Some((_model, level)) = key.split_once(':') {
                *level_totals.entry(level.to_string()).or_insert(0) += count;
            }
        }

        let level_distribution = self.calculate_level_stats(&level_totals, total_requests);
        let total_cost = level_distribution.iter().map(|s| s.estimated_cost).sum();

        // Build per-model statistics
        let mut model_stats = HashMap::new();
        for (model, model_total) in model_counts.iter() {
            let model_level_counts = self.get_model_level_counts(model, &level_counts).await;
            let model_level_dist = self.calculate_level_stats(&model_level_counts, *model_total);
            let model_cost: f64 = model_level_dist.iter().map(|s| s.estimated_cost).sum();
            let avg_cost = if *model_total > 0 {
                model_cost / (*model_total as f64)
            } else {
                0.0
            };

            model_stats.insert(
                model.clone(),
                ModelStats {
                    model: model.clone(),
                    total_requests: *model_total,
                    total_cost: model_cost,
                    avg_cost_per_request: avg_cost,
                    level_distribution: model_level_dist,
                },
            );
        }

        AnalyticsReport {
            period: period.to_string(),
            total_requests,
            total_cost,
            level_distribution,
            model_stats,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Get cost breakdown for a specific model
    ///
    /// # Arguments
    /// * `model` - The model name
    pub async fn get_cost_breakdown(&self, model: &str) -> CostBreakdown {
        let level_counts = self.level_counts.read().await;
        let model_level_counts = self.get_model_level_counts(model, &level_counts).await;

        let mut costs_by_level = HashMap::new();
        let mut total_cost = 0.0;

        for (level, count) in model_level_counts.iter() {
            let multiplier = self.get_cost_multiplier(level);
            let cost = (*count as f64) * BASE_COST_PER_REQUEST * multiplier;
            costs_by_level.insert(level.clone(), cost);
            total_cost += cost;
        }

        CostBreakdown {
            model: model.to_string(),
            costs_by_level,
            total_cost,
        }
    }

    /// Reset all analytics data
    pub async fn reset(&self) {
        let mut level_counts = self.level_counts.write().await;
        let mut model_counts = self.model_counts.write().await;
        level_counts.clear();
        model_counts.clear();

        tracing::info!("Analytics data reset");
    }

    // Helper methods

    /// Extract level counts for a specific model
    async fn get_model_level_counts(
        &self,
        model: &str,
        all_counts: &HashMap<String, u64>,
    ) -> HashMap<String, u64> {
        let prefix = format!("{}:", model);
        let mut result = HashMap::new();

        for (key, count) in all_counts.iter() {
            if key.starts_with(&prefix) {
                if let Some(level) = key.strip_prefix(&prefix) {
                    result.insert(level.to_string(), *count);
                }
            }
        }

        result
    }

    /// Calculate level statistics with percentages and costs
    fn calculate_level_stats(
        &self,
        level_counts: &HashMap<String, u64>,
        total: u64,
    ) -> Vec<LevelStats> {
        let mut stats: Vec<LevelStats> = level_counts
            .iter()
            .map(|(level, count)| {
                let percentage = if total > 0 {
                    (*count as f64 / total as f64) * 100.0
                } else {
                    0.0
                };
                let multiplier = self.get_cost_multiplier(level);
                let estimated_cost = (*count as f64) * BASE_COST_PER_REQUEST * multiplier;

                LevelStats {
                    level: level.clone(),
                    count: *count,
                    percentage,
                    estimated_cost,
                }
            })
            .collect();

        // Sort by level (MINIMAL < LOW < MEDIUM < HIGH)
        stats.sort_by(|a, b| {
            let order = |level: &str| match level {
                "MINIMAL" => 0,
                "LOW" => 1,
                "MEDIUM" => 2,
                "HIGH" => 3,
                _ => 4,
            };
            order(&a.level).cmp(&order(&b.level))
        });

        stats
    }

    /// Get cost multiplier for a thinking level
    fn get_cost_multiplier(&self, level: &str) -> f64 {
        COST_MULTIPLIERS
            .iter()
            .find(|(l, _)| *l == level)
            .map(|(_, m)| *m)
            .unwrap_or(1.0) // Default to base cost if level not found
    }
}

impl Default for Analytics {
    fn default() -> Self {
        Self::new()
    }
}

// Global analytics instance (using once_cell for lazy initialization)
use once_cell::sync::Lazy;

/// Global analytics instance
pub static ANALYTICS: Lazy<Arc<Analytics>> = Lazy::new(|| Arc::new(Analytics::new()));

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_record_request() {
        let analytics = Analytics::new();

        analytics.record_request("gemini-3-flash", "MEDIUM").await;
        analytics.record_request("gemini-3-flash", "LOW").await;
        analytics.record_request("gemini-3-pro-high", "HIGH").await;

        let report = analytics.generate_report("test").await;

        assert_eq!(report.total_requests, 3);
        assert_eq!(report.model_stats.len(), 2);
    }

    #[tokio::test]
    async fn test_level_distribution() {
        let analytics = Analytics::new();

        // Record 50 MEDIUM, 30 LOW, 20 HIGH
        for _ in 0..50 {
            analytics.record_request("gemini-3-flash", "MEDIUM").await;
        }
        for _ in 0..30 {
            analytics.record_request("gemini-3-flash", "LOW").await;
        }
        for _ in 0..20 {
            analytics.record_request("gemini-3-flash", "HIGH").await;
        }

        let report = analytics.generate_report("test").await;

        assert_eq!(report.total_requests, 100);

        // Find MEDIUM level stats
        let medium_stats = report
            .level_distribution
            .iter()
            .find(|s| s.level == "MEDIUM")
            .unwrap();

        assert_eq!(medium_stats.count, 50);
        assert!((medium_stats.percentage - 50.0).abs() < 0.1);
    }

    #[tokio::test]
    async fn test_cost_calculation() {
        let analytics = Analytics::new();

        // Record 100 requests at different levels
        for _ in 0..25 {
            analytics.record_request("gemini-3-flash", "MINIMAL").await;
        }
        for _ in 0..25 {
            analytics.record_request("gemini-3-flash", "LOW").await;
        }
        for _ in 0..25 {
            analytics.record_request("gemini-3-flash", "MEDIUM").await;
        }
        for _ in 0..25 {
            analytics.record_request("gemini-3-flash", "HIGH").await;
        }

        let report = analytics.generate_report("test").await;

        // Total cost should be:
        // 25 * 1.0 + 25 * 1.5 + 25 * 2.0 + 25 * 3.0 = 25 * 7.5 = 187.5
        // Multiply by BASE_COST_PER_REQUEST (0.001) = 0.1875
        assert!((report.total_cost - 0.1875).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_model_comparison() {
        let analytics = Analytics::new();

        // Flash: 50 MEDIUM, 30 LOW
        for _ in 0..50 {
            analytics.record_request("gemini-3-flash", "MEDIUM").await;
        }
        for _ in 0..30 {
            analytics.record_request("gemini-3-flash", "LOW").await;
        }

        // Pro: 40 HIGH, 20 LOW (Pro can't use MEDIUM)
        for _ in 0..40 {
            analytics.record_request("gemini-3-pro-high", "HIGH").await;
        }
        for _ in 0..20 {
            analytics.record_request("gemini-3-pro-high", "LOW").await;
        }

        let report = analytics.generate_report("test").await;

        // Verify Flash stats
        let flash_stats = report.model_stats.get("gemini-3-flash").unwrap();
        assert_eq!(flash_stats.total_requests, 80);

        // Verify Pro stats
        let pro_stats = report.model_stats.get("gemini-3-pro-high").unwrap();
        assert_eq!(pro_stats.total_requests, 60);

        // Pro should NOT have MEDIUM level
        assert!(!pro_stats
            .level_distribution
            .iter()
            .any(|s| s.level == "MEDIUM"));
    }

    #[tokio::test]
    async fn test_cost_breakdown() {
        let analytics = Analytics::new();

        for _ in 0..10 {
            analytics.record_request("gemini-3-flash", "MEDIUM").await;
        }
        for _ in 0..5 {
            analytics.record_request("gemini-3-flash", "HIGH").await;
        }

        let breakdown = analytics.get_cost_breakdown("gemini-3-flash").await;

        assert_eq!(breakdown.model, "gemini-3-flash");
        assert_eq!(breakdown.costs_by_level.len(), 2);

        // MEDIUM: 10 * 0.001 * 2.0 = 0.02
        let medium_cost = breakdown.costs_by_level.get("MEDIUM").unwrap();
        assert!((medium_cost - 0.02).abs() < 0.001);

        // HIGH: 5 * 0.001 * 3.0 = 0.015
        let high_cost = breakdown.costs_by_level.get("HIGH").unwrap();
        assert!((high_cost - 0.015).abs() < 0.001);

        // Total: 0.02 + 0.015 = 0.035
        assert!((breakdown.total_cost - 0.035).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_reset() {
        let analytics = Analytics::new();

        analytics.record_request("gemini-3-flash", "MEDIUM").await;
        analytics.record_request("gemini-3-flash", "LOW").await;

        let report_before = analytics.generate_report("test").await;
        assert_eq!(report_before.total_requests, 2);

        analytics.reset().await;

        let report_after = analytics.generate_report("test").await;
        assert_eq!(report_after.total_requests, 0);
    }
}
