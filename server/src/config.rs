//! 配置管理模块
//! 
//! 支持从环境变量和配置文件加载配置

use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 命令行参数
#[derive(Parser, Debug)]
#[command(name = "antigravity-server")]
#[command(about = "Antigravity API Server - 本地 AI 网关")]
pub struct CliArgs {
    /// 服务器监听端口
    #[arg(short, long, env = "ANTIGRAVITY_PORT", default_value = "8045")]
    pub port: u16,

    /// 服务器监听地址
    #[arg(long, env = "ANTIGRAVITY_HOST", default_value = "0.0.0.0")]
    pub host: String,

    /// API Key (用于认证)
    #[arg(long, env = "ANTIGRAVITY_API_KEY", default_value = "sk-antigravity")]
    pub api_key: String,

    /// 账号数据目录
    #[arg(long, env = "ANTIGRAVITY_ACCOUNTS_DIR", default_value = "./data/accounts")]
    pub accounts_dir: PathBuf,

    /// 配置文件目录
    #[arg(long, env = "ANTIGRAVITY_CONFIG_DIR", default_value = "./data/config")]
    pub config_dir: PathBuf,

    /// 上游代理 (可选, 格式: http://host:port 或 socks5://host:port)
    #[arg(long, env = "ANTIGRAVITY_UPSTREAM_PROXY")]
    pub upstream_proxy: Option<String>,

    /// 请求超时时间(秒)
    #[arg(long, env = "ANTIGRAVITY_REQUEST_TIMEOUT", default_value = "120")]
    pub request_timeout: u64,

    /// 静态文件目录 (Web UI)
    #[arg(long, env = "ANTIGRAVITY_STATIC_DIR", default_value = "./static")]
    pub static_dir: PathBuf,
}

/// 服务器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
}

/// 代理配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub api_key: String,
    pub request_timeout: u64,
    pub upstream_proxy: Option<String>,
    #[serde(default)]
    pub model_mapping: std::collections::HashMap<String, String>,
}

/// 完整应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub proxy: ProxyConfig,
    pub accounts_dir: PathBuf,
    pub config_dir: PathBuf,
    pub static_dir: PathBuf,
}

impl AppConfig {
    /// 从命令行参数和环境变量加载配置
    pub fn load() -> anyhow::Result<Self> {
        let args = CliArgs::parse();

        // 确保目录存在
        std::fs::create_dir_all(&args.accounts_dir)?;
        std::fs::create_dir_all(&args.config_dir)?;

        // 尝试从配置文件加载模型映射
        let config_file = args.config_dir.join("config.json");
        let model_mapping = if config_file.exists() {
            let content = std::fs::read_to_string(&config_file)?;
            let file_config: serde_json::Value = serde_json::from_str(&content)?;
            file_config.get("model_mapping")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default()
        } else {
            Self::default_model_mapping()
        };

        Ok(Self {
            server: ServerConfig {
                port: args.port,
                host: args.host,
            },
            proxy: ProxyConfig {
                api_key: args.api_key,
                request_timeout: args.request_timeout,
                upstream_proxy: args.upstream_proxy,
                model_mapping,
            },
            accounts_dir: args.accounts_dir,
            config_dir: args.config_dir,
            static_dir: args.static_dir,
        })
    }

    /// 默认模型映射
    fn default_model_mapping() -> std::collections::HashMap<String, String> {
        let mut mapping = std::collections::HashMap::new();
        
        // Gemini 模型
        mapping.insert("gemini-3-flash".to_string(), "gemini-2.5-flash-preview-05-20".to_string());
        mapping.insert("gemini-3-pro-high".to_string(), "gemini-2.5-pro-exp-03-25".to_string());
        mapping.insert("gemini-3-pro-low".to_string(), "gemini-2.5-pro-preview-05-06".to_string());
        mapping.insert("gemini-2.5-pro".to_string(), "gemini-2.5-pro-preview-05-06".to_string());
        mapping.insert("gemini-2.5-flash".to_string(), "gemini-2.5-flash-preview-05-20".to_string());
        mapping.insert("gemini-2.5-flash-lite".to_string(), "gemini-2.5-flash-lite-preview-06-17".to_string());
        mapping.insert("gemini-2.5-flash-thinking".to_string(), "gemini-2.5-flash-preview-04-17".to_string());
        
        // Claude 模型 (映射到 Gemini)
        mapping.insert("claude-sonnet-4-5".to_string(), "gemini-2.5-pro-preview-05-06".to_string());
        mapping.insert("claude-sonnet-4-5-thinking".to_string(), "gemini-2.5-pro-exp-03-25".to_string());
        mapping.insert("claude-opus-4-5-thinking".to_string(), "gemini-2.5-pro-exp-03-25".to_string());
        
        // 图像模型
        mapping.insert("gemini-3-pro-image".to_string(), "imagen-3.0-generate-002".to_string());
        mapping.insert("gemini-3-pro-image-16x9".to_string(), "imagen-3.0-generate-002".to_string());
        mapping.insert("gemini-3-pro-image-9x16".to_string(), "imagen-3.0-generate-002".to_string());
        mapping.insert("gemini-3-pro-image-4k".to_string(), "imagen-3.0-generate-002".to_string());
        
        mapping
    }

    /// 保存配置到文件
    pub fn save(&self) -> anyhow::Result<()> {
        let config_file = self.config_dir.join("config.json");
        let content = serde_json::to_string_pretty(&serde_json::json!({
            "model_mapping": self.proxy.model_mapping,
        }))?;
        std::fs::write(config_file, content)?;
        Ok(())
    }
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            api_key: "sk-antigravity".to_string(),
            request_timeout: 120,
            upstream_proxy: None,
            model_mapping: std::collections::HashMap::new(),
        }
    }
}
