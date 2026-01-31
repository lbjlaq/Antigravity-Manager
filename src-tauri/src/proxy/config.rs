use serde::{Deserialize, Serialize};
// use std::path::PathBuf;
use std::collections::HashMap;

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
        Self::Auto
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ZaiDispatchMode {
    /// Never use z.ai.
    Off,
    /// Use z.ai for all Anthropic protocol requests.
    Exclusive,
    /// Treat z.ai as one additional slot in the shared pool.
    Pooled,
    /// Use z.ai only when the Google pool is unavailable.
    Fallback,
}

impl Default for ZaiDispatchMode {
    fn default() -> Self {
        Self::Off
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZaiModelDefaults {
    /// Default model for "opus" family (when the incoming model is a Claude id).
    #[serde(default = "default_zai_opus_model")]
    pub opus: String,
    /// Default model for "sonnet" family (when the incoming model is a Claude id).
    #[serde(default = "default_zai_sonnet_model")]
    pub sonnet: String,
    /// Default model for "haiku" family (when the incoming model is a Claude id).
    #[serde(default = "default_zai_haiku_model")]
    pub haiku: String,
}

impl Default for ZaiModelDefaults {
    fn default() -> Self {
        Self {
            opus: default_zai_opus_model(),
            sonnet: default_zai_sonnet_model(),
            haiku: default_zai_haiku_model(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZaiMcpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub web_search_enabled: bool,
    #[serde(default)]
    pub web_reader_enabled: bool,
    #[serde(default)]
    pub vision_enabled: bool,
}

impl Default for ZaiMcpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            web_search_enabled: false,
            web_reader_enabled: false,
            vision_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZaiConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_zai_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub dispatch_mode: ZaiDispatchMode,
    /// Optional per-model mapping overrides for Anthropic/Claude model ids.
    /// Key: incoming `model` string, Value: upstream z.ai model id (e.g. `glm-4.7`).
    #[serde(default)]
    pub model_mapping: HashMap<String, String>,
    #[serde(default)]
    pub models: ZaiModelDefaults,
    #[serde(default)]
    pub mcp: ZaiMcpConfig,
}

impl Default for ZaiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: default_zai_base_url(),
            api_key: String::new(),
            dispatch_mode: ZaiDispatchMode::Off,
            model_mapping: HashMap::new(),
            models: ZaiModelDefaults::default(),
            mcp: ZaiMcpConfig::default(),
        }
    }
}

/// 实验性功能配置 (Feature Flags)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentalConfig {
    /// 启用双层签名缓存 (Signature Cache)
    #[serde(default = "default_true")]
    pub enable_signature_cache: bool,

    /// 启用工具循环自动恢复 (Tool Loop Recovery)
    #[serde(default = "default_true")]
    pub enable_tool_loop_recovery: bool,

    /// 启用跨模型兼容性检查 (Cross-Model Checks)
    #[serde(default = "default_true")]
    pub enable_cross_model_checks: bool,

    /// 启用上下文用量缩放 (Context Usage Scaling)
    /// 激进模式: 缩放用量并激活自动压缩以突破 200k 限制
    /// 默认关闭以保持透明度,让客户端能触发原生压缩指令
    #[serde(default = "default_false")]
    pub enable_usage_scaling: bool,

    /// 上下文压缩阈值 L1 (Tool Trimming)
    #[serde(default = "default_threshold_l1")]
    pub context_compression_threshold_l1: f32,

    /// 上下文压缩阈值 L2 (Thinking Compression)
    #[serde(default = "default_threshold_l2")]
    pub context_compression_threshold_l2: f32,

    /// 上下文压缩阈值 L3 (Fork + Summary)
    #[serde(default = "default_threshold_l3")]
    pub context_compression_threshold_l3: f32,
}

impl Default for ExperimentalConfig {
    fn default() -> Self {
        Self {
            enable_signature_cache: true,
            enable_tool_loop_recovery: true,
            enable_cross_model_checks: true,
            enable_usage_scaling: false,  // 默认关闭,回归透明模式
            context_compression_threshold_l1: 0.4,
            context_compression_threshold_l2: 0.55,
            context_compression_threshold_l3: 0.7,
        }
    }
}

fn default_threshold_l1() -> f32 { 0.4 }
fn default_threshold_l2() -> f32 { 0.55 }
fn default_threshold_l3() -> f32 { 0.7 }

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

// ============================================================================
// SECURITY MONITOR CONFIG
// ============================================================================

