// 移除冗余的顶层导入，因为这些在代码中已由 full path 或局部导入处理
use dashmap::DashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

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
    session_pins: Arc<DashMap<String, (String, std::time::Instant)>>, // key -> (account_id, last_seen)
    cooldowns: Arc<DashMap<String, std::time::Instant>>,              // "<quota_group>:<account_id>" -> available_at
    data_dir: PathBuf,
}

impl TokenManager {
    const SESSION_PIN_TTL_SECS: u64 = 30 * 60; // 30 minutes
    const MAX_RATE_LIMIT_WAIT_MS: u64 = 120_000; // 2 minutes

    /// 创建新的 TokenManager
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            tokens: Arc::new(DashMap::new()),
            current_index: Arc::new(AtomicUsize::new(0)),
            last_used_account: Arc::new(tokio::sync::Mutex::new(None)),
            session_pins: Arc::new(DashMap::new()),
            cooldowns: Arc::new(DashMap::new()),
            data_dir,
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
        self.session_pins.clear();
        self.cooldowns.clear();
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
                    tracing::warn!("加载账号失败 {:?}: {}", path, e);
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
            tracing::warn!(
                "Skipping disabled account file: {:?} (email={})",
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
    
    /// 获取当前可用的 Token（带 60s 时间窗口锁定机制）
    /// 参数 `_quota_group` 用于区分 "claude" vs "gemini" 组
    /// 参数 `force_rotate` 为 true 时将忽略锁定，强制切换账号
    pub async fn get_token(
        &self,
        quota_group: &str,
        force_rotate: bool,
        session_key: Option<&str>,
    ) -> Result<(String, String, String), String> {
        let mut tokens_snapshot: Vec<ProxyToken> = self.tokens.iter().map(|e| e.value().clone()).collect();
        let total = tokens_snapshot.len();
        if total == 0 {
            return Err("Token pool is empty".to_string());
        }

        self.cleanup_expired_cooldowns();

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

        let mut attempted: HashSet<String> = HashSet::new();
        let mut last_error: Option<String> = None;

        let session_pin_key = session_key
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| format!("{}:{}", quota_group, s));

        for attempt in 0..total {
            let rotate = force_rotate || attempt > 0;

            // ===== 【优化】原子化锁定检查与选择 =====
            let mut target_token: Option<ProxyToken> = None;
            
            // Prefer per-session pinning when a session key is available (keeps conversations on the same account).
            // Otherwise fall back to the legacy global 60s "last_used" lock.
            if quota_group != "image_gen" {
                if let Some(pin_key) = session_pin_key.as_deref() {
                    if !rotate {
                        if let Some((pinned_account_id, last_seen)) =
                            self.session_pins.get(pin_key).map(|p| p.value().clone())
                        {
                            if last_seen.elapsed().as_secs() <= Self::SESSION_PIN_TTL_SECS
                                && !attempted.contains(&pinned_account_id)
                                && !self.is_in_cooldown(quota_group, &pinned_account_id)
                            {
                                if let Some(found) = tokens_snapshot
                                    .iter()
                                    .find(|t| t.account_id == pinned_account_id)
                                {
                                    tracing::info!(
                                        "Session pin hit ({}), reusing account: {}",
                                        pin_key,
                                        found.email
                                    );
                                    target_token = Some(found.clone());
                                    // Touch pin
                                    self.session_pins.insert(
                                        pin_key.to_string(),
                                        (pinned_account_id, std::time::Instant::now()),
                                    );
                                } else {
                                    // Account no longer exists; drop pin.
                                    self.session_pins.remove(pin_key);
                                }
                            } else if last_seen.elapsed().as_secs() > Self::SESSION_PIN_TTL_SECS {
                                self.session_pins.remove(pin_key);
                            }
                        }
                    }

                    if target_token.is_none() {
                        // Session-aware selection (round-robin, no global lock).
                        let start_idx = self.current_index.fetch_add(1, Ordering::SeqCst) % total;
                        for offset in 0..total {
                            let idx = (start_idx + offset) % total;
                            let candidate = &tokens_snapshot[idx];
                            if attempted.contains(&candidate.account_id) {
                                continue;
                            }
                            if self.is_in_cooldown(quota_group, &candidate.account_id) {
                                continue;
                            }
                            target_token = Some(candidate.clone());
                            // Always (re)bind the session to the chosen account for cache continuity.
                            self.session_pins.insert(
                                pin_key.to_string(),
                                (candidate.account_id.clone(), std::time::Instant::now()),
                            );
                            if rotate {
                                tracing::info!(
                                    "Session pin rotated ({}), switching to account: {}",
                                    pin_key,
                                    candidate.email
                                );
                            } else {
                                tracing::info!(
                                    "Session pin created ({}), using account: {}",
                                    pin_key,
                                    candidate.email
                                );
                            }
                            break;
                        }
                    }
                } else if !rotate {
                // 在锁内一站式完成：1. 检查锁定 2. 选择新账号 3. 更新锁定
                let mut last_used = self.last_used_account.lock().await;
                
                // A. 尝试复用锁定账号
                if let Some((account_id, last_time)) = &*last_used {
                    if last_time.elapsed().as_secs() < 60 && !attempted.contains(account_id) {
                        if self.is_in_cooldown(quota_group, account_id) {
                            // Don't reuse a globally locked account if it is rate-limited.
                        } else if let Some(found) =
                            tokens_snapshot.iter().find(|t| &t.account_id == account_id)
                        {
                            tracing::info!("60s 时间窗口内，强制复用上一个账号: {}", found.email);
                            target_token = Some(found.clone());
                        }
                    }
                }
                
                // B. 若无锁定，则轮询选择新账号并立即建立锁定
                if target_token.is_none() {
                    let start_idx = self.current_index.fetch_add(1, Ordering::SeqCst) % total;
                    for offset in 0..total {
                        let idx = (start_idx + offset) % total;
                        let candidate = &tokens_snapshot[idx];
                        if attempted.contains(&candidate.account_id) {
                            continue;
                        }
                        if self.is_in_cooldown(quota_group, &candidate.account_id) {
                            continue;
                        }
                        target_token = Some(candidate.clone());
                        // 【关键】在锁内立即更新，确保后续并发请求能看到
                        *last_used = Some((candidate.account_id.clone(), std::time::Instant::now()));
                        tracing::info!("切换到新账号并建立 60s 锁定: {}", candidate.email);
                        break;
                    }
                }
                // 锁在此处自动释放
                }
            } else {
                // 画图请求或强制轮换，不使用 session 锁定
                let start_idx = self.current_index.fetch_add(1, Ordering::SeqCst) % total;
                for offset in 0..total {
                    let idx = (start_idx + offset) % total;
                    let candidate = &tokens_snapshot[idx];
                    if attempted.contains(&candidate.account_id) {
                        continue;
                    }
                    if self.is_in_cooldown(quota_group, &candidate.account_id) {
                        continue;
                    }
                    target_token = Some(candidate.clone());
                    
                    if rotate {
                        tracing::info!("强制切换到账号: {}", candidate.email);
                    }
                    break;
                }
            }
            
            let mut token = if let Some(t) = target_token {
                t
            } else if let Some(wait_ms) =
                self.min_cooldown_wait_ms(quota_group, &tokens_snapshot, &attempted)
            {
                if wait_ms > Self::MAX_RATE_LIMIT_WAIT_MS {
                    return Err(format!(
                        "RESOURCE_EXHAUSTED: All accounts rate-limited. Min wait {}ms exceeds {}ms.",
                        wait_ms,
                        Self::MAX_RATE_LIMIT_WAIT_MS
                    ));
                }

                tracing::warn!(
                    "All accounts rate-limited; waiting {}ms then retrying selection",
                    wait_ms
                );
                tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                self.cleanup_expired_cooldowns();
                attempted.clear();
                continue;
            } else {
                return Err(last_error.clone().unwrap_or_else(|| "All accounts exhausted".to_string()));
            };

        
            // 3. 检查 token 是否过期（提前5分钟刷新）
            let now = chrono::Utc::now().timestamp();
            if now >= token.timestamp - 300 {
                tracing::info!("账号 {} 的 token 即将过期，正在刷新...", token.email);

                // 调用 OAuth 刷新 token
                match crate::modules::oauth::refresh_access_token(&token.refresh_token).await {
                    Ok(token_response) => {
                        tracing::info!("Token 刷新成功！");

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
                            tracing::warn!("保存刷新后的 token 失败 ({}): {}", token.email, e);
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

                        // 如果当前账号被锁定复用，刷新失败后必须解除锁定，避免下一次仍选中同一账号
                        if quota_group != "image_gen" {
                            let mut last_used = self.last_used_account.lock().await;
                            if matches!(&*last_used, Some((id, _)) if id == &token.account_id) {
                                *last_used = None;
                            }
                        }
                        continue;
                    }
                }
            }

            // 4. 确保有 project_id
            let project_id = if let Some(pid) = &token.project_id {
                if Self::is_legacy_mock_project_id(pid) {
                    tracing::warn!(
                        "账号 {} 存在历史 mock project_id '{}'，将重新获取/回退默认 project_id",
                        token.email,
                        pid
                    );
                    // Force refetch path below.
                    if let Some(mut entry) = self.tokens.get_mut(&token.account_id) {
                        entry.project_id = None;
                    }
                    String::new()
                } else {
                    pid.clone()
                }
            } else {
                String::new()
            };

            let project_id = if !project_id.is_empty() {
                project_id
            } else {
                tracing::info!("账号 {} 缺少 project_id，尝试获取...", token.email);
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
                        last_error = Some(format!(
                            "Failed to fetch project_id for {}: {}",
                            token.email, e
                        ));
                        attempted.insert(token.account_id.clone());

                        if quota_group != "image_gen" {
                            let mut last_used = self.last_used_account.lock().await;
                            if matches!(&*last_used, Some((id, _)) if id == &token.account_id) {
                                *last_used = None;
                            }
                        }
                        continue;
                    }
                }
            };

