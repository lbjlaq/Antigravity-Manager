//! 账号管理模块 (Web 版本 - 简化版)
//! 仅保留核心的账号存储功能，移除桌面应用特有的功能

use std::fs;
use std::path::PathBuf;

use tokio::sync::RwLock;
use serde_json;
use uuid::Uuid;

use crate::models::{Account, AccountIndex, AccountSummary, TokenData, QuotaData};
use crate::modules::logger;

const DATA_DIR: &str = ".antigravity_tools";
const ACCOUNTS_INDEX: &str = "accounts.json";
const ACCOUNTS_DIR: &str = "accounts";

/// 账号管理器 (异步安全)
pub struct AccountManager {
    cache: RwLock<Vec<Account>>,
}

impl AccountManager {
    /// 创建新的账号管理器
    pub async fn new() -> Self {
        let _ = get_data_dir().unwrap_or_else(|_| PathBuf::from(".")); // Ensure dir exists
        let manager = Self {
            cache: RwLock::new(Vec::new()),
        };
        
        // 加载账号
        if let Ok(accounts) = load_all_accounts() {
            *manager.cache.write().await = accounts;
        }
        
        manager
    }

    /// 列出所有账号
    pub async fn list_accounts(&self) -> Result<Vec<Account>, String> {
        Ok(self.cache.read().await.clone())
    }

    /// 获取单个账号
    pub async fn get_account(&self, account_id: &str) -> Option<Account> {
        self.cache.read().await
            .iter()
            .find(|a| a.id == account_id)
            .cloned()
    }

    /// 添加账号
    pub async fn add_account(&self, email: &str, refresh_token: &str) -> Result<Account, String> {
        // 检查是否已存在
        {
            let accounts = self.cache.read().await;
            if accounts.iter().any(|a| a.email == email) {
                return Err(format!("账号已存在: {}", email));
            }
        }

        // 创建 Token（需要先刷新获取 access_token）
        let token = refresh_token_api(refresh_token).await?;
        
        // 创建账号
        let account_id = Uuid::new_v4().to_string();
        let account = Account::new(account_id, email.to_string(), token);
        
        // 保存到文件
        save_account(&account)?;
        
        // 更新索引
        update_index_add(&account)?;
        
        // 更新缓存
        self.cache.write().await.push(account.clone());
        
        Ok(account)
    }

    /// 删除账号
    pub async fn delete_account(&self, account_id: &str) -> Result<(), String> {
        // 从文件删除
        delete_account_file(account_id)?;
        
        // 更新索引
        update_index_remove(account_id)?;
        
        // 更新缓存
        self.cache.write().await.retain(|a| a.id != account_id);
        
        Ok(())
    }

    /// 切换账号（Web 版本仅更新当前账号标记）
    pub async fn switch_account(&self, account_id: &str) -> Result<Account, String> {
        let account = self.get_account(account_id).await
            .ok_or_else(|| format!("账号不存在: {}", account_id))?;
        
        // 更新索引中的当前账号
        let mut index = load_account_index()?;
        index.current_account_id = Some(account_id.to_string());
        save_account_index(&index)?;
        
        Ok(account)
    }

    /// 获取当前账号
    pub async fn get_current_account(&self) -> Option<Account> {
        let index = load_account_index().ok()?;
        let current_id = index.current_account_id?;
        self.get_account(&current_id).await
    }

    /// 更新账号配额
    pub async fn update_quota(&self, account_id: &str, quota: QuotaData) -> Result<(), String> {
        // 更新缓存
        {
            let mut accounts = self.cache.write().await;
            if let Some(account) = accounts.iter_mut().find(|a| a.id == account_id) {
                account.update_quota(quota.clone());
                // 保存到文件
                save_account(account)?;
            }
        }
        Ok(())
    }
}

// ========== 文件操作函数（与原始项目保持一致）==========

/// 获取数据目录路径
pub fn get_data_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("无法获取用户主目录")?;
    let data_dir = home.join(DATA_DIR);
    
    if !data_dir.exists() {
        fs::create_dir_all(&data_dir)
            .map_err(|e| format!("创建数据目录失败: {}", e))?;
    }
    
    Ok(data_dir)
}

/// 获取账号目录路径
pub fn get_accounts_dir() -> Result<PathBuf, String> {
    let data_dir = get_data_dir()?;
    let accounts_dir = data_dir.join(ACCOUNTS_DIR);
    
    if !accounts_dir.exists() {
        fs::create_dir_all(&accounts_dir)
            .map_err(|e| format!("创建账号目录失败: {}", e))?;
    }
    
    Ok(accounts_dir)
}

