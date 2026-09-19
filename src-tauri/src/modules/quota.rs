use crate::models::QuotaData;
use crate::modules::config;
use rquest;
use serde::{Deserialize, Serialize};
use serde_json::json;

// Quota API endpoints (fallback order: Sandbox → Daily → Prod)
const QUOTA_API_ENDPOINTS: [&str; 3] = [
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:fetchAvailableModels",
    "https://daily-cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels",
    "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels",
];

// Quota Summary API endpoints (weekly + 5h grouped quota, fallback order 同上)
const QUOTA_SUMMARY_ENDPOINTS: [&str; 3] = [
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:retrieveUserQuotaSummary",
    "https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
    "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
];

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

// ---- retrieveUserQuotaSummary 响应反序列化结构 ----

#[derive(Debug, Deserialize)]
struct QuotaSummaryResponse {
    groups: Vec<QuotaSummaryGroup>,
}

#[derive(Debug, Deserialize)]
struct QuotaSummaryGroup {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    description: Option<String>,
    buckets: Vec<QuotaSummaryBucket>,
}

#[derive(Debug, Deserialize)]
struct QuotaSummaryBucket {
    #[serde(rename = "bucketId")]
    bucket_id: Option<String>,
    window: Option<String>,
    #[serde(rename = "remainingFraction")]
    remaining_fraction: Option<f64>,
    #[serde(rename = "resetTime")]
    reset_time: Option<String>,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoadProjectResponse {
    #[serde(rename = "cloudaicompanionProject")]
    project_id: Option<String>,
    #[serde(rename = "currentTier")]
    current_tier: Option<Tier>,
    #[serde(rename = "paidTier")]
    paid_tier: Option<Tier>,
    #[serde(rename = "allowedTiers")]
    allowed_tiers: Option<Vec<Tier>>,
    #[serde(rename = "ineligibleTiers")]
    ineligible_tiers: Option<Vec<IneligibleTier>>,
}

#[derive(Debug, Deserialize)]
struct IneligibleTier {
    #[allow(dead_code)]
    #[serde(rename = "reasonCode")]
    reason_code: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Tier {
    #[allow(dead_code)]
    is_default: Option<bool>,
    id: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "quotaTier")]
    quota_tier: Option<String>,
    name: Option<String>,
    #[allow(dead_code)]
    slug: Option<String>,
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

const CLOUD_CODE_LOAD_PROJECT_ENDPOINTS: [&str; 3] = [
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:loadCodeAssist",
    "https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
    "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
];

/// Fetch project ID and subscription tier
async fn fetch_project_id(
    access_token: &str,
    email: &str,
    account_id: Option<&str>,
) -> (Option<String>, Option<String>) {
    let client = create_standard_client(account_id).await;
    let meta = json!({"metadata": {"ideType": "ANTIGRAVITY"}});

    for (ep_idx, ep_url) in CLOUD_CODE_LOAD_PROJECT_ENDPOINTS.iter().enumerate() {
        let res = client
            .post(*ep_url)
            .header(
                rquest::header::AUTHORIZATION,
                format!("Bearer {}", access_token),
            )
            .header(rquest::header::CONTENT_TYPE, "application/json")
            .header(
                rquest::header::USER_AGENT,
                crate::constants::NATIVE_OAUTH_USER_AGENT.as_str(),
            )
            .json(&meta)
            .send()
            .await;

        match res {
            Ok(res) => {
                if res.status().is_success() {
                    if let Ok(data) = res.json::<LoadProjectResponse>().await {
                        let project_id = data.project_id.clone();

                        // 等级提取优先级：
                        // 1. 优先取 paid_tier：若有付费信息（g1-pro-tier / g1-ultra-tier），直接作为权威付费等级；
                        // 2. 其次取 current_tier：若当前生效档位为 free-tier，则为 FREE；
                        // 3. 再次检查 allowed_tiers：若包含且仅能用 free-tier，则为 FREE；
                        // 4. 若 paid_tier 与 current_tier 均为 null（如地理位置受限、受限账号），则权威判为 FREE，绝不误判为 PRO。
                        let raw_tier = data
                            .paid_tier
                            .as_ref()
                            .and_then(|t| t.id.clone().or_else(|| t.name.clone()))
                            .or_else(|| {
                                data.current_tier
                                    .as_ref()
                                    .and_then(|t| t.id.clone().or_else(|| t.name.clone()))
                            })
                            .or_else(|| {
                                data.allowed_tiers.as_ref().and_then(|allowed| {
                                    allowed
                                        .iter()
                                        .find(|t| {
                                            t.id.as_deref() == Some("free-tier")
                                                || t.is_default == Some(true)
                                        })
                                        .and_then(|t| t.id.clone().or_else(|| t.name.clone()))
                                })
                            })
                            .unwrap_or_else(|| "free-tier".to_string());

                        let subscription_tier = {
                            let normalized =
                                crate::models::quota::normalize_subscription_tier(&raw_tier);
                            if crate::models::quota::is_known_tier(&normalized) {
                                Some(normalized)
                            } else {
                                // standard-tier 或其他未知档位安全归入 FREE
                                Some("FREE".to_string())
                            }
                        };

                        if let Some(ref tier) = subscription_tier {
                            crate::modules::logger::log_info(&format!(
                                "📊 [{}] Subscription identified successfully: {}",
                                email, tier
                            ));
                        }

                        if ep_idx > 0 {
                            crate::modules::logger::log_info(&format!(
                                "loadCodeAssist fallback succeeded at endpoint #{}",
                                ep_idx + 1
                            ));
                        }

                        return (project_id, subscription_tier);
                    }
                } else {
                    crate::modules::logger::log_warn(&format!(
                        "⚠️  [{}] loadCodeAssist failed at {}: Status: {}",
                        email,
                        ep_url,
                        res.status()
                    ));
                    continue;
                }
            }
            Err(e) => {
                crate::modules::logger::log_error(&format!(
                    "❌ [{}] loadCodeAssist network error at {}: {}",
                    email, ep_url, e
                ));
                continue;
            }
        }
    }

    (None, None)
}

/// Unified entry point for fetching account quota
pub async fn fetch_quota(
    access_token: &str,
    email: &str,
    account_id: Option<&str>,
) -> crate::error::AppResult<(QuotaData, Option<String>)> {
    fetch_quota_with_cache(access_token, email, None, account_id).await
}

/// Fetch quota with cache support
pub async fn fetch_quota_with_cache(
    access_token: &str,
    email: &str,
    cached_project_id: Option<&str>,
    account_id: Option<&str>,
) -> crate::error::AppResult<(QuotaData, Option<String>)> {
    use crate::error::AppError;

    // `loadCodeAssist` 是订阅等级的唯一权威来源，必须每次都调用。
    //
    // 历史实现为了「省一次 API 调用」，在 project_id 已缓存且账号已有 tier 时直接跳过
    // loadCodeAssist。后果是：一个账号一旦被写入错误的 tier，就再也不可能被纠正
    // —— 这正是「免费账号被标记成 Pro 后永远是 Pro」无法自愈的原因。
    //
    // 而实测三个接口（fetchAvailableModels / retrieveUserQuotaSummary / loadCodeAssist）
    // 都完全忽略 project 字段，缓存 project_id 本身不带来任何收益，
    // 这个「优化」只剩副作用，因此移除。
    //
    // 现在：始终调用；仅在上游返回空值时用缓存/已存值兜底，避免网络抖动把数据抹掉。
    let (fresh_project_id, fresh_tier) = fetch_project_id(access_token, email, account_id).await;

    let project_id = fresh_project_id.or_else(|| cached_project_id.map(|s| s.to_string()));

    let existing_tier = account_id
        .and_then(|id| crate::modules::load_account(id).ok())
        .and_then(|acc| acc.quota.and_then(|q| q.subscription_tier));

    // 上游值优先；上游这次没给（网络失败 / 未识别）才保留旧值。
    let subscription_tier = fresh_tier.or(existing_tier);

    // We keep project_id to store in the DB, but we NO LONGER force inject it into payload if it's absent

    let client = create_standard_client(account_id).await;
    let payload = if let Some(ref pid) = project_id {
        json!({ "project": pid })
    } else {
        json!({}) // Empty payload fallback
    };

    let mut last_error: Option<AppError> = None;

    for (ep_idx, ep_url) in QUOTA_API_ENDPOINTS.iter().enumerate() {
        let has_next = ep_idx + 1 < QUOTA_API_ENDPOINTS.len();

        match client
            .post(*ep_url)
            .bearer_auth(access_token)
            .header(
                rquest::header::USER_AGENT,
                crate::constants::NATIVE_OAUTH_USER_AGENT.as_str(),
            )
            .json(&payload)
            .send()
            .await
        {
            Ok(response) => {
                // Convert HTTP error status to AppError
                if let Err(_) = response.error_for_status_ref() {
                    let status = response.status();

                    // 403 说明该账号本身不可用（未验证 / 被限制）。
                    //
                    // 这里曾经有一段「剥离 project 后重试」的逻辑，现已删除：
                    // 实测上游完全忽略 project 字段，剥离后仍是同一个 403，
                    // 只会白白多打一次请求、拖慢判定。
                    if status == rquest::StatusCode::FORBIDDEN {
                        crate::modules::logger::log_warn(&format!(
                            "Account unauthorized (403 Forbidden), marking as forbidden"
                        ));
                        let mut q = QuotaData::new();
                        q.is_forbidden = true;
                        // 保留本次 loadCodeAssist 拿到的等级；拿不到才回退到已存值
                        q.subscription_tier = subscription_tier.clone();
                        return Ok((q, project_id.clone()));
                    }

                    let text = response.text().await.unwrap_or_default();

                    // 429/5xx: fallback to next endpoint
                    if has_next
                        && (status == rquest::StatusCode::TOO_MANY_REQUESTS
                            || status.is_server_error())
                    {
                        crate::modules::logger::log_warn(&format!(
                            "Quota API {} returned {}, falling back to next endpoint",
                            ep_url, status
                        ));
                        last_error = Some(AppError::Unknown(format!("HTTP {} - {}", status, text)));
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue; // 换下一个 endpoint
                    }

                    return Err(AppError::Unknown(format!(
                        "API Error: {} - {}",
                        status, text
                    )));
                }

                if ep_idx > 0 {
                    crate::modules::logger::log_info(&format!(
                        "Quota API fallback succeeded at endpoint #{}",
                        ep_idx + 1
                    ));
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
                        // Register forwarding rules (including those mapping to gemini-pro-agent)
                        quota_data
                            .model_forwarding_rules
                            .insert(old_id, info.new_model_id);
                    }
                }

                // 归一化订阅等级。注意：**不再**用模型列表做兜底推断
                // （fetchAvailableModels 对免费号和 Pro 号返回完全相同的全量目录）。
                let final_tier =
                    crate::models::quota::resolve_subscription_tier(subscription_tier.as_deref());
                quota_data.subscription_tier = Some(final_tier);

                // Best-effort: fetch grouped quota summary (weekly + 5h windows).
                // Failure here must not block the primary quota result.
                let quota_groups =
                    fetch_quota_summary(access_token, email, project_id.as_deref(), account_id)
                        .await;

                // [FIX #3426] Fuse real bucket quotas into models so UI doesn't show fake 100%
                if let Some(ref groups) = quota_groups {
                    for model in quota_data.models.iter_mut() {
                        let name_lower = model.name.to_lowercase();
                        let is_claude_or_gpt =
                            name_lower.starts_with("claude") || name_lower.starts_with("gpt");
                        let is_gemini = name_lower.starts_with("gemini");

                        for group in groups {
                            let gname = group.display_name.to_lowercase();
                            let matches_group = if is_claude_or_gpt {
                                gname.contains("claude")
                                    || gname.contains("gpt")
                                    || gname.contains("3p")
                            } else if is_gemini {
                                gname.contains("gemini")
                                    || (!gname.contains("claude")
                                        && !gname.contains("gpt")
                                        && !gname.contains("3p"))
                            } else {
                                false
                            };

                            if matches_group {
                                // 找到 5h 桶和 weekly 桶，综合计算受限程度最大的实际可用配额
                                let bucket_5h = group.buckets.iter().find(|b| {
                                    let win = b.window.to_lowercase();
                                    let bid = b.bucket_id.to_lowercase();
                                    win.contains("5h")
                                        || bid.contains("5h")
                                        || win.contains("hour")
                                        || bid.contains("hour")
                                });
                                let bucket_weekly = group.buckets.iter().find(|b| {
                                    let win = b.window.to_lowercase();
                                    let bid = b.bucket_id.to_lowercase();
                                    win.contains("week")
                                        || bid.contains("week")
                                        || win.contains("7d")
                                        || bid.contains("7d")
                                });

                                let chosen_bucket = match (bucket_5h, bucket_weekly) {
                                    (Some(h), Some(w)) => {
                                        // 若周配额耗尽 (<= 0.001)，模型直接受限于周配额，重置时间使用周重置
                                        if w.remaining_fraction <= 0.001 {
                                            Some(w)
                                        } else if h.remaining_fraction <= w.remaining_fraction {
                                            Some(h)
                                        } else {
                                            Some(w)
                                        }
                                    }
                                    (Some(h), None) => Some(h),
                                    (None, Some(w)) => Some(w),
                                    _ => group.buckets.first(),
                                };

                                if let Some(b) = chosen_bucket {
                                    model.percentage =
                                        (b.remaining_fraction * 100.0).round() as i32;
                                    if !b.reset_time.is_empty() {
                                        model.reset_time = b.reset_time.clone();
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }

                quota_data.quota_groups = quota_groups;

                return Ok((quota_data, project_id.clone()));
            }
            Err(e) => {
                crate::modules::logger::log_warn(&format!(
                    "Quota API request failed at {}: {}",
                    ep_url, e
                ));
                last_error = Some(AppError::from(e));
                if has_next {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
                continue; // 换下一个 endpoint
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        AppError::Unknown("Quota fetch failed: all endpoints exhausted".to_string())
    }))
}

/// Fetch grouped quota summary (weekly + 5h windows) via retrieveUserQuotaSummary.
///
/// Best-effort: returns `None` on any failure so that the primary 5h quota fetch
/// (fetchAvailableModels) is never blocked by this auxiliary endpoint.
async fn fetch_quota_summary(
    access_token: &str,
    email: &str,
    project_id: Option<&str>,
    account_id: Option<&str>,
) -> Option<Vec<crate::models::quota::QuotaGroup>> {
    let client = create_standard_client(account_id).await;
    let payload = if let Some(pid) = project_id {
        json!({ "project": pid })
    } else {
        json!({})
    };

    for ep_url in QUOTA_SUMMARY_ENDPOINTS.iter() {
        let res = client
            .post(*ep_url)
            .bearer_auth(access_token)
            .header(
                rquest::header::USER_AGENT,
                crate::constants::NATIVE_OAUTH_USER_AGENT.as_str(),
            )
            .json(&payload)
            .send()
            .await;

        match res {
            Ok(response) => {
                let status = response.status();
                if !status.is_success() {
                    crate::modules::logger::log_warn(&format!(
                        "QuotaSummary API {} returned {}, trying next endpoint",
                        ep_url, status
                    ));
                    continue;
                }

                let summary: QuotaSummaryResponse = match response.json().await {
                    Ok(s) => s,
                    Err(e) => {
                        crate::modules::logger::log_warn(&format!(
                            "QuotaSummary JSON parse failed for {}: {}",
                            email, e
                        ));
                        return None;
                    }
                };

                let groups: Vec<crate::models::quota::QuotaGroup> = summary
                    .groups
                    .into_iter()
                    .map(|g| crate::models::quota::QuotaGroup {
                        display_name: g.display_name.unwrap_or_default(),
                        description: g.description,
                        buckets: g
                            .buckets
                            .into_iter()
                            .map(|b| crate::models::quota::QuotaBucket {
                                bucket_id: b.bucket_id.unwrap_or_default(),
                                window: b.window.unwrap_or_default(),
                                remaining_fraction: b.remaining_fraction.unwrap_or(0.0),
                                reset_time: b.reset_time.unwrap_or_default(),
                                display_name: b.display_name,
                                description: b.description,
                            })
                            .collect(),
                    })
                    .collect();

                tracing::debug!("[{}] QuotaSummary fetched {} groups", email, groups.len());
                return Some(groups);
            }
            Err(e) => {
                crate::modules::logger::log_warn(&format!(
                    "QuotaSummary API request failed at {}: {}",
                    ep_url, e
                ));
                continue;
            }
        }
    }

    None
}

/// Internal fetch quota logic
#[allow(dead_code)]
pub async fn fetch_quota_inner(
    access_token: &str,
    email: &str,
) -> crate::error::AppResult<(QuotaData, Option<String>)> {
    fetch_quota_with_cache(access_token, email, None, None).await
}

/// Batch fetch all account quotas (backup functionality)
#[allow(dead_code)]
pub async fn fetch_all_quotas(
    accounts: Vec<(String, String, String)>,
) -> Vec<(String, crate::error::AppResult<QuotaData>)> {
    let mut results = Vec::new();
    for (id, email, access_token) in accounts {
        let res = fetch_quota(&access_token, &email, Some(&id)).await;
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
    let new_token =
        crate::modules::oauth::ensure_fresh_token(&account.token, Some(&account.id)).await?;

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
    )
    .await;
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
    let (fresh_quota, _) =
        fetch_quota_with_cache(&token, &email, Some(&pid), Some(&account_owned.id))
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
