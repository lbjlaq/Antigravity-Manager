use super::{quota::QuotaData, token::TokenData};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub(crate) fn default_priority() -> u8 {
    50
}

pub(crate) fn validate_priority(priority: u8) -> Result<(), String> {
    if !(1..=100).contains(&priority) {
        return Err("priority must be an integer between 1 and 100".to_string());
    }
    Ok(())
}

pub(crate) fn deserialize_priority<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<u8, D::Error> {
    let priority = u8::deserialize(deserializer)?;
    validate_priority(priority).map_err(serde::de::Error::custom)?;
    Ok(priority)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveLimitStatus {
    pub model: String,
    pub status: u16,
    pub reason: String,
    pub until: i64,
    pub detected_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// 账号数据结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    /// Lower values are selected first when assigning a new proxy account.
    #[serde(
        default = "default_priority",
        deserialize_with = "deserialize_priority"
    )]
    pub priority: u8,
    pub token: TokenData,
    /// 可选的设备指纹，用于切换账号时固定机器信息
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_profile: Option<DeviceProfile>,
    /// 设备指纹历史（生成/采集时记录），不含基线
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub device_history: Vec<DeviceProfileVersion>,
    pub quota: Option<QuotaData>,
    /// Disabled accounts are ignored by the proxy token pool (e.g. revoked refresh_token -> invalid_grant).
    #[serde(default)]
    pub disabled: bool,
    /// Optional human-readable reason for disabling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
    /// Unix timestamp when the account was disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_at: Option<i64>,
    /// User manually disabled proxy feature (does not affect app usage).
    #[serde(default)]
    pub proxy_disabled: bool,
    /// Optional human-readable reason for proxy disabling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_disabled_reason: Option<String>,
    /// Unix timestamp when the proxy was disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_disabled_at: Option<i64>,
    /// 受配额保护禁用的模型列表 [NEW #621]
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub protected_models: HashSet<String>,
    /// Temporary live upstream throttles observed from actual generation requests.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub live_limited_models: HashMap<String, LiveLimitStatus>,
    /// [NEW] 403 验证阻止状态 (VALIDATION_REQUIRED)
    #[serde(default)]
    pub validation_blocked: bool,
    /// [NEW] 验证阻止截止时间戳
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_blocked_until: Option<i64>,
    /// [NEW] 验证阻止原因
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_blocked_reason: Option<String>,
    /// [NEW] 验证链接 URL (#1522)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_url: Option<String>,
    pub created_at: i64,
    pub last_used: i64,
    /// 绑定的代理 ID (None = 使用全局代理池)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_id: Option<String>,
    /// 代理绑定时间
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_bound_at: Option<i64>,
    /// 用户自定义标签
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_label: Option<String>,
    /// [NEW] 运行时 QoS 状态 ("suspicious" | "jailed" | "probing" | "half_open")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qos_status: Option<String>,
    /// [NEW] 运行时 QoS 冷却剩余时间 (秒)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qos_reset_seconds: Option<u64>,
}

