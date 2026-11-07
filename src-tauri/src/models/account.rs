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

/// 上游封禁证据（如 TOS_VIOLATION）。
///
/// Daily/Sandbox 配额端点对被封禁账号仍会返回 200，因此不能把「配额刷新成功」
/// 当作解封依据；只有生产端点（cloudcode-pa.googleapis.com）明确给出封禁原因时
/// 才写入该结构，并在生产端点恢复 200 后才清除。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountBanInfo {
    /// 上游 reason code，如 `TOS_VIOLATION`。
    pub code: String,
    /// 上游返回的可读错误消息。
    pub message: String,
    /// 申诉链接（若上游提供）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appeal_url: Option<String>,
}

impl AccountBanInfo {
    /// 判断一次上游响应是否属于账号级封禁。
    ///
    /// 只接受 403；429/5xx/网络错误都属于「状态未知」，绝不能据此封号。
    pub fn classify_upstream(status: u16, body: &str) -> Option<Self> {
        if status != 403 {
            return None;
        }
        Self::classify_text(body)
    }

    /// 从 403 响应体（原始 JSON 或已拼接的日志文本）中识别封禁证据。
    pub fn classify_text(body: &str) -> Option<Self> {
        let lower = body.to_lowercase();

        // 只保留高置信、账号级 TOS 封禁信号。`suspicious_activity`、泛化的
        // `third-party`、临时 `account is locked` 等词会把风控/验证 403 误升级为
        // 永久封禁；新增关键词前必须先用真实封禁样本验证并补负向测试。
        const BAN_REASON_CODES: [&str; 9] = [
            "tos_violation",
            "terms_of_service_violation",
            "policy_violation",
            "abuse_detected",
            "account_suspended",
            "account_banned",
            "user_disabled",
            "account_disabled",
            "service_disabled",
        ];
        const BAN_PHRASES: [&str; 6] = [
            "violation of terms of service",
            "violation of the terms of service",
            "service has been disabled in this account",
            "account has been suspended",
            "account is suspended",
            "account has been disabled",
        ];

        let reason_code = BAN_REASON_CODES
            .iter()
            .find(|code| lower.contains(*code))
            .map(|code| (*code).to_string());
        let has_phrase = BAN_PHRASES.iter().any(|phrase| lower.contains(*phrase));

        if reason_code.is_none() && !has_phrase {
            return None;
        }

        let mut message: Option<String> = None;
        let mut code = reason_code;
        let mut appeal_url: Option<String> = None;

        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) {
            Self::extract_from_json(&parsed, &mut message, &mut code, &mut appeal_url);

            // 部分上游会把内层 JSON 序列化进 error 字符串。
            if let Some(inner) = parsed.get("error").and_then(|value| value.as_str()) {
                if let Ok(inner_parsed) = serde_json::from_str::<serde_json::Value>(inner) {
                    Self::extract_from_json(
                        &inner_parsed,
                        &mut message,
                        &mut code,
                        &mut appeal_url,
                    );
                }
            }
        }

