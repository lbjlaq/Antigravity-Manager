// Token Persistence Logic

use super::manager::{truncate_reason, TokenManager};

impl TokenManager {
    /// Save project ID to account file
    pub(crate) async fn save_project_id(
        &self,
        account_id: &str,
        project_id: &str,
    ) -> Result<(), String> {
        let entry = self.tokens.get(account_id).ok_or("账号不存在")?;

        let path = &entry.account_path;

        let mut content: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(path).map_err(|e| format!("读取文件失败: {}", e))?,
        )
        .map_err(|e| format!("解析 JSON 失败: {}", e))?;

        content["token"]["project_id"] = serde_json::Value::String(project_id.to_string());

        let json_str = serde_json::to_string_pretty(&content)
            .map_err(|e| format!("序列化 JSON 失败: {}", e))?;

        std::fs::write(path, json_str).map_err(|e| format!("写入文件失败: {}", e))?;

        tracing::debug!("已保存 project_id 到账号 {}", account_id);
        Ok(())
    }

    /// Save refreshed token to account file
    pub(crate) async fn save_refreshed_token(
        &self,
        account_id: &str,
        token_response: &crate::modules::oauth::TokenResponse,
    ) -> Result<(), String> {
        let entry = self.tokens.get(account_id).ok_or("账号不存在")?;

        let path = &entry.account_path;

        let mut content: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(path).map_err(|e| format!("读取文件失败: {}", e))?,
        )
        .map_err(|e| format!("解析 JSON 失败: {}", e))?;

        let now = chrono::Utc::now().timestamp();

        content["token"]["access_token"] =
            serde_json::Value::String(token_response.access_token.clone());
        content["token"]["expires_in"] =
            serde_json::Value::Number(token_response.expires_in.into());
        content["token"]["expiry_timestamp"] =
            serde_json::Value::Number((now + token_response.expires_in).into());

        let json_str = serde_json::to_string_pretty(&content)
            .map_err(|e| format!("序列化 JSON 失败: {}", e))?;

        std::fs::write(path, json_str).map_err(|e| format!("写入文件失败: {}", e))?;

        tracing::debug!("已保存刷新后的 token 到账号 {}", account_id);
        Ok(())
    }

    /// Disable an account
    pub(crate) async fn disable_account(
        &self,
        account_id: &str,
        reason: &str,
    ) -> Result<(), String> {
        let path = if let Some(entry) = self.tokens.get(account_id) {
            entry.account_path.clone()
        } else {
            self.data_dir
                .join("accounts")
                .join(format!("{}.json", account_id))
        };

        let mut content: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&path).map_err(|e| format!("读取文件失败: {}", e))?,
        )
        .map_err(|e| format!("解析 JSON 失败: {}", e))?;

        let now = chrono::Utc::now().timestamp();
        content["disabled"] = serde_json::Value::Bool(true);
        content["disabled_at"] = serde_json::Value::Number(now.into());
        content["disabled_reason"] = serde_json::Value::String(truncate_reason(reason, 800));

        let json_str = serde_json::to_string_pretty(&content)
            .map_err(|e| format!("序列化 JSON 失败: {}", e))?;

        std::fs::write(&path, json_str).map_err(|e| format!("写入文件失败: {}", e))?;

        self.tokens.remove(account_id);

        tracing::warn!("Account disabled: {} ({:?})", account_id, path);
        Ok(())
    }

    /// Set validation block for an account
    pub async fn set_validation_block_public(
        &self,
        account_id: &str,
        block_until: i64,
        reason: &str,
    ) -> Result<(), String> {
        self.set_validation_block(account_id, block_until, reason)
            .await
    }

    /// Internal validation block setter
    async fn set_validation_block(
        &self,
        account_id: &str,
        block_until: i64,
        reason: &str,
    ) -> Result<(), String> {
        let path = if let Some(entry) = self.tokens.get(account_id) {
            entry.account_path.clone()
        } else {
            self.data_dir
                .join("accounts")
                .join(format!("{}.json", account_id))
        };

        let mut content: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&path).map_err(|e| format!("读取文件失败: {}", e))?,
        )
        .map_err(|e| format!("解析 JSON 失败: {}", e))?;

        content["validation_blocked"] = serde_json::Value::Bool(true);
        content["validation_blocked_until"] = serde_json::Value::Number(block_until.into());
        content["validation_blocked_reason"] =
            serde_json::Value::String(truncate_reason(reason, 500));

        let json_str = serde_json::to_string_pretty(&content)
            .map_err(|e| format!("序列化 JSON 失败: {}", e))?;

        std::fs::write(&path, json_str).map_err(|e| format!("写入文件失败: {}", e))?;

        self.tokens.remove(account_id);

        tracing::warn!(
            "Account validation blocked until {}: {} ({:?})",
            chrono::DateTime::from_timestamp(block_until, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| block_until.to_string()),
            account_id,
            path
        );
        Ok(())
    }

    /// Get token by email (for warmup scenarios)
    pub async fn get_token_by_email(
        &self,
        email: &str,
    ) -> Result<(String, String, String, u64), String> {
        let token_info = {
            let mut found = None;
            for entry in self.tokens.iter() {
                let token = entry.value();
                if token.email == email {
                    found = Some((
                        token.account_id.clone(),
                        token.access_token.clone(),
                        token.refresh_token.clone(),
                        token.timestamp,
                        token.expires_in,
                        chrono::Utc::now().timestamp(),
                        token.project_id.clone(),
                    ));
                    break;
                }
            }
            found
        };

        let (account_id, current_access_token, refresh_token, timestamp, expires_in, now, project_id_opt) =
            match token_info {
                Some(info) => info,
                None => return Err(format!("未找到账号: {}", email)),
            };

        let project_id = project_id_opt.unwrap_or_else(|| "bamboo-precept-lgxtn".to_string());

        if now < timestamp + expires_in - 300 {
            return Ok((current_access_token, project_id, email.to_string(), 0));
        }

        tracing::info!("[Warmup] Token for {} is expiring, refreshing...", email);

        match crate::modules::oauth::refresh_access_token(&refresh_token).await {
            Ok(token_response) => {
                tracing::info!("[Warmup] Token refresh successful for {}", email);
                let new_now = chrono::Utc::now().timestamp();

                if let Some(mut entry) = self.tokens.get_mut(&account_id) {
                    entry.access_token = token_response.access_token.clone();
                    entry.expires_in = token_response.expires_in;
                    entry.timestamp = new_now;
                }

                let _ = self
                    .save_refreshed_token(&account_id, &token_response)
                    .await;

                Ok((
                    token_response.access_token,
                    project_id,
                    email.to_string(),
                    0,
                ))
            }
            Err(e) => Err(format!(
                "[Warmup] Token refresh failed for {}: {}",
                email, e
            )),
        }
    }

    /// Add a new account
    pub async fn add_account(&self, email: &str, refresh_token: &str) -> Result<(), String> {
        let token_info = crate::modules::oauth::refresh_access_token(refresh_token)
            .await
            .map_err(|e| format!("Invalid refresh token: {}", e))?;

        let project_id =
            crate::proxy::project_resolver::fetch_project_id(&token_info.access_token)
                .await
                .unwrap_or_else(|_| "bamboo-precept-lgxtn".to_string());

        let email_clone = email.to_string();
        let refresh_token_clone = refresh_token.to_string();

        tokio::task::spawn_blocking(move || {
            let token_data = crate::models::TokenData::new(
                token_info.access_token,
                refresh_token_clone,
                token_info.expires_in,
                Some(email_clone.clone()),
                Some(project_id),
                None,
            );

            crate::modules::account::upsert_account(email_clone, None, token_data)
        })
        .await
        .map_err(|e| format!("Task join error: {}", e))?
        .map_err(|e| format!("Failed to save account: {}", e))?;

        self.reload_all_accounts().await.map(|_| ())
    }

    /// Exchange OAuth code for refresh token
    pub async fn exchange_code(&self, code: &str, redirect_uri: &str) -> Result<String, String> {
        crate::modules::oauth::exchange_code(code, redirect_uri)
            .await
            .and_then(|t| {
                t.refresh_token
                    .ok_or_else(|| "No refresh token returned by Google".to_string())
            })
    }

    /// Get OAuth URL with custom redirect
    pub fn get_oauth_url_with_redirect(&self, redirect_uri: &str, state: &str) -> String {
        crate::modules::oauth::get_auth_url(redirect_uri, state)
    }

    /// Get user info from refresh token
    pub async fn get_user_info(
        &self,
        refresh_token: &str,
    ) -> Result<crate::modules::oauth::UserInfo, String> {
        let token = crate::modules::oauth::refresh_access_token(refresh_token)
            .await
            .map_err(|e| format!("刷新 Access Token 失败: {}", e))?;

        crate::modules::oauth::get_user_info(&token.access_token).await
    }
}