impl Account {
    pub fn new(id: String, email: String, token: TokenData) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id,
            email,
            name: None,
            priority: default_priority(),
            token,
            device_profile: None,
            device_history: Vec::new(),
            quota: None,
            disabled: false,
            disabled_reason: None,
            disabled_at: None,
            proxy_disabled: false,
            proxy_disabled_reason: None,
            proxy_disabled_at: None,
            protected_models: HashSet::new(),
            live_limited_models: HashMap::new(),
            validation_blocked: false,
            validation_blocked_until: None,
            validation_blocked_reason: None,
            validation_url: None,
            created_at: now,
            last_used: now,
            proxy_id: None,
            proxy_bound_at: None,
            custom_label: None,
            qos_status: None,
            qos_reset_seconds: None,
        }
    }

    pub fn update_last_used(&mut self) {
        self.last_used = chrono::Utc::now().timestamp();
    }

    pub fn update_quota(&mut self, mut quota: QuotaData) {
        // A failed/partial summary is not evidence that a previously observed bucket recovered.
        if let Some(existing) = &self.quota {
            if let Some(old_groups) = &existing.quota_groups {
                let groups = quota.quota_groups.get_or_insert_with(Vec::new);
                for old_group in old_groups {
                    let index = groups
                        .iter()
                        .position(|g| g.display_name == old_group.display_name)
                        .unwrap_or_else(|| {
                            groups.push(crate::models::quota::QuotaGroup {
                                buckets: Vec::new(),
                                ..old_group.clone()
                            });
                            groups.len() - 1
                        });
                    for old_bucket in &old_group.buckets {
                        let mut previous = old_bucket.clone();
                        let observed_at = *previous
                            .observed_at
                            .get_or_insert(existing.last_updated.saturating_mul(1000));
                        if let Some(current) = groups[index]
                            .buckets
                            .iter_mut()
                            .find(|b| b.bucket_id == previous.bucket_id)
                        {
                            let current_observed_at = current
                                .observed_at
                                .unwrap_or(quota.last_updated.saturating_mul(1000));
                            if current_observed_at <= observed_at {
                                *current = previous;
                            }
                        } else {
                            groups[index].buckets.push(previous);
                        }
                    }
                }
            }
        }
        if quota.subscription_tier.is_none() {
            if let Some(ref existing) = self.quota {
                quota.subscription_tier = existing.subscription_tier.clone();
            }
        }
        quota.ensure_subscription_tier();

        // 如果账号当前模型列表中无任何 Claude 或 GPT 模型（例如不支持 Opus 5.5 且 4.6 / GPT 均已下线），
        // 彻底清理历史保留的虚假 3P 配额分组，杜绝幽灵配额复活并留空展示
        let has_claude_or_gpt = quota.models.iter().any(|m| {
            let n = m.name.to_lowercase();
            n.starts_with("claude") || n.starts_with("gpt")
        });
        if !has_claude_or_gpt {
            if let Some(ref mut groups) = quota.quota_groups {
                groups.retain(|g| {
                    let gname = g.display_name.to_lowercase();
                    !gname.contains("claude") && !gname.contains("gpt") && !gname.contains("3p")
                });
            }
        }

        self.quota = Some(quota);
    }
}

/// 账号索引数据（accounts.json）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountIndex {
    pub version: String,
    pub accounts: Vec<AccountSummary>,
    pub current_account_id: Option<String>,
    #[serde(default)]
    pub current_target_ide: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_classic_account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_ide_account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_agy_account_id: Option<String>,
}

/// 客户端独立当前账号状态（同时指示经典款、Antigravity IDE、Antigravity CLI 三端状态）
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ActiveClientsStatus {
    pub classic_account_id: Option<String>,
    pub classic_email: Option<String>,
    pub ide_account_id: Option<String>,
    pub ide_email: Option<String>,
    pub agy_account_id: Option<String>,
    pub agy_email: Option<String>,
}

/// 账号摘要信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSummary {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub proxy_disabled: bool,
    /// 受保护的模型列表 [NEW] 供 UI 显示锁定图标
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub protected_models: HashSet<String>,
    pub created_at: i64,
    pub last_used: i64,
}

impl AccountIndex {
    pub fn new() -> Self {
        Self {
            version: "2.0".to_string(),
            accounts: Vec::new(),
            current_account_id: None,
            current_target_ide: None,
            current_classic_account_id: None,
            current_ide_account_id: None,
            current_agy_account_id: None,
        }
    }
}

impl Default for AccountIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// 设备指纹（storage.json 中 telemetry 相关字段）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfile {
    pub machine_id: String,
    pub mac_machine_id: String,
    pub dev_device_id: String,
    pub sqm_id: String,
}

/// 指纹历史版本
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfileVersion {
    pub id: String,
    pub created_at: i64,
    pub label: String,
    pub profile: DeviceProfile,
    #[serde(default)]
    pub is_current: bool,
}

/// 导出账号项（用于备份/迁移）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountExportItem {
    pub email: String,
    pub refresh_token: String,
}

/// [NEW #3726] 批量导入输入项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportAccountEntry {
    #[serde(default)]
    pub email: Option<String>,
    pub refresh_token: String,
}

/// [NEW #3726] 批量导入实时进度事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchImportProgress {
    pub current: usize,
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_email: Option<String>,
}

/// [NEW #3726] 批量导入最终统计结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchImportResult {
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