            return Ok((token.access_token, project_id, token.email));
        }

        Err(last_error.unwrap_or_else(|| "All accounts failed".to_string()))
    }

    fn clear_session_pins_for_account(&self, account_id: &str) {
        let keys: Vec<String> = self
            .session_pins
            .iter()
            .filter_map(|entry| {
                let (pinned_account_id, _) = entry.value();
                if pinned_account_id == account_id {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();

        for k in keys {
            self.session_pins.remove(&k);
        }
    }

    fn cleanup_expired_cooldowns(&self) {
        let now = std::time::Instant::now();
        let expired: Vec<String> = self
            .cooldowns
            .iter()
            .filter_map(|e| {
                if *e.value() <= now {
                    Some(e.key().clone())
                } else {
                    None
                }
            })
            .collect();

        for k in expired {
            self.cooldowns.remove(&k);
        }
    }

    fn cooldown_key(quota_group: &str, account_id: &str) -> String {
        format!("{}:{}", quota_group, account_id)
    }

    fn is_in_cooldown(&self, quota_group: &str, account_id: &str) -> bool {
        let key = Self::cooldown_key(quota_group, account_id);
        if let Some(entry) = self.cooldowns.get(&key) {
            if *entry.value() > std::time::Instant::now() {
                return true;
            }
            self.cooldowns.remove(&key);
        }
        false
    }

    fn min_cooldown_wait_ms(
        &self,
        quota_group: &str,
        tokens_snapshot: &[ProxyToken],
        attempted: &HashSet<String>,
    ) -> Option<u64> {
        let now = std::time::Instant::now();
        let mut min_ms: Option<u64> = None;

        for token in tokens_snapshot {
            if attempted.contains(&token.account_id) {
                continue;
            }
            let key = Self::cooldown_key(quota_group, &token.account_id);
            if let Some(entry) = self.cooldowns.get(&key) {
                let until = *entry.value();
                if until > now {
                    let ms = until.duration_since(now).as_millis() as u64;
                    min_ms = Some(min_ms.map(|m| m.min(ms)).unwrap_or(ms));
                }
            }
        }

        min_ms
    }

    pub fn mark_rate_limited_by_email(&self, quota_group: &str, email: &str, delay_ms: u64) {
        if email.trim().is_empty() {
            return;
        }

        // Emails should be unique; scan is OK (small pool).
        for entry in self.tokens.iter() {
            if entry.value().email == email {
                let account_id = entry.value().account_id.clone();
                let until = std::time::Instant::now()
                    .checked_add(std::time::Duration::from_millis(delay_ms))
                    .unwrap_or_else(|| std::time::Instant::now() + std::time::Duration::from_secs(3600));
                let key = Self::cooldown_key(quota_group, &account_id);
                self.cooldowns.insert(key, until);
                return;
            }
        }
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
        self.clear_session_pins_for_account(account_id);
        // Remove any cooldown entries for this account across all quota groups.
        let keys: Vec<String> = self
            .cooldowns
            .iter()
            .filter_map(|e| {
                if e.key().ends_with(&format!(":{}", account_id)) {
                    Some(e.key().clone())
                } else {
                    None
                }
            })
            .collect();
        for k in keys {
            self.cooldowns.remove(&k);
        }
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
        
        tracing::info!("已保存 project_id 到账号 {}", account_id);
        Ok(())
    }

    fn is_legacy_mock_project_id(project_id: &str) -> bool {
        let mut it = project_id.splitn(4, '-');
        let Some(adj) = it.next() else { return false; };
        let Some(noun) = it.next() else { return false; };
        let Some(rest) = it.next() else { return false; };
        if it.next().is_some() {
            return false;
        }
        if rest.len() != 5 || !rest.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
            return false;
        }
        matches!(adj, "useful" | "bright" | "swift" | "calm" | "bold")
            && matches!(noun, "fuze" | "wave" | "spark" | "flow" | "core")
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
        
        tracing::info!("已保存刷新后的 token 到账号 {}", account_id);
        Ok(())
    }
    
    pub fn len(&self) -> usize {
        self.tokens.len()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_token(account_id: &str, email: &str) -> ProxyToken {
        ProxyToken {
            account_id: account_id.to_string(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            expires_in: 3600,
            // Far future expiry_timestamp to avoid refresh paths in tests.
            timestamp: chrono::Utc::now().timestamp() + 3600 * 24,
            email: email.to_string(),
            account_path: std::env::temp_dir().join(format!("{account_id}.json")),
            project_id: Some("test-project".to_string()),
            subscription_tier: Some("PRO".to_string()),
        }
    }

    #[tokio::test]
    async fn test_session_pins_keep_conversations_on_same_account() {
        let manager = TokenManager::new(std::env::temp_dir());
        manager.tokens.insert("a".to_string(), make_token("a", "a@example.com"));
        manager.tokens.insert("b".to_string(), make_token("b", "b@example.com"));

        let (_, _, email_s1_first) = manager.get_token("claude", false, Some("s1")).await.unwrap();
        let (_, _, email_s2_first) = manager.get_token("claude", false, Some("s2")).await.unwrap();
        let (_, _, email_s1_again) = manager.get_token("claude", false, Some("s1")).await.unwrap();

        assert_eq!(email_s1_first, email_s1_again);
        assert_ne!(email_s1_first, email_s2_first);
    }

    #[tokio::test]
    async fn test_clear_session_pins_for_account() {
        let manager = TokenManager::new(std::env::temp_dir());
        manager
            .session_pins
            .insert("claude:s1".to_string(), ("a".to_string(), std::time::Instant::now()));
        manager
            .session_pins
            .insert("claude:s2".to_string(), ("b".to_string(), std::time::Instant::now()));

        manager.clear_session_pins_for_account("a");
        assert!(manager.session_pins.get("claude:s1").is_none());
        assert!(manager.session_pins.get("claude:s2").is_some());
    }
}
