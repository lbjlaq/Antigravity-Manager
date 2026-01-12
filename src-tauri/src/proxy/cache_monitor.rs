//! Cache Monitor - Signature Cache Observability System
//!
//! Story-008-02: Signature Cache Monitoring
//!
//! Provides comprehensive metrics for signature cache performance:
//! - Cache hit rate tracking (AC1)
//! - Signature reuse analysis (AC2)
//! - Cost attribution tracking (AC3)
//! - Performance benchmarking (AC4)
//! - Dashboard integration support (AC5)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Cost savings calculation for cached requests
/// AC3: Cost attribution tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostSavings {
    /// Total cost saved by cache hits (estimated)
    pub total_saved: f64,

    /// Cost saved per account (email -> amount)
    pub per_account: HashMap<String, f64>,

    /// Cost saved per user/session (if applicable)
    pub per_user: HashMap<String, f64>,

    /// Total savings percentage vs. uncached baseline
    pub savings_percentage: f64,

    /// Hourly savings breakdown (last 24 hours)
    pub hourly_savings: Vec<(i64, f64)>,

    /// Daily savings breakdown (last 7 days)
    pub daily_savings: Vec<(i64, f64)>,
}

impl Default for CostSavings {
    fn default() -> Self {
        Self {
            total_saved: 0.0,
            per_account: HashMap::new(),
            per_user: HashMap::new(),
            savings_percentage: 0.0,
            hourly_savings: Vec::new(),
            daily_savings: Vec::new(),
        }
    }
}

/// Statistics for a specific signature
/// AC2: Signature reuse analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureStats {
    /// The signature string (truncated for display)
    pub signature: String,

    /// Number of times this signature has been reused
    pub reuse_count: u64,

    /// Last time this signature was used
    pub last_used: DateTime<Utc>,

    /// First time this signature was cached
    pub first_cached: DateTime<Utc>,

    /// Estimated cost saved by reusing this signature
    pub cost_saved: f64,

    /// Average lookup time for this signature (ms)
    pub avg_lookup_time: f64,

    /// High-value flag (≥3 reuses)
    pub high_value: bool,
}

/// Performance metrics for cache operations
/// AC4: Performance benchmarking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// Cache lookup time at p50 (median) in milliseconds
    pub lookup_p50: f64,

    /// Cache lookup time at p95 in milliseconds (target: <10ms)
    pub lookup_p95: f64,

    /// Cache lookup time at p99 in milliseconds
    pub lookup_p99: f64,

    /// Cache write time at p95 in milliseconds (target: <5ms)
    pub write_p95: f64,

    /// Current memory usage estimate (bytes)
    pub memory_usage: u64,

    /// Total number of cache operations performed
    pub total_operations: u64,

    /// Performance degradation alert (>20% from baseline)
    pub degradation_alert: bool,
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            lookup_p50: 0.0,
            lookup_p95: 0.0,
            lookup_p99: 0.0,
            write_p95: 0.0,
            memory_usage: 0,
            total_operations: 0,
            degradation_alert: false,
        }
    }
}

/// Comprehensive cache metrics
/// AC1-AC4: All metrics aggregated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMetrics {
    /// Total cache hit count
    pub hit_count: u64,

    /// Total cache miss count
    pub miss_count: u64,

    /// Cache hit rate (hits / (hits + misses))
    /// Target: ≥30%
    pub hit_rate: f32,

    /// Top N most reused signatures
    pub top_signatures: Vec<SignatureStats>,

    /// Cost savings analysis
    pub cost_savings: CostSavings,

    /// Performance metrics
    pub performance: PerformanceMetrics,

    /// Last updated timestamp
    pub updated_at: DateTime<Utc>,
}

impl Default for CacheMetrics {
    fn default() -> Self {
        Self {
            hit_count: 0,
            miss_count: 0,
            hit_rate: 0.0,
            top_signatures: Vec::new(),
            cost_savings: CostSavings::default(),
            performance: PerformanceMetrics::default(),
            updated_at: Utc::now(),
        }
    }
}

/// Internal signature tracking data
#[derive(Debug, Clone)]
struct SignatureTracker {
    signature: String,
    reuse_count: u64,
    first_cached: DateTime<Utc>,
    last_used: DateTime<Utc>,
    cost_saved: f64,
    lookup_times: Vec<f64>,
}

impl SignatureTracker {
    fn new(signature: String) -> Self {
        let now = Utc::now();
        Self {
            signature,
            reuse_count: 0,
            first_cached: now,
            last_used: now,
            cost_saved: 0.0,
            lookup_times: Vec::new(),
        }
    }

