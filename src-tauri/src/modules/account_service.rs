use crate::models::{Account, TokenData};
use crate::modules;

/// 账号服务层 - 彻底解除对 Tauri 运行时的依赖
pub struct AccountService {
    pub integration: crate::modules::integration::SystemManager,
}

impl AccountService {
    pub fn new(integration: crate::modules::integration::SystemManager) -> Self {
        Self { integration }
    }

    /// 添加账号逻辑
    pub async fn add_account(&self, refresh_token: &str) -> Result<Account, String> {
        // [FIX #1583] 生成临时 UUID 作为账号上下文，确保代理选择有明确上下文
        let temp_account_id = uuid::Uuid::new_v4().to_string();
        
        // 1. 获取 Token (使用临时 ID)
        let token_res = modules::oauth::refresh_access_token(refresh_token, Some(&temp_account_id)).await?;

        // 2. 获取用户信息
        let user_info = modules::oauth::get_user_info(&token_res.access_token, Some(&temp_account_id)).await?;

        // 3. 获取项目 ID (尝试)
        let project_id = crate::proxy::project_resolver::fetch_project_id(&token_res.access_token)
            .await
            .ok();

        // 4. 构造 TokenData
        let token = TokenData::new(
            token_res.access_token,
            refresh_token.to_string(),
            token_res.expires_in,
            Some(user_info.email.clone()),
            project_id,
            None,
        );

        // 5. 持久化
        let account = modules::upsert_account(
            user_info.email.clone(),
            user_info.get_display_name(),
            token
        )?;

        modules::logger::log_info(&format!("[Service] Added/Updated account: {}", account.email));
        Ok(account)
    }

    /// 删除账号逻辑
    pub fn delete_account(&self, account_id: &str) -> Result<(), String> {
        modules::delete_account(account_id)?;
        self.integration.update_tray();
        Ok(())
    }

    /// 切换账号逻辑
    pub async fn switch_account(&self, account_id: &str) -> Result<(), String> {
        modules::account::switch_account(account_id, &self.integration).await
    }

    /// 列表获取
    pub async fn list_accounts(&self) -> Result<Vec<Account>, String> {
        modules::list_accounts().await
    }

    /// 获取当前 ID
    pub fn get_current_id(&self) -> Result<Option<String>, String> {
        modules::get_current_account_id()
    }

    // --- OAuth 逻辑 ---

    pub async fn prepare_oauth_url(&self) -> Result<String, String> {
        let handle = match &self.integration {
            modules::integration::SystemManager::Desktop(h) => Some(h.clone()),
            modules::integration::SystemManager::Headless => None,
        };
        modules::oauth_server::prepare_oauth_url(handle).await
    }

    pub async fn start_oauth_login(&self) -> Result<Account, String> {
        let handle = match &self.integration {
            modules::integration::SystemManager::Desktop(h) => Some(h.clone()),
            modules::integration::SystemManager::Headless => None,
        };
        let token_res = modules::oauth_server::start_oauth_flow(handle).await?;
        self.process_oauth_token(token_res).await
    }

    pub async fn complete_oauth_login(&self) -> Result<Account, String> {
        let handle = match &self.integration {
            modules::integration::SystemManager::Desktop(h) => Some(h.clone()),
            modules::integration::SystemManager::Headless => None,
        };
        let token_res = modules::oauth_server::complete_oauth_flow(handle).await?;
        self.process_oauth_token(token_res).await
    }

    pub fn cancel_oauth_login(&self) {
        modules::oauth_server::cancel_oauth_flow();
    }

    pub async fn submit_oauth_code(&self, code: String, state: Option<String>) -> Result<(), String> {
        modules::oauth_server::submit_oauth_code(code, state).await
    }

    async fn process_oauth_token(&self, token_res: modules::oauth::TokenResponse) -> Result<Account, String> {
        let refresh_token = token_res.refresh_token.ok_or_else(|| {
            "未获取到 Refresh Token。请撤销权限后重试。".to_string()
        })?;

        // [FIX #1583] 生成临时 UUID 作为账号上下文
        let temp_account_id = uuid::Uuid::new_v4().to_string();
        
        let user_info = modules::oauth::get_user_info(&token_res.access_token, Some(&temp_account_id)).await?;
        let project_id = crate::proxy::project_resolver::fetch_project_id(&token_res.access_token)
            .await
            .ok();

        let token_data = crate::models::TokenData::new(
            token_res.access_token,
            refresh_token,
            token_res.expires_in,
            Some(user_info.email.clone()),
            project_id,
            None,
        );

        let account = modules::upsert_account(
            user_info.email.clone(),
            user_info.get_display_name(),
            token_data,
        )?;

        // 发送 UI 更新通知 (通过 integration)
        self.integration.update_tray();

        Ok(account)
    }
}
