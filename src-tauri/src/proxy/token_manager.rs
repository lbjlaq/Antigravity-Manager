// 移除冗余的顶层导入，因为这些在代码中已由 full path 或局部导入处理
use dashmap::DashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

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
}

pub struct TokenManager {
    tokens: Arc<DashMap<String, ProxyToken>>,  // account_id -> ProxyToken
    current_index: Arc<AtomicUsize>,
    last_used_account: Arc<tokio::sync::Mutex<Option<(String, std::time::Instant)>>>,
    pinned_account: Arc<RwLock<Option<String>>>,
    data_dir: PathBuf,
}

impl TokenManager {
    /// 创建新的 TokenManager
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            tokens: Arc::new(DashMap::new()),
            current_index: Arc::new(AtomicUsize::new(0)),
            last_used_account: Arc::new(tokio::sync::Mutex::new(None)),
            pinned_account: Arc::new(RwLock::new(None)),
            data_dir,
        }
    }
    
    /// 从主应用账号目录加载所有账号
    pub async fn load_accounts(&self) -> Result<usize, String> {
        let accounts_dir = self.data_dir.join("accounts");
        
        if !accounts_dir.exists() {
            return Err(format!("账号目录不存在: {:?}", accounts_dir));
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

    /// 从磁盘重新加载账号（增量同步，避免短暂空池）
    pub async fn reload_accounts(&self) -> Result<usize, String> {
        use std::collections::{HashMap, HashSet};

        let accounts_dir = self.data_dir.join("accounts");
        if !accounts_dir.exists() {
            self.tokens.clear();
            let mut last_used = self.last_used_account.lock().await;
            *last_used = None;
            let mut pinned = self.pinned_account.write().await;
            *pinned = None;
            return Ok(0);
        }

        let entries = std::fs::read_dir(&accounts_dir)
            .map_err(|e| format!("读取账号目录失败: {}", e))?;

        let mut next: HashMap<String, ProxyToken> = HashMap::new();
        for entry in entries {
            let entry = entry.map_err(|e| format!("读取目录项失败: {}", e))?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            match self.load_single_account(&path).await {
                Ok(Some(token)) => {
                    next.insert(token.account_id.clone(), token);
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("加载账号失败 {:?}: {}", path, e);
                }
            }
        }

        let next_keys: HashSet<String> = next.keys().cloned().collect();

        // 删除不存在的旧 token
        let existing_keys: Vec<String> = self.tokens.iter().map(|e| e.key().clone()).collect();
        for key in existing_keys {
            if !next_keys.contains(&key) {
                self.tokens.remove(&key);
            }
        }

        // 覆盖/新增
        for (key, token) in next {
            self.tokens.insert(key, token);
        }

        // 清理 last_used / pinned 指向的无效账号
        {
            let mut last_used = self.last_used_account.lock().await;
            if let Some((account_id, _)) = &*last_used {
                if !self.tokens.contains_key(account_id) {
                    *last_used = None;
                }
            }
        }
        {
            let mut pinned = self.pinned_account.write().await;
            if let Some(account_id) = pinned.as_ref() {
                if !self.tokens.contains_key(account_id) {
                    *pinned = None;
                }
            }
        }

        Ok(self.tokens.len())
    }

    /// 固定（pin）指定账号为优先账号；传 None 表示取消固定
    pub async fn pin_account(&self, account_id: Option<String>) {
        let mut pinned = self.pinned_account.write().await;
        *pinned = account_id;
    }

    pub async fn pinned_account_id(&self) -> Option<String> {
        self.pinned_account.read().await.clone()
    }

    /// 加载单个账号
    async fn load_single_account(&self, path: &PathBuf) -> Result<Option<ProxyToken>, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("读取文件失败: {}", e))?;
        
        let account: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| format!("解析 JSON 失败: {}", e))?;
        
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
        
        Ok(Some(ProxyToken {
            account_id,
            access_token,
            refresh_token,
            expires_in,
            timestamp,
            email,
            account_path: path.clone(),
            project_id,
        }))
    }
    
    /// 获取当前可用的 Token（带 60s 时间窗口锁定机制 + pin 账号支持）
    /// 参数 `quota_group` 用于区分 "claude" vs "gemini" 组
    /// 参数 `force_rotate` 为 true 时将忽略锁定，强制切换账号
    pub async fn get_token(&self, quota_group: &str, force_rotate: bool) -> Result<(String, String, String), String> {
        let total = self.tokens.len();
        if total == 0 {
            return Err("Token pool is empty".to_string());
        }

        // 0. 如果有 pin 且不强制轮换，优先使用指定账号
        let mut target_token: Option<ProxyToken> = None;
        if !force_rotate {
            if let Some(pinned_id) = self.pinned_account.read().await.clone() {
                if let Some(entry) = self.tokens.get(&pinned_id) {
                    tracing::info!("Pinned 账号生效: {}", entry.email);
                    target_token = Some(entry.value().clone());
                } else {
                    tracing::warn!("Pinned 账号不存在于池中: {}", pinned_id);
                }
            }
        }

        // 1. 检查时间窗口锁定 (60秒内强制复用上一个账号)
        // 优化策略: 画图请求 (image_gen) 默认不锁定，以最大化并发能力
        if target_token.is_none() && !force_rotate && quota_group != "image_gen" {
            let last_used = self.last_used_account.lock().await;
            if let Some((account_id, last_time)) = &*last_used {
                if last_time.elapsed().as_secs() < 60 {
                    if let Some(entry) = self.tokens.get(account_id) {
                        tracing::info!("60s 时间窗口内，强制复用上一个账号: {}", entry.email);
                        target_token = Some(entry.value().clone());
                    }
                }
            }
        }

        // 2. 如果没有锁定、锁定失效或强制轮换，则进行轮询记录并更新锁定信息
        let mut token = if let Some(t) = target_token {
            // 如果是 pin 模式，同样更新 last_used（用于复用统计/日志一致性）
            if !force_rotate && quota_group != "image_gen" {
                let mut last_used = self.last_used_account.lock().await;
                *last_used = Some((t.account_id.clone(), std::time::Instant::now()));
            }
            t
        } else {
            // 简单轮换策略 (Round Robin)
            let idx = self.current_index.fetch_add(1, Ordering::SeqCst) % total;
            let selected_token = self.tokens.iter()
                .nth(idx)
                .map(|entry| entry.value().clone())
                .ok_or("Failed to retrieve token from pool")?;

            // 更新最后使用的账号及时间 (如果是普通对话请求)
            if quota_group != "image_gen" {
                let mut last_used = self.last_used_account.lock().await;
                *last_used = Some((selected_token.account_id.clone(), std::time::Instant::now()));
            }

            let action_msg = if force_rotate { "强制切换" } else { "切换" };
            tracing::info!("{}到账号: {}", action_msg, selected_token.email);
            selected_token
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
                }
                Err(e) => {
                    tracing::error!("Token 刷新失败: {}，尝试下一个账号", e);
                    return Err(format!("Token refresh failed: {}", e));
                }
            }
        }

        // 4. 确保有 project_id
        let project_id = if let Some(pid) = &token.project_id {
            pid.clone()
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
                    return Err(format!("Failed to fetch project_id: {}", e));
                }
            }
        };

        Ok((token.access_token, project_id, token.email))
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
    
    /// 保存刷新后的 token 到账号文件
    #[allow(dead_code)]
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
