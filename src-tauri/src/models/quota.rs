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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// 一个模型组 (如 Gemini Models / Claude and GPT models)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaGroup {
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
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
    pub supported_mime_types: Option<std::collections::HashMap<String, bool>>,
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

/// 是否为三个已知等级之一
pub fn is_known_tier(tier: &str) -> bool {
    matches!(tier, "ULTRA" | "PRO" | "FREE")
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
        "PRO" => 1,
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
            "standard-tier"
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
}
