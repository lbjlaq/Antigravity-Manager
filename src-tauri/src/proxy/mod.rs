// proxy 模块 - API 反代服务

// 现有模块 (保留)
pub mod config;
pub mod token_manager;
pub mod project_resolver;
pub mod server;
pub mod security;

// 新架构模块
pub mod mappers;           // 协议转换器
pub mod handlers;          // API 端点处理器
pub mod middleware;        // Axum 中间件
pub mod upstream;          // 上游客户端
pub mod common;            // 公共工具
pub mod providers;         // Extra upstream providers (z.ai, etc.)
pub mod zai_vision_mcp;    // Built-in Vision MCP server state
pub mod zai_vision_tools;  // Built-in Vision MCP tools (z.ai vision API)
pub mod monitor;           // 监控
pub mod observability;     // request attribution (safe)
pub mod privacy;           // redaction/helpers (safe-by-default)
pub mod rate_limit;        // 限流跟踪
pub mod sticky_config;     // 粘性调度配置
pub mod session_manager;   // 会话指纹管理
pub mod zai_auth;          // z.ai token normalization
pub mod zai_web_tools;     // z.ai web tools helpers (URL normalization)


pub use config::ProxyConfig;
pub use config::ProxyAuthMode;
pub use config::ZaiConfig;
pub use config::ZaiDispatchMode;
pub use token_manager::TokenManager;
pub use server::AxumServer;
pub use security::ProxySecurityConfig;
