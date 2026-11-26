//! 后台全量配额自动刷新调度器 (Fixes #3958)。
//!
//! 在后端常驻轮询协程中消费 `auto_refresh` 与 `refresh_interval` 配置，
//! 确保无论在无头模式 (`--headless`) 还是 GUI 模式下均能独立且定时地刷新账号配额；
//! 同时收敛全局定时刷新权威来源，避免多前端窗口（如主窗口 + 悬浮窗）并发重复轮询。

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

use crate::modules::{config, logger};

/// 启动后给账号索引加载留出的短暂等待
const INITIAL_STARTUP_DELAY: Duration = Duration::from_secs(5);
/// 调度器状态检测轮询步长（秒）
const POLL_INTERVAL_SECS: u64 = 10;

/// 单进程只允许启动一次后台定时器
static AUTO_REFRESH_STARTED: AtomicBool = AtomicBool::new(false);
/// 上次成功触发/完成全量刷新的时间戳（秒）
static LAST_REFRESH_TIMESTAMP: AtomicI64 = AtomicI64::new(0);
/// 标识当前是否有全量刷新任务正在执行中，防止慢速网络下多轮任务重叠并发
static IS_REFRESHING: AtomicBool = AtomicBool::new(false);

/// 记录全量配额刷新的最新时间戳（无论由启动刷新、手动触发还是后台定时触发）
pub fn record_refresh_timestamp(ts: i64) {
    LAST_REFRESH_TIMESTAMP.store(ts, Ordering::SeqCst);
}

/// 获取上次全量刷新的时间戳
pub fn get_last_refresh_timestamp() -> i64 {
    LAST_REFRESH_TIMESTAMP.load(Ordering::SeqCst)
}

/// 检查当前是否正在执行全量刷新
pub fn is_refreshing() -> bool {
    IS_REFRESHING.load(Ordering::SeqCst)
}

/// 判断当前时间点是否满足自动刷新触发条件（纯函数，便于单测验证）
pub fn should_trigger_auto_refresh(
    auto_refresh_enabled: bool,
    refresh_interval_mins: i32,
    now_ts: i64,
    last_refresh_ts: i64,
    currently_refreshing: bool,
) -> bool {
    if !auto_refresh_enabled || refresh_interval_mins <= 0 {
        return false;
    }
    if currently_refreshing {
        return false;
    }
    let interval_secs = (refresh_interval_mins.max(1) as i64).saturating_mul(60);
    now_ts.saturating_sub(last_refresh_ts) >= interval_secs
}

/// RAII 刷新并发互斥守卫：保证在任务成功、失败或异常 Panic 栈展开时均能安全释放锁
pub struct RefreshLockGuard;

impl Drop for RefreshLockGuard {
    fn drop(&mut self) {
        IS_REFRESHING.store(false, Ordering::SeqCst);
    }
}

/// 尝试获取刷新任务并发锁；若当前已有刷新任务在执行中则返回 None
pub fn try_acquire_refresh_lock() -> Option<RefreshLockGuard> {
    if IS_REFRESHING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        Some(RefreshLockGuard)
    } else {
        None
    }
}

