//! Token 管理器 (用于反代轮询)

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use dashmap::DashMap;
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use super::AccountService;

/// 代理用 Token
#[derive(Debug, Clone)]
pub struct ProxyToken {
    pub account_id: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub timestamp: i64,
    pub email: String,
    pub account_path: PathBuf,
    pub project_id: Option<String>,
    pub session_id: String,
}

/// Token 管理器
pub struct TokenManager {
    /// 配置
    config: Arc<RwLock<AppConfig>>,
    
    /// 账号服务
    account_service: Arc<AccountService>,
    
    /// Token 缓存 (account_id -> ProxyToken)
    tokens: Arc<DashMap<String, ProxyToken>>,
    
    /// 当前轮询索引
    current_index: Arc<AtomicUsize>,
}

impl TokenManager {
    /// 创建新的 Token 管理器
    pub async fn new(
        config: Arc<RwLock<AppConfig>>,
        account_service: Arc<AccountService>,
    ) -> AppResult<Self> {
        let manager = Self {
            config,
            account_service,
            tokens: Arc::new(DashMap::new()),
            current_index: Arc::new(AtomicUsize::new(0)),
        };
        
        manager.reload().await?;
        
        Ok(manager)
    }

    /// 重新加载所有账号
    pub async fn reload(&self) -> AppResult<usize> {
        let accounts = self.account_service.list().await;
        let accounts_dir = self.config.read().await.accounts_dir.clone();
        
        self.tokens.clear();
        
        for account in accounts {
            let path = accounts_dir.join(format!("{}.json", account.id));
            
            let proxy_token = ProxyToken {
                account_id: account.id.clone(),
                access_token: account.token.access_token.clone(),
                refresh_token: account.token.refresh_token.clone(),
                expires_in: account.token.expires_in,
                timestamp: account.token.expiry_timestamp,
                email: account.email.clone(),
                account_path: path,
                project_id: account.token.project_id.clone(),
                session_id: account.token.session_id.clone()
                    .unwrap_or_else(generate_session_id),
            };
            
            self.tokens.insert(account.id.clone(), proxy_token);
        }
        
        let count = self.tokens.len();
        tracing::info!("Token 管理器加载了 {} 个账号", count);
        
        Ok(count)
    }

    /// 获取当前可用的 Token (轮询)
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
            match self.refresh_token(&token).await {
                Ok(new_token) => {
                    token = new_token;
                }
                Err(e) => {
                    tracing::error!("刷新 token 失败: {}", e);
                    // 继续使用过期的 token
                }
            }
        }
        
        // 如果没有 project_id，尝试获取
        if token.project_id.is_none() {
            tracing::info!("账号 {} 缺少 project_id，尝试获取...", token.email);
            
            match self.fetch_project_id(&token.access_token).await {
                Ok(project_id) => {
                    token.project_id = Some(project_id.clone());
                    
                    // 更新缓存
                    if let Some(mut entry) = self.tokens.get_mut(&token.account_id) {
                        entry.project_id = Some(project_id);
                    }
                }
                Err(e) => {
                    tracing::warn!("获取 project_id 失败: {}, 使用占位符", e);
                    token.project_id = Some(generate_mock_project_id());
                }
            }
        }
        
        Some(token)
    }

    /// 刷新单个 Token
    async fn refresh_token(&self, token: &ProxyToken) -> AppResult<ProxyToken> {
        use super::OAuthService;
        
        let oauth = OAuthService::new(self.config.clone());
        let response = oauth.refresh_token(&token.refresh_token).await?;
        
        let now = chrono::Utc::now().timestamp();
        let mut new_token = token.clone();
        new_token.access_token = response.access_token;
        new_token.expires_in = response.expires_in;
        new_token.timestamp = now + response.expires_in;
        
        // 更新缓存
        if let Some(mut entry) = self.tokens.get_mut(&token.account_id) {
            entry.access_token = new_token.access_token.clone();
            entry.expires_in = new_token.expires_in;
            entry.timestamp = new_token.timestamp;
        }
        
        // 更新文件
        self.save_token_to_file(&new_token).await?;
        
        tracing::info!("账号 {} Token 刷新成功", token.email);
        
        Ok(new_token)
    }

    /// 保存 Token 到文件
    async fn save_token_to_file(&self, token: &ProxyToken) -> AppResult<()> {
        if let Ok(content) = std::fs::read_to_string(&token.account_path) {
            if let Ok(mut account) = serde_json::from_str::<serde_json::Value>(&content) {
                account["token"]["access_token"] = serde_json::Value::String(token.access_token.clone());
                account["token"]["expires_in"] = serde_json::Value::Number(token.expires_in.into());
                account["token"]["expiry_timestamp"] = serde_json::Value::Number(token.timestamp.into());
                
                if let Some(project_id) = &token.project_id {
                    account["token"]["project_id"] = serde_json::Value::String(project_id.clone());
                }
                
                let content = serde_json::to_string_pretty(&account)
                    .map_err(|e| AppError::Internal(format!("序列化失败: {}", e)))?;
                
                std::fs::write(&token.account_path, content)
                    .map_err(|e| AppError::Internal(format!("写入文件失败: {}", e)))?;
            }
        }
        
        Ok(())
    }

    /// 获取 Project ID
    async fn fetch_project_id(&self, access_token: &str) -> AppResult<String> {
        use super::create_http_client_with_proxy;
        
        let config = self.config.read().await;
        let client = create_http_client_with_proxy(15, config.proxy.upstream_proxy.as_deref());
        
        let response = client
            .get("https://aistudio.google.com/api/u/0/projects/new")
            .header("Authorization", format!("Bearer {}", access_token))
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("请求失败: {}", e)))?;

        if response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            
            // 尝试解析 project_id
            let re = regex::Regex::new(r#"projects/([a-zA-Z0-9-]+)"#).unwrap();
            if let Some(caps) = re.captures(&text) {
                return Ok(caps.get(1).unwrap().as_str().to_string());
            }
        }
        
        Err(AppError::Internal("无法获取 project_id".to_string()))
    }

    /// 获取可用 Token 数量
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
}

/// 生成 sessionId
fn generate_session_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let num: i64 = -rng.gen_range(1_000_000_000_000_000_000..9_000_000_000_000_000_000);
    num.to_string()
}

/// 生成占位符 Project ID
fn generate_mock_project_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let num: u64 = rng.gen_range(100000..999999);
    format!("mock-project-{}", num)
}
