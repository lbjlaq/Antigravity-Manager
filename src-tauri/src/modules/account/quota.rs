//! Quota fetching and protection logic.

use reqwest::StatusCode;
use serde::Serialize;

use super::crud::upsert_account;
use super::storage::{list_accounts, load_account, load_account_index, save_account, save_account_index};
use crate::error::{AppError, AppResult};
use crate::models::{Account, QuotaData, TokenData};
use crate::modules;

/// Statistics for batch quota refresh.
#[derive(Serialize)]
pub struct RefreshStats {
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub details: Vec<String>,
}

/// Update account quota with protection logic.
pub fn update_account_quota(account_id: &str, quota: QuotaData) -> Result<(), String> {
    let mut account = load_account(account_id)?;
    account.update_quota(quota);

    // Quota protection logic
    if let Ok(config) = crate::modules::config::load_app_config() {
        if config.quota_protection.enabled {
            if let Some(ref q) = account.quota {
                let threshold = config.quota_protection.threshold_percentage as i32;

                for model in &q.models {
                    let standard_id =
                        match crate::proxy::common::model_mapping::normalize_to_standard_id(
                            &model.name,
                        ) {
                            Some(id) => id,
                            None => continue,
                        };

                    if !config
                        .quota_protection
                        .monitored_models
                        .contains(&standard_id)
                    {
                        continue;
                    }

                    if model.percentage <= threshold {
                        if !account.protected_models.contains(&standard_id) {
                            crate::modules::logger::log_info(&format!(
                                "[Quota] Triggering model protection: {} ({} [{}] remaining {}% <= threshold {}%)",
                                account.email, standard_id, model.name, model.percentage, threshold
                            ));
                            account.protected_models.insert(standard_id.clone());
                        }
                    } else if account.protected_models.contains(&standard_id) {
                        crate::modules::logger::log_info(&format!(
                            "[Quota] Model protection recovered: {} ({} [{}] quota restored to {}%)",
                            account.email, standard_id, model.name, model.percentage
                        ));
                        account.protected_models.remove(&standard_id);
                    }
                }

                // Migrate from account-level to model-level protection
                if account.proxy_disabled
                    && account
                        .proxy_disabled_reason
                        .as_ref()
                        .map_or(false, |r| r == "quota_protection")
                {
                    crate::modules::logger::log_info(&format!(
                        "[Quota] Migrating account {} from account-level to model-level protection",
                        account.email
                    ));
                    account.proxy_disabled = false;
                    account.proxy_disabled_reason = None;
                    account.proxy_disabled_at = None;
                }
            }
        }
    }

    save_account(&account)?;

    // [FIX] Trigger TokenManager account reload signal
    crate::proxy::server::trigger_account_reload(account_id);

    Ok(())
}

/// Toggle proxy disabled status for an account.
pub fn toggle_proxy_status(account_id: &str, enable: bool, reason: Option<&str>) -> Result<(), String> {
    let mut account = load_account(account_id)?;

    account.proxy_disabled = !enable;
    account.proxy_disabled_reason = if !enable {
        reason.map(|s| s.to_string())
    } else {
        None
    };
    account.proxy_disabled_at = if !enable {
        Some(chrono::Utc::now().timestamp())
    } else {
        None
    };

    save_account(&account)?;

    let mut index = load_account_index()?;
    if let Some(summary) = index.accounts.iter_mut().find(|a| a.id == account_id) {
        summary.proxy_disabled = !enable;
        save_account_index(&index)?;
    }

    Ok(())
}

/// Quota query with retry.
pub async fn fetch_quota_with_retry(account: &mut Account) -> AppResult<QuotaData> {
    use crate::modules::oauth;

    // 1. Ensure Token is valid
    // [FIX #1583] Pass account_id for proper context
    let token = match oauth::ensure_fresh_token(&account.token, Some(&account.id)).await {
        Ok(t) => t,
        Err(e) => {
            if e.contains("invalid_grant") {
                modules::logger::log_error(&format!(
                    "Disabling account {} due to invalid_grant during token refresh",
                    account.email
                ));
                account.disabled = true;
                account.disabled_at = Some(chrono::Utc::now().timestamp());
                account.disabled_reason = Some(format!("invalid_grant: {}", e));
                let _ = save_account(account);
            }
            return Err(AppError::OAuth(e));
        }
    };

    if token.access_token != account.token.access_token {
        modules::logger::log_info(&format!("Time-based Token refresh: {}", account.email));
        account.token = token.clone();

        let name = fetch_display_name_if_missing(account, &token.access_token).await;
        account.name = name.clone();
        upsert_account(account.email.clone(), name, token.clone()).map_err(AppError::Account)?;
    }

    // Supplement display name if missing
    if account.name.is_none()
        || account
            .name
            .as_ref()
            .map_or(false, |n| n.trim().is_empty())
    {
        let name = fetch_display_name_if_missing(account, &account.token.access_token).await;
        if name.is_some() {
            account.name = name.clone();
            let _ = upsert_account(account.email.clone(), name, account.token.clone());
        }
    }

    // 2. Attempt query
    let result: AppResult<(QuotaData, Option<String>)> =
        modules::fetch_quota(&account.token.access_token, &account.email).await;

    // Update project_id if changed
    if let Ok((ref _q, ref project_id)) = result {
        if project_id.is_some() && *project_id != account.token.project_id {
            modules::logger::log_info(&format!(
                "Detected project_id update ({}), saving...",
                account.email
            ));
            account.token.project_id = project_id.clone();
            let _ = upsert_account(
                account.email.clone(),
                account.name.clone(),
                account.token.clone(),
            );
        }
    }

    // 3. Handle 401 error
    if let Err(AppError::Network(ref e)) = result {
        if let Some(status) = e.status() {
            if status == StatusCode::UNAUTHORIZED {
                return handle_401_retry(account).await;
            }
        }
    }

    result.map(|(q, _)| q)
}

