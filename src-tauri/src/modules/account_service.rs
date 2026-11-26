use crate::models::{Account, TokenData};
use crate::modules;
use crate::proxy::proxy_pool::AddAccountProxyOptions;

/// 账号服务层 - 彻底解除对 Tauri 运行时的依赖
pub struct AccountService {
    pub integration: crate::modules::integration::SystemManager,
}

impl AccountService {
    pub fn new(integration: crate::modules::integration::SystemManager) -> Self {
        Self { integration }
    }

    async fn apply_initial_proxy_binding(
        account_id: &str,
        proxy_options: &AddAccountProxyOptions,
    ) -> Result<(), String> {
        let Some(ref proxy_id) = proxy_options.proxy_id else {
            return Ok(());
        };
        let strict = if proxy_options.strict_proxy {
            true
        } else if let Some(pool) = crate::proxy::proxy_pool::get_global_proxy_pool() {
            pool.is_strict_mode().await
        } else {
            crate::modules::config::load_app_config()
                .map(|c| {
                    crate::proxy::config::effective_proxy_pool_strict_mode(&c.proxy.proxy_pool)
                })
                .unwrap_or(false)
        };
        if let Err(e) = crate::proxy::proxy_pool::bind_account_proxy_persistent(
            account_id.to_string(),
            proxy_id.clone(),
        )
        .await
        {
            if strict {
                return Err(format!(
                    "[Strict Proxy] 账号已添加但自动绑定代理节点 '{}' 失败: {}",
                    proxy_id, e
                ));
            }
            modules::logger::log_warn(&format!(
                "[Service] Failed to auto-bind proxy {} for new account {}: {}",
                proxy_id, account_id, e
            ));
        }
        Ok(())
    }

    /// 添加账号逻辑
    pub async fn add_account(&self, refresh_token: &str) -> Result<Account, String> {
        self.add_account_with_options(refresh_token, AddAccountProxyOptions::default())
            .await
    }

    /// 添加账号逻辑（支持预选代理节点与严格代理模式，添加后自动绑定）
    pub async fn add_account_with_options(
        &self,
        refresh_token: &str,
        proxy_options: AddAccountProxyOptions,
    ) -> Result<Account, String> {
        let mut normalized_options = proxy_options.normalized();
        let validated_proxy_id =
            crate::proxy::proxy_pool::validate_add_account_proxy_options(&normalized_options)
                .await?;
        normalized_options.proxy_id = validated_proxy_id;

        // 1. 获取 Token（严格遵守预选代理与严格模式）
        let token_res = modules::oauth::refresh_access_token_with_options(
            refresh_token,
            None,
            None,
            Some(&normalized_options),
        )
        .await?;

        // 2. 获取用户信息
        let user_info = modules::oauth::get_user_info_with_options(
            &token_res.access_token,
            None,
            Some(&normalized_options),
        )
        .await?;

        // 3. 获取项目 ID (尝试)
        let project_id = crate::proxy::project_resolver::fetch_project_id_with_options(
            &token_res.access_token,
            None,
            Some(&normalized_options),
        )
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

        // 5.0 立即绑定账号到预选代理节点，使后续封号探针/配额刷新也通过同一出口执行
        Self::apply_initial_proxy_binding(&account.id, &normalized_options).await?;

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
        self.prepare_oauth_url_with_options(oauth_client_key, AddAccountProxyOptions::default())
            .await
    }

    pub async fn prepare_oauth_url_with_options(
        &self,
        oauth_client_key: Option<String>,
        proxy_options: AddAccountProxyOptions,
    ) -> Result<String, String> {
        let handle = match &self.integration {
            modules::integration::SystemManager::Desktop(h) => Some(h.clone()),
            _ => None,
        };
        modules::oauth_server::prepare_oauth_url_with_options(
            handle,
            oauth_client_key,
            proxy_options,
        )
        .await
    }

    pub async fn start_oauth_login(
        &self,
        oauth_client_key: Option<String>,
    ) -> Result<Account, String> {
        self.start_oauth_login_with_options(oauth_client_key, AddAccountProxyOptions::default())
            .await
    }

    pub async fn start_oauth_login_with_options(
        &self,
        oauth_client_key: Option<String>,
        proxy_options: AddAccountProxyOptions,
    ) -> Result<Account, String> {
        let handle = match &self.integration {
            modules::integration::SystemManager::Desktop(h) => Some(h.clone()),
            _ => None,
        };
        let (token_res, effective_proxy_options) =
            modules::oauth_server::start_oauth_flow_with_options(
                handle,
                oauth_client_key,
                proxy_options,
            )
            .await?;
        self.process_oauth_token_with_options(token_res, effective_proxy_options)
            .await
    }

    pub async fn complete_oauth_login(&self) -> Result<Account, String> {
        let handle = match &self.integration {
            modules::integration::SystemManager::Desktop(h) => Some(h.clone()),
            _ => None,
        };
        let (token_res, effective_proxy_options) =
            modules::oauth_server::complete_oauth_flow_with_options(handle).await?;
        self.process_oauth_token_with_options(token_res, effective_proxy_options)
            .await
    }

    pub fn cancel_oauth_login(&self) {
        modules::oauth_server::cancel_oauth_flow();
    }

    pub async fn submit_oauth_code(
        &self,
        code: String,
        state: Option<String>,
    ) -> Result<Account, String> {
        self.submit_oauth_code_with_options(code, state, None).await
    }

    pub async fn submit_oauth_code_with_options(
        &self,
        code: String,
        state: Option<String>,
        proxy_options: Option<AddAccountProxyOptions>,
    ) -> Result<Account, String> {
        let (token_res, effective_proxy_options) =
            modules::oauth_server::submit_oauth_code_with_options(code, state, proxy_options)
                .await?;
        self.process_oauth_token_with_options(token_res, effective_proxy_options)
            .await
    }

    pub async fn process_oauth_token(
        &self,
        token_res: modules::oauth::TokenResponse,
    ) -> Result<Account, String> {
        self.process_oauth_token_with_options(token_res, AddAccountProxyOptions::default())
            .await
    }

    pub async fn process_oauth_token_with_options(
        &self,
        token_res: modules::oauth::TokenResponse,
        proxy_options: AddAccountProxyOptions,
    ) -> Result<Account, String> {
        let mut normalized_options = proxy_options.normalized();
        let validated_proxy_id =
            crate::proxy::proxy_pool::validate_add_account_proxy_options(&normalized_options)
                .await?;
        normalized_options.proxy_id = validated_proxy_id;

        let refresh_token = token_res
            .refresh_token
            .ok_or_else(|| "未获取到 Refresh Token。请撤销权限后重试。".to_string())?;

        let user_info = modules::oauth::get_user_info_with_options(
            &token_res.access_token,
            None,
            Some(&normalized_options),
        )
        .await?;
        let project_id = crate::proxy::project_resolver::fetch_project_id_with_options(
            &token_res.access_token,
            None,
            Some(&normalized_options),
        )
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

        Self::apply_initial_proxy_binding(&account.id, &normalized_options).await?;

        // 发送 UI 更新通知 (通过 integration)
        self.integration.update_tray();

        Ok(account)
    }
}
