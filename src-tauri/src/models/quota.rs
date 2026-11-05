use serde::{Deserialize, Serialize};

/// 单个配额桶 (对应 retrieveUserQuotaSummary 里的一个 bucket)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaBucket {
    /// 桶 ID,如 "gemini-weekly" / "gemini-5h" / "3p-weekly" / "3p-5h"
    pub bucket_id: String,
    /// 窗口类型: "weekly" / "5h"
    pub window: String,
    /// 剩余比例 0.0-1.0
    pub remaining_fraction: f64,
    /// 重置时间 (RFC3339)
    pub reset_time: String,
    /// Successful bucket observation time in milliseconds; absent in older snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<i64>,
    /// Usage recorded by this instance, populated only when returning the account list.
    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub cycle_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl QuotaBucket {
    /// Valid current weekly interval, with an inclusive start and exclusive reset.
    pub(crate) fn weekly_cycle_bounds(&self, now: i64) -> Option<(i64, i64)> {
        let window = format!("{} {}", self.window, self.bucket_id).to_lowercase();
        if !(window.contains("week") || window.contains("7d"))
            || !(0.0..=1.0).contains(&self.remaining_fraction)
        {
            return None;
        }
        let end = chrono::DateTime::parse_from_rfc3339(&self.reset_time)
            .ok()?
            .timestamp();
        let start = end.checked_sub(7 * 24 * 60 * 60)?;
        (start <= now && now < end).then_some((start, end))
    }
}

/// 一个模型组 (如 Gemini Models / Claude and GPT models)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaGroup {
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub buckets: Vec<QuotaBucket>,
}

/// 模型配额信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelQuota {
    pub name: String,
    pub percentage: i32, // 剩余百分比 0-100
    pub reset_time: String,

    // -- 动态参数解析与持久化 --
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_images: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_thinking: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_mime_types: Option<std::collections::HashMap<String, bool>>,
}

/// 对外暴露的模型能力与元数据投影（用于 /v1/models 与模型检索）
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelListingMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_images: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_thinking: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported_mime_types: Option<std::collections::HashMap<String, bool>>,
}

impl ModelListingMeta {
    /// 逐字段合并：当前字段为 None 时，用 other 的对应字段填充；保持先见者优先的稳定性
    pub fn merge_from(&mut self, other: &Self) {
        if self.display_name.is_none() {
            self.display_name = other.display_name.clone();
        }
        if self.supports_images.is_none() {
            self.supports_images = other.supports_images;
        }
        if self.supports_thinking.is_none() {
            self.supports_thinking = other.supports_thinking;
        }
        if self.thinking_budget.is_none() {
            self.thinking_budget = other.thinking_budget;
        }
        if self.recommended.is_none() {
            self.recommended = other.recommended;
        }
        if self.max_tokens.is_none() {
            self.max_tokens = other.max_tokens;
        }
        if self.max_output_tokens.is_none() {
            self.max_output_tokens = other.max_output_tokens;
        }
        if self.supported_mime_types.is_none() {
            self.supported_mime_types = other.supported_mime_types.clone();
        }
    }
}

impl From<&ModelQuota> for ModelListingMeta {
    fn from(q: &ModelQuota) -> Self {
        Self {
            display_name: q.display_name.clone(),
            supports_images: q.supports_images,
            supports_thinking: q.supports_thinking,
            thinking_budget: q.thinking_budget,
            recommended: q.recommended,
            max_tokens: q.max_tokens,
            max_output_tokens: q.max_output_tokens,
            supported_mime_types: q.supported_mime_types.clone(),
        }
    }
}

impl From<&crate::models::OfficialModelInfo> for ModelListingMeta {
    fn from(info: &crate::models::OfficialModelInfo) -> Self {
        Self {
            display_name: info.display_name.clone(),
            supports_images: info.supports_images,
            supports_thinking: info.supports_thinking,
            thinking_budget: info.thinking_budget.map(|b| b as i32),
            recommended: info.recommended,
            max_tokens: info.max_tokens.map(|t| t as i32),
            max_output_tokens: info.max_output_tokens.map(|t| t as i32),
            supported_mime_types: info.supported_mime_types.clone(),
        }
    }
}

/// 配额数据结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaData {
    pub models: Vec<ModelQuota>,
    pub last_updated: i64,
    #[serde(default)]
    pub is_forbidden: bool,
    /// 禁止访问的原因 (403 详细信息)
    #[serde(default)]
    pub forbidden_reason: Option<String>,
    /// 订阅等级 (FREE/PRO/ULTRA)
    #[serde(default)]
    pub subscription_tier: Option<String>,
    /// 模型淘汰重定向规则表 (old_model_id -> new_model_id)
    #[serde(default)]
    pub model_forwarding_rules: std::collections::HashMap<String, String>,
    /// 按模型组的配额摘要 (weekly + 5h 双窗口),来自 retrieveUserQuotaSummary
    #[serde(default)]
    pub quota_groups: Option<Vec<QuotaGroup>>,
}