/// 启动后台自动配额刷新调度协程
pub fn start_auto_refresh(
    app_handle: Option<tauri::AppHandle>,
    proxy_state: crate::commands::proxy::ProxyServiceState,
) {
    if AUTO_REFRESH_STARTED.swap(true, Ordering::SeqCst) {
        logger::log_info("[AutoRefresh] Background auto-refresh worker is already running");
        return;
    }

    tauri::async_runtime::spawn(async move {
        logger::log_info(
            "[AutoRefresh] Background auto-refresh scheduler started. Initializing timer...",
        );

        // 初始等待，避开启动阶段的瞬态并发
        tokio::time::sleep(INITIAL_STARTUP_DELAY).await;

        // 如果启动时未设置时间戳，初始化为当前时间戳（等待满一个周期再触发首次定时刷新）
        if get_last_refresh_timestamp() == 0 {
            record_refresh_timestamp(chrono::Utc::now().timestamp());
        }

        let mut interval = tokio::time::interval(Duration::from_secs(POLL_INTERVAL_SECS));
        // 系统从睡眠/休眠中恢复时跳过补发堆积 ticks，防止瞬间突发磁盘读与 CPU 颠簸
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        let mut cached_config: Option<crate::models::AppConfig> = None;
        let mut consecutive_config_errors = 0u32;

        loop {
            interval.tick().await;

            let app_config = match config::load_app_config() {
                Ok(cfg) => {
                    consecutive_config_errors = 0;
                    cached_config = Some(cfg.clone());
                    cfg
                }
                Err(e) => {
                    consecutive_config_errors = consecutive_config_errors.saturating_add(1);
                    if consecutive_config_errors == 1
                        || consecutive_config_errors.is_multiple_of(60)
                    {
                        logger::log_warn(&format!(
                            "[AutoRefresh] Failed to load config in scheduler tick (streak: {}): {}",
                            consecutive_config_errors, e
                        ));
                    }
                    match cached_config.as_ref() {
                        Some(cached) => cached.clone(),
                        None => continue,
                    }
                }
            };

            let now = chrono::Utc::now().timestamp();
            let last = get_last_refresh_timestamp();

            // 检测系统时钟回拨（如休眠唤醒、NTP对时修正），重置权威锚点避免调度死锁
            if last > now {
                logger::log_warn(
                    "[AutoRefresh] System clock moved backward; resetting refresh anchor to current time",
                );
                record_refresh_timestamp(now);
            }

            if should_trigger_auto_refresh(
                !config::is_silent_mode_effective(&app_config) && app_config.auto_refresh,
                app_config.refresh_interval,
                now,
                last,
                is_refreshing(),
            ) {
                if let Some(guard) = try_acquire_refresh_lock() {
                    logger::log_info(&format!(
                        "[AutoRefresh] ⏰ Refresh interval elapsed ({} mins). Auto-refreshing all account quotas in background...",
                        app_config.refresh_interval
                    ));

                    let state_clone = proxy_state.clone();
                    let handle_clone = app_handle.clone();
                    let refresh_interval_mins = app_config.refresh_interval;

                    // 异步执行刷新，防止长时间阻塞主轮询 tick
                    tokio::spawn(async move {
                        let _task_guard = guard;
                        match crate::commands::refresh_all_quotas_internal(
                            &state_clone,
                            handle_clone,
                        )
                        .await
                        {
                            Ok(stats) => {
                                logger::log_info(&format!(
                                    "[AutoRefresh] ✅ Background auto-refresh completed: {}/{} success, {} failed",
                                    stats.success, stats.total, stats.failed
                                ));
                                record_refresh_timestamp(chrono::Utc::now().timestamp());
                            }
                            Err(e) => {
                                logger::log_warn(&format!(
                                    "[AutoRefresh] ❌ Background auto-refresh failed: {}. Will retry in 60s.",
                                    e
                                ));
                                // 失败时设置锚点为 60 秒后重试，避免短暂网络异常推迟整个长刷新周期
                                let interval_secs =
                                    (refresh_interval_mins.max(1) as i64).saturating_mul(60);
                                let now_ts = chrono::Utc::now().timestamp();
                                let retry_anchor =
                                    now_ts.saturating_sub(interval_secs).saturating_add(60);
                                record_refresh_timestamp(retry_anchor);
                            }
                        }
                    });
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_trigger_auto_refresh_disabled() {
        let now = 10000;
        let last = 9000; // 1000s ago
        assert!(!should_trigger_auto_refresh(false, 15, now, last, false));
    }

    #[test]
    fn test_should_trigger_auto_refresh_invalid_interval() {
        let now = 10000;
        let last = 9000;
        assert!(!should_trigger_auto_refresh(true, 0, now, last, false));
        assert!(!should_trigger_auto_refresh(true, -5, now, last, false));
    }

    #[test]
    fn test_should_trigger_auto_refresh_currently_refreshing() {
        let now = 10000;
        let last = 1000; // 9000s ago (> 15 mins)
        assert!(!should_trigger_auto_refresh(true, 15, now, last, true));
    }

    #[test]
    fn test_should_trigger_auto_refresh_interval_not_elapsed() {
        let now = 10000;
        let last = 10000 - 14 * 60; // 14 mins ago (< 15 mins)
        assert!(!should_trigger_auto_refresh(true, 15, now, last, false));
    }

    #[test]
    fn test_should_trigger_auto_refresh_interval_elapsed() {
        let now = 10000;
        let last = 10000 - 15 * 60; // Exactly 15 mins ago
        assert!(should_trigger_auto_refresh(true, 15, now, last, false));

        let last_older = 10000 - 20 * 60; // 20 mins ago
        assert!(should_trigger_auto_refresh(
            true, 15, now, last_older, false
        ));
    }

    #[test]
    fn test_record_and_get_refresh_timestamp() {
        let ts = 1731670000;
        record_refresh_timestamp(ts);
        assert_eq!(get_last_refresh_timestamp(), ts);
    }

    #[test]
    fn test_should_trigger_auto_refresh_disabled_with_elapsed_time() {
        let now = 10000;
        let last = 10000 - 3600; // 1 hour ago (> 15 mins)
        assert!(!should_trigger_auto_refresh(false, 15, now, last, false));
    }

    #[test]
    fn test_should_trigger_auto_refresh_boundary_condition() {
        let now = 10000;
        let interval_mins = 15;
        let interval_secs = 15 * 60; // 900s

        // Exact boundary - 1 sec (899s elapsed): must NOT trigger
        let last_899 = now - (interval_secs - 1);
        assert!(!should_trigger_auto_refresh(
            true,
            interval_mins,
            now,
            last_899,
            false
        ));

        // Exact boundary (900s elapsed): must trigger
        let last_900 = now - interval_secs;
        assert!(should_trigger_auto_refresh(
            true,
            interval_mins,
            now,
            last_900,
            false
        ));

        // Exact boundary + 1 sec (901s elapsed): must trigger
        let last_901 = now - (interval_secs + 1);
        assert!(should_trigger_auto_refresh(
            true,
            interval_mins,
            now,
            last_901,
            false
        ));
    }

    #[test]
    fn test_should_trigger_auto_refresh_minimum_valid_interval() {
        let now = 5000;
        let interval_mins = 1; // 60s
        assert!(!should_trigger_auto_refresh(
            true,
            interval_mins,
            now,
            now - 59,
            false
        ));
        assert!(should_trigger_auto_refresh(
            true,
            interval_mins,
            now,
            now - 60,
            false
        ));
        assert!(should_trigger_auto_refresh(
            true,
            interval_mins,
            now,
            now - 61,
            false
        ));
    }

    #[test]
    fn test_should_trigger_auto_refresh_clock_skew_backward() {
        let now = 10000;
        // Minor clock skew: last refresh is slightly in the future
        let last_future = 10005;
        assert!(!should_trigger_auto_refresh(
            true,
            15,
            now,
            last_future,
            false
        ));

        // Major clock skew: last refresh is far in the future
        let last_far_future = 13600;
        assert!(!should_trigger_auto_refresh(
            true,
            15,
            now,
            last_far_future,
            false
        ));

        // Negative now_ts relative to last_refresh
        assert!(!should_trigger_auto_refresh(true, 15, -1000, 1000, false));
    }

    #[test]
    fn test_should_trigger_auto_refresh_overflow_and_extremes() {
        // Extreme negative interval: i32::MIN must not panic and must return false
        assert!(!should_trigger_auto_refresh(
            true,
            i32::MIN,
            10000,
            9000,
            false
        ));

        // Extremely large interval: i32::MAX mins
        // (i32::MAX as i64) * 60 = 128_849_018_820 seconds
        let large_interval = i32::MAX;
        let now = 200_000_000_000_i64;
        let last_recent = now - 1000;
        assert!(!should_trigger_auto_refresh(
            true,
            large_interval,
            now,
            last_recent,
            false
        ));

        let last_ancient = 0_i64;
        assert!(should_trigger_auto_refresh(
            true,
            large_interval,
            now,
            last_ancient,
            false
        ));

        // Arithmetic saturation bounds
        assert!(should_trigger_auto_refresh(true, 15, i64::MAX, 0, false));
        assert!(!should_trigger_auto_refresh(true, 15, i64::MIN, 0, false));
    }

    #[test]
    fn test_is_refreshing_concurrency_cas_invariant() {
        // Ensure starting from clean state
        IS_REFRESHING.store(false, Ordering::SeqCst);
        assert!(!is_refreshing());

        // First lock acquisition must succeed
        let guard_first = try_acquire_refresh_lock();
        assert!(guard_first.is_some(), "First acquisition must succeed");
        assert!(is_refreshing(), "is_refreshing() must reflect active state");

        // Second competing acquisition while active must fail
        let guard_second = try_acquire_refresh_lock();
        assert!(
            guard_second.is_none(),
            "Second acquisition must fail while task is in flight"
        );

        // Drop first guard to release lock
        drop(guard_first);
        assert!(
            !is_refreshing(),
            "is_refreshing() must return to false after guard is dropped"
        );

        // Now acquisition can succeed again
        let guard_third = try_acquire_refresh_lock();
        assert!(
            guard_third.is_some(),
            "Lock can be acquired again after drop"
        );
    }

    #[test]
    fn test_timestamp_synchronization_with_trigger_decision() {
        let base_time = 1731670000_i64;
        record_refresh_timestamp(base_time);
        assert_eq!(get_last_refresh_timestamp(), base_time);

        let recorded = get_last_refresh_timestamp();
        let interval_mins = 15;
        let interval_secs = 15 * 60;

        // Sub-interval: not ready
        assert!(!should_trigger_auto_refresh(
            true,
            interval_mins,
            recorded + interval_secs - 1,
            recorded,
            false
        ));

        // Exact interval: ready
        assert!(should_trigger_auto_refresh(
            true,
            interval_mins,
            recorded + interval_secs,
            recorded,
            false
        ));

        // Advance timestamp to simulate completed cycle
        let next_refresh_time = recorded + interval_secs;
        record_refresh_timestamp(next_refresh_time);
        assert_eq!(get_last_refresh_timestamp(), next_refresh_time);

        // Same point in time is now not ready
        assert!(!should_trigger_auto_refresh(
            true,
            interval_mins,
            next_refresh_time,
            get_last_refresh_timestamp(),
            false
        ));
    }
}