    fn record_reuse(&mut self, lookup_time: f64, estimated_cost: f64) {
        self.reuse_count += 1;
        self.last_used = Utc::now();
        self.cost_saved += estimated_cost;

        // Keep only last 100 lookup times for memory efficiency
        self.lookup_times.push(lookup_time);
        if self.lookup_times.len() > 100 {
            self.lookup_times.remove(0);
        }
    }

    fn to_stats(&self) -> SignatureStats {
        let avg_lookup_time = if self.lookup_times.is_empty() {
            0.0
        } else {
            self.lookup_times.iter().sum::<f64>() / self.lookup_times.len() as f64
        };

        SignatureStats {
            signature: truncate_signature(&self.signature, 100),
            reuse_count: self.reuse_count,
            last_used: self.last_used,
            first_cached: self.first_cached,
            cost_saved: self.cost_saved,
            avg_lookup_time,
            high_value: self.reuse_count >= 3,
        }
    }
}

/// Truncate signature for display
fn truncate_signature(sig: &str, max_len: usize) -> String {
    if sig.len() <= max_len {
        sig.to_string()
    } else {
        format!("{}...", &sig[..max_len])
    }
}

/// Cache Monitor - Main observability engine
///
/// Tracks cache performance metrics and provides analytics for:
/// - Hit rate optimization
/// - Signature reuse patterns
/// - Cost attribution
/// - Performance benchmarking
pub struct CacheMonitor {
    /// Hit/miss counters
    metrics: Arc<RwLock<CacheMetrics>>,

    /// Signature-level tracking
    signatures: Arc<RwLock<HashMap<String, SignatureTracker>>>,

    /// Lookup time tracking (for percentile calculation)
    lookup_times: Arc<RwLock<VecDeque<f64>>>,

    /// Write time tracking
    write_times: Arc<RwLock<VecDeque<f64>>>,

    /// Cost tracking by account
    account_costs: Arc<RwLock<HashMap<String, f64>>>,

    /// Hourly savings tracking (timestamp -> savings)
    hourly_savings: Arc<RwLock<VecDeque<(i64, f64)>>>,

    /// Daily savings tracking
    daily_savings: Arc<RwLock<VecDeque<(i64, f64)>>>,

    /// Baseline performance (for degradation detection)
    baseline_p95: Arc<RwLock<f64>>,
}

impl CacheMonitor {
    /// Create a new cache monitor
    /// Story-012-03: Restore metrics from database on startup
    pub fn new() -> Self {
        let monitor = Self {
            metrics: Arc::new(RwLock::new(CacheMetrics::default())),
            signatures: Arc::new(RwLock::new(HashMap::new())),
            lookup_times: Arc::new(RwLock::new(VecDeque::new())),
            write_times: Arc::new(RwLock::new(VecDeque::new())),
            account_costs: Arc::new(RwLock::new(HashMap::new())),
            hourly_savings: Arc::new(RwLock::new(VecDeque::new())),
            daily_savings: Arc::new(RwLock::new(VecDeque::new())),
            baseline_p95: Arc::new(RwLock::new(0.0)),
        };

        // Story-012-03: Restore metrics from database
        match crate::modules::proxy_db::load_cache_metrics() {
            Ok(saved) => {
                // Use try_write() to avoid blocking within async runtime
                // Constructor is called from both sync and async contexts
                match monitor.metrics.try_write() {
                    Ok(mut metrics) => {
                        let hit_count = saved.hit_count;
                        let miss_count = saved.miss_count;
                        *metrics = saved;
                        tracing::info!(
                            "[CacheMonitor] Restored cache metrics: hits={}, misses={}, hit_rate={:.2}%",
                            hit_count,
                            miss_count,
                            metrics.hit_rate * 100.0
                        );
                    }
                    Err(_) => {
                        tracing::warn!("[CacheMonitor] Failed to acquire metrics lock during restoration (lock busy)");
                    }
                }
            }
            Err(e) => {
                tracing::info!(
                    "[CacheMonitor] No saved metrics found (first run or empty database): {}. Using defaults.",
                    e
                );
            }
        }

        monitor
    }

