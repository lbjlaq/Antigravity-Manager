use crate::models::AccountType;
use serde::{Deserialize, Serialize};

// Google OAuth configuration - Antigravity (existing)
const ANTIGRAVITY_CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
const ANTIGRAVITY_CLIENT_SECRET: &str = "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";

// Google OAuth configuration - GeminiCLI
const GEMINICLI_CLIENT_ID: &str =
    "REPLACE_WITH_YOUR_GEMINICLI_CLIENT_ID";
const GEMINICLI_CLIENT_SECRET: &str = "REPLACE_WITH_YOUR_GEMINICLI_CLIENT_SECRET";

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";

/// Get OAuth client credentials based on account type
pub fn get_oauth_credentials(account_type: AccountType) -> (&'static str, &'static str) {
    match account_type {
        AccountType::Antigravity => (ANTIGRAVITY_CLIENT_ID, ANTIGRAVITY_CLIENT_SECRET),
        AccountType::GeminiCli => (GEMINICLI_CLIENT_ID, GEMINICLI_CLIENT_SECRET),
    }
}

/// Get OAuth scopes based on account type
pub fn get_scopes(account_type: AccountType) -> String {
    match account_type {
        AccountType::Antigravity => vec![
            "https://www.googleapis.com/auth/cloud-platform",
            "https://www.googleapis.com/auth/userinfo.email",
            "https://www.googleapis.com/auth/userinfo.profile",
            "https://www.googleapis.com/auth/cclog",
            "https://www.googleapis.com/auth/experimentsandconfigs",
        ]
        .join(" "),
        AccountType::GeminiCli => vec![
            "https://www.googleapis.com/auth/cloud-platform",
            "https://www.googleapis.com/auth/userinfo.email",
            "https://www.googleapis.com/auth/userinfo.profile",
        ]
        .join(" "),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_in: i64,
    #[serde(default)]
    pub token_type: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserInfo {
    pub email: String,
    pub name: Option<String>,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub picture: Option<String>,
}

impl UserInfo {
    /// Get best display name
    pub fn get_display_name(&self) -> Option<String> {
        // Prefer name
        if let Some(name) = &self.name {
            if !name.trim().is_empty() {
                return Some(name.clone());
            }
        }

        // If name is empty, combine given_name and family_name
        match (&self.given_name, &self.family_name) {
            (Some(given), Some(family)) => Some(format!("{} {}", given, family)),
            (Some(given), None) => Some(given.clone()),
            (None, Some(family)) => Some(family.clone()),
            (None, None) => None,
        }
    }
}

/// Generate OAuth authorization URL
pub fn get_auth_url(redirect_uri: &str, state: &str, account_type: AccountType) -> String {
    let (client_id, _) = get_oauth_credentials(account_type);
    let scopes = get_scopes(account_type);

    let params = vec![
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("response_type", "code"),
        ("scope", &scopes as &str),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("include_granted_scopes", "true"),
        ("state", state),
    ];

    let url = url::Url::parse_with_params(AUTH_URL, &params).expect("Invalid Auth URL");
    url.to_string()
}

/// Exchange authorization code for token
pub async fn exchange_code(
    code: &str,
    redirect_uri: &str,
    account_type: AccountType,
) -> Result<TokenResponse, String> {
    // [PHASE 2] 对于登录行为，尚未有 account_id，使用全局池阶梯逻辑
    let client = if let Some(pool) = crate::proxy::proxy_pool::get_global_proxy_pool() {
        pool.get_effective_standard_client(None, 60).await
    } else {
        crate::utils::http::get_long_standard_client()
    };

    let (client_id, client_secret) = get_oauth_credentials(account_type);
    let params = [
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
    ];

    tracing::debug!(
        "[OAuth] Sending exchange_code request with User-Agent: {}",
        crate::constants::NATIVE_OAUTH_USER_AGENT.as_str()
    );

    let response = client
        .post(TOKEN_URL)
        .header(rquest::header::USER_AGENT, crate::constants::NATIVE_OAUTH_USER_AGENT.as_str())
        .form(&params)
        .send()
        .await
        .map_err(|e| {
            if e.is_connect() || e.is_timeout() {
                format!("Token exchange request failed: {}. 请检查你的网络代理设置，确保可以稳定连接 Google 服务。", e)
            } else {
                format!("Token exchange request failed: {}", e)
            }
        })?;

    if response.status().is_success() {
        let token_res = response
            .json::<TokenResponse>()
            .await
            .map_err(|e| format!("Token parsing failed: {}", e))?;

        // Add detailed logs
        crate::modules::logger::log_info(&format!(
            "Token exchange successful! access_token: {}..., refresh_token: {}",
            &token_res.access_token.chars().take(20).collect::<String>(),
            if token_res.refresh_token.is_some() {
                "✓"
            } else {
                "✗ Missing"
            }
        ));

        // Log warning if refresh_token is missing
        if token_res.refresh_token.is_none() {
            crate::modules::logger::log_warn(
                "Warning: Google did not return a refresh_token. Potential reasons:\n\
                 1. User has previously authorized this application\n\
                 2. Need to revoke access in Google Cloud Console and retry\n\
                 3. OAuth parameter configuration issue",
            );
        }

        Ok(token_res)
    } else {
        let error_text = response.text().await.unwrap_or_default();
        Err(format!("Token exchange failed: {}", error_text))
    }
}

/// Refresh access_token using refresh_token
pub async fn refresh_access_token(
    refresh_token: &str,
    account_id: Option<&str>,
    account_type: AccountType,
) -> Result<TokenResponse, String> {
    // [PHASE 2] 根据 account_id 使用对应的代理
    let client = if let Some(pool) = crate::proxy::proxy_pool::get_global_proxy_pool() {
        pool.get_effective_standard_client(account_id, 60).await
    } else {
        crate::utils::http::get_long_standard_client()
    };

    let (client_id, client_secret) = get_oauth_credentials(account_type);
    let params = [
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ];

    // [FIX #1583] 提供更详细的日志，帮助诊断 Docker 环境下的代理问题
    if let Some(id) = account_id {
        crate::modules::logger::log_info(&format!("Refreshing Token for account: {}...", id));
    } else {
        crate::modules::logger::log_info("Refreshing Token for generic request (no account_id)...");
    }

    tracing::debug!(
        "[OAuth] Sending refresh_access_token request with User-Agent: {}",
        crate::constants::NATIVE_OAUTH_USER_AGENT.as_str()
    );

    let response = client
        .post(TOKEN_URL)
        .header(
            rquest::header::USER_AGENT,
            crate::constants::NATIVE_OAUTH_USER_AGENT.as_str(),
        )
        .form(&params)
        .send()
        .await
        .map_err(|e| {
            if e.is_connect() || e.is_timeout() {
                format!(
                    "Refresh request failed: {}. 无法连接 Google 授权服务器，请检查代理设置。",
                    e
                )
            } else {
                format!("Refresh request failed: {}", e)
            }
        })?;

    if response.status().is_success() {
        let token_data = response
            .json::<TokenResponse>()
            .await
            .map_err(|e| format!("Refresh data parsing failed: {}", e))?;

        crate::modules::logger::log_info(&format!(
            "Token refreshed successfully! Expires in: {} seconds",
            token_data.expires_in
        ));
        Ok(token_data)
    } else {
        let error_text = response.text().await.unwrap_or_default();
        Err(format!("Refresh failed: {}", error_text))
    }
}

/// Get user info
pub async fn get_user_info(
    access_token: &str,
    account_id: Option<&str>,
) -> Result<UserInfo, String> {
    let client = if let Some(pool) = crate::proxy::proxy_pool::get_global_proxy_pool() {
        pool.get_effective_client(account_id, 15).await
    } else {
        crate::utils::http::get_client()
    };

    let response = client
        .get(USERINFO_URL)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("User info request failed: {}", e))?;

    if response.status().is_success() {
        response
            .json::<UserInfo>()
            .await
            .map_err(|e| format!("User info parsing failed: {}", e))
    } else {
        let error_text = response.text().await.unwrap_or_default();
        Err(format!("Failed to get user info: {}", error_text))
    }
}

/// Check and refresh Token if needed
/// Returns the latest access_token
pub async fn ensure_fresh_token(
    current_token: &crate::models::TokenData,
    account_id: Option<&str>,
    account_type: AccountType,
) -> Result<crate::models::TokenData, String> {
    let now = chrono::Local::now().timestamp();

    // If no expiry or more than 5 minutes valid, return direct
    if current_token.expiry_timestamp > now + 300 {
        return Ok(current_token.clone());
    }

    // Need to refresh
    crate::modules::logger::log_info(&format!(
        "Token expiring soon for account {:?}, refreshing...",
        account_id
    ));
    let response =
        refresh_access_token(&current_token.refresh_token, account_id, account_type).await?;

    // Construct new TokenData
    Ok(crate::models::TokenData::new(
        response.access_token,
        current_token.refresh_token.clone(), // refresh_token may not be returned on refresh
        response.expires_in,
        current_token.email.clone(),
        current_token.project_id.clone(), // Keep original project_id
        None,                             // session_id will be generated in token_manager
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_auth_url_contains_state() {
        let redirect_uri = "http://localhost:8080/callback";
        let state = "test-state-123456";
        let url = get_auth_url(redirect_uri, state, AccountType::Antigravity);

        assert!(url.contains("state=test-state-123456"));
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A8080%2Fcallback"));
        assert!(url.contains("response_type=code"));
    }
}
