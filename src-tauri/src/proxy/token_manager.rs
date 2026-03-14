// 移除冗余的顶层导入，因为这些在代码中已由 full path 或局部导入处理
use dashmap::DashMap;
use std::collections::{HashSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::proxy::rate_limit::RateLimitTracker;
use crate::proxy::sticky_config::StickySessionConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnDiskAccountState {
    Enabled,
    Disabled,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ProxyToken {
    pub account_id: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub timestamp: i64,
    pub email: String,
    pub account_path: PathBuf, // 账号文件路径，用于更新
    pub project_id: Option<String>,
    pub subscription_tier: Option<String>, // "FREE" | "PRO" | "ULTRA"
    pub remaining_quota: Option<i32>,      // [FIX #563] Remaining quota for priority sorting
    pub protected_models: HashSet<String>, // [NEW #621]
    pub health_score: f32,                 // [NEW] 健康分数 (0.0 - 1.0)
    pub reset_time: Option<i64>,           // [NEW] 配额刷新时间戳（用于排序优化）
    pub validation_blocked: bool,          // [NEW] Check for validation block (VALIDATION_REQUIRED temporary block)
    pub validation_blocked_until: i64,     // [NEW] Timestamp until which the account is blocked
    pub validation_url: Option<String>,    // [NEW] Validation URL (#1522)
    pub model_quotas: HashMap<String, i32>, // [OPTIMIZATION] In-memory cache for model-specific quotas
    pub model_limits: HashMap<String, u64>, // [NEW] max_output_tokens per model from quota data
}

pub struct TokenManager {
    tokens: Arc<DashMap<String, ProxyToken>>, // account_id -> ProxyToken
    current_index: Arc<AtomicUsize>,
    last_used_account: Arc<tokio::sync::Mutex<Option<(String, std::time::Instant)>>>,
    data_dir: PathBuf,
    rate_limit_tracker: Arc<RateLimitTracker>, // 新增: 限流跟踪器
    sticky_config: Arc<tokio::sync::RwLock<StickySessionConfig>>, // 新增：调度配置
    session_accounts: Arc<DashMap<String, String>>, // 新增：会话与账号映射 (SessionID -> AccountID)
    preferred_account_id: Arc<tokio::sync::RwLock<Option<String>>>, // [FIX #820] 优先使用的账号ID（固定账号模式）
    health_scores: Arc<DashMap<String, f32>>,                       // account_id -> health_score
    circuit_breaker_config: Arc<tokio::sync::RwLock<crate::models::CircuitBreakerConfig>>, // [NEW] 熔断配置缓存
    /// 支持优雅关闭时主动 abort 后台任务
    auto_cleanup_handle: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    cancel_token: CancellationToken,
}

impl TokenManager {
    /// 创建新的 TokenManager
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            tokens: Arc::new(DashMap::new()),
            current_index: Arc::new(AtomicUsize::new(0)),
            last_used_account: Arc::new(tokio::sync::Mutex::new(None)),
            data_dir,
            rate_limit_tracker: Arc::new(RateLimitTracker::new()),
            sticky_config: Arc::new(tokio::sync::RwLock::new(StickySessionConfig::default())),
            session_accounts: Arc::new(DashMap::new()),
            preferred_account_id: Arc::new(tokio::sync::RwLock::new(None)), // [FIX #820]
            health_scores: Arc::new(DashMap::new()),
            circuit_breaker_config: Arc::new(tokio::sync::RwLock::new(
                crate::models::CircuitBreakerConfig::default(),
            )),
            auto_cleanup_handle: Arc::new(tokio::sync::Mutex::new(None)),
            cancel_token: CancellationToken::new(),
        }
    }

    /// 启动限流记录自动清理后台任务（每15秒检查并清除过期记录）
    pub async fn start_auto_cleanup(&self) {
        let tracker = self.rate_limit_tracker.clone();
        let cancel = self.cancel_token.child_token();

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        tracing::info!("Auto-cleanup task received cancel signal");
                        break;
                    }
                    _ = interval.tick() => {
                        let cleaned = tracker.cleanup_expired();
                        if cleaned > 0 {
                            tracing::info!(
                                "Auto-cleanup: Removed {} expired rate limit record(s)",
                                cleaned
                            );
                        }
                    }
                }
            }
        });

        // 先 abort 旧任务（防止任务泄漏），再存储新 handle
        let mut guard = self.auto_cleanup_handle.lock().await;
        if let Some(old) = guard.take() {
            old.abort();
            tracing::warn!("Aborted previous auto-cleanup task");
        }
        *guard = Some(handle);

        tracing::info!("Rate limit auto-cleanup task started (interval: 15s)");
    }

    /// 从主应用账号目录加载所有账号
    pub async fn load_accounts(&self) -> Result<usize, String> {
        let accounts_dir = self.data_dir.join("accounts");

        if !accounts_dir.exists() {
            return Err(format!("账号目录不存在: {:?}", accounts_dir));
        }

        // Reload should reflect current on-disk state (accounts can be added/removed/disabled).
        self.tokens.clear();
        self.current_index.store(0, Ordering::SeqCst);
        {
            let mut last_used = self.last_used_account.lock().await;
            *last_used = None;
        }

        let entries = std::fs::read_dir(&accounts_dir)
            .map_err(|e| format!("读取账号目录失败: {}", e))?;

        let mut count = 0;

        for entry in entries {
            let entry = entry.map_err(|e| format!("读取目录项失败: {}", e))?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            // 尝试加载账号
            match self.load_single_account(&path).await {
                Ok(Some(token)) => {
                    let account_id = token.account_id.clone();
                    self.tokens.insert(account_id, token);
                    count += 1;
                }
                Ok(None) => {
                    // 跳过无效账号
                }
                Err(e) => {
                    tracing::debug!("加载账号失败 {:?}: {}", path, e);
                }
            }
        }

        Ok(count)
    }

    /// 重新加载指定账号（用于配额更新后的实时同步）
    pub async fn reload_account(&self, account_id: &str) -> Result<(), String> {
        let path = self
            .data_dir
            .join("accounts")
            .join(format!("{}.json", account_id));
        if !path.exists() {
            return Err(format!("账号文件不存在: {:?}", path));
        }

        match self.load_single_account(&path).await {
            Ok(Some(token)) => {
                self.tokens.insert(account_id.to_string(), token);
                // [NEW] 重新加载账号时自动清除该账号的限流记录
                self.clear_rate_limit(account_id);
                Ok(())
            }
            Ok(None) => {
                // [FIX] 账号被禁用或不可用时，从内存池中彻底移除 (Issue #1565)
                // load_single_account returning None means the account should be skipped in its
                // current state (disabled / proxy_disabled / quota_protection / validation_blocked...).
                self.remove_account(account_id);
                Ok(())
            }
            Err(e) => Err(format!("同步账号失败: {}", e)),
        }
    }

    /// 重新加载所有账号
    pub async fn reload_all_accounts(&self) -> Result<usize, String> {
        let count = self.load_accounts().await?;
        // [NEW] 重新加载所有账号时自动清除所有限流记录
        self.clear_all_rate_limits();
        Ok(count)
    }

    /// 从内存中彻底移除指定账号及其关联数据 (Issue #1477)
    pub fn remove_account(&self, account_id: &str) {
        // ... (省略原有逻辑)
        if self.tokens.remove(account_id).is_some() {
            tracing::info!("[Proxy] Removed account {} from memory cache", account_id);
        }
        self.health_scores.remove(account_id);
        self.clear_rate_limit(account_id);
        self.session_accounts.retain(|_, v| v != account_id);
        if let Ok(mut preferred) = self.preferred_account_id.try_write() {
            if preferred.as_deref() == Some(account_id) {
                *preferred = None;
                tracing::info!("[Proxy] Cleared preferred account status for {}", account_id);
            }
        }
    }

    /// 根据账号 ID 获取完整的 ProxyToken 对象 (v4.1.29)
    pub fn get_token_by_id(&self, account_id: &str) -> Option<ProxyToken> {
        self.tokens.get(account_id).map(|t| t.clone())
    }

    /// Check if an account has been disabled on disk.
    ///
    /// Safety net: avoids selecting a disabled account when the in-memory pool hasn't been
    /// reloaded yet (e.g. fixed account mode / sticky session).
    ///
    /// Note: this is intentionally tolerant to transient read/parse failures (e.g. concurrent
    /// writes). Failures are reported as `Unknown` so callers can skip without purging the in-memory
    /// token pool.
    async fn get_account_state_on_disk(account_path: &std::path::PathBuf) -> OnDiskAccountState {
        const MAX_RETRIES: usize = 2;
        const RETRY_DELAY_MS: u64 = 5;

        for attempt in 0..=MAX_RETRIES {
            let content = match tokio::fs::read_to_string(account_path).await {
                Ok(c) => c,
                Err(e) => {
                    // If the file is gone, the in-memory token is definitely stale.
                    if e.kind() == std::io::ErrorKind::NotFound {
                        return OnDiskAccountState::Disabled;
                    }
                    if attempt < MAX_RETRIES {
                        tokio::time::sleep(std::time::Duration::from_millis(RETRY_DELAY_MS)).await;
                        continue;
                    }
                    tracing::debug!(
                        "Failed to read account file on disk {:?}: {}",
                        account_path,
                        e
                    );
                    return OnDiskAccountState::Unknown;
                }
            };

            let account = match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(v) => v,
                Err(e) => {
                    if attempt < MAX_RETRIES {
                        tokio::time::sleep(std::time::Duration::from_millis(RETRY_DELAY_MS)).await;
                        continue;
                    }
                    tracing::debug!(
                        "Failed to parse account JSON on disk {:?}: {}",
                        account_path,
                        e
                    );
                    return OnDiskAccountState::Unknown;
                }
            };

            let disabled = account
                .get("disabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
                || account
                    .get("proxy_disabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                || account
                    .get("quota")
                    .and_then(|q| q.get("is_forbidden"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

            return if disabled {
                OnDiskAccountState::Disabled
            } else {
                OnDiskAccountState::Enabled
            };
        }

        OnDiskAccountState::Unknown
    }

    /// 加载单个账号
    async fn load_single_account(&self, path: &PathBuf) -> Result<Option<ProxyToken>, String> {
        let content = std::fs::read_to_string(path).map_err(|e| format!("读取文件失败: {}", e))?;

        let mut account: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| format!("解析 JSON 失败: {}", e))?;

        // [修复 #1344] 先检查账号是否被手动禁用(非配额保护原因)
        let is_proxy_disabled = account
            .get("proxy_disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let disabled_reason = account
            .get("proxy_disabled_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if is_proxy_disabled && disabled_reason != "quota_protection" {
            // Account manually disabled
            tracing::debug!(
                "Account skipped due to manual disable: {:?} (email={}, reason={})",
                path,
                account
                    .get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>"),
                disabled_reason
            );
            return Ok(None);
        }

        // [NEW] Check for validation block (VALIDATION_REQUIRED temporary block)
        if account
            .get("validation_blocked")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            let block_until = account
                .get("validation_blocked_until")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            let now = chrono::Utc::now().timestamp();

            if now < block_until {
                // Still blocked
                tracing::debug!(
                    "Skipping validation-blocked account: {:?} (email={}, blocked until {})",
                    path,
                    account
                        .get("email")
                        .and_then(|v| v.as_str())
                        .unwrap_or("<unknown>"),
                    chrono::DateTime::from_timestamp(block_until, 0)
                        .map(|dt| dt.format("%H:%M:%S").to_string())
                        .unwrap_or_else(|| block_until.to_string())
                );
                return Ok(None);
            } else {
                // Block expired - clear it
                account["validation_blocked"] = serde_json::json!(false);
                account["validation_blocked_until"] = serde_json::json!(0);
                account["validation_blocked_reason"] = serde_json::Value::Null;

                let updated_json =
                    serde_json::to_string_pretty(&account).map_err(|e| e.to_string())?;
                std::fs::write(path, updated_json).map_err(|e| e.to_string())?;
                tracing::info!(
                    "Validation block expired and cleared for account: {}",
                    account
                        .get("email")
                        .and_then(|v| v.as_str())
                        .unwrap_or("<unknown>")
                );
            }
        }

        // 最终检查账号主开关
        if account
            .get("disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            tracing::debug!(
                "Skipping disabled account file: {:?} (email={})",
                path,
                account
                    .get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>")
            );
            return Ok(None);
        }

        // Safety check: verify state on disk again to handle concurrent mid-parse writes
        if Self::get_account_state_on_disk(path).await == OnDiskAccountState::Disabled {
            tracing::debug!("Account file {:?} is disabled on disk, skipping.", path);
            return Ok(None);
        }

        // 配额保护检查 - 只处理配额保护逻辑
        // 这样可以在加载时自动恢复配额已恢复的账号
        if self.check_and_protect_quota(&mut account, path).await {
            tracing::debug!(
                "Account skipped due to quota protection: {:?} (email={})",
                path,
                account
                    .get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>")
            );
            return Ok(None);
        }

        // [兼容性] 再次确认最终状态（可能被 check_and_protect_quota 修改）
        if account
            .get("proxy_disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            tracing::debug!(
                "Skipping proxy-disabled account file: {:?} (email={})",
                path,
                account
                    .get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>")
            );
            return Ok(None);
        }

        let account_id = account["id"].as_str()
            .ok_or("缺少 id 字段")?
            .to_string();

        let email = account["email"].as_str()
            .ok_or("缺少 email 字段")?
            .to_string();

        let token_obj = account["token"].as_object()
            .ok_or("缺少 token 字段")?;

        let access_token = token_obj["access_token"].as_str()
            .ok_or("缺少 access_token")?
            .to_string();

        let refresh_token = token_obj["refresh_token"].as_str()
            .ok_or("缺少 refresh_token")?
            .to_string();

        let expires_in = token_obj["expires_in"].as_i64()
            .ok_or("缺少 expires_in")?;

        let timestamp = token_obj["expiry_timestamp"].as_i64()
            .ok_or("缺少 expiry_timestamp")?;

        // project_id 是可选的
        let project_id = token_obj
            .get("project_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        // 【新增】提取订阅等级 (subscription_tier 为 "FREE" | "PRO" | "ULTRA")
        let subscription_tier = account
            .get("quota")
            .and_then(|q| q.get("subscription_tier"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // [FIX #563] 提取最大剩余配额百分比用于优先级排序 (Option<i32> now)
        let remaining_quota = account
            .get("quota")
            .and_then(|q| self.calculate_quota_stats(q));
            // .filter(|&r| r > 0); // 移除 >0 过滤，因为 0% 也是有效数据，只是优先级低

        // 【新增 #621】提取受限模型列表
        let protected_models: HashSet<String> = account
            .get("protected_models")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let health_score = self.health_scores.get(&account_id).map(|v| *v).unwrap_or(1.0);

        // [NEW] 提取最近的配额刷新时间（用于排序优化：刷新时间越近优先级越高）
        let reset_time = self.extract_earliest_reset_time(&account);

        // [OPTIMIZATION] 构建模型配额内存缓存，避免排序时读取磁盘
        let mut model_quotas = HashMap::new();
        // [NEW] 构建模型输出限额内存缓存 (max_output_tokens)
        let mut model_limits: HashMap<String, u64> = HashMap::new();
        if let Some(models) = account.get("quota").and_then(|q| q.get("models")).and_then(|m| m.as_array()) {
            for model in models {
                if let (Some(name), Some(pct)) = (model.get("name").and_then(|v| v.as_str()), model.get("percentage").and_then(|v| v.as_i64())) {
                    // Normalize name to standard ID
                    let standard_id = crate::proxy::common::model_mapping::normalize_to_standard_id(name)
                        .unwrap_or_else(|| name.to_string());
                    model_quotas.insert(standard_id, pct as i32);
                }
                // [NEW] 解析并缓存 max_output_tokens (按原始 model name，不归一化)
                if let (Some(name), Some(limit)) = (
                    model.get("name").and_then(|v| v.as_str()),
                    model.get("max_output_tokens").and_then(|v| v.as_u64()),
                ) {
                    model_limits.insert(name.to_string(), limit);
                }
            }
        }

        // [NEW] 启动时自动同步持久化的淘汰模型路由表，注入热更新拦截器
        if let Some(rules) = account.get("quota").and_then(|q| q.get("model_forwarding_rules")).and_then(|r| r.as_object()) {
            for (k, v) in rules {
                if let Some(new_model) = v.as_str() {
                    crate::proxy::common::model_mapping::update_dynamic_forwarding_rules(
                        k.to_string(),
                        new_model.to_string()
                    );
                }
            }
        }

        Ok(Some(ProxyToken {
            account_id,
            access_token,
            refresh_token,
            expires_in,
            timestamp,
            email,
            account_path: path.clone(),
            project_id,
            subscription_tier,
            remaining_quota,
            protected_models,
            health_score,
            reset_time,
            validation_blocked: account.get("validation_blocked").and_then(|v| v.as_bool()).unwrap_or(false),
            validation_blocked_until: account.get("validation_blocked_until").and_then(|v| v.as_i64()).unwrap_or(0),
            validation_url: account.get("validation_url").and_then(|v| v.as_str()).map(|s| s.to_string()),
            model_quotas,
            model_limits,
        }))
    }

    /// 检查账号是否应该被配额保护
    /// 如果配额低于阈值，自动禁用账号并返回 true
    async fn check_and_protect_quota(
        &self,
        account_json: &mut serde_json::Value,
        account_path: &PathBuf,
    ) -> bool {
        // 1. 加载配额保护配置
        let config = match crate::modules::config::load_app_config() {
            Ok(cfg) => cfg.quota_protection,
            Err(_) => return false, // 配置加载失败，跳过保护
        };

        if !config.enabled {
            return false; // 配额保护未启用
        }

        // 2. 获取配额信息
        // 注意：我们需要 clone 配额信息来遍历，避免借用冲突，但修改是针对 account_json 的
        let quota = match account_json.get("quota") {
            Some(q) => q.clone(),
            None => return false, // 无配额信息，跳过
        };

        // 3. [兼容性 #621] 检查是否被旧版账号级配额保护禁用,尝试恢复并转为模型级
        let is_proxy_disabled = account_json
            .get("proxy_disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let reason = account_json.get("proxy_disabled_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if is_proxy_disabled && reason == "quota_protection" {
            // 如果是被旧版账号级保护禁用的,尝试恢复并转为模型级
            return self
                .check_and_restore_quota(account_json, account_path, &quota, &config)
                .await;
        }

        // [修复 #1344] 不再处理其他禁用原因,让调用方负责检查手动禁用

        // 4. 获取模型列表
        let models = match quota.get("models").and_then(|m| m.as_array()) {
            Some(m) => m,
            None => return false,
        };

        // 5. [重构] 聚合判定逻辑：按 Standard ID 对账号所有型号进行分组
        // 解决如 Pro-Low (0%) 和 Pro-High (100%) 在同一账号内导致状态冲突的问题
        let mut group_min_percentage: HashMap<String, i32> = HashMap::new();

        for model in models {
            let name = model.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let percentage = model.get("percentage").and_then(|v| v.as_i64()).unwrap_or(100) as i32;

            if let Some(std_id) = crate::proxy::common::model_mapping::normalize_to_standard_id(name) {
                let entry = group_min_percentage.entry(std_id).or_insert(100);
                if percentage < *entry {
                    *entry = percentage;
                }
            }
        }

        // 6. 遍历受监控的 Standard ID，根据组内“最差状态”执行锁定或恢复
        let threshold = config.threshold_percentage as i32;
        let account_id = account_json
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let mut changed = false;

        for std_id in &config.monitored_models {
            // 获取该组的最低百分比，如果账号没该组型号则视为 100%
            let min_pct = group_min_percentage.get(std_id).cloned().unwrap_or(100);

            if min_pct <= threshold {
                // 只要组内有一个不行，触发全组保护
                if self
                    .trigger_quota_protection(
                        account_json,
                        &account_id,
                        account_path,
                        min_pct,
                        threshold,
                        std_id,
                    )
                    .await
                    .unwrap_or(false)
                {
                    changed = true;
                }
            } else {
                // 只有全组都好（或者没这型号），才尝试从之前受限状态恢复
                let protected_models = account_json
                    .get("protected_models")
                    .and_then(|v| v.as_array());
                
                let is_protected = protected_models.map_or(false, |arr| {
                    arr.iter().any(|m| m.as_str() == Some(std_id as &str))
                });

                if is_protected {
                    if self
                        .restore_quota_protection(
                            account_json,
                            &account_id,
                            account_path,
                            std_id,
                        )
                        .await
                        .unwrap_or(false)
                    {
                        changed = true;
                    }
                }
            }
        }

        let _ = changed; // 避免 unused 警告，如果后续逻辑需要可以继续使用

        // 我们不再因为配额原因返回 true（即不再跳过账号），
        // 而是加载并在 get_token 时进行过滤。
        false
    }

    /// 计算账号的最大剩余配额百分比（用于排序）
    /// 返回值: Option<i32> (max_percentage)
    fn calculate_quota_stats(&self, quota: &serde_json::Value) -> Option<i32> {
        let models = match quota.get("models").and_then(|m| m.as_array()) {
            Some(m) => m,
            None => return None,
        };

        let mut max_percentage = 0;
        let mut has_data = false;

        for model in models {
            if let Some(pct) = model.get("percentage").and_then(|v| v.as_i64()) {
                let pct_i32 = pct as i32;
                if pct_i32 > max_percentage {
                    max_percentage = pct_i32;
                }
                has_data = true;
            }
        }

        if has_data {
            Some(max_percentage)
        } else {
            None
        }
    }

    /// 从磁盘读取特定模型的 quota 百分比 [FIX] 排序使用目标模型的 quota 而非 max
    ///
    /// # 参数
    /// * `account_path` - 账号 JSON 文件路径
    /// * `model_name` - 目标模型名称（已标准化）
    #[allow(dead_code)] // 预留给精确配额读取逻辑
    fn get_model_quota_from_json(account_path: &PathBuf, model_name: &str) -> Option<i32> {
        let content = std::fs::read_to_string(account_path).ok()?;
        let account: serde_json::Value = serde_json::from_str(&content).ok()?;
        let models = account.get("quota")?.get("models")?.as_array()?;

        for model in models {
            if let Some(name) = model.get("name").and_then(|v| v.as_str()) {
                if crate::proxy::common::model_mapping::normalize_to_standard_id(name)
                    .unwrap_or_else(|| name.to_string())
                    == model_name
                {
                    return model
                        .get("percentage")
                        .and_then(|v| v.as_i64())
                        .map(|p| p as i32);
                }
            }
        }
        None
    }

    fn get_available_models_from_json(account_path: &PathBuf) -> Option<HashSet<String>> {
        let content = std::fs::read_to_string(account_path).ok()?;
        let account: serde_json::Value = serde_json::from_str(&content).ok()?;
        let models = account.get("quota")?.get("models")?.as_array()?;
        let mut result = HashSet::new();
        for model in models {
            if let Some(name) = model.get("name").and_then(|v| v.as_str()) {
                let normalized = name.trim().to_lowercase();
                if !normalized.is_empty() {
                    result.insert(normalized);
                }
            }
        }
        Some(result)
    }

    fn build_dynamic_model_candidates(model_name: &str) -> Option<Vec<String>> {
        let model = model_name.trim().to_lowercase();
        if model.is_empty() {
            return None;
        }

        let pro_family = [
            "gemini-3-pro",
            "gemini-3-pro-preview",
            "gemini-3-pro-high",
            "gemini-3-pro-low",
            "gemini-3.1-pro",
            "gemini-3.1-pro-preview",
            "gemini-3.1-pro-high",
            "gemini-3.1-pro-low",
        ];

        if !pro_family.contains(&model.as_str()) {
            return None;
        }

        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut push = |candidate: &str| {
            let c = candidate.to_string();
            if seen.insert(c.clone()) {
                out.push(c);
            }
        };

        // Keep requested model as top priority, then fallback across the same family.
        push(&model);
        push("gemini-3.1-pro-preview");
        push("gemini-3-pro-preview");
        push("gemini-3.1-pro-high");
        push("gemini-3-pro-high");
        push("gemini-3.1-pro-low");
        push("gemini-3-pro-low");

        Some(out)
    }

    pub async fn resolve_dynamic_model_for_account(
        &self,
        account_id: &str,
        mapped_model: &str,
    ) -> String {
        let candidates = match Self::build_dynamic_model_candidates(mapped_model) {
            Some(c) => c,
            None => return mapped_model.to_string(),
        };

        let account_path = match self.tokens.get(account_id) {
            Some(token) => token.account_path.clone(),
            None => return mapped_model.to_string(),
        };

        let available_models = match Self::get_available_models_from_json(&account_path) {
            Some(models) if !models.is_empty() => models,
            _ => return mapped_model.to_string(),
        };

        for candidate in candidates {
            if available_models.contains(&candidate) {
                if candidate != mapped_model.to_lowercase() {
                    tracing::info!(
                        "[Dynamic-Model-Rewrite] account={} {} -> {}",
                        account_id,
                        mapped_model,
                        candidate
                    );
                }
                return candidate;
            }
        }

        mapped_model.to_string()
    }

    /// 测试辅助函数：公开访问 get_model_quota_from_json
    #[cfg(test)]
    pub fn get_model_quota_from_json_for_test(account_path: &PathBuf, model_name: &str) -> Option<i32> {
        Self::get_model_quota_from_json(account_path, model_name)
    }

    /// 触发配额保护，限制特定模型 (Issue #621)
    /// 返回 true 如果发生了改变
    async fn trigger_quota_protection(
        &self,
        account_json: &mut serde_json::Value,
        account_id: &str,
        account_path: &PathBuf,
        current_val: i32,
        threshold: i32,
        model_name: &str,
    ) -> Result<bool, String> {
        // 1. 初始化 protected_models 数组（如果不存在）
        if account_json.get("protected_models").is_none() {
            account_json["protected_models"] = serde_json::Value::Array(Vec::new());
        }

        let protected_models = account_json["protected_models"].as_array_mut().unwrap();

        // 2. 检查是否已存在
        if !protected_models
            .iter()
            .any(|m| m.as_str() == Some(model_name))
        {
            protected_models.push(serde_json::Value::String(model_name.to_string()));

            tracing::info!(
                "账号 {} 的模型 {} 因配额受限（{}% <= {}%）已被加入保护列表",
                account_id,
                model_name,
                current_val,
                threshold
            );

            // 3. 写入磁盘
            std::fs::write(account_path, serde_json::to_string_pretty(account_json).unwrap())
                .map_err(|e| format!("写入文件失败: {}", e))?;

            // [FIX] 触发 TokenManager 的账号重新加载信号，确保内存中的 protected_models 同步
            crate::proxy::server::trigger_account_reload(account_id);

            return Ok(true);
        }

        Ok(false)
    }

    /// 检查并从账号级保护恢复（迁移至模型级，Issue #621）
    async fn check_and_restore_quota(
        &self,
        account_json: &mut serde_json::Value,
        account_path: &PathBuf,
        quota: &serde_json::Value,
        config: &crate::models::QuotaProtectionConfig,
    ) -> bool {
        // [兼容性] 如果该账号当前处于 proxy_disabled=true 且原因是 quota_protection，
        // 我们将其 proxy_disabled 设为 false，但同时更新其 protected_models 列表。
        tracing::info!(
            "正在迁移账号 {} 从全局配额保护模式至模型级保护模式",
            account_json
                .get("email")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
        );

        account_json["proxy_disabled"] = serde_json::Value::Bool(false);
        account_json["proxy_disabled_reason"] = serde_json::Value::Null;
        account_json["proxy_disabled_at"] = serde_json::Value::Null;

        let threshold = config.threshold_percentage as i32;
        let mut protected_list = Vec::new();

        if let Some(models) = quota.get("models").and_then(|m| m.as_array()) {
            for model in models {
                let name = model.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if !config.monitored_models.iter().any(|m| m == name) { continue; }

                let percentage = model.get("percentage").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                if percentage <= threshold {
                    protected_list.push(serde_json::Value::String(name.to_string()));
                }
            }
        }

        account_json["protected_models"] = serde_json::Value::Array(protected_list);

        let _ = std::fs::write(account_path, serde_json::to_string_pretty(account_json).unwrap());

        false // 返回 false 表示现在已可以尝试加载该账号（模型级过滤会在 get_token 时发生）
    }

    /// 恢复特定模型的配额保护 (Issue #621)
    /// 返回 true 如果发生了改变
    async fn restore_quota_protection(
        &self,
        account_json: &mut serde_json::Value,
        account_id: &str,
        account_path: &PathBuf,
        model_name: &str,
    ) -> Result<bool, String> {
        if let Some(arr) = account_json
            .get_mut("protected_models")
            .and_then(|v| v.as_array_mut())
        {
            let original_len = arr.len();
            arr.retain(|m| m.as_str() != Some(model_name));

            if arr.len() < original_len {
                tracing::info!(
                    "账号 {} 的模型 {} 配额已恢复，移出保护列表",
                    account_id,
                    model_name
                );
                std::fs::write(
                    account_path,
                    serde_json::to_string_pretty(account_json).unwrap(),
                )
                .map_err(|e| format!("写入文件失败: {}", e))?;
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// P2C 算法的候选池大小 - 从前 N 个最优候选中随机选择
    const P2C_POOL_SIZE: usize = 5;

    /// Power of 2 Choices (P2C) 选择算法
    /// 从前 5 个候选中随机选 2 个，选择配额更高的 -> 避免热点
    /// 返回选中的索引
    ///
    /// # 参数
    /// * `candidates` - 已排序的候选 token 列表
    /// * `attempted` - 已尝试失败的账号 ID 集合
    /// * `normalized_target` - 归一化后的目标模型名
    /// * `quota_protection_enabled` - 是否启用配额保护
    fn select_with_p2c<'a>(
        &self,
        candidates: &'a [ProxyToken],
        attempted: &HashSet<String>,
        normalized_target: &str,
        quota_protection_enabled: bool,
    ) -> Option<&'a ProxyToken> {
        use rand::Rng;

        // 过滤可用 token
        let available: Vec<&ProxyToken> = candidates.iter()
            .filter(|t| !attempted.contains(&t.account_id))
            .filter(|t| !quota_protection_enabled || !t.protected_models.contains(normalized_target))
            .collect();

        if available.is_empty() { return None; }
        if available.len() == 1 { return Some(available[0]); }

        // P2C: 从前 min(P2C_POOL_SIZE, len) 个中随机选 2 个
        let pool_size = available.len().min(Self::P2C_POOL_SIZE);
        let mut rng = rand::thread_rng();

        let pick1 = rng.gen_range(0..pool_size);
        let pick2 = rng.gen_range(0..pool_size);
        // 确保选择不同的两个候选
        let pick2 = if pick2 == pick1 {
            (pick1 + 1) % pool_size
        } else {
            pick2
        };

        let c1 = available[pick1];
        let c2 = available[pick2];

        // 选择配额更高的
        let selected = if c1.remaining_quota.unwrap_or(0) >= c2.remaining_quota.unwrap_or(0) {
            c1
        } else {
            c2
        };

        tracing::debug!(
            "🎲 [P2C] Selected {} ({}%) from [{}({}%), {}({}%)]",
            selected.email, selected.remaining_quota.unwrap_or(0),
            c1.email, c1.remaining_quota.unwrap_or(0),
            c2.email, c2.remaining_quota.unwrap_or(0)
        );

        Some(selected)
    }

    /// 先发送取消信号，再带超时等待任务完成
    ///
    /// # 参数
    /// * `timeout` - 等待任务完成的超时时间
    pub async fn graceful_shutdown(&self, timeout: std::time::Duration) {
        tracing::info!("Initiating graceful shutdown of background tasks...");

        // 发送取消信号给所有后台任务
        self.cancel_token.cancel();

        // 带超时等待任务完成
        match tokio::time::timeout(timeout, self.abort_background_tasks()).await {
            Ok(_) => tracing::info!("All background tasks cleaned up gracefully"),
            Err(_) => tracing::warn!("Graceful cleanup timed out after {:?}, tasks were force-aborted", timeout),
        }
    }

    /// 中止并等待所有后台任务完成
    /// abort() 仅设置取消标志，必须 await 确认清理完成
    pub async fn abort_background_tasks(&self) {
        Self::abort_task(&self.auto_cleanup_handle, "Auto-cleanup task").await;
    }

    /// 中止单个后台任务并记录结果
    ///
    /// # 参数
    /// * `handle` - 任务句柄的 Mutex 引用
    /// * `task_name` - 任务名称（用于日志）
    async fn abort_task(
        handle: &tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
        task_name: &str,
    ) {
        let Some(handle) = handle.lock().await.take() else {
            return;
        };

        handle.abort();
        match handle.await {
            Ok(()) => tracing::debug!("{} completed", task_name),
            Err(e) if e.is_cancelled() => tracing::info!("{} aborted", task_name),
            Err(e) => tracing::warn!("{} error: {}", task_name, e),
        }
    }

    /// 获取当前可用的 Token（支持粘性会话与智能调度）
    /// 参数 `quota_group` 用于区分 "claude" vs "gemini" 组
    /// 参数 `force_rotate` 为 true 时将忽略锁定，强制切换账号
    /// 参数 `session_id` 用于跨请求维持会话粘性
    /// 参数 `target_model` 用于检查配额保护 (Issue #621)
    pub async fn get_token(
        &self,
        quota_group: &str,
        force_rotate: bool,
        session_id: Option<&str>,
        target_model: &str,
    ) -> Result<(String, String, String, String, u64), String> {
        // [FIX] 检查并处理待重新加载的账号（配额保护同步）
        let pending_reload = crate::proxy::server::take_pending_reload_accounts();
        for account_id in pending_reload {
            if let Err(e) = self.reload_account(&account_id).await {
                tracing::warn!("[Quota] Failed to reload account {}: {}", account_id, e);
            } else {
                tracing::info!(
                    "[Quota] Reloaded account {} (protected_models synced)",
                    account_id
                );
            }
        }

        // [FIX #1477] 检查并处理待删除的账号（彻底清理缓存）
        let pending_delete = crate::proxy::server::take_pending_delete_accounts();
        for account_id in pending_delete {
            self.remove_account(&account_id);
            tracing::info!(
                "[Proxy] Purged deleted account {} from all caches",
                account_id
            );
        }

        // 【优化 Issue #284】添加 5 秒超时，防止死锁
        let timeout_duration = std::time::Duration::from_secs(5);
        match tokio::time::timeout(
            timeout_duration,
            self.get_token_internal(quota_group, force_rotate, session_id, target_model),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(
                "Token acquisition timeout (5s) - system too busy or deadlock detected".to_string(),
            ),
        }
    }

    /// 内部实现：获取 Token 的核心逻辑
    async fn get_token_internal(
        &self,
        quota_group: &str,
        force_rotate: bool,
        session_id: Option<&str>,
        target_model: &str,
    ) -> Result<(String, String, String, String, u64), String> {
        let mut tokens_snapshot: Vec<ProxyToken> =
            self.tokens.iter().map(|e| e.value().clone()).collect();
        let mut total = tokens_snapshot.len();
        if total == 0 {
            return Err("Token pool is empty".to_string());
        }

        // [NEW] 1. 动态能力过滤 (Capability Filter)
        
        // 定义常量
        const RESET_TIME_THRESHOLD_SECS: i64 = 600; // 10 分钟阈值

        // 归一化目标模型名为标准 ID
        let normalized_target = crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
            .unwrap_or_else(|| target_model.to_string());

        // 仅保留明确拥有该模型配额的账号
        // 这一步确保了 "保证有模型才可以进入轮询"，特别是对 Opus 4.6 等高端模型
        let candidate_count_before = tokens_snapshot.len();
        
        // 此处假设所有受支持的模型都会出现在 model_quotas 中
        // 如果 API 返回的配额信息不完整，可能会导致误杀，但为了严格性，我们执行此过滤
        tokens_snapshot.retain(|t| t.model_quotas.contains_key(&normalized_target));

        if tokens_snapshot.is_empty() {
            if candidate_count_before > 0 {
                // 如果过滤前有账号，过滤后没了，说明所有账号都没有该模型的配额
                tracing::warn!("No accounts have satisfied quota for model: {}", normalized_target);
                return Err(format!("No accounts available with quota for model: {}", normalized_target));
            }
            return Err("Token pool is empty".to_string());
        }

        tokens_snapshot.sort_by(|a, b| {
            // Priority 0: 严格的订阅等级排序 (ULTRA > PRO > FREE)
            // 用户要求：轮询应当遵循 Ultra -> Pro -> Free
            // 既然已经过滤掉了不支持该模型的账号，剩下的都是支持的
            // 此时我们优先使用高级订阅
            let tier_priority = |tier: &Option<String>| {
                let t = tier.as_deref().unwrap_or("").to_lowercase();
                if t.contains("ultra") { 0 }
                else if t.contains("pro") { 1 }
                else if t.contains("free") { 2 }
                else { 3 }
            };

            let tier_cmp = tier_priority(&a.subscription_tier)
                .cmp(&tier_priority(&b.subscription_tier));
            if tier_cmp != std::cmp::Ordering::Equal {
                return tier_cmp;
            }

            // Priority 1: 目标模型的 quota (higher is better) -> 保护低配额账号
            // 经过过滤，key 肯定存在
            let quota_a = a.model_quotas.get(&normalized_target).copied().unwrap_or(0);
            let quota_b = b.model_quotas.get(&normalized_target).copied().unwrap_or(0);

            let quota_cmp = quota_b.cmp(&quota_a);
            if quota_cmp != std::cmp::Ordering::Equal {
                return quota_cmp;
            }

            // Priority 2: Health score (higher is better)
            let health_cmp = b.health_score.partial_cmp(&a.health_score)
                .unwrap_or(std::cmp::Ordering::Equal);
            if health_cmp != std::cmp::Ordering::Equal {
                return health_cmp;
            }

            // Priority 3: Reset time (earlier is better, but only if diff > 10 min)
            let reset_a = a.reset_time.unwrap_or(i64::MAX);
            let reset_b = b.reset_time.unwrap_or(i64::MAX);
            if (reset_a - reset_b).abs() >= RESET_TIME_THRESHOLD_SECS {
                reset_a.cmp(&reset_b)
            } else {
                std::cmp::Ordering::Equal
            }
        });

        // 【调试日志】打印排序后的账号顺序（显示目标模型的 quota）
        tracing::debug!(
            "🔄 [Token Rotation] target={} Accounts: {:?}",
            normalized_target,
            tokens_snapshot.iter().map(|t| format!(
                "{}(quota={}%, reset={:?}, health={:.2})",
                t.email,
                t.model_quotas.get(&normalized_target).copied().unwrap_or(0),
                t.reset_time.map(|ts| {
                    let now = chrono::Utc::now().timestamp();
                    let diff_secs = ts - now;
                    if diff_secs > 0 {
                        format!("{}m", diff_secs / 60)
                    } else {
                        "now".to_string()
                    }
                }),
                t.health_score
            )).collect::<Vec<_>>()
        );

        // 0. 读取当前调度配置
        let scheduling = self.sticky_config.read().await.clone();
        use crate::proxy::sticky_config::SchedulingMode;

        // 【新增】检查配额保护是否启用（如果关闭，则忽略 protected_models 检查）
        let quota_protection_enabled = crate::modules::config::load_app_config()
            .map(|cfg| cfg.quota_protection.enabled)
            .unwrap_or(false);

        // ===== [FIX #820] 固定账号模式：优先使用指定账号 =====
        let preferred_id = self.preferred_account_id.read().await.clone();
        if let Some(ref pref_id) = preferred_id {
            // 查找优先账号
            if let Some(preferred_token) = tokens_snapshot
                .iter()
                .find(|t| &t.account_id == pref_id)
                .cloned()
            {
                // 检查账号是否可用（未限流、未被配额保护）
                match Self::get_account_state_on_disk(&preferred_token.account_path).await {
                    OnDiskAccountState::Disabled => {
                        tracing::warn!(
                            "🔒 [FIX #820] Preferred account {} is disabled on disk, purging and falling back",
                            preferred_token.email
                        );
                        self.remove_account(&preferred_token.account_id);
                        tokens_snapshot.retain(|t| t.account_id != preferred_token.account_id);
                        total = tokens_snapshot.len();

                        {
                            let mut preferred = self.preferred_account_id.write().await;
                            if preferred.as_deref() == Some(pref_id.as_str()) {
                                *preferred = None;
                            }
                        }

                        if total == 0 {
                            return Err("Token pool is empty".to_string());
                        }
                    }
                    OnDiskAccountState::Unknown => {
                        tracing::warn!(
                            "🔒 [FIX #820] Preferred account {} state on disk is unavailable, falling back",
                            preferred_token.email
                        );
                        // Don't purge on transient read/parse failures; just skip this token for this request.
                        tokens_snapshot.retain(|t| t.account_id != preferred_token.account_id);
                        total = tokens_snapshot.len();
                        if total == 0 {
                            return Err("Token pool is empty".to_string());
                        }
                    }
                    OnDiskAccountState::Enabled => {
                        let normalized_target =
                            crate::proxy::common::model_mapping::normalize_to_standard_id(
                                target_model,
                            )
                            .unwrap_or_else(|| target_model.to_string());

                let is_rate_limited = self
                    .is_rate_limited(&preferred_token.account_id, Some(&normalized_target))
                    .await;
                let is_quota_protected = quota_protection_enabled
                    && preferred_token
                        .protected_models
                        .contains(&normalized_target);

                if !is_rate_limited && !is_quota_protected {
                    tracing::info!(
                        "🔒 [FIX #820] Using preferred account: {} (fixed mode)",
                        preferred_token.email
                    );

                    // 直接使用优先账号，跳过轮询逻辑
                    let mut token = preferred_token.clone();

                    // 检查 token 是否过期（提前5分钟刷新）
                    let now = chrono::Utc::now().timestamp();
                    if now >= token.timestamp - 300 {
                        tracing::debug!("账号 {} 的 token 即将过期，正在刷新...", token.email);
                        match crate::modules::oauth::refresh_access_token(&token.refresh_token, Some(&token.account_id))
                            .await
                        {
                            Ok(token_response) => {
                                token.access_token = token_response.access_token.clone();
                                token.expires_in = token_response.expires_in;
                                token.timestamp = now + token_response.expires_in;

                                if let Some(mut entry) = self.tokens.get_mut(&token.account_id) {
                                    entry.access_token = token.access_token.clone();
                                    entry.expires_in = token.expires_in;
                                    entry.timestamp = token.timestamp;
                                }
                                let _ = self
                                    .save_refreshed_token(&token.account_id, &token_response)
                                    .await;
                            }
                            Err(e) => {
                                tracing::warn!("Preferred account token refresh failed: {}", e);
                                // 继续使用旧 token，让后续逻辑处理失败
                            }
                        }
                    }

                    // 确保有 project_id (filter empty strings to trigger re-fetch)
                    let project_id = if let Some(pid) = &token.project_id {
                        if pid.is_empty() { None } else { Some(pid.clone()) }
                    } else {
                        None
                    };
                    let project_id = if let Some(pid) = project_id {
                        pid
                    } else {
                        match crate::proxy::project_resolver::fetch_project_id(&token.access_token)
                            .await
                        {
                            Ok(pid) => {
                                if let Some(mut entry) = self.tokens.get_mut(&token.account_id) {
                                    entry.project_id = Some(pid.clone());
                                }
                                let _ = self.save_project_id(&token.account_id, &pid).await;
                                pid
                            }
                            Err(_) => "bamboo-precept-lgxtn".to_string(), // fallback
                        }
                    };

                    return Ok((token.access_token, project_id, token.email, token.account_id, 0));
                } else {
                    if is_rate_limited {
                        tracing::warn!("🔒 [FIX #820] Preferred account {} is rate-limited, falling back to round-robin", preferred_token.email);
                    } else {
                        tracing::warn!("🔒 [FIX #820] Preferred account {} is quota-protected for {}, falling back to round-robin", preferred_token.email, target_model);
                    }
                }
                    }
                }
            } else {
                tracing::warn!("🔒 [FIX #820] Preferred account {} not found in pool, falling back to round-robin", pref_id);
            }
        }
        // ===== [END FIX #820] =====

        // 【优化 Issue #284】将锁操作移到循环外，避免重复获取锁
        // 预先获取 last_used_account 的快照，避免在循环中多次加锁
        let last_used_account_id = if quota_group != "image_gen" {
            let last_used = self.last_used_account.lock().await;
            last_used.clone()
        } else {
            None
        };

        let mut attempted: HashSet<String> = HashSet::new();
        let mut last_error: Option<String> = None;
        let mut need_update_last_used: Option<(String, std::time::Instant)> = None;

        for attempt in 0..total {
            let rotate = force_rotate || attempt > 0;

            // ===== 【核心】粘性会话与智能调度逻辑 =====
            let mut target_token: Option<ProxyToken> = None;

            // 归一化目标模型名为标准 ID，用于配额保护检查
            let normalized_target = crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());

            // 模式 A: 粘性会话处理 (CacheFirst 或 Balance 且有 session_id)
            if !rotate
                && session_id.is_some()
                && scheduling.mode != SchedulingMode::PerformanceFirst
            {
                let sid = session_id.unwrap();

                // 1. 检查会话是否已绑定账号
                if let Some(bound_id) = self.session_accounts.get(sid).map(|v| v.clone()) {
                    // 【修复】先通过 account_id 找到对应的账号，获取其 email
                    // 2. 转换 email -> account_id 检查绑定的账号是否限流
                    if let Some(bound_token) =
                        tokens_snapshot.iter().find(|t| t.account_id == bound_id)
                    {
                        let key = self
                            .email_to_account_id(&bound_token.email)
                            .unwrap_or_else(|| bound_token.account_id.clone());
                        // [FIX] Pass None for specific model wait time if not applicable
                        let reset_sec = self.rate_limit_tracker.get_remaining_wait(&key, None);
                        if reset_sec > 0 {
                            // 【修复 Issue #284】立即解绑并切换账号，不再阻塞等待
                            // 原因：阻塞等待会导致并发请求时客户端 socket 超时 (UND_ERR_SOCKET)
                            tracing::debug!(
                                "Sticky Session: Bound account {} is rate-limited ({}s), unbinding and switching.",
                                bound_token.email, reset_sec
                            );
                            self.session_accounts.remove(sid);
                        } else if !attempted.contains(&bound_id)
                            && !(quota_protection_enabled
                                && bound_token.protected_models.contains(&normalized_target))
                        {
                            // 3. 账号可用且未被标记为尝试失败，优先复用
                            tracing::debug!("Sticky Session: Successfully reusing bound account {} for session {}", bound_token.email, sid);
                            target_token = Some(bound_token.clone());
                        } else if quota_protection_enabled
                            && bound_token.protected_models.contains(&normalized_target)
                        {
                            tracing::debug!("Sticky Session: Bound account {} is quota-protected for model {} [{}], unbinding and switching.", bound_token.email, normalized_target, target_model);
                            self.session_accounts.remove(sid);
                        }
                    } else {
                        // 绑定的账号已不存在（可能被删除），解绑
                        tracing::debug!(
                            "Sticky Session: Bound account not found for session {}, unbinding",
                            sid
                        );
                        self.session_accounts.remove(sid);
                    }
                }
            }

            // 模式 B: 原子化 60s 全局锁定 (针对无 session_id 情况的默认保护)
            // 【修复】性能优先模式应跳过 60s 锁定；
            if target_token.is_none()
                && !rotate
                && quota_group != "image_gen"
                && scheduling.mode != SchedulingMode::PerformanceFirst
            {
                // 【优化】使用预先获取的快照，不再在循环内加锁
                if let Some((account_id, last_time)) = &last_used_account_id {
                    // [FIX #3] 60s 锁定逻辑应检查 `attempted` 集合，避免重复尝试失败的账号
                    if last_time.elapsed().as_secs() < 60 && !attempted.contains(account_id) {
                        if let Some(found) =
                            tokens_snapshot.iter().find(|t| &t.account_id == account_id)
                        {
                            // 【修复】检查限流状态和配额保护，避免复用已被锁定的账号
                            if !self
                                .is_rate_limited(&found.account_id, Some(&normalized_target))
                                .await
                                && !(quota_protection_enabled
                                    && found.protected_models.contains(&normalized_target))
                            {
                                tracing::debug!(
                                    "60s Window: Force reusing last account: {}",
                                    found.email
                                );
                                target_token = Some(found.clone());
                            } else {
                                if self
                                    .is_rate_limited(&found.account_id, Some(&normalized_target))
                                    .await
                                {
                                    tracing::debug!(
                                        "60s Window: Last account {} is rate-limited, skipping",
                                        found.email
                                    );
                                } else {
                                    tracing::debug!("60s Window: Last account {} is quota-protected for model {} [{}], skipping", found.email, normalized_target, target_model);
                                }
                            }
                        }
                    }
                }

                // 若无锁定，则使用 P2C 选择账号 (避免热点问题)
                if target_token.is_none() {
                    // 先过滤出未限流的账号
                    let mut non_limited: Vec<ProxyToken> = Vec::new();
                    for t in &tokens_snapshot {
                        if !self.is_rate_limited(&t.account_id, Some(&normalized_target)).await {
                            non_limited.push(t.clone());
                        }
                    }

                    if let Some(selected) = self.select_with_p2c(
                        &non_limited, &attempted, &normalized_target, quota_protection_enabled
                    ) {
                        target_token = Some(selected.clone());
                        need_update_last_used = Some((selected.account_id.clone(), std::time::Instant::now()));

                        // 如果是会话首次分配且需要粘性，在此建立绑定
                        if let Some(sid) = session_id {
                            if scheduling.mode != SchedulingMode::PerformanceFirst {
                                self.session_accounts
                                    .insert(sid.to_string(), selected.account_id.clone());
                                tracing::debug!(
                                    "Sticky Session: Bound new account {} to session {}",
                                    selected.email,
                                    sid
                                );
                            }
                        }
                    }
                }
            } else if target_token.is_none() {
                // 模式 C: P2C 选择 (替代纯轮询)
                tracing::debug!(
                    "🔄 [Mode C] P2C selection from {} candidates",
                    total
                );

                // 先过滤出未限流的账号
                let mut non_limited: Vec<ProxyToken> = Vec::new();
                for t in &tokens_snapshot {
                    if !self.is_rate_limited(&t.account_id, Some(&normalized_target)).await {
                        non_limited.push(t.clone());
                    }
                }

                if let Some(selected) = self.select_with_p2c(
                    &non_limited, &attempted, &normalized_target, quota_protection_enabled
                ) {
                    tracing::debug!("  {} - SELECTED via P2C", selected.email);
                    target_token = Some(selected.clone());

                    if rotate {
                        tracing::debug!("Force Rotation: Switched to account: {}", selected.email);
                    }
                }
            }

            let mut token = match target_token {
                Some(t) => t,
                None => {
                    // 乐观重置策略: 双层防护机制
                    // 计算最短等待时间
                    let min_wait = tokens_snapshot
                        .iter()
                        .filter_map(|t| self.rate_limit_tracker.get_reset_seconds(&t.account_id))
                        .min();

                    // Layer 1: 如果最短等待时间 <= 2秒,执行缓冲延迟
                    if let Some(wait_sec) = min_wait {
                        if wait_sec <= 2 {
                            let wait_ms = (wait_sec as f64 * 1000.0) as u64;
                            tracing::warn!(
                                "All accounts rate-limited but shortest wait is {}s. Applying {}ms buffer for state sync...",
                                wait_sec, wait_ms
                            );

                            // 缓冲延迟
                            tokio::time::sleep(tokio::time::Duration::from_millis(wait_ms)).await;

                            // 重新尝试选择账号
                            let retry_token = tokens_snapshot.iter()
                                .find(|t| !attempted.contains(&t.account_id) 
                                    && !self.is_rate_limited_sync(&t.account_id, Some(&normalized_target))
                                    && !(quota_protection_enabled && t.protected_models.contains(&normalized_target)));

                            if let Some(t) = retry_token {
                                tracing::info!(
                                    "✅ Buffer delay successful! Found available account: {}",
                                    t.email
                                );
                                t.clone()
                            } else {
                                // Layer 2: 缓冲后仍无可用账号,执行乐观重置
                                tracing::warn!(
                                    "Buffer delay failed. Executing optimistic reset for all {} accounts...",
                                    tokens_snapshot.len()
                                );

                                // 清除所有限流记录
                                self.rate_limit_tracker.clear_all();

                                // 再次尝试选择账号
                                let final_token = tokens_snapshot
                                    .iter()
                                    .find(|t| !attempted.contains(&t.account_id)
                                        && !(quota_protection_enabled && t.protected_models.contains(&normalized_target)));

                                if let Some(t) = final_token {
                                    tracing::info!(
                                        "✅ Optimistic reset successful! Using account: {}",
                                        t.email
                                    );
                                    t.clone()
                                } else {
                                    return Err(
                                        "All accounts failed after optimistic reset.".to_string()
                                    );
                                }
                            }
                        } else {
                            return Err(format!("All accounts limited. Wait {}s.", wait_sec));
                        }
                    } else {
                        return Err("All accounts failed or unhealthy.".to_string());
                    }
                }
            };

            // Safety net: avoid selecting an account that has been disabled on disk but still
            // exists in the in-memory snapshot (e.g. stale cache + sticky session binding).
            match Self::get_account_state_on_disk(&token.account_path).await {
                OnDiskAccountState::Disabled => {
                    tracing::warn!(
                        "Selected account {} is disabled on disk, purging and retrying",
                        token.email
                    );
                    attempted.insert(token.account_id.clone());
                    self.remove_account(&token.account_id);
                    continue;
                }
                OnDiskAccountState::Unknown => {
                    tracing::warn!(
                        "Selected account {} state on disk is unavailable, skipping",
                        token.email
                    );
                    attempted.insert(token.account_id.clone());
                    continue;
                }
                OnDiskAccountState::Enabled => {}
            }

            // 3. 检查 token 是否过期（提前5分钟刷新）
            let now = chrono::Utc::now().timestamp();
            if now >= token.timestamp - 300 {
                tracing::debug!("账号 {} 的 token 即将过期，正在刷新...", token.email);

                // 调用 OAuth 刷新 token
                match crate::modules::oauth::refresh_access_token(&token.refresh_token, Some(&token.account_id)).await {
                    Ok(token_response) => {
                        tracing::debug!("Token 刷新成功！");

                        // 更新本地内存对象供后续使用
                        token.access_token = token_response.access_token.clone();
                        token.expires_in = token_response.expires_in;
                        token.timestamp = now + token_response.expires_in;

                        // 同步更新跨线程共享的 DashMap
                        if let Some(mut entry) = self.tokens.get_mut(&token.account_id) {
                            entry.access_token = token.access_token.clone();
                            entry.expires_in = token.expires_in;
                            entry.timestamp = token.timestamp;
                        }

                        // 同步落盘（避免重启后继续使用过期 timestamp 导致频繁刷新）
                        if let Err(e) = self
                            .save_refreshed_token(&token.account_id, &token_response)
                            .await
                        {
                            tracing::debug!("保存刷新后的 token 失败 ({}): {}", token.email, e);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Token 刷新失败 ({}): {}，尝试下一个账号", token.email, e);
                        if e.contains("\"invalid_grant\"") || e.contains("invalid_grant") {
                            tracing::error!(
                                "Disabling account due to invalid_grant ({}): refresh_token likely revoked/expired",
                                token.email
                            );
                            let _ = self
                                .disable_account(
                                    &token.account_id,
                                    &format!("invalid_grant: {}", e),
                                )
                                .await;
                            self.tokens.remove(&token.account_id);
                        }
                        // Avoid leaking account emails to API clients; details are still in logs.
                        last_error = Some(format!("Token refresh failed: {}", e));
                        attempted.insert(token.account_id.clone());

                        // 【优化】标记需要清除锁定，避免在循环内加锁
                        if quota_group != "image_gen" {
                            if matches!(&last_used_account_id, Some((id, _)) if id == &token.account_id)
                            {
                                need_update_last_used =
                                    Some((String::new(), std::time::Instant::now()));
                                // 空字符串表示需要清除
                            }
                        }
                        continue;
                    }
                }
            }

            // 4. 确保有 project_id (filter empty strings to trigger re-fetch)
            let project_id = if let Some(pid) = &token.project_id {
                if pid.is_empty() { None } else { Some(pid.clone()) }
            } else {
                None
            };
            let project_id = if let Some(pid) = project_id {
                pid
            } else {
                tracing::debug!("账号 {} 缺少 project_id，尝试获取...", token.email);
                match crate::proxy::project_resolver::fetch_project_id(&token.access_token).await {
                    Ok(pid) => {
                        if let Some(mut entry) = self.tokens.get_mut(&token.account_id) {
                            entry.project_id = Some(pid.clone());
                        }
                        let _ = self.save_project_id(&token.account_id, &pid).await;
                        pid
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to fetch project_id for {}, using fallback: {}",
                            token.email, e
                        );
                        // [FIX #1794] 为 503 问题提供稳定兜底，不跳过该账号
                        "bamboo-precept-lgxtn".to_string()
                    }
                }
            };

            // 【优化】在成功返回前，统一更新 last_used_account（如果需要）
            if let Some((new_account_id, new_time)) = need_update_last_used {
                if quota_group != "image_gen" {
                    let mut last_used = self.last_used_account.lock().await;
                    if new_account_id.is_empty() {
                        // 空字符串表示需要清除锁定
                        *last_used = None;
                    } else {
                        *last_used = Some((new_account_id, new_time));
                    }
                }
            }

            return Ok((token.access_token, project_id, token.email, token.account_id, 0));
        }

        Err(last_error.unwrap_or_else(|| "All accounts failed".to_string()))
    }

    async fn disable_account(&self, account_id: &str, reason: &str) -> Result<(), String> {
        let path = if let Some(entry) = self.tokens.get(account_id) {
            entry.account_path.clone()
        } else {
            self.data_dir
                .join("accounts")
                .join(format!("{}.json", account_id))
        };

        let mut content: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&path).map_err(|e| format!("读取文件失败: {}", e))?,
        )
        .map_err(|e| format!("解析 JSON 失败: {}", e))?;

        let now = chrono::Utc::now().timestamp();
        content["disabled"] = serde_json::Value::Bool(true);
        content["disabled_at"] = serde_json::Value::Number(now.into());
        content["disabled_reason"] = serde_json::Value::String(truncate_reason(reason, 800));

        std::fs::write(&path, serde_json::to_string_pretty(&content).unwrap())
            .map_err(|e| format!("写入文件失败: {}", e))?;

        // 【修复 Issue #3】从内存中移除禁用的账号，防止被60s锁定逻辑继续使用
        self.tokens.remove(account_id);

        tracing::warn!("Account disabled: {} ({:?})", account_id, path);
        Ok(())
    }

    /// 保存 project_id 到账号文件
    async fn save_project_id(&self, account_id: &str, project_id: &str) -> Result<(), String> {
        let entry = self.tokens.get(account_id)
            .ok_or("账号不存在")?;

        let path = &entry.account_path;

        let mut content: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(path).map_err(|e| format!("读取文件失败: {}", e))?
        ).map_err(|e| format!("解析 JSON 失败: {}", e))?;

        content["token"]["project_id"] = serde_json::Value::String(project_id.to_string());

        std::fs::write(path, serde_json::to_string_pretty(&content).unwrap())
            .map_err(|e| format!("写入文件失败: {}", e))?;

        tracing::debug!("已保存 project_id 到账号 {}", account_id);
        Ok(())
    }

    /// 保存刷新后的 token 到账号文件
    async fn save_refreshed_token(&self, account_id: &str, token_response: &crate::modules::oauth::TokenResponse) -> Result<(), String> {
        let entry = self.tokens.get(account_id)
            .ok_or("账号不存在")?;

        let path = &entry.account_path;

        let mut content: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(path).map_err(|e| format!("读取文件失败: {}", e))?
        ).map_err(|e| format!("解析 JSON 失败: {}", e))?;

        let now = chrono::Utc::now().timestamp();

        content["token"]["access_token"] = serde_json::Value::String(token_response.access_token.clone());
        content["token"]["expires_in"] = serde_json::Value::Number(token_response.expires_in.into());
        content["token"]["expiry_timestamp"] = serde_json::Value::Number((now + token_response.expires_in).into());

        std::fs::write(path, serde_json::to_string_pretty(&content).unwrap())
            .map_err(|e| format!("写入文件失败: {}", e))?;

        tracing::debug!("已保存刷新后的 token 到账号 {}", account_id);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// 通过 email 获取指定账号的 Token（用于预热等需要指定账号的场景）
    /// 此方法会自动刷新过期的 token
    pub async fn get_token_by_email(
        &self,
        email: &str,
    ) -> Result<(String, String, String, String, u64), String> {
        // 查找账号信息
        let token_info = {
            let mut found = None;
            for entry in self.tokens.iter() {
                let token = entry.value();
                if token.email == email {
                    found = Some((
                        token.account_id.clone(),
                        token.access_token.clone(),
                        token.refresh_token.clone(),
                        token.timestamp,
                        token.expires_in,
                        chrono::Utc::now().timestamp(),
                        token.project_id.clone(),
                    ));
                    break;
                }
            }
            found
        };

        let (
            account_id,
            current_access_token,
            refresh_token,
            timestamp,
            expires_in,
            now,
            project_id_opt,
        ) = match token_info {
            Some(info) => info,
            None => return Err(format!("未找到账号: {}", email)),
        };

        let project_id = project_id_opt
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "bamboo-precept-lgxtn".to_string());

        // 检查是否过期 (提前5分钟)
        if now < timestamp + expires_in - 300 {
            return Ok((current_access_token, project_id, email.to_string(), account_id, 0));
        }

        tracing::info!("[Warmup] Token for {} is expiring, refreshing...", email);

        // 调用 OAuth 刷新 token
        match crate::modules::oauth::refresh_access_token(&refresh_token, Some(&account_id)).await {
            Ok(token_response) => {
                tracing::info!("[Warmup] Token refresh successful for {}", email);
                let new_now = chrono::Utc::now().timestamp();

                // 更新缓存
                if let Some(mut entry) = self.tokens.get_mut(&account_id) {
                    entry.access_token = token_response.access_token.clone();
                    entry.expires_in = token_response.expires_in;
                    entry.timestamp = new_now;
                }

                // 保存到磁盘
                let _ = self
                    .save_refreshed_token(&account_id, &token_response)
                    .await;

                Ok((
                    token_response.access_token,
                    project_id,
                    email.to_string(),
                    account_id,
                    0,
                ))
            }
            Err(e) => Err(format!(
                "[Warmup] Token refresh failed for {}: {}",
                email, e
            )),
        }
    }

    // ===== 限流管理方法 =====

    /// 标记账号限流(从外部调用,通常在 handler 中)
    /// 参数为 email，内部会自动转换为 account_id
    pub async fn mark_rate_limited(
        &self,
        email: &str,
        status: u16,
        retry_after_header: Option<&str>,
        error_body: &str,
    ) {
        // [NEW] 检查熔断是否启用 (使用内存缓存，极快)
        let config = self.circuit_breaker_config.read().await.clone();
        if !config.enabled {
            return;
        }

        // 【替代方案】转换 email -> account_id
        let key = self.email_to_account_id(email).unwrap_or_else(|| email.to_string());

        self.rate_limit_tracker.parse_from_error(
            &key,
            status,
            retry_after_header,
            error_body,
            None,
            &config.backoff_steps, // [NEW] 传入配置
        );
    }

    /// 检查账号是否在限流中 (支持模型级)
    pub async fn is_rate_limited(&self, account_id: &str, model: Option<&str>) -> bool {
        // [NEW] 检查熔断是否启用
        let config = self.circuit_breaker_config.read().await;
        if !config.enabled {
            return false;
        }
        self.rate_limit_tracker.is_rate_limited(account_id, model)
    }

    /// [NEW] 检查账号是否在限流中 (同步版本，仅用于 Iterator)
    pub fn is_rate_limited_sync(&self, account_id: &str, model: Option<&str>) -> bool {
        // 同步版本无法读取 async RwLock，这里使用 blocking_read
        let config = self.circuit_breaker_config.blocking_read();
        if !config.enabled {
            return false;
        }
        self.rate_limit_tracker.is_rate_limited(account_id, model)
    }

    /// 获取距离限流重置还有多少秒
    #[allow(dead_code)]
    pub fn get_rate_limit_reset_seconds(&self, account_id: &str) -> Option<u64> {
        self.rate_limit_tracker.get_reset_seconds(account_id)
    }

    /// 清除过期的限流记录
    #[allow(dead_code)]
    pub fn clean_expired_rate_limits(&self) {
        self.rate_limit_tracker.cleanup_expired();
    }

    /// 【替代方案】通过 email 查找对应的 account_id
    /// 用于将 handlers 传入的 email 转换为 tracker 使用的 account_id
    fn email_to_account_id(&self, email: &str) -> Option<String> {
        self.tokens
            .iter()
            .find(|entry| entry.value().email == email)
            .map(|entry| entry.value().account_id.clone())
    }

    /// 清除指定账号的限流记录
    pub fn clear_rate_limit(&self, account_id: &str) -> bool {
        self.rate_limit_tracker.clear(account_id)
    }

    /// 清除所有限流记录
    pub fn clear_all_rate_limits(&self) {
        self.rate_limit_tracker.clear_all();
    }

    /// 标记账号请求成功，重置连续失败计数
    ///
    /// 在请求成功完成后调用，将该账号的失败计数归零，
    /// 下次失败时从最短的锁定时间开始（智能限流）。
    pub fn mark_account_success(&self, account_id: &str) {
        self.rate_limit_tracker.mark_success(account_id);
    }

    /// 检查是否有可用的 Google 账号
    ///
    /// 用于"仅兜底"模式的智能判断:当所有 Google 账号不可用时才使用外部提供商。
    ///
    /// # 参数
    /// - `quota_group`: 配额组("claude" 或 "gemini"),暂未使用但保留用于未来扩展
    /// - `target_model`: 目标模型名称(已归一化),用于配额保护检查
    ///
    /// # 返回值
    /// - `true`: 至少有一个可用账号(未限流且未被配额保护)
    /// - `false`: 所有账号都不可用(被限流或被配额保护)
    ///
    /// # 示例
    /// ```ignore
    /// // 检查是否有可用账号处理 claude-sonnet 请求
    /// let has_available = token_manager.has_available_account("claude", "claude-sonnet-4-20250514").await;
    /// if !has_available {
    ///     // 切换到外部提供商
    /// }
    /// ```
    pub async fn has_available_account(&self, _quota_group: &str, target_model: &str) -> bool {
        // 检查配额保护是否启用
        let quota_protection_enabled = crate::modules::config::load_app_config()
            .map(|cfg| cfg.quota_protection.enabled)
            .unwrap_or(false);

        // 遍历所有账号,检查是否有可用的
        for entry in self.tokens.iter() {
            let token = entry.value();

            // 1. 检查是否被限流
            if self.is_rate_limited(&token.account_id, None).await {
                tracing::debug!(
                    "[Fallback Check] Account {} is rate-limited, skipping",
                    token.email
                );
                continue;
            }

            // 2. 检查是否被配额保护(如果启用)
            if quota_protection_enabled && token.protected_models.contains(target_model) {
                tracing::debug!(
                    "[Fallback Check] Account {} is quota-protected for model {}, skipping",
                    token.email,
                    target_model
                );
                continue;
            }

            // 找到至少一个可用账号
            tracing::debug!(
                "[Fallback Check] Found available account: {} for model {}",
                token.email,
                target_model
            );
            return true;
        }

        // 所有账号都不可用
        tracing::info!(
            "[Fallback Check] No available Google accounts for model {}, fallback should be triggered",
            target_model
        );
        false
    }

    /// 从账号文件获取配额刷新时间
    ///
    /// 返回该账号最近的配额刷新时间字符串（ISO 8601 格式）
    ///
    /// # 参数
    /// - `account_id`: 账号 ID（用于查找账号文件）
    pub fn get_quota_reset_time(&self, account_id: &str) -> Option<String> {
        // 直接用 account_id 查找账号文件（文件名是 {account_id}.json）
        let account_path = self.data_dir.join("accounts").join(format!("{}.json", account_id));

        let content = std::fs::read_to_string(&account_path).ok()?;
        let account: serde_json::Value = serde_json::from_str(&content).ok()?;

        // 获取 quota.models 中最早的 reset_time（最保守的锁定策略）
        account
            .get("quota")
            .and_then(|q| q.get("models"))
            .and_then(|m| m.as_array())
            .and_then(|models| {
                models.iter()
                    .filter_map(|m| m.get("reset_time").and_then(|r| r.as_str()))
                    .filter(|s| !s.is_empty())
                    .min()
                    .map(|s| s.to_string())
            })
    }

    /// 使用配额刷新时间精确锁定账号
    ///
    /// 当 API 返回 429 但没有 quotaResetDelay 时,尝试使用账号的配额刷新时间
    ///
    /// # 参数
    /// - `account_id`: 账号 ID
    /// - `reason`: 限流原因（QuotaExhausted/ServerError 等）
    /// - `model`: 可选的模型名称,用于模型级别限流
    pub fn set_precise_lockout(&self, account_id: &str, reason: crate::proxy::rate_limit::RateLimitReason, model: Option<String>) -> bool {
        // [FIX #2209] 统一归一化模型名称
        let normalized_model = model.as_deref().and_then(|m| crate::proxy::common::model_mapping::normalize_to_standard_id(m));
        let model_to_lock = normalized_model.or(model);

        if let Some(reset_time_str) = self.get_quota_reset_time(account_id) {
            tracing::info!("找到账号 {} 的配额刷新时间: {}", account_id, reset_time_str);
            self.rate_limit_tracker.set_lockout_until_iso(account_id, &reset_time_str, reason, model_to_lock)
        } else {
            tracing::debug!("未找到账号 {} 的配额刷新时间,将使用默认退避策略", account_id);
            false
        }
    }

    /// 实时刷新配额并精确锁定账号
    ///
    /// 当 429 发生时调用此方法:
    /// 1. 实时调用配额刷新 API 获取最新的 reset_time
    /// 2. 使用最新的 reset_time 精确锁定账号
    /// 3. 如果获取失败,返回 false 让调用方使用回退策略
    ///
    /// # 参数
    /// - `model`: 可选的模型名称,用于模型级别限流
    pub async fn fetch_and_lock_with_realtime_quota(
        &self,
        email: &str,
        reason: crate::proxy::rate_limit::RateLimitReason,
        model: Option<String>,
    ) -> bool {
        // 1. 从 tokens 中获取该账号的 access_token 和 account_id
        // 同时获取 account_id，确保锁定 key 与检查 key 一致
        let (access_token, account_id) = {
            let mut found: Option<(String, String)> = None;
            for entry in self.tokens.iter() {
                if entry.value().email == email {
                    found = Some((
                        entry.value().access_token.clone(),
                        entry.value().account_id.clone(),
                    ));
                    break;
                }
            }
            found
        }.unzip();

        let (access_token, account_id) = match (access_token, account_id) {
            (Some(token), Some(id)) => (token, id),
            _ => {
                tracing::warn!("无法找到账号 {} 的 access_token,无法实时刷新配额", email);
                return false;
            }
        };

        // 2. 调用配额刷新 API
        tracing::info!("账号 {} 正在实时刷新配额...", email);
        match crate::modules::quota::fetch_quota(&access_token, email, Some(&account_id)).await {
            Ok((quota_data, _project_id)) => {
                // 3. 从最新配额中提取 reset_time
                let earliest_reset = quota_data
                    .models
                    .iter()
                    .filter_map(|m| {
                        if !m.reset_time.is_empty() {
                            Some(m.reset_time.as_str())
                        } else {
                            None
                        }
                    })
                    .min();

                if let Some(reset_time_str) = earliest_reset {
                    tracing::info!(
                        "账号 {} 实时配额刷新成功,reset_time: {}",
                        email,
                        reset_time_str
                    );
                    
                    // [FIX #2209] 统一归一化模型名称
                    let normalized_model = model.as_deref().and_then(|m| crate::proxy::common::model_mapping::normalize_to_standard_id(m));
                    let model_to_lock = normalized_model.or(model);

                    // [FIX] 使用 account_id 作为 key，与 is_rate_limited 检查一致
                    self.rate_limit_tracker.set_lockout_until_iso(&account_id, reset_time_str, reason, model_to_lock)
                } else {
                    tracing::warn!("账号 {} 配额刷新成功但未找到 reset_time", email);
                    false
                }
            }
            Err(e) => {
                tracing::warn!("账号 {} 实时配额刷新失败: {:?}", email, e);
                false
            }
        }
    }

    /// 标记账号限流(异步版本,支持实时配额刷新)
    ///
    /// 三级降级策略:
    /// 1. 优先: API 返回 quotaResetDelay → 直接使用
    /// 2. 次优: 实时刷新配额 → 获取最新 reset_time
    /// 3. 保底: 使用本地缓存配额 → 读取账号文件
    /// 4. 兜底: 指数退避策略 → 默认锁定时间
    ///
    /// # 参数
    /// - `email`: 账号邮箱,用于查找账号信息
    /// - `status`: HTTP 状态码（如 429、500 等）
    /// - `retry_after_header`: 可选的 Retry-After 响应头
    /// - `error_body`: 错误响应体,用于解析 quotaResetDelay
    /// - `model`: 可选的模型名称,用于模型级别限流
    pub async fn mark_rate_limited_async(
        &self,
        email: &str,
        status: u16,
        retry_after_header: Option<&str>,
        error_body: &str,
        model: Option<&str>, // 🆕 新增模型参数
    ) {
        // [FIX #2209] 统一归一化模型名称，确保锁定 Key 与负载均衡检查 Key 一致
        let normalized_model = model.and_then(|m| crate::proxy::common::model_mapping::normalize_to_standard_id(m));
        let model_to_track = normalized_model.as_deref().or(model);

        // [NEW] 检查熔断是否启用
        let config = self.circuit_breaker_config.read().await.clone();
        if !config.enabled {
            return;
        }

        // [FIX] Convert email to account_id for consistent tracking
        let account_id = self.email_to_account_id(email).unwrap_or_else(|| email.to_string());

        // 检查 API 是否返回了精确的重试时间
        let has_explicit_retry_time = retry_after_header.is_some() ||
            error_body.contains("quotaResetDelay");

        if has_explicit_retry_time {
            // API 返回了精确时间(quotaResetDelay),直接使用,无需实时刷新
            if let Some(m) = model {
                tracing::debug!(
                    "账号 {} 的模型 {} 的 429 响应包含 quotaResetDelay,直接使用 API 返回的时间",
                    account_id,
                    m
                );
            } else {
                tracing::debug!(
                    "账号 {} 的 429 响应包含 quotaResetDelay,直接使用 API 返回的时间",
                    account_id
                );
            }
            self.rate_limit_tracker.parse_from_error(
                &account_id,
                status,
                retry_after_header,
                error_body,
                model_to_track.map(|s| s.to_string()),
                &config.backoff_steps, // [NEW] 传入配置
            );
            return;
        }

        // 确定限流原因
        let reason = if error_body.to_lowercase().contains("model_capacity") {
            crate::proxy::rate_limit::RateLimitReason::ModelCapacityExhausted
        } else if error_body.to_lowercase().contains("exhausted")
            || error_body.to_lowercase().contains("quota")
        {
            crate::proxy::rate_limit::RateLimitReason::QuotaExhausted
        } else {
            crate::proxy::rate_limit::RateLimitReason::Unknown
        };

        // API 未返回 quotaResetDelay,需要实时刷新配额获取精确锁定时间
        if let Some(m) = model_to_track {
            tracing::info!(
                "账号 {} 的模型 {} 的 429 响应未包含 quotaResetDelay,尝试实时刷新配额...",
                account_id,
                m
            );
        } else {
            tracing::info!(
                "账号 {} 的 429 响应未包含 quotaResetDelay,尝试实时刷新配额...",
                account_id
            );
        }

        // [FIX] 传入 email 而不是 account_id，因为 fetch_and_lock_with_realtime_quota 期望 email
        if self.fetch_and_lock_with_realtime_quota(email, reason, model_to_track.map(|s| s.to_string())).await {
            tracing::info!("账号 {} 已使用实时配额精确锁定", email);
            return;
        }

        // 实时刷新失败,尝试使用本地缓存的配额刷新时间
        if self.set_precise_lockout(&account_id, reason, model_to_track.map(|s| s.to_string())) {
            tracing::info!("账号 {} 已使用本地缓存配额锁定", account_id);
            return;
        }

        // 都失败了,回退到指数退避策略
        tracing::warn!("账号 {} 无法获取配额刷新时间,使用指数退避策略", account_id);
        self.rate_limit_tracker.parse_from_error(
            &account_id,
            status,
            retry_after_header,
            error_body,
            model_to_track.map(|s| s.to_string()),
            &config.backoff_steps, // [NEW] 传入配置
        );
    }

    // ===== 调度配置相关方法 =====

    /// 获取当前调度配置
    pub async fn get_sticky_config(&self) -> StickySessionConfig {
        self.sticky_config.read().await.clone()
    }

    /// 更新调度配置
    pub async fn update_sticky_config(&self, new_config: StickySessionConfig) {
        let mut config = self.sticky_config.write().await;
        *config = new_config;
        tracing::debug!("Scheduling configuration updated: {:?}", *config);
    }

    /// [NEW] 更新熔断器配置
    pub async fn update_circuit_breaker_config(&self, config: crate::models::CircuitBreakerConfig) {
        let mut lock = self.circuit_breaker_config.write().await;
        *lock = config;
        tracing::debug!("Circuit breaker configuration updated");
    }

    /// [NEW] 获取熔断器配置
    pub async fn get_circuit_breaker_config(&self) -> crate::models::CircuitBreakerConfig {
        self.circuit_breaker_config.read().await.clone()
    }

    /// 清除特定会话的粘性映射
    #[allow(dead_code)]
    pub fn clear_session_binding(&self, session_id: &str) {
        self.session_accounts.remove(session_id);
    }

    /// 清除所有会话的粘性映射
    pub fn clear_all_sessions(&self) {
        self.session_accounts.clear();
    }

    // ===== [FIX #820] 固定账号模式相关方法 =====

    /// 设置优先使用的账号ID（固定账号模式）
    /// 传入 Some(account_id) 启用固定账号模式，传入 None 恢复轮询模式
    pub async fn set_preferred_account(&self, account_id: Option<String>) {
        let mut preferred = self.preferred_account_id.write().await;
        if let Some(ref id) = account_id {
            tracing::info!("🔒 [FIX #820] Fixed account mode enabled: {}", id);
        } else {
            tracing::info!("🔄 [FIX #820] Round-robin mode enabled (no preferred account)");
        }
        *preferred = account_id;
    }

    /// 获取当前优先使用的账号ID
    pub async fn get_preferred_account(&self) -> Option<String> {
        self.preferred_account_id.read().await.clone()
    }

    /// 使用 Authorization Code 交换 Refresh Token (Web OAuth)
    pub async fn exchange_code(&self, code: &str, redirect_uri: &str) -> Result<String, String> {
        crate::modules::oauth::exchange_code(code, redirect_uri)
            .await
            .and_then(|t| {
                t.refresh_token
                    .ok_or_else(|| "No refresh token returned by Google".to_string())
            })
    }

    /// 获取 OAuth URL (支持自定义 Redirect URI)
    pub fn get_oauth_url_with_redirect(&self, redirect_uri: &str, state: &str) -> String {
        crate::modules::oauth::get_auth_url(redirect_uri, state)
    }

    /// 获取用户信息 (Email 等)
    pub async fn get_user_info(
        &self,
        refresh_token: &str,
    ) -> Result<crate::modules::oauth::UserInfo, String> {
        // 先获取 Access Token
        let token = crate::modules::oauth::refresh_access_token(refresh_token, None)
            .await
            .map_err(|e| format!("刷新 Access Token 失败: {}", e))?;

        crate::modules::oauth::get_user_info(&token.access_token, None).await
    }

    /// 添加新账号 (纯后端实现，不依赖 Tauri AppHandle)
    pub async fn add_account(&self, email: &str, refresh_token: &str) -> Result<(), String> {
        // 1. 获取 Access Token (验证 refresh_token 有效性)
        let token_info = crate::modules::oauth::refresh_access_token(refresh_token, None)
            .await
            .map_err(|e| format!("Invalid refresh token: {}", e))?;

        // 2. 获取项目 ID (Project ID)
        let project_id = crate::proxy::project_resolver::fetch_project_id(&token_info.access_token)
            .await
            .unwrap_or_else(|_| "bamboo-precept-lgxtn".to_string()); // Fallback

        // 3. 委托给 modules::account::add_account 处理 (包含文件写入、索引更新、锁)
        let email_clone = email.to_string();
        let refresh_token_clone = refresh_token.to_string();

        tokio::task::spawn_blocking(move || {
            let token_data = crate::models::TokenData::new(
                token_info.access_token,
                refresh_token_clone,
                token_info.expires_in,
                Some(email_clone.clone()),
                Some(project_id),
                None, // session_id
                true,
            )
            .with_oauth_client_key(token_info.oauth_client_key.clone());

            crate::modules::account::upsert_account(email_clone, None, token_data)
        })
        .await
        .map_err(|e| format!("Task join error: {}", e))?
        .map_err(|e| format!("Failed to save account: {}", e))?;

        // 4. 重新加载 (更新内存)
        self.reload_all_accounts().await.map(|_| ())
    }

    /// 记录请求成功，增加健康分
    pub fn record_success(&self, account_id: &str) {
        self.health_scores
            .entry(account_id.to_string())
            .and_modify(|s| *s = (*s + 0.05).min(1.0))
            .or_insert(1.0);
        tracing::debug!("📈 Health score increased for account {}", account_id);
    }

    /// 记录请求失败，降低健康分
    pub fn record_failure(&self, account_id: &str) {
        self.health_scores
            .entry(account_id.to_string())
            .and_modify(|s| *s = (*s - 0.2).max(0.0))
            .or_insert(0.8);
        tracing::warn!("📉 Health score decreased for account {}", account_id);
    }

    /// [NEW] 从账号配额信息中提取最近的刷新时间戳
    ///
    /// Claude 模型（sonnet/opus）共用同一个刷新时间，只需取 claude 系列的 reset_time
    /// 返回 Unix 时间戳（秒），用于排序时比较
    fn extract_earliest_reset_time(&self, account: &serde_json::Value) -> Option<i64> {
        let models = account
            .get("quota")
            .and_then(|q| q.get("models"))
            .and_then(|m| m.as_array())?;

        let mut earliest_ts: Option<i64> = None;

        for model in models {
            // 优先取 claude 系列的 reset_time（sonnet/opus 共用）
            let model_name = model.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if !model_name.contains("claude") {
                continue;
            }

            if let Some(reset_time_str) = model.get("reset_time").and_then(|r| r.as_str()) {
                if reset_time_str.is_empty() {
                    continue;
                }
                // 解析 ISO 8601 时间字符串为时间戳
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(reset_time_str) {
                    let ts = dt.timestamp();
                    if earliest_ts.is_none() || ts < earliest_ts.unwrap() {
                        earliest_ts = Some(ts);
                    }
                }
            }
        }

        // 如果没有 claude 模型的时间，尝试取任意模型的最近时间
        if earliest_ts.is_none() {
            for model in models {
                if let Some(reset_time_str) = model.get("reset_time").and_then(|r| r.as_str()) {
                    if reset_time_str.is_empty() {
                        continue;
                    }
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(reset_time_str) {
                        let ts = dt.timestamp();
                        if earliest_ts.is_none() || ts < earliest_ts.unwrap() {
                            earliest_ts = Some(ts);
                        }
                    }
                }
            }
        }

        earliest_ts
    }

    /// 获取当前所有可用账号中收集到的官方下发的所有动态模型集合
    pub fn get_all_collected_models(&self) -> std::collections::HashSet<String> {
        let mut all_models = std::collections::HashSet::new();
        for entry in self.tokens.iter() {
            let token = entry.value();
            for model_id in token.model_quotas.keys() {
                all_models.insert(model_id.clone());
            }
        }
        all_models
    }

    /// [NEW] 从指定账号的动态额度数据中获取特定模型的 max_output_tokens
    ///
    /// # 返回
    /// - `Some(u64)`: 找到了动态限额数据
    /// - `None`: 账号不存在或该模型无数据（调用方应继续查静态默认表）
    pub fn get_model_output_limit_for_account(&self, account_id: &str, model_name: &str) -> Option<u64> {
        self.tokens
            .get(account_id)
            .and_then(|token| token.model_limits.get(model_name).copied())
    }

    /// Helper to find account ID by email
    pub fn get_account_id_by_email(&self, email: &str) -> Option<String> {
        for entry in self.tokens.iter() {
            if entry.value().email == email {
                return Some(entry.key().clone());
            }
        }
        None
    }

    /// Set validation blocked status for an account (internal)
    pub async fn set_validation_block(&self, account_id: &str, block_until: i64, reason: &str) -> Result<(), String> {
        // 1. Update memory
        if let Some(mut token) = self.tokens.get_mut(account_id) {
             token.validation_blocked = true;
             token.validation_blocked_until = block_until;
        }

        // 2. Persist to disk
        let path = self.data_dir.join("accounts").join(format!("{}.json", account_id));
        if !path.exists() {
             return Err(format!("Account file not found: {:?}", path));
        }

        let content = std::fs::read_to_string(&path)
             .map_err(|e| format!("Failed to read account file: {}", e))?;

        let mut account: serde_json::Value = serde_json::from_str(&content)
             .map_err(|e| format!("Failed to parse account JSON: {}", e))?;

        account["validation_blocked"] = serde_json::Value::Bool(true);
        account["validation_blocked_until"] = serde_json::Value::Number(serde_json::Number::from(block_until));
        account["validation_blocked_reason"] = serde_json::Value::String(reason.to_string());

        // [NEW] 尝试从消息中提取验证链接 (#1522)
        let extracted_url = if let Ok(parsed_json) = serde_json::from_str::<serde_json::Value>(reason) {
             // 尝试从特定的 Google RPC error 结构中取
             let mut url = None;
             if let Some(details) = parsed_json.pointer("/error/details") {
                 if let Some(arr) = details.as_array() {
                     for detail in arr {
                         if let Some(meta) = detail.get("metadata") {
                             if let Some(v_url) = meta.get("validation_url").and_then(|v| v.as_str()) {
                                 url = Some(v_url.to_string());
                                 break;
                             }
                             if let Some(a_url) = meta.get("appeal_url").and_then(|v| v.as_str()) {
                                 url = Some(a_url.to_string());
                                 break;
                             }
                         }
                     }
                 }
             }
             url
        } else {
             // 回退方案：通过更严格的正则及反序列化解码可能的 \u0026
             let url_regex = regex::Regex::new(r#"https://[^\s"'\\]+"#).unwrap();
             url_regex.find(reason).map(|m| {
                 let raw_url = m.as_str().to_string();
                 raw_url.replace("\\u0026", "&")
             })
        };
        
        if let Some(url) = extracted_url {
             account["validation_url"] = serde_json::Value::String(url.clone());
             if let Some(mut token) = self.tokens.get_mut(account_id) {
                 token.validation_url = Some(url);
             }
        }

        // Clear sticky session if blocked
        self.session_accounts.retain(|_, v| *v != account_id);

        let json_str = serde_json::to_string_pretty(&account)
             .map_err(|e| format!("Failed to serialize account JSON: {}", e))?;

        std::fs::write(&path, json_str)
             .map_err(|e| format!("Failed to write account file: {}", e))?;

        tracing::info!(
             "🚫 Account {} validation blocked until {} (reason: {})",
             account_id,
             block_until,
             reason
        );

        Ok(())
    }

    /// Public method to set validation block (called from handlers)
    pub async fn set_validation_block_public(&self, account_id: &str, block_until: i64, reason: &str) -> Result<(), String> {
        self.set_validation_block(account_id, block_until, reason).await
    }

    /// Set is_forbidden status for an account (called when proxy encounters 403)
    pub async fn set_forbidden(&self, account_id: &str, reason: &str) -> Result<(), String> {
        // [FIX] 调用封装好的模块函数，确保线程安全地更新账号文件和索引
        crate::modules::account::mark_account_forbidden(account_id, reason)?;

        // Clear sticky session if forbidden
        self.session_accounts.retain(|_, v| *v != account_id);

        // [FIX] 从内存池中移除账号，避免重试时再次选中
        self.remove_account(account_id);

        tracing::warn!(
            "🚫 Account {} marked as forbidden (403): {}",
            account_id,
            truncate_reason(reason, 1000)
        );

        Ok(())
    }
}

