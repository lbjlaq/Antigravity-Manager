use dashmap::DashMap;
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
    pub session_id: String,  // sessionId
}

pub struct TokenManager {
    tokens: Arc<DashMap<String, ProxyToken>>,  // account_id -> ProxyToken
    current_index: Arc<AtomicUsize>,
    data_dir: PathBuf,
}

impl TokenManager {
    /// 创建新的 TokenManager
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            tokens: Arc::new(DashMap::new()),
            current_index: Arc::new(AtomicUsize::new(0)),
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
        
        // project_id 和 session_id 是可选的
        let project_id = token_obj.get("project_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let session_id = token_obj.get("session_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| generate_session_id());
        
        Ok(Some(ProxyToken {
            account_id,
            access_token,
            refresh_token,
            expires_in,
            timestamp,
            email,
            account_path: path.clone(),
            project_id,
            session_id,
        }))
    }
    
    /// 获取当前可用的 Token（轮换机制）
    /// 如果 project_id 缺失，会尝试动态获取
    /// 如果 token 过期，会自动刷新
    pub async fn get_token(&self) -> Option<ProxyToken> {
        let total = self.tokens.len();
        if total == 0 {
            return None;
        }
        
        let idx = self.current_index.fetch_add(1, Ordering::SeqCst) % total;
        let mut token = self.tokens.iter().nth(idx).map(|entry| entry.value().clone())?;
        
        // 检查 token 是否过期（提前5分钟刷新）
        let now = chrono::Utc::now().timestamp();
        if now >= token.timestamp - 300 {
            tracing::info!("账号 {} 的 token 即将过期，正在刷新...", token.email);
            
            // 调用 OAuth 刷新 token
            match crate::modules::oauth::refresh_access_token(&token.refresh_token).await {
                Ok(token_response) => {
                    tracing::info!("Token 刷新成功！有效期: {} 秒", token_response.expires_in);
                    
                    // 更新 token 信息
                    token.access_token = token_response.access_token.clone();
                    token.expires_in = token_response.expires_in;
                    token.timestamp = now + token_response.expires_in;
                    
                    // 保存到文件
                    if let Err(e) = self.save_refreshed_token(&token.account_id, &token_response).await {
                        tracing::warn!("保存刷新后的 token 失败: {}", e);
                    }
                    
                    // 更新 DashMap 中的值
                    if let Some(mut entry) = self.tokens.get_mut(&token.account_id) {
                        entry.access_token = token.access_token.clone();
                        entry.expires_in = token.expires_in;
                        entry.timestamp = token.timestamp;
                    }
                },
                Err(e) => {
                    tracing::error!("刷新 token 失败: {}", e);
                    // 继续使用过期的 token，让 API 返回 401
                }
            }
        }
        
        // 如果没有 project_id，尝试获取
        if token.project_id.is_none() {
            tracing::info!("账号 {} 缺少 project_id，尝试获取...", token.email);
            
            match crate::modules::proxy::project_resolver::fetch_project_id(&token.access_token).await {
                Ok(project_id) => {
                    tracing::info!("成功获取 project_id: {}", project_id);
                    
                    // 更新到内存
                    token.project_id = Some(project_id.clone());
                    
                    // 保存到文件
                    if let Err(e) = self.save_project_id(&token.account_id, &project_id).await {
                        tracing::warn!("保存 project_id 失败: {}", e);
                    }
                    
                    // 更新 DashMap 中的值
                    if let Some(mut entry) = self.tokens.get_mut(&token.account_id) {
                        entry.project_id = Some(project_id);
                    }
                },
                Err(e) => {
                    tracing::warn!("获取 project_id 失败: {}, 使用占位符", e);
                    // 使用占位符 ID
                    let mock_id = crate::modules::proxy::project_resolver::generate_mock_project_id();
                    token.project_id = Some(mock_id.clone());
                    
                    // 保存占位符
                    if let Err(e) = self.save_project_id(&token.account_id, &mock_id).await {
                        tracing::warn!("保存占位符 project_id 失败: {}", e);
                    }
                    
                    // 更新内存
                    if let Some(mut entry) = self.tokens.get_mut(&token.account_id) {
                        entry.project_id = Some(mock_id);
                    }
                }
            }
        }
        
        Some(token)
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
    
    /// 获取当前加载的账号数量
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// 从 Account 同步账号到 TokenManager
    pub async fn sync_from_account(&self, account: &crate::models::Account) -> Result<(), String> {
        let accounts_dir = self.data_dir.join("accounts");
        let account_path = accounts_dir.join(format!("{}.json", account.id));
        
        // 构建 ProxyToken
        let token = ProxyToken {
            account_id: account.id.clone(),
            access_token: account.token.access_token.clone(),
            refresh_token: account.token.refresh_token.clone(),
            expires_in: account.token.expires_in,
            timestamp: account.token.expiry_timestamp,
            email: account.email.clone(),
            account_path: account_path.clone(),
            project_id: account.token.project_id.clone(),
            session_id: account.token.session_id.clone().unwrap_or_else(|| generate_session_id()),
        };
        
        self.tokens.insert(account.id.clone(), token);
        tracing::info!("已同步账号 {} 到 TokenManager", account.email);
        Ok(())
    }

    /// 从 AccountManager 同步所有账号
    pub async fn sync_from_account_manager(&self, accounts: &[crate::models::Account]) -> Result<usize, String> {
        // 清空现有 tokens
        self.tokens.clear();
        
        if accounts.is_empty() {
            tracing::warn!("AccountManager 返回的账号列表为空");
            return Ok(0);
        }
        
        let mut count = 0;
        for account in accounts {
            tracing::debug!("正在同步账号: {} ({})", account.email, account.id);
            match self.sync_from_account(account).await {
                Ok(_) => {
                    count += 1;
                    tracing::debug!("✅ 账号 {} 同步成功", account.email);
                },
                Err(e) => {
                    tracing::error!("❌ 同步账号 {} 到 TokenManager 失败: {}", account.email, e);
                }
            }
        }
        
        tracing::info!("TokenManager 已同步 {} 个账号（共 {} 个）", count, accounts.len());
        Ok(count)
    }

    /// 移除账号
    pub fn remove_account(&self, account_id: &str) {
        self.tokens.remove(account_id);
    }

}

/// 生成 sessionId
/// 格式：负数大整数字符串
fn generate_session_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    // 生成 1e18 到 9e18 之间的负数
    let num: i64 = -rng.gen_range(1_000_000_000_000_000_000..9_000_000_000_000_000_000);
    num.to_string()
}
