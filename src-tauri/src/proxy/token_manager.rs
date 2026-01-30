
// 移除冗余的顶层导入，因为这些在代码中已由 full path 或局部导入处理
use dashmap::DashMap;
use std::collections::{HashSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::proxy::rate_limit::RateLimitTracker;
use crate::proxy::sticky_config::StickySessionConfig;
// 移除冗余的顶层导入，因为这些在代码中已由 full path 或局部导入处理




// [NEW] Token Lease RAII Guard
#[derive(Debug)]
pub struct TokenLease {
    pub access_token: String,
    pub project_id: String,
    pub email: String,
    pub account_id: String,
    active_requests: Arc<DashMap<String, AtomicUsize>>,
}

impl Drop for TokenLease {
    fn drop(&mut self) {
        if let Some(counter) = self.active_requests.get(&self.account_id) {
            counter.fetch_sub(1, Ordering::SeqCst);
            tracing::debug!("⬇️ Connection released: {} (active: {})", self.email, counter.load(Ordering::SeqCst));
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProxyToken {
    pub account_id: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub timestamp: i64,
    pub email: String,
    pub account_path: PathBuf,  // 账号文件路径，用于更新
    pub project_id: Option<String>,
    pub subscription_tier: Option<String>, // "FREE" | "PRO" | "ULTRA"
    pub remaining_quota: Option<i32>, // [FIX #563] Remaining quota for priority sorting
    pub protected_models: HashSet<String>, // [NEW #621]
    pub health_score: f32, // [NEW] 健康分数 (0.0 - 1.0)
    pub model_quotas: HashMap<String, i32>, // [NEW] Strict Model Quotas (Remaining %)
}


pub struct TokenManager {
    tokens: Arc<DashMap<String, ProxyToken>>,  // account_id -> ProxyToken
    current_index: Arc<AtomicUsize>,
    last_used_account: Arc<tokio::sync::Mutex<Option<(String, std::time::Instant)>>>,
    data_dir: PathBuf,
    rate_limit_tracker: Arc<RateLimitTracker>,  // 新增: 限流跟踪器
    sticky_config: Arc<tokio::sync::RwLock<StickySessionConfig>>, // 新增：调度配置
    session_accounts: Arc<DashMap<String, (String, std::time::Instant)>>, // [FIX] Store timestamp for cleanup
    preferred_account_id: Arc<tokio::sync::RwLock<Option<String>>>, // [FIX #820] 优先使用的账号ID（固定账号模式）
    health_scores: Arc<DashMap<String, f32>>, // account_id -> health_score
    active_requests: Arc<DashMap<String, AtomicUsize>>, // [NEW] Least Connections tracking
    circuit_breaker_config: Arc<tokio::sync::RwLock<crate::models::CircuitBreakerConfig>>, // [NEW] 熔断配置缓存
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
            active_requests: Arc::new(DashMap::new()),
            circuit_breaker_config: Arc::new(tokio::sync::RwLock::new(crate::models::CircuitBreakerConfig::default())),
        }
    }

    /// 启动限流记录自动清理后台任务（每15秒检查并清除过期记录）
    pub fn start_auto_cleanup(&self) {
        let tracker = self.rate_limit_tracker.clone();
        let session_map = self.session_accounts.clone(); // [FIX] Capture session map
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
            let mut session_cleanup_interval = 0; // Run session cleanup every ~10 minutes (40 ticks)

            loop {
                interval.tick().await;
                let cleaned = tracker.cleanup_expired();
                if cleaned > 0 {
                    tracing::info!("🧹 Auto-cleanup: Removed {} expired rate limit record(s)", cleaned);
                }

                // Session Cleanup (Every 10 mins)
                session_cleanup_interval += 1;
                if session_cleanup_interval >= 40 {
                    session_cleanup_interval = 0;
                    let now = std::time::Instant::now();
                    let expiry = std::time::Duration::from_secs(24 * 3600); // 24h retention
                    let mut removed_sessions = 0;
                    
                    session_map.retain(|_, (_, ts)| {
                        if now.duration_since(*ts) > expiry {
                            removed_sessions += 1;
                            false
                        } else {
                            true
                        }
                    });

                    if removed_sessions > 0 {
                        tracing::info!("🧹 Session Cleanup: Removed {} expired sessions", removed_sessions);
                    }
                }
            }
        });
        tracing::info!("✅ Rate limit & Session auto-cleanup task started");
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
                },
                Ok(None) => {
                    // 跳过无效账号
                },
                Err(e) => {
                    tracing::debug!("加载账号失败 {:?}: {}", path, e);
                }
            }
        }
        
        Ok(count)
    }

    /// 重新加载指定账号（用于配额更新后的实时同步）
    pub async fn reload_account(&self, account_id: &str) -> Result<(), String> {
        let path = self.data_dir.join("accounts").join(format!("{}.json", account_id));
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
            Ok(None) => Err("账号加载失败".to_string()),
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
    
    /// 加载单个账号
    async fn load_single_account(&self, path: &PathBuf) -> Result<Option<ProxyToken>, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("读取文件失败: {}", e))?;
        
        let mut account: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| format!("解析 JSON 失败: {}", e))?;

        if account
            .get("disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            tracing::debug!(
                "Skipping disabled account file: {:?} (email={})",
                path,
                account.get("email").and_then(|v| v.as_str()).unwrap_or("<unknown>")
            );
            return Ok(None);
        }

        // 【新增】配额保护检查 - 在检查 proxy_disabled 之前执行
        // 这样可以在加载时自动恢复配额已恢复的账号
        if self.check_and_protect_quota(&mut account, path).await {
            tracing::debug!(
                "Account skipped due to quota protection: {:?} (email={})",
                path,
                account.get("email").and_then(|v| v.as_str()).unwrap_or("<unknown>")
            );
            return Ok(None);
        }

        // 检查主动禁用状态
        if account
            .get("proxy_disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            tracing::debug!(
                "Skipping proxy-disabled account file: {:?} (email={})",
                path,
                account.get("email").and_then(|v| v.as_str()).unwrap_or("<unknown>")
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
        let project_id = token_obj.get("project_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        
        
        // 【新增】提取订阅等级 (subscription_tier 为 "FREE" | "PRO" | "ULTRA")
        let subscription_tier = account.get("quota")
            .and_then(|q| q.get("subscription_tier"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        
        // [FIX #563] 提取最大剩余配额百分比用于优先级排序 (Option<i32> now)
        let remaining_quota = account.get("quota")
            .and_then(|q| self.calculate_quota_stats(q));
            // .filter(|&r| r > 0); // 移除 >0 过滤，因为 0% 也是有效数据，只是优先级低
        
        // 【新增 #621】提取受限模型列表
        let protected_models: HashSet<String> = account.get("protected_models")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        // [NEW] 提取所有模型的剩余配额映射
        let mut model_quotas = HashMap::new();
        if let Some(models) = account.get("quota").and_then(|q| q.get("models")).and_then(|m| m.as_array()) {
            for m in models {
                if let (Some(name), Some(pct)) = (m.get("name").and_then(|v| v.as_str()), m.get("percentage").and_then(|v| v.as_i64())) {
                    model_quotas.insert(name.to_string(), pct as i32);
                }
            }
        }
        
        let health_score = self.health_scores.get(&account_id).map(|v| *v).unwrap_or(1.0);
        
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
            model_quotas, // [NEW]
        }))
    }

    
    /// 检查账号是否应该被配额保护
    /// 如果配额低于阈值，自动禁用账号并返回 true
    async fn check_and_protect_quota(&self, account_json: &mut serde_json::Value, account_path: &PathBuf) -> bool {
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

        // 3. 检查是否已经被账号级或模型级配额保护禁用
        let is_proxy_disabled = account_json.get("proxy_disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        
        let reason = account_json.get("proxy_disabled_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        if is_proxy_disabled {
            if reason == "quota_protection" {
                // [兼容性 #621] 如果是被旧版账号级保护禁用的，尝试恢复并转为模型级
                return self.check_and_restore_quota(account_json, account_path, &quota, &config).await;
            }
            return true; // 其他原因禁用，跳过加载
        }
        
        // 4. 获取模型列表
        let models = match quota.get("models").and_then(|m| m.as_array()) {
            Some(m) => m,
            None => return false,
        };

        // 5. 遍历受监控的模型，检查保护与恢复
        let threshold = config.threshold_percentage as i32;


        let mut changed = false;

        for model in models {
            let name = model.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if !config.monitored_models.iter().any(|m| m == name) {
                continue; 
            }

            let percentage = model.get("percentage").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let account_id = account_json.get("id").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();

            if percentage <= threshold {
                // 触发保护 (Issue #621 改为模型级)
                if self.trigger_quota_protection(account_json, &account_id, account_path, percentage, threshold, name).await.unwrap_or(false) {
                    changed = true;
                }
            } else {
                // 尝试恢复 (如果之前受限)
                let protected_models = account_json.get("protected_models").and_then(|v| v.as_array());
                let is_protected = protected_models.map_or(false, |arr| {
                    arr.iter().any(|m| m.as_str() == Some(name))
                });

                if is_protected {
                    if self.restore_quota_protection(account_json, &account_id, account_path, name).await.unwrap_or(false) {
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
        if !protected_models.iter().any(|m| m.as_str() == Some(model_name)) {
            protected_models.push(serde_json::Value::String(model_name.to_string()));
            
            tracing::info!(
                "账号 {} 的模型 {} 因配额受限（{}% <= {}%）已被加入保护列表",
                account_id, model_name, current_val, threshold
            );
            
            // 3. 写入磁盘
            let json_str = serde_json::to_string_pretty(account_json)
                .map_err(|e| format!("序列化 JSON 失败: {}", e))?;
            
            std::fs::write(account_path, json_str)
                .map_err(|e| format!("写入文件失败: {}", e))?;
            
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
            account_json.get("email").and_then(|v| v.as_str()).unwrap_or("unknown")
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
        
        if let Ok(json_str) = serde_json::to_string_pretty(account_json) {
             if let Err(e) = std::fs::write(account_path, json_str) {
                 tracing::error!("[check_and_restore_quota] Failed to write account file: {}", e);
             }
        } else {
             tracing::error!("[check_and_restore_quota] Failed to serialize account json");
        }
        
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
        if let Some(arr) = account_json.get_mut("protected_models").and_then(|v| v.as_array_mut()) {
            let original_len = arr.len();
            arr.retain(|m| m.as_str() != Some(model_name));
            
            if arr.len() < original_len {
                tracing::info!("账号 {} 的模型 {} 配额已恢复，移出保护列表", account_id, model_name);
                let json_str = serde_json::to_string_pretty(account_json)
                    .map_err(|e| format!("序列化 JSON 失败: {}", e))?;
                    
                std::fs::write(account_path, json_str)
                    .map_err(|e| format!("写入文件失败: {}", e))?;
                return Ok(true);
            }
        }
        
        Ok(false)
    }

    /// 尝试刷新 Token (如果即将过期)
    async fn try_refresh_token_if_needed(&self, token: &mut ProxyToken) {
        let now = chrono::Utc::now().timestamp();
        if now >= token.timestamp - 300 {
            tracing::debug!("账号 {} 的 token 即将过期，正在刷新...", token.email);
            match crate::modules::oauth::refresh_access_token(&token.refresh_token).await {
                Ok(token_response) => {
                    token.access_token = token_response.access_token.clone();
                    token.expires_in = token_response.expires_in;
                    token.timestamp = now + token_response.expires_in;

                    if let Some(mut entry) = self.tokens.get_mut(&token.account_id) {
                        entry.access_token = token.access_token.clone();
                        entry.expires_in = token.expires_in;
                        entry.timestamp = token.timestamp;
                    }
                    let _ = self.save_refreshed_token(&token.account_id, &token_response).await;
                }
                Err(e) => {
                    tracing::warn!("Token refresh failed: {}", e);
                }
            }
        }
    }

    /// 确保 Token 包含 Project ID (如果缺少则获取)
    async fn ensure_project_id(&self, token: &mut ProxyToken) -> String {
        if let Some(pid) = &token.project_id {
            pid.clone()
        } else {
            match crate::proxy::project_resolver::fetch_project_id(&token.access_token).await {
                Ok(pid) => {
                    // Update cache
                    if let Some(mut entry) = self.tokens.get_mut(&token.account_id) {
                        entry.project_id = Some(pid.clone());
                    }
                    // Persist to disk logic (optional/simplified here as save_project_id is implied available or need implementation)
                    // Assuming save_project_id allows async
                    let _ = self.save_project_id(&token.account_id, &pid).await;
                    pid
                }
                Err(_) => crate::proxy::project_resolver::generate_mock_project_id() // fallback to random ID
            }
        }
    }
    
    // Placeholder for save methods if they are not nearby, but assuming they exist in the file based on context.
    // If save_refreshed_token / save_project_id are not standard methods on self (probably defined later in the file),
    // I need to be careful.
    // based on original code loop:
    // let _ = self.save_refreshed_token(&token.account_id, &token_response).await;
    // So they exist on `self`.


    
    /// 获取当前可用的 Token（支持粘性会话与智能调度）
    /// 参数 `quota_group` 用于区分 "claude" vs "gemini" 组
    /// 参数 `force_rotate` 为 true 时将忽略锁定，强制切换账号
    /// 参数 `session_id` 用于跨请求维持会话粘性
    /// 参数 `target_model` 用于检查配额保护 (Issue #621)
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
    ) -> Result<TokenLease, String> {
        // 【优化 Issue #284】添加 120 秒超时 (因为 CacheFirst/Fixed 模式可能需要等待)
        let timeout_duration = std::time::Duration::from_secs(120);
        match tokio::time::timeout(timeout_duration, self.get_token_internal(quota_group, force_rotate, session_id, target_model)).await {
            Ok(result) => result,
            Err(_) => Err("Token acquisition timeout (5s) - system too busy or deadlock detected".to_string()),
        }
    }

    /// 内部实现：获取 Token 的核心逻辑
    async fn get_token_internal(
        &self, 
        quota_group: &str, 
        force_rotate: bool, 
        session_id: Option<&str>,
        target_model: &str,
    ) -> Result<TokenLease, String> {
        let mut tokens_snapshot: Vec<ProxyToken> = self.tokens.iter().map(|e| e.value().clone()).collect();
        let total = tokens_snapshot.len();
        if total == 0 {
            return Err("Token pool is empty".to_string());
        }

        // ===== 【核心过滤】严格剔除配额为 0 的账号 [NEW] =====
        // 在任何排序和选择之前，必须先剔除完全不可用的账号
        // 1. 归一化目标模型名为标准 ID (e.g. "claude-3-opus" -> "claude-3-opus-20240229")
        let normalized_target = crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
            .unwrap_or_else(|| target_model.to_string());
        
        tokens_snapshot.retain(|t| {
            // 检查模型特定配额
            // 逻辑: 如果 model_quotas 中存在该 key (不管是原始 key 还是 normalized key)，且 value <= 0，则剔除
            
            // case 1: check exact match
            if let Some(&pct) = t.model_quotas.get(target_model) {
                if pct <= 0 { return false; }
            }
            
            // case 2: check normalized match (if different)
            if normalized_target != target_model {
                if let Some(&pct) = t.model_quotas.get(&normalized_target) {
                    if pct <= 0 { return false; }
                }
            }

            // case 3: fuzzy lookup (iterate keys) - fallback if needed, but exact/normalized should cover 99%
            // 如果确实找不到 quota info，我们默认为可用 (保守策略)
            true
        });

        if tokens_snapshot.is_empty() {
             return Err(format!("No accounts available with remaining quota > 0 for model '{}'", target_model));
        }

        // ===== 【优化】根据订阅等级和剩余配额排序 =====
        // [FIX #563] 优先级: ULTRA > PRO > FREE, 同tier内优先高配额账号
        // 理由: ULTRA/PRO 重置快，优先消耗；FREE 重置慢，用于兜底
        //       高配額账号优先使用，避免低配额账号被用光
        tokens_snapshot.sort_by(|a, b| {
            // [SMART-SPILLOVER] Active Requests Limits
            // Define limits based on tier
            let get_concurrency_limit = |tier: &Option<String>| -> usize {
                match tier.as_deref() {
                    Some(t) if t.contains("ultra") => 8, // High concurrency for Ultra
                    Some(t) if t.contains("pro") => 3,   // Moderate for Pro
                    Some(_) => 1,                        // Strict for Free/Standard
                    None => 1,
                }
            };

            let limit_a = get_concurrency_limit(&a.subscription_tier);
            let limit_b = get_concurrency_limit(&b.subscription_tier);

            let active_a = self.active_requests.get(&a.account_id).map(|c| c.load(Ordering::SeqCst)).unwrap_or(0);
            let active_b = self.active_requests.get(&b.account_id).map(|c| c.load(Ordering::SeqCst)).unwrap_or(0);

            let overloaded_a = active_a >= limit_a;
            let overloaded_b = active_b >= limit_b;

            // [CRITICAL] Availability First: Non-overloaded accounts always beat overloaded ones
            if overloaded_a != overloaded_b {
                if overloaded_a {
                    return std::cmp::Ordering::Greater; // A is overloaded, B is not -> B wins (Less)
                } else {
                    return std::cmp::Ordering::Less;    // A is not, B is -> A wins (Less)
                }
            }

            // [TIER] If both active statuses are same (both OK or both Overloaded), prefer higher tier
            // Lower value means higher priority (0=Ultra, 1=Pro, 2=Free)
            let tier_priority = |tier: &Option<String>| match tier.as_deref() {
                Some(t) if t.contains("ultra") => 0,
                Some(t) if t.contains("pro") => 1,
                Some(t) if t.contains("free") => 2,
                _ => 3,
            };
            
            // First: compare by subscription tier
            let tier_cmp = tier_priority(&a.subscription_tier)
                .cmp(&tier_priority(&b.subscription_tier));
            
            if tier_cmp != std::cmp::Ordering::Equal {
                return tier_cmp;
            }

            // Second: compare by health score (higher is better)
            let health_cmp = b.health_score.partial_cmp(&a.health_score).unwrap_or(std::cmp::Ordering::Equal);
            if health_cmp != std::cmp::Ordering::Equal {
                return health_cmp;
            }

            // [NEW] Third: compare by active connections (Least Connections strategy)
            // We want accounts with FEWER active requests first.
            let active_a = self.active_requests.get(&a.account_id).map(|c| c.load(Ordering::SeqCst)).unwrap_or(0);
            let active_b = self.active_requests.get(&b.account_id).map(|c| c.load(Ordering::SeqCst)).unwrap_or(0);
            let active_cmp = active_a.cmp(&active_b);

            if active_cmp != std::cmp::Ordering::Equal {
                return active_cmp;
            }
            
            // Fourth: compare by remaining quota percentage (higher is better)
            // Accounts with unknown/zero percentage go last within their tier
            let quota_a = a.remaining_quota.unwrap_or(0);
            let quota_b = b.remaining_quota.unwrap_or(0);
            quota_b.cmp(&quota_a)
        });
        
        // 【调试日志】打印排序后的账号顺序 (TOP 5)
        tracing::debug!(
            "🔄 [Token Rotation] Candidates (Top 5): {:?}",
            tokens_snapshot.iter().take(5).map(|t| {
                let active = self.active_requests.get(&t.account_id).map(|c| c.load(Ordering::SeqCst)).unwrap_or(0);
                format!(
                    "{} [Active:{}, T:{:?}, Q:{:?}]", 
                    t.email, active, t.subscription_tier, t.remaining_quota
                )
            }).collect::<Vec<_>>()
        );

        // 0. 读取当前调度配置
        let scheduling = self.sticky_config.read().await.clone();
        tracing::info!("🔍 [Debug] get_token_internal | Mode: {:?} | Selected Accs: {} | Target: {}", scheduling.mode, scheduling.selected_accounts.len(), target_model);
        use crate::proxy::sticky_config::SchedulingMode;
        
        // 【新增】Selected 模式：仅保留选中的账号
        if scheduling.mode == SchedulingMode::Selected {
            let selected_set: HashSet<&String> = scheduling.selected_accounts.iter().collect();
            
            // Normalized target already computed above
            // let normalized_target = crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
            //     .unwrap_or_else(|| target_model.to_string());

            tokens_snapshot.retain(|t| {
                // 1. 必须在选中的账号列表中
                if !selected_set.contains(&t.account_id) {
                    return false;
                }

                // 2. [NEW] 检查该账号是否配置了模型白名单
                if let Some(allowed_models) = scheduling.selected_models.get(&t.account_id) {
                    if !allowed_models.is_empty() {
                        // 如果配置了白名单，必须包含当前请求的模型
                        // 支持模糊匹配：只要 whitelist item 是 target 的子串，或者 target 是 item 的子串 (宽松匹配)
                        // 或者完全相等
                        let is_allowed = allowed_models.iter().any(|m| {
                            m == target_model || 
                            m == &normalized_target ||
                            target_model.contains(m) || 
                            m.contains(target_model)
                        });

                        if !is_allowed {
                            return false;
                        }
                    }
                }

                // 3. [STRICT] Double check quota again inside selected mode? 
                //    (Already done globally at start, so strictly not needed, but safe to keep logic clean)
                true
            });
            
            if tokens_snapshot.is_empty() {
                return Err(format!("Selected mode is active but no valid accounts matches the selection for model '{}'.", target_model));
            }
            tracing::debug!("🎯 [Selected Mode] Using subset of {} accounts for model {}", tokens_snapshot.len(), target_model);
        }
        
        // 【新增】检查配额保护是否启用（如果关闭，则忽略 protected_models 检查）
        let quota_protection_enabled = crate::modules::config::load_app_config()
            .map(|cfg| cfg.quota_protection.enabled)
            .unwrap_or(false);

        // 【安全修正】过滤后更新 total，防止下标越界
        let total = tokens_snapshot.len();

        // ===== [FIX #820] 固定账号模式：优先使用指定账号 =====
        // ===== [FIX #820] 固定账号模式：优先使用指定账号 (STRICT MODE) =====
        let preferred_id = self.preferred_account_id.read().await.clone();
        if let Some(ref pref_id) = preferred_id {
            // 查找优先账号
            if let Some(preferred_token) = tokens_snapshot.iter().find(|t| &t.account_id == pref_id) {
                // 检查账号是否可用（未限流、未被配额保护）
                let normalized_target = crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                    .unwrap_or_else(|| target_model.to_string());

                // 【修复】Strict Fixed Mode: 如果账号限流，进行等待而不是切换
                let mut retry_count = 0;
                loop {
                    let is_rate_limited = self.is_rate_limited(&preferred_token.account_id, Some(&normalized_target)).await;
                    let is_quota_protected = quota_protection_enabled && preferred_token.protected_models.contains(&normalized_target);

                    if !is_rate_limited && !is_quota_protected {
                        tracing::info!(
                            "🔒 [FIX #820] Using preferred account: {} (fixed mode)",
                            preferred_token.email
                        );

                        // 直接使用优先账号
                        let mut token = preferred_token.clone();
                        // 尝试刷新 Token
                        self.try_refresh_token_if_needed(&mut token).await; 
                        // 确保有 Project ID
                        let project_id = self.ensure_project_id(&mut token).await;

                        // [NEW] Increment active requests
                        self.active_requests.entry(token.account_id.clone())
                            .or_insert(AtomicUsize::new(0))
                            .fetch_add(1, Ordering::SeqCst);
                        
                        tracing::debug!("⬆️ Connection acquired: {} (active: {})", token.email, self.active_requests.get(&token.account_id).unwrap().load(Ordering::SeqCst));

                        return Ok(TokenLease {
                            access_token: token.access_token,
                            project_id,
                            email: token.email,
                            account_id: token.account_id.clone(),
                            active_requests: self.active_requests.clone(),
                        });
                    } else {
                        // 能够等待的情况：仅限 Rate Limit，且等待时间在允许范围内
                        // 配额保护 (Quota Protected) 通常意味着耗尽，等待 60s 也无济于事，除非 Reset 即将到来
                        // 这里简单处理：检查 wait time
                        if is_rate_limited {
                           let key = self.email_to_account_id(&preferred_token.email).unwrap_or_else(|| preferred_token.account_id.clone());
                           let wait_sec = self.rate_limit_tracker.get_remaining_wait(&key, Some(&normalized_target));
                           
                           if wait_sec > 0 && wait_sec <= scheduling.max_wait_seconds {
                               tracing::warn!("🔒 [Strict Fixed] Account {} limited, waiting {}s...", preferred_token.email, wait_sec);
                               tokio::time::sleep(std::time::Duration::from_secs(wait_sec)).await;
                               retry_count += 1;
                               if retry_count < 2 { continue; } // 允许重试一次
                           }
                        }
                        
                        // 无法等待或等待后仍不可用 -> 直接报错，不回退
                        return Err(format!("Preferred account {} is unavailable (Limited/Protected), strict mode enabled.", preferred_token.email));
                    }
                }
            } else {
                 return Err(format!("Preferred account {} not found in pool.", pref_id));
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
            if !rotate && session_id.is_some() && scheduling.mode != SchedulingMode::PerformanceFirst {
                let sid = session_id.unwrap();
                
                // 1. 检查会话是否已绑定账号
                if let Some(bound_entry) = self.session_accounts.get(sid) {
                    let (bound_id, _) = bound_entry.value();
                    let bound_id = bound_id.clone();
                    // Drop reference to avoid deadlock if we need to write later
                    drop(bound_entry); 
                    
                    // Update access time [Async optimization: do it loosely or explicitly update]
                    // We update it if we successfully reuse it.
                    // 【修复】先通过 account_id 找到对应的账号，获取其 email
                    // 2. 转换 email -> account_id 检查绑定的账号是否限流
                    if let Some(bound_token) = tokens_snapshot.iter().find(|t| t.account_id == bound_id) {
                        let key = self.email_to_account_id(&bound_token.email).unwrap_or_else(|| bound_token.account_id.clone());
                        // [FIX] Pass None for specific model wait time if not applicable
                        let reset_sec = self.rate_limit_tracker.get_remaining_wait(&key, Some(&normalized_target));
                        
                        // 【Cache-First Wait Logic】
                        // 如果在 CacheFirst 模式下，且等待时间小于设定阈值，则主动等待
                        if reset_sec > 0 && scheduling.mode == SchedulingMode::CacheFirst && reset_sec <= scheduling.max_wait_seconds {
                             tracing::info!("Sticky Session: Account {} limited ({}s), waiting (max: {}s)...", bound_token.email, reset_sec, scheduling.max_wait_seconds);
                             tokio::time::sleep(std::time::Duration::from_secs(reset_sec)).await;
                             // Wake up and reuse (assuming it cleared, loop/retry logic handled implicitly by reusing bound token if actually cleared)
                             // NOTE: Since we just slept, let's optimistically assume it's clear or try.
                             // But strict check is below. If it's still > 0 after sleep (rare but possible), we fall through.
                             // Better: Continue and let the next check decide.
                        }
                        
                        let reset_sec_after_wait = self.rate_limit_tracker.get_remaining_wait(&key, Some(&normalized_target));

                        if reset_sec_after_wait > 0 {
                            // 【修复 Issue #284】立即解绑并切换账号，不再阻塞等待
                            // 原因：阻塞等待会导致并发请求时客户端 socket 超时 (UND_ERR_SOCKET)
                            tracing::debug!(
                                "Sticky Session: Bound account {} is rate-limited ({}s), unbinding and switching.",
                                bound_token.email, reset_sec
                            );
                            self.session_accounts.remove(sid);
                        } else if !attempted.contains(&bound_id) && !(quota_protection_enabled && bound_token.protected_models.contains(&normalized_target)) {
                            // 3. 账号可用且未被标记为尝试失败，优先复用
                            tracing::debug!("Sticky Session: Successfully reusing bound account {} for session {}", bound_token.email, sid);
                            target_token = Some(bound_token.clone());
                            
                            // [FIX] Update session timestamp to keep it alive
                            if let Some(mut entry) = self.session_accounts.get_mut(sid) {
                                entry.value_mut().1 = std::time::Instant::now();
                            }
                        } else if quota_protection_enabled && bound_token.protected_models.contains(&normalized_target) {
                            tracing::debug!("Sticky Session: Bound account {} is quota-protected for model {} [{}], unbinding and switching.", bound_token.email, normalized_target, target_model);
                            self.session_accounts.remove(sid);
                        }
                    } else {
                        // 绑定的账号已不存在（可能被删除），解绑
                        tracing::debug!("Sticky Session: Bound account not found for session {}, unbinding", sid);
                        self.session_accounts.remove(sid);
                    }
                }
            }

            // 模式 B: 原子化 60s 全局锁定 (针对无 session_id 情况的默认保护)
            // 【修复】性能优先模式应跳过 60s 锁定；
            if target_token.is_none() && !rotate && quota_group != "image_gen" && scheduling.mode != SchedulingMode::PerformanceFirst {
                // 【优化】使用预先获取的快照，不再在循环内加锁
                if let Some((account_id, last_time)) = &last_used_account_id {
                    // [FIX #3] 60s 锁定逻辑应检查 `attempted` 集合，避免重复尝试失败的账号
                    if last_time.elapsed().as_secs() < 60 && !attempted.contains(account_id) {
                        if let Some(found) = tokens_snapshot.iter().find(|t| &t.account_id == account_id) {
                            // 【修复】检查限流状态和配额保护，避免复用已被锁定的账号
                            if !self.is_rate_limited(&found.account_id, Some(&normalized_target)).await && !(quota_protection_enabled && found.protected_models.contains(&normalized_target)) {
                                tracing::debug!("60s Window: Force reusing last account: {}", found.email);
                                target_token = Some(found.clone());
                            } else {
                                if self.is_rate_limited(&found.account_id, Some(&normalized_target)).await {
                                    tracing::debug!("60s Window: Last account {} is rate-limited, skipping", found.email);
                                } else {
                                    tracing::debug!("60s Window: Last account {} is quota-protected for model {} [{}], skipping", found.email, normalized_target, target_model);
                                }
                            }
                        }
                    }
                }
                
                // 若无锁定，则轮询选择新账号
                if target_token.is_none() {
                    let start_idx = self.current_index.fetch_add(1, Ordering::SeqCst) % total;
                    for offset in 0..total {
                        let idx = (start_idx + offset) % total;
                        let candidate = &tokens_snapshot[idx];
                        if attempted.contains(&candidate.account_id) {
                            continue;
                        }

                        // 【新增 #621】模型级限流检查
                        if quota_protection_enabled && candidate.protected_models.contains(&normalized_target) {
                            tracing::debug!("Account {} is quota-protected for model {} [{}], skipping", candidate.email, normalized_target, target_model);
                            continue;
                        }

                        // 【新增】主动避开限流或 5xx 锁定的账号 (高可用优化)
                        if self.is_rate_limited(&candidate.account_id, Some(&normalized_target)).await { // Changed to account_id
                            continue;
                        }

                        target_token = Some(candidate.clone());
                        // 【优化】标记需要更新，稍后统一写回
                        need_update_last_used = Some((candidate.account_id.clone(), std::time::Instant::now()));
                        
                        // 如果是会话首次分配且需要粘性，在此建立绑定
                        if let Some(sid) = session_id {
                            if scheduling.mode != SchedulingMode::PerformanceFirst {
                                self.session_accounts.insert(sid.to_string(), (candidate.account_id.clone(), std::time::Instant::now()));
                                tracing::debug!("Sticky Session: Bound new account {} to session {}", candidate.email, sid);
                            }
                        }
                        break;
                    }
                }
            } else if target_token.is_none() {
                // 模式 C: 纯轮询模式 (Round-robin) 或强制轮换
                let start_idx = self.current_index.fetch_add(1, Ordering::SeqCst) % total;
                tracing::debug!("🔄 [Mode C] Round-robin from idx {}, total: {}", start_idx, total);
                for offset in 0..total {
                    let idx = (start_idx + offset) % total;
                    let candidate = &tokens_snapshot[idx];
                    
                    if attempted.contains(&candidate.account_id) {
                        tracing::debug!("  [{}] {} - SKIP: already attempted", idx, candidate.email);
                        continue;
                    }

                    // 【新增 #621】模型级限流检查
                    if quota_protection_enabled && candidate.protected_models.contains(&normalized_target) {
                        tracing::debug!("  ⛔ {} - SKIP: quota-protected for {} [{}]", candidate.email, normalized_target, target_model);
                        continue;
                    }

                    // 【新增】主动避开限流或 5xx 锁定的账号
                    if self.is_rate_limited(&candidate.account_id, Some(&normalized_target)).await { // Changed to account_id
                        tracing::debug!("  ⏳ {} - SKIP: rate-limited", candidate.email);
                        continue;
                    }

                    tracing::debug!("  [{}] {} - SELECTED", idx, candidate.email);
                    target_token = Some(candidate.clone());
                    
                    if rotate {
                        tracing::debug!("Force Rotation: Switched to account: {}", candidate.email);
                    }
                    break;
                }
            }
            
            let mut token = match target_token {
                Some(t) => t,
                None => {
                    let mut wait_ms = 0;
                    // 乐观重置策略: 双层防护机制
                    // 计算最短等待时间
                    let min_wait = tokens_snapshot.iter()
                        .filter_map(|t| self.rate_limit_tracker.get_reset_seconds(&t.account_id))
                        .min();
                    
                    // Layer 1: 如果最短等待时间 <= 2秒,执行缓冲延迟
                    if let Some(wait_sec) = min_wait {
                        if wait_sec <= 2 {
                            wait_ms = (wait_sec as f64 * 1000.0) as u64;
                            tracing::warn!(
                                "All accounts rate-limited but shortest wait is {}s. Applying {}ms buffer for state sync...",
                                wait_sec, wait_ms
                            );
                            
                            // 缓冲延迟
                            tokio::time::sleep(tokio::time::Duration::from_millis(wait_ms)).await;
                            
                            // 重新尝试选择账号
                            let retry_token = tokens_snapshot.iter()
                                .find(|t| !attempted.contains(&t.account_id) && !self.is_rate_limited_sync(&t.account_id, None));
                            
                            if let Some(t) = retry_token {
                                tracing::info!("✅ Buffer delay successful! Found available account: {}", t.email);
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
                                let final_token = tokens_snapshot.iter()
                                    .find(|t| !attempted.contains(&t.account_id));
                                
                                if let Some(t) = final_token {
                                    tracing::info!("✅ Optimistic reset successful! Using account: {}", t.email);
                                    t.clone()
                                } else {
                                    return Err("All accounts failed after optimistic reset.".to_string());
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

        
            // 3. 检查 token 是否过期（提前5分钟刷新）
            let now = chrono::Utc::now().timestamp();
            if now >= token.timestamp - 300 {
                tracing::debug!("账号 {} 的 token 即将过期，正在刷新...", token.email);

                // 调用 OAuth 刷新 token
                match crate::modules::oauth::refresh_access_token(&token.refresh_token).await {
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
                        if let Err(e) = self.save_refreshed_token(&token.account_id, &token_response).await {
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
                                .disable_account(&token.account_id, &format!("invalid_grant: {}", e))
                                .await;
                            self.tokens.remove(&token.account_id);
                        }
                        // Avoid leaking account emails to API clients; details are still in logs.
                        last_error = Some(format!("Token refresh failed: {}", e));
                        attempted.insert(token.account_id.clone());

                        // 【优化】标记需要清除锁定，避免在循环内加锁
                        if quota_group != "image_gen" {
                            if matches!(&last_used_account_id, Some((id, _)) if id == &token.account_id) {
                                need_update_last_used = Some((String::new(), std::time::Instant::now())); // 空字符串表示需要清除
                            }
                        }
                        continue;
                    }
                }
            }

            // 4. 确保有 project_id
            let project_id = if let Some(pid) = &token.project_id {
                pid.clone()
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
                        tracing::error!("Failed to fetch project_id for {}: {}", token.email, e);
                        last_error = Some(format!("Failed to fetch project_id for {}: {}", token.email, e));
                        attempted.insert(token.account_id.clone());

                        // 【优化】标记需要清除锁定，避免在循环内加锁
                        if quota_group != "image_gen" {
                            if matches!(&last_used_account_id, Some((id, _)) if id == &token.account_id) {
                                need_update_last_used = Some((String::new(), std::time::Instant::now())); // 空字符串表示需要清除
                            }
                        }
                        continue;
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

            // [NEW] Increment active requests
            self.active_requests.entry(token.account_id.clone())
                .or_insert(AtomicUsize::new(0))
                .fetch_add(1, Ordering::SeqCst);
            
            let active_count = self.active_requests.get(&token.account_id).unwrap().load(Ordering::SeqCst);
            tracing::debug!("⬆️ Connection acquired: {} (active: {})", token.email, active_count);

            return Ok(TokenLease {
                access_token: token.access_token,
                project_id,
                email: token.email,
                account_id: token.account_id.clone(),
                active_requests: self.active_requests.clone(),
            });
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

        let json_str = serde_json::to_string_pretty(&content)
            .map_err(|e| format!("序列化 JSON 失败: {}", e))?;

        std::fs::write(&path, json_str)
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
        
        let json_str = serde_json::to_string_pretty(&content)
            .map_err(|e| format!("序列化 JSON 失败: {}", e))?;

        std::fs::write(path, json_str)
            .map_err(|e| format!("写入文件失败: {}", e))?;
        
        tracing::debug!("已保存 project_id 到账号 {}", account_id);
        Ok(())
    }

    /// [FEEDBACK-LOOP] 报告 429 错误，大幅降低健康分
    pub fn report_429_penalty(&self, account_id: &str) {
        if let Some(mut score) = self.health_scores.get_mut(account_id) {
            let old_score = *score;
            // 惩罚机制: 每次 429，分数减半，至少降到 0.1
            *score = (*score * 0.5).max(0.01); 
            tracing::warn!("⚠️ Account {} hit 429! Health penalty: {:.2} -> {:.2}", account_id, old_score, *score);
        }
        
        // 可选：在这里也可以触发一个临时的 "Cool Down"，但目前 Health Score 降低已经足够让它掉到底部
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
        
        let json_str = serde_json::to_string_pretty(&content)
            .map_err(|e| format!("序列化 JSON 失败: {}", e))?;

        std::fs::write(path, json_str)
            .map_err(|e| format!("写入文件失败: {}", e))?;
        
        tracing::debug!("已保存刷新后的 token 到账号 {}", account_id);
        Ok(())
    }
    
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// 通过 email 获取指定账号的 Token（用于预热等需要指定账号的场景）
    /// 此方法会自动刷新过期的 token
    pub async fn get_token_by_email(&self, email: &str) -> Result<(String, String, String, u64), String> {
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

        let project_id = project_id_opt.unwrap_or_else(|| "bamboo-precept-lgxtn".to_string());
        
        // 检查是否过期 (提前5分钟)
        if now < timestamp + expires_in - 300 {
            return Ok((current_access_token, project_id, email.to_string(), 0));
        }

        tracing::info!("[Warmup] Token for {} is expiring, refreshing...", email);

        // 调用 OAuth 刷新 token
        match crate::modules::oauth::refresh_access_token(&refresh_token).await {
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
                let _ = self.save_refreshed_token(&account_id, &token_response).await;

                Ok((token_response.access_token, project_id, email.to_string(), 0))
            }
            Err(e) => Err(format!("[Warmup] Token refresh failed for {}: {}", email, e)),
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



    /// [FIX] 获取当前有效的账号数量 (考虑调度模式)
    /// 如果是 Selected 模式，只返回选中账号的数量
    pub async fn effective_len(&self) -> usize {
        let config = self.sticky_config.read().await;
        if matches!(config.mode, crate::proxy::sticky_config::SchedulingMode::Selected) {
            config.selected_accounts.len()
        } else {
            self.tokens.len()
        }
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
        self.tokens.iter()
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
    /// 在请求成功完成后调用，归零失败计数并清除关联锁。
    pub fn mark_account_success(&self, email: &str, model: Option<&str>) {
        if let Some(account_id) = self.email_to_account_id(email) {
            self.rate_limit_tracker.mark_success(&account_id, model);
        } else {
             // Fallback if email not found (rare)
             self.rate_limit_tracker.mark_success(email, model);
        }
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
    pub fn get_quota_reset_time(&self, email: &str) -> Option<String> {
        // 尝试从账号文件读取配额信息
        let accounts_dir = self.data_dir.join("accounts");
        
        // 遍历账号文件查找对应的 email
        if let Ok(entries) = std::fs::read_dir(&accounts_dir) {
            for entry in entries.flatten() {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if let Ok(account) = serde_json::from_str::<serde_json::Value>(&content) {
                        // 检查 email 是否匹配
                        if account.get("email").and_then(|e| e.as_str()) == Some(email) {
                            // 获取 quota.models 中最早的 reset_time
                            if let Some(models) = account
                                .get("quota")
                                .and_then(|q| q.get("models"))
                                .and_then(|m| m.as_array()) 
                            {
                                // 找到最早的 reset_time（最保守的锁定策略）
                                let mut earliest_reset: Option<&str> = None;
                                for model in models {
                                    if let Some(reset_time) = model.get("reset_time").and_then(|r| r.as_str()) {
                                        if !reset_time.is_empty() {
                                            match earliest_reset {
                                                Some(current_min) => {
                                                    if reset_time < current_min {
                                                        earliest_reset = Some(reset_time);
                                                    }
                                                }
                                                None => {
                                                    earliest_reset = Some(reset_time);
                                                }
                                            }
                                        }
                                    }
                                }
                                if let Some(reset) = earliest_reset {
                                    return Some(reset.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }
    
    /// 使用配额刷新时间精确锁定账号
    /// 
    /// 当 API 返回 429 但没有 quotaResetDelay 时,尝试使用账号的配额刷新时间
    /// 
    /// # 参数
    /// - `model`: 可选的模型名称,用于模型级别限流
    pub fn set_precise_lockout(&self, email: &str, reason: crate::proxy::rate_limit::RateLimitReason, model: Option<String>) -> bool {
        if let Some(reset_time_str) = self.get_quota_reset_time(email) {
            tracing::info!("找到账号 {} 的配额刷新时间: {}", email, reset_time_str);
            self.rate_limit_tracker.set_lockout_until_iso(email, &reset_time_str, reason, model)
        } else {
            tracing::debug!("未找到账号 {} 的配额刷新时间,将使用默认退避策略", email);
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
        // 1. 从 tokens 中获取该账号的 access_token
        let access_token = {
            let mut found_token: Option<String> = None;
            for entry in self.tokens.iter() {
                if entry.value().email == email {
                    found_token = Some(entry.value().access_token.clone());
                    break;
                }
            }
            found_token
        };
        
        let access_token = match access_token {
            Some(t) => t,
            None => {
                tracing::warn!("无法找到账号 {} 的 access_token,无法实时刷新配额", email);
                return false;
            }
        };
        
        // 2. 调用配额刷新 API
        tracing::info!("账号 {} 正在实时刷新配额...", email);
        match crate::modules::quota::fetch_quota(&access_token, email).await {
            Ok((quota_data, _project_id)) => {
                // 3. 从最新配额中提取 reset_time
                let earliest_reset = quota_data.models.iter()
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
                        email, reset_time_str
                    );
                    self.rate_limit_tracker.set_lockout_until_iso(email, reset_time_str, reason, model)
                } else {
                    tracing::warn!("账号 {} 配额刷新成功但未找到 reset_time", email);
                    false
                }
            },
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
    /// - `model`: 可选的模型名称,用于模型级别限流。传入实际使用的模型可以避免不同模型配额互相影响
    pub async fn mark_rate_limited_async(
        &self,
        email: &str,
        status: u16,
        retry_after_header: Option<&str>,
        error_body: &str,
        model: Option<&str>,  // 🆕 新增模型参数
    ) {
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
                tracing::debug!("账号 {} 的模型 {} 的 429 响应包含 quotaResetDelay,直接使用 API 返回的时间", account_id, m);
            } else {
                tracing::debug!("账号 {} 的 429 响应包含 quotaResetDelay,直接使用 API 返回的时间", account_id);
            }
            self.rate_limit_tracker.parse_from_error(
                &account_id,
                status,
                retry_after_header,
                error_body,
                model.map(|s| s.to_string()),
                &config.backoff_steps, // [NEW] 传入配置
            );
            return;
        }
        
        // 确定限流原因
        let reason = if error_body.to_lowercase().contains("model_capacity") {
            crate::proxy::rate_limit::RateLimitReason::ModelCapacityExhausted
        } else if error_body.to_lowercase().contains("exhausted") || error_body.to_lowercase().contains("quota") {
            crate::proxy::rate_limit::RateLimitReason::QuotaExhausted
        } else {
            crate::proxy::rate_limit::RateLimitReason::Unknown
        };
        
        // API 未返回 quotaResetDelay,需要实时刷新配额获取精确锁定时间
        if let Some(m) = model {
            tracing::info!("账号 {} 的模型 {} 的 429 响应未包含 quotaResetDelay,尝试实时刷新配额...", account_id, m);
        } else {
            tracing::info!("账号 {} 的 429 响应未包含 quotaResetDelay,尝试实时刷新配额...", account_id);
        }
        
        if self.fetch_and_lock_with_realtime_quota(&account_id, reason, model.map(|s| s.to_string())).await {
            tracing::info!("账号 {} 已使用实时配额精确锁定", account_id);
            return;
        }
        
        // 实时刷新失败,尝试使用本地缓存的配额刷新时间
        if self.set_precise_lockout(&account_id, reason, model.map(|s| s.to_string())) {
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
            model.map(|s| s.to_string()),
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
        crate::modules::oauth::exchange_code(code, redirect_uri).await
            .and_then(|t| t.refresh_token.ok_or_else(|| "No refresh token returned by Google".to_string()))
    }

    /// 获取 OAuth URL (支持自定义 Redirect URI)
    pub fn get_oauth_url_with_redirect(&self, redirect_uri: &str, state: &str) -> String {
        crate::modules::oauth::get_auth_url(redirect_uri, state)
    }

    /// 获取用户信息 (Email 等)
    pub async fn get_user_info(&self, refresh_token: &str) -> Result<crate::modules::oauth::UserInfo, String> {
        // 先获取 Access Token
        let token = crate::modules::oauth::refresh_access_token(refresh_token).await
            .map_err(|e| format!("刷新 Access Token 失败: {}", e))?;
            
        crate::modules::oauth::get_user_info(&token.access_token).await
    }

    /// 添加新账号 (纯后端实现，不依赖 Tauri AppHandle)
    pub async fn add_account(&self, email: &str, refresh_token: &str) -> Result<(), String> {
        // 1. 获取 Access Token (验证 refresh_token 有效性)
        let token_info = crate::modules::oauth::refresh_access_token(refresh_token)
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
            );
            
            crate::modules::account::upsert_account(email_clone, None, token_data)
        }).await
        .map_err(|e| format!("Task join error: {}", e))?
        .map_err(|e| format!("Failed to save account: {}", e))?;

        // 4. 重新加载 (更新内存)
        self.reload_all_accounts().await.map(|_| ())
    }
    
/// 记录请求成功，增加健康分
    pub fn record_success(&self, account_id: &str) {
        self.health_scores.entry(account_id.to_string())
            .and_modify(|s| *s = (*s + 0.05).min(1.0))
            .or_insert(1.0);
        tracing::debug!("📈 Health score increased for account {}", account_id);
    }

    /// 记录请求失败，降低健康分
    pub fn record_failure(&self, account_id: &str) {
        self.health_scores.entry(account_id.to_string())
            .and_modify(|s| *s = (*s - 0.2).max(0.0))
            .or_insert(0.8);
        tracing::warn!("📉 Health score decreased for account {}", account_id);
    }

}

/// 截断过长的原因字符串
fn truncate_reason(reason: &str, max_len: usize) -> String {
    if reason.len() <= max_len {
        reason.to_string()
    } else {
        format!("{}...", &reason[..max_len - 3])
    }
}
