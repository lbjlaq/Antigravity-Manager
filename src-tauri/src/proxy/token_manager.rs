// 移除冗余的顶层导入，因为这些在代码中已由 full path 或局部导入处理
use dashmap::DashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::proxy::rate_limit::RateLimitTracker;
use crate::proxy::sticky_config::StickySessionConfig;

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
}

pub struct TokenManager {
    tokens: Arc<DashMap<String, ProxyToken>>,  // account_id -> ProxyToken
    current_index: Arc<AtomicUsize>,
    last_used_account: Arc<tokio::sync::Mutex<Option<(String, std::time::Instant)>>>,
    data_dir: PathBuf,
    rate_limit_tracker: Arc<RateLimitTracker>,  // 新增: 限流跟踪器
    sticky_config: Arc<tokio::sync::RwLock<StickySessionConfig>>, // 新增：调度配置
    session_accounts: Arc<DashMap<String, String>>, // 新增：会话与账号映射 (SessionID -> AccountID)
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
        }
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
    
    /// 加载单个账号
    async fn load_single_account(&self, path: &PathBuf) -> Result<Option<ProxyToken>, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("读取文件失败: {}", e))?;
        
        let account: serde_json::Value = serde_json::from_str(&content)
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
        }))
    }
    
    /// 获取当前可用的 Token（支持粘性会话与智能调度）
    /// 参数 `quota_group` 用于区分 "claude" vs "gemini" 组
    /// 参数 `force_rotate` 为 true 时将忽略锁定，强制切换账号
    /// 参数 `session_id` 用于跨请求维持会话粘性
    pub async fn get_token(&self, quota_group: &str, force_rotate: bool, session_id: Option<&str>) -> Result<(String, String, String), String> {
        // 【优化 Issue #284】添加 5 秒超时，防止死锁
        let timeout_duration = std::time::Duration::from_secs(5);
        match tokio::time::timeout(timeout_duration, self.get_token_internal(quota_group, force_rotate, session_id)).await {
            Ok(result) => result,
            Err(_) => Err("Token acquisition timeout (5s) - system too busy or deadlock detected".to_string()),
        }
    }

    /// 内部实现：获取 Token 的核心逻辑
    async fn get_token_internal(&self, quota_group: &str, force_rotate: bool, session_id: Option<&str>) -> Result<(String, String, String), String> {
        let mut tokens_snapshot: Vec<ProxyToken> = self.tokens.iter().map(|e| e.value().clone()).collect();
        let total = tokens_snapshot.len();
        if total == 0 {
            return Err("Token pool is empty".to_string());
        }

        // ===== 【优化】根据订阅等级排序 (优先级: ULTRA > PRO > FREE) =====
        // 理由: ULTRA/PRO 重置快，优先消耗；FREE 重置慢，用于兜底
        tokens_snapshot.sort_by(|a, b| {
            let tier_priority = |tier: &Option<String>| match tier.as_deref() {
                Some("ULTRA") => 0,
                Some("PRO") => 1,
                Some("FREE") => 2,
                _ => 3,
            };
            tier_priority(&a.subscription_tier).cmp(&tier_priority(&b.subscription_tier))
        });

        // 0. 读取当前调度配置
        let scheduling = self.sticky_config.read().await.clone();
        use crate::proxy::sticky_config::SchedulingMode;

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
            
            // 模式 A: 粘性会话处理 (CacheFirst 或 Balance 且有 session_id)
            if !rotate && session_id.is_some() && scheduling.mode != SchedulingMode::PerformanceFirst {
                let sid = session_id.unwrap();
                
                // 1. 检查会话是否已绑定账号
                if let Some(bound_id) = self.session_accounts.get(sid).map(|v| v.clone()) {
                    // 2. 检查绑定的账号是否限流 (使用精准的剩余时间接口)
                    let reset_sec = self.rate_limit_tracker.get_remaining_wait(&bound_id);
                    if reset_sec > 0 {
                        // 【修复 Issue #284】立即解绑并切换账号，不再阻塞等待
                        // 原因：阻塞等待会导致并发请求时客户端 socket 超时 (UND_ERR_SOCKET)
                        tracing::warn!("Session {} bound account {} is rate-limited ({}s remaining). Unbinding and switching to next available account.", sid, bound_id, reset_sec);
                        self.session_accounts.remove(sid);
                    } else if !attempted.contains(&bound_id) {
                        // 3. 账号可用且未被标记为尝试失败，优先复用
                        if let Some(found) = tokens_snapshot.iter().find(|t| t.account_id == bound_id) {
                            tracing::debug!("Sticky Session: Successfully reusing bound account {} for session {}", found.email, sid);
                            target_token = Some(found.clone());
                        }
                    }
                }
            }

            // 模式 B: 原子化 60s 全局锁定 (针对无 session_id 情况的默认保护)
            if target_token.is_none() && !rotate && quota_group != "image_gen" {
                // 【优化】使用预先获取的快照，不再在循环内加锁
                if let Some((account_id, last_time)) = &last_used_account_id {
                    if last_time.elapsed().as_secs() < 60 && !attempted.contains(account_id) {
                        if let Some(found) = tokens_snapshot.iter().find(|t| &t.account_id == account_id) {
                            tracing::debug!("60s Window: Force reusing last account: {}", found.email);
                            target_token = Some(found.clone());
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

                        // 【新增】主动避开限流或 5xx 锁定的账号 (来自 PR #28 的高可用思路)
                        if self.is_rate_limited(&candidate.account_id) {
                            continue;
                        }

                        target_token = Some(candidate.clone());
                        // 【优化】标记需要更新，稍后统一写回
                        need_update_last_used = Some((candidate.account_id.clone(), std::time::Instant::now()));
                        
                        // 如果是会话首次分配且需要粘性，在此建立绑定
                        if let Some(sid) = session_id {
                            if scheduling.mode != SchedulingMode::PerformanceFirst {
                                self.session_accounts.insert(sid.to_string(), candidate.account_id.clone());
                                tracing::debug!("Sticky Session: Bound new account {} to session {}", candidate.email, sid);
                            }
                        }
                        break;
                    }
                }
            } else if target_token.is_none() {
                // 模式 C: 纯轮询模式 (Round-robin) 或强制轮换
                let start_idx = self.current_index.fetch_add(1, Ordering::SeqCst) % total;
                for offset in 0..total {
                    let idx = (start_idx + offset) % total;
                    let candidate = &tokens_snapshot[idx];
                    if attempted.contains(&candidate.account_id) {
                        continue;
                    }

                    // 【新增】主动避开限流或 5xx 锁定的账号
                    if self.is_rate_limited(&candidate.account_id) {
                        continue;
                    }

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
                    // 如果所有账号都被尝试过或都处于限流中，计算最短等待时间
                    let min_wait = tokens_snapshot.iter()
                        .filter_map(|t| self.rate_limit_tracker.get_reset_seconds(&t.account_id))
                        .min()
                        .unwrap_or(60);
                    
                    return Err(format!("All accounts are currently limited or unhealthy. Please wait {}s.", min_wait));
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

            return Ok((token.access_token, project_id, token.email));
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
    
    // ===== 限流管理方法 =====
    
    /// 标记账号限流(从外部调用,通常在 handler 中)
    pub fn mark_rate_limited(
        &self,
        account_id: &str,
        status: u16,
        retry_after_header: Option<&str>,
        error_body: &str,
    ) {
        self.rate_limit_tracker.parse_from_error(
            account_id,
            status,
            retry_after_header,
            error_body,
        );
    }
    
    /// 检查账号是否在限流中
    pub fn is_rate_limited(&self, account_id: &str) -> bool {
        self.rate_limit_tracker.is_rate_limited(account_id)
    }
    
    /// 获取距离限流重置还有多少秒
    #[allow(dead_code)]
    pub fn get_rate_limit_reset_seconds(&self, account_id: &str) -> Option<u64> {
        self.rate_limit_tracker.get_reset_seconds(account_id)
    }
    
    /// 清除过期的限流记录
    #[allow(dead_code)]
    pub fn cleanup_expired_rate_limits(&self) -> usize {
        self.rate_limit_tracker.cleanup_expired()
    }
    
    /// 清除指定账号的限流记录
    #[allow(dead_code)]
    pub fn clear_rate_limit(&self, account_id: &str) -> bool {
        self.rate_limit_tracker.clear(account_id)
    }
    
    /// 标记账号请求成功，重置连续失败计数
    /// 
    /// 在请求成功完成后调用，将该账号的失败计数归零，
    /// 下次失败时从最短的锁定时间开始（智能限流）。
    pub fn mark_account_success(&self, account_id: &str) {
        self.rate_limit_tracker.mark_success(account_id);
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
                                            if earliest_reset.is_none() || reset_time < earliest_reset.unwrap() {
                                                earliest_reset = Some(reset_time);
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
    /// 当 API 返回 429 但没有 quotaResetDelay 时，尝试使用账号的配额刷新时间
    pub fn set_precise_lockout(&self, email: &str, reason: crate::proxy::rate_limit::RateLimitReason) -> bool {
        if let Some(reset_time_str) = self.get_quota_reset_time(email) {
            tracing::info!("找到账号 {} 的配额刷新时间: {}", email, reset_time_str);
            self.rate_limit_tracker.set_lockout_until_iso(email, &reset_time_str, reason)
        } else {
            tracing::debug!("未找到账号 {} 的配额刷新时间，将使用默认退避策略", email);
            false
        }
    }
    
    /// 实时刷新配额并精确锁定账号
    /// 
    /// 当 429 发生时调用此方法：
    /// 1. 实时调用配额刷新 API 获取最新的 reset_time
    /// 2. 使用最新的 reset_time 精确锁定账号
    /// 3. 如果获取失败，返回 false 让调用方使用回退策略
    pub async fn fetch_and_lock_with_realtime_quota(
        &self,
        email: &str,
        reason: crate::proxy::rate_limit::RateLimitReason,
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
                tracing::warn!("无法找到账号 {} 的 access_token，无法实时刷新配额", email);
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
                        "账号 {} 实时配额刷新成功，reset_time: {}",
                        email, reset_time_str
                    );
                    self.rate_limit_tracker.set_lockout_until_iso(email, reset_time_str, reason)
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
    
    /// 标记账号限流（异步版本，支持实时配额刷新）
    /// 
    /// 三级降级策略：
    /// 1. 优先：API 返回 quotaResetDelay → 直接使用
    /// 2. 次优：实时刷新配额 → 获取最新 reset_time
    /// 3. 保底：使用本地缓存配额 → 读取账号文件
    /// 4. 兜底：指数退避策略 → 默认锁定时间
    pub async fn mark_rate_limited_async(
        &self,
        account_id: &str,
        status: u16,
        retry_after_header: Option<&str>,
        error_body: &str,
    ) {
        // 检查 API 是否返回了精确的重试时间
        let has_explicit_retry_time = retry_after_header.is_some() || 
            error_body.contains("quotaResetDelay");
        
        if has_explicit_retry_time {
            // API 返回了精确时间（quotaResetDelay），直接使用，无需实时刷新
            tracing::debug!("账号 {} 的 429 响应包含 quotaResetDelay，直接使用 API 返回的时间", account_id);
            self.rate_limit_tracker.parse_from_error(
                account_id,
                status,
                retry_after_header,
                error_body,
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
        
        // API 未返回 quotaResetDelay，需要实时刷新配额获取精确锁定时间
        tracing::info!("账号 {} 的 429 响应未包含 quotaResetDelay，尝试实时刷新配额...", account_id);
        if self.fetch_and_lock_with_realtime_quota(account_id, reason).await {
            tracing::info!("账号 {} 已使用实时配额精确锁定", account_id);
            return;
        }
        
        // 实时刷新失败，尝试使用本地缓存的配额刷新时间
        if self.set_precise_lockout(account_id, reason) {
            tracing::info!("账号 {} 已使用本地缓存配额锁定", account_id);
            return;
        }
        
        // 都失败了，回退到指数退避策略
        tracing::warn!("账号 {} 无法获取配额刷新时间，使用指数退避策略", account_id);
        self.rate_limit_tracker.parse_from_error(
            account_id,
            status,
            retry_after_header,
            error_body,
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

    /// 清除特定会话的粘性映射
    #[allow(dead_code)]
    pub fn clear_session_binding(&self, session_id: &str) {
        self.session_accounts.remove(session_id);
    }

    /// 清除所有会话的粘性映射
    pub fn clear_all_sessions(&self) {
        self.session_accounts.clear();
    }
}

fn truncate_reason(reason: &str, max_len: usize) -> String {
    if reason.chars().count() <= max_len {
        return reason.to_string();
    }
    let mut s: String = reason.chars().take(max_len).collect();
    s.push('…');
    s
}