/// 导出账号响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountExportResponse {
    pub accounts: Vec<AccountExportItem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_priority_defaults_and_rejects_invalid_values() {
        let token = TokenData::new(
            "test".into(),
            "test".into(),
            3600,
            None,
            None,
            None,
            false,
            None,
        );
        let account = Account::new("test".into(), "test@test.invalid".into(), token);
        assert_eq!(account.priority, 50);
        let mut json = serde_json::to_value(account).unwrap();
        json.as_object_mut().unwrap().remove("priority");
        assert_eq!(
            serde_json::from_value::<Account>(json.clone())
                .unwrap()
                .priority,
            50
        );
        for priority in [1, 100] {
            json["priority"] = serde_json::json!(priority);
            assert_eq!(
                serde_json::from_value::<Account>(json.clone())
                    .unwrap()
                    .priority,
                priority
            );
        }
        for value in [
            serde_json::json!(0),
            serde_json::json!(101),
            serde_json::json!(-1),
            serde_json::json!(1.5),
            serde_json::json!("50"),
            serde_json::Value::Null,
        ] {
            json["priority"] = value;
            assert!(serde_json::from_value::<Account>(json.clone()).is_err());
        }
        assert!(
            crate::modules::account::update_account_priority("missing", 0)
                .unwrap_err()
                .contains("priority")
        );
        assert!(
            crate::modules::account::update_account_priority("missing", 101)
                .unwrap_err()
                .contains("priority")
        );
    }

    #[test]
    fn test_update_quota_purges_phantom_3p_groups_when_no_claude_or_gpt_models() {
        let token = TokenData::new(
            "test".into(),
            "test".into(),
            3600,
            None,
            None,
            None,
            false,
            None,
        );
        let mut account = Account::new("test".into(), "test@test.invalid".into(), token);

        // 先给账号注入包含 Claude 分组与模型的数据
        let mut initial_quota = QuotaData::new();
        initial_quota.models = vec![crate::models::quota::ModelQuota {
            name: "claude-sonnet-4-6".to_string(),
            percentage: 100,
            reset_time: "2026-11-08T00:00:00Z".to_string(),
            display_name: None,
            supports_images: None,
            supports_thinking: None,
            thinking_budget: None,
            recommended: None,
            max_tokens: None,
            max_output_tokens: None,
            model: None,
            supported_mime_types: None,
        }];
        initial_quota.quota_groups = Some(vec![
            crate::models::quota::QuotaGroup {
                display_name: "Gemini Models".to_string(),
                description: None,
                buckets: vec![],
            },
            crate::models::quota::QuotaGroup {
                display_name: "Claude and GPT models".to_string(),
                description: None,
                buckets: vec![],
            },
        ]);
        account.update_quota(initial_quota);
        assert_eq!(
            account
                .quota
                .as_ref()
                .unwrap()
                .quota_groups
                .as_ref()
                .unwrap()
                .len(),
            2
        );

        // 现在模拟新一轮上游数据：Claude 4.6 下线，账号中仅剩 Gemini 模型
        let mut updated_quota = QuotaData::new();
        updated_quota.models = vec![crate::models::quota::ModelQuota {
            name: "gemini-3.8-flash-high".to_string(),
            percentage: 80,
            reset_time: "2026-11-05T00:00:00Z".to_string(),
            display_name: None,
            supports_images: None,
            supports_thinking: None,
            thinking_budget: None,
            recommended: None,
            max_tokens: None,
            max_output_tokens: None,
            model: None,
            supported_mime_types: None,
        }];
        updated_quota.quota_groups = Some(vec![
            crate::models::quota::QuotaGroup {
                display_name: "Gemini Models".to_string(),
                description: None,
                buckets: vec![],
            },
            crate::models::quota::QuotaGroup {
                display_name: "Claude and GPT models".to_string(),
                description: None,
                buckets: vec![],
            },
        ]);

        account.update_quota(updated_quota);

        let final_groups = account
            .quota
            .as_ref()
            .unwrap()
            .quota_groups
            .as_ref()
            .unwrap();
        // 3P 分组必须被彻底剥离留空，不得残留任何 Claude and GPT models 分组
        assert_eq!(final_groups.len(), 1);
        assert_eq!(final_groups[0].display_name, "Gemini Models");
    }
}
