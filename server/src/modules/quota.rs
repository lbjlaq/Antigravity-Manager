use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::models::QuotaData;

const QUOTA_API_URL: &str = "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels";
const LOAD_PROJECT_API_URL: &str = "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist";
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
}

/// 创建配置好的 HTTP Client
fn create_client() -> reqwest::Client {
    crate::utils::http::create_client(15)
}

/// 获取 Project ID
async fn fetch_project_id(access_token: &str) -> Option<String> {
    let client = create_client();
    let body = json!({
        "metadata": {
            "ideType": "ANTIGRAVITY"
        }
    });

    // 简单的重试
    for _ in 0..2 {
        match client
            .post(LOAD_PROJECT_API_URL)
            .bearer_auth(access_token)
            .header("User-Agent", USER_AGENT)
            .json(&body)
            .send()
            .await 
        {
            Ok(res) => {
                if res.status().is_success() {
                    if let Ok(data) = res.json::<LoadProjectResponse>().await {
                        return data.project_id;
                    }
                }
            }
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }
    None
}

/// 查询账号配额
pub async fn fetch_quota(access_token: &str) -> crate::error::AppResult<QuotaData> {
    use crate::error::AppError;
    crate::modules::logger::log_info("开始外部查询配额...");
    let client = create_client();
    
    // 1. 获取 Project ID
    let project_id = fetch_project_id(access_token).await;
    crate::modules::logger::log_info(&format!("Project ID 获取结果: {:?}", project_id));
    
    // 2. 构建请求体
    let mut payload = serde_json::Map::new();
    if let Some(pid) = project_id {
        payload.insert("project".to_string(), json!(pid));
    }
    
    let url = QUOTA_API_URL;
    let max_retries = 3;
    let mut last_error: Option<AppError> = None;

    crate::modules::logger::log_info(&format!("发送配额请求至 {}", url));

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
                        return Ok(q);
                    }
                    
                    // 其他错误继续重试逻辑
                    if attempt < max_retries {
                         let text = response.text().await.unwrap_or_default();
                         crate::modules::logger::log_warn(&format!("API 错误: {} - {} (尝试 {}/{})", status, text, attempt, max_retries));
                         last_error = Some(AppError::Unknown(format!("HTTP {} - {}", status, text)));
                         tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                         continue;
                    } else {
                         let text = response.text().await.unwrap_or_default();
                         return Err(AppError::Unknown(format!("API 错误: {} - {}", status, text)));
                    }
                }

                let quota_response: QuotaResponse = response
                    .json()
                    .await
                    .map_err(|e| AppError::Network(e.to_string()))?;
                
                let mut quota_data = QuotaData::new();
                
                crate::modules::logger::log_info(&format!("Quota API 返回了 {} 个模型:", quota_response.models.len()));

                for (name, info) in quota_response.models {
                    crate::modules::logger::log_info(&format!("   - {}", name));
                    if let Some(quota_info) = info.quota_info {
                        let percentage = quota_info.remaining_fraction
                            .map(|f| (f * 100.0) as i32)
                            .unwrap_or(0);
                        
                        let reset_time = quota_info.reset_time.unwrap_or_default();
                        
                        // 只保存我们关心的模型
                        if name.contains("gemini") || name.contains("claude") {
                            quota_data.add_model(name, percentage, reset_time);
                        }
                    }
                }
                
                return Ok(quota_data);
            },
            Err(e) => {
                crate::modules::logger::log_warn(&format!("请求失败: {} (尝试 {}/{})", e, attempt, max_retries));
                last_error = Some(AppError::Network(e.to_string()));
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
pub async fn fetch_all_quotas(accounts: Vec<(String, String)>) -> Vec<(String, crate::error::AppResult<QuotaData>)> {
    let mut results = Vec::new();
    
    for (account_id, access_token) in accounts {
        let result = fetch_quota(&access_token).await;
        results.push((account_id, result));
    }
    
    results
}
