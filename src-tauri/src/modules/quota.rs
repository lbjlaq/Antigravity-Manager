use crate::models::QuotaData;
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::json;

const QUOTA_API_URL: &str = "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels";
const USER_AGENT: &str = "antigravity/1.11.3 Darwin/arm64";

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

/// 创建配置好的 HTTP Client
fn create_client() -> reqwest::Client {
    crate::utils::http::create_client(15)
}

const CLOUD_CODE_BASE_URL: &str = "https://cloudcode-pa.googleapis.com";

/// 获取项目 ID 和订阅类型
async fn fetch_project_id(access_token: &str, email: &str) -> (Option<String>, Option<String>) {
    let client = create_client();
    let meta = json!({"metadata": {"ideType": "ANTIGRAVITY"}});

    let res = client
        .post(format!("{}/v1internal:loadCodeAssist", CLOUD_CODE_BASE_URL))
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", access_token),
        )
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::USER_AGENT, "antigravity/windows/amd64")
        .json(&meta)
        .send()
        .await;

    match res {
        Ok(res) => {
            if res.status().is_success() {
                if let Ok(data) = res.json::<LoadProjectResponse>().await {
                    let project_id = data.project_id.clone();

                    // 核心逻辑：优先从 paid_tier 获取订阅 ID，这比 current_tier 更能反映真实账户权益
                    let subscription_tier = data
                        .paid_tier
                        .and_then(|t| t.id)
                        .or_else(|| data.current_tier.and_then(|t| t.id));

                    if let Some(ref tier) = subscription_tier {
                        crate::modules::logger::log_info(&format!(
                            "📊 [{}] 订阅识别成功: {}",
                            email, tier
                        ));
                    }

                    return (project_id, subscription_tier);
                }
            } else {
                crate::modules::logger::log_warn(&format!(
                    "⚠️  [{}] loadCodeAssist 失败: Status: {}",
                    email,
                    res.status()
                ));
            }
        }
        Err(e) => {
            crate::modules::logger::log_error(&format!(
                "❌ [{}] loadCodeAssist 网络错误: {}",
                email, e
            ));
        }
    }

    (None, None)
}

/// 查询账号配额的统一入口
pub async fn fetch_quota(
    access_token: &str,
    email: &str,
) -> crate::error::AppResult<(QuotaData, Option<String>)> {
    fetch_quota_inner(access_token, email).await
}

