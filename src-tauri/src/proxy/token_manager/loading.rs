// Account Loading and Reloading Logic

use super::manager::TokenManager;
use super::models::ProxyToken;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::Ordering;

impl TokenManager {
    /// Extract earliest reset time from account quota models
    ///
    /// Claude models (sonnet/opus) share the same reset time, so we prioritize claude series.
    /// Returns Unix timestamp (seconds) for sorting comparison.
    fn extract_earliest_reset_time(&self, account: &serde_json::Value) -> Option<i64> {
        let models = account
            .get("quota")
            .and_then(|q| q.get("models"))
            .and_then(|m| m.as_array())?;

        let mut earliest_ts: Option<i64> = None;

        // First pass: prioritize Claude models
        for model in models {
            let model_name = model.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if !model_name.contains("claude") {
                continue;
            }

            if let Some(reset_time_str) = model.get("reset_time").and_then(|r| r.as_str()) {
                if reset_time_str.is_empty() {
                    continue;
                }
                // Parse ISO 8601 time string to timestamp
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(reset_time_str) {
                    let ts = dt.timestamp();
                    if earliest_ts.is_none() || ts < earliest_ts.unwrap() {
                        earliest_ts = Some(ts);
                    }
                }
            }
        }

        // Second pass: if no claude model time found, try any model
        if earliest_ts.is_none() {
            for model in models {
                if let Some(reset_time_str) = model.get("reset_time").and_then(|r| r.as_str()) {
                    if reset_time_str.is_empty() {
                        continue;
                    }
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(reset_time_str) {
                        let ts = dt.timestamp();
                        if earliest_ts.is_none() || ts < earliest_ts.unwrap() {
                            earliest_ts = Some(ts);
                        }
                    }
                }
            }
        }

        earliest_ts
    }

    /// Load all accounts from the accounts directory
    /// [FIX] Only loads accounts that exist in account_index.json to prevent resurrection
    pub async fn load_accounts(&self) -> Result<usize, String> {
        let accounts_dir = self.data_dir.join("accounts");

        if !accounts_dir.exists() {
            return Err(format!("账号目录不存在: {:?}", accounts_dir));
        }

        self.tokens.clear();
        self.current_index.store(0, Ordering::SeqCst);
        {
            let mut last_used = self.last_used_account.lock().await;
            *last_used = None;
        }

        // [FIX] Load valid account IDs from index FIRST to prevent loading deleted accounts
        let valid_ids: HashSet<String> = match crate::modules::account::storage::load_account_index() {
            Ok(index) => index.accounts.iter().map(|s| s.id.clone()).collect(),
            Err(e) => {
                tracing::warn!("Failed to load account index, falling back to directory scan: {}", e);
                HashSet::new()
            }
        };

        let use_index_filter = !valid_ids.is_empty();

        let entries = std::fs::read_dir(&accounts_dir)
            .map_err(|e| format!("读取账号目录失败: {}", e))?;

        let mut count = 0;

        for entry in entries {
            let entry = entry.map_err(|e| format!("读取目录项失败: {}", e))?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            // [FIX] Skip files not in account_index.json (orphaned/deleted accounts)
            if use_index_filter {
                let file_stem = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                
                if !valid_ids.contains(file_stem) {
                    tracing::debug!(
                        "Skipping orphaned account file (not in index): {:?}",
                        path
                    );
                    continue;
                }
            }

            match self.load_single_account(&path).await {
                Ok(Some(token)) => {
                    let account_id = token.account_id.clone();
                    self.tokens.insert(account_id, token);
                    count += 1;
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::debug!("加载账号失败 {:?}: {}", path, e);
                }
            }
        }

        Ok(count)
    }

    /// Reload a specific account
    pub async fn reload_account(&self, account_id: &str) -> Result<(), String> {
        let path = self
            .data_dir
            .join("accounts")
            .join(format!("{}.json", account_id));
        if !path.exists() {
            return Err(format!("账号文件不存在: {:?}", path));
        }

        match self.load_single_account(&path).await {
            Ok(Some(token)) => {
                self.tokens.insert(account_id.to_string(), token);
                self.clear_rate_limit(account_id);
                Ok(())
            }
            Ok(None) => Err("账号加载失败".to_string()),
            Err(e) => Err(format!("同步账号失败: {}", e)),
        }
    }

    /// Reload all accounts
    pub async fn reload_all_accounts(&self) -> Result<usize, String> {
        let count = self.load_accounts().await?;
        self.clear_all_rate_limits();
        Ok(count)
    }