    /// Record a cache hit
    /// AC1: Cache hit rate tracking
    pub async fn record_hit(&self, signature: &str, lookup_time: f64, account: Option<&str>) {
        // Update hit counter
        let mut metrics = self.metrics.write().await;
        metrics.hit_count += 1;
        metrics.hit_rate = self.calculate_hit_rate(metrics.hit_count, metrics.miss_count);
        drop(metrics);

        // Track signature reuse
        let mut sigs = self.signatures.write().await;
        let tracker = sigs.entry(signature.to_string()).or_insert_with(|| {
            SignatureTracker::new(signature.to_string())
        });

        // Estimate cost saved (example: $0.001 per cached request)
        let estimated_cost = 0.001;
        tracker.record_reuse(lookup_time, estimated_cost);
        drop(sigs);

        // Track lookup time
        let mut times = self.lookup_times.write().await;
        times.push_back(lookup_time);
        if times.len() > 10000 {
            times.pop_front();
        }
        drop(times);

        // Track account-level costs
        if let Some(acc) = account {
            let mut costs = self.account_costs.write().await;
            *costs.entry(acc.to_string()).or_insert(0.0) += estimated_cost;
        }

        // Track hourly/daily savings
        self.record_savings(estimated_cost).await;

        tracing::debug!("[CacheMonitor] Hit recorded: lookup_time={:.2}ms", lookup_time);
    }

    /// Record a cache miss
    /// AC1: Cache hit rate tracking
    pub async fn record_miss(&self, _signature: &str) {
        let mut metrics = self.metrics.write().await;
        metrics.miss_count += 1;
        metrics.hit_rate = self.calculate_hit_rate(metrics.hit_count, metrics.miss_count);

        tracing::debug!("[CacheMonitor] Miss recorded, hit_rate={:.2}%", metrics.hit_rate * 100.0);
    }

    /// Record a cache write operation
    /// AC4: Performance tracking
    pub async fn record_write(&self, write_time: f64) {
        let mut times = self.write_times.write().await;
        times.push_back(write_time);
        if times.len() > 10000 {
            times.pop_front();
        }

        tracing::debug!("[CacheMonitor] Write recorded: write_time={:.2}ms", write_time);
    }

