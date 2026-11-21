use crate::types::{ModelQuota, QuotaSummary};
use anyhow::{anyhow, Result};
use serde_json::Value;

const QUOTA_API_URL: &str =
    "https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary";

pub struct QuotaClient {
    client: reqwest::Client,
}

impl QuotaClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// 查询用户当前详细配额
    pub async fn fetch_quota(&self, access_token: &str) -> Result<QuotaSummary> {
        let res = self
            .client
            .post(QUOTA_API_URL)
            .bearer_auth(access_token)
            .json(&serde_json::json!({}))
            .send()
            .await?;

        if !res.status().is_success() {
            let err_text = res.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to fetch quota from upstream: {}", err_text));
        }

        let body: Value = res.json().await?;
        let summary = parse_quota_response(&body);
        Ok(summary)
    }
}

/// 解析 Google Quota 响应，提取 Gemini / Claude 各模型池剩余百分比
pub fn parse_quota_response(val: &Value) -> QuotaSummary {
    let mut models_list = Vec::new();
    let mut gemini_pct: Option<f64> = None;
    let mut claude_pct: Option<f64> = None;

    if let Some(buckets) = val.get("buckets").and_then(|b| b.as_array()) {
        for bucket in buckets {
            let model_id = bucket
                .get("modelId")
                .or_else(|| bucket.get("model"))
                .and_then(|m| m.as_str())
                .unwrap_or_default();

            let percentage = bucket
                .get("remainingPercentage")
                .or_else(|| bucket.get("remainingFraction"))
                .and_then(|p| p.as_f64())
                .unwrap_or(0.0);

            // 统一归一化为 0..100
            let normalized_pct = if percentage <= 1.0 && percentage > 0.0 {
                percentage * 100.0
            } else {
                percentage
            };

            let reset_time = bucket
                .get("resetTime")
                .and_then(|r| r.as_str())
                .map(|s| s.to_string());

            if !model_id.is_empty() {
                models_list.push(ModelQuota {
                    name: model_id.to_string(),
                    percentage: normalized_pct,
                    reset_time,
                    display_name: None,
                });

                let lower = model_id.to_ascii_lowercase();
                if lower.contains("flash") || lower.contains("pro") {
                    gemini_pct = match gemini_pct {
                        Some(prev) => Some(prev.min(normalized_pct)),
                        None => Some(normalized_pct),
                    };
                } else if lower.contains("claude") || lower.contains("anthropic") {
                    claude_pct = match claude_pct {
                        Some(prev) => Some(prev.min(normalized_pct)),
                        None => Some(normalized_pct),
                    };
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
