use crate::models::QuotaData;
use crate::modules::config;
use rquest;
use serde::{Deserialize, Serialize};
use serde_json::json;

const QUOTA_API_URL: &str = "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels";

/// Critical retry threshold: considered near recovery when quota reaches 95%
const NEAR_READY_THRESHOLD: i32 = 95;
const MAX_RETRIES: u32 = 3;
const RETRY_DELAY_SECS: u64 = 30;

#[derive(Debug, Serialize, Deserialize)]
struct QuotaResponse {
    models: std::collections::HashMap<String, ModelInfo>,
    #[serde(rename = "deprecatedModelIds")]
    deprecated_model_ids: Option<std::collections::HashMap<String, DeprecatedModelInfo>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DeprecatedModelInfo {
    #[serde(rename = "newModelId")]
    new_model_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ModelInfo {
    #[serde(rename = "quotaInfo")]
    quota_info: Option<QuotaInfo>,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    #[serde(rename = "supportsImages")]
    supports_images: Option<bool>,
    #[serde(rename = "supportsThinking")]
    supports_thinking: Option<bool>,
    #[serde(rename = "thinkingBudget")]
    thinking_budget: Option<i32>,
    recommended: Option<bool>,
    #[serde(rename = "maxTokens")]
    max_tokens: Option<i32>,
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: Option<i32>,
    #[serde(rename = "supportedMimeTypes")]
    supported_mime_types: Option<std::collections::HashMap<String, bool>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct QuotaInfo {
    #[serde(rename = "remainingFraction")]
    remaining_fraction: Option<f64>,
    #[serde(rename = "resetTime")]
    reset_time: Option<String>,
}

/// Get shared HTTP Client (15s timeout) for pure info fetching (No JA3)
async fn create_standard_client(account_id: Option<&str>) -> rquest::Client {
    if let Some(pool) = crate::proxy::proxy_pool::get_global_proxy_pool() {
        pool.get_effective_standard_client(account_id, 15).await
    } else {
        crate::utils::http::get_standard_client()
    }
}

/// Get shared HTTP Client (60s timeout) for pure info fetching (No JA3)
#[allow(dead_code)] // 预留给预热/后台任务调用
async fn create_long_standard_client(account_id: Option<&str>) -> rquest::Client {
    if let Some(pool) = crate::proxy::proxy_pool::get_global_proxy_pool() {
        pool.get_effective_standard_client(account_id, 60).await
    } else {
        crate::utils::http::get_long_standard_client()
    }
}

/// GeminiCLI 使用的 User-Agent
const GEMINI_CLI_USER_AGENT: &str = "GeminiCLI/0.1.5 (Windows; AMD64)";
/// Antigravity sandbox 端点
const ANTIGRAVITY_CLOUD_CODE_BASE_URL: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com";
/// GeminiCLI prod 端点
const GEMINI_CLI_CLOUD_CODE_BASE_URL: &str = "https://cloudcode-pa.googleapis.com";
/// Legacy fallback project_id from older builds (invalid for GeminiCLI)
const LEGACY_FALLBACK_PROJECT_ID: &str = "bamboo-precept-lgxtn";

fn extract_project_id_from_value(data: &serde_json::Value) -> Option<String> {
    data.get("cloudaicompanionProject").and_then(|project| {
        if let Some(pid) = project.as_str() {
            return Some(pid.to_string());
        }
        if let Some(obj) = project.as_object() {
            if let Some(pid) = obj.get("id").and_then(|v| v.as_str()) {
                return Some(pid.to_string());
            }
            if let Some(pid) = obj.get("projectId").and_then(|v| v.as_str()) {
                return Some(pid.to_string());
            }
        }
        None
    })
}

fn extract_subscription_tier_from_value(data: &serde_json::Value) -> Option<String> {
    // paidTier > currentTier > allowedTiers(default)
    let get_tier = |tier: &serde_json::Value| -> Option<String> {
        tier.get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                tier.get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
    };

    if let Some(paid) = data.get("paidTier") {
        if let Some(tier) = get_tier(paid) {
            return Some(tier);
        }
    }

    let is_ineligible = data
        .get("ineligibleTiers")
        .and_then(|v| v.as_array())
        .map(|arr| !arr.is_empty())
        .unwrap_or(false);

    if !is_ineligible {
        if let Some(current) = data.get("currentTier") {
            if let Some(tier) = get_tier(current) {
                return Some(tier);
            }
        }
    }

    if let Some(allowed) = data.get("allowedTiers").and_then(|v| v.as_array()) {
        if let Some(default_tier) = allowed
            .iter()
            .find(|t| t.get("isDefault").and_then(|v| v.as_bool()) == Some(true))
        {
            if let Some(tier) = get_tier(default_tier) {
                return Some(format!("{} (Restricted)", tier));
            }
        }
        if let Some(first_tier) = allowed.first() {
            if let Some(tier) = get_tier(first_tier) {
                return Some(format!("{} (Restricted)", tier));
            }
        }
    }

    None
}

/// Fetch project ID and subscription tier
async fn fetch_project_id(
    access_token: &str,
    email: &str,
    account_id: Option<&str>,
    account_type: crate::models::AccountType,
) -> (Option<String>, Option<String>) {
    let client = create_standard_client(account_id).await;
    let meta = json!({
        "metadata": {
            "ideType": "ANTIGRAVITY",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI"
        }
    });

    let (base_url, user_agent) = match account_type {
        crate::models::AccountType::GeminiCli => (
            GEMINI_CLI_CLOUD_CODE_BASE_URL,
            GEMINI_CLI_USER_AGENT.to_string(),
        ),
        crate::models::AccountType::Antigravity => (
            ANTIGRAVITY_CLOUD_CODE_BASE_URL,
            crate::constants::NATIVE_OAUTH_USER_AGENT.to_string(),
        ),
    };

    let res = client
        .post(format!("{}/v1internal:loadCodeAssist", base_url))
        .header(
            rquest::header::AUTHORIZATION,
            format!("Bearer {}", access_token),
        )
        .header(rquest::header::CONTENT_TYPE, "application/json")
        .header(rquest::header::USER_AGENT, user_agent.as_str())
        .json(&meta)
        .send()
        .await;

    match res {
        Ok(res) => {
            if res.status().is_success() {
                if let Ok(data) = res.json::<serde_json::Value>().await {
                    let mut project_id = extract_project_id_from_value(&data);
                    let subscription_tier = extract_subscription_tier_from_value(&data);

                    if project_id.is_none() {
                        // 与 gcli2api 对齐：loadCodeAssist 拿不到项目时，尝试 onboardUser 回退
                        project_id = crate::proxy::project_resolver::fetch_project_id(
                            access_token,
                            account_type,
                        )
                        .await
                        .ok();
                    }

                    if let Some(ref tier) = subscription_tier {
                        crate::modules::logger::log_info(&format!(
                            "📊 [{}] Subscription identified successfully: {}",
                            email, tier
                        ));
                    }

                    return (project_id, subscription_tier);
                } else {
                    crate::modules::logger::log_warn(&format!(
                        "⚠️  [{}] loadCodeAssist response parse failed, fallback to project resolver",
                        email
                    ));
                }
            } else {
                crate::modules::logger::log_warn(&format!(
                    "⚠️  [{}] loadCodeAssist failed: Status: {}",
                    email,
                    res.status()
                ));
            }
        }
        Err(e) => {
            crate::modules::logger::log_error(&format!(
                "❌ [{}] loadCodeAssist network error: {}",
                email, e
            ));
        }
    }

    // 最后兜底：直接走 project_resolver（内含 onboardUser 回退）
    (
        crate::proxy::project_resolver::fetch_project_id(access_token, account_type)
            .await
            .ok(),
        None,
    )
}

fn build_geminicli_probe_payload(project_id: &str, model_name: &str) -> serde_json::Value {
    // Keep payload minimal and aligned with GeminiCLI upstream format.
    json!({
        "model": model_name,
        "project": project_id,
        "request": {
            "contents": [{
                "role": "user",
                "parts": [{ "text": "ping" }]
            }],
            "generationConfig": {
                "maxOutputTokens": 1,
                "temperature": 0
            }
        }
    })
}

async fn probe_geminicli_models_from_generate_content(
    client: &rquest::Client,
    access_token: &str,
    project_id: &str,
) -> Vec<crate::models::quota::ModelQuota> {
    use std::collections::HashSet;

    let candidates: Vec<(&str, Vec<(&str, &str)>)> = vec![
        (
            "gemini-2.5-flash",
            vec![("gemini-2.5-flash", "Gemini 2.5 Flash")],
        ),
        ("gemini-2.5-pro", vec![("gemini-2.5-pro", "Gemini 2.5 Pro")]),
        (
            "gemini-3-pro-preview",
            vec![
                ("gemini-3-pro-preview", "Gemini 3 Pro (Preview)"),
                ("gemini-3-pro-high", "Gemini 3 Pro (High)"),
                ("gemini-3-pro-low", "Gemini 3 Pro (Low)"),
                ("gemini-3.1-pro-preview", "Gemini 3.1 Pro (Preview)"),
                ("gemini-3.1-pro-high", "Gemini 3.1 Pro (High)"),
                ("gemini-3.1-pro-low", "Gemini 3.1 Pro (Low)"),
            ],
        ),
        (
            "gemini-3.1-pro-preview",
            vec![
                ("gemini-3.1-pro-preview", "Gemini 3.1 Pro (Preview)"),
                ("gemini-3.1-pro-high", "Gemini 3.1 Pro (High)"),
                ("gemini-3.1-pro-low", "Gemini 3.1 Pro (Low)"),
            ],
        ),
        ("gemini-3-flash", vec![("gemini-3-flash", "Gemini 3 Flash")]),
        (
            "gemini-3-pro-image",
            vec![("gemini-3-pro-image", "Gemini 3 Pro Image")],
        ),
    ];

    let mut discovered_models: Vec<crate::models::quota::ModelQuota> = Vec::new();
    let mut seen = HashSet::new();

    for (probe_model, exposed_models) in candidates {
        let probe_payload = build_geminicli_probe_payload(project_id, probe_model);
        let response = client
            .post(format!(
                "{}/v1internal:generateContent",
                GEMINI_CLI_CLOUD_CODE_BASE_URL
            ))
            .bearer_auth(access_token)
            .header(rquest::header::USER_AGENT, GEMINI_CLI_USER_AGENT)
            .json(&probe_payload)
            .send()
            .await;

        match response {
            Ok(resp) => {
                if resp.status().is_success() {
                    for (model_name, display_name) in exposed_models {
                        if seen.insert(model_name.to_string()) {
                            discovered_models.push(crate::models::quota::ModelQuota {
                                name: model_name.to_string(),
                                percentage: 100,
                                reset_time: String::new(),
                                display_name: Some(display_name.to_string()),
                                supports_images: None,
                                supports_thinking: None,
                                thinking_budget: None,
                                recommended: Some(true),
                                max_tokens: None,
                                max_output_tokens: None,
                                supported_mime_types: None,
                            });
                        }
                    }
                } else {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    tracing::debug!(
                        "[GeminiCLI] Probe model {} unavailable (HTTP {}): {}",
                        probe_model,
                        status,
                        body
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    "[GeminiCLI] Probe model {} request failed: {}",
                    probe_model,
                    e
                );
            }
        }
    }

    discovered_models.sort_by(|a, b| a.name.cmp(&b.name));
    discovered_models
}

/// Unified entry point for fetching account quota
pub async fn fetch_quota(
    access_token: &str,
    email: &str,
    account_id: Option<&str>,
    account_type: crate::models::AccountType,
) -> crate::error::AppResult<(QuotaData, Option<String>)> {
    fetch_quota_with_cache(access_token, email, None, account_id, account_type).await
}

/// Fetch quota with cache support
pub async fn fetch_quota_with_cache(
    access_token: &str,
    email: &str,
    cached_project_id: Option<&str>,
    account_id: Option<&str>,
    account_type: crate::models::AccountType,
) -> crate::error::AppResult<(QuotaData, Option<String>)> {
    use crate::error::AppError;

    // Optimization: Skip loadCodeAssist call if project_id is cached to save API quota.
    // GeminiCLI compatibility: ignore legacy fallback pid and force re-resolve.
    let valid_cached_project_id = cached_project_id.and_then(|pid| {
        let normalized = pid.trim();
        if normalized.is_empty() {
            return None;
        }
        if account_type == crate::models::AccountType::GeminiCli
            && normalized == LEGACY_FALLBACK_PROJECT_ID
        {
            crate::modules::logger::log_warn(&format!(
                "[GeminiCLI] Ignoring legacy cached project_id for {}, forcing resolver refresh",
                email
            ));
            return None;
        }
        Some(normalized)
    });

    let (project_id, subscription_tier) = if let Some(pid) = valid_cached_project_id {
        (Some(pid.to_string()), None)
    } else {
        fetch_project_id(access_token, email, account_id, account_type).await
    };

    // We keep project_id to store in the DB, but we NO LONGER force inject it into payload if it's absent

    let client = create_standard_client(account_id).await;
    let payload = if let Some(ref pid) = project_id {
        json!({ "project": pid })
    } else {
        json!({}) // Empty payload fallback
    };

    let url = QUOTA_API_URL;
    let mut last_error: Option<AppError> = None;

    let quota_user_agent = match account_type {
        crate::models::AccountType::GeminiCli => GEMINI_CLI_USER_AGENT.to_string(),
        crate::models::AccountType::Antigravity => {
            crate::constants::NATIVE_OAUTH_USER_AGENT.to_string()
        }
    };

    for attempt in 1..=MAX_RETRIES {
        match client
            .post(url)
            .bearer_auth(access_token)
            .header(rquest::header::USER_AGENT, quota_user_agent.as_str())
            .json(&json!(payload))
            .send()
            .await
        {
            Ok(response) => {
                // Convert HTTP error status to AppError
                if let Err(_) = response.error_for_status_ref() {
                    let status = response.status();

                    // ✅ Special handling for 403 Forbidden - return directly, no retry
                    if status == rquest::StatusCode::FORBIDDEN {
                        if account_type == crate::models::AccountType::GeminiCli {
                            // GeminiCLI 与 gcli2api 行为对齐：403 不直接永久标记 forbidden
                            // fetchAvailableModels 在某些账号阶段会返回 403，但实际 generateContent 仍可恢复。
                            crate::modules::logger::log_warn(
                                "[GeminiCLI] fetchAvailableModels returned 403; entering degraded mode (do not mark forbidden).",
                            );

                            let mut q = QuotaData::new();
                            q.subscription_tier = subscription_tier.clone();
                            q.forbidden_reason = Some(
                                "fetchAvailableModels returned 403; degraded mode enabled"
                                    .to_string(),
                            );

                            let recovered_project_id = if project_id.is_some() {
                                project_id.clone()
                            } else {
                                crate::proxy::project_resolver::fetch_project_id(
                                    access_token,
                                    account_type,
                                )
                                .await
                                .ok()
                            };

                            if let Some(ref pid) = recovered_project_id {
                                let discovered_models =
                                    probe_geminicli_models_from_generate_content(
                                        &client,
                                        access_token,
                                        pid,
                                    )
                                    .await;
                                if !discovered_models.is_empty() {
                                    crate::modules::logger::log_info(&format!(
                                        "[GeminiCLI] 403 fallback probe discovered {} models for {}",
                                        discovered_models.len(),
                                        email
                                    ));
                                    q.models = discovered_models;
                                    q.forbidden_reason = Some(
                                        "fetchAvailableModels returned 403; populated by generateContent probe"
                                            .to_string(),
                                    );
                                } else {
                                    crate::modules::logger::log_warn(&format!(
                                        "[GeminiCLI] 403 fallback probe found no available models for {}",
                                        email
                                    ));
                                }
                            }

                            return Ok((q, recovered_project_id));
                        }

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
                        crate::modules::logger::log_warn(&format!(
                            "API Error: {} - {} (Attempt {}/{})",
                            status, text, attempt, MAX_RETRIES
                        ));
                        last_error = Some(AppError::Unknown(format!("HTTP {} - {}", status, text)));
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    } else {
                        let text = response.text().await.unwrap_or_default();
                        return Err(AppError::Unknown(format!(
                            "API Error: {} - {}",
                            status, text
                        )));
                    }
                }

                let quota_response: QuotaResponse =
                    response.json().await.map_err(AppError::from)?;

                let mut quota_data = QuotaData::new();

                // Use debug level for detailed info to avoid console noise
                tracing::debug!("Quota API returned {} models", quota_response.models.len());

                for (name, info) in quota_response.models {
                    if let Some(quota_info) = info.quota_info {
                        let percentage = quota_info
                            .remaining_fraction
                            .map(|f| (f * 100.0) as i32)
                            .unwrap_or(0);

                        let reset_time = quota_info.reset_time.clone().unwrap_or_default();

                        // Only keep models we care about (exclude internal chat models)
                        if name.starts_with("gemini")
                            || name.starts_with("claude")
                            || name.starts_with("gpt")
                            || name.starts_with("image")
                            || name.starts_with("imagen")
                        {
                            let model_quota = crate::models::quota::ModelQuota {
                                name,
                                percentage,
                                reset_time,
                                display_name: info.display_name,
                                supports_images: info.supports_images,
                                supports_thinking: info.supports_thinking,
                                thinking_budget: info.thinking_budget,
                                recommended: info.recommended,
                                max_tokens: info.max_tokens,
                                max_output_tokens: info.max_output_tokens,
                                supported_mime_types: info.supported_mime_types,
                            };
                            quota_data.add_model(model_quota);
                        }
                    }
                }

                // Parse deprecated model routing rules
                if let Some(deprecated) = quota_response.deprecated_model_ids {
                    for (old_id, info) in deprecated {
                        quota_data
                            .model_forwarding_rules
                            .insert(old_id, info.new_model_id);
                    }
                }

                // Set subscription tier
                quota_data.subscription_tier = subscription_tier.clone();

                return Ok((quota_data, project_id.clone()));
            }
            Err(e) => {
                crate::modules::logger::log_warn(&format!(
                    "Request failed: {} (Attempt {}/{})",
                    e, attempt, MAX_RETRIES
                ));
                last_error = Some(AppError::from(e));
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
pub async fn fetch_quota_inner(
    access_token: &str,
    email: &str,
    account_type: crate::models::AccountType,
) -> crate::error::AppResult<(QuotaData, Option<String>)> {
    fetch_quota_with_cache(access_token, email, None, None, account_type).await
}

/// Batch fetch all account quotas (backup functionality)
#[allow(dead_code)]
pub async fn fetch_all_quotas(
    accounts: Vec<(String, String, String, crate::models::AccountType)>,
) -> Vec<(String, crate::error::AppResult<QuotaData>)> {
    let mut results = Vec::new();
    for (id, email, access_token, account_type) in accounts {
        let res = fetch_quota(&access_token, &email, Some(&id), account_type).await;
        results.push((email, res.map(|(q, _)| q)));
    }
    results
}

/// Get valid token (auto-refresh if expired)
pub async fn get_valid_token_for_warmup(
    account: &crate::models::account::Account,
) -> Result<(String, String), String> {
    let mut account = account.clone();

    // Check and auto-refresh token
    let new_token = crate::modules::oauth::ensure_fresh_token(
        &account.token,
        Some(&account.id),
        account.account_type,
    )
    .await?;

    // If token changed (meant refreshed), save it
    if new_token.access_token != account.token.access_token {
        account.token = new_token;
        if let Err(e) = crate::modules::account::save_account(&account) {
            crate::modules::logger::log_warn(&format!(
                "[Warmup] Failed to save refreshed token: {}",
                e
            ));
        } else {
            crate::modules::logger::log_info(&format!(
                "[Warmup] Successfully refreshed and saved new token for {}",
                account.email
            ));
        }
    }

    // Fetch project_id
    let (project_id, _) = fetch_project_id(
        &account.token.access_token,
        &account.email,
        Some(&account.id),
        account.account_type,
    )
    .await;
    let final_pid = if let Some(pid) = project_id {
        pid
    } else if account.account_type == crate::models::AccountType::GeminiCli {
        return Err("[Warmup] GeminiCLI account missing project_id after resolver".to_string());
    } else {
        "bamboo-precept-lgxtn".to_string()
    };

    Ok((account.token.access_token, final_pid))
}

/// Send warmup request via proxy internal API
pub async fn warmup_model_directly(
    access_token: &str,
    model_name: &str,
    project_id: &str,
    email: &str,
    percentage: i32,
    _account_id: Option<&str>,
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

    // Use a no-proxy client for local loopback requests
    // This prevents Docker environments from routing localhost through external proxies
    let client = rquest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .no_proxy()
        .build()
        .unwrap_or_else(|_| rquest::Client::new());
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
                crate::modules::logger::log_info(&format!(
                    "[Warmup] ✓ Triggered {} for {} (was {}%)",
                    model_name, email, percentage
                ));
                true
            } else {
                let text = response.text().await.unwrap_or_default();
                crate::modules::logger::log_warn(&format!(
                    "[Warmup] ✗ {} for {} (was {}%): HTTP {} - {}",
                    model_name, email, percentage, status, text
                ));
                false
            }
        }
        Err(e) => {
            crate::modules::logger::log_warn(&format!(
                "[Warmup] ✗ {} for {} (was {}%): {}",
                model_name, email, percentage, e
            ));
            false
        }
    }
}

/// Smart warmup for all accounts
pub async fn warm_up_all_accounts() -> Result<String, String> {
    let mut retry_count = 0;

    loop {
        let all_accounts = crate::modules::account::list_accounts().unwrap_or_default();
        // [FIX] 过滤掉禁用反代的账号
        let target_accounts: Vec<_> = all_accounts
            .into_iter()
            .filter(|a| !a.disabled && !a.proxy_disabled)
            .collect();

        if target_accounts.is_empty() {
            return Ok("No accounts available".to_string());
        }

        crate::modules::logger::log_info(&format!(
            "[Warmup] Screening models for {} accounts...",
            target_accounts.len()
        ));

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
                    let quota = fetch_quota_with_cache(
                        &token,
                        &account.email,
                        Some(&pid),
                        Some(&account.id),
                        account.account_type,
                    )
                    .await
                    .ok();
                    Some((account.id.clone(), account.email.clone(), token, pid, quota))
                });
                handles.push(handle);
            }

            for handle in handles {
                if let Ok(Some((id, email, token, pid, Some((fresh_quota, _))))) = handle.await {
                    // [FIX] 预热阶段检测到 403 时，使用统一禁用逻辑，确保账号文件和索引同时更新
                    if fresh_quota.is_forbidden {
                        crate::modules::logger::log_warn(&format!(
                            "[Warmup] Account {} returned 403 Forbidden during quota fetch, marking as forbidden",
                            email
                        ));
                        let _ = crate::modules::account::mark_account_forbidden(
                            &id,
                            "Warmup: 403 Forbidden - quota fetch denied",
                        );
                        continue;
                    }
                    let mut account_warmed_series = std::collections::HashSet::new();
                    for m in fresh_quota.models {
                        if m.percentage >= 100 {
                            let model_to_ping = m.name.clone();

                            // Removed hardcoded whitelist - now warms up any model at 100%
                            if !account_warmed_series.contains(&model_to_ping) {
                                warmup_items.push((
                                    id.clone(),
                                    email.clone(),
                                    model_to_ping.clone(),
                                    token.clone(),
                                    pid.clone(),
                                    m.percentage,
                                ));
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
            warmup_items.retain(|(_, email, model, _, _, _)| {
                let history_key = format!("{}:{}:100", email, model);
                !crate::modules::scheduler::check_cooldown(&history_key, 14400)
            });

            if warmup_items.is_empty() {
                let skipped = total_before;
                crate::modules::logger::log_info(&format!(
                    "[Warmup] Returning to frontend: All models in cooldown, skipped {}",
                    skipped
                ));
                return Ok(format!(
                    "All models are in cooldown, skipped {} items",
                    skipped
                ));
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

                    for (id, email, model, token, pid, pct) in batch.iter() {
                        let id = id.clone();
                        let email = email.clone();
                        let model = model.clone();
                        let token = token.clone();
                        let pid = pid.clone();
                        let pct = *pct;

                        let handle = tokio::spawn(async move {
                            let result =
                                warmup_model_directly(&token, &model, &pid, &email, pct, Some(&id))
                                    .await;
                            (result, email, model)
                        });
                        handles.push(handle);
                    }

                    for handle in handles {
                        match handle.await {
                            Ok((true, email, model)) => {
                                success += 1;
                                let history_key = format!("{}:{}:100", email, model);
                                crate::modules::scheduler::record_warmup_history(
                                    &history_key,
                                    now_ts,
                                );
                            }
                            _ => {}
                        }
                    }

                    if batch_idx < (warmup_items.len() + batch_size - 1) / batch_size - 1 {
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    }
                }

                crate::modules::logger::log_info(&format!(
                    "[Warmup] Warmup task completed: success {}/{}",
                    success, total
                ));
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                let _ = crate::modules::account::refresh_all_quotas_logic().await;
            });
            crate::modules::logger::log_info(&format!(
                "[Warmup] Returning to frontend: Warmup task triggered for {} models",
                total
            ));
            return Ok(format!("Warmup task triggered for {} models", total));
        }

        if has_near_ready_models && retry_count < MAX_RETRIES {
            retry_count += 1;
            crate::modules::logger::log_info(&format!(
                "[Warmup] Critical recovery model detected, waiting {}s to retry ({}/{})",
                RETRY_DELAY_SECS, retry_count, MAX_RETRIES
            ));
            tokio::time::sleep(tokio::time::Duration::from_secs(RETRY_DELAY_SECS)).await;
            continue;
        }

        return Ok("No models need warmup".to_string());
    }
}

/// Warmup for single account
pub async fn warm_up_account(account_id: &str) -> Result<String, String> {
    let accounts = crate::modules::account::list_accounts().unwrap_or_default();
    let account_owned = accounts
        .iter()
        .find(|a| a.id == account_id)
        .cloned()
        .ok_or_else(|| "Account not found".to_string())?;

    if account_owned.disabled || account_owned.proxy_disabled {
        return Err("Account is disabled".to_string());
    }

    let email = account_owned.email.clone();
    let (token, pid) = get_valid_token_for_warmup(&account_owned).await?;
    let (fresh_quota, _) = fetch_quota_with_cache(
        &token,
        &email,
        Some(&pid),
        Some(&account_owned.id),
        account_owned.account_type,
    )
    .await
    .map_err(|e| format!("Failed to fetch quota: {}", e))?;

    // [FIX] 预热阶段检测到 403 时，使用统一的 mark_account_forbidden 逻辑，
    // 确保账号文件和索引文件同时更新，且前端刷新后能感知到禁用状态
    if fresh_quota.is_forbidden {
        crate::modules::logger::log_warn(&format!(
            "[Warmup] Account {} returned 403 Forbidden during quota fetch, marking as forbidden",
            email
        ));
        let reason = "Warmup: 403 Forbidden - quota fetch denied";
        let _ = crate::modules::account::mark_account_forbidden(account_id, reason);
        return Err("Account is forbidden (403)".to_string());
    }

    let mut models_to_warm = Vec::new();
    let mut warmed_series = std::collections::HashSet::new();

    for m in fresh_quota.models {
        if m.percentage >= 100 {
            let model_name = m.name.clone();

            // Removed hardcoded whitelist - now warms up any model at 100%
            if !warmed_series.contains(&model_name) {
                models_to_warm.push((model_name.clone(), m.percentage));
                warmed_series.insert(model_name);
            }
        }
    }

    if models_to_warm.is_empty() {
        return Ok("No warmup needed".to_string());
    }

    let warmed_count = models_to_warm.len();
    let account_id_clone = account_id.to_string();

    tokio::spawn(async move {
        for (name, pct) in models_to_warm {
            if warmup_model_directly(&token, &name, &pid, &email, pct, Some(&account_id_clone))
                .await
            {
                let history_key = format!("{}:{}:100", email, name);
                let now_ts = chrono::Utc::now().timestamp();
                crate::modules::scheduler::record_warmup_history(&history_key, now_ts);
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
        let _ = crate::modules::account::refresh_all_quotas_logic().await;
    });

    Ok(format!(
        "Successfully triggered warmup for {} model series",
        warmed_count
    ))
}
