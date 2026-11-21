pub mod auth;
pub mod device;
pub mod quota;
pub mod storage;
pub mod switcher;
pub mod types;

use anyhow::{anyhow, Result};
use std::sync::Arc;
pub use types::*;

/// Antigravity Manager 统一门面 API
#[derive(Clone)]
pub struct AntigravityManager {
    storage: Arc<storage::StorageManager>,
    oauth: Arc<auth::OAuthClient>,
    quota: Arc<quota::QuotaClient>,
}

impl AntigravityManager {
    pub fn new() -> Result<Self> {
        let storage = Arc::new(storage::StorageManager::new()?);
        let oauth = Arc::new(auth::OAuthClient::new());
        let quota = Arc::new(quota::QuotaClient::new());
        Ok(Self {
            storage,
            oauth,
            quota,
        })
    }

    /// 列出所有账号摘要
    pub fn list_accounts(&self) -> Result<Vec<AccountSummary>> {
        self.storage.list_account_summaries()
    }

    /// 获取单个账号完整实体 (含 Token 与配额)
    pub fn get_account(&self, id: &str) -> Result<Option<Account>> {
        self.storage.get_account(id)
    }

    /// 通过 ID/Email/前缀定位账号
    pub fn find_account(&self, query: &str) -> Result<Option<Account>> {
        self.storage.find_account(query)
    }

    /// 删除账号
    pub fn delete_account(&self, query: &str) -> Result<bool> {
        let acc = self.find_account(query)?.ok_or_else(|| anyhow!("Account not found: {}", query))?;
        self.storage.delete_account(&acc.id)
    }

    /// 刷新单个账号配额
    pub async fn refresh_account_quota(&self, account_id: &str) -> Result<QuotaSummary> {
        let mut account = self
            .storage
            .get_account(account_id)?
            .ok_or_else(|| anyhow!("Account not found: {}", account_id))?;

        // 1. 先使用 refresh_token 换取有效 access_token
        let refreshed_token = self
            .oauth
            .refresh_access_token(&account.token.refresh_token)
            .await?;
        account.token = refreshed_token;

        // 2. 查询最新配额
        let quota = self.quota.fetch_quota(&account.token.access_token).await?;
        account.quota = Some(quota.clone());
        account.last_used = Some(chrono::Utc::now().timestamp());

        // 3. 回写持久化
        self.storage.save_account(&account)?;
        Ok(quota)
    }

    /// 批量并发刷新所有账号配额
    pub async fn refresh_all_quotas(&self) -> Result<Vec<(String, Result<QuotaSummary>)>> {
        let summaries = self.storage.list_account_summaries()?;
        let mut results = Vec::new();

        for s in summaries {
            let res = self.refresh_account_quota(&s.id).await;
            results.push((s.email, res));
        }

        Ok(results)
    }

    /// 执行热切换
    pub fn switch_account(&self, query: &str, target: TargetHost, restart: bool) -> Result<Account> {
        let mut account = self
            .find_account(query)?
            .ok_or_else(|| anyhow!("Account not found matching query: {}", query))?;

        // 确保有设备指纹隔离配置，若无则自动生成并持久化
        if account.device_profile.is_none() {
            let profile = device::generate_device_profile();
            account.device_profile = Some(profile);
            let _ = self.storage.save_account(&account);
        }

        // 执行切换
        switcher::SwitchEngine::switch(&account, target, restart)?;

        // 更新激活记录
        let target_str = match target {
            TargetHost::App => "app",
            TargetHost::Ide => "ide",
            TargetHost::Plugin => "plugin",
            TargetHost::Agy => "agy",
            TargetHost::All => "all",
        };
        let _ = self.storage.set_active_account_id(target_str, &account.id);

        Ok(account)
    }

    /// 获取各宿主激活状态
    pub fn get_active_hosts(&self) -> ActiveHostStatus {
        switcher::SwitchEngine::detect_active_hosts()
    }

    /// 通过已有 refresh_token 手工添加账号
    pub async fn add_account_by_token(&self, refresh_token: &str, tags: Vec<String>) -> Result<Account> {
        let token_info = self.oauth.refresh_access_token(refresh_token).await?;
        let user_info = self.oauth.fetch_user_info(&token_info.access_token).await?;

        let id = uuid::Uuid::new_v4().to_string();
        let device_profile = device::generate_device_profile();

        let account = Account {
            id: id.clone(),
            email: user_info.email,
            name: user_info.name,
            priority: Some(0),
            token: token_info,
            device_profile: Some(device_profile),
            quota: None,
            tags,
            disabled: false,
            disabled_reason: None,
            banned: false,
            created_at: Some(chrono::Utc::now().timestamp()),
            last_used: None,
        };

        self.storage.save_account(&account)?;

        // 立即尝试刷新一次初始配额
        let _ = self.refresh_account_quota(&id).await;

        self.storage.get_account(&id)?.ok_or_else(|| anyhow!("Failed to load saved account"))
    }
}