    /// Calculate hit rate
    fn calculate_hit_rate(&self, hits: u64, misses: u64) -> f32 {
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            hits as f32 / total as f32
        }
    }

    /// Get current hit rate
    /// AC1: Hit rate metric
    pub async fn get_hit_rate(&self) -> f32 {
        self.metrics.read().await.hit_rate
    }

    /// Get top N most reused signatures
    /// AC2: Signature reuse analysis
    pub async fn get_top_signatures(&self, limit: usize) -> Vec<SignatureStats> {
        let sigs = self.signatures.read().await;

        let mut stats: Vec<SignatureStats> = sigs.values()
            .map(|tracker| tracker.to_stats())
            .collect();

        // Sort by reuse count descending
        stats.sort_by(|a, b| b.reuse_count.cmp(&a.reuse_count));

        stats.into_iter().take(limit).collect()
    }

    /// Calculate cost savings
    /// AC3: Cost attribution
    pub async fn calculate_cost_savings(&self) -> CostSavings {
        let sigs = self.signatures.read().await;
        let total_saved: f64 = sigs.values().map(|t| t.cost_saved).sum();

        let account_costs = self.account_costs.read().await.clone();

        let metrics = self.metrics.read().await;
        let total_requests = metrics.hit_count + metrics.miss_count;
        let baseline_cost = total_requests as f64 * 0.001; // Estimated baseline
        let savings_percentage = if baseline_cost > 0.0 {
            (total_saved / baseline_cost) * 100.0
        } else {
            0.0
        };

        let hourly_savings = self.hourly_savings.read().await
            .iter()
            .cloned()
            .collect();

        let daily_savings = self.daily_savings.read().await
            .iter()
            .cloned()
            .collect();

        CostSavings {
            total_saved,
            per_account: account_costs,
            per_user: HashMap::new(), // Future: session-based tracking
            savings_percentage,
            hourly_savings,
            daily_savings,
        }
    }

    /// Export comprehensive metrics
    /// AC5: Dashboard integration
    pub async fn export_metrics(&self) -> CacheMetrics {
        let mut metrics = self.metrics.read().await.clone();

        // Update top signatures
        metrics.top_signatures = self.get_top_signatures(10).await;

        // Update cost savings
        metrics.cost_savings = self.calculate_cost_savings().await;

        // Update performance metrics
        metrics.performance = self.calculate_performance_metrics().await;

        metrics.updated_at = Utc::now();

        metrics
    }

    /// Calculate performance metrics
    /// AC4: Performance benchmarking
    async fn calculate_performance_metrics(&self) -> PerformanceMetrics {
        let lookup_times = self.lookup_times.read().await;
        let write_times = self.write_times.read().await;

        let lookup_p50 = calculate_percentile(&lookup_times, 0.50);
        let lookup_p95 = calculate_percentile(&lookup_times, 0.95);
        let lookup_p99 = calculate_percentile(&lookup_times, 0.99);
        let write_p95 = calculate_percentile(&write_times, 0.95);

        // Check for performance degradation
        let baseline = *self.baseline_p95.read().await;
        let degradation_alert = if baseline > 0.0 {
            lookup_p95 > baseline * 1.2 // 20% degradation threshold
        } else {
            false
        };

        // Update baseline if not set
        if baseline == 0.0 && lookup_p95 > 0.0 {
            *self.baseline_p95.write().await = lookup_p95;
        }

        // Estimate memory usage (rough calculation)
        let sig_count = self.signatures.read().await.len();
        let memory_usage = (sig_count * 500) as u64; // ~500 bytes per signature tracker

        let total_operations = lookup_times.len() as u64 + write_times.len() as u64;

        PerformanceMetrics {
            lookup_p50,
            lookup_p95,
            lookup_p99,
            write_p95,
            memory_usage,
            total_operations,
            degradation_alert,
        }
    }

    /// Record savings with hourly/daily aggregation
    async fn record_savings(&self, amount: f64) {
        let now = Utc::now();
        let hour = now.timestamp() / 3600 * 3600; // Round to hour
        let day = now.timestamp() / 86400 * 86400; // Round to day

        // Update hourly savings
        {
            let mut hourly = self.hourly_savings.write().await;
            if let Some((last_hour, last_amount)) = hourly.back_mut() {
                if *last_hour == hour {
                    *last_amount += amount;
                } else {
                    hourly.push_back((hour, amount));
                }
            } else {
                hourly.push_back((hour, amount));
            }

            // Keep only last 24 hours
            let cutoff = now.timestamp() - 86400;
            while hourly.front().is_some_and(|(h, _)| *h < cutoff) {
                hourly.pop_front();
            }
        }

        // Update daily savings
        {
            let mut daily = self.daily_savings.write().await;
            if let Some((last_day, last_amount)) = daily.back_mut() {
                if *last_day == day {
                    *last_amount += amount;
                } else {
                    daily.push_back((day, amount));
                }
            } else {
                daily.push_back((day, amount));
            }

            // Keep only last 7 days
            let cutoff = now.timestamp() - 7 * 86400;
            while daily.front().is_some_and(|(d, _)| *d < cutoff) {
                daily.pop_front();
            }
        }
    }

    /// Clear all metrics (for testing or reset)
    pub async fn clear(&self) {
        *self.metrics.write().await = CacheMetrics::default();
        self.signatures.write().await.clear();
        self.lookup_times.write().await.clear();
        self.write_times.write().await.clear();
        self.account_costs.write().await.clear();
        self.hourly_savings.write().await.clear();
        self.daily_savings.write().await.clear();
        *self.baseline_p95.write().await = 0.0;

        tracing::info!("[CacheMonitor] All metrics cleared");
    }
}