/// 截断过长的原因字符串
fn truncate_reason(reason: &str, max_len: usize) -> String {
    if reason.len() <= max_len {
        reason.to_string()
    } else {
        // [FIX] 确保字符截断在有效边界，防止 panic
        let end = reason
            .char_indices()
            .map(|(i, _)| i)
            .filter(|&i| i <= max_len - 3)
            .last()
            .unwrap_or(0);
        format!("{}...", &reason[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[tokio::test]
    async fn test_reload_account_purges_cache_when_account_becomes_proxy_disabled() {
        let tmp_root = std::env::temp_dir().join(format!(
            "antigravity-token-manager-test-{}",
            uuid::Uuid::new_v4()
        ));
        let accounts_dir = tmp_root.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();

        let account_id = "acc1";
        let email = "a@test.com";
        let now = chrono::Utc::now().timestamp();
        let account_path = accounts_dir.join(format!("{}.json", account_id));

        let account_json = serde_json::json!({
            "id": account_id,
            "email": email,
            "token": {
                "access_token": "atk",
                "refresh_token": "rtk",
                "expires_in": 3600,
                "expiry_timestamp": now + 3600
            },
            "disabled": false,
            "proxy_disabled": false,
            "created_at": now,
            "last_used": now
        });
        std::fs::write(&account_path, serde_json::to_string_pretty(&account_json).unwrap()).unwrap();

        let manager = TokenManager::new(tmp_root.clone());
        manager.load_accounts().await.unwrap();
        assert!(manager.tokens.get(account_id).is_some());

        // Prime extra caches to ensure remove_account() is really called.
        manager
            .session_accounts
            .insert("sid1".to_string(), account_id.to_string());
        {
            let mut preferred = manager.preferred_account_id.write().await;
            *preferred = Some(account_id.to_string());
        }

        // Mark account as proxy-disabled on disk (manual disable).
        let mut disabled_json = account_json.clone();
        disabled_json["proxy_disabled"] = serde_json::Value::Bool(true);
        disabled_json["proxy_disabled_reason"] = serde_json::Value::String("manual".to_string());
        disabled_json["proxy_disabled_at"] = serde_json::Value::Number(now.into());
        std::fs::write(&account_path, serde_json::to_string_pretty(&disabled_json).unwrap()).unwrap();

        manager.reload_account(account_id).await.unwrap();

        assert!(manager.tokens.get(account_id).is_none());
        assert!(manager.session_accounts.get("sid1").is_none());
        assert!(manager.preferred_account_id.read().await.is_none());

        let _ = std::fs::remove_dir_all(&tmp_root);
    }

    #[tokio::test]
    async fn test_fixed_account_mode_skips_preferred_when_disabled_on_disk_without_reload() {
        let tmp_root = std::env::temp_dir().join(format!(
            "antigravity-token-manager-test-fixed-mode-{}",
            uuid::Uuid::new_v4()
        ));
        let accounts_dir = tmp_root.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();

        let now = chrono::Utc::now().timestamp();

        let write_account = |id: &str, email: &str, proxy_disabled: bool| {
            let account_path = accounts_dir.join(format!("{}.json", id));
            let json = serde_json::json!({
                "id": id,
                "email": email,
                "token": {
                    "access_token": format!("atk-{}", id),
                    "refresh_token": format!("rtk-{}", id),
                    "expires_in": 3600,
                    "expiry_timestamp": now + 3600,
                    "project_id": format!("pid-{}", id)
                },
                "disabled": false,
                "proxy_disabled": proxy_disabled,
                "proxy_disabled_reason": if proxy_disabled { "manual" } else { "" },
                "created_at": now,
                "last_used": now
            });
            std::fs::write(&account_path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
        };

        // Two accounts in pool.
        write_account("acc1", "a@test.com", false);
        write_account("acc2", "b@test.com", false);

        let manager = TokenManager::new(tmp_root.clone());
        manager.load_accounts().await.unwrap();

        // Enable fixed account mode for acc1.
        manager.set_preferred_account(Some("acc1".to_string())).await;

        // Disable acc1 on disk WITHOUT reloading the in-memory pool (simulates stale cache).
        write_account("acc1", "a@test.com", true);

        let (_token, _project_id, email, account_id, _wait_ms) = manager
            .get_token("gemini", false, Some("sid1"), "gemini-1.5-flash")
            .await
            .unwrap();

        // Should fall back to another account instead of using the disabled preferred one.
        assert_eq!(account_id, "acc2");
        assert_eq!(email, "b@test.com");
        assert!(manager.tokens.get("acc1").is_none());
        assert!(manager.get_preferred_account().await.is_none());

        let _ = std::fs::remove_dir_all(&tmp_root);
    }

    #[tokio::test]
    async fn test_sticky_session_skips_bound_account_when_disabled_on_disk_without_reload() {
        let tmp_root = std::env::temp_dir().join(format!(
            "antigravity-token-manager-test-sticky-disabled-{}",
            uuid::Uuid::new_v4()
        ));
        let accounts_dir = tmp_root.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();

        let now = chrono::Utc::now().timestamp();

        let write_account = |id: &str, email: &str, percentage: i64, proxy_disabled: bool| {
            let account_path = accounts_dir.join(format!("{}.json", id));
            let json = serde_json::json!({
                "id": id,
                "email": email,
                "token": {
                    "access_token": format!("atk-{}", id),
                    "refresh_token": format!("rtk-{}", id),
                    "expires_in": 3600,
                    "expiry_timestamp": now + 3600,
                    "project_id": format!("pid-{}", id)
                },
                "quota": {
                    "models": [
                        { "name": "gemini-1.5-flash", "percentage": percentage }
                    ]
                },
                "disabled": false,
                "proxy_disabled": proxy_disabled,
                "proxy_disabled_reason": if proxy_disabled { "manual" } else { "" },
                "created_at": now,
                "last_used": now
            });
            std::fs::write(&account_path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
        };

        // Two accounts in pool. acc1 has higher quota -> should be selected and bound first.
        write_account("acc1", "a@test.com", 90, false);
        write_account("acc2", "b@test.com", 10, false);

        let manager = TokenManager::new(tmp_root.clone());
        manager.load_accounts().await.unwrap();

        // Prime: first request should bind the session to acc1.
        let (_token, _project_id, _email, account_id, _wait_ms) = manager
            .get_token("gemini", false, Some("sid1"), "gemini-1.5-flash")
            .await
            .unwrap();
        assert_eq!(account_id, "acc1");
        assert_eq!(
            manager.session_accounts.get("sid1").map(|v| v.clone()),
            Some("acc1".to_string())
        );

        // Disable acc1 on disk WITHOUT reloading the in-memory pool (simulates stale cache).
        write_account("acc1", "a@test.com", 90, true);

        let (_token, _project_id, email, account_id, _wait_ms) = manager
            .get_token("gemini", false, Some("sid1"), "gemini-1.5-flash")
            .await
            .unwrap();

        // Should fall back to another account instead of reusing the disabled bound one.
        assert_eq!(account_id, "acc2");
        assert_eq!(email, "b@test.com");
        assert!(manager.tokens.get("acc1").is_none());
        assert_ne!(
            manager.session_accounts.get("sid1").map(|v| v.clone()),
            Some("acc1".to_string())
        );

        let _ = std::fs::remove_dir_all(&tmp_root);
    }

    /// 创建测试用的 ProxyToken
    fn create_test_token(
        email: &str,
        tier: Option<&str>,
        health_score: f32,
        reset_time: Option<i64>,
        remaining_quota: Option<i32>,
    ) -> ProxyToken {
        ProxyToken {
            account_id: email.to_string(),
            access_token: "test_token".to_string(),
            refresh_token: "test_refresh".to_string(),
            expires_in: 3600,
            timestamp: chrono::Utc::now().timestamp() + 3600,
            email: email.to_string(),
            account_path: PathBuf::from("/tmp/test"),
            project_id: None,
            subscription_tier: tier.map(|s| s.to_string()),
            remaining_quota,
            protected_models: HashSet::new(),
            health_score,
            reset_time,
            validation_blocked: false,
            validation_blocked_until: 0,
            validation_url: None,
            model_quotas: HashMap::new(),
            model_limits: HashMap::new(),
        }
    }

    /// 测试排序比较函数（与 get_token_internal 中的逻辑一致）
    fn compare_tokens(a: &ProxyToken, b: &ProxyToken) -> Ordering {
        const RESET_TIME_THRESHOLD_SECS: i64 = 600; // 10 分钟阈值

        let tier_priority = |tier: &Option<String>| {
            let t = tier.as_deref().unwrap_or("").to_lowercase();
            if t.contains("ultra") { 0 }
            else if t.contains("pro") { 1 }
            else if t.contains("free") { 2 }
            else { 3 }
        };

        // First: compare by subscription tier
        let tier_cmp = tier_priority(&a.subscription_tier).cmp(&tier_priority(&b.subscription_tier));
        if tier_cmp != Ordering::Equal {
            return tier_cmp;
        }

        // Second: compare by health score (higher is better)
        let health_cmp = b.health_score.partial_cmp(&a.health_score).unwrap_or(Ordering::Equal);
        if health_cmp != Ordering::Equal {
            return health_cmp;
        }

        // Third: compare by reset time (earlier/closer is better)
        let reset_a = a.reset_time.unwrap_or(i64::MAX);
        let reset_b = b.reset_time.unwrap_or(i64::MAX);
        let reset_diff = (reset_a - reset_b).abs();

        if reset_diff >= RESET_TIME_THRESHOLD_SECS {
            let reset_cmp = reset_a.cmp(&reset_b);
            if reset_cmp != Ordering::Equal {
                return reset_cmp;
            }
        }

        // Fourth: compare by remaining quota percentage (higher is better)
        let quota_a = a.remaining_quota.unwrap_or(0);
        let quota_b = b.remaining_quota.unwrap_or(0);
        quota_b.cmp(&quota_a)
    }

    #[test]
    fn test_sorting_tier_priority() {
        // ULTRA > PRO > FREE
        let ultra = create_test_token("ultra@test.com", Some("ULTRA"), 1.0, None, Some(50));
        let pro = create_test_token("pro@test.com", Some("PRO"), 1.0, None, Some(50));
        let free = create_test_token("free@test.com", Some("FREE"), 1.0, None, Some(50));

        assert_eq!(compare_tokens(&ultra, &pro), Ordering::Less);
        assert_eq!(compare_tokens(&pro, &free), Ordering::Less);
        assert_eq!(compare_tokens(&ultra, &free), Ordering::Less);
        assert_eq!(compare_tokens(&free, &ultra), Ordering::Greater);
    }

    #[test]
    fn test_sorting_health_score_priority() {
        // 同等级下，健康分高的优先
        let high_health = create_test_token("high@test.com", Some("PRO"), 1.0, None, Some(50));
        let low_health = create_test_token("low@test.com", Some("PRO"), 0.5, None, Some(50));

        assert_eq!(compare_tokens(&high_health, &low_health), Ordering::Less);
        assert_eq!(compare_tokens(&low_health, &high_health), Ordering::Greater);
    }

    #[test]
    fn test_sorting_reset_time_priority() {
        let now = chrono::Utc::now().timestamp();

        // 刷新时间更近（30分钟后）的优先于更远（5小时后）的
        let soon_reset = create_test_token("soon@test.com", Some("PRO"), 1.0, Some(now + 1800), Some(50));  // 30分钟后
        let late_reset = create_test_token("late@test.com", Some("PRO"), 1.0, Some(now + 18000), Some(50)); // 5小时后

        assert_eq!(compare_tokens(&soon_reset, &late_reset), Ordering::Less);
        assert_eq!(compare_tokens(&late_reset, &soon_reset), Ordering::Greater);
    }

    #[test]
    fn test_sorting_reset_time_threshold() {
        let now = chrono::Utc::now().timestamp();

        // 差异小于10分钟（600秒）视为相同优先级，此时按配额排序
        let reset_a = create_test_token("a@test.com", Some("PRO"), 1.0, Some(now + 1800), Some(80));  // 30分钟后, 80%配额
        let reset_b = create_test_token("b@test.com", Some("PRO"), 1.0, Some(now + 2100), Some(50));  // 35分钟后, 50%配额

        // 差5分钟 < 10分钟阈值，视为相同，按配额排序（80% > 50%）
        assert_eq!(compare_tokens(&reset_a, &reset_b), Ordering::Less);
    }

    #[test]
    fn test_sorting_reset_time_beyond_threshold() {
        let now = chrono::Utc::now().timestamp();

        // 差异超过10分钟，按刷新时间排序（忽略配额）
        let soon_low_quota = create_test_token("soon@test.com", Some("PRO"), 1.0, Some(now + 1800), Some(20));   // 30分钟后, 20%
        let late_high_quota = create_test_token("late@test.com", Some("PRO"), 1.0, Some(now + 18000), Some(90)); // 5小时后, 90%

        // 差4.5小时 > 10分钟，刷新时间优先，30分钟 < 5小时
        assert_eq!(compare_tokens(&soon_low_quota, &late_high_quota), Ordering::Less);
    }

    #[test]
    fn test_sorting_quota_fallback() {
        // 其他条件相同时，配额高的优先
        let high_quota = create_test_token("high@test.com", Some("PRO"), 1.0, None, Some(80));
        let low_quota = create_test_token("low@test.com", Some("PRO"), 1.0, None, Some(20));

        assert_eq!(compare_tokens(&high_quota, &low_quota), Ordering::Less);
        assert_eq!(compare_tokens(&low_quota, &high_quota), Ordering::Greater);
    }

    #[test]
    fn test_sorting_missing_reset_time() {
        let now = chrono::Utc::now().timestamp();

        // 没有 reset_time 的账号应该排在有 reset_time 的后面
        let with_reset = create_test_token("with@test.com", Some("PRO"), 1.0, Some(now + 1800), Some(50));
        let without_reset = create_test_token("without@test.com", Some("PRO"), 1.0, None, Some(50));

        assert_eq!(compare_tokens(&with_reset, &without_reset), Ordering::Less);
    }

    #[test]
    fn test_full_sorting_integration() {
        let now = chrono::Utc::now().timestamp();

        let mut tokens = vec![
            create_test_token("free_high@test.com", Some("FREE"), 1.0, Some(now + 1800), Some(90)),
            create_test_token("pro_low_health@test.com", Some("PRO"), 0.5, Some(now + 1800), Some(90)),
            create_test_token("pro_soon@test.com", Some("PRO"), 1.0, Some(now + 1800), Some(50)),   // 30分钟后
            create_test_token("pro_late@test.com", Some("PRO"), 1.0, Some(now + 18000), Some(90)),  // 5小时后
            create_test_token("ultra@test.com", Some("ULTRA"), 1.0, Some(now + 36000), Some(10)),
        ];

        tokens.sort_by(compare_tokens);

        // 预期顺序:
        // 1. ULTRA (最高等级，即使刷新时间最远)
        // 2. PRO + 高健康分 + 30分钟后刷新
        // 3. PRO + 高健康分 + 5小时后刷新
        // 4. PRO + 低健康分
        // 5. FREE (最低等级，即使配额最高)
        assert_eq!(tokens[0].email, "ultra@test.com");
        assert_eq!(tokens[1].email, "pro_soon@test.com");
        assert_eq!(tokens[2].email, "pro_late@test.com");
        assert_eq!(tokens[3].email, "pro_low_health@test.com");
        assert_eq!(tokens[4].email, "free_high@test.com");
    }

    #[test]
    fn test_realistic_scenario() {
        // 模拟用户描述的场景:
        // a 账号 claude 4h55m 后刷新
        // b 账号 claude 31m 后刷新
        // 应该优先使用 b（31分钟后刷新）
        let now = chrono::Utc::now().timestamp();

        let account_a = create_test_token("a@test.com", Some("PRO"), 1.0, Some(now + 295 * 60), Some(80)); // 4h55m
        let account_b = create_test_token("b@test.com", Some("PRO"), 1.0, Some(now + 31 * 60), Some(30));  // 31m

        // b 应该排在 a 前面（刷新时间更近）
        assert_eq!(compare_tokens(&account_b, &account_a), Ordering::Less);

        let mut tokens = vec![account_a.clone(), account_b.clone()];
        tokens.sort_by(compare_tokens);

        assert_eq!(tokens[0].email, "b@test.com");
        assert_eq!(tokens[1].email, "a@test.com");
    }

    #[test]
    fn test_extract_earliest_reset_time() {
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        // 测试包含 claude 模型的 reset_time 提取
        let account_with_claude = serde_json::json!({
            "quota": {
                "models": [
                    {"name": "gemini-flash", "reset_time": "2025-01-31T10:00:00Z"},
                    {"name": "claude-sonnet", "reset_time": "2025-01-31T08:00:00Z"},
                    {"name": "claude-opus", "reset_time": "2025-01-31T08:00:00Z"}
                ]
            }
        });

        let result = manager.extract_earliest_reset_time(&account_with_claude);
        assert!(result.is_some());
        // 应该返回 claude 的时间（08:00）而不是 gemini 的（10:00）
        let expected_ts = chrono::DateTime::parse_from_rfc3339("2025-01-31T08:00:00Z")
            .unwrap()
            .timestamp();
        assert_eq!(result.unwrap(), expected_ts);
    }

    #[test]
    fn test_extract_reset_time_no_claude() {
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        // 没有 claude 模型时，应该取任意模型的最近时间
        let account_no_claude = serde_json::json!({
            "quota": {
                "models": [
                    {"name": "gemini-flash", "reset_time": "2025-01-31T10:00:00Z"},
                    {"name": "gemini-pro", "reset_time": "2025-01-31T08:00:00Z"}
                ]
            }
        });

        let result = manager.extract_earliest_reset_time(&account_no_claude);
        assert!(result.is_some());
        let expected_ts = chrono::DateTime::parse_from_rfc3339("2025-01-31T08:00:00Z")
            .unwrap()
            .timestamp();
        assert_eq!(result.unwrap(), expected_ts);
    }

    #[test]
    fn test_extract_reset_time_missing_quota() {
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        // 没有 quota 字段时应返回 None
        let account_no_quota = serde_json::json!({
            "email": "test@test.com"
        });

        assert!(manager.extract_earliest_reset_time(&account_no_quota).is_none());
    }

    // ===== P2C 算法测试 =====

    /// 创建带 protected_models 的测试 Token
    fn create_test_token_with_protected(
        email: &str,
        remaining_quota: Option<i32>,
        protected_models: HashSet<String>,
    ) -> ProxyToken {
        ProxyToken {
            account_id: email.to_string(),
            access_token: "test_token".to_string(),
            refresh_token: "test_refresh".to_string(),
            expires_in: 3600,
            timestamp: chrono::Utc::now().timestamp() + 3600,
            email: email.to_string(),
            account_path: PathBuf::from("/tmp/test"),
            project_id: None,
            subscription_tier: Some("PRO".to_string()),
            remaining_quota,
            protected_models,
            health_score: 1.0,
            reset_time: None,
            validation_blocked: false,
            validation_blocked_until: 0,
            validation_url: None,
            model_quotas: HashMap::new(),
            model_limits: HashMap::new(),
        }
    }

    #[test]
    fn test_p2c_selects_higher_quota() {
        // P2C 应选择配额更高的账号
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        let low_quota = create_test_token("low@test.com", Some("PRO"), 1.0, None, Some(20));
        let high_quota = create_test_token("high@test.com", Some("PRO"), 1.0, None, Some(80));

        let candidates = vec![low_quota, high_quota];
        let attempted: HashSet<String> = HashSet::new();

        // 运行多次确保选择高配额账号
        for _ in 0..10 {
            let result = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet", false);
            assert!(result.is_some());
            // P2C 从两个候选中选择配额更高的
            // 由于只有两个候选，应该总是选择 high_quota
            assert_eq!(result.unwrap().email, "high@test.com");
        }
    }

    #[test]
    fn test_p2c_skips_attempted() {
        // P2C 应跳过已尝试的账号
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        let token_a = create_test_token("a@test.com", Some("PRO"), 1.0, None, Some(80));
        let token_b = create_test_token("b@test.com", Some("PRO"), 1.0, None, Some(50));

        let candidates = vec![token_a, token_b];
        let mut attempted: HashSet<String> = HashSet::new();
        attempted.insert("a@test.com".to_string());

        let result = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet", false);
        assert!(result.is_some());
        assert_eq!(result.unwrap().email, "b@test.com");
    }

    #[test]
    fn test_p2c_skips_protected_models() {
        // P2C 应跳过对目标模型有保护的账号 (quota_protection_enabled = true)
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        let mut protected = HashSet::new();
        protected.insert("claude-sonnet".to_string());

        let protected_account = create_test_token_with_protected("protected@test.com", Some(90), protected);
        let normal_account = create_test_token_with_protected("normal@test.com", Some(50), HashSet::new());

        let candidates = vec![protected_account, normal_account];
        let attempted: HashSet<String> = HashSet::new();

        let result = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet", true);
        assert!(result.is_some());
        assert_eq!(result.unwrap().email, "normal@test.com");
    }

    #[test]
    fn test_p2c_single_candidate() {
        // 单候选时直接返回
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        let token = create_test_token("single@test.com", Some("PRO"), 1.0, None, Some(50));
        let candidates = vec![token];
        let attempted: HashSet<String> = HashSet::new();

        let result = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet", false);
        assert!(result.is_some());
        assert_eq!(result.unwrap().email, "single@test.com");
    }

    #[test]
    fn test_p2c_empty_candidates() {
        // 空候选返回 None
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        let candidates: Vec<ProxyToken> = vec![];
        let attempted: HashSet<String> = HashSet::new();

        let result = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet", false);
        assert!(result.is_none());
    }

    #[test]
    fn test_p2c_all_attempted() {
        // 所有账号都已尝试时返回 None
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        let token_a = create_test_token("a@test.com", Some("PRO"), 1.0, None, Some(80));
        let token_b = create_test_token("b@test.com", Some("PRO"), 1.0, None, Some(50));

        let candidates = vec![token_a, token_b];
        let mut attempted: HashSet<String> = HashSet::new();
        attempted.insert("a@test.com".to_string());
        attempted.insert("b@test.com".to_string());

        let result = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet", false);
        assert!(result.is_none());
    }

    // ===== Ultra 优先逻辑测试 =====

    /// 测试 is_ultra_required_model 辅助函数
    #[test]
    fn test_is_ultra_required_model() {
        // 需要 Ultra 账号的高端模型
        const ULTRA_REQUIRED_MODELS: &[&str] = &[
            "claude-opus-4-6",
            "claude-opus-4-5",
            "opus",
        ];

        fn is_ultra_required_model(model: &str) -> bool {
            let lower = model.to_lowercase();
            ULTRA_REQUIRED_MODELS.iter().any(|m| lower.contains(m))
        }

        // 应该识别为高端模型
        assert!(is_ultra_required_model("claude-opus-4-6"));
        assert!(is_ultra_required_model("claude-opus-4-5"));
        assert!(is_ultra_required_model("Claude-Opus-4-6")); // 大小写不敏感
        assert!(is_ultra_required_model("CLAUDE-OPUS-4-5")); // 大小写不敏感
        assert!(is_ultra_required_model("opus")); // 通配匹配
        assert!(is_ultra_required_model("opus-4-6-latest"));
        assert!(is_ultra_required_model("models/claude-opus-4-6"));

        // 应该识别为普通模型
        assert!(!is_ultra_required_model("claude-sonnet-4-5"));
        assert!(!is_ultra_required_model("claude-sonnet"));
        assert!(!is_ultra_required_model("gemini-1.5-flash"));
        assert!(!is_ultra_required_model("gemini-2.0-pro"));
        assert!(!is_ultra_required_model("claude-haiku"));
    }

    /// 测试高端模型排序：Ultra 账号优先于 Pro 账号（即使 Pro 配额更高）
    #[test]
    fn test_ultra_priority_for_high_end_models() {
        const RESET_TIME_THRESHOLD_SECS: i64 = 600;

        // 模拟高端模型排序逻辑
        fn compare_tokens_for_model(a: &ProxyToken, b: &ProxyToken, target_model: &str) -> Ordering {
            const ULTRA_REQUIRED_MODELS: &[&str] = &["claude-opus-4-6", "claude-opus-4-5", "opus"];
            let requires_ultra = {
                let lower = target_model.to_lowercase();
                ULTRA_REQUIRED_MODELS.iter().any(|m| lower.contains(m))
            };

            let tier_priority = |tier: &Option<String>| {
                let t = tier.as_deref().unwrap_or("").to_lowercase();
                if t.contains("ultra") { 0 }
                else if t.contains("pro") { 1 }
                else if t.contains("free") { 2 }
                else { 3 }
            };

            // Priority 0: 高端模型时，订阅等级优先
            if requires_ultra {
                let tier_cmp = tier_priority(&a.subscription_tier)
                    .cmp(&tier_priority(&b.subscription_tier));
                if tier_cmp != Ordering::Equal {
                    return tier_cmp;
                }
            }

            // Priority 1: Quota (higher is better)
            let quota_a = a.remaining_quota.unwrap_or(0);
            let quota_b = b.remaining_quota.unwrap_or(0);
            let quota_cmp = quota_b.cmp(&quota_a);
            if quota_cmp != Ordering::Equal {
                return quota_cmp;
            }

            // Priority 2: Health score
            let health_cmp = b.health_score.partial_cmp(&a.health_score)
                .unwrap_or(Ordering::Equal);
            if health_cmp != Ordering::Equal {
                return health_cmp;
            }

            // Priority 3: Tier (for non-high-end models)
            if !requires_ultra {
                let tier_cmp = tier_priority(&a.subscription_tier)
                    .cmp(&tier_priority(&b.subscription_tier));
                if tier_cmp != Ordering::Equal {
                    return tier_cmp;
                }
            }

            Ordering::Equal
        }

        // 创建测试账号：Ultra 低配额 vs Pro 高配额
        let ultra_low_quota = create_test_token("ultra@test.com", Some("ULTRA"), 1.0, None, Some(20));
        let pro_high_quota = create_test_token("pro@test.com", Some("PRO"), 1.0, None, Some(80));

        // 高端模型 (Opus 4.6): Ultra 应该优先，即使配额低
        assert_eq!(
            compare_tokens_for_model(&ultra_low_quota, &pro_high_quota, "claude-opus-4-6"),
            Ordering::Less, // Ultra 排在前面
            "Opus 4.6 should prefer Ultra account over Pro even with lower quota"
        );

        // 高端模型 (Opus 4.5): Ultra 应该优先
        assert_eq!(
            compare_tokens_for_model(&ultra_low_quota, &pro_high_quota, "claude-opus-4-5"),
            Ordering::Less,
            "Opus 4.5 should prefer Ultra account over Pro"
        );

        // 普通模型 (Sonnet): 高配额 Pro 应该优先
        assert_eq!(
            compare_tokens_for_model(&ultra_low_quota, &pro_high_quota, "claude-sonnet-4-5"),
            Ordering::Greater, // Pro (高配额) 排在前面
            "Sonnet should prefer high-quota Pro over low-quota Ultra"
        );

        // 普通模型 (Flash): 高配额 Pro 应该优先
        assert_eq!(
            compare_tokens_for_model(&ultra_low_quota, &pro_high_quota, "gemini-1.5-flash"),
            Ordering::Greater,
            "Flash should prefer high-quota Pro over low-quota Ultra"
        );
    }

    /// 测试排序：同为 Ultra 时按配额排序
    #[test]
    fn test_ultra_accounts_sorted_by_quota() {
        fn compare_tokens_for_model(a: &ProxyToken, b: &ProxyToken, target_model: &str) -> Ordering {
            const ULTRA_REQUIRED_MODELS: &[&str] = &["claude-opus-4-6", "claude-opus-4-5", "opus"];
            let requires_ultra = {
                let lower = target_model.to_lowercase();
                ULTRA_REQUIRED_MODELS.iter().any(|m| lower.contains(m))
            };

            let tier_priority = |tier: &Option<String>| {
                let t = tier.as_deref().unwrap_or("").to_lowercase();
                if t.contains("ultra") { 0 }
                else if t.contains("pro") { 1 }
                else if t.contains("free") { 2 }
                else { 3 }
            };

            if requires_ultra {
                let tier_cmp = tier_priority(&a.subscription_tier)
                    .cmp(&tier_priority(&b.subscription_tier));
                if tier_cmp != Ordering::Equal {
                    return tier_cmp;
                }
            }

            let quota_a = a.remaining_quota.unwrap_or(0);
            let quota_b = b.remaining_quota.unwrap_or(0);
            quota_b.cmp(&quota_a)
        }

        let ultra_high = create_test_token("ultra_high@test.com", Some("ULTRA"), 1.0, None, Some(80));
        let ultra_low = create_test_token("ultra_low@test.com", Some("ULTRA"), 1.0, None, Some(20));

        // Opus 4.6: 同为 Ultra，高配额优先
        assert_eq!(
            compare_tokens_for_model(&ultra_high, &ultra_low, "claude-opus-4-6"),
            Ordering::Less, // ultra_high 排在前面
            "Among Ultra accounts, higher quota should come first"
        );
    }

    /// 测试完整排序场景：混合账号池
    #[test]
    fn test_full_sorting_mixed_accounts() {
        fn sort_tokens_for_model(tokens: &mut Vec<ProxyToken>, target_model: &str) {
            const ULTRA_REQUIRED_MODELS: &[&str] = &["claude-opus-4-6", "claude-opus-4-5", "opus"];
            let requires_ultra = {
                let lower = target_model.to_lowercase();
                ULTRA_REQUIRED_MODELS.iter().any(|m| lower.contains(m))
            };

            tokens.sort_by(|a, b| {
                let tier_priority = |tier: &Option<String>| {
                    let t = tier.as_deref().unwrap_or("").to_lowercase();
                    if t.contains("ultra") { 0 }
                    else if t.contains("pro") { 1 }
                    else if t.contains("free") { 2 }
                    else { 3 }
                };

                if requires_ultra {
                    let tier_cmp = tier_priority(&a.subscription_tier)
                        .cmp(&tier_priority(&b.subscription_tier));
                    if tier_cmp != Ordering::Equal {
                        return tier_cmp;
                    }
                }

                let quota_a = a.remaining_quota.unwrap_or(0);
                let quota_b = b.remaining_quota.unwrap_or(0);
                let quota_cmp = quota_b.cmp(&quota_a);
                if quota_cmp != Ordering::Equal {
                    return quota_cmp;
                }

                if !requires_ultra {
                    let tier_cmp = tier_priority(&a.subscription_tier)
                        .cmp(&tier_priority(&b.subscription_tier));
                    if tier_cmp != Ordering::Equal {
                        return tier_cmp;
                    }
                }

                Ordering::Equal
            });
        }

        // 创建混合账号池
        let ultra_high = create_test_token("ultra_high@test.com", Some("ULTRA"), 1.0, None, Some(80));
        let ultra_low = create_test_token("ultra_low@test.com", Some("ULTRA"), 1.0, None, Some(20));
        let pro_high = create_test_token("pro_high@test.com", Some("PRO"), 1.0, None, Some(90));
        let pro_low = create_test_token("pro_low@test.com", Some("PRO"), 1.0, None, Some(30));
        let free = create_test_token("free@test.com", Some("FREE"), 1.0, None, Some(100));

        // 高端模型 (Opus 4.6) 排序
        let mut tokens_opus = vec![pro_high.clone(), free.clone(), ultra_low.clone(), pro_low.clone(), ultra_high.clone()];
        sort_tokens_for_model(&mut tokens_opus, "claude-opus-4-6");

        let emails_opus: Vec<&str> = tokens_opus.iter().map(|t| t.email.as_str()).collect();
        // 期望顺序: Ultra(高配额) > Ultra(低配额) > Pro(高配额) > Pro(低配额) > Free
        assert_eq!(
            emails_opus,
            vec!["ultra_high@test.com", "ultra_low@test.com", "pro_high@test.com", "pro_low@test.com", "free@test.com"],
            "Opus 4.6 should sort Ultra first, then by quota within each tier"
        );

        // 普通模型 (Sonnet) 排序
        let mut tokens_sonnet = vec![pro_high.clone(), free.clone(), ultra_low.clone(), pro_low.clone(), ultra_high.clone()];
        sort_tokens_for_model(&mut tokens_sonnet, "claude-sonnet-4-5");

        let emails_sonnet: Vec<&str> = tokens_sonnet.iter().map(|t| t.email.as_str()).collect();
        // 期望顺序: Free(100%) > Pro(90%) > Ultra(80%) > Pro(30%) > Ultra(20%) - 按配额优先
        assert_eq!(
            emails_sonnet,
            vec!["free@test.com", "pro_high@test.com", "ultra_high@test.com", "pro_low@test.com", "ultra_low@test.com"],
            "Sonnet should sort by quota first, then by tier as tiebreaker"
        );
    }
}