impl QuotaData {
    pub fn new() -> Self {
        Self {
            models: Vec::new(),
            last_updated: chrono::Utc::now().timestamp(),
            is_forbidden: false,
            forbidden_reason: None,
            subscription_tier: None,
            model_forwarding_rules: std::collections::HashMap::new(),
            quota_groups: None,
        }
    }

    pub fn add_model(&mut self, model: ModelQuota) {
        self.models.push(model);
    }

    /// 确保当前配额具备有效的订阅等级 (ULTRA/PRO/FREE)
    ///
    /// 只做「归一化 + 校验」：已有合法值就保留，无法识别的值置空，
    /// 等待下一次 `loadCodeAssist` 用上游权威值回填。
    /// **绝不使用模型列表做推断**（见 `resolve_subscription_tier` 的说明）。
    pub fn ensure_subscription_tier(&mut self) {
        self.subscription_tier = match self.subscription_tier.as_deref() {
            Some(raw) => {
                let normalized = normalize_subscription_tier(raw);
                if is_known_tier(&normalized) {
                    Some(normalized)
                } else {
                    None
                }
            }
            None => None,
        };
    }
}

/// 是否为已知等级之一 (ULTRA | PRO | FREE | ENTERPRISE)
pub fn is_known_tier(tier: &str) -> bool {
    matches!(tier, "ULTRA" | "PRO" | "FREE" | "ENTERPRISE")
}

/// 订阅等级标准化：统一规范为 "ULTRA" | "PRO" | "FREE"
///
/// 输入应优先是 `loadCodeAssist` 返回的 `UserTier.id`（稳定标识），实测取值：
///   - `"free-tier"`     → 免费档（其 `name` 是自由文本 `"Antigravity Starter Quota"`）
///   - `"standard-tier"` → 付费基础档（`allowedTiers` 里的非默认项）
///   - `"g1-pro-tier"`   → Google AI Pro（`name` = "Google AI Pro"）
///   - `"g1-ultra-tier"` → Google AI Ultra（`name` = "Google AI Ultra"）
///
/// 同时也兼容按 `name` 文本传入的历史数据。`"helium"` 是 Ultra 档的内部代号
/// （见上游 `UserTier.UpgradeType` 的 `GDP_HELIUM` / `GOOGLE_ONE_HELIUM`），
/// 因此一并归入 ULTRA。
///
/// 未识别时**原样返回**，由调用方决定如何处理，不做猜测。
pub fn normalize_subscription_tier(tier: &str) -> String {
    let lower = tier.trim().to_lowercase();
    if lower.is_empty() {
        return String::new();
    }

    // 1) Ultra 档（含内部代号 helium）
    if lower.contains("ultra") || lower.contains("helium") {
        return "ULTRA".to_string();
    }

    // 2) 免费档：free-tier / "Antigravity Starter Quota"
    if lower.contains("free") || lower.contains("starter") {
        return "FREE".to_string();
    }

    // 3) 付费档：g1-pro-tier / "Google AI Pro" / premium / advanced
    if lower.contains("pro") || lower.contains("premium") || lower.contains("advanced") {
        return "PRO".to_string();
    }

    // 4) 企业 / GCP 档：standard-tier / "Antigravity" enterprise / GCP_* / business / vertex
    if lower.contains("enterprise")
        || lower.contains("standard")
        || lower.contains("business")
        || lower.starts_with("gcp_")
        || lower.contains("vertex")
    {
        return "ENTERPRISE".to_string();
    }

    tier.to_string()
}

/// 解析订阅等级。
///
/// **重要：不再用模型列表做兜底推断。**
///
/// 历史实现会在等级无法识别时检查模型列表，只要出现 `claude*` / `gpt*` 就判为 PRO。
/// 但实测 `fetchAvailableModels` 返回的是**全量静态目录**：免费号与 Pro 号拿到的
/// 33 个模型**完全相同**（都含 `claude-opus-4-6-thinking` / `gpt-oss-120b-medium`），
/// 且 `remainingFraction` 恒为 1。模型是否存在与档位毫无关系，该兜底会把所有
/// 免费号误判成 PRO —— 这正是「免费账号被标记为 Pro」的根因。
///
/// 现在：能识别就返回识别结果；识别不了就返回 "FREE"，等待上游回填。
pub fn resolve_subscription_tier(raw_tier: Option<&str>) -> String {
    if let Some(tier) = raw_tier {
        let normalized = normalize_subscription_tier(tier);
        if is_known_tier(&normalized) {
            return normalized;
        }
    }
    "FREE".to_string()
}

