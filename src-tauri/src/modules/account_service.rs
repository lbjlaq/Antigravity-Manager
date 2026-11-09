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
        // [FIX #1583] 生成临时 UUID 作为账号上下文，避免传递 None 导致代理选择异常
        let temp_account_id = uuid::Uuid::new_v4().to_string();

        // 1. 获取 Token (使用临时 ID 确保代理选择有明确上下文)
        let token_res =
            modules::oauth::refresh_access_token(refresh_token, Some(&temp_account_id)).await?;

        // 2. 获取用户信息
        let user_info =
            modules::oauth::get_user_info(&token_res.access_token, Some(&temp_account_id)).await?;

        // 3. 获取项目 ID (尝试)
        let project_id = crate::proxy::project_resolver::fetch_project_id(&token_res.access_token)
            .await
            .ok();

        // 4. 构造 TokenData
        let token = TokenData::new(
            token_res.access_token.clone(),
            refresh_token.to_string(),
            token_res.expires_in,
            Some(user_info.email.clone()),
            project_id,
            None,
            false, // 个人账号默认不开启 GCP TOS
            token_res.id_token.clone(),
        )
        .with_oauth_client_key(token_res.oauth_client_key.clone());

        // 5. 持久化
        let mut account =
            modules::upsert_account(user_info.email.clone(), user_info.get_display_name(), token)?;

        // 5.1 统一刷新入口：添加新账号时强制执行一次数据面封号探针；
        // 封号直接落盘并跳过配额，未封号才拉取配额。
        match modules::account::fetch_quota_with_retry_forced(&mut account).await {
            Ok(quota) => {
                if let Err(e) = modules::update_account_quota(&account.id, quota) {
                    modules::logger::log_warn(&format!(
                        "[Service] Failed to persist quota for {}: {}",
                        account.email, e
                    ));
                }
                if let Ok(reloaded) = modules::account::load_account(&account.id) {
                    account = reloaded;
                }
            }
            Err(e) => {
                modules::logger::log_warn(&format!(
                    "[Service] Unified quota refresh failed for {}: {}",
                    account.email, e
                ));
            }
        }

        modules::logger::log_info(&format!(
            "[Service] Added/Updated account: {}",
            account.email
        ));
        Ok(account)
    }

    /// 删除账号逻辑
    pub fn delete_account(&self, account_id: &str) -> Result<(), String> {
        modules::delete_account(account_id)?;
        self.integration.update_tray();
        Ok(())
    }

    /// 切换账号逻辑
    pub async fn switch_account(
        &self,
        account_id: &str,
        target_ide: Option<&str>,
    ) -> Result<(), String> {
        modules::account::switch_account(account_id, target_ide, &self.integration).await
    }

    /// 列表获取
    pub fn list_accounts(&self) -> Result<Vec<Account>, String> {
        modules::list_accounts()
    }

    /// 获取当前 ID
    pub fn get_current_id(&self) -> Result<Option<String>, String> {
        modules::get_current_account_id()
    }

    // --- OAuth 逻辑 ---

    pub async fn prepare_oauth_url(
        &self,
        oauth_client_key: Option<String>,
    ) -> Result<String, String> {
        let handle = match &self.integration {
            modules::integration::SystemManager::Desktop(h) => Some(h.clone()),
            _ => None,
        };
        modules::oauth_server::prepare_oauth_url(handle, oauth_client_key).await
    }

    pub async fn start_oauth_login(
        &self,
        oauth_client_key: Option<String>,
    ) -> Result<Account, String> {
        let handle = match &self.integration {
            modules::integration::SystemManager::Desktop(h) => Some(h.clone()),
            _ => None,
        };
        let token_res = modules::oauth_server::start_oauth_flow(handle, oauth_client_key).await?;
        self.process_oauth_token(token_res).await
    }

    pub async fn complete_oauth_login(&self) -> Result<Account, String> {
        let handle = match &self.integration {
            modules::integration::SystemManager::Desktop(h) => Some(h.clone()),
            _ => None,
        };
        let token_res = modules::oauth_server::complete_oauth_flow(handle).await?;
        self.process_oauth_token(token_res).await
    }

    pub fn cancel_oauth_login(&self) {
        modules::oauth_server::cancel_oauth_flow();
    }

    pub async fn submit_oauth_code(
        &self,
        code: String,
        state: Option<String>,
    ) -> Result<Account, String> {
        let token_res = modules::oauth_server::submit_oauth_code(code, state).await?;
        self.process_oauth_token(token_res).await
    }

    pub async fn process_oauth_token(
        &self,
        token_res: modules::oauth::TokenResponse,
    ) -> Result<Account, String> {
        let refresh_token = token_res
            .refresh_token
            .ok_or_else(|| "未获取到 Refresh Token。请撤销权限后重试。".to_string())?;

        // [FIX #1583] 生成临时 UUID 作为账号上下文
        let temp_account_id = uuid::Uuid::new_v4().to_string();

        let user_info =
            modules::oauth::get_user_info(&token_res.access_token, Some(&temp_account_id)).await?;
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
            false, // 默认不开启，由后续逻辑或用户手动调整
            token_res.id_token,
        )
        .with_oauth_client_key(token_res.oauth_client_key.clone());

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
