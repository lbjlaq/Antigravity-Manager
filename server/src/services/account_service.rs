//! 账号管理服务

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::models::{Account, AccountSummary, AddAccountRequest, TokenData, QuotaData};
use super::OAuthService;

/// 账号管理服务
pub struct AccountService {
    /// 配置
    config: Arc<RwLock<AppConfig>>,
    
    /// 账号缓存 (id -> Account)
    accounts: RwLock<HashMap<String, Account>>,
}

impl AccountService {
    /// 创建新的账号服务
    pub async fn new(config: Arc<RwLock<AppConfig>>) -> AppResult<Self> {
        let service = Self {
            config,
            accounts: RwLock::new(HashMap::new()),
        };
        
        // 加载已有账号
        service.reload().await?;
        
        Ok(service)
    }

    /// 获取账号目录
    async fn accounts_dir(&self) -> PathBuf {
        self.config.read().await.accounts_dir.clone()
    }

    /// 重新加载所有账号
    pub async fn reload(&self) -> AppResult<()> {
        let accounts_dir = self.accounts_dir().await;
        
        if !accounts_dir.exists() {
            std::fs::create_dir_all(&accounts_dir)
                .map_err(|e| AppError::Internal(format!("创建账号目录失败: {}", e)))?;
            return Ok(());
        }

        let entries = std::fs::read_dir(&accounts_dir)
            .map_err(|e| AppError::Internal(format!("读取账号目录失败: {}", e)))?;

        let mut accounts = HashMap::new();

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            match self.load_account_from_file(&path) {
                Ok(account) => {
                    tracing::info!("加载账号: {} ({})", account.email, account.id);
                    accounts.insert(account.id.clone(), account);
                }
                Err(e) => {
                    tracing::warn!("加载账号失败 {:?}: {}", path, e);
                }
            }
        }

        tracing::info!("共加载 {} 个账号", accounts.len());
        *self.accounts.write().await = accounts;

