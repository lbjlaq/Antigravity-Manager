use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::models::QuotaData;
use crate::modules::config;

const QUOTA_API_URL: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:fetchAvailableModels";

/// Critical retry threshold: considered near recovery when quota reaches 95%
const NEAR_READY_THRESHOLD: i32 = 95;
const MAX_RETRIES: u32 = 3;
const RETRY_DELAY_SECS: u64 = 30;

#[derive(Debug, Serialize, Deserialize)]
struct QuotaResponse {
    models: std::collections::HashMap<String, ModelInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ModelInfo {
    #[serde(rename = "quotaInfo")]
    quota_info: Option<QuotaInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct QuotaInfo {
    #[serde(rename = "remainingFraction")]
    remaining_fraction: Option<f64>,
    #[serde(rename = "resetTime")]
    reset_time: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoadProjectResponse {
    #[serde(rename = "cloudaicompanionProject")]
    project_id: Option<String>,
    #[serde(rename = "currentTier")]
    current_tier: Option<Tier>,
    #[serde(rename = "paidTier")]
    paid_tier: Option<Tier>,
}

#[derive(Debug, Deserialize)]
struct Tier {
    id: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "quotaTier")]
    quota_tier: Option<String>,
    #[allow(dead_code)]
    name: Option<String>,
    #[allow(dead_code)]
    slug: Option<String>,
}

/// Get shared HTTP Client (15s timeout)
fn create_client() -> reqwest::Client {
    crate::utils::http::get_client()
}

/// Get shared HTTP Client (60s timeout)
fn create_warmup_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .no_proxy()
        .build()
        .unwrap_or_else(|_| crate::utils::http::get_long_client())
}

const CLOUD_CODE_BASE_URL: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com";

/// Fetch project ID and subscription tier
async fn fetch_project_id(access_token: &str, email: &str) -> (Option<String>, Option<String>) {
    let client = create_client();
    let meta = json!({"metadata": {"ideType": "ANTIGRAVITY"}});

    let res = client
        .post(format!("{}/v1internal:loadCodeAssist", CLOUD_CODE_BASE_URL))
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {}", access_token))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::USER_AGENT, crate::constants::USER_AGENT.as_str())
        .json(&meta)
        .send()
        .await;

    match res {
        Ok(res) => {
            if res.status().is_success() {
                if let Ok(data) = res.json::<LoadProjectResponse>().await {
                    let project_id = data.project_id.clone();
                    
                    // Core logic: Priority to subscription ID from paid_tier, which better reflects actual account benefits than current_tier
                    let subscription_tier = data.paid_tier
                        .and_then(|t| t.id)
                        .or_else(|| data.current_tier.and_then(|t| t.id));
                    
                    if let Some(ref tier) = subscription_tier {
                        crate::modules::logger::log_info(&format!(
                            "📊 [{}] Subscription identified successfully: {}", email, tier
                        ));
                    }
                    
                    return (project_id, subscription_tier);
                }
            } else {
                crate::modules::logger::log_warn(&format!(
                    "⚠️  [{}] loadCodeAssist failed: Status: {}", email, res.status()
                ));
            }
        }
        Err(e) => {
            crate::modules::logger::log_error(&format!("❌ [{}] loadCodeAssist network error: {}", email, e));
        }
    }
    
    (None, None)
}

/// Unified entry point for fetching account quota
pub async fn fetch_quota(access_token: &str, email: &str) -> crate::error::AppResult<(QuotaData, Option<String>)> {
    fetch_quota_with_cache(access_token, email, None).await
}