        let fallback_message = body
            .trim()
            .strip_prefix("Forbidden (403): ")
            .unwrap_or_else(|| body.trim());
        let message = message
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                if fallback_message.is_empty() {
                    "Upstream account ban detected".to_string()
                } else {
                    fallback_message.chars().take(800).collect()
                }
            });

        // JSON 解析失败（例如日志前缀拼接）时，从原始文本里兜底提取完整 URL。
        // 这里必须容忍 `https:\/\/...` 这类转义形式，且不能把查询串里的 `&` 当成终点。
        if appeal_url.is_none() {
            appeal_url = Self::extract_url_from_text(body);
        }

        Some(Self {
            code: code.unwrap_or_else(|| "BANNED".to_string()),
            message,
            appeal_url: appeal_url.map(|url| {
                url.replace("\\u0026", "&")
                    .trim()
                    .trim_end_matches(|c: char| matches!(c, ',' | '"' | '\\' | '}' | ']'))
                    .to_string()
            }),
        })
    }

    /// 从任意文本中提取完整 URL（支持 JSON 转义的 `https:\/\/...`）。
    ///
    /// 不能使用 `[^\\s"'\\]` 这类排除反斜杠的字符类：上游 JSON 常把斜杠转义成
    /// `\/`，那样会在 `https:\` 处截断，导致复制出的申诉链接不完整。
    pub fn extract_url_from_text(text: &str) -> Option<String> {
        // 优先从 appeal_url / validation_url 字段之后取 URL，避免误取错误消息里的
        // 说明性链接（例如文档首页）而不是真正的申诉/验证入口。
        let source = text
            .find("appeal_url")
            .map(|index| &text[index..])
            .or_else(|| text.find("validation_url").map(|index| &text[index..]))
            .unwrap_or(text);

        let regex = regex::Regex::new(r#"https?:\\?/\\?/[^\s"']+"#).ok()?;
        let matched = regex.find(source)?.as_str();
        let decoded = matched
            .replace("\\/", "/")
            .replace("\\u0026", "&")
            .replace("\\u003d", "=")
            .replace("&amp;", "&");
        let trimmed = decoded
            .trim_end_matches(|c: char| matches!(c, ',' | '"' | '\\' | '}' | ']' | ')' | '.'));
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            Some(trimmed.to_string())
        } else {
            None
        }
    }

    fn extract_from_json(
        parsed: &serde_json::Value,
        message: &mut Option<String>,
        code: &mut Option<String>,
        appeal_url: &mut Option<String>,
    ) {
        let error = parsed.get("error").unwrap_or(parsed);

        if message.is_none() {
            if let Some(value) = error.get("message").and_then(|value| value.as_str()) {
                *message = Some(value.to_string());
            }
        }

        if let Some(details) = error.get("details").and_then(|value| value.as_array()) {
            for detail in details {
                if let Some(reason) = detail.get("reason").and_then(|value| value.as_str()) {
                    let lower_reason = reason.to_lowercase();
                    // 只接受与封禁语义相关的 reason code，避免被
                    // SUBSCRIPTION_REQUIRED / PERMISSION_DENIED 等泛化原因覆盖。
                    if ["violation", "suspend", "disab", "ban"]
                        .iter()
                        .any(|marker| lower_reason.contains(marker))
                    {
                        *code = Some(reason.to_string());
                    }
                }

                if appeal_url.is_none() {
                    if let Some(url) = detail
                        .get("metadata")
                        .and_then(|metadata| metadata.get("appeal_url"))
                        .and_then(|value| value.as_str())
                    {
                        *appeal_url = Some(url.to_string());
                    }
                }

                if appeal_url.is_none() {
                    if let Some(url) = detail
                        .get("links")
                        .and_then(|value| value.as_array())
                        .and_then(|links| {
                            links.iter().find_map(|link| {
                                let description = link
                                    .get("description")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or_default()
                                    .to_lowercase();
                                if description.contains("appeal") {
                                    link.get("url")
                                        .and_then(|value| value.as_str())
                                        .map(|value| value.to_string())
                                } else {
                                    None
                                }
                            })
                        })
                    {
                        *appeal_url = Some(url);
                    }
                }
            }
        }

        if appeal_url.is_none() {
            if let Some(url) = error.get("appeal_url").and_then(|value| value.as_str()) {
                *appeal_url = Some(url.to_string());
            }
        }
    }
}

/// 账号锁定原因分类。
///
/// 403/封禁的持久化状态会在日配额刷新、验证解除、定向测活等多个路径里读写；
/// 统一在这里分类，避免各处用略有差异的字符串前缀判断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccountLockKind {
    /// 上游确认的账号级封禁（`banned: ...`）。
    Banned,
    /// 明确的生产端点 403 锁定。
    HardForbidden,
    /// 人机验证/风控挑战，解除后应允许恢复。
    Validation,
    /// 配额保护，不是 403。
    QuotaProtection,
    /// 用户手动或其他原因。
    Manual,
}