        Ok(())
    }

    /// 从文件加载单个账号
    fn load_account_from_file(&self, path: &PathBuf) -> AppResult<Account> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| AppError::Internal(format!("读取文件失败: {}", e)))?;
        
        let account: Account = serde_json::from_str(&content)
            .map_err(|e| AppError::Internal(format!("解析 JSON 失败: {}", e)))?;
        
        Ok(account)
    }

    /// 保存账号到文件
    async fn save_account_to_file(&self, account: &Account) -> AppResult<()> {
        let accounts_dir = self.accounts_dir().await;
        let path = accounts_dir.join(format!("{}.json", account.id));
        
        let content = serde_json::to_string_pretty(account)
            .map_err(|e| AppError::Internal(format!("序列化失败: {}", e)))?;
        
        std::fs::write(&path, content)
            .map_err(|e| AppError::Internal(format!("写入文件失败: {}", e)))?;
        
        Ok(())
    }

    /// 列出所有账号
    pub async fn list(&self) -> Vec<Account> {
        self.accounts.read().await.values().cloned().collect()
    }

    /// 列出账号摘要
    pub async fn list_summary(&self) -> Vec<AccountSummary> {
        self.accounts.read().await
            .values()
            .map(AccountSummary::from)
            .collect()
    }

    /// 获取单个账号
    pub async fn get(&self, id: &str) -> Option<Account> {
        self.accounts.read().await.get(id).cloned()
    }

    /// 通过邮箱获取账号
    pub async fn get_by_email(&self, email: &str) -> Option<Account> {
        self.accounts.read().await
            .values()
            .find(|a| a.email == email)
            .cloned()
    }

    /// 添加账号 (通过 refresh_token)
    pub async fn add(&self, request: AddAccountRequest) -> AppResult<Account> {
        // 1. 刷新 token 获取 access_token
        let oauth_service = OAuthService::new(self.config.clone());
        let token_response = oauth_service.refresh_token(&request.refresh_token).await?;
        
        // 2. 获取用户信息
        let user_info = oauth_service.get_user_info(&token_response.access_token).await?;
        
        // 3. 检查是否已存在
        if self.get_by_email(&user_info.email).await.is_some() {
            // 更新现有账号
            return self.update_by_email(&user_info.email, request.refresh_token, token_response).await;
        }
        
        // 4. 创建新账号
        let token_data = TokenData::new(
            token_response.access_token,
            request.refresh_token,
            token_response.expires_in,
            Some(user_info.email.clone()),
            None, // project_id 稍后获取
            None, // session_id 稍后生成
        );
        
        let account = Account::new(
            user_info.email,
            request.name.or(user_info.display_name()),
            token_data,
        );
        
        // 5. 保存到文件
        self.save_account_to_file(&account).await?;
        
        // 6. 添加到缓存
        self.accounts.write().await.insert(account.id.clone(), account.clone());
        
        tracing::info!("添加账号成功: {}", account.email);
        
        Ok(account)
    }

    /// 更新现有账号的 Token
    async fn update_by_email(
        &self,
        email: &str,
        refresh_token: String,
        token_response: crate::models::TokenResponse,
    ) -> AppResult<Account> {
        let mut accounts = self.accounts.write().await;
        
        let account = accounts.values_mut()
            .find(|a| a.email == email)
            .ok_or_else(|| AppError::NotFound(format!("账号不存在: {}", email)))?;
        
        account.token.access_token = token_response.access_token;
        account.token.refresh_token = refresh_token;
        account.token.expires_in = token_response.expires_in;
        account.token.expiry_timestamp = chrono::Utc::now().timestamp() + token_response.expires_in;
        account.updated_at = chrono::Utc::now().timestamp();
        
        let account = account.clone();
        drop(accounts);
        
        self.save_account_to_file(&account).await?;
        
        tracing::info!("更新账号成功: {}", email);
        
        Ok(account)
    }

    /// 删除账号
    pub async fn delete(&self, id: &str) -> AppResult<()> {
        // 从缓存移除
        let account = self.accounts.write().await.remove(id);
        
        if let Some(account) = account {
            // 删除文件
            let accounts_dir = self.accounts_dir().await;
            let path = accounts_dir.join(format!("{}.json", id));
            
            if path.exists() {
                std::fs::remove_file(&path)
                    .map_err(|e| AppError::Internal(format!("删除文件失败: {}", e)))?;
            }
            
            tracing::info!("删除账号成功: {}", account.email);
            Ok(())
        } else {
            Err(AppError::NotFound(format!("账号不存在: {}", id)))
        }
    }

    /// 刷新账号 Token
    pub async fn refresh_token(&self, id: &str) -> AppResult<Account> {
        let account = self.get(id).await
            .ok_or_else(|| AppError::NotFound(format!("账号不存在: {}", id)))?;
        
        let oauth_service = OAuthService::new(self.config.clone());
        let token_response = oauth_service.refresh_token(&account.token.refresh_token).await?;
        
        // 更新账号
        let mut accounts = self.accounts.write().await;
        if let Some(acc) = accounts.get_mut(id) {
            acc.token.access_token = token_response.access_token;
            acc.token.expires_in = token_response.expires_in;
            acc.token.expiry_timestamp = chrono::Utc::now().timestamp() + token_response.expires_in;
            acc.updated_at = chrono::Utc::now().timestamp();
            
            let acc = acc.clone();
            drop(accounts);
            
            self.save_account_to_file(&acc).await?;
            
            tracing::info!("刷新 Token 成功: {}", acc.email);
            return Ok(acc);
        }
        
        Err(AppError::NotFound(format!("账号不存在: {}", id)))
    }

    /// 更新账号配额
    pub async fn update_quota(&self, id: &str, quota: QuotaData) -> AppResult<()> {
        let mut accounts = self.accounts.write().await;
        
        if let Some(account) = accounts.get_mut(id) {
            account.quota = Some(quota);
            account.updated_at = chrono::Utc::now().timestamp();
            
            let account = account.clone();
            drop(accounts);
            
            self.save_account_to_file(&account).await?;
            Ok(())
        } else {
            Err(AppError::NotFound(format!("账号不存在: {}", id)))
        }
    }

    /// 获取账号数量
    pub async fn count(&self) -> usize {
        self.accounts.read().await.len()
    }
}
