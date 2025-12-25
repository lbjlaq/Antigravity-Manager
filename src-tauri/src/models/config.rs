use serde::{Deserialize, Serialize};
use crate::proxy::ProxyConfig;

/// 应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub language: String,
    pub theme: String,
    pub auto_refresh: bool,
    pub refresh_interval: i32,  // 分钟
    pub auto_sync: bool,
    pub sync_interval: i32,  // 分钟
    pub default_export_path: Option<String>,
    #[serde(default)]
    pub proxy: ProxyConfig,
    /// Antigravity 可执行文件路径（Linux 上可能需要手动配置）
    #[serde(default)]
    pub antigravity_executable: Option<String>,
}

impl AppConfig {
    pub fn new() -> Self {
        Self {
            language: "zh-CN".to_string(),
            theme: "system".to_string(),
            auto_refresh: false,
            refresh_interval: 15,
            auto_sync: false,
            sync_interval: 5,
            default_export_path: None,
            proxy: ProxyConfig::default(),
            antigravity_executable: None,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::new()
    }
}
