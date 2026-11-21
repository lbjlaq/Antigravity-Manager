pub mod auth;
pub mod config;
pub mod device;
pub mod quota;
pub mod service;
pub mod stats;
pub mod storage;
pub mod strategy;
pub mod switcher;
pub mod types;

use anyhow::{anyhow, Result};
use std::sync::Arc;
pub use config::*;
pub use quota::QuotaFetchResult;
pub use service::*;
pub use stats::*;
pub use strategy::*;
pub use types::*;

/// 默认配额缓存保鲜时长 (2 小时)
pub const DEFAULT_QUOTA_TTL_SECS: i64 = 2 * 3600;

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

    /// 列出所有账号完整实体
    pub fn list_full_accounts(&self) -> Result<Vec<Account>> {
        self.storage.list_full_accounts()
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

    /// 手动禁用账号
    pub fn disable_account(&self, query: &str, reason: Option<&str>) -> Result<Account> {
        let mut acc = self.find_account(query)?.ok_or_else(|| anyhow!("Account not found: {}", query))?;
        acc.disabled = true;
        acc.disabled_reason = reason.map(String::from).or_else(|| Some("Manually disabled".to_string()));
        self.storage.save_account(&acc)?;
        Ok(acc)
    }

    /// 手动启用账号
    pub fn enable_account(&self, query: &str) -> Result<Account> {
        let mut acc = self.find_account(query)?.ok_or_else(|| anyhow!("Account not found: {}", query))?;
        acc.disabled = false;
        acc.disabled_reason = None;
        self.storage.save_account(&acc)?;
        Ok(acc)
    }

    /// 标签管理：添加标签到账号
    pub fn add_account_tags(&self, query: &str, tags_to_add: &[String]) -> Result<Account> {
        let mut acc = self.find_account(query)?.ok_or_else(|| anyhow!("Account not found: {}", query))?;
        for tag in tags_to_add {
            let clean = tag.trim();
            if clean.is_empty() {
                continue;
            }
            if clean.chars().count() > 15 {
                return Err(anyhow!("Tag '{}' exceeds maximum length of 15 characters", clean));
            }
            if !acc.tags.iter().any(|t| t.eq_ignore_ascii_case(clean)) {
                if acc.tags.len() >= 8 {
                    return Err(anyhow!("Account already has maximum limit of 8 tags"));
                }
                acc.tags.push(clean.to_string());
            }
        }
        self.storage.save_account(&acc)?;
        Ok(acc)
    }

    /// 标签管理：移除账号中的标签
    pub fn remove_account_tags(&self, query: &str, tags_to_remove: &[String]) -> Result<Account> {
        let mut acc = self.find_account(query)?.ok_or_else(|| anyhow!("Account not found: {}", query))?;
        acc.tags.retain(|t| !tags_to_remove.iter().any(|r| r.trim().eq_ignore_ascii_case(t)));
        self.storage.save_account(&acc)?;
        Ok(acc)
    }

    /// 标签管理：直接设置完整标签列表
    pub fn set_account_tags(&self, query: &str, new_tags: &[String]) -> Result<Account> {
        let mut acc = self.find_account(query)?.ok_or_else(|| anyhow!("Account not found: {}", query))?;
        let mut clean_tags = Vec::new();
        for tag in new_tags {
            let clean = tag.trim();
            if clean.is_empty() {
                continue;
            }
            if clean.chars().count() > 15 {
                return Err(anyhow!("Tag '{}' exceeds maximum length of 15 characters", clean));
            }
            if !clean_tags.iter().any(|t: &String| t.eq_ignore_ascii_case(clean)) {
                if clean_tags.len() >= 8 {
                    return Err(anyhow!("Maximum limit of 8 tags exceeded"));
                }
                clean_tags.push(clean.to_string());
            }
        }
        acc.tags = clean_tags;
        self.storage.save_account(&acc)?;
        Ok(acc)
    }

    /// 账号过滤与排序
    pub fn query_accounts(
        &self,
        filter_tag: Option<&str>,
        healthy_only: bool,
        sort_order: Option<AccountSortOrder>,
    ) -> Result<Vec<Account>> {
        let mut accounts = self.storage.list_full_accounts()?;

        // 1. 过滤
        accounts.retain(|acc| {
            if healthy_only && (acc.disabled || acc.banned) {
                return false;
            }
            if let Some(tag) = filter_tag {
                let tag_lower = tag.to_lowercase();
                if !acc.tags.iter().any(|t| t.to_lowercase() == tag_lower) {
                    return false;
                }
            }
            true
        });

        // 2. 排序
        if let Some(order) = sort_order {
            match order {
                AccountSortOrder::QuotaDesc => {
                    accounts.sort_by(|a, b| {
                        let qa = Self::calculate_account_effective_quota(a, ModelPoolPreference::Balanced);
                        let qb = Self::calculate_account_effective_quota(b, ModelPoolPreference::Balanced);
                        qb.partial_cmp(&qa).unwrap_or(std::cmp::Ordering::Equal)
                    });
                }
                AccountSortOrder::QuotaAsc => {
                    accounts.sort_by(|a, b| {
                        let qa = Self::calculate_account_effective_quota(a, ModelPoolPreference::Balanced);
                        let qb = Self::calculate_account_effective_quota(b, ModelPoolPreference::Balanced);
                        qa.partial_cmp(&qb).unwrap_or(std::cmp::Ordering::Equal)
                    });
                }
                AccountSortOrder::PriorityDesc => {
                    accounts.sort_by(|a, b| b.priority.unwrap_or(0).cmp(&a.priority.unwrap_or(0)));
                }
                AccountSortOrder::LastUsedAsc => {
                    accounts.sort_by_key(|a| a.last_used.unwrap_or(0));
                }
                AccountSortOrder::LastUsedDesc => {
                    accounts.sort_by_key(|a| std::cmp::Reverse(a.last_used.unwrap_or(0)));
                }
                AccountSortOrder::CreatedAtDesc => {
                    accounts.sort_by_key(|a| std::cmp::Reverse(a.created_at.unwrap_or(0)));
                }
                AccountSortOrder::EmailAsc => {
                    accounts.sort_by(|a, b| a.email.cmp(&b.email));
                }
            }
        }

        Ok(accounts)
    }

    /// 计算账号在特定偏好下的有效额度评分 (0.0 ~ 100.0)
    pub fn calculate_account_effective_quota(acc: &Account, prefer: ModelPoolPreference) -> f64 {
        let quota = match &acc.quota {
            Some(q) => q,
            None => return 0.0,
        };

        match prefer {
            ModelPoolPreference::Claude => quota.claude_percentage.unwrap_or(0.0),
            ModelPoolPreference::Gemini => quota.gemini_percentage.unwrap_or(0.0),
            ModelPoolPreference::Balanced => {
                let c = quota.claude_percentage.unwrap_or(0.0);
                let g = quota.gemini_percentage.unwrap_or(0.0);
                if quota.claude_percentage.is_some() && quota.gemini_percentage.is_some() {
                    (c * 0.5) + (g * 0.5)
                } else {
                    c.max(g)
                }
            }
        }
    }

    /// 智能选号 (Smart Account Picker)
    pub fn pick_best_account(&self, filter: AccountSelectionFilter) -> Result<Option<Account>> {
        let mut candidates = self.storage.list_full_accounts()?;
        let prefer = filter.prefer_pool.unwrap_or(ModelPoolPreference::Balanced);
        let strategy = filter.strategy.unwrap_or(SwitchStrategy::MaxQuota);

        candidates.retain(|acc| {
            if filter.healthy_only && (acc.disabled || acc.banned) {
                return false;
            }
            if let Some(tag) = &filter.required_tag {
                let tag_lower = tag.to_lowercase();
                if !acc.tags.iter().any(|t| t.to_lowercase() == tag_lower) {
                    return false;
                }
            }
            if let Some(min_pct) = filter.min_quota_percentage {
                let eff = Self::calculate_account_effective_quota(acc, prefer);
                if eff < min_pct {
                    return false;
                }
            }
            true
        });

        if candidates.is_empty() {
            return Ok(None);
        }

        match strategy {
            SwitchStrategy::MaxQuota => {
                candidates.sort_by(|a, b| {
                    let qa = Self::calculate_account_effective_quota(a, prefer);
                    let qb = Self::calculate_account_effective_quota(b, prefer);
                    qb.partial_cmp(&qa).unwrap_or(std::cmp::Ordering::Equal)
                });
                Ok(candidates.into_iter().next())
            }
            SwitchStrategy::LeastUsed => {
                candidates.sort_by_key(|a| a.last_used.unwrap_or(0));
                Ok(candidates.into_iter().next())
            }
            SwitchStrategy::Priority => {
                candidates.sort_by(|a, b| b.priority.unwrap_or(0).cmp(&a.priority.unwrap_or(0)));
                Ok(candidates.into_iter().next())
            }
            SwitchStrategy::Random => {
                use rand::seq::SliceRandom;
                let mut rng = rand::thread_rng();
                Ok(candidates.choose(&mut rng).cloned())
            }
        }
    }

    /// 刷新单个账号配额 (支持失败模式智能诊断与自愈标记)
    pub async fn refresh_account_quota(&self, account_id: &str) -> Result<QuotaSummary> {
        let mut account = self
            .storage
            .get_account(account_id)?
            .ok_or_else(|| anyhow!("Account not found: {}", account_id))?;

        // 1. 先使用 refresh_token 换取有效 access_token
        let refreshed_token = match self.oauth.refresh_access_token(&account.token.refresh_token).await {
            Ok(tok) => tok,
            Err(e) => {
                // 如果 refresh_token 换取直接报错 invalid_grant，自动打标禁用
                let err_str = e.to_string();
                if err_str.contains("invalid_grant") || err_str.contains("revoked") {
                    account.disabled = true;
                    account.disabled_reason = Some("Refresh token revoked or expired".to_string());
                    let _ = self.storage.save_account(&account);
                }
                return Err(e);
            }
        };
        account.token = refreshed_token;

        // 2. 检查或自动发现 Project ID
        if account.token.project_id.is_none() {
            if let Some(pid) = self.quota.fetch_project_id(&account.token.access_token).await {
                account.token.project_id = Some(pid);
            }
        }

        // 3. 查询最新配额并自愈
        let fetch_res = self
            .quota
            .fetch_quota_safe(&account.token.access_token, account.token.project_id.as_deref())
            .await;

        match fetch_res {
            QuotaFetchResult::Success(quota) => {
                account.quota = Some(quota.clone());
                // 成功时解除以往的临时错误标记
                if account.disabled && account.disabled_reason.as_deref().unwrap_or("").contains("403") {
                    account.disabled = false;
                    account.disabled_reason = None;
                }
                self.storage.save_account(&account)?;
                Ok(quota)
            }
            QuotaFetchResult::NeedDisable { reason } => {
                account.disabled = true;
                account.disabled_reason = Some(reason.clone());
                self.storage.save_account(&account)?;
                Err(anyhow!("Account Disabled: {}", reason))
            }
            QuotaFetchResult::TransientError(msg) => Err(anyhow!("Transient error: {}", msg)),
        }
    }

    /// 批量并发刷新所有账号配额 (带请求节流与智能限频，防 429)
    pub async fn refresh_all_quotas(&self) -> Result<Vec<(String, Result<QuotaSummary>)>> {
        let summaries = self.storage.list_account_summaries()?;
        let mut results = Vec::new();

        for (idx, s) in summaries.into_iter().enumerate() {
            if idx > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
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

        if account.device_profile.is_none() {
            let profile = device::generate_device_profile();
            account.device_profile = Some(profile);
            let _ = self.storage.save_account(&account);
        }

        switcher::SwitchEngine::switch(&account, target, restart)?;

        account.last_used = Some(chrono::Utc::now().timestamp());
        let _ = self.storage.save_account(&account);

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

    /// 获取本地纯数据库统计大盘 (零代理、零拦截，只读模式)
    pub fn get_local_usage_stats(&self) -> Result<MasterAnalyticsReport> {
        stats::StatsEngine::generate_master_report(&self.storage)
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
        let _ = self.refresh_account_quota(&id).await;

        self.storage.get_account(&id)?.ok_or_else(|| anyhow!("Failed to load saved account"))
    }
}
