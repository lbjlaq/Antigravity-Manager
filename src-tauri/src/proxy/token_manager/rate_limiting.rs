// Rate Limiting Logic

use super::manager::TokenManager;

impl TokenManager {
    /// Mark account as rate limited (sync version)
    pub async fn mark_rate_limited(
        &self,
        email: &str,
        status: u16,
        retry_after_header: Option<&str>,
        error_body: &str,
    ) {
        let config = self.circuit_breaker_config.read().await.clone();
        if !config.enabled {
            return;
        }

        let key = self
            .email_to_account_id(email)
            .unwrap_or_else(|| email.to_string());

        self.rate_limit_tracker.parse_from_error(
            &key,
            status,
            retry_after_header,
            error_body,
            None,
            &config.backoff_steps,
        );
    }

    /// Mark account as rate limited (async version with real-time quota refresh)
    pub async fn mark_rate_limited_async(
        &self,
        email: &str,
        status: u16,
        retry_after_header: Option<&str>,
        error_body: &str,
        model: Option<&str>,
    ) {
        let config = self.circuit_breaker_config.read().await.clone();
        if !config.enabled {
            return;
        }

        let account_id = self
            .email_to_account_id(email)
            .unwrap_or_else(|| email.to_string());

        let has_explicit_retry_time =
            retry_after_header.is_some() || error_body.contains("quotaResetDelay");

        if has_explicit_retry_time {
            if let Some(m) = model {
                tracing::debug!(
                    "账号 {} 的模型 {} 的 429 响应包含 quotaResetDelay",
                    account_id,
                    m
                );
            }
            self.rate_limit_tracker.parse_from_error(
                &account_id,
                status,
                retry_after_header,
                error_body,
                model.map(|s| s.to_string()),
                &config.backoff_steps,
            );
            return;
        }

        let reason = if error_body.to_lowercase().contains("model_capacity") {
            crate::proxy::rate_limit::RateLimitReason::ModelCapacityExhausted
        } else if error_body.to_lowercase().contains("exhausted")
            || error_body.to_lowercase().contains("quota")
        {
            crate::proxy::rate_limit::RateLimitReason::QuotaExhausted
        } else {
            crate::proxy::rate_limit::RateLimitReason::Unknown
        };

        if let Some(m) = model {
            tracing::info!(
                "账号 {} 的模型 {} 的 429 响应未包含 quotaResetDelay，尝试实时刷新配额...",
                account_id,
                m
            );
        }

        if self
            .fetch_and_lock_with_realtime_quota(&account_id, reason, model.map(|s| s.to_string()))
            .await
        {
            tracing::info!("账号 {} 已使用实时配额精确锁定", account_id);
            return;
        }

        if self.set_precise_lockout(&account_id, reason, model.map(|s| s.to_string())) {
            tracing::info!("账号 {} 已使用本地缓存配额锁定", account_id);
            return;
        }

        tracing::warn!("账号 {} 无法获取配额刷新时间，使用指数退避策略", account_id);
        self.rate_limit_tracker.parse_from_error(
            &account_id,
            status,
            retry_after_header,
            error_body,
            model.map(|s| s.to_string()),
            &config.backoff_steps,
        );
    }

    /// Check if account is rate limited (async)
    pub async fn is_rate_limited(&self, account_id: &str, model: Option<&str>) -> bool {
        let config = self.circuit_breaker_config.read().await;
        if !config.enabled {
            return false;
        }
        self.rate_limit_tracker.is_rate_limited(account_id, model)
    }

    /// Check if account is rate limited (sync version for iterators)
    pub fn is_rate_limited_sync(&self, account_id: &str, model: Option<&str>) -> bool {
        let config = self.circuit_breaker_config.blocking_read();
        if !config.enabled {
            return false;
        }
        self.rate_limit_tracker.is_rate_limited(account_id, model)
    }

    /// Get remaining wait time for rate limit reset
    #[allow(dead_code)]
    pub fn get_rate_limit_reset_seconds(&self, account_id: &str) -> Option<u64> {
        self.rate_limit_tracker.get_reset_seconds(account_id)
    }

    /// Clean expired rate limit records
    #[allow(dead_code)]
    pub fn clean_expired_rate_limits(&self) {
        self.rate_limit_tracker.cleanup_expired();
    }

