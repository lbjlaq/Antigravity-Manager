use serde::{Deserialize, Serialize, Serializer};

fn serialize_priority<S: Serializer>(val: &Option<i32>, s: S) -> Result<S::Ok, S::Error> {
    let p = match val {
        Some(v) if *v >= 1 && *v <= 100 => *v as u8,
        Some(v) if *v > 100 => 100u8,
        _ => 50u8,
    };
    s.serialize_u8(p)
}

fn serialize_i64_or_zero<S: Serializer>(val: &Option<i64>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_i64(val.unwrap_or(0))
}

fn serialize_expires_in<S: Serializer>(val: &Option<i64>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_i64(val.unwrap_or(3600))
}

fn serialize_token_type<S: Serializer>(val: &Option<String>, s: S) -> Result<S::Ok, S::Error> {
    let t = val.as_deref().filter(|v| !v.is_empty()).unwrap_or("Bearer");
    s.serialize_str(t)
}

fn serialize_bool_or_false<S: Serializer>(val: &Option<bool>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_bool(val.unwrap_or(false))
}

fn serialize_string_or_empty<S: Serializer>(val: &Option<String>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(val.as_deref().unwrap_or(""))
}

fn serialize_f64_or_zero<S: Serializer>(val: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64(val.unwrap_or(0.0))
}

fn serialize_f64_as_i32<S: Serializer>(val: &f64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_i32(val.round().clamp(0.0, 100.0) as i32)
}

/// 支持切换的宿主目标
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetHost {
    /// 所有运行中的宿主
    All,
    /// Antigravity 原生桌面版 (Antigravity.app)
    App,
    /// Antigravity IDE 定制版 (Antigravity IDE.app)
    Ide,
    /// Antigravity IDE 插件版 (VSCode/Cursor ACP 插件)
    Plugin,
    /// Antigravity CLI 命令行工具 (agy)
    #[serde(alias = "cli")]
    Agy,
}

impl std::str::FromStr for TargetHost {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "all" => Ok(TargetHost::All),
            "app" | "classic" | "desktop" => Ok(TargetHost::App),
            "ide" => Ok(TargetHost::Ide),
            "plugin" | "acp" | "ide-plugin" => Ok(TargetHost::Plugin),
            "agy" | "cli" => Ok(TargetHost::Agy),
            _ => Err(format!(
                "Unknown target host: {}. Available: all, app, ide, plugin, agy",
                s
            )),
        }
    }
}

/// OAuth 令牌信息
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenInfo {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default, serialize_with = "serialize_expires_in")]
    pub expires_in: Option<i64>,
    #[serde(default, serialize_with = "serialize_token_type")]
    pub token_type: Option<String>,
    #[serde(default, serialize_with = "serialize_i64_or_zero")]
    pub expiry_timestamp: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_client_key: Option<String>,
    #[serde(default, serialize_with = "serialize_bool_or_false")]
    pub is_gcp_tos: Option<bool>,
}

/// 账号实体 (存储在 ~/.antigravity_tools/accounts/<id>.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub email: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, serialize_with = "serialize_priority")]
    pub priority: Option<i32>,
    pub token: TokenInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_profile: Option<DeviceProfile>,
    #[serde(default)]
    pub quota: Option<QuotaSummary>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_at: Option<i64>,
    #[serde(default)]
    pub banned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banned_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banned_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banned_code: Option<String>,
    #[serde(default)]
    pub proxy_disabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_disabled_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_disabled_at: Option<i64>,
    #[serde(default)]
    pub manual_proxy_disabled: bool,
    #[serde(default)]
    pub validation_blocked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_blocked_until: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ban_appeal_url: Option<String>,
    #[serde(default, serialize_with = "serialize_i64_or_zero")]
    pub created_at: Option<i64>,
    #[serde(default, serialize_with = "serialize_i64_or_zero")]
    pub last_used: Option<i64>,
}

impl Account {
    /// 账号本身是否受限不可用（不包含旧版反代网关的 `proxy_disabled` 调度标记）
    pub fn is_restricted(&self) -> bool {
        self.disabled
            || self.banned
            || self.validation_blocked
            || self
                .quota
                .as_ref()
                .and_then(|q| q.is_forbidden)
                .unwrap_or(false)
    }

