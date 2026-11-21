use serde::{Deserialize, Serialize};

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
            _ => Err(format!("Unknown target host: {}. Available: all, app, ide, plugin, agy", s)),
        }
    }
}

/// OAuth 令牌信息
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenInfo {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub expiry_timestamp: Option<i64>,
    #[serde(default)]
    pub project_id: Option<String>,
}

/// 账号实体 (存储在 ~/.antigravity_tools/accounts/<id>.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub email: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub priority: Option<i32>,
    pub token: TokenInfo,
    #[serde(default)]
    pub device_profile: Option<DeviceProfile>,
    #[serde(default)]
    pub quota: Option<QuotaSummary>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub disabled_reason: Option<String>,
    #[serde(default)]
    pub banned: bool,
    #[serde(default)]
    pub created_at: Option<i64>,
    #[serde(default)]
    pub last_used: Option<i64>,
}

/// 账号索引摘要 (存储在 accounts.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSummary {
    pub id: String,
    pub email: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub created_at: Option<i64>,
    #[serde(default)]
    pub last_used: Option<i64>,
}

/// 配额明细 (完全兼容主分支 accounts.json 与历史格式)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuotaSummary {
    #[serde(default)]
    pub gemini_percentage: Option<f64>,
    #[serde(default)]
    pub claude_percentage: Option<f64>,
    #[serde(default)]
    pub models: Vec<ModelQuota>,
    #[serde(default)]
    pub last_updated: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelQuota {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub percentage: f64,
    #[serde(default)]
    pub reset_time: Option<String>,
    #[serde(default)]
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
    pub ide_account: Option<String>,
    pub ide_running: bool,
    pub plugin_account: Option<String>,
    pub plugin_running: bool,
    pub agy_account: Option<String>,
}
