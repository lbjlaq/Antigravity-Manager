pub use am_proxy;
pub mod auth;
pub mod cli_statusline;
pub mod config;
pub mod context_monitor;
pub mod device;
pub mod quota;
pub mod runtime;
pub mod service;
pub mod stats;
pub mod storage;
pub mod strategy;
pub mod switcher;
pub mod types;
pub mod update;
pub mod version;

use anyhow::{anyhow, Result};
pub use config::*;
pub use quota::QuotaFetchResult;
pub use runtime::{shared_http_client, trim_process_memory};
pub use service::*;
pub use stats::*;
use std::sync::Arc;
pub use strategy::*;
pub use types::*;
pub use update::*;
pub use version::*;

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

    /// 创建复用当前管理器存储与 HTTP 客户端的后台守护服务（避免双份连接池与状态）
    pub fn create_daemon_service(&self) -> service::CoreDaemonService {
        service::CoreDaemonService::from_shared(
            Arc::clone(&self.storage),
            Arc::clone(&self.oauth),
            Arc::clone(&self.quota),
        )
    }

    pub(crate) fn from_arcs(
        storage: Arc<storage::StorageManager>,
        oauth: Arc<auth::OAuthClient>,
        quota: Arc<quota::QuotaClient>,
    ) -> Self {
        Self {
            storage,
            oauth,
            quota,
        }
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
        let acc = self
            .find_account(query)?
            .ok_or_else(|| anyhow!("Account not found: {}", query))?;
        self.storage.delete_account(&acc.id)
    }

    /// 手动禁用账号
    pub fn disable_account(&self, query: &str, reason: Option<&str>) -> Result<Account> {
        let mut acc = self
            .find_account(query)?
            .ok_or_else(|| anyhow!("Account not found: {}", query))?;
        acc.disabled = true;
        acc.disabled_reason = reason
            .map(String::from)
            .or_else(|| Some("Manually disabled".to_string()));
        self.storage.save_account(&acc)?;
        Ok(acc)
    }

    /// 手动启用账号
    pub fn enable_account(&self, query: &str) -> Result<Account> {
        let mut acc = self
            .find_account(query)?
            .ok_or_else(|| anyhow!("Account not found: {}", query))?;
        acc.disabled = false;
        acc.disabled_reason = None;
        self.storage.save_account(&acc)?;
        Ok(acc)
    }

    /// 重新调整账号持久化顺序
    pub fn reorder_accounts(&self, ordered_ids: &[String]) -> Result<()> {
        self.storage.reorder_accounts(ordered_ids)
    }

    /// 标签管理：添加标签到账号
    pub fn add_account_tags(&self, query: &str, tags_to_add: &[String]) -> Result<Account> {
        let mut acc = self
            .find_account(query)?
            .ok_or_else(|| anyhow!("Account not found: {}", query))?;
        for tag in tags_to_add {
            let clean = tag.trim();
            if clean.is_empty() {
                continue;
            }
            if clean.chars().count() > 15 {
                return Err(anyhow!(
                    "Tag '{}' exceeds maximum length of 15 characters",
                    clean
                ));
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
        let mut acc = self
            .find_account(query)?
            .ok_or_else(|| anyhow!("Account not found: {}", query))?;
        acc.tags.retain(|t| {
            !tags_to_remove
                .iter()
                .any(|r| r.trim().eq_ignore_ascii_case(t))
        });
        self.storage.save_account(&acc)?;
        Ok(acc)
    }

    /// 标签管理：直接设置完整标签列表
    pub fn set_account_tags(&self, query: &str, new_tags: &[String]) -> Result<Account> {
        let mut acc = self
            .find_account(query)?
            .ok_or_else(|| anyhow!("Account not found: {}", query))?;
        let mut clean_tags = Vec::new();
        for tag in new_tags {
            let clean = tag.trim();
            if clean.is_empty() {
                continue;
            }
            if clean.chars().count() > 15 {
                return Err(anyhow!(
                    "Tag '{}' exceeds maximum length of 15 characters",
                    clean
                ));
            }
            if !clean_tags
                .iter()
                .any(|t: &String| t.eq_ignore_ascii_case(clean))
            {
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
            if healthy_only && acc.is_restricted() {
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
                        let qa = Self::calculate_account_effective_quota(
                            a,
                            ModelPoolPreference::Balanced,
                        );
                        let qb = Self::calculate_account_effective_quota(
                            b,
                            ModelPoolPreference::Balanced,
                        );
                        qb.partial_cmp(&qa).unwrap_or(std::cmp::Ordering::Equal)
                    });
                }
                AccountSortOrder::QuotaAsc => {
                    accounts.sort_by(|a, b| {
                        let qa = Self::calculate_account_effective_quota(
                            a,
                            ModelPoolPreference::Balanced,
                        );
                        let qb = Self::calculate_account_effective_quota(
                            b,
                            ModelPoolPreference::Balanced,
                        );
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

    fn account_tier_rank(acc: &Account) -> u8 {
        let sub = acc
            .quota
            .as_ref()
            .and_then(|q| q.subscription_tier.as_deref())
            .unwrap_or("")
            .to_ascii_uppercase();
        if sub.contains("ULTRA") || sub.contains("HELIUM") {
            4
        } else if sub.contains("PRO") || sub.contains("PREMIUM") {
            3
        } else if sub.contains("ENTERPRISE") {
            2
        } else {
            1
        }
    }

    fn account_earliest_future_reset(acc: &Account, now: i64) -> Option<i64> {
        let q = acc.quota.as_ref()?;
        let mut earliest: Option<i64> = None;
        if let Some(groups) = &q.quota_groups {
            for g in groups {
                for b in &g.buckets {
                    if let Some(rt) = b.reset_time.as_deref() {
                        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(rt) {
                            let ts = dt.timestamp();
                            if ts > now {
                                earliest = Some(earliest.map_or(ts, |e| e.min(ts)));
                            }
                        }
                    }
                }
            }
        }
        for m in &q.models {
            if let Some(rt) = m.reset_time.as_deref() {
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(rt) {
                    let ts = dt.timestamp();
                    if ts > now {
                        earliest = Some(earliest.map_or(ts, |e| e.min(ts)));
                    }
                }
            }
        }
        earliest
    }

    /// 智能选号候选排序列表（供单选与排除当前账号切换复用）
    pub fn pick_ranked_accounts(&self, filter: AccountSelectionFilter) -> Result<Vec<Account>> {
        let mut candidates = self.storage.list_full_accounts()?;
        let prefer = filter.prefer_pool.unwrap_or(ModelPoolPreference::Balanced);
        let strategy = filter.strategy.unwrap_or(SwitchStrategy::MaxQuota);

        candidates.retain(|acc| {
            if filter.healthy_only && acc.is_restricted() {
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
            return Ok(Vec::new());
        }

        match strategy {
            SwitchStrategy::MaxQuota => {
                candidates.sort_by(|a, b| {
                    let qa = Self::calculate_account_effective_quota(a, prefer);
                    let qb = Self::calculate_account_effective_quota(b, prefer);
                    qb.partial_cmp(&qa)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| Self::account_tier_rank(b).cmp(&Self::account_tier_rank(a)))
                        .then_with(|| a.last_used.unwrap_or(0).cmp(&b.last_used.unwrap_or(0)))
                });
            }
            SwitchStrategy::ExpiringSoon => {
                let now = chrono::Utc::now().timestamp();
                candidates.sort_by(|a, b| {
                    let qa = Self::calculate_account_effective_quota(a, prefer);
                    let qb = Self::calculate_account_effective_quota(b, prefer);
                    let a_elig = qa >= 20.0;
                    let b_elig = qb >= 20.0;
                    b_elig
                        .cmp(&a_elig)
                        .then_with(|| {
                            let ra =
                                Self::account_earliest_future_reset(a, now).unwrap_or(i64::MAX);
                            let rb =
                                Self::account_earliest_future_reset(b, now).unwrap_or(i64::MAX);
                            ra.cmp(&rb)
                        })
                        .then_with(|| qb.partial_cmp(&qa).unwrap_or(std::cmp::Ordering::Equal))
                });
            }
            SwitchStrategy::TierFirst => {
                candidates.sort_by(|a, b| {
                    let qa = Self::calculate_account_effective_quota(a, prefer);
                    let qb = Self::calculate_account_effective_quota(b, prefer);
                    let a_avail = qa > 0.0;
                    let b_avail = qb > 0.0;
                    b_avail
                        .cmp(&a_avail)
                        .then_with(|| Self::account_tier_rank(b).cmp(&Self::account_tier_rank(a)))
                        .then_with(|| qb.partial_cmp(&qa).unwrap_or(std::cmp::Ordering::Equal))
                        .then_with(|| a.last_used.unwrap_or(0).cmp(&b.last_used.unwrap_or(0)))
                });
            }
            SwitchStrategy::LeastUsed => {
                candidates.sort_by(|a, b| {
                    let qa = Self::calculate_account_effective_quota(a, prefer);
                    let qb = Self::calculate_account_effective_quota(b, prefer);
                    let a_avail = qa > 0.0;
                    let b_avail = qb > 0.0;
                    b_avail
                        .cmp(&a_avail)
                        .then_with(|| a.last_used.unwrap_or(0).cmp(&b.last_used.unwrap_or(0)))
                        .then_with(|| qb.partial_cmp(&qa).unwrap_or(std::cmp::Ordering::Equal))
                });
            }
            SwitchStrategy::Priority => {
                candidates.sort_by(|a, b| b.priority.unwrap_or(0).cmp(&a.priority.unwrap_or(0)));
            }
            SwitchStrategy::Random => {
                use rand::seq::SliceRandom;
                let mut rng = rand::thread_rng();
                candidates.shuffle(&mut rng);
            }
        }

        Ok(candidates)
    }

    /// 智能选号 (Smart Account Picker)
    pub fn pick_best_account(&self, filter: AccountSelectionFilter) -> Result<Option<Account>> {
        Ok(self.pick_ranked_accounts(filter)?.into_iter().next())
    }

    /// 刷新单个账号配额 (支持失败模式智能诊断与自愈标记)
    /// 智能优化：仅当 access_token 缺失、已过期或剩余有效期低于 5 分钟 (300s) 时才触发 OAuth 刷新，
    /// 既极大提升刷新速率（秒级响应），又大幅降低被 Google OAuth 端点触发限流风控的风险。
    pub async fn refresh_account_quota(&self, account_id: &str) -> Result<QuotaSummary> {
        let mut account = self
            .storage
            .get_account(account_id)?
            .ok_or_else(|| anyhow!("Account not found: {}", account_id))?;

        let now = chrono::Utc::now().timestamp();
        let need_refresh_token = match account.token.expiry_timestamp {
            Some(exp) => account.token.access_token.trim().is_empty() || (exp - now) < 300,
            None => true,
        };

        if need_refresh_token {
            let refreshed_token = match self
                .oauth
                .refresh_access_token(&account.token.refresh_token)
                .await
            {
                Ok(tok) => tok,
                Err(e) => {
                    // 如果 refresh_token 换取直接报错 invalid_grant，自动打标凭据失效
                    let err_str = e.to_string();
                    if err_str.contains("invalid_grant") || err_str.contains("revoked") {
                        account.disabled = true;
                        account.disabled_reason =
                            Some("Refresh token revoked or expired".to_string());
                        let _ = self.storage.save_account(&account);
                    }
                    return Err(e);
                }
            };
            // 仅合并更新新令牌字段，保留已有的 project_id、id_token 与 is_gcp_tos
            account.token.access_token = refreshed_token.access_token;
            account.token.expires_in = refreshed_token.expires_in;
            account.token.expiry_timestamp = refreshed_token.expiry_timestamp;
            if refreshed_token.token_type.is_some() {
                account.token.token_type = refreshed_token.token_type;
            }
            if refreshed_token.id_token.is_some() {
                account.token.id_token = refreshed_token.id_token;
            }
            if !refreshed_token.refresh_token.trim().is_empty() {
                account.token.refresh_token = refreshed_token.refresh_token;
            }
        }

        // 2. 通过 loadCodeAssist 拉取 Project ID 与权威订阅等级 (PRO / ULTRA / ENTERPRISE / FREE)
        let (fetched_pid, fetched_tier, fetched_raw_tier) = self
            .quota
            .fetch_project_and_tier(&account.token.access_token)
            .await;
        if account.token.project_id.is_none() {
            if let Some(pid) = fetched_pid {
                account.token.project_id = Some(pid);
            }
        }

        // 3. 查询最新配额并自愈
        let fetch_res = self
            .quota
            .fetch_quota_safe(
                &account.token.access_token,
                account.token.project_id.as_deref(),
            )
            .await;

        match fetch_res {
            QuotaFetchResult::Success(mut quota) => {
                // 回填订阅等级：优先使用本次 loadCodeAssist 返回值，其次保留已有值，避免刷配额时抹除 PRO/ULTRA 标签
                if quota.subscription_tier.is_none() {
                    quota.subscription_tier = fetched_tier.or_else(|| {
                        account
                            .quota
                            .as_ref()
                            .and_then(|q| q.subscription_tier.clone())
                    });
                }
                if quota.raw_tier.is_none() {
                    quota.raw_tier = fetched_raw_tier
                        .or_else(|| account.quota.as_ref().and_then(|q| q.raw_tier.clone()));
                }
                account.quota = Some(quota.clone());
                // 成功时解除以往的临时错误标记
                if account.disabled
                    && account
                        .disabled_reason
                        .as_deref()
                        .unwrap_or("")
                        .contains("403")
                {
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
    pub fn switch_account(
        &self,
        query: &str,
        target: TargetHost,
        restart: bool,
    ) -> Result<Account> {
        let mut account = self
            .find_account(query)?
            .ok_or_else(|| anyhow!("Account not found matching query: {}", query))?;

        if account.device_profile.is_none() {
            let profile = device::generate_device_profile();
            account.device_profile = Some(profile);
            let _ = self.storage.save_account(&account);
        }

        switcher::SwitchEngine::switch(&account, target, restart)?;
        switcher::SwitchEngine::invalidate_active_hosts_cache();

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

    /// 获取各宿主激活状态（严格对齐主分支真实凭据指纹与 `refresh_token` 逆向比对逻辑）
    pub fn get_active_hosts(&self) -> ActiveHostStatus {
        Self::resolve_active_hosts_with_storage(&self.storage)
    }

    pub(crate) fn resolve_active_hosts_with_storage(
        storage: &storage::StorageManager,
    ) -> ActiveHostStatus {
        let mut status = switcher::SwitchEngine::detect_active_hosts();

        let (idx_curr, idx_target, idx_app, idx_ide, idx_acp, idx_agy) = storage
            .get_active_index_context()
            .unwrap_or((None, None, None, None, None, None));

        let summaries = storage.list_account_summaries().unwrap_or_default();

        let find_email_by_id = |id: &str| -> Option<String> {
            summaries
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.email.clone())
                .or_else(|| storage.get_account(id).ok().flatten().map(|a| a.email))
        };

        // 针对外部提取的真实 refresh_token 极速匹配本地账号池 (O(1) 优先命中当前候选，未命中再遍历)
        let find_by_rt = |rt: &str| -> Option<(String, String)> {
            let trimmed = rt.trim();
            if trimmed.is_empty() {
                return None;
            }
            let fast_candidates = [
                idx_ide.as_deref(),
                idx_app.as_deref(),
                idx_agy.as_deref(),
                idx_acp.as_deref(),
                idx_curr.as_deref(),
            ];
            for cid in fast_candidates.into_iter().flatten() {
                if let Ok(Some(acc)) = storage.get_account(cid) {
                    if acc.token.refresh_token.trim() == trimmed {
                        return Some((acc.id, acc.email));
                    }
                }
            }
            for s in &summaries {
                if let Ok(Some(acc)) = storage.get_account(&s.id) {
                    if acc.token.refresh_token.trim() == trimmed {
                        return Some((acc.id, acc.email));
                    }
                }
            }
            None
        };

        let resolve_cred =
            |cred: (Option<String>, Option<String>)| -> Option<(Option<String>, String)> {
                let (rt_opt, email_opt) = cred;
                if let Some(ref rt) = rt_opt {
                    if let Some((id, email)) = find_by_rt(rt) {
                        return Some((Some(id), email));
                    }
                }
                if let Some(email) = email_opt {
                    let matched_id = summaries
                        .iter()
                        .find(|s| s.email.eq_ignore_ascii_case(&email))
                        .map(|s| s.id.clone());
                    return Some((matched_id, email));
                }
                None
            };

        let ide_sqlite_email =
            switcher::SwitchEngine::extract_refresh_token_from_sqlite("Antigravity IDE")
                .and_then(|rt| find_by_rt(&rt).map(|(_, email)| email))
                .or_else(|| switcher::SwitchEngine::read_account_from_sqlite("Antigravity IDE"));

        let ide_app_storage_email = if ide_sqlite_email.is_none() {
            switcher::SwitchEngine::read_app_storage_account("Antigravity IDE", status.ide_running)
        } else {
            None
        };

        let keyring_resolved =
            switcher::SwitchEngine::read_system_keyring_credential().and_then(&resolve_cred);
        let standalone_resolved = if keyring_resolved.is_none() {
            switcher::SwitchEngine::read_jetski_standalone_credential().and_then(&resolve_cred)
        } else {
            None
        };

        let app_storage_email =
            switcher::SwitchEngine::read_app_storage_account("Antigravity", status.app_running);

        let is_classic_v2 = status
            .app_version
            .as_deref()
            .map(|v| version::compare_version(v, "2.0.0") != std::cmp::Ordering::Less)
            .unwrap_or(true);

        let classic_sqlite_email = if !is_classic_v2 || status.app_version.is_none() {
            switcher::SwitchEngine::extract_refresh_token_from_sqlite("Antigravity")
                .and_then(|rt| find_by_rt(&rt).map(|(_, email)| email))
                .or_else(|| switcher::SwitchEngine::read_account_from_sqlite("Antigravity"))
        } else {
            None
        };

        let cli_dir_exists = switcher::SwitchEngine::has_antigravity_cli_dir();
        let cli_exclusive_resolved =
            switcher::SwitchEngine::read_cli_exclusive_credential().and_then(&resolve_cred);
        let cli_fallback_resolved = if cli_exclusive_resolved.is_none()
            && keyring_resolved.is_none()
            && (status.agy_running || cli_dir_exists || idx_agy.is_some())
        {
            switcher::SwitchEngine::read_gemini_cli_fallback_credential().and_then(&resolve_cred)
        } else {
            None
        };

        let acp_resolved =
            switcher::SwitchEngine::read_acp_plugin_credential().and_then(&resolve_cred);

        let idx_curr_email = idx_curr.as_deref().and_then(&find_email_by_id);
        let idx_app_email = idx_app.as_deref().and_then(&find_email_by_id);
        let idx_ide_email = idx_ide.as_deref().and_then(&find_email_by_id);
        let idx_agy_email = idx_agy.as_deref().and_then(&find_email_by_id);
        let idx_acp_email = idx_acp.as_deref().and_then(&find_email_by_id);

        Self::apply_host_credential_signals(
            &mut status,
            HostCredentialSignals {
                idx_target,
                idx_curr_email,
                idx_app,
                idx_app_email,
                idx_ide,
                idx_ide_email,
                idx_agy,
                idx_agy_email,
                idx_acp_email,
                ide_sqlite_email,
                ide_app_storage_email,
                keyring_resolved,
                standalone_resolved,
                app_storage_email,
                classic_sqlite_email,
                cli_dir_exists,
                cli_exclusive_resolved,
                cli_fallback_resolved,
                acp_resolved,
            },
        );

        status
    }

    /// 根据各宿主独立/共享凭据信号归属每个宿主的当前生效账号（纯函数，便于覆盖全场景单测）
    fn apply_host_credential_signals(
        status: &mut ActiveHostStatus,
        signals: HostCredentialSignals,
    ) {
        let email_eq = |a: Option<&str>, b: &str| -> bool {
            a.map(|x| x.eq_ignore_ascii_case(b)).unwrap_or(false)
        };

        // 1. Antigravity IDE：严禁读取系统 Keyring，优先 state.vscdb -> IDE app_storage.json -> idx_ide
        status.ide_account = signals
            .ide_sqlite_email
            .or(signals.ide_app_storage_email)
            .or(signals.idx_ide_email.clone());

        // 2. Antigravity 原生桌面版 (App)：>= 2.0.0 使用系统 Keyring（及 jetski-standalone 文件回退）+ App 专属 app_storage.json
        let is_classic_v2 = status
            .app_version
            .as_deref()
            .map(|v| version::compare_version(v, "2.0.0") != std::cmp::Ordering::Less)
            .unwrap_or(true);

        let shared_desktop_cred = signals
            .keyring_resolved
            .as_ref()
            .or(signals.standalone_resolved.as_ref());
        let app_present = status.app_running
            || status.app_version.is_some()
            || signals.app_storage_email.is_some()
            || signals.idx_app.is_some();

        if is_classic_v2 {
            if let Some((ref k_id_opt, ref k_email)) = shared_desktop_cred {
                let confirmed_by_app_storage =
                    email_eq(signals.app_storage_email.as_deref(), k_email);
                let confirmed_by_idx_app = (k_id_opt.is_some() && signals.idx_app == *k_id_opt)
                    || email_eq(signals.idx_app_email.as_deref(), k_email);

                let matches_recent_ide_switch = signals.idx_target.as_deref() == Some("ide")
                    && ((k_id_opt.is_some() && signals.idx_ide == *k_id_opt)
                        || email_eq(signals.idx_ide_email.as_deref(), k_email)
                        || email_eq(status.ide_account.as_deref(), k_email));
                let matches_recent_agy_switch = signals.idx_target.as_deref() == Some("agy")
                    && ((k_id_opt.is_some() && signals.idx_agy == *k_id_opt)
                        || email_eq(signals.idx_agy_email.as_deref(), k_email));

                let has_distinct_app_record =
                    signals.app_storage_email.is_some() || signals.idx_app.is_some();

                let allow_shared_cred = if confirmed_by_app_storage || confirmed_by_idx_app {
                    true
                } else if matches_recent_ide_switch {
                    // Antigravity 2.0+ 与 Antigravity IDE 2.6+ 共享系统 Keyring：
                    // 若 App 已安装/运行且无冲突的专属记录，直接反映真实生效的共享 Keyring；
                    // 若 App 未安装，或已有明确不同的 App 专属记录，则避免误覆盖。
                    app_present && !has_distinct_app_record
                } else if matches_recent_agy_switch {
                    status.app_running && !has_distinct_app_record
                } else {
                    // classic/all/外部原生登录（或 Keyring 与最近 ide/agy 切换账号不同）
                    app_present || (status.ide_account.is_none() && signals.idx_agy.is_none())
                };

                if allow_shared_cred {
                    status.app_account = Some(k_email.clone());
                }
            }

            if status.app_account.is_none() {
                status.app_account = signals.app_storage_email.clone();
            }
            if status.app_account.is_none() {
                status.app_account = signals.idx_app_email.clone();
            }
            if status.app_account.is_none() && status.app_version.is_none() {
                status.app_account = signals.classic_sqlite_email.clone();
            }
        } else {
            status.app_account = signals
                .classic_sqlite_email
                .or(signals.app_storage_email.clone())
                .or(signals.idx_app_email.clone());
        }

        // 3. Antigravity CLI (agy)：
        // 优先专属文件 antigravity-oauth-token；原生 agy CLI 在无独立文件时直接读取系统 Keyring，
        // 仅当确认本机使用过/运行着 CLI 时才认领共享 Keyring，避免污染未安装 CLI 的环境。
        let cli_present = status.agy_running || signals.cli_dir_exists || signals.idx_agy.is_some();
        if let Some((_, cli_email)) = signals.cli_exclusive_resolved {
            status.agy_account = Some(cli_email);
        } else if let Some((ref k_id_opt, ref k_email)) = signals.keyring_resolved {
            let explicit_agy = signals.idx_target.as_deref() == Some("agy")
                || (k_id_opt.is_some() && signals.idx_agy == *k_id_opt)
                || email_eq(signals.idx_agy_email.as_deref(), k_email);
            if explicit_agy || (cli_present && signals.idx_agy.is_none()) {
                status.agy_account = Some(k_email.clone());
            }
        }
        if status.agy_account.is_none() {
            status.agy_account = signals.idx_agy_email.clone();
        }
        if status.agy_account.is_none() && cli_present {
            if let Some((_, fb_email)) = signals.cli_fallback_resolved {
                status.agy_account = Some(fb_email);
            }
        }

        // 4. ACP 插件 (Plugin)：优先 acp_token.json / macOS Keychain (gemini:antigravity-acp) -> idx_acp
        if let Some((_, acp_email)) = signals.acp_resolved {
            status.plugin_account = Some(acp_email);
        }
        if status.plugin_account.is_none() {
            status.plugin_account = signals.idx_acp_email;
        }

        // 5. 兼容旧版单一 current_account_id 兜底
        if status.app_account.is_none()
            && status.ide_account.is_none()
            && status.agy_account.is_none()
            && status.plugin_account.is_none()
        {
            if let Some(email) = signals.idx_curr_email {
                match signals.idx_target.as_deref() {
                    Some("ide") => status.ide_account = Some(email),
                    Some("agy" | "cli") => status.agy_account = Some(email),
                    Some("acp" | "plugin") => status.plugin_account = Some(email),
                    _ => status.app_account = Some(email),
                }
            }
        }
    }

    /// 解除账号的 403 / 验证锁定
    pub fn clear_account_forbidden(&self, query: &str) -> Result<Account> {
        let acc = self
            .find_account(query)?
            .ok_or_else(|| anyhow!("Account not found: {}", query))?;
        self.storage.clear_account_forbidden(&acc.id)?;
        self.storage
            .get_account(&acc.id)?
            .ok_or_else(|| anyhow!("Account reload failed: {}", acc.id))
    }

    /// 重置/重新生成账号的设备指纹
    pub fn regenerate_device_profile(&self, query: &str) -> Result<Account> {
        let mut acc = self
            .find_account(query)?
            .ok_or_else(|| anyhow!("Account not found: {}", query))?;
        acc.device_profile = Some(device::generate_device_profile());
        self.storage.save_account(&acc)?;
        Ok(acc)
    }

    /// 获取本地纯数据库统计大盘 (零代理、零拦截，只读模式)
    pub fn get_local_usage_stats(&self) -> Result<MasterAnalyticsReport> {
        stats::StatsEngine::generate_master_report(&self.storage)
    }

    /// 按指定起止毫秒时间戳范围获取本地统计报告（全量保留窗口内真实请求流水）
    pub fn get_local_usage_stats_in_range(
        &self,
        start_ts_ms: Option<i64>,
        end_ts_ms: Option<i64>,
    ) -> Result<MasterAnalyticsReport> {
        stats::StatsEngine::generate_report_in_range(&self.storage, start_ts_ms, end_ts_ms)
    }

    /// 统一保存 OAuth/Token 换取的账号凭证（按邮箱去重原地更新，保留历史设备指纹与标签）
    pub async fn save_account_from_token_info(
        &self,
        token_info: crate::types::TokenInfo,
        tags: Vec<String>,
    ) -> Result<Account> {
        let user_info = self.oauth.fetch_user_info(&token_info.access_token).await?;

        let existing = self
            .storage
            .list_full_accounts()
            .unwrap_or_default()
            .into_iter()
            .find(|a| a.email.eq_ignore_ascii_case(&user_info.email));

        let (id, account) = if let Some(mut acc) = existing {
            let id = acc.id.clone();
            if user_info
                .name
                .as_deref()
                .is_some_and(|n| !n.trim().is_empty())
            {
                acc.name = user_info.name;
            }
            acc.token = token_info;
            for tag in tags {
                let trimmed = tag.trim().to_string();
                if !trimmed.is_empty() && !acc.tags.contains(&trimmed) {
                    acc.tags.push(trimmed);
                }
            }
            acc.disabled = false;
            acc.disabled_reason = None;
            acc.disabled_at = None;
            if acc.device_profile.is_none() {
                acc.device_profile = Some(device::generate_device_profile());
            }
            (id, acc)
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            let device_profile = device::generate_device_profile();
            let acc = Account {
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
                disabled_at: None,
                banned: false,
                banned_reason: None,
                banned_at: None,
                banned_code: None,
                proxy_disabled: false,
                proxy_disabled_reason: None,
                proxy_disabled_at: None,
                manual_proxy_disabled: false,
                validation_blocked: false,
                validation_blocked_until: None,
                validation_url: None,
                ban_appeal_url: None,
                created_at: Some(chrono::Utc::now().timestamp()),
                last_used: None,
            };
            (id, acc)
        };

        self.storage.save_account(&account)?;
        let _ = self.refresh_account_quota(&id).await;

        self.storage
            .get_account(&id)?
            .ok_or_else(|| anyhow!("Failed to load saved account"))
    }

    /// 通过已有 refresh_token 手工添加账号（可选指定 OAuth 客户端类型）
    pub async fn add_account_by_token_with_client(
        &self,
        refresh_token: &str,
        tags: Vec<String>,
        client_key: Option<&str>,
    ) -> Result<Account> {
        let token_info = self
            .oauth
            .refresh_access_token_with_client(refresh_token, client_key)
            .await?;
        self.save_account_from_token_info(token_info, tags).await
    }

    /// 通过已有 refresh_token 手工添加账号
    pub async fn add_account_by_token(
        &self,
        refresh_token: &str,
        tags: Vec<String>,
    ) -> Result<Account> {
        self.add_account_by_token_with_client(refresh_token, tags, None)
            .await
    }

    /// 使用 OAuth 授权码换取令牌并保存/更新账号
    pub async fn complete_oauth_code_exchange(
        &self,
        code: &str,
        redirect_uri: &str,
        client_key: Option<&str>,
    ) -> Result<Account> {
        let token_info = self
            .oauth
            .exchange_code_with_client(code, redirect_uri, client_key)
            .await?;
        self.save_account_from_token_info(token_info, vec![]).await
    }

    /// 从本地客户端 (Antigravity / Antigravity IDE / Keyring / CLI) 只读扫描并导入账号
    pub async fn import_accounts_from_local_db(&self) -> Result<Vec<Account>> {
        let candidates: Vec<Option<String>> = tokio::task::spawn_blocking(|| {
            vec![
                switcher::SwitchEngine::extract_refresh_token_from_sqlite("Antigravity"),
                switcher::SwitchEngine::extract_refresh_token_from_sqlite("Antigravity IDE"),
                switcher::SwitchEngine::read_system_keyring_credential().and_then(|(rt, _)| rt),
                switcher::SwitchEngine::read_jetski_standalone_credential().and_then(|(rt, _)| rt),
                switcher::SwitchEngine::read_cli_exclusive_credential().and_then(|(rt, _)| rt),
                switcher::SwitchEngine::read_gemini_cli_fallback_credential()
                    .and_then(|(rt, _)| rt),
                switcher::SwitchEngine::read_acp_plugin_credential().and_then(|(rt, _)| rt),
            ]
        })
        .await
        .map_err(|e| anyhow!("Local DB scan task join error: {}", e))?;

        let mut unique_tokens = Vec::new();
        for rt in candidates.into_iter().flatten() {
            let trimmed = rt.trim().to_string();
            if !trimmed.is_empty() && !unique_tokens.contains(&trimmed) {
                unique_tokens.push(trimmed);
            }
        }

        if unique_tokens.is_empty() {
            anyhow::bail!("No logged-in Antigravity account found in local IDE/App/CLI storage");
        }

        let mut imported = Vec::new();
        let mut last_err: Option<anyhow::Error> = None;
        for rt in unique_tokens {
            match self
                .add_account_by_token_with_client(&rt, vec![], None)
                .await
            {
                Ok(acc) => imported.push(acc),
                Err(e) => {
                    tracing::warn!("Failed to import discovered local token: {}", e);
                    last_err = Some(e);
                }
            }
        }

        if imported.is_empty() {
            if let Some(err) = last_err {
                return Err(err);
            }
            anyhow::bail!("No valid accounts could be imported from local storage");
        }

        Ok(imported)
    }

    /// 从指定自定义 state.vscdb 文件只读提取凭据并导入账号
    pub async fn import_account_from_custom_db(
        &self,
        db_path: &std::path::Path,
    ) -> Result<Account> {
        if !db_path.exists() {
            anyhow::bail!("Database file not found: {}", db_path.display());
        }
        let path_buf = db_path.to_path_buf();
        let rt = tokio::task::spawn_blocking(move || {
            switcher::SwitchEngine::extract_refresh_token_from_db_path(&path_buf)
        })
        .await
        .map_err(|e| anyhow!("Custom DB read task join error: {}", e))?
        .ok_or_else(|| {
            anyhow!(
                "No valid Antigravity refresh_token found in {}",
                db_path.display()
            )
        })?;

        self.add_account_by_token_with_client(&rt, vec![], None)
            .await
    }

    /// 导出账号备份 JSON（对齐主分支格式：{"accounts": [{"email": "...", "refresh_token": "..."}]}）
    pub fn export_accounts_json(&self) -> Result<String> {
        let accounts = self.list_full_accounts()?;
        let items: Vec<storage::backup::AccountExportItem> = accounts
            .into_iter()
            .map(|a| storage::backup::AccountExportItem {
                email: a.email,
                refresh_token: a.token.refresh_token,
            })
            .collect();
        let resp = storage::backup::AccountExportResponse { accounts: items };
        Ok(serde_json::to_string_pretty(&resp)?)
    }

    /// 从 JSON 字符串导入账号备份
    pub fn import_accounts_json(
        &self,
        backup_json: &str,
        overwrite: bool,
    ) -> Result<storage::backup::ImportReport> {
        storage::backup::BackupManager::import_from_str(&self.storage, backup_json, overwrite)
    }

    /// 按策略智能选号并自动切换到指定宿主
    pub fn smart_switch(
        &self,
        target: TargetHost,
        strategy: Option<&str>,
        prefer: Option<&str>,
        tag: Option<String>,
        restart: bool,
    ) -> Result<Option<Account>> {
        let cfg = AppConfig::load();
        let strategy_str = strategy.unwrap_or(&cfg.auto_switch.strategy);
        let prefer_str = prefer.unwrap_or(&cfg.auto_switch.pool);

        let strat = strategy_str
            .parse::<SwitchStrategy>()
            .unwrap_or(SwitchStrategy::MaxQuota);
        let pref = prefer_str
            .parse::<ModelPoolPreference>()
            .unwrap_or(ModelPoolPreference::Balanced);

        let filter = AccountSelectionFilter {
            strategy: Some(strat),
            prefer_pool: Some(pref),
            required_tag: tag,
            min_quota_percentage: None,
            healthy_only: true,
        };

        let best = self.pick_best_account(filter)?;
        if let Some(ref acc) = best {
            self.switch_account(&acc.id, target, restart)?;
        }
        Ok(best)
    }

    /// 异步安全切号：切号前自动检查并刷新空/即将过期的 access_token，确保导入账号与静置账号一键切号即用
    pub async fn switch_account_with_refresh(
        &self,
        query: &str,
        target: TargetHost,
        restart: bool,
    ) -> Result<Account> {
        let mut account = self
            .find_account(query)?
            .ok_or_else(|| anyhow!("Account not found matching query: {}", query))?;

        service::CoreDaemonService::ensure_valid_token(&mut account, &self.storage, &self.oauth)
            .await?;

        let mgr = self.clone();
        let acc_id = account.id.clone();
        tokio::task::spawn_blocking(move || mgr.switch_account(&acc_id, target, restart))
            .await
            .map_err(|e| anyhow!("Switch task join error: {}", e))?
    }

    /// 异步智能选号并切号（统一遵循主窗口仪表盘选号策略 -> 设置自动切号策略，并优先避开当前正在使用的账号）
    pub async fn smart_switch_with_refresh(
        &self,
        target: TargetHost,
        strategy: Option<&str>,
        prefer: Option<&str>,
        tag: Option<String>,
        restart: bool,
    ) -> Result<Option<Account>> {
        let cfg = AppConfig::load();
        let strategy_str = strategy
            .or(cfg.ui_state.dashboard_pick_strategy.as_deref())
            .unwrap_or(&cfg.auto_switch.strategy);
        let prefer_str = prefer
            .or(cfg.ui_state.dashboard_pool_preference.as_deref())
            .unwrap_or(&cfg.auto_switch.pool);

        let strat = strategy_str
            .parse::<SwitchStrategy>()
            .unwrap_or(SwitchStrategy::MaxQuota);
        let pref = prefer_str
            .parse::<ModelPoolPreference>()
            .unwrap_or(ModelPoolPreference::Balanced);

        let filter = AccountSelectionFilter {
            strategy: Some(strat),
            prefer_pool: Some(pref),
            required_tag: tag,
            min_quota_percentage: None,
            healthy_only: true,
        };

        let ranked = self.pick_ranked_accounts(filter)?;
        if ranked.is_empty() {
            return Ok(None);
        }

        // 解析当前宿主已绑定账号，优先切换到“非当前账号”的顺位第一候选（与主窗口仪表盘一键换号逻辑完全一致）
        let hosts = Self::resolve_active_hosts_with_storage(&self.storage);
        let current_key = match target {
            TargetHost::App => hosts.app_account,
            TargetHost::Ide => hosts.ide_account,
            TargetHost::Plugin => hosts.plugin_account,
            TargetHost::Agy => hosts.agy_account,
            TargetHost::All => None,
        };

        let best = if let Some(ref cur) = current_key {
            ranked
                .iter()
                .find(|a| a.id != *cur && !a.email.eq_ignore_ascii_case(cur))
                .or_else(|| ranked.first())
                .cloned()
        } else {
            ranked.into_iter().next()
        };

        if let Some(acc) = best {
            let switched = self
                .switch_account_with_refresh(&acc.id, target, restart)
                .await?;
            return Ok(Some(switched));
        }
        Ok(None)
    }

    /// 统一的本地回环 OAuth 登录流（含 120s 超时与 state 防 CSRF 校验），供 GUI/CLI 复用
    pub async fn login_via_oauth_loopback<F>(&self, open_url: F) -> Result<Account>
    where
        F: FnOnce(&str),
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| anyhow!("Failed to bind loopback port: {}", e))?;
        let port = listener.local_addr()?.port();

        let redirect_uri = format!("http://127.0.0.1:{}/callback", port);
        let expected_state = uuid::Uuid::new_v4().to_string();
        let auth_url = auth::OAuthClient::build_auth_url(&redirect_uri, &expected_state);

        open_url(&auth_url);

        let (mut socket, _) =
            tokio::time::timeout(std::time::Duration::from_secs(120), listener.accept())
                .await
                .map_err(|_| anyhow!("OAuth login timed out (120s). Please try again."))?
                .map_err(|e| anyhow!("Failed to accept callback connection: {}", e))?;

        let mut buf = [0u8; 2048];
        let n = socket
            .read(&mut buf)
            .await
            .map_err(|e| anyhow!("Failed to read OAuth callback: {}", e))?;
        let request = String::from_utf8_lossy(&buf[..n]);

        let req_path = request
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or_default();

        let query_str = req_path.split('?').nth(1).unwrap_or_default();
        let mut code_opt: Option<&str> = None;
        let mut state_opt: Option<&str> = None;
        for pair in query_str.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                match k {
                    "code" => code_opt = Some(v),
                    "state" => state_opt = Some(v),
                    _ => {}
                }
            }
        }

        if state_opt != Some(expected_state.as_str()) {
            let err_resp = "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<html><body><h2>❌ Authorization Failed: Invalid OAuth state</h2></body></html>";
            let _ = socket.write_all(err_resp.as_bytes()).await;
            return Err(anyhow!("OAuth state mismatch (possible CSRF)"));
        }

        let code = match code_opt {
            Some(c) => c,
            None => {
                let err_resp = "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<html><body><h2>❌ Authorization Failed: Missing code</h2></body></html>";
                let _ = socket.write_all(err_resp.as_bytes()).await;
                return Err(anyhow!("Authorization code not found in callback"));
            }
        };

        let token_info = self
            .oauth
            .exchange_code(code, &redirect_uri)
            .await
            .map_err(|e| anyhow!("Token exchange failed: {}", e))?;

        let success_html = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
            <!DOCTYPE html><html><head><meta charset='utf-8'><title>Antigravity Manager</title></head>\
            <body style='font-family:-apple-system,BlinkMacSystemFont,sans-serif;background:#09090b;color:#f4f4f5;text-align:center;padding:60px 20px;'>\
            <div style='display:inline-block;padding:32px 48px;border-radius:20px;background:#18181b;border:1px solid #27272a;box-shadow:0 12px 30px rgba(0,0,0,0.5);'>\
            <div style='font-size:40px;margin-bottom:16px;'>✅</div>\
            <h2 style='margin:0 0 8px 0;font-size:20px;'>授权成功 / Authorization Successful</h2>\
            <p style='color:#a1a1aa;font-size:13px;margin:0;'>Antigravity Manager 已安全同步凭证，您可以关闭此浏览器标签页。</p>\
            </div></body></html>";
        let _ = socket.write_all(success_html.as_bytes()).await;

        self.add_account_by_token(&token_info.refresh_token, vec![])
            .await
            .map_err(|e| anyhow!("Failed to save account: {}", e))
    }

    /// 跨平台采集系统环境、宿主安装与存活状态诊断信息（供 GUI 与 CLI `doctor` 共用）
    pub fn get_system_info(&self, app_version: &str) -> SystemInfo {
        use std::path::PathBuf;
        use sysinfo::System;

        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let gemini_db = home.join(".gemini/antigravity/conversation_summaries.db");

        let db_size_bytes = if gemini_db.exists() {
            std::fs::metadata(&gemini_db).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };

        let db_size_formatted = if db_size_bytes >= 1024 * 1024 {
            format!("{:.1} MB", db_size_bytes as f64 / (1024.0 * 1024.0))
        } else if db_size_bytes >= 1024 {
            format!("{:.1} KB", db_size_bytes as f64 / 1024.0)
        } else {
            format!("{} B", db_size_bytes)
        };

        // 单遍精简进程扫描：同时采集本管理器内存与四宿主运行状态（避免重复扫描 OS 进程表）
        let mut sys = System::new();
        switcher::SwitchEngine::refresh_processes_minimal(&mut sys, true);

        let self_pid = std::process::id();
        let mut mem_bytes: u64 = 0;
        for (pid, proc) in sys.processes() {
            let name_lower = proc.name().to_string_lossy().to_lowercase();
            if pid.as_u32() == self_pid
                || name_lower.starts_with("am-gui")
                || name_lower.contains("antigravity-manager")
                || name_lower.contains("antigravity manager")
            {
                mem_bytes += proc.memory();
            }
        }
        let memory_usage_mb = if mem_bytes > 0 {
            mem_bytes as f64 / (1024.0 * 1024.0)
        } else {
            28.4
        };

        let config_path = AppConfig::config_path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let hosts = switcher::SwitchEngine::detect_active_hosts_from_sys(&sys);
        let cfg = AppConfig::load();

        let default_app = switcher::SwitchEngine::resolve_app_user_storage_dir("Antigravity")
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let default_ide = switcher::SwitchEngine::resolve_app_user_storage_dir("Antigravity IDE")
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let default_plugin = home
            .join(".antigravity/acp-daemon.json")
            .to_string_lossy()
            .to_string();
        let default_cli = home
            .join(".config/agy/credentials.json")
            .to_string_lossy()
            .to_string();

        let app_path = cfg.custom_paths.app.unwrap_or(default_app);
        let ide_path = cfg.custom_paths.ide.unwrap_or(default_ide);
        let plugin_path = cfg.custom_paths.plugin.unwrap_or(default_plugin);
        let cli_path = cfg.custom_paths.cli.unwrap_or(default_cli);

        let app_installed = !app_path.is_empty() && PathBuf::from(&app_path).exists();
        let ide_installed =
            (!ide_path.is_empty() && PathBuf::from(&ide_path).exists()) || gemini_db.exists();
        let plugin_installed = PathBuf::from(&plugin_path).exists();
        let cli_installed = PathBuf::from(&cli_path).exists();

        let total_logged_requests = stats::StatsEngine::count_total_requests();
        let platform = std::env::consts::OS.to_string();
        let (am_cli_path, am_in_path) = Self::detect_am_cli_status();

        SystemInfo {
            app_version: app_version.to_string(),
            platform,
            am_cli_path,
            am_in_path,
            db_size_bytes,
            db_size_formatted,
            total_logged_requests,
            memory_usage_mb: (memory_usage_mb * 10.0).round() / 10.0,
            config_path,
            app_installed,
            app_path,
            app_live: hosts.app_running,
            ide_installed,
            ide_path,
            ide_live: hosts.ide_running,
            plugin_installed,
            plugin_path,
            plugin_live: hosts.plugin_running,
            cli_installed,
            cli_path,
            cli_live: hosts.agy_running,
        }
    }

    /// 探测当前安装包随附的 `am` 二进制路径与是否已进入系统 PATH（零子进程开销）
    fn detect_am_cli_status() -> (Option<String>, bool) {
        let bin_name = if cfg!(windows) { "am.exe" } else { "am" };

        let is_valid_bin = |p: &std::path::Path| -> bool {
            p.is_file() && p.metadata().map(|m| m.len() > 0).unwrap_or(false)
        };

        // AppImage 运行时位于 /tmp/.mount_* 临时挂载目录，不可作为持久化软链源路径
        let is_ephemeral_appimage =
            cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_some();

        let sibling_am = if is_ephemeral_appimage {
            None
        } else {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.join(bin_name)))
                .filter(|p| is_valid_bin(p))
                .map(|p| p.to_string_lossy().to_string())
        };

        let in_path = std::env::var_os("PATH")
            .map(|paths| {
                std::env::split_paths(&paths).any(|dir| {
                    if dir.as_os_str().is_empty() {
                        return false;
                    }
                    is_valid_bin(&dir.join(bin_name))
                })
            })
            .unwrap_or(false);

        (sibling_am, in_path)
    }
}

/// 系统环境与宿主诊断信息（跨平台）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SystemInfo {
    pub app_version: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub am_cli_path: Option<String>,
    #[serde(default)]
    pub am_in_path: bool,
    pub db_size_bytes: u64,
    pub db_size_formatted: String,
    pub total_logged_requests: u64,
    pub memory_usage_mb: f64,
    pub config_path: String,
    pub app_installed: bool,
    pub app_path: String,
    pub app_live: bool,
    pub ide_installed: bool,
    pub ide_path: String,
    pub ide_live: bool,
    pub plugin_installed: bool,
    pub plugin_path: String,
    pub plugin_live: bool,
    pub cli_installed: bool,
    pub cli_path: String,
    pub cli_live: bool,
}

#[derive(Debug, Default)]
struct HostCredentialSignals {
    idx_target: Option<String>,
    idx_curr_email: Option<String>,
    idx_app: Option<String>,
    idx_app_email: Option<String>,
    idx_ide: Option<String>,
    idx_ide_email: Option<String>,
    idx_agy: Option<String>,
    idx_agy_email: Option<String>,
    idx_acp_email: Option<String>,
    ide_sqlite_email: Option<String>,
    ide_app_storage_email: Option<String>,
    keyring_resolved: Option<(Option<String>, String)>,
    standalone_resolved: Option<(Option<String>, String)>,
    app_storage_email: Option<String>,
    classic_sqlite_email: Option<String>,
    cli_dir_exists: bool,
    cli_exclusive_resolved: Option<(Option<String>, String)>,
    cli_fallback_resolved: Option<(Option<String>, String)>,
    acp_resolved: Option<(Option<String>, String)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_status() -> ActiveHostStatus {
        ActiveHostStatus {
            app_account: None,
            app_running: false,
            app_version: None,
            ide_account: None,
            ide_running: false,
            ide_version: None,
            plugin_account: None,
            plugin_running: false,
            agy_account: None,
            agy_running: false,
        }
    }

    #[test]
    fn app_v2_detects_account_when_last_target_was_ide_and_idx_app_is_none() {
        let mut status = empty_status();
        status.app_running = true;
        status.app_version = Some("2.35.2".to_string());
        status.ide_running = true;
        status.ide_version = Some("2.8.0".to_string());

        AntigravityManager::apply_host_credential_signals(
            &mut status,
            HostCredentialSignals {
                idx_target: Some("ide".to_string()),
                idx_ide: Some("id-c".to_string()),
                idx_ide_email: Some("cunioson@gmail.com".to_string()),
                ide_sqlite_email: Some("cunioson@gmail.com".to_string()),
                keyring_resolved: Some((
                    Some("id-c".to_string()),
                    "cunioson@gmail.com".to_string(),
                )),
                app_storage_email: Some("cunioson@gmail.com".to_string()),
                cli_dir_exists: true,
                acp_resolved: Some((Some("id-p".to_string()), "galalnine@gmail.com".to_string())),
                ..Default::default()
            },
        );

        assert_eq!(status.app_account.as_deref(), Some("cunioson@gmail.com"));
        assert_eq!(status.ide_account.as_deref(), Some("cunioson@gmail.com"));
        assert_eq!(status.agy_account.as_deref(), Some("cunioson@gmail.com"));
        assert_eq!(
            status.plugin_account.as_deref(),
            Some("galalnine@gmail.com")
        );
    }

    #[test]
    fn app_v2_keeps_own_account_when_ide_switches_elsewhere() {
        let mut status = empty_status();
        status.app_running = true;
        status.app_version = Some("2.35.2".to_string());
        status.ide_running = true;

        AntigravityManager::apply_host_credential_signals(
            &mut status,
            HostCredentialSignals {
                idx_target: Some("ide".to_string()),
                idx_app: Some("id-a".to_string()),
                idx_app_email: Some("app@gmail.com".to_string()),
                idx_ide: Some("id-b".to_string()),
                idx_ide_email: Some("ide@gmail.com".to_string()),
                ide_sqlite_email: Some("ide@gmail.com".to_string()),
                keyring_resolved: Some((Some("id-b".to_string()), "ide@gmail.com".to_string())),
                app_storage_email: Some("app@gmail.com".to_string()),
                ..Default::default()
            },
        );

        assert_eq!(status.app_account.as_deref(), Some("app@gmail.com"));
        assert_eq!(status.ide_account.as_deref(), Some("ide@gmail.com"));
    }

    #[test]
    fn app_not_installed_is_not_polluted_by_ide_or_cli_keyring() {
        let mut status = empty_status();
        status.ide_running = true;
        status.ide_version = Some("2.8.0".to_string());

        AntigravityManager::apply_host_credential_signals(
            &mut status,
            HostCredentialSignals {
                idx_target: Some("ide".to_string()),
                idx_ide: Some("id-b".to_string()),
                idx_ide_email: Some("ide@gmail.com".to_string()),
                ide_sqlite_email: Some("ide@gmail.com".to_string()),
                keyring_resolved: Some((Some("id-b".to_string()), "ide@gmail.com".to_string())),
                cli_dir_exists: false,
                ..Default::default()
            },
        );

        assert_eq!(status.app_account, None);
        assert_eq!(status.ide_account.as_deref(), Some("ide@gmail.com"));
        assert_eq!(status.agy_account, None);
    }

    #[test]
    fn app_detects_external_native_login_even_when_idx_target_is_stale_ide() {
        let mut status = empty_status();
        status.app_running = true;
        status.app_version = Some("2.35.2".to_string());

        AntigravityManager::apply_host_credential_signals(
            &mut status,
            HostCredentialSignals {
                idx_target: Some("ide".to_string()),
                idx_ide: Some("id-b".to_string()),
                idx_ide_email: Some("ide@gmail.com".to_string()),
                ide_sqlite_email: Some("ide@gmail.com".to_string()),
                keyring_resolved: Some((None, "external@gmail.com".to_string())),
                app_storage_email: Some("external@gmail.com".to_string()),
                ..Default::default()
            },
        );

        assert_eq!(status.app_account.as_deref(), Some("external@gmail.com"));
        assert_eq!(status.ide_account.as_deref(), Some("ide@gmail.com"));
    }

    #[test]
    fn app_falls_back_to_standalone_file_or_app_storage_when_keyring_unavailable() {
        let mut status = empty_status();
        status.app_running = true;
        status.app_version = Some("2.35.2".to_string());

        AntigravityManager::apply_host_credential_signals(
            &mut status,
            HostCredentialSignals {
                idx_target: Some("ide".to_string()),
                keyring_resolved: None,
                standalone_resolved: None,
                app_storage_email: Some("fallback@gmail.com".to_string()),
                ..Default::default()
            },
        );

        assert_eq!(status.app_account.as_deref(), Some("fallback@gmail.com"));
    }

    #[test]
    fn cli_prefers_exclusive_token_over_shared_keyring() {
        let mut status = empty_status();
        status.app_running = true;
        status.app_version = Some("2.35.2".to_string());

        AntigravityManager::apply_host_credential_signals(
            &mut status,
            HostCredentialSignals {
                idx_target: Some("classic".to_string()),
                keyring_resolved: Some((Some("id-app".to_string()), "app@gmail.com".to_string())),
                cli_dir_exists: true,
                cli_exclusive_resolved: Some((
                    Some("id-cli".to_string()),
                    "cli@gmail.com".to_string(),
                )),
                ..Default::default()
            },
        );

        assert_eq!(status.app_account.as_deref(), Some("app@gmail.com"));
        assert_eq!(status.agy_account.as_deref(), Some("cli@gmail.com"));
    }
}
