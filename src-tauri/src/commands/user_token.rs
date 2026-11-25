use crate::modules::user_token_db::{
    self, TokenIpBinding, UserToken, UserTokenDetailedStats, UserTokenRankItem,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTokenRequest {
    pub username: String,
    pub expires_type: String,
    pub description: Option<String>,
    pub max_ips: i32,
    pub curfew_start: Option<String>,
    pub curfew_end: Option<String>,
    pub custom_expires_at: Option<i64>, // 自定义过期时间戳 (秒)
    #[serde(default)]
    pub allowed_account_ids: Option<Vec<String>>,
}

fn deserialize_double_option<'de, D>(
    deserializer: D,
) -> Result<Option<Option<Vec<String>>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<Vec<String>>::deserialize(deserializer)?;
    Ok(Some(
        crate::modules::user_token_db::normalize_allowed_account_ids(opt),
    ))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateTokenRequest {
    pub username: Option<String>,
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub max_ips: Option<i32>,
    pub curfew_start: Option<Option<String>>,
    pub curfew_end: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub allowed_account_ids: Option<Option<Vec<String>>>,
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
        request.allowed_account_ids,
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
        request.allowed_account_ids,
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

    // 这里简单返回一些数据，请求数最好从数据库聚合查询
    // 目前仅作为演示，请求数暂不精确统计今日的

    Ok(UserTokenStats {
        total_tokens: tokens.len(),
        active_tokens,
        total_users: users.len(),
        today_requests: 0, // TODO: Implement daily stats query
    })
}

/// 获取单个令牌的详细用量统计（走势、模型分布、IP）
#[tauri::command]
pub async fn get_user_token_stats(
    id: String,
    time_range: Option<String>,
) -> Result<UserTokenDetailedStats, String> {
    let range = time_range.unwrap_or_else(|| "7d".to_string());
    user_token_db::get_token_detailed_stats(&id, &range)
}

/// 获取用户 Token 消耗排行榜
#[tauri::command]
pub async fn get_user_tokens_ranking(
    time_range: Option<String>,
) -> Result<Vec<UserTokenRankItem>, String> {
    let range = time_range.unwrap_or_else(|| "all".to_string());
    user_token_db::get_user_tokens_ranking(&range)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_token_request_allowed_account_ids_deserialization() {
        // 1. Omitted JSON key => None (no change)
        let req_omitted: UpdateTokenRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(req_omitted.allowed_account_ids, None);

        // 2. JSON null => Some(None) (clear restriction)
        let req_null: UpdateTokenRequest =
            serde_json::from_str(r#"{"allowed_account_ids": null}"#).unwrap();
        assert_eq!(req_null.allowed_account_ids, Some(None));

        // 3. JSON [] => Some(None) (clear restriction)
        let req_empty: UpdateTokenRequest =
            serde_json::from_str(r#"{"allowed_account_ids": []}"#).unwrap();
        assert_eq!(req_empty.allowed_account_ids, Some(None));

        // 4. JSON ["acc-1"] => Some(Some(vec!["acc-1"]))
        let req_values: UpdateTokenRequest =
            serde_json::from_str(r#"{"allowed_account_ids": ["acc-1", " acc-2 ", "acc-1", ""]}"#)
                .unwrap();
        assert_eq!(
            req_values.allowed_account_ids,
            Some(Some(vec!["acc-1".to_string(), "acc-2".to_string()]))
        );
    }
}
