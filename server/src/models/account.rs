//! 账号数据模型

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{TokenData, QuotaData};

/// 账号信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    /// 唯一标识符
    pub id: String,
    
    /// 电子邮箱
    pub email: String,
    
    /// 显示名称
    #[serde(default)]
    pub name: Option<String>,
    
    /// Token 数据
    pub token: TokenData,
    
    /// 配额数据
    #[serde(default)]
    pub quota: Option<QuotaData>,
    
    /// 创建时间
    #[serde(default = "default_timestamp")]
    pub created_at: i64,
    
    /// 更新时间
    #[serde(default = "default_timestamp")]
    pub updated_at: i64,
}

fn default_timestamp() -> i64 {
    chrono::Utc::now().timestamp()
}

impl Account {
    /// 创建新账号
    pub fn new(email: String, name: Option<String>, token: TokenData) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id: Uuid::new_v4().to_string(),
            email,
            name,
            token,
            quota: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// 更新 Token
    pub fn update_token(&mut self, token: TokenData) {
        self.token = token;
        self.updated_at = chrono::Utc::now().timestamp();
    }

    /// 更新配额
    pub fn update_quota(&mut self, quota: QuotaData) {
        self.quota = Some(quota);
        self.updated_at = chrono::Utc::now().timestamp();
    }

    /// 获取显示名称
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.email)
    }
}

/// 账号摘要 (用于列表展示)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSummary {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub quota: Option<QuotaData>,
    pub token_expires_at: i64,
    pub is_token_valid: bool,
}

impl From<&Account> for AccountSummary {
    fn from(account: &Account) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id: account.id.clone(),
            email: account.email.clone(),
            name: account.name.clone(),
            quota: account.quota.clone(),
            token_expires_at: account.token.expiry_timestamp,
            is_token_valid: account.token.expiry_timestamp > now,
        }
    }
}

/// 添加账号请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddAccountRequest {
    /// Refresh Token (必填)
    pub refresh_token: String,
    
    /// 电子邮箱 (可选，如果不提供会自动获取)
    #[serde(default)]
    pub email: Option<String>,
    
    /// 显示名称 (可选)
    #[serde(default)]
    pub name: Option<String>,
}

/// 批量添加账号请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchAddAccountRequest {
    /// Refresh Token 列表
    pub refresh_tokens: Vec<String>,
}

/// 批量添加结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchAddResult {
    pub success: usize,
    pub failed: usize,
    pub accounts: Vec<Account>,
    pub errors: Vec<String>,
}
