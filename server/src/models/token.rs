//! Token 数据模型

use serde::{Deserialize, Serialize};

/// Token 数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenData {
    /// Access Token
    pub access_token: String,
    
    /// Refresh Token
    pub refresh_token: String,
    
    /// 有效期(秒)
    #[serde(default)]
    pub expires_in: i64,
    
    /// 过期时间戳
    #[serde(default)]
    pub expiry_timestamp: i64,
    
    /// 关联邮箱
    #[serde(default)]
    pub email: Option<String>,
    
    /// Project ID (Google Cloud)
    #[serde(default)]
    pub project_id: Option<String>,
    
    /// Session ID
    #[serde(default)]
    pub session_id: Option<String>,
}

impl TokenData {
    /// 创建新的 Token 数据
    pub fn new(
        access_token: String,
        refresh_token: String,
        expires_in: i64,
        email: Option<String>,
        project_id: Option<String>,
        session_id: Option<String>,
    ) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            access_token,
            refresh_token,
            expires_in,
            expiry_timestamp: now + expires_in,
            email,
            project_id,
            session_id,
        }
    }

    /// 检查 Token 是否过期 (提前5分钟)
    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        now >= self.expiry_timestamp - 300
    }

    /// 检查 Token 是否有效
    pub fn is_valid(&self) -> bool {
        !self.access_token.is_empty() && !self.is_expired()
    }

    /// 更新 Access Token
    pub fn update_access_token(&mut self, access_token: String, expires_in: i64) {
        self.access_token = access_token;
        self.expires_in = expires_in;
        self.expiry_timestamp = chrono::Utc::now().timestamp() + expires_in;
    }
}

/// OAuth Token 响应
#[derive(Debug, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_in: i64,
    #[serde(default)]
    pub token_type: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

/// 用户信息 (从 Google 获取)
#[derive(Debug, Serialize, Deserialize)]
pub struct UserInfo {
    pub email: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub given_name: Option<String>,
    #[serde(default)]
    pub family_name: Option<String>,
    #[serde(default)]
    pub picture: Option<String>,
}

impl UserInfo {
    /// 获取最佳显示名称
    pub fn display_name(&self) -> Option<String> {
        if let Some(name) = &self.name {
            if !name.trim().is_empty() {
                return Some(name.clone());
            }
        }
        
        match (&self.given_name, &self.family_name) {
            (Some(given), Some(family)) => Some(format!("{} {}", given, family)),
            (Some(given), None) => Some(given.clone()),
            (None, Some(family)) => Some(family.clone()),
            (None, None) => None,
        }
    }
}