    /// Load a single account from file
    pub(crate) async fn load_single_account(
        &self,
        path: &PathBuf,
    ) -> Result<Option<ProxyToken>, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("读取文件失败: {}", e))?;

        let mut account: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| format!("解析 JSON 失败: {}", e))?;

        // Check disabled status
        if account
            .get("disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            tracing::debug!(
                "Skipping disabled account file: {:?} (email={})",
                path,
                account
                    .get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>")
            );
            return Ok(None);
        }

        // [FIX #1344] Check manual proxy disable BEFORE quota protection
        // This ensures manually disabled accounts (non-quota reasons) are skipped first
        let is_proxy_disabled = account
            .get("proxy_disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let disabled_reason = account
            .get("proxy_disabled_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if is_proxy_disabled && disabled_reason != "quota_protection" {
            // Account was manually disabled (non-quota protection reason)
            tracing::debug!(
                "Account skipped due to manual disable: {:?} (email={}, reason={})",
                path,
                account
                    .get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>"),
                disabled_reason
            );
            return Ok(None);
        }

        // Quota protection check - only handles quota protection logic
        // This allows auto-recovery of accounts whose quota has been restored
        if self.check_and_protect_quota(&mut account, path).await {
            tracing::debug!(
                "Account skipped due to quota protection: {:?} (email={})",
                path,
                account
                    .get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>")
            );
            return Ok(None);
        }

        // [Compatibility] Re-check proxy_disabled after quota protection migration
        // If account was disabled by old quota protection but quota restored, above check clears it
        // This ensures we don't load accounts that are still disabled
        if account
            .get("proxy_disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            tracing::debug!(
                "Skipping proxy-disabled account file: {:?} (email={})",
                path,
                account
                    .get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>")
            );
            return Ok(None);
        }

        // [FIX] Skip forbidden accounts (quota.is_forbidden) to prevent reselection after restart
        if account
            .get("quota")
            .and_then(|q| q.get("is_forbidden"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            tracing::debug!(
                "Skipping forbidden account file: {:?} (email={})",
                path,
                account
                    .get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>")
            );
            return Ok(None);
        }

        // Check validation block
        if account
            .get("validation_blocked")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            let block_until = account
                .get("validation_blocked_until")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            let now = chrono::Utc::now().timestamp();

            if now < block_until {
                tracing::debug!(
                    "Skipping validation-blocked account: {:?} (email={}, blocked until {})",
                    path,
                    account
                        .get("email")
                        .and_then(|v| v.as_str())
                        .unwrap_or("<unknown>"),
                    chrono::DateTime::from_timestamp(block_until, 0)
                        .map(|dt| dt.format("%H:%M:%S").to_string())
                        .unwrap_or_else(|| block_until.to_string())
                );
                return Ok(None);
            } else {
                tracing::info!(
                    "Validation block expired for account: {:?} (email={}), clearing...",
                    path,
                    account
                        .get("email")
                        .and_then(|v| v.as_str())
                        .unwrap_or("<unknown>")
                );
                account["validation_blocked"] = serde_json::Value::Bool(false);
                account["validation_blocked_until"] = serde_json::Value::Null;
                account["validation_blocked_reason"] = serde_json::Value::Null;

                if let Ok(json_str) = serde_json::to_string_pretty(&account) {
                    let _ = std::fs::write(path, json_str);
                }
            }
        }

        // Extract required fields
        let account_id = account["id"]
            .as_str()
            .ok_or("缺少 id 字段")?
            .to_string();

        let email = account["email"]
            .as_str()
            .ok_or("缺少 email 字段")?
            .to_string();

        let token_obj = account["token"].as_object().ok_or("缺少 token 字段")?;

        let access_token = token_obj["access_token"]
            .as_str()
            .ok_or("缺少 access_token")?
            .to_string();

        let refresh_token = token_obj["refresh_token"]
            .as_str()
            .ok_or("缺少 refresh_token")?
            .to_string();

        let expires_in = token_obj["expires_in"]
            .as_i64()
            .ok_or("缺少 expires_in")?;

        let timestamp = token_obj["expiry_timestamp"]
            .as_i64()
            .ok_or("缺少 expiry_timestamp")?;

        let project_id = token_obj
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let subscription_tier = account
            .get("quota")
            .and_then(|q| q.get("subscription_tier"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let remaining_quota = account
            .get("quota")
            .and_then(|q| self.calculate_quota_stats(q));

        let protected_models: HashSet<String> = account
            .get("protected_models")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let mut model_quotas = HashMap::new();
        if let Some(models) = account
            .get("quota")
            .and_then(|q| q.get("models"))
            .and_then(|m| m.as_array())
        {
            for m in models {
                if let (Some(name), Some(pct)) = (
                    m.get("name").and_then(|v| v.as_str()),
                    m.get("percentage").and_then(|v| v.as_i64()),
                ) {
                    model_quotas.insert(name.to_string(), pct as i32);
                }
            }
        }

        let health_score = self
            .health_scores
            .get(&account_id)
            .map(|v| *v)
            .unwrap_or(1.0);

        Ok(Some(ProxyToken {
            account_id,
            access_token,
            refresh_token,
            expires_in,
            timestamp,
            email,
            account_path: path.clone(),
            project_id,
            subscription_tier,
            remaining_quota,
            protected_models,
            health_score,
            model_quotas,
            verification_needed: account
                .get("verification_needed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            verification_url: account
                .get("verification_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            // [FIX] Extract reset_time from quota.models (not from root!)
            reset_time: self.extract_earliest_reset_time(&account),
            validation_blocked: account
                .get("validation_blocked")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            validation_blocked_until: account
                .get("validation_blocked_until")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            is_forbidden: account
                .get("quota")
                .and_then(|q| q.get("is_forbidden"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        }))
    }
}
