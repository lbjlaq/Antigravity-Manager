use serde::{Deserialize, Serialize};
use super::{token::TokenData, quota::QuotaData};

/// 账号数据结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
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
    pub created_at: i64,
    pub last_used: i64,
}

impl Account {
    pub fn new(id: String, email: String, token: TokenData) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id,
            email,
            name: None,
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
            created_at: now,
            last_used: now,
        }
    }

    pub fn update_last_used(&mut self) {
        self.last_used = chrono::Utc::now().timestamp();
    }

    pub fn update_quota(&mut self, quota: QuotaData) {
        self.quota = Some(quota);
    }
}

/// 账号索引数据（accounts.json）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountIndex {
    pub version: String,
    pub accounts: Vec<AccountSummary>,
    pub current_account_id: Option<String>,
}

/// 账号摘要信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSummary {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub created_at: i64,
    pub last_used: i64,
}

impl AccountIndex {
    pub fn new() -> Self {
        Self {
            version: "2.0".to_string(),
            accounts: Vec::new(),
            current_account_id: None,
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
