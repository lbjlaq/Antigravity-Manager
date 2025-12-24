use serde::{Deserialize, Serialize};

/// 应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub language: String,
    pub theme: String,
    pub auto_refresh: bool,
    pub refresh_interval: i32,
    pub auto_sync: bool,
    pub sync_interval: i32,
    pub default_export_path: Option<String>,
    #[serde(default)]
    pub proxy: ProxyConfig,
}

/// 代理配置 (简化版)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub auto_start: bool,
    #[serde(default)]
    pub anthropic_mapping: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub request_timeout: u64,
    #[serde(default)]
    pub upstream_proxy: UpstreamProxyConfig,
}

/// 上游代理配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpstreamProxyConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub url: String,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 8045,
            api_key: "".to_string(),
            auto_start: false,
            anthropic_mapping: std::collections::HashMap::new(),
            request_timeout: 120,
            upstream_proxy: UpstreamProxyConfig::default(),
        }
    }
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
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::new()
    }
}