/// 轮询调度用的等级优先级：数值越小越优先 (ULTRA=0, PRO=1, FREE=2)。
///
/// 未知等级按 FREE 处理，避免出现「未识别等级排在 FREE 之后」的隐藏档位 ——
/// 那会导致同一个账号在 UI 上显示 FREE、在调度器里却比 FREE 更靠后。
pub fn tier_priority(tier: Option<&str>) -> u8 {
    match normalize_subscription_tier(tier.unwrap_or("")).as_str() {
        "ULTRA" => 0,
        "PRO" | "ENTERPRISE" => 1,
        _ => 2,
    }
}

impl Default for QuotaData {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_subscription_tier() {
        // 上游真实 id（loadCodeAssist 返回的权威字段）
        assert_eq!(normalize_subscription_tier("free-tier"), "FREE");
        assert_eq!(normalize_subscription_tier("g1-pro-tier"), "PRO");
        assert_eq!(
            normalize_subscription_tier("standard-tier"),
            "ENTERPRISE"
        );
        assert_eq!(
            normalize_subscription_tier("GCP_ENTERPRISE_TIER"),
            "ENTERPRISE"
        );
        assert_eq!(
            normalize_subscription_tier("GCP_GE_PAYGO_TIER"),
            "ENTERPRISE"
        );
        assert_eq!(normalize_subscription_tier("g1-ultra-tier"), "ULTRA");
        assert_eq!(normalize_subscription_tier("GOOGLE_ONE_HELIUM"), "ULTRA");
        assert_eq!(normalize_subscription_tier("GDP_HELIUM"), "ULTRA");

        // 上游真实 name（自由文本）
        assert_eq!(
            normalize_subscription_tier("Antigravity Starter Quota"),
            "FREE"
        );
        assert_eq!(normalize_subscription_tier("Google AI Pro"), "PRO");
        assert_eq!(normalize_subscription_tier("Google AI Ultra"), "ULTRA");

        // 历史 / 兼容写法
        assert_eq!(normalize_subscription_tier("Google One AI Premium"), "PRO");
        assert_eq!(normalize_subscription_tier("gemini-advanced"), "PRO");
        assert_eq!(normalize_subscription_tier("Gemini Pro"), "PRO");
        assert_eq!(normalize_subscription_tier("pro"), "PRO");
        assert_eq!(normalize_subscription_tier("gemini-ultra"), "ULTRA");
        assert_eq!(normalize_subscription_tier("ULTRA"), "ULTRA");
        assert_eq!(normalize_subscription_tier("Free"), "FREE");

        // 空值 / 未识别原样返回
        assert_eq!(normalize_subscription_tier(""), "");
        assert_eq!(normalize_subscription_tier("   "), "");
        assert_eq!(
            normalize_subscription_tier("totally-unknown"),
            "totally-unknown"
        );
    }

    #[test]
    fn test_is_known_tier() {
        assert!(is_known_tier("FREE"));
        assert!(is_known_tier("PRO"));
        assert!(is_known_tier("ULTRA"));
        assert!(!is_known_tier(""));
        assert!(!is_known_tier("totally-unknown"));
        assert!(!is_known_tier("pro")); // 未归一化的小写形式不算合法值
    }

    #[test]
    fn test_resolve_subscription_tier_no_model_fallback() {
        // 关键回归：模型列表不再参与推断。
        // 免费号也会拿到 claude / gpt 全量目录，因此 None 必须判 FREE，
        // 否则就是「免费账号被标记成 Pro」的原始 bug。
        assert_eq!(resolve_subscription_tier(None), "FREE");

        // 上游权威 id
        assert_eq!(resolve_subscription_tier(Some("free-tier")), "FREE");
        assert_eq!(resolve_subscription_tier(Some("g1-pro-tier")), "PRO");
        assert_eq!(resolve_subscription_tier(Some("g1-ultra-tier")), "ULTRA");

        // 上游 name
        assert_eq!(
            resolve_subscription_tier(Some("Antigravity Starter Quota")),
            "FREE"
        );
        assert_eq!(resolve_subscription_tier(Some("Google AI Pro")), "PRO");

        // 未识别 -> FREE（不猜 PRO）
        assert_eq!(resolve_subscription_tier(Some("totally-unknown")), "FREE");
        assert_eq!(resolve_subscription_tier(Some("")), "FREE");
    }

