use serde::{Deserialize, Serialize};
// use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyAuthMode {
    Off,
    Strict,
    AllExceptHealth,
    Auto,
}

impl Default for ProxyAuthMode {
    fn default() -> Self {
        Self::Off
    }
}

/// 反代服务配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// 是否启用反代服务
    pub enabled: bool,

    /// 是否允许局域网访问
    /// - false: 仅本机访问 127.0.0.1（默认，隐私优先）
    /// - true: 允许局域网访问 0.0.0.0
    #[serde(default)]
    pub allow_lan_access: bool,

    /// Authorization policy for the proxy.
    /// - off: no auth required
    /// - strict: auth required for all routes
    /// - all_except_health: auth required for all routes except `/healthz`
    /// - auto: recommended defaults (currently: allow_lan_access => all_except_health, else off)
    #[serde(default)]
    pub auth_mode: ProxyAuthMode,
    
    /// 监听端口
    pub port: u16,
    
    /// API 密钥
    pub api_key: String,
    

    /// 是否自动启动
    pub auto_start: bool,

    /// Anthropic 模型映射表 (key: Claude模型名, value: Gemini模型名)
    #[serde(default)]
    pub anthropic_mapping: std::collections::HashMap<String, String>,

    /// OpenAI 模型映射表 (key: OpenAI模型组, value: Gemini模型名)
    #[serde(default)]
    pub openai_mapping: std::collections::HashMap<String, String>,

    /// 自定义精确模型映射表 (key: 原始模型名, value: 目标模型名)
    #[serde(default)]
    pub custom_mapping: std::collections::HashMap<String, String>,

    /// API 请求超时时间(秒)
    #[serde(default = "default_request_timeout")]
    pub request_timeout: u64,

    /// 上游代理配置
    #[serde(default)]
    pub upstream_proxy: UpstreamProxyConfig,
}

/// 上游代理配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpstreamProxyConfig {
    /// 是否启用
    pub enabled: bool,
    /// 代理地址 (http://, https://, socks5://)
    pub url: String,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            allow_lan_access: false, // 默认仅本机访问，隐私优先
            auth_mode: ProxyAuthMode::default(),
            port: 8045,
            api_key: format!("sk-{}", uuid::Uuid::new_v4().simple()),
            auto_start: false,
            anthropic_mapping: std::collections::HashMap::new(),
            openai_mapping: std::collections::HashMap::new(),
            custom_mapping: std::collections::HashMap::new(),
            request_timeout: default_request_timeout(),
            upstream_proxy: UpstreamProxyConfig::default(),
        }
    }
}

fn default_request_timeout() -> u64 {
    120  // 默认 120 秒,原来 60 秒太短
}

impl ProxyConfig {
    /// 获取实际的监听地址
    /// - allow_lan_access = false: 返回 "127.0.0.1"（默认，隐私优先）
    /// - allow_lan_access = true: 返回 "0.0.0.0"（允许局域网访问）
    pub fn get_bind_address(&self) -> &str {
        if self.allow_lan_access {
            "0.0.0.0"
        } else {
            "127.0.0.1"
        }
    }
}
