//! OAuth 服务

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::models::{TokenResponse, UserInfo};
use super::create_http_client_with_proxy;

// Google OAuth 配置
const CLIENT_ID: &str = "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
const CLIENT_SECRET: &str = "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";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";

/// OAuth 服务
pub struct OAuthService {
    config: Arc<RwLock<AppConfig>>,
}

impl OAuthService {
    /// 创建新的 OAuth 服务
    pub fn new(config: Arc<RwLock<AppConfig>>) -> Self {
        Self { config }
    }

    /// 获取 HTTP 客户端
    async fn get_client(&self) -> reqwest::Client {
        let config = self.config.read().await;
        let proxy = config.proxy.upstream_proxy.as_deref();
        create_http_client_with_proxy(30, proxy)
    }

    /// 使用 refresh_token 刷新 access_token
    pub async fn refresh_token(&self, refresh_token: &str) -> AppResult<TokenResponse> {
        let client = self.get_client().await;

        let params = [
            ("client_id", CLIENT_ID),
            ("client_secret", CLIENT_SECRET),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ];

        tracing::info!("正在刷新 Token...");

        let response = client
            .post(TOKEN_URL)
            .form(&params)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("刷新请求失败: {}", e)))?;

        if response.status().is_success() {
            let token_data = response
                .json::<TokenResponse>()
                .await
                .map_err(|e| AppError::Internal(format!("Token 解析失败: {}", e)))?;

            tracing::info!("Token 刷新成功！有效期: {} 秒", token_data.expires_in);
            Ok(token_data)
        } else {
            let error_text = response.text().await.unwrap_or_default();
            tracing::error!("Token 刷新失败: {}", error_text);
            Err(AppError::Upstream(format!("刷新失败: {}", error_text)))
        }
    }

    /// 获取用户信息
    pub async fn get_user_info(&self, access_token: &str) -> AppResult<UserInfo> {
        let client = self.get_client().await;

        let response = client
            .get(USERINFO_URL)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("用户信息请求失败: {}", e)))?;

        if response.status().is_success() {
            response
                .json::<UserInfo>()
                .await
                .map_err(|e| AppError::Internal(format!("用户信息解析失败: {}", e)))
        } else {
            let error_text = response.text().await.unwrap_or_default();
            Err(AppError::Upstream(format!("获取用户信息失败: {}", error_text)))
        }
    }
}
