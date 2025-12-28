use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// 时间桶数据
#[derive(Debug, Clone, Default)]
struct TimeBucket {
    count: u64,
    success: u64,
    error: u64,
    total_latency: u64,
}

/// 全局请求统计追踪器
/// 使用1分钟粒度的时间桶，保留最近12小时（720个桶）的数据
#[derive(Debug)]
pub struct StatsTracker {
    requests_total: AtomicU64,
    requests_ok: AtomicU64,
    requests_err: AtomicU64,
    latencies: RwLock<Vec<u64>>,
    start_time: Instant,
    /// 时间桶数组，每分钟一个桶，共720个（12小时）
    time_buckets: RwLock<Vec<TimeBucket>>,
    /// 当前分钟索引
    current_minute: RwLock<u64>,
}

const BUCKET_COUNT: usize = 720; // 12小时 * 60分钟

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
            start_time: Instant::now(),
            time_buckets: RwLock::new(vec![TimeBucket::default(); BUCKET_COUNT]),
            current_minute: RwLock::new(0),
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

        // 更新时间桶
        self.update_time_bucket(success, latency_ms).await;
    }

    async fn update_time_bucket(&self, success: bool, latency_ms: u64) {
        let elapsed_minutes = self.start_time.elapsed().as_secs() / 60;
        let bucket_index = (elapsed_minutes as usize) % BUCKET_COUNT;

        let mut current_minute = self.current_minute.write().await;
        let mut buckets = self.time_buckets.write().await;

        // 如果进入新的分钟，清理过期的桶
        if elapsed_minutes > *current_minute {
            // 清理从上次记录到当前时间之间的所有桶
            let start = ((*current_minute + 1) as usize) % BUCKET_COUNT;
            let end = bucket_index;

            if start <= end {
                for i in start..=end {
                    buckets[i] = TimeBucket::default();
                }
            } else {
                // 跨越数组边界
                for i in start..BUCKET_COUNT {
                    buckets[i] = TimeBucket::default();
                }
                for i in 0..=end {
                    buckets[i] = TimeBucket::default();
                }
            }
            *current_minute = elapsed_minutes;
        }

        // 更新当前桶
        buckets[bucket_index].count += 1;
        buckets[bucket_index].total_latency += latency_ms;
        if success {
            buckets[bucket_index].success += 1;
        } else {
            buckets[bucket_index].error += 1;
        }
    }

    /// 获取指定时间窗口的数据点
    /// window_minutes: 时间窗口大小（分钟）
    /// points: 返回的数据点数量
    async fn get_time_series(&self, window_minutes: usize, points: usize) -> Vec<TimeSeriesPoint> {
        let elapsed_minutes = self.start_time.elapsed().as_secs() / 60;
        let current_bucket = (elapsed_minutes as usize) % BUCKET_COUNT;
        let buckets = self.time_buckets.read().await;

        let mut result = Vec::with_capacity(points);
        let bucket_per_point = window_minutes.max(points) / points;

        for i in 0..points {
            let mut point = TimeSeriesPoint::default();

            // 聚合每个点对应的桶
            for j in 0..bucket_per_point {
                let offset = (points - 1 - i) * bucket_per_point + j;
                if offset >= window_minutes || offset >= BUCKET_COUNT {
                    continue;
                }

                let bucket_idx = if current_bucket >= offset {
                    current_bucket - offset
                } else {
                    BUCKET_COUNT - (offset - current_bucket)
                };

                let bucket = &buckets[bucket_idx];
                point.count += bucket.count;
                point.success += bucket.success;
                point.error += bucket.error;
                point.total_latency += bucket.total_latency;
            }

            if point.count > 0 {
                point.avg_latency = point.total_latency as f64 / point.count as f64;
                point.success_rate = point.success as f64 / point.count as f64;
            }

            result.push(point);
        }

        result
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

        // 计算最近1分钟的 RPS
        let rps = {
            let elapsed_minutes = self.start_time.elapsed().as_secs() / 60;
            let current_bucket = (elapsed_minutes as usize) % BUCKET_COUNT;
            let buckets = self.time_buckets.read().await;
            let current_count = buckets[current_bucket].count;
            let elapsed_in_minute = self.start_time.elapsed().as_secs() % 60;
            if elapsed_in_minute > 0 {
                current_count as f64 / elapsed_in_minute as f64
            } else {
                current_count as f64
            }
        };

        // 生成3个时间维度的数据（统一12个点）
        let time_series_10m = self.get_time_series(10, 12).await;
        let time_series_1h = self.get_time_series(60, 12).await;
        let time_series_4h = self.get_time_series(240, 12).await;

        StatsSnapshot {
            requests_total: total,
            requests_ok: ok,
            requests_err: err,
            success_rate,
            latency_ms_avg: avg_latency,
            latency_ms_p95: p95_latency,
            rps,
            time_series: TimeSeriesData {
                m10: time_series_10m,
                h1: time_series_1h,
                h4: time_series_4h,
            },
        }
    }
}

/// 时间序列数据点
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct TimeSeriesPoint {
    pub count: u64,
    pub success: u64,
    pub error: u64,
    #[serde(skip)]
    pub total_latency: u64,
    pub avg_latency: f64,
    pub success_rate: f64,
}

/// 多时间维度的时间序列数据
#[derive(Debug, Clone, serde::Serialize)]
pub struct TimeSeriesData {
    #[serde(rename = "10m")]
    pub m10: Vec<TimeSeriesPoint>,
    #[serde(rename = "1h")]
    pub h1: Vec<TimeSeriesPoint>,
    #[serde(rename = "4h")]
    pub h4: Vec<TimeSeriesPoint>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StatsSnapshot {
    pub requests_total: u64,
    pub requests_ok: u64,
    pub requests_err: u64,
    pub success_rate: f64,
    pub latency_ms_avg: f64,
    pub latency_ms_p95: f64,
    pub rps: f64,
    pub time_series: TimeSeriesData,
}

/// 全局统计实例
static STATS: once_cell::sync::Lazy<Arc<StatsTracker>> =
    once_cell::sync::Lazy::new(|| Arc::new(StatsTracker::new()));

pub fn global_stats() -> Arc<StatsTracker> {
    STATS.clone()
}
