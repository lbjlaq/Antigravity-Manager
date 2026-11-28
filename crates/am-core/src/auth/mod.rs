use crate::types::TokenInfo;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::collections::HashMap;

/// 官方个人/标准 OAuth Client ID
pub const OFFICIAL_CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
pub const OFFICIAL_CLIENT_SECRET: &str = "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";

/// 官方企业 / GCP (Vertex AI) OAuth Client ID
pub const GCP_CLIENT_ID: &str =
    "884354919052-36trc1jjb3tguiac32ov6cod268c5blh.apps.googleusercontent.com";
pub const GCP_CLIENT_SECRET: &str = "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";

pub const BUILTIN_STANDARD_CLIENT_KEY: &str = "antigravity";
pub const BUILTIN_GCP_CLIENT_KEY: &str = "antigravity_gcp";

pub const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";

pub const OAUTH_SCOPES: &[&str] = &[
    "openid",
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
    "https://www.googleapis.com/auth/cclog",
    "https://www.googleapis.com/auth/experimentsandconfigs",
    "https://www.googleapis.com/auth/aicode",
];

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    token_type: Option<String>,
    id_token: Option<String>,
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
            client: crate::runtime::shared_http_client().clone(),
        }
    }

    /// 从用户粘贴的完整回调 URL 或裸授权码中提取并解码 code 参数
    pub fn extract_code_from_input(input: &str) -> String {
        let trimmed = input.trim();
        if trimmed.contains("code=") {
            trimmed
                .split("code=")
                .nth(1)
                .and_then(|s| s.split('&').next())
                .and_then(|s| s.split('#').next())
                .and_then(|c| urlencoding::decode(c).ok().map(|cow| cow.into_owned()))
                .unwrap_or_else(|| trimmed.to_string())
        } else {
            trimmed.to_string()
        }
    }

    /// 解析 OAuth 客户端配置：(resolved_client_key, client_id, client_secret, is_gcp_tos)
    pub fn resolve_client_credentials(
        client_key: Option<&str>,
    ) -> (&'static str, &'static str, &'static str, bool) {
        let normalized = client_key
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(|k| k.to_ascii_lowercase());
        if let Some(ref key) = normalized {
            if key == BUILTIN_GCP_CLIENT_KEY
                || key == "enterprise"
                || key.contains("gcp")
                || key.contains("vertex")
            {
                return (
                    BUILTIN_GCP_CLIENT_KEY,
                    GCP_CLIENT_ID,
                    GCP_CLIENT_SECRET,
                    true,
                );
            }
        }
        (
            BUILTIN_STANDARD_CLIENT_KEY,
            OFFICIAL_CLIENT_ID,
            OFFICIAL_CLIENT_SECRET,
            false,
        )
    }

    /// 构造授权 URL (对齐 Google RFC 8252 规范)
    pub fn build_auth_url(redirect_uri: &str, state: &str) -> String {
        Self::build_auth_url_with_client(redirect_uri, state, None).0
    }

    /// 按指定客户端构造授权 URL，返回 `(auth_url, resolved_client_key)`
    pub fn build_auth_url_with_client(
        redirect_uri: &str,
        state: &str,
        client_key: Option<&str>,
    ) -> (String, String) {
        let (resolved_key, client_id, _, _) = Self::resolve_client_credentials(client_key);
        let scopes = OAUTH_SCOPES.join(" ");
        let url = format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&access_type=offline&prompt=consent&include_granted_scopes=true",
            GOOGLE_AUTH_URL,
            urlencoding::encode(client_id),
            urlencoding::encode(redirect_uri),
            urlencoding::encode(&scopes),
            urlencoding::encode(state)
        );
        (url, resolved_key.to_string())
    }

    /// 使用授权码 code 换取令牌组
    pub async fn exchange_code(&self, code: &str, redirect_uri: &str) -> Result<TokenInfo> {
        self.exchange_code_with_client(code, redirect_uri, None)
            .await
    }

    /// 使用授权码 code 与指定客户端换取令牌组
    pub async fn exchange_code_with_client(
        &self,
        code: &str,
        redirect_uri: &str,
        client_key: Option<&str>,
    ) -> Result<TokenInfo> {
        let (resolved_key, client_id, client_secret, is_gcp_tos) =
            Self::resolve_client_credentials(client_key);

        let mut params = HashMap::new();
        params.insert("client_id", client_id);
        params.insert("client_secret", client_secret);
        params.insert("code", code);
        params.insert("grant_type", "authorization_code");
        params.insert("redirect_uri", redirect_uri);

        let res = self
            .client
            .post(GOOGLE_TOKEN_URL)
            .form(&params)
            .send()
            .await?;

        let status = res.status();
        let body = res.text().await?;

        if !status.is_success() {
            return Err(anyhow!(
                "OAuth exchange failed with status {}: {}",
                status,
                body
            ));
        }

        let token_resp: GoogleTokenResponse = serde_json::from_str(&body)?;
        let now = chrono::Utc::now().timestamp();
        let expires_in = token_resp.expires_in.unwrap_or(3600);

        Ok(TokenInfo {
            access_token: token_resp.access_token,
            refresh_token: token_resp
                .refresh_token
                .ok_or_else(|| anyhow!("No refresh_token returned by Google"))?,
            expires_in: Some(expires_in),
            token_type: token_resp.token_type,
            expiry_timestamp: Some(now + expires_in),
            project_id: None,
            id_token: token_resp.id_token,
            oauth_client_key: Some(resolved_key.to_string()),
            is_gcp_tos: Some(is_gcp_tos),
        })
    }

    async fn refresh_with_credentials(
        &self,
        refresh_token: &str,
        resolved_key: &str,
        client_id: &str,
        client_secret: &str,
        is_gcp_tos: bool,
    ) -> std::result::Result<TokenInfo, (reqwest::StatusCode, String)> {
        let mut params = HashMap::new();
        params.insert("client_id", client_id);
        params.insert("client_secret", client_secret);
        params.insert("refresh_token", refresh_token);
        params.insert("grant_type", "refresh_token");

        let res = self
            .client
            .post(GOOGLE_TOKEN_URL)
            .form(&params)
            .send()
            .await
            .map_err(|e| (reqwest::StatusCode::BAD_GATEWAY, e.to_string()))?;

        let status = res.status();
        let body = res
            .text()
            .await
            .map_err(|e| (reqwest::StatusCode::BAD_GATEWAY, e.to_string()))?;

        if !status.is_success() {
            return Err((status, body));
        }

        let token_resp: GoogleTokenResponse =
            serde_json::from_str(&body).map_err(|e| (status, e.to_string()))?;
        let now = chrono::Utc::now().timestamp();
        let expires_in = token_resp.expires_in.unwrap_or(3600);

        Ok(TokenInfo {
            access_token: token_resp.access_token,
            refresh_token: token_resp
                .refresh_token
                .unwrap_or_else(|| refresh_token.to_string()),
            expires_in: Some(expires_in),
            token_type: token_resp.token_type,
            expiry_timestamp: Some(now + expires_in),
            project_id: None,
            id_token: token_resp.id_token,
            oauth_client_key: Some(resolved_key.to_string()),
            is_gcp_tos: Some(is_gcp_tos),
        })
    }

    /// 刷新 Access Token
    pub async fn refresh_access_token(&self, refresh_token: &str) -> Result<TokenInfo> {
        self.refresh_access_token_with_client(refresh_token, None)
            .await
    }

    /// 按指定客户端刷新 Access Token；若客户端不匹配则自动回退尝试另一内置官方客户端
    pub async fn refresh_access_token_with_client(
        &self,
        refresh_token: &str,
        client_key: Option<&str>,
    ) -> Result<TokenInfo> {
        let primary = Self::resolve_client_credentials(client_key);
        let fallback = if primary.0 == BUILTIN_GCP_CLIENT_KEY {
            Self::resolve_client_credentials(Some(BUILTIN_STANDARD_CLIENT_KEY))
        } else {
            Self::resolve_client_credentials(Some(BUILTIN_GCP_CLIENT_KEY))
        };

        match self
            .refresh_with_credentials(refresh_token, primary.0, primary.1, primary.2, primary.3)
            .await
        {
            Ok(info) => Ok(info),
            Err((status, body)) => {
                let lower = body.to_ascii_lowercase();
                let is_client_mismatch = status == reqwest::StatusCode::BAD_REQUEST
                    || status == reqwest::StatusCode::UNAUTHORIZED
                    || status == reqwest::StatusCode::FORBIDDEN
                    || lower.contains("unauthorized_client")
                    || lower.contains("invalid_client");
                if is_client_mismatch {
                    if let Ok(info) = self
                        .refresh_with_credentials(
                            refresh_token,
                            fallback.0,
                            fallback.1,
                            fallback.2,
                            fallback.3,
                        )
                        .await
                    {
                        return Ok(info);
                    }
                }
                Err(anyhow!(
                    "OAuth refresh failed with status {}: {}",
                    status,
                    body
                ))
            }
        }
    }

    /// 获取 Google 账号用户信息
    pub async fn fetch_user_info(&self, access_token: &str) -> Result<GoogleUserInfo> {
        let res = self
            .client
            .get(GOOGLE_USERINFO_URL)
            .bearer_auth(access_token)
            .send()
            .await?;

        let status = res.status();
        let body = res.text().await?;

        if !status.is_success() {
            return Err(anyhow!("Failed to fetch userinfo: status {}", status));
        }

        let user_info: GoogleUserInfo = serde_json::from_str(&body)?;
        Ok(user_info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_personal_and_enterprise_clients() {
        let (key_std, id_std, _, gcp_std) = OAuthClient::resolve_client_credentials(None);
        assert_eq!(key_std, BUILTIN_STANDARD_CLIENT_KEY);
        assert_eq!(id_std, OFFICIAL_CLIENT_ID);
        assert!(!gcp_std);

        let (key_ent, id_ent, _, gcp_ent) =
            OAuthClient::resolve_client_credentials(Some("antigravity_gcp"));
        assert_eq!(key_ent, BUILTIN_GCP_CLIENT_KEY);
        assert_eq!(id_ent, GCP_CLIENT_ID);
        assert!(gcp_ent);

        let (url_ent, resolved) = OAuthClient::build_auth_url_with_client(
            "http://localhost:1234/oauth-callback",
            "st",
            Some("enterprise"),
        );
        assert_eq!(resolved, BUILTIN_GCP_CLIENT_KEY);
        assert!(url_ent.contains(GCP_CLIENT_ID));
        assert!(url_ent.contains("include_granted_scopes=true"));
    }
}
