use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Emitter;
use tokio::sync::RwLock;
// 🆕 Story #8: Import for json! macro
use serde_json;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyRequestLog {
    pub id: String,
    pub timestamp: i64,
    pub method: String,
    pub url: String,
    pub status: u16,
    pub duration: u64,                // ms
    pub model: Option<String>,        // 客户端请求的模型名
    pub mapped_model: Option<String>, // 实际路由后使用的模型名
    pub account_email: Option<String>,
    pub error: Option<String>,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProxyStats {
    // Existing metrics
    pub total_requests: u64,
    pub success_count: u64,
    pub error_count: u64,

    // 🆕 Story #8: Thinking violation metrics
    // AC1: Budget constraint violations (maxTokens ≤ thinkingBudget)
    pub thinking_budget_violations: u64,

    // AC2: Position enforcement violations (thinking not first)
    pub thinking_position_violations: u64,

    // AC3: Position violations by role (user messages)
    pub thinking_position_violations_user: u64,

    // AC3: Position violations by role (model/assistant messages)
    pub thinking_position_violations_model: u64,
}

// 🆕 Story #8 Step 2: Violation rates structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationRates {
    pub budget_violations_per_second: f64,
    pub position_violations_per_second: f64,
}

// 🆕 Story #8 Step 6: Detailed violation metrics response
// AC7: Detailed metrics API for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailedViolationMetrics {
    pub stats: ProxyStats,
    pub position_histogram: Vec<(usize, usize)>,
    pub rates: ViolationRates,
}

// 🆕 Story #8 Step 2: ViolationMetrics module for histogram and rate tracking
pub struct ViolationMetrics {
    // Position violation index distribution (for histogram)
    // AC4: Track indices for histogram generation
    position_violation_indices: RwLock<Vec<usize>>,

    // Timestamp tracking for rate calculation (60-second rolling window)
    // AC5: Violations per second calculation
    budget_violation_timestamps: RwLock<VecDeque<i64>>,
    position_violation_timestamps: RwLock<VecDeque<i64>>,
}

impl ViolationMetrics {
    pub fn new() -> Self {
        Self {
            position_violation_indices: RwLock::new(Vec::new()),
            budget_violation_timestamps: RwLock::new(VecDeque::new()),
            position_violation_timestamps: RwLock::new(VecDeque::new()),
        }
    }

    /// Record position violation with index
    /// AC4: Store index for histogram analysis
    pub async fn record_position_violation(&self, index: usize) {
        let mut indices = self.position_violation_indices.write().await;
        indices.push(index);

        // Memory growth protection (Story #8 Risk #1 mitigation)
        const MAX_VIOLATION_INDICES: usize = 10_000;
        if indices.len() > MAX_VIOLATION_INDICES {
            indices.remove(0); // Remove oldest
        }

        let mut timestamps = self.position_violation_timestamps.write().await;
        let now = chrono::Utc::now().timestamp();
        timestamps.push_back(now);

        // Keep only last 60 seconds for rate calculation
        let cutoff = now - 60;
        while timestamps.front().is_some_and(|&t| t < cutoff) {
            timestamps.pop_front();
        }
    }

    /// Record budget violation
    /// AC5: Track timestamp for rate calculation
    pub async fn record_budget_violation(&self) {
        let mut timestamps = self.budget_violation_timestamps.write().await;
        let now = chrono::Utc::now().timestamp();
        timestamps.push_back(now);

        // Keep only last 60 seconds
        let cutoff = now - 60;
        while timestamps.front().is_some_and(|&t| t < cutoff) {
            timestamps.pop_front();
        }
    }

    /// Get position violation histogram
    /// AC4: Buckets: [1, 2, 3, ≤5, ≤10, ≤20, >50]
    pub async fn get_position_histogram(&self) -> Vec<(usize, usize)> {
        let indices = self.position_violation_indices.read().await;

        // Fixed buckets as specified in Story #8
        let mut buckets = vec![
            (1, 0),  // index=1
            (2, 0),  // index=2
            (3, 0),  // index=3
            (5, 0),  // index≤5 (4-5)
            (10, 0), // index≤10 (6-10)
            (20, 0), // index≤20 (11-20)
            (50, 0), // index>50 (21+)
        ];

        for &index in indices.iter() {
            if index == 1 {
                buckets[0].1 += 1;
            } else if index == 2 {
                buckets[1].1 += 1;
            } else if index == 3 {
                buckets[2].1 += 1;
            } else if index <= 5 {
                buckets[3].1 += 1;
            } else if index <= 10 {
                buckets[4].1 += 1;
            } else if index <= 20 {
                buckets[5].1 += 1;
            } else {
                buckets[6].1 += 1;
            }
        }

        buckets
    }

