use crate::types::TokenInfo;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::collections::HashMap;

/// 官方唯一 OAuth Client ID
pub const OFFICIAL_CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
pub const OFFICIAL_CLIENT_SECRET: &str = "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";

pub const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";

pub const OAUTH_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
    "https://www.googleapis.com/auth/cloud-platform",
];

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    token_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GoogleUserInfo {
    pub email: String,
    pub name: Option<String>,
}

pub struct OAuthClient {
    client: reqwest::Client,
}

impl OAuthClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// 构造授权 URL
    pub fn build_auth_url(redirect_uri: &str, state: &str) -> String {
        let scopes = OAUTH_SCOPES.join(" ");
        format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&access_type=offline&prompt=consent",
            GOOGLE_AUTH_URL,
            urlencoding::encode(OFFICIAL_CLIENT_ID),
            urlencoding::encode(redirect_uri),
            urlencoding::encode(&scopes),
            urlencoding::encode(state),
        )
    }

    /// 使用授权 Code 换取 Token
    pub async fn exchange_code(&self, code: &str, redirect_uri: &str) -> Result<TokenInfo> {
        let mut params = HashMap::new();
        params.insert("client_id", OFFICIAL_CLIENT_ID);
        params.insert("client_secret", OFFICIAL_CLIENT_SECRET);
        params.insert("code", code);
        params.insert("grant_type", "authorization_code");
        params.insert("redirect_uri", redirect_uri);

        let res = self
            .client
            .post(GOOGLE_TOKEN_URL)
            .form(&params)
            .send()
            .await?;

        if !res.status().is_success() {
            let err_text = res.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to exchange oauth code: {}", err_text));
        }

        let token_resp: GoogleTokenResponse = res.json().await?;
        let refresh_token = token_resp
            .refresh_token
            .ok_or_else(|| anyhow!("No refresh_token returned from Google OAuth"))?;

        let now = chrono::Utc::now().timestamp();
        let expiry = token_resp.expires_in.map(|exp| now + exp);

        Ok(TokenInfo {
            access_token: token_resp.access_token,
            refresh_token,
            expires_in: token_resp.expires_in,
            token_type: token_resp.token_type,
            expiry_timestamp: expiry,
            project_id: None,
        })
    }

    /// 使用 refresh_token 刷新 access_token
    pub async fn refresh_access_token(&self, refresh_token: &str) -> Result<TokenInfo> {
        let mut params = HashMap::new();
        params.insert("client_id", OFFICIAL_CLIENT_ID);
        params.insert("client_secret", OFFICIAL_CLIENT_SECRET);
        params.insert("refresh_token", refresh_token);
        params.insert("grant_type", "refresh_token");

        let res = self
            .client
            .post(GOOGLE_TOKEN_URL)
            .form(&params)
            .send()
            .await?;

        if !res.status().is_success() {
            let err_text = res.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to refresh token: {}", err_text));
        }

        let token_resp: GoogleTokenResponse = res.json().await?;
        let now = chrono::Utc::now().timestamp();
        let expiry = token_resp.expires_in.map(|exp| now + exp);

        Ok(TokenInfo {
            access_token: token_resp.access_token,
            refresh_token: token_resp
                .refresh_token
                .unwrap_or_else(|| refresh_token.to_string()),
            expires_in: token_resp.expires_in,
            token_type: token_resp.token_type,
            expiry_timestamp: expiry,
            project_id: None,
        })
    }

    /// 获取用户信息 (email, name)
    pub async fn fetch_user_info(&self, access_token: &str) -> Result<GoogleUserInfo> {
        let res = self
            .client
            .get(GOOGLE_USERINFO_URL)
            .bearer_auth(access_token)
            .send()
            .await?;

        if !res.status().is_success() {
            let err_text = res.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to fetch user info: {}", err_text));
        }

        let user_info: GoogleUserInfo = res.json().await?;
        Ok(user_info)
    }
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
    }
}