/// 查询账号配额逻辑
pub async fn fetch_quota_inner(
    access_token: &str,
    email: &str,
) -> crate::error::AppResult<(QuotaData, Option<String>)> {
    use crate::error::AppError;
    // crate::modules::logger::log_info(&format!("[{}] 开始外部查询配额...", email));

    // 1. 获取 Project ID 和订阅类型
    let (project_id, subscription_tier) = fetch_project_id(access_token, email).await;

    let final_project_id = project_id.as_deref().unwrap_or("bamboo-precept-lgxtn");

    let client = create_client();
    let payload = json!({
        "project": final_project_id
    });

    let url = QUOTA_API_URL;
    let max_retries = 3;
    let mut last_error: Option<AppError> = None;

    for attempt in 1..=max_retries {
        match client
            .post(url)
            .bearer_auth(access_token)
            .header("User-Agent", USER_AGENT)
            .json(&json!(payload))
            .send()
            .await
        {
            Ok(response) => {
                // 将 HTTP 错误状态转换为 AppError
                if let Err(_) = response.error_for_status_ref() {
                    let status = response.status();

                    // ✅ 特殊处理 403 Forbidden - 直接返回,不重试
                    if status == reqwest::StatusCode::FORBIDDEN {
                        crate::modules::logger::log_warn(&format!(
                            "账号无权限 (403 Forbidden),标记为 forbidden 状态"
                        ));
                        let mut q = QuotaData::new();
                        q.is_forbidden = true;
                        q.subscription_tier = subscription_tier.clone();
                        return Ok((q, project_id.clone()));
                    }

                    // 其他错误继续重试逻辑
                    if attempt < max_retries {
                        let text = response.text().await.unwrap_or_default();
                        crate::modules::logger::log_warn(&format!(
                            "API 错误: {} - {} (尝试 {}/{})",
                            status, text, attempt, max_retries
                        ));
                        last_error = Some(AppError::Unknown(format!("HTTP {} - {}", status, text)));
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    } else {
                        let text = response.text().await.unwrap_or_default();
                        return Err(AppError::Unknown(format!(
                            "API 错误: {} - {}",
                            status, text
                        )));
                    }
                }

                let quota_response: QuotaResponse =
                    response.json().await.map_err(|e| AppError::Network(e))?;

                let mut quota_data = QuotaData::new();

                // 使用 debug 级别记录详细信息，避免控制台噪音
                tracing::debug!("Quota API 返回了 {} 个模型", quota_response.models.len());

                for (name, info) in quota_response.models {
                    if let Some(quota_info) = info.quota_info {
                        let percentage = quota_info
                            .remaining_fraction
                            .map(|f| (f * 100.0) as i32)
                            .unwrap_or(0);

                        let reset_time = quota_info.reset_time.unwrap_or_default();

                        // 只保存我们关心的模型
                        if name.contains("gemini") || name.contains("claude") {
                            quota_data.add_model(name, percentage, reset_time);
                        }
                    }
                }

                // 设置订阅类型
                quota_data.subscription_tier = subscription_tier.clone();

                return Ok((quota_data, project_id.clone()));
            }
            Err(e) => {
                crate::modules::logger::log_warn(&format!(
                    "请求失败: {} (尝试 {}/{})",
                    e, attempt, max_retries
                ));
                last_error = Some(AppError::Network(e));
                if attempt < max_retries {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| AppError::Unknown("配额查询失败".to_string())))
}

/// 批量查询所有账号配额 (备用功能)
#[allow(dead_code)]
pub async fn fetch_all_quotas(
    accounts: Vec<(String, String)>,
) -> Vec<(String, crate::error::AppResult<QuotaData>)> {
    let mut results = Vec::new();

    for (account_id, access_token) in accounts {
        // 在批量查询中，我们将 account_id 传入以供日志标识
        let result = fetch_quota(&access_token, &account_id)
            .await
            .map(|(q, _)| q);
        results.push((account_id, result));
    }

    results
}

/// 一键预热所有账号 - 触发5小时配额恢复周期
pub async fn warm_up_all_accounts() -> Result<String, String> {
    let accounts =
        crate::modules::account::list_accounts().map_err(|e| format!("加载账号失败: {}", e))?;

    if accounts.is_empty() {
        return Err("没有可用账号".to_string());
    }

    // Filter Pro/Ultra accounts
    let pro_ultra_accounts: Vec<_> = accounts
        .into_iter()
        .filter(|a| {
            let tier = a
                .quota
                .as_ref()
                .and_then(|q| q.subscription_tier.as_ref())
                .map(|s| s.to_lowercase())
                .unwrap_or_default();
            tier.contains("pro") || tier.contains("ultra")
        })
        .collect();

    if pro_ultra_accounts.is_empty() {
        return Err("没有 Pro/Ultra 账号".to_string());
    }

    let upstream = std::sync::Arc::new(crate::proxy::upstream::client::UpstreamClient::new(None));
    let total_tasks = pro_ultra_accounts.len() * 4; // Estimate 4 models per account
    let (tx, mut _rx) = tokio::sync::mpsc::channel(total_tasks);

    for account in pro_ultra_accounts {
        let access_token = account.token.access_token.clone();
        let upstream = upstream.clone();
        let tx = tx.clone();
        let project_id = "bamboo-precept-lgxtn"; // Hardcoded default

        // Dynamic Warm-up: Iterate over available models in quota
        let mut models_to_warm = Vec::new();
        if let Some(quota) = &account.quota {
            for m in &quota.models {
                models_to_warm.push(m.name.clone());
            }
        }

        if models_to_warm.is_empty() {
            models_to_warm = vec![
                "gemini-3-pro-high".to_string(),
                "gemini-3-flash".to_string(),
                "gemini-3-pro-image".to_string(),
                "claude-sonnet-4-5-thinking".to_string(),
            ];
        }

        for model_name in models_to_warm {
            // Skip image models - warmup consumes too much quota (10%+)
            if model_name.to_lowercase().contains("image") {
                tracing::info!(
                    "[Warmup] Skipping image model {} (quota-expensive)",
                    model_name
                );
                continue;
            }

            let at = access_token.clone();
            let up = upstream.clone();
            let txc = tx.clone();
            let m_name = model_name.clone();

            tokio::spawn(async move {
                let is_image = m_name.to_lowercase().contains("image");

                let body = if is_image {
                    serde_json::json!({
                        "project": project_id,
                        "model": m_name,
                        "request": {
                            "contents": [{ "role": "user", "parts": [{ "text": "a single white pixel" }] }],
                            "generationConfig": {
                                "candidateCount": 1,
                                "imageConfig": {
                                    "aspectRatio": "1:1"
                                }
                            },
                            "safetySettings": [
                                { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "OFF" },
                                { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "OFF" },
                                { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "OFF" },
                                { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "OFF" },
                                { "category": "HARM_CATEGORY_CIVIC_INTEGRITY", "threshold": "OFF" }
                            ]
                        }
                    })
                } else {
                    serde_json::json!({
                        "project": project_id,
                        "model": m_name,
                        "request": {
                            "contents": [{ "role": "user", "parts": [{ "text": "." }] }],
                            "generationConfig": { "maxOutputTokens": 1 }
                        }
                    })
                };

                let res = up
                    .call_v1_internal("generateContent", &at, body, None)
                    .await;

                let _ = txc.send(format!("{}: {}", m_name, res.is_ok())).await;
            });
        }
    }

    Ok(format!("已启动智能预热任务"))
}

/// 单账号预热 - 触发指定账号的5小时配额恢复周期
pub async fn warm_up_account(account_id: &str) -> Result<String, String> {
    let accounts =
        crate::modules::account::list_accounts().map_err(|e| format!("加载账号失败: {}", e))?;

    let account = accounts
        .into_iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| "账号不存在".to_string())?;

    let upstream = std::sync::Arc::new(crate::proxy::upstream::client::UpstreamClient::new(None));
    let access_token = account.token.access_token.clone();
    let project_id = "bamboo-precept-lgxtn";

    // Dynamic Warm-up: Iterate over available models in quota
    let mut models_to_warm = Vec::new();
    if let Some(quota) = &account.quota {
        for m in &quota.models {
            models_to_warm.push(m.name.clone());
        }
    }

    if models_to_warm.is_empty() {
        models_to_warm = vec![
            "gemini-3-pro-high".to_string(),
            "gemini-3-flash".to_string(),
            "gemini-3-pro-image".to_string(),
            "claude-sonnet-4-5-thinking".to_string(),
        ];
    }

    for model_name in models_to_warm {
        // Skip image models - warmup consumes too much quota (10%+)
        if model_name.to_lowercase().contains("image") {
            tracing::info!(
                "[Warmup] Skipping image model {} (quota-expensive)",
                model_name
            );
            continue;
        }

        let at = access_token.clone();
        let up = upstream.clone();
        let m_name = model_name.clone();

        tokio::spawn(async move {
            let is_image = m_name.to_lowercase().contains("image");

            let body = if is_image {
                serde_json::json!({
                    "project": project_id,
                    "model": m_name,
                    "request": {
                        "contents": [{ "role": "user", "parts": [{ "text": "a single white pixel" }] }],
                        "generationConfig": {
                            "candidateCount": 1,
                            "imageConfig": { "aspectRatio": "1:1" }
                        },
                        "safetySettings": [
                            { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "OFF" },
                            { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "OFF" },
                            { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "OFF" },
                            { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "OFF" },
                            { "category": "HARM_CATEGORY_CIVIC_INTEGRITY", "threshold": "OFF" }
                        ]
                    }
                })
            } else {
                serde_json::json!({
                    "project": project_id,
                    "model": m_name,
                    "request": {
                        "contents": [{ "role": "user", "parts": [{ "text": "." }] }],
                        "generationConfig": { "maxOutputTokens": 1 }
                    }
                })
            };

            let _ = up
                .call_v1_internal("generateContent", &at, body, None)
                .await;
        });
    }

    Ok(format!("已启动账号预热"))
}
