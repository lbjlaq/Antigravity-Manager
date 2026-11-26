pub mod fingerprint;

use crate::types::{ModelQuota, QuotaGroup, QuotaSummary};
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
    NeedDisable {
        reason: String,
    },
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
            client: crate::runtime::shared_http_client().clone(),
            ua: fingerprint::get_official_user_agent(),
        }
    }

    /// 第一步：通过 loadCodeAssist 拉取账号激活的项目 Project ID 与订阅等级 (paidTier / currentTier)
    pub async fn fetch_project_and_tier(
        &self,
        access_token: &str,
    ) -> (Option<String>, Option<String>, Option<String>) {
        let meta = serde_json::json!({ "metadata": { "ideType": "ANTIGRAVITY" } });
        for endpoint in LOAD_PROJECT_ENDPOINTS {
            if let Ok(res) = self
                .client
                .post(endpoint)
                .timeout(std::time::Duration::from_secs(15))
                .bearer_auth(access_token)
                .header(reqwest::header::USER_AGENT, &self.ua)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .json(&meta)
                .send()
                .await
            {
                if res.status().is_success() {
                    if let Ok(val) = res.json::<Value>().await {
                        let pid = val
                            .get("cloudaicompanionProject")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let raw_tier = val
                            .get("paidTier")
                            .and_then(|t| t.get("id"))
                            .and_then(|v| v.as_str())
                            .or_else(|| {
                                val.get("currentTier")
                                    .and_then(|t| t.get("id"))
                                    .and_then(|v| v.as_str())
                            })
                            .map(|s| s.to_string());
                        let norm_tier = raw_tier.as_deref().map(Self::normalize_subscription_tier);
                        return (pid, norm_tier, raw_tier);
                    }
                }
            }
        }
        (None, None, None)
    }

    /// 兼容旧签名：仅获取 Project ID
    pub async fn fetch_project_id(&self, access_token: &str) -> Option<String> {
        self.fetch_project_and_tier(access_token).await.0
    }

    /// 将上游 paidTier/currentTier 原始 ID 归一化为 FREE / PRO / ULTRA / ENTERPRISE
    pub fn normalize_subscription_tier(raw: &str) -> String {
        let upper = raw.trim().to_ascii_uppercase();
        if upper.contains("ULTRA") || upper.contains("HELIUM") {
            "ULTRA".to_string()
        } else if upper.contains("ENTERPRISE") || upper.contains("PAYGO") {
            "ENTERPRISE".to_string()
        } else if upper.contains("PRO") || upper.contains("PREMIUM") || upper.contains("STANDARD") {
            "PRO".to_string()
        } else {
            "FREE".to_string()
        }
    }

    async fn post_first_ok(
        &self,
        endpoints: &[&str],
        access_token: &str,
        body: &Value,
    ) -> (Option<Value>, reqwest::StatusCode, String, Option<String>) {
        let mut last_status = reqwest::StatusCode::OK;
        let mut last_err_body = String::new();
        let mut last_net_err: Option<String> = None;

        for endpoint in endpoints {
            match self
                .client
                .post(*endpoint)
                .timeout(std::time::Duration::from_secs(15))
                .bearer_auth(access_token)
                .header(reqwest::header::USER_AGENT, &self.ua)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .json(body)
                .send()
                .await
            {
                Ok(res) => {
                    let status = res.status();
                    last_status = status;
                    if status.is_success() {
                        if let Ok(val) = res.json::<Value>().await {
                            return (Some(val), status, String::new(), None);
                        }
                    } else {
                        last_err_body = res.text().await.unwrap_or_default();
                    }
                }
                Err(e) => {
                    last_net_err = Some(format!("Network request failed: {}", e));
                }
            }
        }

        (None, last_status, last_err_body, last_net_err)
    }

    /// 第二步：并发查询 retrieveUserQuotaSummary（双轨 5h/weekly 分组配额）与 fetchAvailableModels（全模型配额明细），
    /// 合并产出完整 QuotaSummary 并包含错误模式自愈分析
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

        let (summary_res, models_res) = tokio::join!(
            self.post_first_ok(&USER_QUOTA_SUMMARY_ENDPOINTS, access_token, &body),
            self.post_first_ok(&FETCH_MODELS_ENDPOINTS, access_token, &body),
        );

        if summary_res.0.is_some() || models_res.0.is_some() {
            let summary =
                Self::merge_quota_responses(summary_res.0.as_ref(), models_res.0.as_ref());
            return QuotaFetchResult::Success(summary);
        }

        // 两个端点均失败：优先使用显式 HTTP 错误码（401 / 403）进行自愈诊断
        let (last_status, last_err_body, last_net_err) = if summary_res.1 != reqwest::StatusCode::OK
        {
            (summary_res.1, summary_res.2, summary_res.3)
        } else {
            (models_res.1, models_res.2, models_res.3)
        };

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

        if last_status == reqwest::StatusCode::OK {
            if let Some(net_err) = last_net_err {
                return QuotaFetchResult::TransientError(net_err);
            }
        }

        QuotaFetchResult::TransientError(format!("Status {}: {}", last_status, last_err_body))
    }

    /// 合并 retrieveUserQuotaSummary（groups）与 fetchAvailableModels（models）的真实接口数据
    fn merge_quota_responses(
        summary_val: Option<&Value>,
        models_val: Option<&Value>,
    ) -> QuotaSummary {
        let mut combined = match summary_val {
            Some(v) => Self::parse_quota_summary(v),
            None => QuotaSummary::default(),
        };

        if let Some(mv) = models_val {
            let from_models = Self::parse_quota_summary(mv);
            if combined.models.is_empty() && !from_models.models.is_empty() {
                combined.models = from_models.models;
            }
            if combined.quota_groups.is_none() && from_models.quota_groups.is_some() {
                combined.quota_groups = from_models.quota_groups;
            }
            if combined.subscription_tier.is_none() {
                combined.subscription_tier = from_models.subscription_tier;
            }
            if combined.gemini_percentage.is_none() {
                combined.gemini_percentage = from_models.gemini_percentage;
            }
            if combined.claude_percentage.is_none() {
                combined.claude_percentage = from_models.claude_percentage;
            }
        }

        // 若满额 5h 桶在 retrieveUserQuotaSummary 中未携带 resetTime，则从 fetchAvailableModels 同池模型的真实 resetTime 回填
        if let Some(groups) = combined.quota_groups.as_mut() {
            let now_ms = chrono::Utc::now().timestamp_millis();
            for group in groups.iter_mut() {
                let gname = group.display_name.as_deref().unwrap_or("").to_lowercase();
                let gdesc = group.description.as_deref().unwrap_or("").to_lowercase();
                let is_claude = gname.contains("claude")
                    || gdesc.contains("claude")
                    || gname.contains("3p")
                    || gdesc.contains("3p");
                let is_gemini = (gname.contains("gemini") || gdesc.contains("gemini"))
                    && !gname.contains("argon");

                for bucket in group.buckets.iter_mut() {
                    if bucket.observed_at.is_none() {
                        bucket.observed_at = Some(now_ms);
                    }
                    let is_5h = bucket.window.as_deref() == Some("5h")
                        || bucket
                            .bucket_id
                            .as_deref()
                            .map(|id| id.ends_with("-5h"))
                            .unwrap_or(false);
                    let missing_reset = bucket
                        .reset_time
                        .as_deref()
                        .map(|s| s.trim().is_empty())
                        .unwrap_or(true);
                    if is_5h && missing_reset {
                        if let Some(rst) = combined.models.iter().find_map(|m| {
                            let lower = m.name.to_lowercase();
                            let matched = (is_claude
                                && (lower.contains("claude") || lower.contains("gpt")))
                                || (is_gemini && lower.contains("gemini"));
                            if !matched {
                                return None;
                            }
                            let rst = m.reset_time.as_deref()?.trim();
                            if rst.is_empty() {
                                return None;
                            }
                            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(rst) {
                                let diff_secs = dt.timestamp() - chrono::Utc::now().timestamp();
                                if diff_secs > 0 && diff_secs <= 24 * 3600 {
                                    return Some(rst.to_string());
                                }
                            }
                            None
                        }) {
                            bucket.reset_time = Some(rst);
                        }
                    }
                }
            }
        }

        // 以权威 quota_groups 重算聚合百分比（优先 5h / weekly）
        if combined.quota_groups.is_some() {
            let mut probe = combined.clone();
            probe.gemini_percentage = None;
            probe.claude_percentage = None;
            if let Some(g) = probe.calculate_gemini_percentage() {
                combined.gemini_percentage = Some(g);
            }
            if let Some(c) = probe.calculate_claude_percentage() {
                combined.claude_percentage = Some(c);
            }
        }

        combined.last_updated = Some(chrono::Utc::now().timestamp());
        combined
    }

    /// 解析 Google 配额响应 JSON (兼容 retrieveUserQuotaSummary 的 groups 字段与 fetchAvailableModels 字典)
    fn parse_quota_summary(val: &Value) -> QuotaSummary {
        let mut models = Vec::new();
        let mut gemini_pct: Option<f64> = None;
        let mut claude_pct: Option<f64> = None;

        // 1. 如果 models 是数组
        if let Some(arr) = val.get("models").and_then(|v| v.as_array()) {
            for item in arr {
                let name = item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let percentage = item
                    .get("percentage")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let reset_time = item
                    .get("resetTime")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let display_name = item
                    .get("displayName")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

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
        // 2. 如果 models 是键值字典 (fetchAvailableModels)
        } else if let Some(map) = val.get("models").and_then(|v| v.as_object()) {
            for (model_id, item) in map {
                let display_name = item
                    .get("displayName")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let mut percentage = 100.0;
                let mut reset_time = None;

                if let Some(qinfo) = item.get("quotaInfo") {
                    if let Some(frac) = qinfo.get("remainingFraction").and_then(|v| v.as_f64()) {
                        percentage = (frac * 100.0).round();
                    }
                    if let Some(rst) = qinfo.get("resetTime").and_then(|v| v.as_str()) {
                        reset_time = Some(rst.to_string());
                    }
                }

                let lower = model_id.to_lowercase();
                if lower.contains("claude") {
                    claude_pct = Some(claude_pct.map_or(percentage, |prev| prev.max(percentage)));
                } else if lower.contains("gemini") {
                    gemini_pct = Some(gemini_pct.map_or(percentage, |prev| prev.max(percentage)));
                }

                models.push(ModelQuota {
                    name: model_id.clone(),
                    percentage,
                    reset_time,
                    display_name,
                });
            }
        }

        let sub_tier = val
            .get("subscriptionTier")
            .or_else(|| val.get("subscription_tier"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // retrieveUserQuotaSummary 返回顶层 "groups"（camelCase 字段），同时兼容 quotaGroups / quota_groups
        let now_ms = chrono::Utc::now().timestamp_millis();
        let quota_groups = val
            .get("groups")
            .or_else(|| val.get("quotaGroups"))
            .or_else(|| val.get("quota_groups"))
            .and_then(|v| serde_json::from_value::<Vec<QuotaGroup>>(v.clone()).ok())
            .map(|mut groups| {
                for g in &mut groups {
                    for b in &mut g.buckets {
                        if b.observed_at.is_none() {
                            b.observed_at = Some(now_ms);
                        }
                    }
                }
                groups
            });

        let mut summary = QuotaSummary {
            gemini_percentage: gemini_pct,
            claude_percentage: claude_pct,
            subscription_tier: sub_tier,
            quota_groups,
            models,
            last_updated: Some(chrono::Utc::now().timestamp()),
            ..Default::default()
        };

        if summary.gemini_percentage.is_none() {
            summary.gemini_percentage = summary.calculate_gemini_percentage();
        }
        if summary.claude_percentage.is_none() {
            summary.claude_percentage = summary.calculate_claude_percentage();
        }

        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_retrieve_user_quota_summary_and_merges_models() {
        let future_5h = (chrono::Utc::now() + chrono::Duration::hours(4)).to_rfc3339();
        let summary_json = serde_json::json!({
            "groups": [
                {
                    "displayName": "Gemini Models",
                    "description": "Models within this group: Gemini 4 Neon, Gemini 4 Argon",
                    "buckets": [
                        {
                            "bucketId": "gemini-weekly",
                            "displayName": "Weekly Limit Remaining",
                            "window": "weekly",
                            "resetTime": "2026-12-02T03:07:16Z",
                            "remainingFraction": 0.8754359
                        },
                        {
                            "bucketId": "gemini-5h",
                            "displayName": "Five Hour Limit Remaining",
                            "window": "5h",
                            "resetTime": future_5h,
                            "remainingFraction": 0.93624127
                        }
                    ]
                },
                {
                    "displayName": "Claude and GPT models",
                    "description": "Models within this group: Claude Opus 5.5, Claude Sonnet 5.5",
                    "buckets": [
                        {
                            "bucketId": "3p-weekly",
                            "displayName": "Weekly Limit Remaining",
                            "window": "weekly",
                            "remainingFraction": 1.0
                        },
                        {
                            "bucketId": "3p-5h",
                            "displayName": "Five Hour Limit Remaining",
                            "window": "5h",
                            "remainingFraction": 1.0
                        }
                    ]
                }
            ]
        });

        let models_json = serde_json::json!({
            "models": {
                "claude-sonnet-5-5-high": {
                    "displayName": "Claude Sonnet 5.5 (High)",
                    "quotaInfo": {
                        "remainingFraction": 1.0,
                        "resetTime": future_5h
                    }
                },
                "gemini-3.8-flash-high": {
                    "displayName": "Gemini 3.8 Flash (High)",
                    "quotaInfo": {
                        "remainingFraction": 0.936,
                        "resetTime": future_5h
                    }
                }
            }
        });

        let merged = QuotaClient::merge_quota_responses(Some(&summary_json), Some(&models_json));
        let groups = merged
            .quota_groups
            .expect("quota_groups must be parsed from 'groups'");
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].display_name.as_deref(), Some("Gemini Models"));
        assert_eq!(groups[0].buckets[1].bucket_id.as_deref(), Some("gemini-5h"));
        assert!((groups[0].buckets[1].remaining_fraction.unwrap() - 0.93624127).abs() < 1e-6);

        // 满额 3p-5h 无 resetTime 时应从 fetchAvailableModels 自动回填真实 resetTime
        let claude_5h = groups[1]
            .buckets
            .iter()
            .find(|b| b.window.as_deref() == Some("5h"))
            .unwrap();
        assert_eq!(claude_5h.reset_time.as_deref(), Some(future_5h.as_str()));
        assert_eq!(merged.models.len(), 2);
        // 5h=93.62%, weekly=87.54%，真实可用瓶颈水位取较小值 87.54%
        assert!((merged.gemini_percentage.unwrap() - 87.54359).abs() < 0.01);
        assert!((merged.claude_percentage.unwrap() - 100.0).abs() < 0.01);
    }
}
