//! 应用启动后的异步全量配额刷新。
//!
//! 账号加载就绪后单进程触发一次，受 `startup_refresh` 开关控制；
//! 实际刷新由 `refresh_all_quotas_logic` 以滑动窗口执行，
//! 并发账号数受 `refresh_concurrency`（默认 5）控制，
//! 每个账号完成后按 `refresh_slide_delay_secs`（默认 0.2 秒）让位给下一个。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::modules::{account, config, logger};

/// 启动后给前端快照水合、账号索引加载留出的短暂等待
const STARTUP_DELAY: Duration = Duration::from_secs(1);
/// 等待账号索引就绪的最长时间
const ACCOUNT_READY_TIMEOUT: Duration = Duration::from_secs(60);
/// 账号索引就绪轮询间隔
const ACCOUNT_READY_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// 单进程只允许触发一次，避免代理重启、多窗口或重复 setup 造成多次全量刷新
static STARTUP_REFRESH_TRIGGERED: AtomicBool = AtomicBool::new(false);

/// 在后台等待账号加载就绪，然后异步刷新一次全部账号配额。
pub fn spawn_startup_quota_refresh(
    app_handle: Option<tauri::AppHandle>,
    proxy_state: crate::commands::proxy::ProxyServiceState,
) {
    if STARTUP_REFRESH_TRIGGERED.swap(true, Ordering::SeqCst) {
        return;
    }

    tauri::async_runtime::spawn(async move {
        let app_config = match config::load_app_config() {
            Ok(cfg) => cfg,
            Err(e) => {
                logger::log_warn(&format!(
                    "[StartupRefresh] Failed to load config, skipping startup quota refresh: {}",
                    e
                ));
                return;
            }
        };

        if !app_config.startup_refresh {
            logger::log_info("[StartupRefresh] Startup quota refresh is disabled by config");
            return;
        }

        tokio::time::sleep(STARTUP_DELAY).await;

        // 首次完整读取会触发旧版单文件索引的恢复/迁移；之后改用轻量探测轮询，避免日志与 I/O 放大
        let mut account_count = match account::load_account_index() {
            Ok(index) => index.accounts.len(),
            Err(e) => {
                logger::log_warn(&format!(
                    "[StartupRefresh] Account index not ready yet: {}",
                    e
                ));
                0
            }
        };

        if account_count == 0 {
            let deadline = tokio::time::Instant::now() + ACCOUNT_READY_TIMEOUT;
            loop {
                account_count = account::indexed_account_count();
                if account_count > 0 {
                    break;
                }

                if tokio::time::Instant::now() >= deadline {
                    logger::log_info(
                        "[StartupRefresh] No account became ready before timeout, skipping startup quota refresh",
                    );
                    return;
                }

                tokio::time::sleep(ACCOUNT_READY_POLL_INTERVAL).await;
            }
        }

        logger::log_info(&format!(
            "[StartupRefresh] {} accounts are ready, starting asynchronous refresh of all quotas...",
            account_count
        ));

        match crate::commands::refresh_all_quotas_internal(&proxy_state, app_handle).await {
            Ok(stats) => logger::log_info(&format!(
                "[StartupRefresh] Startup quota refresh completed: {}/{} success, {} failed",
                stats.success, stats.total, stats.failed
            )),
            Err(e) => logger::log_error(&format!(
                "[StartupRefresh] Startup quota refresh failed: {}",
                e
            )),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_delay_is_one_second() {
        assert_eq!(STARTUP_DELAY, Duration::from_secs(1));
    }
}