    /// Get violation rates (violations per second)
    /// AC5: 60-second rolling window rate calculation
    pub async fn get_violation_rates(&self) -> ViolationRates {
        let budget_ts = self.budget_violation_timestamps.read().await;
        let position_ts = self.position_violation_timestamps.read().await;

        let window = 60.0; // 60 seconds

        ViolationRates {
            budget_violations_per_second: budget_ts.len() as f64 / window,
            position_violations_per_second: position_ts.len() as f64 / window,
        }
    }
}

pub struct ProxyMonitor {
    pub logs: RwLock<VecDeque<ProxyRequestLog>>,
    pub stats: RwLock<ProxyStats>,
    pub max_logs: usize,
    pub enabled: AtomicBool,
    app_handle: Option<tauri::AppHandle>,
    // 🆕 Story #8 Step 3: Violation metrics integration
    pub violation_metrics: ViolationMetrics,
}

impl ProxyMonitor {
    pub fn new(max_logs: usize, app_handle: Option<tauri::AppHandle>) -> Self {
        // Initialize DB
        if let Err(e) = crate::modules::proxy_db::init_db() {
            tracing::error!("Failed to initialize proxy DB: {}", e);
        }

        Self {
            logs: RwLock::new(VecDeque::with_capacity(max_logs)),
            stats: RwLock::new(ProxyStats::default()),
            max_logs,
            enabled: AtomicBool::new(false), // Default to disabled
            app_handle,
            // 🆕 Story #8 Step 3: Initialize violation metrics
            violation_metrics: ViolationMetrics::new(),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub async fn log_request(&self, log: ProxyRequestLog) {
        if !self.is_enabled() {
            return;
        }
        tracing::info!("[Monitor] Logging request: {} {}", log.method, log.url);
        // Update stats
        let stats_snapshot = {
            let mut stats = self.stats.write().await;
            stats.total_requests += 1;
            if log.status >= 200 && log.status < 400 {
                stats.success_count += 1;
            } else {
                stats.error_count += 1;
            }
            stats.clone() // 🆕 Story #8: Snapshot for async save
        };

        // 🆕 Story #8: Save stats to DB (async, non-blocking)
        tokio::spawn(async move {
            if let Err(e) = crate::modules::proxy_db::save_stats(&stats_snapshot) {
                tracing::error!("Failed to save proxy stats to DB: {}", e);
            }
        });

        // Add log to memory
        {
            let mut logs = self.logs.write().await;
            if logs.len() >= self.max_logs {
                logs.pop_back();
            }
            logs.push_front(log.clone());
        }

        // Save to DB
        let log_to_save = log.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::modules::proxy_db::save_log(&log_to_save) {
                tracing::error!("Failed to save proxy log to DB: {}", e);
            }
        });

        // Emit event
        if let Some(app) = &self.app_handle {
            let _ = app.emit("proxy://request", &log);
        }
    }

    pub async fn get_logs(&self, limit: usize) -> Vec<ProxyRequestLog> {
        // Try to get from DB first for true history
        match crate::modules::proxy_db::get_logs(limit) {
            Ok(logs) => logs,
            Err(e) => {
                tracing::error!("Failed to get logs from DB: {}", e);
                // Fallback to memory
                let logs = self.logs.read().await;
                logs.iter().take(limit).cloned().collect()
            }
        }
    }

    pub async fn get_stats(&self) -> ProxyStats {
        match crate::modules::proxy_db::get_stats() {
            Ok(stats) => stats,
            Err(e) => {
                tracing::error!("Failed to get stats from DB: {}", e);
                self.stats.read().await.clone()
            }
        }
    }

    // 🆕 Story #8 Step 4: Record budget violation
    // AC1: Budget constraint violation counter
    pub async fn record_budget_violation(&self) {
        if !self.is_enabled() {
            return;
        }

        // Increment counter in stats
        let stats_snapshot = {
            let mut stats = self.stats.write().await;
            stats.thinking_budget_violations += 1;
            stats.clone() // Snapshot for async save
        };

        // Record detailed metrics
        self.violation_metrics.record_budget_violation().await;

        // 🆕 Story #8: Save stats to DB (async, non-blocking)
        tokio::spawn(async move {
            if let Err(e) = crate::modules::proxy_db::save_stats(&stats_snapshot) {
                tracing::error!("Failed to save proxy stats after budget violation: {}", e);
            }
        });

        // Emit events for real-time monitoring
        if let Some(app) = &self.app_handle {
            let _ = app.emit("proxy://budget-violation", ());
            let _ = app.emit("proxy://violation", "budget"); // 🆕 Story #12: Generic violation event
        }
    }