/// Calculate percentile from a sorted list of values
fn calculate_percentile(values: &VecDeque<f64>, percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    let mut sorted: Vec<f64> = values.iter().copied().collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let index = ((sorted.len() as f64 - 1.0) * percentile) as usize;
    sorted[index]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::proxy_db;
    use rusqlite::Connection;

    /// Helper: Clear cache_metrics table for test isolation
    fn clear_cache_metrics() {
        proxy_db::init_db().expect("DB init should succeed");
        let db_path = proxy_db::get_proxy_db_path().expect("DB path should exist");
        let conn = Connection::open(db_path).expect("DB connection should succeed");
        conn.execute("DELETE FROM cache_metrics", [])
            .expect("Clear should succeed");
    }

    #[tokio::test]
    async fn test_cache_monitor_creation() {
        clear_cache_metrics();

        let monitor = CacheMonitor::new();
        let metrics = monitor.export_metrics().await;

        assert_eq!(metrics.hit_count, 0);
        assert_eq!(metrics.miss_count, 0);
        assert_eq!(metrics.hit_rate, 0.0);
    }

    #[tokio::test]
    async fn test_record_hit_updates_counter() {
        clear_cache_metrics();

        let monitor = CacheMonitor::new();

        monitor.record_hit("sig1", 5.0, None).await;
        monitor.record_hit("sig1", 6.0, None).await;

        let metrics = monitor.export_metrics().await;
        assert_eq!(metrics.hit_count, 2);
        assert_eq!(metrics.miss_count, 0);
    }

    #[tokio::test]
    async fn test_record_miss_updates_counter() {
        clear_cache_metrics();

        let monitor = CacheMonitor::new();

        monitor.record_miss("sig1").await;
        monitor.record_miss("sig2").await;

        let metrics = monitor.export_metrics().await;
        assert_eq!(metrics.hit_count, 0);
        assert_eq!(metrics.miss_count, 2);
    }

    #[tokio::test]
    async fn test_hit_rate_calculation() {
        clear_cache_metrics();

        let monitor = CacheMonitor::new();

        monitor.record_hit("sig1", 5.0, None).await;
        monitor.record_miss("sig2").await;
        monitor.record_hit("sig3", 6.0, None).await;

        let hit_rate = monitor.get_hit_rate().await;
        assert!((hit_rate - 0.666).abs() < 0.01); // 2/3 ≈ 0.666
    }

    #[tokio::test]
    async fn test_signature_reuse_tracking() {
        let monitor = CacheMonitor::new();

        let sig = "test_signature_123";
        monitor.record_hit(sig, 5.0, None).await;
        monitor.record_hit(sig, 6.0, None).await;
        monitor.record_hit(sig, 7.0, None).await;

        let top = monitor.get_top_signatures(10).await;
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].reuse_count, 3);
        assert!(top[0].high_value); // ≥3 reuses
    }

    #[tokio::test]
    async fn test_top_signatures_sorting() {
        let monitor = CacheMonitor::new();

        // Signature 1: 5 hits
        for _ in 0..5 {
            monitor.record_hit("sig1", 5.0, None).await;
        }

        // Signature 2: 3 hits
        for _ in 0..3 {
            monitor.record_hit("sig2", 5.0, None).await;
        }

        // Signature 3: 10 hits
        for _ in 0..10 {
            monitor.record_hit("sig3", 5.0, None).await;
        }

        let top = monitor.get_top_signatures(3).await;
        assert_eq!(top[0].reuse_count, 10); // sig3
        assert_eq!(top[1].reuse_count, 5);  // sig1
        assert_eq!(top[2].reuse_count, 3);  // sig2
    }

    #[tokio::test]
    async fn test_cost_attribution_per_account() {
        let monitor = CacheMonitor::new();

        monitor.record_hit("sig1", 5.0, Some("account1@example.com")).await;
        monitor.record_hit("sig2", 6.0, Some("account1@example.com")).await;
        monitor.record_hit("sig3", 7.0, Some("account2@example.com")).await;

        let savings = monitor.calculate_cost_savings().await;

        assert!(savings.per_account.contains_key("account1@example.com"));
        assert!(savings.per_account.contains_key("account2@example.com"));

        let acc1_savings = savings.per_account.get("account1@example.com").unwrap();
        let acc2_savings = savings.per_account.get("account2@example.com").unwrap();

        assert!((acc1_savings - 0.002).abs() < 0.0001); // 2 hits * $0.001
        assert!((acc2_savings - 0.001).abs() < 0.0001); // 1 hit * $0.001
    }

    #[tokio::test]
    async fn test_performance_metrics_percentile() {
        let monitor = CacheMonitor::new();

        // Record varied lookup times
        monitor.record_hit("sig1", 1.0, None).await;
        monitor.record_hit("sig2", 5.0, None).await;
        monitor.record_hit("sig3", 10.0, None).await;
        monitor.record_hit("sig4", 15.0, None).await;
        monitor.record_hit("sig5", 20.0, None).await;

        let metrics = monitor.export_metrics().await;

        assert!(metrics.performance.lookup_p50 > 0.0);
        assert!(metrics.performance.lookup_p95 > 0.0);
        assert!(metrics.performance.lookup_p95 >= metrics.performance.lookup_p50);
    }

    #[tokio::test]
    async fn test_clear_metrics() {
        let monitor = CacheMonitor::new();

        monitor.record_hit("sig1", 5.0, None).await;
        monitor.record_miss("sig2").await;

        monitor.clear().await;

        let metrics = monitor.export_metrics().await;
        assert_eq!(metrics.hit_count, 0);
        assert_eq!(metrics.miss_count, 0);
        assert_eq!(metrics.top_signatures.len(), 0);
    }

    #[tokio::test]
    async fn test_savings_percentage_calculation() {
        clear_cache_metrics();

        let monitor = CacheMonitor::new();

        // Simulate 30% hit rate
        for _ in 0..30 {
            monitor.record_hit("sig", 5.0, None).await;
        }
        for _ in 0..70 {
            monitor.record_miss("sig_miss").await;
        }

        let savings = monitor.calculate_cost_savings().await;

        // 30 hits * $0.001 saved / 100 total * $0.001 baseline = 30%
        assert!((savings.savings_percentage - 30.0).abs() < 1.0);
    }
}
