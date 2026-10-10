use crate::modules::cloudflared::CloudflaredConfig;
use crate::proxy::ProxyConfig;
use serde::{Deserialize, Serialize};

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub language: String,
    pub theme: String,
    pub auto_refresh: bool,
    pub refresh_interval: i32, // minutes
    pub auto_sync: bool,
    pub sync_interval: i32, // minutes
    pub default_export_path: Option<String>,
    #[serde(default)]
    pub proxy: ProxyConfig,
    pub antigravity_executable: Option<String>, // [NEW] Manually specified Antigravity executable path
    pub antigravity_ide_executable: Option<String>, // [NEW] Manually specified Antigravity IDE executable path
    pub antigravity_cli_executable: Option<String>, // [NEW] Manually specified Antigravity CLI (agy) path
    pub antigravity_args: Option<Vec<String>>,      // [NEW] Antigravity startup arguments
    #[serde(default)]
    pub auto_launch: bool,     // Launch on startup
    /// Login-item launches stay in the tray. Missing values stay on so existing
    /// autostart entries, which already pass `--minimized`, keep that behavior.
    #[serde(default = "default_quiet_autostart")]
    pub quiet_autostart: bool,
    #[serde(default)]
    pub scheduled_warmup: ScheduledWarmupConfig, // [NEW] Scheduled warmup configuration
    #[serde(default)]
    pub phase_scheduler: PhaseSchedulerConfig, // 多账号相控阵智能错峰调度配置
    #[serde(default)]
    pub quota_protection: QuotaProtectionConfig, // [NEW] Quota protection configuration
    #[serde(default)]
    pub pinned_quota_models: PinnedQuotaModelsConfig, // [NEW] Pinned quota models list
    #[serde(default)]
    pub circuit_breaker: CircuitBreakerConfig, // [NEW] Circuit breaker configuration
    #[serde(default)]
    pub hidden_menu_items: Vec<String>, // Hidden menu item path list
    #[serde(default)]
    pub cloudflared: CloudflaredConfig, // [NEW] Cloudflared configuration
    #[serde(default)]
    pub lightweight_mode: bool, // [NEW] Lightweight mode: destroy webview on minimize/close to tray
    #[serde(default)]
    pub suggestion_delete_thinking_store: Option<bool>, // [NEW] 建议删除历史思考块缓存开关
    #[serde(default)]
    pub thinking_cleanup_dismissed: Option<bool>, // [NEW] 用户是否已确认/忽略该建议
    #[serde(default)]
    pub dismissed_thinking_cleanup_version: Option<String>, // [NEW] 用户已确认或忽略建议的目标版本号
}

fn default_quiet_autostart() -> bool {
    true
}

/// 多账号相控阵智能调度配置 (Phase Scheduler Configuration)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseSchedulerConfig {
    /// 是否开启相控阵智能调度
    pub enabled: bool,
    /// 运行模式: "steady" (平稳续航模式) | "burst" (限定时间狂暴模式)
    #[serde(default = "default_phase_mode")]
    pub mode: String,
    /// 平稳模式开工基准时间 (格式: "HH:MM", 如 "09:00")
    #[serde(default = "default_work_start_time")]
    pub work_start_time: String,
    /// 平稳模式计划工作时长 (小时, 如 12)
    #[serde(default = "default_work_duration")]
    pub work_duration_hours: u32,
    /// 狂暴模式持续时长 (小时, 如 3)
    #[serde(default = "default_burst_duration")]
    pub burst_duration_hours: u32,
    /// 狂暴模式触发类型: "scheduled" (预约爆发) | "immediate" (立即爆发)
    #[serde(default = "default_burst_mode_type")]
    pub burst_mode_type: String,
    /// 狂暴模式预约开始时间 (格式: "HH:MM", 如 "20:00")
    #[serde(default = "default_burst_start_time")]
    pub burst_start_time: String,
    /// 是否启用 Mac 原生硬件定时暗唤醒 (RTC wake)
    #[serde(default = "default_auto_dark_wake")]
    pub auto_dark_wake: bool,
    /// 优先监控与预热的模型清单
    #[serde(default = "default_warmup_models")]
    pub monitored_models: Vec<String>,
}

fn default_phase_mode() -> String {
    "steady".to_string()
}

fn default_work_start_time() -> String {
    "09:00".to_string()
}

fn default_work_duration() -> u32 {
    12
}

fn default_burst_duration() -> u32 {
    3
}

fn default_burst_mode_type() -> String {
    "scheduled".to_string()
}

fn default_burst_start_time() -> String {
    "20:00".to_string()
}

fn default_auto_dark_wake() -> bool {
    true
}

impl PhaseSchedulerConfig {
    pub fn new() -> Self {
        Self {
            enabled: false,
            mode: default_phase_mode(),
            work_start_time: default_work_start_time(),
            work_duration_hours: default_work_duration(),
            burst_duration_hours: default_burst_duration(),
            burst_mode_type: default_burst_mode_type(),
            burst_start_time: default_burst_start_time(),
            auto_dark_wake: default_auto_dark_wake(),
            monitored_models: default_warmup_models(),
        }
    }
}

impl Default for PhaseSchedulerConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Scheduled warmup configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledWarmupConfig {
    /// Whether smart warmup is enabled
    pub enabled: bool,

    /// Warmup mode: "smart" (hybrid), "timer" (scheduled timer), "quota_full" (100% quota recovery)
    #[serde(default = "default_warmup_mode")]
    pub mode: String,