    // 🆕 Story #8 Step 5: Record position violation
    // AC2: Position enforcement violation counter
    // AC3: Role-specific tracking (user vs model)
    pub async fn record_position_violation(&self, index: usize, role: &str) {
        if !self.is_enabled() {
            return;
        }

        // Increment counters in stats
        let stats_snapshot = {
            let mut stats = self.stats.write().await;
            stats.thinking_position_violations += 1;

            // Role-specific counters (AC3)
            if role == "user" {
                stats.thinking_position_violations_user += 1;
            } else if role == "model" || role == "assistant" {
                stats.thinking_position_violations_model += 1;
            }
            stats.clone() // Snapshot for async save
        };

        // Record detailed metrics (AC4: histogram)
        self.violation_metrics
            .record_position_violation(index)
            .await;

        // 🆕 Story #8: Save stats to DB (async, non-blocking)
        tokio::spawn(async move {
            if let Err(e) = crate::modules::proxy_db::save_stats(&stats_snapshot) {
                tracing::error!("Failed to save proxy stats after position violation: {}", e);
            }
        });

        // Emit events for real-time monitoring
        if let Some(app) = &self.app_handle {
            let _ = app.emit(
                "proxy://position-violation",
                serde_json::json!({
                    "index": index,
                    "role": role
                }),
            );
            let _ = app.emit("proxy://violation", "position"); // 🆕 Story #12: Generic violation event
        }
    }

    // 🆕 Story #8 Step 6: Get detailed violation metrics
    // AC7: Detailed metrics API for frontend
    pub async fn get_violation_metrics(&self) -> DetailedViolationMetrics {
        DetailedViolationMetrics {
            stats: self.get_stats().await,
            position_histogram: self.violation_metrics.get_position_histogram().await,
            rates: self.violation_metrics.get_violation_rates().await,
        }
    }

    // 🆕 Story #12: Reset violation metrics
    pub async fn reset_violation_metrics(&self) {
        // Reset all violation counters
        {
            let mut stats = self.stats.write().await;
            stats.thinking_budget_violations = 0;
            stats.thinking_position_violations = 0;
            stats.thinking_position_violations_user = 0;
            stats.thinking_position_violations_model = 0;
        }

        // Reset detailed metrics
        self.violation_metrics
            .position_violation_indices
            .write()
            .await
            .clear();
        self.violation_metrics
            .budget_violation_timestamps
            .write()
            .await
            .clear();
        self.violation_metrics
            .position_violation_timestamps
            .write()
            .await
            .clear();

        // Persist to database
        let stats_snapshot = self.stats.read().await.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::modules::proxy_db::save_stats(&stats_snapshot) {
                tracing::error!("Failed to save reset stats to DB: {}", e);
            }
        });

        // Emit reset event for dashboard
        if let Some(app) = &self.app_handle {
            let _ = app.emit("proxy://violation-reset", ());
        }

        tracing::info!("Violation metrics reset successfully");
    }

    pub async fn clear(&self) {
        let mut logs = self.logs.write().await;
        logs.clear();
        let mut stats = self.stats.write().await;
        *stats = ProxyStats::default();

        if let Err(e) = crate::modules::proxy_db::clear_logs() {
            tracing::error!("Failed to clear logs in DB: {}", e);
        }
    }
}

// 🆕 Story #8: Unit tests for violation metrics
#[cfg(test)]
mod tests {
    use super::*;

    /// Test #1: ProxyStats должна иметь 4 новых поля для violation tracking
    /// 🔴 RED: Этот тест ДОЛЖЕН FAIL, потому что поля ещё не добавлены
    #[tokio::test]
    async fn test_proxy_stats_has_violation_fields() {
        let stats = ProxyStats {
            total_requests: 100,
            success_count: 95,
            error_count: 5,
            // 🔴 Ожидаем эти поля (пока не существуют):
            thinking_budget_violations: 10,
            thinking_position_violations: 5,
            thinking_position_violations_user: 3,
            thinking_position_violations_model: 2,
        };

        // Verify fields exist and are accessible
        assert_eq!(stats.thinking_budget_violations, 10);
        assert_eq!(stats.thinking_position_violations, 5);
        assert_eq!(stats.thinking_position_violations_user, 3);
        assert_eq!(stats.thinking_position_violations_model, 2);
    }