    pub fn restriction_reason(&self) -> Option<String> {
        if self.banned {
            return Some(
                self.banned_reason
                    .clone()
                    .unwrap_or_else(|| "Banned (TOS Violation)".to_string()),
            );
        }
        if let Some(quota) = &self.quota {
            if quota.is_forbidden.unwrap_or(false) {
                return Some(
                    quota
                        .forbidden_reason
                        .clone()
                        .unwrap_or_else(|| "Forbidden (403)".to_string()),
                );
            }
        }
        if self.validation_blocked {
            return Some("Validation Blocked".to_string());
        }
        if self.disabled {
            return Some(
                self.disabled_reason
                    .clone()
                    .unwrap_or_else(|| "Disabled".to_string()),
            );
        }
        None
    }
}

/// 账号索引摘要 (存储在 accounts.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSummary {
    pub id: String,
    pub email: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, serialize_with = "serialize_i64_or_zero")]
    pub created_at: Option<i64>,
    #[serde(default, serialize_with = "serialize_i64_or_zero")]
    pub last_used: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuotaBucket {
    #[serde(
        default,
        alias = "bucketId",
        serialize_with = "serialize_string_or_empty"
    )]
    pub bucket_id: Option<String>,
    #[serde(default, serialize_with = "serialize_string_or_empty")]
    pub window: Option<String>,
    #[serde(
        default,
        alias = "remainingFraction",
        serialize_with = "serialize_f64_or_zero"
    )]
    pub remaining_fraction: Option<f64>,
    #[serde(
        default,
        alias = "resetTime",
        serialize_with = "serialize_string_or_empty"
    )]
    pub reset_time: Option<String>,
    #[serde(default, alias = "observedAt", skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<i64>,
    #[serde(
        default,
        alias = "displayName",
        skip_serializing_if = "Option::is_none"
    )]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuotaGroup {
    #[serde(
        default,
        alias = "displayName",
        serialize_with = "serialize_string_or_empty"
    )]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub buckets: Vec<QuotaBucket>,
}

/// 配额明细 (完全兼容主分支 accounts.json 与历史格式)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuotaSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gemini_percentage: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_percentage: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_tier: Option<String>,
    #[serde(default, serialize_with = "serialize_bool_or_false")]
    pub is_forbidden: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forbidden_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_groups: Option<Vec<QuotaGroup>>,
    #[serde(default)]
    pub models: Vec<ModelQuota>,
    #[serde(default, serialize_with = "serialize_i64_or_zero")]
    pub last_updated: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_forwarding_rules: Option<serde_json::Value>,
}

impl QuotaSummary {
    /// 计算或推导 Gemini 额度百分比 (0.0 .. 100.0)
    pub fn calculate_gemini_percentage(&self) -> Option<f64> {
        if let Some(p) = self.gemini_percentage {
            return Some(p);
        }

        // 优先从 quota_groups 中提取（若同时存在 5h 与 weekly 桶，取真实可用瓶颈最小值）
        if let Some(groups) = &self.quota_groups {
            for group in groups {
                let name = group.display_name.as_deref().unwrap_or("").to_lowercase();
                let desc = group.description.as_deref().unwrap_or("").to_lowercase();
                if (name.contains("gemini") || desc.contains("gemini")) && !name.contains("argon") {
                    let b5 = group
                        .buckets
                        .iter()
                        .find(|b| b.window.as_deref() == Some("5h"))
                        .and_then(|b| b.remaining_fraction);
                    let bw = group
                        .buckets
                        .iter()
                        .find(|b| b.window.as_deref() == Some("weekly"))
                        .and_then(|b| b.remaining_fraction);
                    let eff = match (b5, bw) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (Some(a), None) => Some(a),
                        (None, Some(b)) => Some(b),
                        (None, None) => None,
                    };
                    if let Some(rf) = eff {
                        return Some((rf * 100.0).clamp(0.0, 100.0));
                    }
                }
            }
        }

        // 降级从 models 中推导
        let gemini_models: Vec<&ModelQuota> = self
            .models
            .iter()
            .filter(|m| m.name.to_lowercase().contains("gemini"))
            .collect();
        if !gemini_models.is_empty() {
            let avg = gemini_models.iter().map(|m| m.percentage).sum::<f64>()
                / gemini_models.len() as f64;
            return Some(avg.clamp(0.0, 100.0));
        }

