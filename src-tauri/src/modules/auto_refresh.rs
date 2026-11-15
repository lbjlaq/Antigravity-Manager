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

        loop {
            interval.tick().await;

            let app_config = match config::load_app_config() {
                Ok(cfg) => cfg,
                Err(e) => {
                    logger::log_warn(&format!(
                        "[AutoRefresh] Failed to load config in scheduler tick: {}",
                        e
                    ));
                    continue;
                }
            };

            let now = chrono::Utc::now().timestamp();
            let last = get_last_refresh_timestamp();

            if should_trigger_auto_refresh(
                app_config.auto_refresh,
                app_config.refresh_interval,
                now,
                last,
                is_refreshing(),
            ) {
                if IS_REFRESHING
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    logger::log_info(&format!(
                        "[AutoRefresh] ⏰ Refresh interval elapsed ({} mins). Auto-refreshing all account quotas in background...",
                        app_config.refresh_interval
                    ));

                    let state_clone = proxy_state.clone();
                    let handle_clone = app_handle.clone();

                    // 异步执行刷新，防止长时间阻塞主轮询 tick
                    tokio::spawn(async move {
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
                            }
                            Err(e) => {
                                logger::log_warn(&format!(
                                    "[AutoRefresh] ❌ Background auto-refresh failed: {}",
                                    e
                                ));
                            }
                        }
                        record_refresh_timestamp(chrono::Utc::now().timestamp());
                        IS_REFRESHING.store(false, Ordering::SeqCst);
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
}