async fn fetch_display_name_if_missing(account: &Account, access_token: &str) -> Option<String> {
    use crate::modules::oauth;

    if account.name.is_none() || account.name.as_ref().map_or(false, |n| n.trim().is_empty()) {
        modules::logger::log_info(&format!(
            "Account {} missing display name, attempting to fetch...",
            account.email
        ));
        // [FIX #1583] Pass account_id for proper context
        match oauth::get_user_info(access_token, Some(&account.id)).await {
            Ok(user_info) => {
                let display_name = user_info.get_display_name();
                modules::logger::log_info(&format!(
                    "Successfully fetched display name: {:?}",
                    display_name
                ));
                display_name
            }
            Err(e) => {
                modules::logger::log_warn(&format!("Failed to fetch display name: {}", e));
                None
            }
        }
    } else {
        account.name.clone()
    }
}

async fn handle_401_retry(account: &mut Account) -> AppResult<QuotaData> {
    use crate::modules::oauth;

    modules::logger::log_warn(&format!(
        "401 Unauthorized for {}, forcing refresh...",
        account.email
    ));

    // [FIX #1583] Pass account_id for proper context
    let token_res = match oauth::refresh_access_token(&account.token.refresh_token, Some(&account.id)).await {
        Ok(t) => t,
        Err(e) => {
            if e.contains("invalid_grant") {
                modules::logger::log_error(&format!(
                    "Disabling account {} due to invalid_grant during forced refresh",
                    account.email
                ));
                account.disabled = true;
                account.disabled_at = Some(chrono::Utc::now().timestamp());
                account.disabled_reason = Some(format!("invalid_grant: {}", e));
                let _ = save_account(account);
            }
            return Err(AppError::OAuth(e));
        }
    };

    let new_token = TokenData::new(
        token_res.access_token.clone(),
        account.token.refresh_token.clone(),
        token_res.expires_in,
        account.token.email.clone(),
        account.token.project_id.clone(),
        None,
    );

    let name = fetch_display_name_if_missing(account, &token_res.access_token).await;

    account.token = new_token.clone();
    account.name = name.clone();
    upsert_account(account.email.clone(), name, new_token.clone()).map_err(AppError::Account)?;

    let retry_result: AppResult<(QuotaData, Option<String>)> =
        modules::fetch_quota(&new_token.access_token, &account.email).await;

    if let Ok((ref _q, ref project_id)) = retry_result {
        if project_id.is_some() && *project_id != account.token.project_id {
            account.token.project_id = project_id.clone();
            let _ = upsert_account(
                account.email.clone(),
                account.name.clone(),
                account.token.clone(),
            );
        }
    }

    if let Err(AppError::Network(ref e)) = retry_result {
        if let Some(s) = e.status() {
            if s == StatusCode::FORBIDDEN {
                let mut q = QuotaData::new();
                q.is_forbidden = true;
                return Ok(q);
            }
        }
    }

    retry_result.map(|(q, _)| q)
}

/// Core logic to batch refresh all account quotas.
pub async fn refresh_all_quotas_logic() -> Result<RefreshStats, String> {
    use futures::future::join_all;
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    const MAX_CONCURRENT: usize = 5;
    let start = std::time::Instant::now();

    crate::modules::logger::log_info(&format!(
        "Starting batch refresh of all account quotas (Concurrent mode, max: {})",
        MAX_CONCURRENT
    ));
    let accounts = list_accounts().await?;

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT));

    let tasks: Vec<_> = accounts
        .into_iter()
        .filter(|account| {
            if account.disabled {
                crate::modules::logger::log_info(&format!(
                    "  - Skipping {} (Disabled)",
                    account.email
                ));
                return false;
            }
            if let Some(ref q) = account.quota {
                if q.is_forbidden {
                    crate::modules::logger::log_info(&format!(
                        "  - Skipping {} (Forbidden)",
                        account.email
                    ));
                    return false;
                }
            }
            true
        })
        .map(|mut account| {
            let email = account.email.clone();
            let account_id = account.id.clone();
            let permit = semaphore.clone();
            async move {
                let _guard = permit.acquire().await.unwrap();
                crate::modules::logger::log_info(&format!("  - Processing {}", email));
                match fetch_quota_with_retry(&mut account).await {
                    Ok(quota) => {
                        if let Err(e) = update_account_quota(&account_id, quota) {
                            let msg = format!("Account {}: Save quota failed - {}", email, e);
                            crate::modules::logger::log_error(&msg);
                            Err(msg)
                        } else {
                            crate::modules::logger::log_info(&format!("    {} Success", email));
                            Ok(())
                        }
                    }
                    Err(e) => {
                        let msg = format!("Account {}: Fetch quota failed - {}", email, e);
                        crate::modules::logger::log_error(&msg);
                        Err(msg)
                    }
                }
            }
        })
        .collect();

    let total = tasks.len();
    let results = join_all(tasks).await;

    let mut success = 0;
    let mut failed = 0;
    let mut details = Vec::new();

    for result in results {
        match result {
            Ok(()) => success += 1,
            Err(msg) => {
                failed += 1;
                details.push(msg);
            }
        }
    }

    let elapsed = start.elapsed();
    crate::modules::logger::log_info(&format!(
        "Batch refresh completed: {} success, {} failed, took: {}ms",
        success,
        failed,
        elapsed.as_millis()
    ));

    Ok(RefreshStats {
        total,
        success,
        failed,
        details,
    })
}