pub(crate) fn classify_lock_reason(reason: &str) -> AccountLockKind {
    let lower = reason.trim().to_ascii_lowercase();
    if lower.starts_with("banned:") {
        return AccountLockKind::Banned;
    }
    if lower.contains("validation_required")
        || lower.contains("verify your account")
        || lower.contains("validation_url")
        || lower.contains("further action is required")
    {
        return AccountLockKind::Validation;
    }
    if lower.starts_with("forbidden (403)") || lower.contains("403 forbidden") {
        return AccountLockKind::HardForbidden;
    }
    if lower == "quota_protection" {
        return AccountLockKind::QuotaProtection;
    }
    AccountLockKind::Manual
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
    /// 上游已确认的账号封禁（如 TOS_VIOLATION）。
    ///
    /// 与 `disabled` / `proxy_disabled` 不同：该状态只由生产端点探测结果驱动，
    /// 配额刷新成功不会清除它，只有生产端点恢复 200 才会自动解封。
    #[serde(default)]
    pub banned: bool,
    /// 封禁原因码（如 `TOS_VIOLATION`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banned_code: Option<String>,
    /// 封禁原因（上游可读错误消息）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banned_reason: Option<String>,
    /// 封禁发现时间戳。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banned_at: Option<i64>,
    /// 封禁申诉链接（若上游提供）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ban_appeal_url: Option<String>,
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
            banned: false,
            banned_code: None,
            banned_reason: None,
            banned_at: None,
            ban_appeal_url: None,
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

    /// 当前账号是否处于上游确认的封禁状态。
    pub fn is_banned(&self) -> bool {
        self.banned
    }

    /// 将持久化的封禁状态还原为结构化证据。
    pub fn ban_info(&self) -> Option<AccountBanInfo> {
        if !self.banned {
            return None;
        }
        Some(AccountBanInfo {
            code: self
                .banned_code
                .clone()
                .unwrap_or_else(|| "BANNED".to_string()),
            message: self.banned_reason.clone().unwrap_or_default(),
            appeal_url: self.ban_appeal_url.clone(),
        })
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

        // Daily/Sandbox 配额端点对被封禁与 403 账号仍返回 200；一次成功的配额快照
        // 不能抵消生产端点确认过的封禁与 403 锁定。
        // 只有 `clear_account_ban` 或生产端点测活成功 `clear_account_forbidden_and_validation` 才能解除。
        // 验证锁（VALIDATION_REQUIRED / validation_url）是临时风控状态，必须允许日配额成功清除，
        // 否则用户完成验证后账号会永久卡在 is_forbidden。
        let is_validation_lock = self.validation_blocked
            || self
                .proxy_disabled_reason
                .as_deref()
                .is_some_and(|reason| classify_lock_reason(reason) == AccountLockKind::Validation)
            || self
                .quota
                .as_ref()
                .and_then(|quota| quota.forbidden_reason.as_deref())
                .is_some_and(|reason| classify_lock_reason(reason) == AccountLockKind::Validation);

        // 账号级封禁优先级最高：即使同时存在 validation_blocked / validation_required，
        // banned 也必须继续压住 is_forbidden 并清空过期配额。
        let was_forbidden = self.banned
            || (!is_validation_lock
                && (self
                    .quota
                    .as_ref()
                    .is_some_and(|existing| existing.is_forbidden)
                    || (self.proxy_disabled
                        && self.proxy_disabled_reason.as_deref().is_some_and(|reason| {
                            matches!(
                                classify_lock_reason(reason),
                                AccountLockKind::Banned | AccountLockKind::HardForbidden
                            )
                        }))));

        if was_forbidden {
            quota.is_forbidden = true;
            if quota.forbidden_reason.is_none() {
                quota.forbidden_reason = self
                    .banned_reason
                    .clone()
                    .or_else(|| self.quota.as_ref().and_then(|e| e.forbidden_reason.clone()))
                    .or_else(|| self.proxy_disabled_reason.clone());
            }
            if self.banned {
                // 封禁期间不保留过期配额，避免 API/UI 误读为「仍有 100% 可用」。
                quota.models.clear();
                quota.quota_groups = None;
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

    #[test]
    fn classifies_tos_violation_ban_response() {
        let body = r#"{
            "error": {
                "code": 403,
                "message": "This service has been disabled in this account for violation of Terms of Service. Please submit an appeal to continue using this product.",
                "status": "PERMISSION_DENIED",
                "details": [{
                    "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                    "reason": "TOS_VIOLATION",
                    "domain": "cloudcode-pa.googleapis.com",
                    "metadata": {
                        "uiMessage": "true",
                        "appeal_url": "https://forms.gle/hGzM9MEUv2azZsrb9",
                        "appeal_url_link_text": "Submit Appeal"
                    }
                }]
            }
        }"#;

        let info = AccountBanInfo::classify_upstream(403, body).expect("must detect TOS ban");
        assert_eq!(info.code, "TOS_VIOLATION");
        assert!(info.message.contains("violation of Terms of Service"));
        assert_eq!(
            info.appeal_url.as_deref(),
            Some("https://forms.gle/hGzM9MEUv2azZsrb9")
        );
    }

    #[test]
    fn ignores_non_ban_403_and_non_403_statuses() {
        let validation = r#"{"error":{"code":403,"message":"Validation required","status":"PERMISSION_DENIED","details":[{"reason":"VALIDATION_REQUIRED","metadata":{"validation_url":"https://example.test/verify"}}]}}"#;
        assert!(AccountBanInfo::classify_upstream(403, validation).is_none());

        let ban_body = r#"{"error":{"code":403,"message":"violation of Terms of Service","status":"PERMISSION_DENIED"}}"#;
        assert!(AccountBanInfo::classify_upstream(429, ban_body).is_none());
        assert!(AccountBanInfo::classify_upstream(500, ban_body).is_none());

        let model_suspension = r#"{"error":{"code":403,"message":"This model is temporarily suspended for this account"}}"#;
        assert!(
            AccountBanInfo::classify_upstream(403, model_suspension).is_none(),
            "model-level suspension must not ban the whole account"
        );
    }

    #[test]
    fn extracts_full_escaped_appeal_url_without_truncation() {
        let raw = r#"Forbidden (403): {"error":{"message":"violation of Terms of Service","details":[{"metadata":{"appeal_url":"https:\/\/example.com\/appeal?token=abc\u0026next=https%3A%2F%2Fx.test%2Fa"}}]}}"#;
        let url = AccountBanInfo::extract_url_from_text(raw).expect("must extract URL");
        assert_eq!(
            url,
            "https://example.com/appeal?token=abc&next=https%3A%2F%2Fx.test%2Fa"
        );

        let info = AccountBanInfo::classify_text(raw).expect("must classify ban");
        assert_eq!(
            info.appeal_url.as_deref(),
            Some("https://example.com/appeal?token=abc&next=https%3A%2F%2Fx.test%2Fa")
        );
    }

    #[test]
    fn banned_account_keeps_quota_forbidden_after_successful_refresh() {
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
        account.banned = true;
        account.banned_code = Some("TOS_VIOLATION".into());
        account.banned_reason = Some("violation of Terms of Service".into());

        let mut fresh_quota = QuotaData::new();
        fresh_quota.is_forbidden = false;
        account.update_quota(fresh_quota);

        let quota = account.quota.as_ref().unwrap();
        assert!(
            quota.is_forbidden,
            "daily quota success must not lift a ban"
        );
        assert_eq!(
            quota.forbidden_reason.as_deref(),
            Some("violation of Terms of Service")
        );
    }

    #[test]
    fn forbidden_account_keeps_quota_forbidden_after_successful_daily_refresh() {
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
        account.banned = false;
        account.proxy_disabled = true;
        // 账号级 403 必须保持；模型级 403 是否整号停用属于独立策略，不在本测试断言。
        account.proxy_disabled_reason = Some("Forbidden (403): Location not supported".into());
        let mut q = QuotaData::new();
        q.is_forbidden = true;
        q.forbidden_reason = Some("Forbidden (403): Location not supported".into());
        account.quota = Some(q);

        // Daily quota snapshot returns 200 with is_forbidden: false
        let mut fresh_quota = QuotaData::new();
        fresh_quota.is_forbidden = false;
        account.update_quota(fresh_quota);

        let quota = account.quota.as_ref().unwrap();
        assert!(
            quota.is_forbidden,
            "403 锁定账号接收日配额快照时不得丢失 is_forbidden 状态"
        );
        assert_eq!(
            quota.forbidden_reason.as_deref(),
            Some("Forbidden (403): Location not supported")
        );
    }

    #[test]
    fn does_not_classify_generic_third_party_or_appeal_only_response() {
        let body = r#"{
            "error": {
                "code": 403,
                "message": "Account blocked due to detection of unauthorized third-party software. Appeal here: https://docs.google.com/forms/d/e/1FAIpQLScOJWibQ_-hYuVv63kJTgEqlgAaOwRLGFTbXm-QY1gz8U0CGA/viewform",
                "status": "PERMISSION_DENIED"
            }
        }"#;

        assert!(
            AccountBanInfo::classify_upstream(403, body).is_none(),
            "泛化 third-party 文本与 appeal 链接不能单独作为账号级封禁证据；如确有此类封禁，需先引入结构化 reason 白名单并用真实样本验证"
        );
    }

    #[test]
    fn ignores_risk_control_and_temporary_lock_responses() {
        let suspicious = r#"{"error":{"code":403,"message":"We detected unusual activity. Verify your account.","details":[{"reason":"SUSPICIOUS_ACTIVITY","metadata":{"validation_url":"https://example.test/verify"}}]}}"#;
        assert!(
            AccountBanInfo::classify_upstream(403, suspicious).is_none(),
            "risk-control/validation 403 不得升级为永久封禁"
        );

        let null_appeal = r#"{"error":{"code":403,"message":"User not authorized to access model claude-3-5-sonnet","details":[{"reason":"MODEL_ACCESS_DENIED","metadata":{"appeal_url":null}}]}}"#;
        assert!(
            AccountBanInfo::classify_upstream(403, null_appeal).is_none(),
            "appeal_url key 存在但值为 null 不得作为封禁证据"
        );

        let temporary_lock = r#"{"error":{"code":403,"message":"This account is locked for 60 minutes due to repeated requests."}}"#;
        assert!(
            AccountBanInfo::classify_upstream(403, temporary_lock).is_none(),
            "临时锁定不得当成永久封禁"
        );
    }

    #[test]
    fn classifies_lock_reasons_case_insensitively() {
        assert_eq!(
            classify_lock_reason("banned: TOS_VIOLATION"),
            AccountLockKind::Banned
        );
        assert_eq!(
            classify_lock_reason("Forbidden (403): Location not supported"),
            AccountLockKind::HardForbidden
        );
        assert_eq!(
            classify_lock_reason("validation_required"),
            AccountLockKind::Validation
        );
        assert_eq!(
            classify_lock_reason("VALIDATION_REQUIRED"),
            AccountLockKind::Validation
        );
        assert_eq!(
            classify_lock_reason("quota_protection"),
            AccountLockKind::QuotaProtection
        );
        assert_eq!(classify_lock_reason("manual"), AccountLockKind::Manual);
    }

    #[test]
    fn banned_account_takes_precedence_over_validation_lock() {
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
        account.banned = true;
        account.banned_code = Some("TOS_VIOLATION".into());
        account.banned_reason = Some("violation of Terms of Service".into());
        account.validation_blocked = true;
        account.validation_blocked_reason = Some("VALIDATION_REQUIRED".into());
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("validation_required".into());
        let mut q = QuotaData::new();
        q.is_forbidden = true;
        q.forbidden_reason = Some("VALIDATION_REQUIRED".into());
        account.quota = Some(q);

        let mut fresh_quota = QuotaData::new();
        fresh_quota.is_forbidden = false;
        account.update_quota(fresh_quota);

        let quota = account.quota.as_ref().unwrap();
        assert!(
            quota.is_forbidden,
            "账号级封禁必须优先于 validation 锁，不能被日配额 200 冲掉"
        );
        assert_eq!(
            quota.forbidden_reason.as_deref(),
            Some("violation of Terms of Service")
        );
        assert!(account.banned);
    }

    #[test]
    fn validation_lock_is_not_preserved_as_hard_forbidden() {
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
        account.validation_blocked = true;
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("validation_required".into());
        let mut q = QuotaData::new();
        q.is_forbidden = true;
        q.forbidden_reason = Some("VALIDATION_REQUIRED: human captcha".into());
        account.quota = Some(q);

        let mut fresh_quota = QuotaData::new();
        fresh_quota.is_forbidden = false;
        account.update_quota(fresh_quota);

        assert!(
            !account.quota.as_ref().unwrap().is_forbidden,
            "验证锁不得被当成永久 403 保留"
        );
    }
}