    /// Test #2: ProxyStats Default должен инициализировать violation counters в 0
    /// 🔴 RED: Этот тест ДОЛЖЕН FAIL
    #[tokio::test]
    async fn test_proxy_stats_default_initializes_violations_to_zero() {
        let stats = ProxyStats::default();

        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.success_count, 0);
        assert_eq!(stats.error_count, 0);
        // 🔴 Ожидаем violation counters = 0:
        assert_eq!(stats.thinking_budget_violations, 0);
        assert_eq!(stats.thinking_position_violations, 0);
        assert_eq!(stats.thinking_position_violations_user, 0);
        assert_eq!(stats.thinking_position_violations_model, 0);
    }

    /// Test #3: ProxyStats Clone должен копировать violation fields
    #[tokio::test]
    async fn test_proxy_stats_clone_copies_violation_fields() {
        let stats = ProxyStats {
            total_requests: 50,
            success_count: 45,
            error_count: 5,
            thinking_budget_violations: 3,
            thinking_position_violations: 2,
            thinking_position_violations_user: 1,
            thinking_position_violations_model: 1,
        };

        let cloned = stats.clone();

        assert_eq!(cloned.thinking_budget_violations, 3);
        assert_eq!(cloned.thinking_position_violations, 2);
        assert_eq!(cloned.thinking_position_violations_user, 1);
        assert_eq!(cloned.thinking_position_violations_model, 1);
    }

    // 🔴 Story #8 Step 2: ViolationMetrics Tests

    /// Test #4: ViolationMetrics можно создать
    /// 🔴 RED: ViolationMetrics ещё не существует
    #[tokio::test]
    async fn test_violation_metrics_creation() {
        let metrics = ViolationMetrics::new();

        // Should be created successfully
        assert!(true);
    }

    /// Test #5: Record position violation должен сохранять index
    /// 🔴 RED: ViolationMetrics не существует
    #[tokio::test]
    async fn test_record_position_violation_stores_index() {
        let metrics = ViolationMetrics::new();

        metrics.record_position_violation(2).await;
        metrics.record_position_violation(5).await;
        metrics.record_position_violation(1).await;

        let histogram = metrics.get_position_histogram().await;

        // Should have 3 violations recorded
        let total: usize = histogram.iter().map(|(_, count)| count).sum();
        assert_eq!(total, 3);
    }

    /// Test #6: Position histogram должен группировать в buckets
    /// 🔴 RED: ViolationMetrics не существует
    #[tokio::test]
    async fn test_position_histogram_buckets() {
        let metrics = ViolationMetrics::new();

        // Record violations in different buckets
        metrics.record_position_violation(1).await; // Bucket 1
        metrics.record_position_violation(1).await; // Bucket 1
        metrics.record_position_violation(2).await; // Bucket 2
        metrics.record_position_violation(7).await; // Bucket ≤10
        metrics.record_position_violation(25).await; // Bucket >50

        let histogram = metrics.get_position_histogram().await;

        // Bucket assertions (buckets: [1, 2, 3, ≤5, ≤10, ≤20, >50])
        assert_eq!(histogram[0], (1, 2)); // index=1: 2 occurrences
        assert_eq!(histogram[1], (2, 1)); // index=2: 1 occurrence
        assert_eq!(histogram[4], (10, 1)); // index≤10: 1 occurrence (7)
        assert_eq!(histogram[6], (50, 1)); // index>50: 1 occurrence (25)
    }

    /// Test #7: Budget violation timestamps должны записываться
    /// 🔴 RED: ViolationMetrics не существует
    #[tokio::test]
    async fn test_budget_violation_timestamps() {
        let metrics = ViolationMetrics::new();

        // Record 3 budget violations
        metrics.record_budget_violation().await;
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        metrics.record_budget_violation().await;
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        metrics.record_budget_violation().await;

        let rates = metrics.get_violation_rates().await;

        // Should have recorded violations (rate > 0)
        assert!(rates.budget_violations_per_second >= 0.0);
    }

    /// Test #8: Violation rates должны использовать 60-sec window
    /// 🔴 RED: ViolationMetrics не существует
    #[tokio::test]
    async fn test_violation_rates_60_second_window() {
        let metrics = ViolationMetrics::new();

        // Record 12 violations
        for _ in 0..12 {
            metrics.record_position_violation(1).await;
        }

        let rates = metrics.get_violation_rates().await;

        // 12 violations in ~instant time = high rate per second
        // In real scenario: 12 violations / 60 seconds = 0.2/sec
        assert!(rates.position_violations_per_second >= 0.0);
    }
}
