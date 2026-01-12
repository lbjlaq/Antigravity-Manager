use serde::{Deserialize, Serialize};

// Google OAuth 配置
const CLIENT_ID: &str = "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
const CLIENT_SECRET: &str = "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";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";

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
    /// 获取最佳的显示名称
    pub fn get_display_name(&self) -> Option<String> {
        // 优先使用 name
        if let Some(name) = &self.name {
            if !name.trim().is_empty() {
                return Some(name.clone());
            }
        }

        // 如果 name 为空，尝试组合 given_name 和 family_name
        match (&self.given_name, &self.family_name) {
            (Some(given), Some(family)) => Some(format!("{} {}", given, family)),
            (Some(given), None) => Some(given.clone()),
            (None, Some(family)) => Some(family.clone()),
            (None, None) => None,
        }
    }
}

/// 生成 OAuth 授权 URL
pub fn get_auth_url(redirect_uri: &str) -> String {
    let scopes = ["https://www.googleapis.com/auth/cloud-platform",
        "https://www.googleapis.com/auth/userinfo.email",
        "https://www.googleapis.com/auth/userinfo.profile",
        "https://www.googleapis.com/auth/cclog",
        "https://www.googleapis.com/auth/experimentsandconfigs"]
    .join(" ");

    let params = vec![
        ("client_id", CLIENT_ID),
        ("redirect_uri", redirect_uri),
        ("response_type", "code"),
        ("scope", &scopes),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("include_granted_scopes", "true"),
    ];

    let url = url::Url::parse_with_params(AUTH_URL, &params).expect("无效的 Auth URL");
    url.to_string()
}

/// 使用 Authorization Code 交换 Token
pub async fn exchange_code(code: &str, redirect_uri: &str) -> Result<TokenResponse, String> {
    let client = crate::utils::http::create_client(15);

    let params = [
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
    ];

    let response = client
        .post(TOKEN_URL)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Token 交换请求失败: {}", e))?;

    if response.status().is_success() {
        let token_res = response
            .json::<TokenResponse>()
            .await
            .map_err(|e| format!("Token 解析失败: {}", e))?;

        // 添加详细日志
        crate::modules::logger::log_info(&format!(
            "Token 交换成功! access_token: {}..., refresh_token: {}",
            &token_res.access_token.chars().take(20).collect::<String>(),
            if token_res.refresh_token.is_some() {
                "✓"
            } else {
                "✗ 缺失"
            }
        ));

        // 如果缺少 refresh_token,记录警告
        if token_res.refresh_token.is_none() {
            crate::modules::logger::log_warn(
                "警告: Google 未返回 refresh_token。可能原因:\n\
                 1. 用户之前已授权过此应用\n\
                 2. 需要在 Google Cloud Console 撤销授权后重试\n\
                 3. OAuth 参数配置问题",
            );
        }

        Ok(token_res)
    } else {
        let error_text = response.text().await.unwrap_or_default();
        Err(format!("Token 交换失败: {}", error_text))
    }
}

/// 使用 refresh_token 刷新 access_token
pub async fn refresh_access_token(refresh_token: &str) -> Result<TokenResponse, String> {
    let client = crate::utils::http::create_client(15);

    let params = [
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ];

    crate::modules::logger::log_info("正在刷新 Token...");

    let response = client
        .post(TOKEN_URL)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("刷新请求失败: {}", e))?;

    if response.status().is_success() {
        let token_data = response
            .json::<TokenResponse>()
            .await
            .map_err(|e| format!("刷新数据解析失败: {}", e))?;

        crate::modules::logger::log_info(&format!(
            "Token 刷新成功！有效期: {} 秒",
            token_data.expires_in
        ));
        Ok(token_data)
    } else {
        let error_text = response.text().await.unwrap_or_default();
        Err(format!("刷新失败: {}", error_text))
    }
}

/// 获取用户信息
pub async fn get_user_info(access_token: &str) -> Result<UserInfo, String> {
    let client = crate::utils::http::create_client(15);

    let response = client
        .get(USERINFO_URL)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("用户信息请求失败: {}", e))?;

    if response.status().is_success() {
        response
            .json::<UserInfo>()
            .await
            .map_err(|e| format!("用户信息解析失败: {}", e))
    } else {
        let error_text = response.text().await.unwrap_or_default();
        Err(format!("获取用户信息失败: {}", error_text))
    }
}

/// 检查并在需要时刷新 Token
/// 返回最新的 access_token
pub async fn ensure_fresh_token(
    current_token: &crate::models::TokenData,
) -> Result<crate::models::TokenData, String> {
    let now = chrono::Local::now().timestamp();

    // 如果没有过期时间，或者还有超过 5 分钟有效期，直接返回
    if current_token.expiry_timestamp > now + 300 {
        return Ok(current_token.clone());
    }

    // 需要刷新
    crate::modules::logger::log_info("Token 即将过期，正在刷新...");
    let response = refresh_access_token(&current_token.refresh_token).await?;

    // 构造新 TokenData
    Ok(crate::models::TokenData::new(
        response.access_token,
        current_token.refresh_token.clone(), // 刷新时不一定会返回新的 refresh_token
        response.expires_in,
        current_token.email.clone(),
        current_token.project_id.clone(), // 保留原有 project_id
        None,                             // session_id 会在 token_manager 中生成
    ))
}
