pub mod fingerprint;

use crate::types::{ModelQuota, QuotaSummary};
use anyhow::{anyhow, Result};
use serde_json::Value;

/// Google 官方三大内部控制面端点 (按官方故障自愈优先级排序: Daily -> Sandbox -> Prod)
const LOAD_PROJECT_ENDPOINTS: [&str; 3] = [
    "https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:loadCodeAssist",
    "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
];

const FETCH_MODELS_ENDPOINTS: [&str; 2] = [
    "https://daily-cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels",
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:fetchAvailableModels",
];

const USER_QUOTA_SUMMARY_ENDPOINTS: [&str; 2] = [
    "https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:retrieveUserQuotaSummary",
];

pub struct QuotaClient {
    client: reqwest::Client,
    ua: String,
}

impl QuotaClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            ua: fingerprint::get_official_user_agent(),
        }
    }

    /// 第一步：通过 loadCodeAssist 拉取账号激活的项目 Project ID (原生应用必备前置步骤)
    pub async fn fetch_project_id(&self, access_token: &str) -> Option<String> {
        let meta = serde_json::json!({ "metadata": { "ideType": "ANTIGRAVITY" } });
        for endpoint in LOAD_PROJECT_ENDPOINTS {
            if let Ok(res) = self
                .client
                .post(endpoint)
                .bearer_auth(access_token)
                .header(reqwest::header::USER_AGENT, &self.ua)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .json(&meta)
                .send()
                .await
            {
                if res.status().is_success() {
                    if let Ok(val) = res.json::<Value>().await {
                        if let Some(pid) = val.get("cloudaicompanionProject").and_then(|p| p.as_str()) {
                            return Some(pid.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    /// 第二步：查询用户当前详细配额 (优先带 Project 参数，并对齐官方端点与响应结构)
    pub async fn fetch_quota(&self, access_token: &str, project_id: Option<&str>) -> Result<QuotaSummary> {
        let payload = if let Some(pid) = project_id {
            serde_json::json!({ "project": pid })
        } else {
            serde_json::json!({})
        };

        // 1. 尝试从 retrieveUserQuotaSummary 查询（主要额度源）
        for endpoint in USER_QUOTA_SUMMARY_ENDPOINTS {
            let res = self
                .client
                .post(endpoint)
                .bearer_auth(access_token)
                .header(reqwest::header::USER_AGENT, &self.ua)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .json(&payload)
                .send()
                .await;

            if let Ok(resp) = res {
                let status = resp.status();
                if status.is_success() {
                    let val: Value = resp.json().await?;
                    return Ok(parse_quota_summary_response(&val));
                } else if status == reqwest::StatusCode::FORBIDDEN {
                    let text = resp.text().await.unwrap_or_default();
                    return Err(anyhow!("Account Forbidden (403): {}", text));
                }
            }
        }

        // 2. 备用兜底：从 fetchAvailableModels 获取模型配额信息
        for endpoint in FETCH_MODELS_ENDPOINTS {
            let res = self
                .client
                .post(endpoint)
                .bearer_auth(access_token)
                .header(reqwest::header::USER_AGENT, &self.ua)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .json(&payload)
                .send()
                .await;

            if let Ok(resp) = res {
                if resp.status().is_success() {
                    let val: Value = resp.json().await?;
                    return Ok(parse_fetch_models_response(&val));
                }
            }
        }

        Err(anyhow!("Failed to retrieve quota from all upstream endpoints"))
    }
}

/// 解析 retrieveUserQuotaSummary 官方结构
fn parse_quota_summary_response(val: &Value) -> QuotaSummary {
    let mut models_list = Vec::new();
    let mut gemini_pct: Option<f64> = None;
    let mut claude_pct: Option<f64> = None;

    if let Some(groups) = val.get("groups").and_then(|g| g.as_array()) {
        for group in groups {
            if let Some(buckets) = group.get("buckets").and_then(|b| b.as_array()) {
                for bucket in buckets {
                    let name = bucket.get("displayName").and_then(|d| d.as_str()).unwrap_or_default();
                    let remaining_fraction = bucket.get("remainingFraction").and_then(|f| f.as_f64()).unwrap_or(0.0);
                    let percentage = remaining_fraction * 100.0;
                    let reset_time = bucket.get("resetTime").and_then(|r| r.as_str()).map(|s| s.to_string());

                    models_list.push(ModelQuota {
                        name: name.to_string(),
                        percentage,
                        reset_time,
                        display_name: Some(name.to_string()),
                    });

                    let lower = name.to_ascii_lowercase();
                    if lower.contains("gemini") || lower.contains("pro") || lower.contains("flash") {
                        gemini_pct = match gemini_pct {
                            Some(prev) => Some(prev.min(percentage)),
                            None => Some(percentage),
                        };
                    } else if lower.contains("claude") || lower.contains("anthropic") {
                        claude_pct = match claude_pct {
                            Some(prev) => Some(prev.min(percentage)),
                            None => Some(percentage),
                        };
                    }
                }
            }
        }
    }

    QuotaSummary {
        gemini_percentage: gemini_pct,
        claude_percentage: claude_pct,
        models: models_list,
        last_updated: Some(chrono::Utc::now().timestamp()),
    }
}

/// 解析 fetchAvailableModels 官方结构
fn parse_fetch_models_response(val: &Value) -> QuotaSummary {
    let mut models_list = Vec::new();
    let mut gemini_pct: Option<f64> = None;
    let mut claude_pct: Option<f64> = None;

    if let Some(models) = val.get("models").and_then(|m| m.as_object()) {
        for (name, model_info) in models {
            let percentage = model_info
                .get("quotaInfo")
                .and_then(|q| q.get("remainingFraction"))
                .and_then(|f| f.as_f64())
                .map(|f| f * 100.0)
                .unwrap_or(100.0);

            let reset_time = model_info
                .get("quotaInfo")
                .and_then(|q| q.get("resetTime"))
                .and_then(|r| r.as_str())
                .map(|s| s.to_string());

            let display_name = model_info
                .get("displayName")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string());

            models_list.push(ModelQuota {
                name: name.clone(),
                percentage,
                reset_time,
                display_name,
            });

            let lower = name.to_ascii_lowercase();
            if lower.contains("flash") || lower.contains("pro") {
                gemini_pct = match gemini_pct {
                    Some(prev) => Some(prev.min(percentage)),
                    None => Some(percentage),
                };
            } else if lower.contains("claude") || lower.contains("anthropic") {
                claude_pct = match claude_pct {
                    Some(prev) => Some(prev.min(percentage)),
                    None => Some(percentage),
                };
            }
        }
    }

    QuotaSummary {
        gemini_percentage: gemini_pct,
        claude_percentage: claude_pct,
        models: models_list,
        last_updated: Some(chrono::Utc::now().timestamp()),
    }
}
