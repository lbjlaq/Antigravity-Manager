// Quota Protection Logic

use super::manager::TokenManager;
use std::path::PathBuf;

impl TokenManager {
    /// [FIX] Check if account exists in index by extracting ID from path
    fn account_exists_by_path(account_path: &PathBuf) -> bool {
        let account_id = account_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        
        if account_id.is_empty() {
            return false;
        }
        
        match crate::modules::account::storage::load_account_index() {
            Ok(index) => index.accounts.iter().any(|s| s.id == account_id),
            Err(_) => false,
        }
    }

    /// Check if account should be quota protected
    pub(crate) async fn check_and_protect_quota(
        &self,
        account_json: &mut serde_json::Value,
        account_path: &PathBuf,
    ) -> bool {
        // [FIX] Check if account exists in index before any operations
        if !Self::account_exists_by_path(account_path) {
            tracing::warn!("check_and_protect_quota: Account {:?} not in index, skipping", account_path);
            return false;
        }

        let config = match crate::modules::config::load_app_config() {
            Ok(cfg) => cfg.quota_protection,
            Err(_) => return false,
        };

        if !config.enabled {
            return false;
        }

        let quota = match account_json.get("quota") {
            Some(q) => q.clone(),
            None => return false,
        };

        let is_proxy_disabled = account_json
            .get("proxy_disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let reason = account_json
            .get("proxy_disabled_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if is_proxy_disabled {
            if reason == "quota_protection" {
                return self
                    .check_and_restore_quota(account_json, account_path, &quota, &config)
                    .await;
            }
            return true;
        }

        let models = match quota.get("models").and_then(|m| m.as_array()) {
            Some(m) => m,
            None => return false,
        };

        let threshold = config.threshold_percentage as i32;
        let mut changed = false;

        for model in models {
            let name = model.get("name").and_then(|v| v.as_str()).unwrap_or("");
            // [FIX] Normalize model name to standard ID for proper matching
            let standard_id = crate::proxy::common::model_mapping::normalize_to_standard_id(name)
                .unwrap_or_else(|| name.to_string());
            if !config.monitored_models.iter().any(|m| m == &standard_id) {
                continue;
            }

            let percentage = model
                .get("percentage")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;
            let account_id = account_json
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            if percentage <= threshold {
                // [FIX] Pass normalized standard_id instead of raw name for consistency
                if self
                    .trigger_quota_protection(
                        account_json,
                        &account_id,
                        account_path,
                        percentage,
                        threshold,
                        &standard_id,
                    )
                    .await
                    .unwrap_or(false)
                {
                    changed = true;
                }
            } else {
                // [FIX] Use normalized standard_id for consistency with trigger
                let protected_models = account_json
                    .get("protected_models")
                    .and_then(|v| v.as_array());
                let is_protected = protected_models
                    .map_or(false, |arr| arr.iter().any(|m| m.as_str() == Some(&standard_id)));

                if is_protected {
                    if self
                        .restore_quota_protection(account_json, &account_id, account_path, &standard_id)
                        .await
                        .unwrap_or(false)
                    {
                        changed = true;
                    }
                }
            }
        }

        let _ = changed;
        false
    }

    /// Calculate max remaining quota percentage
    pub(crate) fn calculate_quota_stats(&self, quota: &serde_json::Value) -> Option<i32> {
        let models = match quota.get("models").and_then(|m| m.as_array()) {
            Some(m) => m,
            None => return None,
        };

        let mut max_percentage = 0;
        let mut has_data = false;

        for model in models {
            if let Some(pct) = model.get("percentage").and_then(|v| v.as_i64()) {
                let pct_i32 = pct as i32;
                if pct_i32 > max_percentage {
                    max_percentage = pct_i32;
                }
                has_data = true;
            }
        }

        if has_data {
            Some(max_percentage)
        } else {
            None
        }
    }

    /// Trigger quota protection for a specific model
    async fn trigger_quota_protection(
        &self,
        account_json: &mut serde_json::Value,
        account_id: &str,
        account_path: &PathBuf,
        current_val: i32,
        threshold: i32,
        model_name: &str,
    ) -> Result<bool, String> {
        // [FIX] Check if account exists in index before writing
        if !Self::account_exists_by_path(account_path) {
            tracing::warn!("trigger_quota_protection: Account {} not in index, skipping", account_id);
            return Ok(false);
        }

        if account_json.get("protected_models").is_none() {
            account_json["protected_models"] = serde_json::Value::Array(Vec::new());
        }

        let protected_models = account_json["protected_models"].as_array_mut().unwrap();

        if !protected_models
            .iter()
            .any(|m| m.as_str() == Some(model_name))
        {
            protected_models.push(serde_json::Value::String(model_name.to_string()));

            tracing::info!(
                "账号 {} 的模型 {} 因配额受限（{}% <= {}%）已被加入保护列表",
                account_id,
                model_name,
                current_val,
                threshold
            );

            let json_str = serde_json::to_string_pretty(account_json)
                .map_err(|e| format!("序列化 JSON 失败: {}", e))?;

            // [FIX] Use tokio::fs::write instead of blocking std::fs::write
            tokio::fs::write(account_path, json_str)
                .await
                .map_err(|e| format!("写入文件失败: {}", e))?;

            return Ok(true);
        }

        Ok(false)
    }

    /// Check and restore quota from account-level protection
    async fn check_and_restore_quota(
        &self,
        account_json: &mut serde_json::Value,
        account_path: &PathBuf,
        quota: &serde_json::Value,
        config: &crate::models::QuotaProtectionConfig,
    ) -> bool {
        // [FIX] Check if account exists in index before writing
        if !Self::account_exists_by_path(account_path) {
            tracing::warn!("check_and_restore_quota: Account {:?} not in index, skipping", account_path);
            return false;
        }

        tracing::info!(
            "正在迁移账号 {} 从全局配额保护模式至模型级保护模式",
            account_json
                .get("email")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
        );

        account_json["proxy_disabled"] = serde_json::Value::Bool(false);
        account_json["proxy_disabled_reason"] = serde_json::Value::Null;
        account_json["proxy_disabled_at"] = serde_json::Value::Null;

        let threshold = config.threshold_percentage as i32;
        let mut protected_list = Vec::new();

        if let Some(models) = quota.get("models").and_then(|m| m.as_array()) {
            for model in models {
                let name = model.get("name").and_then(|v| v.as_str()).unwrap_or("");
                // [FIX] Normalize model name before comparing with monitored_models
                let standard_id = crate::proxy::common::model_mapping::normalize_to_standard_id(name)
                    .unwrap_or_else(|| name.to_string());
                if !config.monitored_models.iter().any(|m| m == &standard_id) {
                    continue;
                }

                let percentage = model
                    .get("percentage")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;
                if percentage <= threshold {
                    protected_list.push(serde_json::Value::String(standard_id));
                }
            }
        }

        account_json["protected_models"] = serde_json::Value::Array(protected_list);

        if let Ok(json_str) = serde_json::to_string_pretty(account_json) {
            // [FIX] Use tokio::fs::write instead of blocking std::fs::write
            if let Err(e) = tokio::fs::write(account_path, json_str).await {
                tracing::error!(
                    "[check_and_restore_quota] Failed to write account file: {}",
                    e
                );
            }
        } else {
            tracing::error!("[check_and_restore_quota] Failed to serialize account json");
        }

        false
    }

    /// Restore quota protection for a specific model
    async fn restore_quota_protection(
        &self,
        account_json: &mut serde_json::Value,
        account_id: &str,
        account_path: &PathBuf,
        model_name: &str,
    ) -> Result<bool, String> {
        // [FIX] Check if account exists in index before writing
        if !Self::account_exists_by_path(account_path) {
            tracing::warn!("restore_quota_protection: Account {} not in index, skipping", account_id);
            return Ok(false);
        }

        if let Some(arr) = account_json
            .get_mut("protected_models")
            .and_then(|v| v.as_array_mut())
        {
            let original_len = arr.len();
            arr.retain(|m| m.as_str() != Some(model_name));

            if arr.len() < original_len {
                tracing::info!(
                    "账号 {} 的模型 {} 配额已恢复，移出保护列表",
                    account_id,
                    model_name
                );
                let json_str = serde_json::to_string_pretty(account_json)
                    .map_err(|e| format!("序列化 JSON 失败: {}", e))?;

                // [FIX] Use tokio::fs::write instead of blocking std::fs::write
                tokio::fs::write(account_path, json_str)
                    .await
                    .map_err(|e| format!("写入文件失败: {}", e))?;
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Read quota percentage for a specific model from JSON file
    /// Used for precise sorting by target model's quota instead of max
    ///
    /// # Arguments
    /// * `account_path` - Path to account JSON file
    /// * `model_name` - Target model name (already normalized)
    #[allow(dead_code)]
    pub fn get_model_quota_from_json(account_path: &PathBuf, model_name: &str) -> Option<i32> {
        let content = std::fs::read_to_string(account_path).ok()?;
        let account: serde_json::Value = serde_json::from_str(&content).ok()?;
        let models = account.get("quota")?.get("models")?.as_array()?;

        for model in models {
            if let Some(name) = model.get("name").and_then(|v| v.as_str()) {
                if crate::proxy::common::model_mapping::normalize_to_standard_id(name)
                    .unwrap_or_else(|| name.to_string())
                    == model_name
                {
                    return model
                        .get("percentage")
                        .and_then(|v| v.as_i64())
                        .map(|p| p as i32);
                }
            }
        }
        None
    }

    /// Test helper: public access to get_model_quota_from_json
    #[cfg(test)]
    pub fn get_model_quota_from_json_for_test(account_path: &PathBuf, model_name: &str) -> Option<i32> {
        Self::get_model_quota_from_json(account_path, model_name)
    }
}
