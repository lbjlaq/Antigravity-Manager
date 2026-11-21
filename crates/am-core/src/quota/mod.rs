pub mod fingerprint;

use crate::types::{ModelQuota, QuotaSummary};
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

/// 官方配额请求返回的结构化结果，包含可能的自愈诊断
#[derive(Debug)]
pub enum QuotaFetchResult {
    Success(QuotaSummary),
    /// 需要自动标记 Disable 的永久性异常 (如 401 token expired, 403 location, 403 subscription)
    NeedDisable { reason: String },
    /// 临时性网络异常
    TransientError(String),
}

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
                        if let Some(pid) = val
                            .get("cloudaicompanionProject")
                            .and_then(|v| v.as_str())
                        {
                            return Some(pid.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    /// 第二步：查询当前账号的配额明细（含错误模式自愈分析）
    pub async fn fetch_quota_safe(
        &self,
        access_token: &str,
        project_id: Option<&str>,
    ) -> QuotaFetchResult {
        let body = if let Some(pid) = project_id {
            serde_json::json!({ "project": pid })
        } else {
            serde_json::json!({})
        };

        let mut last_status = reqwest::StatusCode::OK;
        let mut last_err_body = String::new();

        for endpoint in USER_QUOTA_SUMMARY_ENDPOINTS {
            match self
                .client
                .post(endpoint)
                .bearer_auth(access_token)
                .header(reqwest::header::USER_AGENT, &self.ua)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .json(&body)
                .send()
                .await
            {
                Ok(res) => {
                    let status = res.status();
                    last_status = status;
                    if status.is_success() {
                        if let Ok(val) = res.json::<Value>().await {
                            let summary = Self::parse_quota_summary(&val);
                            return QuotaFetchResult::Success(summary);
                        }
                    } else {
                        let text = res.text().await.unwrap_or_default();
                        last_err_body = text;
                    }
                }
                Err(e) => {
                    return QuotaFetchResult::TransientError(format!("Network request failed: {}", e));
                }
            }
        }

        // 尝试 fallback: fetchAvailableModels
        for endpoint in FETCH_MODELS_ENDPOINTS {
            if let Ok(res) = self
                .client
                .post(endpoint)
                .bearer_auth(access_token)
                .header(reqwest::header::USER_AGENT, &self.ua)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .json(&body)
                .send()
                .await
            {
                if res.status().is_success() {
                    if let Ok(val) = res.json::<Value>().await {
                        let summary = Self::parse_quota_summary(&val);
                        return QuotaFetchResult::Success(summary);
                    }
                }
            }
        }

        // 诊断错误原因，判断是否需要标记为 Disable
        if last_status == reqwest::StatusCode::UNAUTHORIZED {
            return QuotaFetchResult::NeedDisable {
                reason: "Token expired or revoked by Google (401)".to_string(),
            };
        } else if last_status == reqwest::StatusCode::FORBIDDEN {
            if last_err_body.contains("UNSUPPORTED_LOCATION") {
                return QuotaFetchResult::NeedDisable {
                    reason: "Gemini Code Assist not available in your location (403)".to_string(),
                };
            } else if last_err_body.contains("SUBSCRIPTION_REQUIRED") {
                return QuotaFetchResult::NeedDisable {
                    reason: "Subscription or active billing required (403)".to_string(),
                };
            } else {
                return QuotaFetchResult::NeedDisable {
                    reason: "Google Account Forbidden (403)".to_string(),
                };
            }
        }

        QuotaFetchResult::TransientError(format!("Status {}: {}", last_status, last_err_body))
    }

    /// 解析 Google 配额响应 JSON
    fn parse_quota_summary(val: &Value) -> QuotaSummary {
        let mut models = Vec::new();
        let mut gemini_pct: Option<f64> = None;
        let mut claude_pct: Option<f64> = None;

        if let Some(arr) = val.get("models").and_then(|v| v.as_array()) {
            for item in arr {
                let name = item.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                let percentage = item.get("percentage").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let reset_time = item.get("resetTime").and_then(|v| v.as_str()).map(|s| s.to_string());
                let display_name = item.get("displayName").and_then(|v| v.as_str()).map(|s| s.to_string());

                let lower = name.to_lowercase();
                if lower.contains("claude") {
                    claude_pct = Some(claude_pct.map_or(percentage, |prev| prev.max(percentage)));
                } else if lower.contains("gemini") {
                    gemini_pct = Some(gemini_pct.map_or(percentage, |prev| prev.max(percentage)));
                }

                models.push(ModelQuota {
                    name,
                    percentage,
                    reset_time,
                    display_name,
                });
            }
        }

        QuotaSummary {
            gemini_percentage: gemini_pct,
            claude_percentage: claude_pct,
            models,
            last_updated: Some(chrono::Utc::now().timestamp()),
        }
    }
}