/// Fetch quota with cache support
pub async fn fetch_quota_with_cache(
    access_token: &str,
    email: &str,
    cached_project_id: Option<&str>,
) -> crate::error::AppResult<(QuotaData, Option<String>)> {
    use crate::error::AppError;
    
    // Optimization: Skip loadCodeAssist call if project_id is cached to save API quota
    let (project_id, subscription_tier) = if let Some(pid) = cached_project_id {
        (Some(pid.to_string()), None)
    } else {
        fetch_project_id(access_token, email).await
    };
    
    let final_project_id = project_id.as_deref().unwrap_or("bamboo-precept-lgxtn");
    
    let client = create_client();
    let payload = json!({
        "project": final_project_id
    });
    
    let url = QUOTA_API_URL;
    let mut last_error: Option<AppError> = None;

    for attempt in 1..=MAX_RETRIES {
        match client
            .post(url)
            .bearer_auth(access_token)
            .header(reqwest::header::USER_AGENT, crate::constants::USER_AGENT.as_str())
            .json(&json!(payload))
            .send()
            .await
        {
            Ok(response) => {
                // Convert HTTP error status to AppError
                if let Err(_) = response.error_for_status_ref() {
                    let status = response.status();
                    
                    // ✅ Special handling for 403 Forbidden - return directly, no retry
                    if status == reqwest::StatusCode::FORBIDDEN {
                        crate::modules::logger::log_warn(&format!(
                            "Account unauthorized (403 Forbidden), marking as forbidden"
                        ));
                        let mut q = QuotaData::new();
                        q.is_forbidden = true;
                        q.subscription_tier = subscription_tier.clone();
                        return Ok((q, project_id.clone()));
                    }
                    
                    // Continue retry logic for other errors
                    if attempt < MAX_RETRIES {
                         let text = response.text().await.unwrap_or_default();
                         crate::modules::logger::log_warn(&format!("API Error: {} - {} (Attempt {}/{})", status, text, attempt, MAX_RETRIES));
                         last_error = Some(AppError::Unknown(format!("HTTP {} - {}", status, text)));
                         tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                         continue;
                    } else {
                         let text = response.text().await.unwrap_or_default();
                         return Err(AppError::Unknown(format!("API Error: {} - {}", status, text)));
                    }
                }

                let quota_response: QuotaResponse = response
                    .json()
                    .await
                    .map_err(|e| AppError::Network(e))?;
                
                let mut quota_data = QuotaData::new();
                
                // Use debug level for detailed info to avoid console noise
                tracing::debug!("Quota API returned {} models", quota_response.models.len());

                for (name, info) in quota_response.models {
                    if let Some(quota_info) = info.quota_info {
                        let percentage = quota_info.remaining_fraction
                            .map(|f| (f * 100.0) as i32)
                            .unwrap_or(0);
                        
                        let reset_time = quota_info.reset_time.unwrap_or_default();
                        
                        // Only keep models we care about
                        if name.contains("gemini") || name.contains("claude") {
                            quota_data.add_model(name, percentage, reset_time);
                        }
                    }
                }
                
                // Set subscription tier
                quota_data.subscription_tier = subscription_tier.clone();
                
                return Ok((quota_data, project_id.clone()));
            },
            Err(e) => {
                crate::modules::logger::log_warn(&format!("Request failed: {} (Attempt {}/{})", e, attempt, MAX_RETRIES));
                last_error = Some(AppError::Network(e));
                if attempt < MAX_RETRIES {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    }
    
    Err(last_error.unwrap_or_else(|| AppError::Unknown("Quota fetch failed".to_string())))
}

/// Internal fetch quota logic
#[allow(dead_code)]
pub async fn fetch_quota_inner(access_token: &str, email: &str) -> crate::error::AppResult<(QuotaData, Option<String>)> {
    fetch_quota_with_cache(access_token, email, None).await
}

/// Batch fetch all account quotas (backup functionality)
#[allow(dead_code)]
pub async fn fetch_all_quotas(accounts: Vec<(String, String)>) -> Vec<(String, crate::error::AppResult<QuotaData>)> {
    let mut results = Vec::new();
    
    for (account_id, access_token) in accounts {
        // In batch queries, pass account_id for log identification
        let result = fetch_quota(&access_token, &account_id).await.map(|(q, _)| q);
        results.push((account_id, result));
    }
    
    results
}

/// Get valid token (auto-refresh if expired)
pub async fn get_valid_token_for_warmup(account: &crate::models::account::Account) -> Result<(String, String), String> {
    let mut account = account.clone();
    
    // Check and auto-refresh token
    // [FIX #1583] Pass account_id for proper context
    let new_token = crate::modules::oauth::ensure_fresh_token(&account.token, Some(&account.id)).await?;
    
    // If token changed (meant refreshed), save it
    if new_token.access_token != account.token.access_token {
        account.token = new_token;
        if let Err(e) = crate::modules::account::save_account(&account) {
            crate::modules::logger::log_warn(&format!("[Warmup] Failed to save refreshed token: {}", e));
        } else {
            crate::modules::logger::log_info(&format!("[Warmup] Successfully refreshed and saved new token for {}", account.email));
        }
    }
    
    // Fetch project_id
    let (project_id, _) = fetch_project_id(&account.token.access_token, &account.email).await;
    let final_pid = project_id.unwrap_or_else(|| "bamboo-precept-lgxtn".to_string());
    
    Ok((account.token.access_token, final_pid))
}

/// Send warmup request via proxy internal API
pub async fn warmup_model_directly(
    access_token: &str,
    model_name: &str,
    project_id: &str,
    email: &str,
    percentage: i32,
) -> bool {
    // Get currently configured proxy port
    let port = config::load_app_config()
        .map(|c| c.proxy.port)
        .unwrap_or(8045);

    let warmup_url = format!("http://127.0.0.1:{}/internal/warmup", port);
    let body = json!({
        "email": email,
        "model": model_name,
        "access_token": access_token,
        "project_id": project_id
    });

    let client = create_warmup_client();
    let resp = client
        .post(&warmup_url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await;

    match resp {
        Ok(response) => {
            let status = response.status();
            if status.is_success() {
                crate::modules::logger::log_info(&format!("[Warmup] ✓ Triggered {} for {} (was {}%)", model_name, email, percentage));
                true
            } else {
                let text = response.text().await.unwrap_or_default();
                crate::modules::logger::log_warn(&format!("[Warmup] ✗ {} for {} (was {}%): HTTP {} - {}", model_name, email, percentage, status, text));
                false
            }
        }
        Err(e) => {
            crate::modules::logger::log_warn(&format!("[Warmup] ✗ {} for {} (was {}%): {}", model_name, email, percentage, e));
            false
        }
    }
}

/// Smart warmup for all accounts
pub async fn warm_up_all_accounts() -> Result<String, String> {
    let mut retry_count = 0;
    
    loop {
        let all_accounts = crate::modules::account::list_accounts().await.unwrap_or_default();
        
        // [FIX] Filter out disabled and proxy_disabled accounts
        let target_accounts: Vec<_> = all_accounts
            .into_iter()
            .filter(|a| !a.disabled && !a.proxy_disabled)
            .collect();

        if target_accounts.is_empty() {
            return Ok("No accounts available".to_string());
        }

        crate::modules::logger::log_info(&format!("[Warmup] Screening models for {} accounts...", target_accounts.len()));

        let mut warmup_items = Vec::new();
        let mut has_near_ready_models = false;

        // Concurrently fetch quotas (batch size 5)
        let batch_size = 5;
        for batch in target_accounts.chunks(batch_size) {
            let mut handles = Vec::new();
            for account in batch {
                let account = account.clone();
                let handle = tokio::spawn(async move {
                    let (token, pid) = match get_valid_token_for_warmup(&account).await {
                        Ok(t) => t,
                        Err(_) => return None,
                    };
                    let quota = fetch_quota_with_cache(&token, &account.email, Some(&pid)).await.ok();
                    Some((account.email.clone(), token, pid, quota))
                });
                handles.push(handle);
            }

            for handle in handles {
                if let Ok(Some((email, token, pid, Some((fresh_quota, _))))) = handle.await {
                    let mut account_warmed_series = std::collections::HashSet::new();
                    for m in fresh_quota.models {
                        if m.percentage >= 100 {
                            let model_to_ping = m.name.clone();
                            
                            // [FIX] Removed hardcoded whitelist - use user config instead
                            // [SAFE] Fail-closed: if config can't be loaded, skip warmup candidate
                            let monitored_models = match crate::modules::config::load_app_config() {
                                Ok(cfg) => cfg.scheduled_warmup.monitored_models,
                                Err(e) => {
                                    crate::modules::logger::log_warn(&format!(
                                        "[Warmup] Failed to load app config, skipping warmup candidate {} for {}: {}",
                                        model_to_ping, email, e
                                    ));
                                    continue;
                                }
                            };
                            
                            // Only warmup models in user's monitored list (or all if list is empty)
                            let should_warmup = monitored_models.is_empty() || monitored_models.contains(&model_to_ping);
                            
                            if should_warmup && !account_warmed_series.contains(&model_to_ping) {
                                warmup_items.push((email.clone(), model_to_ping.clone(), token.clone(), pid.clone(), m.percentage));
                                account_warmed_series.insert(model_to_ping);
                            }
                        } else if m.percentage >= NEAR_READY_THRESHOLD {
                            has_near_ready_models = true;
                        }
                    }
                }
            }
        }

        if !warmup_items.is_empty() {
            let total_before = warmup_items.len();
            
            // Filter out models warmed up within 4 hours
            warmup_items.retain(|(email, model, _, _, _)| {
                let history_key = format!("{}:{}:100", email, model);
                !crate::modules::scheduler::check_cooldown(&history_key, 14400)
            });
            
            if warmup_items.is_empty() {
                let skipped = total_before;
                crate::modules::logger::log_info(&format!("[Warmup] Returning to frontend: All models in cooldown, skipped {}", skipped));
                return Ok(format!("All models are in 4-hour cooldown, skipped {} items", skipped));
            }
            
            let total = warmup_items.len();
            let skipped = total_before - total;
            
            if skipped > 0 {
                crate::modules::logger::log_info(&format!(
                    "[Warmup] Skipped {} models in cooldown, preparing to warmup {}",
                    skipped, total
                ));
            }
            
            crate::modules::logger::log_info(&format!(
                "[Warmup] 🔥 Starting manual warmup for {} models",
                total
            ));
            
            tokio::spawn(async move {
                let mut success = 0;
                let batch_size = 3;
                let now_ts = chrono::Utc::now().timestamp();
                
                for (batch_idx, batch) in warmup_items.chunks(batch_size).enumerate() {
                    let mut handles = Vec::new();
                    
                    for (email, model, token, pid, pct) in batch.iter() {
                        let email = email.clone();
                        let model = model.clone();
                        let token = token.clone();
                        let pid = pid.clone();
                        let pct = *pct;
                        
                        let handle = tokio::spawn(async move {
                            let result = warmup_model_directly(&token, &model, &pid, &email, pct).await;
                            (result, email, model)
                        });
                        handles.push(handle);
                    }
                    
                    for handle in handles {
                        match handle.await {
                            Ok((true, email, model)) => {
                                success += 1;
                                let history_key = format!("{}:{}:100", email, model);
                                crate::modules::scheduler::record_warmup_history(&history_key, now_ts);
                            }
                            _ => {}
                        }
                    }
                    
                    if batch_idx < (warmup_items.len() + batch_size - 1) / batch_size - 1 {
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    }
                }
                
                crate::modules::logger::log_info(&format!("[Warmup] Warmup task completed: success {}/{}", success, total));
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                let _ = crate::modules::account::refresh_all_quotas_logic().await;
            });
            crate::modules::logger::log_info(&format!("[Warmup] Returning to frontend: Warmup task triggered for {} models", total));
            return Ok(format!("Warmup task triggered for {} models", total));
        }

        if has_near_ready_models && retry_count < MAX_RETRIES {
            retry_count += 1;
            crate::modules::logger::log_info(&format!("[Warmup] Critical recovery model detected, waiting {}s to retry ({}/{})", RETRY_DELAY_SECS, retry_count, MAX_RETRIES));
            tokio::time::sleep(tokio::time::Duration::from_secs(RETRY_DELAY_SECS)).await;
            continue;
        }

        return Ok("No models need warmup".to_string());
    }
}

/// Warmup for single account
pub async fn warm_up_account(account_id: &str) -> Result<String, String> {
    let accounts = crate::modules::account::list_accounts().await.unwrap_or_default();
    let account_owned = accounts.iter().find(|a| a.id == account_id).cloned().ok_or_else(|| "Account not found".to_string())?;
    
    let email = account_owned.email.clone();
    let (token, pid) = get_valid_token_for_warmup(&account_owned).await?;
    let (fresh_quota, _) = fetch_quota_with_cache(&token, &email, Some(&pid)).await.map_err(|e| format!("Failed to fetch quota: {}", e))?;
    
    let mut models_to_warm = Vec::new();
    let mut warmed_series = std::collections::HashSet::new();

    for m in fresh_quota.models {
        if m.percentage >= 100 {
            let model_name = m.name.clone();
            
            // [FIX] Removed hardcoded whitelist - use user config instead
            // [SAFE] Fail-closed for single-account warmup too
            let monitored_models = match crate::modules::config::load_app_config() {
                Ok(cfg) => cfg.scheduled_warmup.monitored_models,
                Err(e) => {
                    crate::modules::logger::log_warn(&format!(
                        "[Warmup] Failed to load app config for account {}, skipping model {}: {}",
                        email, model_name, e
                    ));
                    continue;
                }
            };
            
            // Only warmup models in user's monitored list (or all if list is empty)
            let should_warmup = monitored_models.is_empty() || monitored_models.contains(&model_name);
            
            if should_warmup && !warmed_series.contains(&model_name) {
                models_to_warm.push((model_name.clone(), m.percentage));
                warmed_series.insert(model_name);
            }
        }
    }

    if models_to_warm.is_empty() {
        return Ok("No warmup needed".to_string());
    }

    let warmed_count = models_to_warm.len();
    
    tokio::spawn(async move {
        for (name, pct) in models_to_warm {
            if warmup_model_directly(&token, &name, &pid, &email, pct).await {
                let history_key = format!("{}:{}:100", email, name);
                let now_ts = chrono::Utc::now().timestamp();
                crate::modules::scheduler::record_warmup_history(&history_key, now_ts);
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
        let _ = crate::modules::account::refresh_all_quotas_logic().await;
    });

    Ok(format!("Successfully triggered warmup for {} model series", warmed_count))
}