    #[test]
    fn test_tier_priority() {
        assert_eq!(tier_priority(Some("ULTRA")), 0);
        assert_eq!(tier_priority(Some("g1-ultra-tier")), 0);
        assert_eq!(tier_priority(Some("PRO")), 1);
        assert_eq!(tier_priority(Some("g1-pro-tier")), 1);
        assert_eq!(tier_priority(Some("FREE")), 2);
        assert_eq!(tier_priority(Some("free-tier")), 2);

        // 未知 / 缺失一律按 FREE 处理，不允许出现排在 FREE 之后的隐藏档位
        assert_eq!(tier_priority(None), 2);
        assert_eq!(tier_priority(Some("")), 2);
        assert_eq!(tier_priority(Some("garbage")), 2);
    }

    #[test]
    fn test_ensure_subscription_tier_normalizes_and_drops_unknown() {
        let mut quota = QuotaData::new();

        // 上游 name 形态能被归一化
        quota.subscription_tier = Some("Antigravity Starter Quota".to_string());
        quota.ensure_subscription_tier();
        assert_eq!(quota.subscription_tier.as_deref(), Some("FREE"));

        quota.subscription_tier = Some("g1-pro-tier".to_string());
        quota.ensure_subscription_tier();
        assert_eq!(quota.subscription_tier.as_deref(), Some("PRO"));

        // 无法识别 -> 置空，等上游回填（绝不能落成 PRO）
        quota.subscription_tier = Some("totally-unknown".to_string());
        quota.ensure_subscription_tier();
        assert_eq!(quota.subscription_tier, None);

        // None 保持 None
        quota.subscription_tier = None;
        quota.ensure_subscription_tier();
        assert_eq!(quota.subscription_tier, None);
    }

    #[test]
    fn test_quota_group_deserialization_with_missing_buckets() {
        let json = r#"{"display_name":"Gemini Models"}"#;
        let group: QuotaGroup =
            serde_json::from_str(json).expect("Should deserialize with missing buckets");
        assert_eq!(group.display_name, "Gemini Models");
        assert!(group.buckets.is_empty());
    }

    #[test]
    fn test_model_listing_meta_serialization_skips_none() {
        let meta = ModelListingMeta {
            display_name: Some("Gemini 3.7 Flash".to_string()),
            supports_images: Some(true),
            supports_thinking: Some(true),
            thinking_budget: Some(2048),
            ..Default::default()
        };
        let val = serde_json::to_value(&meta).unwrap();
        assert_eq!(val["display_name"], "Gemini 3.7 Flash");
        assert_eq!(val["supports_images"], true);
        assert_eq!(val["supports_thinking"], true);
        assert_eq!(val["thinking_budget"], 2048);
        assert!(val.get("max_tokens").is_none());
        assert!(val.get("max_output_tokens").is_none());
        assert!(val.get("recommended").is_none());
        assert!(val.get("supported_mime_types").is_none());
    }

    #[test]
    fn test_model_listing_meta_merge_from() {
        let mut meta_a = ModelListingMeta {
            display_name: Some("Model A".to_string()),
            supports_images: Some(true),
            ..Default::default()
        };
        let meta_b = ModelListingMeta {
            display_name: Some("Model B".to_string()),
            supports_images: Some(false),
            supports_thinking: Some(true),
            max_output_tokens: Some(8192),
            ..Default::default()
        };
        meta_a.merge_from(&meta_b);

        // 先见者优先 (meta_a 保持已有值)
        assert_eq!(meta_a.display_name.as_deref(), Some("Model A"));
        assert_eq!(meta_a.supports_images, Some(true));
        // None 字段被补全
        assert_eq!(meta_a.supports_thinking, Some(true));
        assert_eq!(meta_a.max_output_tokens, Some(8192));
    }

    #[test]
    fn test_model_listing_meta_from_model_quota() {
        let quota = ModelQuota {
            name: "gemini-3.7-flash-high".to_string(),
            percentage: 100,
            reset_time: "2026-10-16T10:00:00Z".to_string(),
            display_name: Some("Gemini 3.7 Flash (High)".to_string()),
            supports_images: Some(true),
            supports_thinking: Some(true),
            thinking_budget: Some(1024),
            recommended: Some(true),
            max_tokens: Some(1048576),
            max_output_tokens: Some(65536),
            model: None,
            supported_mime_types: None,
        };
        let meta = ModelListingMeta::from(&quota);
        assert_eq!(
            meta.display_name.as_deref(),
            Some("Gemini 3.7 Flash (High)")
        );
        assert_eq!(meta.supports_images, Some(true));
        assert_eq!(meta.supports_thinking, Some(true));
        assert_eq!(meta.thinking_budget, Some(1024));
        assert_eq!(meta.recommended, Some(true));
        assert_eq!(meta.max_tokens, Some(1048576));
        assert_eq!(meta.max_output_tokens, Some(65536));
    }
}