/// IP Blacklist configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistConfig {
    /// Enable IP blacklist filtering
    #[serde(default)]
    pub enabled: bool,
    /// Auto-block after N failed attempts (0 = disabled)
    #[serde(default)]
    pub auto_block_threshold: u32,
    /// Auto-block duration in seconds (default: 1 hour)
    #[serde(default = "default_block_duration")]
    pub auto_block_duration: u64,
}

impl Default for BlacklistConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_block_threshold: 0,
            auto_block_duration: default_block_duration(),
        }
    }
}

fn default_block_duration() -> u64 {
    3600 // 1 hour
}

/// IP Whitelist configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhitelistConfig {
    /// Enable IP whitelist filtering
    #[serde(default)]
    pub enabled: bool,
    /// Whitelist-only mode: only whitelisted IPs can access
    #[serde(default)]
    pub strict_mode: bool,
    /// Whitelist takes priority over blacklist
    #[serde(default = "default_true")]
    pub priority_over_blacklist: bool,
}

impl Default for WhitelistConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strict_mode: false,
            priority_over_blacklist: true,
        }
    }
}

/// Access logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessLogConfig {
    /// Enable access logging
    #[serde(default)]
    pub enabled: bool,
    /// Log retention days (0 = forever)
    #[serde(default = "default_log_retention_days")]
    pub retention_days: u32,
    /// Only log blocked requests
    #[serde(default)]
    pub blocked_only: bool,
}

impl Default for AccessLogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: default_log_retention_days(),
            blocked_only: false,
        }
    }
}

fn default_log_retention_days() -> u32 {
    30
}

/// Security monitor configuration (IP filtering, access logging)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecurityMonitorConfig {
    /// IP blacklist settings
    #[serde(default)]
    pub blacklist: BlacklistConfig,
    /// IP whitelist settings
    #[serde(default)]
    pub whitelist: WhitelistConfig,
    /// Access log settings
    #[serde(default)]
    pub access_log: AccessLogConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugLoggingConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub output_dir: Option<String>,
}

impl Default for DebugLoggingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            output_dir: None,
        }
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
    
    /// Web UI 管理后台密码 (可选，如未设置则使用 api_key)
    pub admin_password: Option<String>,

    /// 是否自动启动
    pub auto_start: bool,

    /// 自定义精确模型映射表 (key: 原始模型名, value: 目标模型名)
    #[serde(default)]
    pub custom_mapping: std::collections::HashMap<String, String>,

    /// API 请求超时时间(秒)
    #[serde(default = "default_request_timeout")]
    pub request_timeout: u64,

    /// 是否开启请求日志记录 (监控)
    #[serde(default)]
    pub enable_logging: bool,

    /// 调试日志配置 (保存完整链路)
    #[serde(default)]
    pub debug_logging: DebugLoggingConfig,

    /// 上游代理配置
    #[serde(default)]
    pub upstream_proxy: UpstreamProxyConfig,

    /// [NEW] 自定义 User-Agent 覆盖 (用于绕过某些检测)
    #[serde(default)]
    pub user_agent_override: Option<String>,

    /// z.ai provider configuration (Anthropic-compatible).
    #[serde(default)]
    pub zai: ZaiConfig,

    /// 账号调度配置 (粘性会话/限流重试)
    #[serde(default)]
    pub scheduling: crate::proxy::sticky_config::StickySessionConfig,

    /// 实验性功能配置
    #[serde(default)]
    pub experimental: ExperimentalConfig,

    /// Security monitor configuration (IP blacklist/whitelist, access logging)
    #[serde(default)]
    pub security_monitor: SecurityMonitorConfig,
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
            admin_password: None,
            auto_start: false,
            custom_mapping: std::collections::HashMap::new(),
            request_timeout: default_request_timeout(),
            enable_logging: true, // 默认开启，支持 token 统计功能
            debug_logging: DebugLoggingConfig::default(),
            upstream_proxy: UpstreamProxyConfig::default(),
            user_agent_override: None,
            zai: ZaiConfig::default(),
            scheduling: crate::proxy::sticky_config::StickySessionConfig::default(),
            experimental: ExperimentalConfig::default(),
            security_monitor: SecurityMonitorConfig::default(),
        }
    }
}

fn default_request_timeout() -> u64 {
    120 // 默认 120 秒,原来 60 秒太短
}

fn default_zai_base_url() -> String {
    "https://api.z.ai/api/anthropic".to_string()
}

fn default_zai_opus_model() -> String {
    "glm-4.7".to_string()
}

fn default_zai_sonnet_model() -> String {
    "glm-4.7".to_string()
}

fn default_zai_haiku_model() -> String {
    "glm-4.5-air".to_string()
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