        None
    }

    /// 计算或推导 Claude 额度百分比 (0.0 .. 100.0)
    pub fn calculate_claude_percentage(&self) -> Option<f64> {
        if let Some(p) = self.claude_percentage {
            return Some(p);
        }

        // 优先从 quota_groups 中提取（若同时存在 5h 与 weekly 桶，取真实可用瓶颈最小值）
        if let Some(groups) = &self.quota_groups {
            for group in groups {
                let name = group.display_name.as_deref().unwrap_or("").to_lowercase();
                let desc = group.description.as_deref().unwrap_or("").to_lowercase();
                if name.contains("claude")
                    || desc.contains("claude")
                    || name.contains("3p")
                    || desc.contains("3p")
                {
                    let b5 = group
                        .buckets
                        .iter()
                        .find(|b| b.window.as_deref() == Some("5h"))
                        .and_then(|b| b.remaining_fraction);
                    let bw = group
                        .buckets
                        .iter()
                        .find(|b| b.window.as_deref() == Some("weekly"))
                        .and_then(|b| b.remaining_fraction);
                    let eff = match (b5, bw) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (Some(a), None) => Some(a),
                        (None, Some(b)) => Some(b),
                        (None, None) => None,
                    };
                    if let Some(rf) = eff {
                        return Some((rf * 100.0).clamp(0.0, 100.0));
                    }
                }
            }
        }

        // 降级从 models 中推导
        let claude_models: Vec<&ModelQuota> = self
            .models
            .iter()
            .filter(|m| m.name.to_lowercase().contains("claude"))
            .collect();
        if !claude_models.is_empty() {
            let avg = claude_models.iter().map(|m| m.percentage).sum::<f64>()
                / claude_models.len() as f64;
            return Some(avg.clamp(0.0, 100.0));
        }

        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelQuota {
    #[serde(default)]
    pub name: String,
    #[serde(default, serialize_with = "serialize_f64_as_i32")]
    pub percentage: f64,
    #[serde(default, serialize_with = "serialize_string_or_empty")]
    pub reset_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// 设备指纹特征隔离
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfile {
    pub machine_id: String,
    pub mac_machine_id: String,
    pub dev_device_id: String,
    pub sqm_id: String,
}

/// 多宿主当前激活状态汇总
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActiveHostStatus {
    pub app_account: Option<String>,
    pub app_running: bool,
    #[serde(default)]
    pub app_version: Option<String>,
    pub ide_account: Option<String>,
    pub ide_running: bool,
    #[serde(default)]
    pub ide_version: Option<String>,
    pub plugin_account: Option<String>,
    pub plugin_running: bool,
    pub agy_account: Option<String>,
    #[serde(default)]
    pub agy_running: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_account() -> Account {
        Account {
            id: "acc-1".to_string(),
            email: "user@example.com".to_string(),
            name: None,
            priority: None,
            token: TokenInfo::default(),
            device_profile: None,
            quota: None,
            tags: Vec::new(),
            disabled: false,
            disabled_reason: None,
            disabled_at: None,
            banned: false,
            banned_reason: None,
            banned_at: None,
            banned_code: None,
            proxy_disabled: false,
            proxy_disabled_reason: None,
            proxy_disabled_at: None,
            manual_proxy_disabled: false,
            validation_blocked: false,
            validation_blocked_until: None,
            validation_url: None,
            ban_appeal_url: None,
            created_at: None,
            last_used: None,
        }
    }

    #[test]
    fn proxy_disabled_alone_does_not_restrict_account() {
        let mut acc = sample_account();
        acc.proxy_disabled = true;
        acc.proxy_disabled_reason = Some("批量禁用".to_string());
        assert!(!acc.is_restricted());
        assert!(acc.restriction_reason().is_none());
    }

    #[test]
    fn real_account_restrictions_are_detected() {
        let mut banned = sample_account();
        banned.banned = true;
        assert!(banned.is_restricted());
        assert!(banned.restriction_reason().is_some());

        let mut forbidden = sample_account();
        forbidden.quota = Some(QuotaSummary {
            is_forbidden: Some(true),
            forbidden_reason: Some("403".to_string()),
            ..Default::default()
        });
        assert!(forbidden.is_restricted());
        assert_eq!(forbidden.restriction_reason().as_deref(), Some("403"));

        let mut validation = sample_account();
        validation.validation_blocked = true;
        assert!(validation.is_restricted());
        assert_eq!(
            validation.restriction_reason().as_deref(),
            Some("Validation Blocked")
        );

        let mut disabled = sample_account();
        disabled.disabled = true;
        disabled.disabled_reason = Some("invalid_grant".to_string());
        assert!(disabled.is_restricted());
        assert_eq!(
            disabled.restriction_reason().as_deref(),
            Some("invalid_grant")
        );
    }
}
