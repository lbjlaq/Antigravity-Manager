use crate::auth::OAuthClient;
use crate::config::AppConfig;
use crate::quota::QuotaClient;
use crate::storage::StorageManager;
use crate::strategy::{AccountSelectionFilter, ModelPoolPreference, SwitchStrategy};
use crate::switcher::SwitchEngine;
use crate::types::{Account, TargetHost};
use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tracing::{info, warn};

/// 后台智能守护服务 (负责 AT 惰性保鲜、配额轮询与耗尽自动切号)
pub struct CoreDaemonService {
    storage: Arc<StorageManager>,
    oauth: Arc<OAuthClient>,
    quota_client: Arc<QuotaClient>,
    running: Arc<AtomicBool>,
    wakeup: Arc<Notify>,
}

impl CoreDaemonService {
    pub fn new() -> Result<Self> {
        let storage = Arc::new(StorageManager::new()?);
        let oauth = Arc::new(OAuthClient::new());
        let quota_client = Arc::new(QuotaClient::new());
        Ok(Self::from_shared(storage, oauth, quota_client))
    }

    /// 复用主管理器已有的存储、OAuth 与配额客户端实例，避免在进程内维护多份重复状态
    pub fn from_shared(
        storage: Arc<StorageManager>,
        oauth: Arc<OAuthClient>,
        quota_client: Arc<QuotaClient>,
    ) -> Self {
        Self {
            storage,
            oauth,
            quota_client,
            running: Arc::new(AtomicBool::new(false)),
            wakeup: Arc::new(Notify::new()),
        }
    }

    /// 返回配置变更唤醒句柄（保存设置后调用 `notify_one()` 立即重算调度周期，无需 60s 忙等轮询）
    pub fn wakeup_handle(&self) -> Arc<Notify> {
        Arc::clone(&self.wakeup)
    }

    /// 惰性令牌刷新: 只有当 Access Token 距离过期不足 5 分钟 (300秒) 时才发起 Google OAuth 刷新
    pub async fn ensure_valid_token(
        account: &mut Account,
        storage: &StorageManager,
        oauth: &OAuthClient,
    ) -> Result<bool> {
        let now = chrono::Utc::now().timestamp();
        let expiry = account.token.expiry_timestamp.unwrap_or(0);

        // 如果距离过期还有 5 分钟以上且 access_token 不为空，无需刷新
        if !account.token.access_token.trim().is_empty() && (expiry - now) > 300 {
            return Ok(false);
        }

        // 需要刷新 AT
        if account.token.refresh_token.trim().is_empty() {
            anyhow::bail!("Account {} has no refresh_token", account.email);
        }

        info!(
            "Access token expired or expiring soon for {}, refreshing...",
            account.email
        );
        match oauth
            .refresh_access_token(&account.token.refresh_token)
            .await
        {
            Ok(new_token) => {
                account.token.access_token = new_token.access_token;
                account.token.expires_in = new_token.expires_in;
                account.token.expiry_timestamp = new_token.expiry_timestamp;
                if new_token.token_type.is_some() {
                    account.token.token_type = new_token.token_type;
                }
                if new_token.id_token.is_some() {
                    account.token.id_token = new_token.id_token;
                }
                if !new_token.refresh_token.trim().is_empty() {
                    account.token.refresh_token = new_token.refresh_token;
                }
                storage.save_account(account)?;
                info!("Successfully refreshed access token for {}", account.email);
                Ok(true)
            }
            Err(e) => {
                warn!(
                    "Failed to refresh access token for {}: {}",
                    account.email, e
                );
                Err(e)
            }
        }
    }

