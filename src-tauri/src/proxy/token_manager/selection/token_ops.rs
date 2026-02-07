// Token Operations: Refresh and Project ID

use super::super::manager::TokenManager;
use super::super::models::ProxyToken;
use std::collections::HashSet;

impl TokenManager {
    /// Try to refresh token if needed
    pub(crate) async fn try_refresh_token(
        &self,
        token: &mut ProxyToken,
        attempted: &mut HashSet<String>,
        last_error: &mut Option<String>,
        quota_group: &str,
        last_used_account_id: &Option<(String, std::time::Instant)>,
        need_update_last_used: &mut Option<(String, std::time::Instant)>,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().timestamp();
        if now >= token.timestamp - 300 {
            tracing::debug!("账号 {} 的 token 即将过期，正在刷新...", token.email);

            match crate::modules::oauth::refresh_access_token(&token.refresh_token, Some(&token.account_id)).await {
                Ok(token_response) => {
                    tracing::debug!("Token 刷新成功！");
                    token.access_token = token_response.access_token.clone();
                    token.expires_in = token_response.expires_in;
                    token.timestamp = now + token_response.expires_in;

                    if let Some(mut entry) = self.tokens.get_mut(&token.account_id) {
                        entry.access_token = token.access_token.clone();
                        entry.expires_in = token.expires_in;
                        entry.timestamp = token.timestamp;
                    }

                    if let Err(e) = self
                        .save_refreshed_token(&token.account_id, &token_response)
                        .await
                    {
                        tracing::debug!("保存刷新后的 token 失败 ({}): {}", token.email, e);
                    }
                }
                Err(e) => {
                    tracing::error!("Token 刷新失败 ({}): {}", token.email, e);
                    if e.contains("\"invalid_grant\"") || e.contains("invalid_grant") {
                        tracing::error!(
                            "Disabling account due to invalid_grant ({})",
                            token.email
                        );
                        let _ = self
                            .disable_account(&token.account_id, &format!("invalid_grant: {}", e))
                            .await;
                        self.tokens.remove(&token.account_id);
                    }
                    *last_error = Some(format!("Token refresh failed: {}", e));
                    attempted.insert(token.account_id.clone());

                    if quota_group != "image_gen" {
                        if matches!(last_used_account_id, Some((id, _)) if id == &token.account_id)
                        {
                            *need_update_last_used =
                                Some((String::new(), std::time::Instant::now()));
                        }
                    }
                    return Err("continue".to_string());
                }
            }
        }
        Ok(())
    }

    /// Ensure token has project ID
    pub(crate) async fn ensure_project_id(
        &self,
        token: &mut ProxyToken,
        attempted: &mut HashSet<String>,
        last_error: &mut Option<String>,
        quota_group: &str,
        last_used_account_id: &Option<(String, std::time::Instant)>,
        need_update_last_used: &mut Option<(String, std::time::Instant)>,
    ) -> Result<String, String> {
        if let Some(pid) = &token.project_id {
            return Ok(pid.clone());
        }

        tracing::debug!("账号 {} 缺少 project_id，尝试获取...", token.email);
        match crate::proxy::project_resolver::fetch_project_id(&token.access_token).await {
            Ok(pid) => {
                if let Some(mut entry) = self.tokens.get_mut(&token.account_id) {
                    entry.project_id = Some(pid.clone());
                }
                let _ = self.save_project_id(&token.account_id, &pid).await;
                Ok(pid)
            }
            Err(e) => {
                tracing::error!("Failed to fetch project_id for {}: {}", token.email, e);
                *last_error = Some(format!("Failed to fetch project_id for {}: {}", token.email, e));
                attempted.insert(token.account_id.clone());

                if quota_group != "image_gen" {
                    if matches!(last_used_account_id, Some((id, _)) if id == &token.account_id) {
                        *need_update_last_used = Some((String::new(), std::time::Instant::now()));
                    }
                }
                Err("continue".to_string())
            }
        }
    }
}