    /// Warmup interval in minutes for timer/smart mode
    #[serde(default = "default_warmup_interval")]
    pub interval_minutes: u64,

    /// List of models to warmup
    #[serde(default = "default_warmup_models")]
    pub monitored_models: Vec<String>,
}

fn default_warmup_mode() -> String {
    "smart".to_string()
}

fn default_warmup_interval() -> u64 {
    120
}

fn default_warmup_models() -> Vec<String> {
    vec![
        "gemini-3-flash".to_string(),
        "claude".to_string(),
        "gemini-3-pro-high".to_string(),
        "gemini-3.1-flash-image".to_string(),
    ]
}

impl ScheduledWarmupConfig {
    pub fn new() -> Self {
        Self {
            enabled: false,
            mode: default_warmup_mode(),
            interval_minutes: default_warmup_interval(),
            monitored_models: default_warmup_models(),
        }
    }
}

impl Default for ScheduledWarmupConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Quota protection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaProtectionConfig {
    /// Whether quota protection is enabled
    pub enabled: bool,

    /// Reserved quota percentage (1-99)
    pub threshold_percentage: u32,

    /// List of monitored models (e.g. gemini-3-flash, gemini-3-pro-high, gemini-3.1-pro-high, claude-sonnet-4-6)
    #[serde(default = "default_monitored_models")]
    pub monitored_models: Vec<String>,
}

fn default_monitored_models() -> Vec<String> {
    vec![
        "claude".to_string(),
        "gemini-3-pro-high".to_string(),
        "gemini-3-flash".to_string(),
        "gemini-3.1-flash-image".to_string(),
    ]
}

impl QuotaProtectionConfig {
    pub fn new() -> Self {
        Self {
            enabled: false,
            threshold_percentage: 10, // Default 10% reserve
            monitored_models: default_monitored_models(),
        }
    }
}

impl Default for QuotaProtectionConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Pinned quota models configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinnedQuotaModelsConfig {
    /// List of pinned models (displayed outside the account list)
    #[serde(default = "default_pinned_models")]
    pub models: Vec<String>,
}

fn default_pinned_models() -> Vec<String> {
    vec![
        "gemini-3-pro-high".to_string(),
        "gemini-3-flash".to_string(),
        "gemini-3.1-flash-image".to_string(),
        "claude-sonnet-4-6-thinking".to_string(),
    ]
}

impl PinnedQuotaModelsConfig {
    pub fn new() -> Self {
        Self {
            models: default_pinned_models(),
        }
    }
}

impl Default for PinnedQuotaModelsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Circuit breaker configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Whether circuit breaker is enabled
    pub enabled: bool,

    /// Unified backoff steps (seconds)
    /// Default: [60, 300, 1800, 7200]
    #[serde(default = "default_backoff_steps")]
    pub backoff_steps: Vec<u64>,

    /// Optional 5h zero-quota lock; exhausted weekly quota always blocks scheduling.
    #[serde(default = "default_lock_on_zero_quota")]
    pub lock_on_zero_quota: bool,
}

fn default_backoff_steps() -> Vec<u64> {
    vec![60, 300, 1800, 7200]
}

fn default_lock_on_zero_quota() -> bool {
    false
}

impl CircuitBreakerConfig {
    pub fn new() -> Self {
        Self {
            enabled: true,
            backoff_steps: default_backoff_steps(),
            lock_on_zero_quota: false,
        }
    }
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl AppConfig {
    pub fn new() -> Self {
        Self {
            language: crate::modules::i18n::default_language(),
            theme: "system".to_string(),
            auto_refresh: true,
            refresh_interval: 15,
            auto_sync: false,
            sync_interval: 5,
            default_export_path: None,
            proxy: ProxyConfig::default(),
            antigravity_executable: None,
            antigravity_ide_executable: None,
            antigravity_cli_executable: None,
            antigravity_args: None,
            auto_launch: false,
            quiet_autostart: true,
            scheduled_warmup: ScheduledWarmupConfig::default(),
            phase_scheduler: PhaseSchedulerConfig::default(),
            quota_protection: QuotaProtectionConfig::default(),
            pinned_quota_models: PinnedQuotaModelsConfig::default(),
            circuit_breaker: CircuitBreakerConfig::default(),
            hidden_menu_items: Vec::new(),
            cloudflared: CloudflaredConfig::default(),
            lightweight_mode: false,
            suggestion_delete_thinking_store: None,
            thinking_cleanup_dismissed: None,
            dismissed_thinking_cleanup_version: None,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;

    #[test]
    fn saved_language_is_preserved_when_loading_config() {
        let mut config = AppConfig::new();
        for language in ["en", "zh", "zh-TW", "ru"] {
            config.language = language.to_string();
            let saved = serde_json::to_string(&config).unwrap();
            let restored: AppConfig = serde_json::from_str(&saved).unwrap();
            assert_eq!(restored.language, language);
        }
    }

    #[test]
    fn quiet_autostart_defaults_to_true_when_missing() {
        let mut config = AppConfig::new();
        config.quiet_autostart = false;
        let mut value = serde_json::to_value(&config).unwrap();
        value.as_object_mut().unwrap().remove("quiet_autostart");
        let restored: AppConfig = serde_json::from_value(value).unwrap();
        assert!(restored.quiet_autostart);

        let saved = serde_json::to_string(&config).unwrap();
        let restored: AppConfig = serde_json::from_str(&saved).unwrap();
        assert!(!restored.quiet_autostart);
    }
}