    /// 执行单次健康检查与智能轮转检查 (供定时器或事件触发)
    pub async fn check_and_auto_rotate(&self) -> Result<()> {
        let app_cfg = AppConfig::load();
        let cfg = &app_cfg.auto_switch;
        if !cfg.enabled {
            return Ok(());
        }

        let threshold_pct = cfg.effective_exhaustion_threshold_pct();
        let storage_for_hosts = Arc::clone(&self.storage);
        let active_hosts = tokio::task::spawn_blocking(move || {
            crate::AntigravityManager::resolve_active_hosts_with_storage(&storage_for_hosts)
        })
        .await
        .unwrap_or_else(|_| SwitchEngine::detect_active_hosts());
        let full_accounts = self.storage.list_full_accounts()?;

        for target in &cfg.targets {
            let is_running = match target {
                TargetHost::App => active_hosts.app_running,
                TargetHost::Ide => active_hosts.ide_running,
                TargetHost::Plugin => active_hosts.plugin_running,
                _ => true,
            };

            if !is_running {
                continue;
            }

            let current_acc = match target {
                TargetHost::App => active_hosts.app_account.as_ref(),
                TargetHost::Ide => active_hosts.ide_account.as_ref(),
                TargetHost::Plugin => active_hosts.plugin_account.as_ref(),
                TargetHost::Agy => active_hosts.agy_account.as_ref(),
                TargetHost::All => None,
            };

            let Some(current_email) = current_acc else {
                continue;
            };

            // 查找当前使用的账号对象
            let current = full_accounts
                .iter()
                .find(|a| a.email.eq_ignore_ascii_case(current_email));
            let Some(current_account) = current else {
                continue;
            };

            // 检查额度是否低于阈值 (统一 0.0 ~ 100.0 百分制)
            let mut is_exhausted = false;
            if let Some(quota) = &current_account.quota {
                let pool = cfg.pool.to_lowercase();
                let pct = if pool == "claude" {
                    quota.claude_percentage.unwrap_or(0.0)
                } else if pool == "gemini" {
                    quota.gemini_percentage.unwrap_or(0.0)
                } else {
                    quota
                        .gemini_percentage
                        .unwrap_or(0.0)
                        .min(quota.claude_percentage.unwrap_or(0.0))
                };

                if pct <= threshold_pct {
                    is_exhausted = true;
                }
            }

            if is_exhausted {
                info!(
                    "Target {:?} active account {} quota exhausted (<= {:.1}%), selecting replacement...",
                    target, current_email, threshold_pct
                );

                let strategy_type: SwitchStrategy =
                    cfg.strategy.parse().unwrap_or(SwitchStrategy::MaxQuota);
                let pool_type: ModelPoolPreference =
                    cfg.pool.parse().unwrap_or(ModelPoolPreference::Balanced);

                let filter = AccountSelectionFilter {
                    strategy: Some(strategy_type),
                    prefer_pool: Some(pool_type),
                    required_tag: None,
                    healthy_only: true,
                    min_quota_percentage: Some((threshold_pct + 5.0).min(95.0)),
                };

                let manager = crate::AntigravityManager::from_arcs(
                    Arc::clone(&self.storage),
                    Arc::clone(&self.oauth),
                    Arc::clone(&self.quota_client),
                );
                if let Some(mut next_account) = manager.pick_best_account(filter)? {
                    if next_account.id != current_account.id {
                        info!(
                            "Auto-switching target {:?} to {}",
                            target, next_account.email
                        );
                        let _ =
                            Self::ensure_valid_token(&mut next_account, &self.storage, &self.oauth)
                                .await;
                        let acc_clone = next_account.clone();
                        let target_copy = *target;
                        tokio::task::spawn_blocking(move || {
                            SwitchEngine::switch(&acc_clone, target_copy, false)
                        })
                        .await
                        .map_err(|e| anyhow::anyhow!("Switch join error: {}", e))??;
                        SwitchEngine::invalidate_active_hosts_cache();
                        let target_str = match target {
                            TargetHost::App => "app",
                            TargetHost::Ide => "ide",
                            TargetHost::Plugin => "plugin",
                            TargetHost::Agy => "agy",
                            TargetHost::All => "all",
                        };
                        let _ = self
                            .storage
                            .set_active_account_id(target_str, &next_account.id);

                        if cfg.desktop_notification {
                            Self::send_desktop_notification(
                                "Antigravity 自动切号",
                                &format!(
                                    "应用 {:?} 额度耗尽，已自动切至 {}",
                                    target, next_account.email
                                ),
                            );
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// 发送跨平台系统原生通知 (macOS osascript / Windows toast / Linux notify-send)
    pub fn send_desktop_notification(title: &str, body: &str) {
        #[cfg(target_os = "macos")]
        {
            let escape_applescript = |s: &str| {
                s.replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\r', " ")
                    .replace('\n', " ")
            };
            let script = format!(
                "display notification \"{}\" with title \"{}\"",
                escape_applescript(body),
                escape_applescript(title)
            );
            let mut cmd = std::process::Command::new("osascript");
            cmd.arg("-e").arg(script);
            crate::runtime::spawn_and_reap(cmd);
        }

        #[cfg(target_os = "windows")]
        {
            let escape_xml = |s: &str| {
                s.replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
                    .replace('"', "&quot;")
                    .replace('\'', "&apos;")
            };
            let xml = format!(
                "<toast><visual><binding template=\"ToastText02\"><text id=\"1\">{}</text><text id=\"2\">{}</text></binding></visual></toast>",
                escape_xml(title),
                escape_xml(body)
            );
            let ps_script = format!(
                "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
                 $doc = New-Object Windows.Data.Xml.Dom.XmlDocument; \
                 $doc.LoadXml('{}'); \
                 $toast = New-Object Windows.UI.Notifications.ToastNotification $doc; \
                 [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Antigravity Manager').Show($toast)",
                xml.replace('\'', "''")
            );
            let mut cmd = std::process::Command::new("powershell");
            cmd.args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &ps_script,
            ]);
            crate::runtime::spawn_and_reap(cmd);
        }

        #[cfg(target_os = "linux")]
        {
            let mut cmd = std::process::Command::new("notify-send");
            cmd.arg("--").arg(title).arg(body);
            crate::runtime::spawn_and_reap(cmd);
        }
    }

    /// 启动后台事件驱动循环：
    /// - 当配额轮询与自动换号均关闭时彻底挂起等待 `wakeup` 通知（0 定时唤醒，允许 macOS App Nap / 笔记本深度节能）
    /// - 开启任一功能时按周期精准睡眠，并在刷新后执行耗尽自动换号检查
    pub async fn run_loop(&self) {
        self.running.store(true, Ordering::SeqCst);

        while self.running.load(Ordering::SeqCst) {
            let cfg = AppConfig::load();
            if !cfg.quota_polling.enabled && !cfg.auto_switch.enabled {
                self.wakeup.notified().await;
                continue;
            }

            let wait_minutes = if cfg.quota_polling.enabled {
                cfg.quota_polling.interval_minutes.max(5)
            } else {
                5
            };
            let wait_secs = wait_minutes.saturating_mul(60);

            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(wait_secs)) => {
                    if !self.running.load(Ordering::SeqCst) {
                        break;
                    }
                    let latest = AppConfig::load();
                    if latest.quota_polling.enabled || latest.auto_switch.enabled {
                        let force_active_only = !latest.quota_polling.enabled && latest.auto_switch.enabled;
                        let _ = self.poll_quotas_internal(force_active_only).await;
                    }
                    if latest.auto_switch.enabled {
                        let _ = self.check_and_auto_rotate().await;
                    }
                }
                _ = self.wakeup.notified() => {
                    // 配置更新或收到停止信号，立刻重新评估状态
                }
            }
        }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        self.wakeup.notify_one();
    }

    /// 执行后台配额静默轮询
    pub async fn poll_quotas(&self) -> Result<()> {
        self.poll_quotas_internal(false).await
    }

    async fn poll_quotas_internal(&self, force_active_only: bool) -> Result<()> {
        let cfg = AppConfig::load();
        let active_only = force_active_only || cfg.quota_polling.active_only;
        let full_accounts = self.storage.list_full_accounts()?;
        let active_hosts = if active_only {
            let storage_for_hosts = Arc::clone(&self.storage);
            Some(
                tokio::task::spawn_blocking(move || {
                    crate::AntigravityManager::resolve_active_hosts_with_storage(&storage_for_hosts)
                })
                .await
                .unwrap_or_else(|_| SwitchEngine::detect_active_hosts()),
            )
        } else {
            None
        };

        let mut polled_idx = 0usize;
        for mut acc in full_accounts {
            if acc.disabled || acc.banned {
                continue;
            }

            // 如果设置了 active_only，且不在当前宿主激活账号中，则跳过
            if let Some(ref hosts) = active_hosts {
                let is_active = [
                    hosts.app_account.as_deref(),
                    hosts.ide_account.as_deref(),
                    hosts.plugin_account.as_deref(),
                    hosts.agy_account.as_deref(),
                ]
                .iter()
                .filter_map(|x| *x)
                .any(|email| email.eq_ignore_ascii_case(&acc.email));

                if !is_active {
                    continue;
                }
            }

            if polled_idx > 0 {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            polled_idx += 1;

            // 惰性确保 AT 有效；若刷新失败则跳过后续空/过期 AT 请求
            if Self::ensure_valid_token(&mut acc, &self.storage, &self.oauth)
                .await
                .is_err()
            {
                continue;
            }

            let (fetched_pid, fetched_tier, fetched_raw_tier) = self
                .quota_client
                .fetch_project_and_tier(&acc.token.access_token)
                .await;
            let pid = acc.token.project_id.clone().or(fetched_pid);

            if let crate::quota::QuotaFetchResult::Success(mut summary) = self
                .quota_client
                .fetch_quota_safe(&acc.token.access_token, pid.as_deref())
                .await
            {
                if summary.subscription_tier.is_none() {
                    summary.subscription_tier = fetched_tier
                        .or_else(|| acc.quota.as_ref().and_then(|q| q.subscription_tier.clone()));
                }
                if summary.raw_tier.is_none() {
                    summary.raw_tier = fetched_raw_tier
                        .or_else(|| acc.quota.as_ref().and_then(|q| q.raw_tier.clone()));
                }
                acc.quota = Some(summary);
                acc.token.project_id = pid;
                let _ = self.storage.save_account(&acc);
            }
        }

        Ok(())
    }
}