    /// Clear rate limit for specific account
    pub fn clear_rate_limit(&self, account_id: &str) -> bool {
        self.rate_limit_tracker.clear(account_id)
    }

    /// Clear all rate limits
    pub fn clear_all_rate_limits(&self) {
        self.rate_limit_tracker.clear_all();
    }

    /// Mark account request as successful
    pub fn mark_account_success(&self, email: &str, model: Option<&str>) {
        if let Some(account_id) = self.email_to_account_id(email) {
            self.rate_limit_tracker.mark_success(&account_id, model);
        } else {
            self.rate_limit_tracker.mark_success(email, model);
        }
    }

    /// Get quota reset time from account file
    pub fn get_quota_reset_time(&self, email: &str) -> Option<String> {
        let accounts_dir = self.data_dir.join("accounts");

        if let Ok(entries) = std::fs::read_dir(&accounts_dir) {
            for entry in entries.flatten() {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if let Ok(account) = serde_json::from_str::<serde_json::Value>(&content) {
                        if account.get("email").and_then(|e| e.as_str()) == Some(email) {
                            if let Some(models) = account
                                .get("quota")
                                .and_then(|q| q.get("models"))
                                .and_then(|m| m.as_array())
                            {
                                let mut earliest_reset: Option<&str> = None;
                                for model in models {
                                    if let Some(reset_time) =
                                        model.get("reset_time").and_then(|r| r.as_str())
                                    {
                                        if !reset_time.is_empty() {
                                            match earliest_reset {
                                                Some(current_min) => {
                                                    if reset_time < current_min {
                                                        earliest_reset = Some(reset_time);
                                                    }
                                                }
                                                None => {
                                                    earliest_reset = Some(reset_time);
                                                }
                                            }
                                        }
                                    }
                                }
                                if let Some(reset) = earliest_reset {
                                    return Some(reset.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Set precise lockout using quota reset time
    pub fn set_precise_lockout(
        &self,
        email: &str,
        reason: crate::proxy::rate_limit::RateLimitReason,
        model: Option<String>,
    ) -> bool {
        if let Some(reset_time_str) = self.get_quota_reset_time(email) {
            tracing::info!("找到账号 {} 的配额刷新时间: {}", email, reset_time_str);
            self.rate_limit_tracker
                .set_lockout_until_iso(email, &reset_time_str, reason, model)
        } else {
            tracing::debug!("未找到账号 {} 的配额刷新时间", email);
            false
        }
    }

    /// Fetch and lock with real-time quota refresh
    pub async fn fetch_and_lock_with_realtime_quota(
        &self,
        email: &str,
        reason: crate::proxy::rate_limit::RateLimitReason,
        model: Option<String>,
    ) -> bool {
        let access_token = {
            let mut found_token: Option<String> = None;
            for entry in self.tokens.iter() {
                if entry.value().email == email {
                    found_token = Some(entry.value().access_token.clone());
                    break;
                }
            }
            found_token
        };

        let access_token = match access_token {
            Some(t) => t,
            None => {
                tracing::warn!("无法找到账号 {} 的 access_token", email);
                return false;
            }
        };

        tracing::info!("账号 {} 正在实时刷新配额...", email);
        match crate::modules::quota::fetch_quota(&access_token, email).await {
            Ok((quota_data, _project_id)) => {
                let earliest_reset = quota_data
                    .models
                    .iter()
                    .filter_map(|m| {
                        if !m.reset_time.is_empty() {
                            Some(m.reset_time.as_str())
                        } else {
                            None
                        }
                    })
                    .min();

                if let Some(reset_time_str) = earliest_reset {
                    tracing::info!(
                        "账号 {} 实时配额刷新成功，reset_time: {}",
                        email,
                        reset_time_str
                    );
                    self.rate_limit_tracker
                        .set_lockout_until_iso(email, reset_time_str, reason, model)
                } else {
                    tracing::warn!("账号 {} 配额刷新成功但未找到 reset_time", email);
                    false
                }
            }
            Err(e) => {
                tracing::warn!("账号 {} 实时配额刷新失败: {:?}", email, e);
                false
            }
        }
    }
}