/// 加载账号索引
pub fn load_account_index() -> Result<AccountIndex, String> {
    let data_dir = get_data_dir()?;
    let index_path = data_dir.join(ACCOUNTS_INDEX);
    
    if !index_path.exists() {
        return Ok(AccountIndex::new());
    }
    
    let content = fs::read_to_string(&index_path)
        .map_err(|e| format!("读取账号索引失败: {}", e))?;
    
    serde_json::from_str(&content)
        .map_err(|e| format!("解析账号索引失败: {}", e))
}

/// 保存账号索引
pub fn save_account_index(index: &AccountIndex) -> Result<(), String> {
    let data_dir = get_data_dir()?;
    let index_path = data_dir.join(ACCOUNTS_INDEX);
    
    let content = serde_json::to_string_pretty(index)
        .map_err(|e| format!("序列化账号索引失败: {}", e))?;
    
    fs::write(&index_path, content)
        .map_err(|e| format!("保存账号索引失败: {}", e))
}

/// 保存账号数据
pub fn save_account(account: &Account) -> Result<(), String> {
    let accounts_dir = get_accounts_dir()?;
    let account_path = accounts_dir.join(format!("{}.json", account.id));
    
    let content = serde_json::to_string_pretty(account)
        .map_err(|e| format!("序列化账号数据失败: {}", e))?;
    
    fs::write(&account_path, content)
        .map_err(|e| format!("保存账号数据失败: {}", e))
}

/// 加载单个账号
pub fn load_account(account_id: &str) -> Result<Account, String> {
    let accounts_dir = get_accounts_dir()?;
    let account_path = accounts_dir.join(format!("{}.json", account_id));
    
    if !account_path.exists() {
        return Err(format!("账号不存在: {}", account_id));
    }
    
    let content = fs::read_to_string(&account_path)
        .map_err(|e| format!("读取账号数据失败: {}", e))?;
    
    serde_json::from_str(&content)
        .map_err(|e| format!("解析账号数据失败: {}", e))
}

/// 加载所有账号
fn load_all_accounts() -> Result<Vec<Account>, String> {
    let index = load_account_index()?;
    let mut accounts = Vec::new();
    
    for summary in &index.accounts {
        match load_account(&summary.id) {
            Ok(account) => accounts.push(account),
            Err(e) => logger::log_warn(&format!("加载账号 {} 失败: {}", summary.id, e)),
        }
    }
    
    Ok(accounts)
}

/// 删除账号文件
fn delete_account_file(account_id: &str) -> Result<(), String> {
    let accounts_dir = get_accounts_dir()?;
    let account_path = accounts_dir.join(format!("{}.json", account_id));
    
    if account_path.exists() {
        fs::remove_file(&account_path)
            .map_err(|e| format!("删除账号文件失败: {}", e))?;
    }
    
    Ok(())
}

/// 更新索引 - 添加账号
fn update_index_add(account: &Account) -> Result<(), String> {
    let mut index = load_account_index()?;
    
    index.accounts.push(AccountSummary {
        id: account.id.clone(),
        email: account.email.clone(),
        name: account.name.clone(),
        created_at: account.created_at,
        last_used: account.last_used,
    });
    
    if index.current_account_id.is_none() {
        index.current_account_id = Some(account.id.clone());
    }
    
    save_account_index(&index)
}

/// 更新索引 - 移除账号
fn update_index_remove(account_id: &str) -> Result<(), String> {
    let mut index = load_account_index()?;
    
    index.accounts.retain(|s| s.id != account_id);
    
    if index.current_account_id.as_deref() == Some(account_id) {
        index.current_account_id = index.accounts.first().map(|s| s.id.clone());
    }
    
    save_account_index(&index)
}

/// 刷新 Token（通过 Google OAuth API）
async fn refresh_token_api(refresh_token: &str) -> Result<TokenData, String> {
    let client = reqwest::Client::new();
    
    let response = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", "77185425430.apps.googleusercontent.com"),
            ("client_secret", "OTJgUOQcT7lO7GsGZq2G4IlT"),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| format!("Token 刷新请求失败: {}", e))?;
    
    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Token 刷新失败: {}", text));
    }
    
    #[derive(serde::Deserialize)]
    struct TokenResponse {
        access_token: String,
        expires_in: i64,
    }
    
    let token_resp: TokenResponse = response.json().await
        .map_err(|e| format!("解析 Token 响应失败: {}", e))?;
    
    Ok(TokenData::new(
        token_resp.access_token,
        refresh_token.to_string(),
        token_resp.expires_in,
        None,
        None,
        None,
    ))
}
