use serde::{Deserialize, Serialize};
use crate::modules::user_token_db::{self, UserToken, TokenIpBinding};
use crate::modules::security_db;
use crate::modules::token_stats;

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTokenRequest {
    pub username: String,
    pub expires_type: String,
    pub description: Option<String>,
    pub max_ips: i32,
    pub curfew_start: Option<String>,
    pub curfew_end: Option<String>,
    pub custom_expires_at: Option<i64>,  // 自定义过期时间戳 (秒)
    pub allowed_models: Vec<String>,     // 允许访问的模型列表，空列表表示不限制
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateTokenRequest {
    pub username: Option<String>,
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub max_ips: Option<i32>,
    pub curfew_start: Option<Option<String>>,
    pub curfew_end: Option<Option<String>>,
    pub allowed_models: Option<Vec<String>>,  // 允许访问的模型列表，None=不更新
}

// 命令实现

/// 列出所有令牌
#[tauri::command]
pub async fn list_user_tokens() -> Result<Vec<UserToken>, String> {
    user_token_db::list_tokens()
}

/// 创建新令牌
#[tauri::command]
pub async fn create_user_token(request: CreateTokenRequest) -> Result<UserToken, String> {
    user_token_db::create_token(
        request.username,
        request.expires_type,
        request.description,
        request.max_ips,
        request.curfew_start,
        request.curfew_end,
        request.custom_expires_at,
        request.allowed_models,
    )
}

/// 更新令牌
#[tauri::command]
pub async fn update_user_token(id: String, request: UpdateTokenRequest) -> Result<(), String> {
    user_token_db::update_token(
        &id,
        request.username,
        request.description,
        request.enabled,
        request.max_ips,
        request.curfew_start,
        request.curfew_end,
        request.allowed_models,
    )
}

/// 删除令牌
#[tauri::command]
pub async fn delete_user_token(id: String) -> Result<(), String> {
    user_token_db::delete_token(&id)
}

/// 续期令牌
#[tauri::command]
pub async fn renew_user_token(id: String, expires_type: String) -> Result<(), String> {
    user_token_db::renew_token(&id, &expires_type)
}

/// 获取令牌 IP 绑定
#[tauri::command]
pub async fn get_token_ip_bindings(token_id: String) -> Result<Vec<TokenIpBinding>, String> {
    user_token_db::get_token_ips(&token_id)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserTokenStats {
    pub total_tokens: usize,
    pub active_tokens: usize,
    pub total_users: usize,
    pub today_requests: i64,
    pub today_tokens: i64,
}

/// 获取简单的统计信息
#[tauri::command]
pub async fn get_user_token_summary() -> Result<UserTokenStats, String> {
    let tokens = user_token_db::list_tokens()?;
    let active_tokens = tokens.iter().filter(|t| t.enabled).count();
    
    // 统计唯一用户
    let mut users = std::collections::HashSet::new();
    for t in &tokens {
        users.insert(t.username.clone());
    }
    
    // 从安全数据库获取今日请求数
    let ip_stats = security_db::get_ip_stats()?;
    
    // 从 token_stats 获取今日 tokens (24小时)
    let token_summary = token_stats::get_summary_stats(24)?;
    
    Ok(UserTokenStats {
        total_tokens: tokens.len(),
        active_tokens,
        total_users: users.len(),
        today_requests: ip_stats.today_requests as i64,
        today_tokens: token_summary.total_tokens as i64,
    })
}
