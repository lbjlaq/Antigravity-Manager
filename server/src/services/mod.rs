//! 服务层模块

mod account_service;
mod oauth_service;
mod token_manager;
mod http_client;

pub use account_service::AccountService;
pub use oauth_service::OAuthService;
pub use token_manager::TokenManager;
pub use http_client::create_http_client;

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::error::AppError;

/// 应用状态 (共享)
pub struct AppState {
    /// 配置
    pub config: Arc<RwLock<AppConfig>>,
    
    /// 账号服务
    pub account_service: Arc<AccountService>,
    
    /// OAuth 服务
    pub oauth_service: Arc<OAuthService>,
    
    /// Token 管理器 (用于反代)
    pub token_manager: Arc<TokenManager>,
}

impl AppState {
    /// 创建新的应用状态
    pub async fn new(config: AppConfig) -> Result<Self, AppError> {
        let config = Arc::new(RwLock::new(config.clone()));
        
        // 创建账号服务
        let account_service = Arc::new(
            AccountService::new(config.clone()).await?
        );
        
        // 创建 OAuth 服务
        let oauth_service = Arc::new(OAuthService::new(config.clone()));
        
        // 创建 Token 管理器
        let token_manager = Arc::new(
            TokenManager::new(
                config.clone(),
                account_service.clone(),
            ).await?
        );

        Ok(Self {
            config,
            account_service,
            oauth_service,
            token_manager,
        })
    }

    /// 重新加载账号
    pub async fn reload_accounts(&self) -> Result<usize, AppError> {
        self.account_service.reload().await?;
        self.token_manager.reload().await
    }
}
