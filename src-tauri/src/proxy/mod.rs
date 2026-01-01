// proxy 模块 - API 反代服务

// 现有模块 (保留)
pub mod config;
pub mod observability;
pub mod project_resolver;
pub mod security;
pub mod server;
pub mod token_manager;

// 新架构模块
pub mod mappers;           // 协议转换器
pub mod handlers;          // API 端点处理器
pub mod middleware;        // Axum 中间件
pub mod upstream;          // 上游客户端
pub mod common;            // 公共工具
pub mod providers;         // Extra upstream providers (z.ai, etc.)
pub mod zai_vision_mcp;    // Built-in Vision MCP server state
pub mod zai_vision_tools;  // Built-in Vision MCP tools (z.ai vision API)
pub mod zai_web_tools;     // Built-in Web Search/Reader MCP tools (z.ai tools API)
pub mod monitor;           // 监控

pub use config::ProxyAuthMode;
pub use config::ProxyConfig;
pub use config::ZaiConfig;
pub use config::ZaiDispatchMode;
pub use security::ProxySecurityConfig;
pub use server::AxumServer;
pub use token_manager::TokenManager;
