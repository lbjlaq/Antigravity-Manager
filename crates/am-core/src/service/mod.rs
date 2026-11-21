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
use tracing::{info, warn};

/// 后台智能守护服务 (负责 AT 惰性保鲜、配额轮询与耗尽自动切号)
pub struct CoreDaemonService {
    storage: Arc<StorageManager>,
    oauth: Arc<OAuthClient>,
    quota_client: Arc<QuotaClient>,
    config: Arc<AppConfig>,
    running: Arc<AtomicBool>,
}

impl CoreDaemonService {
    pub fn new() -> Result<Self> {
        let storage = Arc::new(StorageManager::new()?);
        let oauth = Arc::new(OAuthClient::new());
        let quota_client = Arc::new(QuotaClient::new());
        let config = Arc::new(AppConfig::load());
        let running = Arc::new(AtomicBool::new(false));

        Ok(Self {
            storage,
            oauth,
            quota_client,
            config,
            running,
        })
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
        if !account.token.access_token.is_empty() && (expiry - now) > 300 {
            return Ok(false);
        }

        // 需要刷新 AT
        if account.token.refresh_token.is_empty() {
            anyhow::bail!("Account {} has no refresh_token", account.email);
        }

        info!("Access token expired or expiring soon for {}, refreshing...", account.email);
        match oauth.refresh_access_token(&account.token.refresh_token).await {
            Ok(new_token) => {
                account.token.access_token = new_token.access_token;
                account.token.expires_in = new_token.expires_in;
                account.token.expiry_timestamp = new_token.expiry_timestamp;
                storage.save_account(account)?;
                info!("Successfully refreshed access token for {}", account.email);
                Ok(true)
            }
            Err(e) => {
                warn!("Failed to refresh access token for {}: {}", account.email, e);
                Err(e)
            }
        }
    }

    /// 执行单次健康检查与智能轮转检查 (供定时器或事件触发)
    pub async fn check_and_auto_rotate(&self) -> Result<()> {
        let cfg = &self.config.auto_switch;
        if !cfg.enabled {
            return Ok(());
        }

        let active_hosts = SwitchEngine::detect_active_hosts();
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
            let current = full_accounts.iter().find(|a| a.email.eq_ignore_ascii_case(current_email));
            let Some(current_account) = current else {
                continue;
            };

            // 检查额度是否低于阈值
            let mut is_exhausted = false;
            if let Some(quota) = &current_account.quota {
                let pool = cfg.pool.to_lowercase();
                let pct = if pool == "claude" {
                    quota.claude_percentage.unwrap_or(0.0)
                } else if pool == "gemini" {
                    quota.gemini_percentage.unwrap_or(0.0)
                } else {
                    quota.gemini_percentage.unwrap_or(0.0).min(quota.claude_percentage.unwrap_or(0.0))
                };

                if pct <= cfg.trigger_exhaustion_threshold {
                    is_exhausted = true;
                }
            }

            if is_exhausted {
                info!(
                    "Target {:?} active account {} quota exhausted (<= {}), selecting replacement...",
                    target, current_email, cfg.trigger_exhaustion_threshold
                );

                let strategy_type: SwitchStrategy = cfg.strategy.parse().unwrap_or(SwitchStrategy::MaxQuota);
                let pool_type: ModelPoolPreference = cfg.pool.parse().unwrap_or(ModelPoolPreference::Balanced);

                let filter = AccountSelectionFilter {
                    strategy: Some(strategy_type),
                    prefer_pool: Some(pool_type),
                    required_tag: None,
                    healthy_only: true,
                    min_quota_percentage: Some(cfg.trigger_exhaustion_threshold + 0.05),
                };

                let manager = crate::AntigravityManager::new()?;
                if let Some(next_account) = manager.pick_best_account(filter)? {
                    if next_account.id != current_account.id {
                        info!("Auto-switching target {:?} to {}", target, next_account.email);
                        SwitchEngine::switch(&next_account, *target, false)?;
                        let target_str = match target {
                            TargetHost::App => "app",
                            TargetHost::Ide => "ide",
                            TargetHost::Plugin => "plugin",
                            TargetHost::Agy => "agy",
                            TargetHost::All => "all",
                        };
                        let _ = self.storage.set_active_account_id(target_str, &next_account.id);

                        if cfg.desktop_notification {
                            Self::send_desktop_notification(
                                "Antigravity 自动切号",
                                &format!("应用 {:?} 额度耗尽，已自动切至 {}", target, next_account.email),
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
            let script = format!(
                "display notification \"{}\" with title \"{}\"",
                body.replace('"', "\\\""),
                title.replace('"', "\\\"")
            );
            let _ = std::process::Command::new("osascript")
                .arg("-e")
                .arg(script)
                .spawn();
        }

        #[cfg(target_os = "windows")]
        {
            let ps_script = format!(
                "[reflection.assembly]::loadwithpartialname('System.Windows.Forms'); [System.Windows.Forms.MessageBox]::Show('{}', '{}')",
                body.replace('\'', "''"),
                title.replace('\'', "''")
            );
            let _ = std::process::Command::new("powershell")
                .arg("-Command")
                .arg(ps_script)
                .spawn();
        }

        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("notify-send")
                .arg(title)
                .arg(body)
                .spawn();
        }
    }

    /// 启动后台运行循环
    pub async fn run_loop(&self) {
        self.running.store(true, Ordering::SeqCst);
        let mut tick = 0u64;

        while self.running.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_secs(60)).await;
            tick += 1;

            let quota_interval = self.config.quota_polling.interval_minutes.max(5);

            // 1. 每隔 quota_interval 分钟执行一次额度轮询
            if self.config.quota_polling.enabled && (tick % quota_interval == 0) {
                let _ = self.poll_quotas().await;
            }

            // 2. 每 1 分钟检测一次是否需要自动故障切换
            if self.config.auto_switch.enabled {
                let _ = self.check_and_auto_rotate().await;
            }
        }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// 执行后台配额静默轮询
    pub async fn poll_quotas(&self) -> Result<()> {
        let full_accounts = self.storage.list_full_accounts()?;
        let active_hosts = SwitchEngine::detect_active_hosts();

        for mut acc in full_accounts {
            if acc.disabled || acc.banned {
                continue;
            }

            // 如果设置了 active_only，且不在当前运行应用中，则跳过
            if self.config.quota_polling.active_only {
                let is_active = [
                    active_hosts.app_account.as_deref(),
                    active_hosts.ide_account.as_deref(),
                    active_hosts.plugin_account.as_deref(),
                    active_hosts.agy_account.as_deref(),
                ]
                .iter()
                .filter_map(|x| *x)
                .any(|email| email.eq_ignore_ascii_case(&acc.email));

                if !is_active {
                    continue;
                }
            }

            // 惰性确保 AT 有效
            let _ = Self::ensure_valid_token(&mut acc, &self.storage, &self.oauth).await;

            let pid = match &acc.token.project_id {
                Some(p) => Some(p.clone()),
                None => self.quota_client.fetch_project_id(&acc.token.access_token).await,
            };

            if let crate::quota::QuotaFetchResult::Success(summary) = self
                .quota_client
                .fetch_quota_safe(&acc.token.access_token, pid.as_deref())
                .await
            {
                acc.quota = Some(summary);
                acc.token.project_id = pid;
                let _ = self.storage.save_account(&acc);
            }
        }

        Ok(())
    }
}
