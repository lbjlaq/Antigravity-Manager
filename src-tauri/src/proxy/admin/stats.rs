use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// 全局请求统计追踪器
#[derive(Debug)]
pub struct StatsTracker {
    requests_total: AtomicU64,
    requests_ok: AtomicU64,
    requests_err: AtomicU64,
    latencies: RwLock<Vec<u64>>,
    hourly_counts: RwLock<[u64; 6]>,
    start_time: Instant,
    last_hour_slot: RwLock<usize>,
}

impl Default for StatsTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl StatsTracker {
    pub fn new() -> Self {
        Self {
            requests_total: AtomicU64::new(0),
            requests_ok: AtomicU64::new(0),
            requests_err: AtomicU64::new(0),
            latencies: RwLock::new(Vec::with_capacity(1000)),
            hourly_counts: RwLock::new([0; 6]),
            start_time: Instant::now(),
            last_hour_slot: RwLock::new(0),
        }
    }

    /// 记录一次请求
    pub async fn record_request(&self, success: bool, latency_ms: u64) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
        if success {
            self.requests_ok.fetch_add(1, Ordering::Relaxed);
        } else {
            self.requests_err.fetch_add(1, Ordering::Relaxed);
        }

        // 记录延迟 (保留最近1000条)
        {
            let mut latencies = self.latencies.write().await;
            if latencies.len() >= 1000 {
                latencies.remove(0);
            }
            latencies.push(latency_ms);
        }

        // 更新小时统计
        self.update_hourly_count().await;
    }

    async fn update_hourly_count(&self) {
        let elapsed_hours = self.start_time.elapsed().as_secs() / 3600;
        let current_slot = (elapsed_hours % 6) as usize;

        let mut last_slot = self.last_hour_slot.write().await;
        let mut hourly = self.hourly_counts.write().await;

        if current_slot != *last_slot {
            // 清零新的时间槽
            hourly[current_slot] = 0;
            *last_slot = current_slot;
        }
        hourly[current_slot] += 1;
    }

    /// 获取统计快照
    pub async fn snapshot(&self) -> StatsSnapshot {
        let total = self.requests_total.load(Ordering::Relaxed);
        let ok = self.requests_ok.load(Ordering::Relaxed);
        let err = self.requests_err.load(Ordering::Relaxed);

        let success_rate = if total > 0 {
            ok as f64 / total as f64
        } else {
            0.0
        };

        let (avg_latency, p95_latency) = {
            let latencies = self.latencies.read().await;
            if latencies.is_empty() {
                (0.0, 0.0)
            } else {
                let avg = latencies.iter().sum::<u64>() as f64 / latencies.len() as f64;
                let mut sorted = latencies.clone();
                sorted.sort_unstable();
                let p95_idx = (sorted.len() as f64 * 0.95) as usize;
                let p95 = sorted.get(p95_idx.min(sorted.len() - 1)).copied().unwrap_or(0) as f64;
                (avg, p95)
            }
        };

        // 计算RPS (基于最近1分钟)
        let elapsed_secs = self.start_time.elapsed().as_secs().max(1);
        let rps = if elapsed_secs <= 60 {
            total as f64 / elapsed_secs as f64
        } else {
            // 使用最近一小时的数据估算
            let hourly = self.hourly_counts.read().await;
            let recent = hourly[0]; // 当前小时
            recent as f64 / 3600.0
        };

        let hourly_counts = {
            let hourly = self.hourly_counts.read().await;
            hourly.to_vec()
        };

        StatsSnapshot {
            requests_total: total,
            requests_ok: ok,
            requests_err: err,
            success_rate,
            latency_ms_avg: avg_latency,
            latency_ms_p95: p95_latency,
            rps,
            hourly_counts,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StatsSnapshot {
    pub requests_total: u64,
    pub requests_ok: u64,
    pub requests_err: u64,
    pub success_rate: f64,
    pub latency_ms_avg: f64,
    pub latency_ms_p95: f64,
    pub rps: f64,
    pub hourly_counts: Vec<u64>,
}

/// 全局统计实例
static STATS: once_cell::sync::Lazy<Arc<StatsTracker>> =
    once_cell::sync::Lazy::new(|| Arc::new(StatsTracker::new()));

pub fn global_stats() -> Arc<StatsTracker> {
    STATS.clone()
}
