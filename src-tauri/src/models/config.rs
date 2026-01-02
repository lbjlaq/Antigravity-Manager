use crate::proxy::ProxyConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 时间范围
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: String,
    pub end: String,
}

impl Default for TimeRange {
    fn default() -> Self {
        Self {
            start: "09:00".to_string(),
            end: "12:00".to_string(),
        }
    }
}

/// 定时预热配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledWarmupConfig {
    pub enabled: bool,
    #[serde(default)]
    pub schedules: HashMap<String, Vec<TimeRange>>,
}

impl Default for ScheduledWarmupConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            schedules: HashMap::new(),
        }
    }
}

/// 应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub language: String,
    pub theme: String,
    pub auto_refresh: bool,
    pub refresh_interval: i32, // 分钟
    pub auto_sync: bool,
    pub sync_interval: i32, // 分钟
    pub default_export_path: Option<String>,
    #[serde(default)]
    pub proxy: ProxyConfig,
    pub antigravity_executable: Option<String>, // [NEW] 手动指定的反重力程序路径
    #[serde(default)]
    pub auto_launch: bool, // 开机自动启动
    #[serde(default)]
    pub scheduled_warmup: ScheduledWarmupConfig,
}

impl AppConfig {
    pub fn new() -> Self {
        Self {
            language: "zh".to_string(),
            theme: "system".to_string(),
            auto_refresh: false,
            refresh_interval: 15,
            auto_sync: false,
            sync_interval: 5,
            default_export_path: None,
            proxy: ProxyConfig::default(),
            antigravity_executable: None,
            auto_launch: false,
            scheduled_warmup: ScheduledWarmupConfig::default(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::new()
    }
}
