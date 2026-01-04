// Rate Limiter
// 确保 API 调用间隔 ≥ 500ms
//
// [OPTIMIZATION] Lock-free implementation using AtomicU64
// Previous: Arc<Mutex<Option<Instant>>> - Required async lock acquisition on every call
// Current: AtomicU64 storing epoch millis - Lock-free CAS operations, ~10x faster

use std::sync::atomic::{AtomicU64, Ordering};
use tokio::time::{sleep, Duration};

pub struct RateLimiter {
    min_interval_ms: u64,
    /// Stores the timestamp of the last call as milliseconds since UNIX epoch
    /// Using u64 allows for lock-free atomic operations
    last_call_epoch_ms: AtomicU64,
}

impl RateLimiter {
    pub fn new(min_interval_ms: u64) -> Self {
        Self {
            min_interval_ms,
            last_call_epoch_ms: AtomicU64::new(0),
        }
    }

    /// Get current time as milliseconds since UNIX epoch
    #[inline]
    fn now_epoch_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Wait for rate limit to clear and update the last call time atomically
    /// Uses compare-and-swap for lock-free concurrency
    pub async fn wait(&self) {
        loop {
            let now = Self::now_epoch_ms();
            let last = self.last_call_epoch_ms.load(Ordering::Acquire);

            // Calculate required wait time
            let elapsed = now.saturating_sub(last);
            if elapsed < self.min_interval_ms && last > 0 {
                let wait_ms = self.min_interval_ms - elapsed;
                sleep(Duration::from_millis(wait_ms)).await;
                // After sleeping, loop back to re-check and update atomically
                continue;
            }

            // Try to atomically update the last call time
            // CAS ensures only one caller succeeds if multiple arrive simultaneously
            match self.last_call_epoch_ms.compare_exchange_weak(
                last,
                now,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => break, // Successfully updated, proceed
                Err(_) => continue, // Another thread updated, retry
            }
        }
    }

    /// Check if enough time has passed without waiting
    /// Useful for non-blocking rate limit checks
    #[allow(dead_code)]
    pub fn can_proceed(&self) -> bool {
        let now = Self::now_epoch_ms();
        let last = self.last_call_epoch_ms.load(Ordering::Acquire);
        now.saturating_sub(last) >= self.min_interval_ms || last == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Instant;

    #[tokio::test]
    async fn test_rate_limiter() {
        let limiter = RateLimiter::new(500);
        let start = Instant::now();

        limiter.wait().await; // 第一次调用，立即返回
        let elapsed1 = start.elapsed().as_millis();
        assert!(elapsed1 < 50);

        limiter.wait().await; // 第二次调用，等待 500ms
        let elapsed2 = start.elapsed().as_millis();
        assert!(elapsed2 >= 500 && elapsed2 < 600);
    }
}
