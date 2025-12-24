//! Antigravity Web Server - 主入口

use std::sync::Arc;
use std::net::SocketAddr;
use axum::{Router, routing::{get, post, delete}};
use tower_http::cors::{CorsLayer, Any};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// 直接使用 lib crate 的模块
use antigravity_server::state;
use antigravity_server::routes;

#[tokio::main]
async fn main() {
    // 初始化日志
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "antigravity_server=info,tower_http=debug".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // 从环境变量或默认值获取配置
    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    
    // 端口优先级：环境变量 > 配置文件 > 默认值
    let port: u16 = {
        // 1. 先尝试从环境变量读取
        if let Ok(port_str) = std::env::var("PORT") {
            if let Ok(port) = port_str.parse::<u16>() {
                tracing::info!("从环境变量读取端口: {}", port);
                port
            } else {
                // 2. 环境变量无效，尝试从配置文件读取
                if let Ok(config) = antigravity_server::modules::load_app_config() {
                    if config.proxy.port > 0 {
                        tracing::info!("从配置文件读取端口: {}", config.proxy.port);
                        config.proxy.port
                    } else {
                        tracing::info!("使用默认端口: 8045");
                        8045
                    }
                } else {
                    tracing::info!("使用默认端口: 8045");
                    8045
                }
            }
        } else {
            // 3. 环境变量不存在，尝试从配置文件读取
            if let Ok(config) = antigravity_server::modules::load_app_config() {
                if config.proxy.port > 0 {
                    tracing::info!("从配置文件读取端口: {}", config.proxy.port);
                    config.proxy.port
                } else {
                    tracing::info!("使用默认端口: 8045");
                    8045
                }
            } else {
                tracing::info!("使用默认端口: 8045");
                8045
            }
        }
    };
    
    // API Key 优先级：环境变量 > 配置文件 > 默认值
    let api_key = {
        // 1. 先尝试从环境变量读取
        if let Ok(key) = std::env::var("API_KEY") {
            tracing::info!("从环境变量读取 API Key");
            key
        } else {
            // 2. 尝试从配置文件读取
            if let Ok(config) = antigravity_server::modules::load_app_config() {
                if !config.proxy.api_key.is_empty() {
                    tracing::info!("从配置文件读取 API Key");
                    config.proxy.api_key
                } else {
                    tracing::warn!("使用默认 API Key: sk-antigravity");
                    "sk-antigravity".to_string()
                }
            } else {
                tracing::warn!("使用默认 API Key: sk-antigravity");
                "sk-antigravity".to_string()
            }
        }
    };

    // 初始化应用状态
    let app_state = Arc::new(state::AppState::new(api_key.clone()).await);

    // CORS 配置 - 极度宽松模式以允许所有开发环境访问
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // 构建路由
    let app = Router::new()
        // 健康检查
        .route("/health", get(routes::health_check))
        // 账号管理 API
        .route("/api/accounts", get(routes::list_accounts))
        .route("/api/accounts", post(routes::add_account))
        .route("/api/accounts/:id", delete(routes::delete_account))
        .route("/api/accounts/:id/switch", post(routes::switch_account))
        .route("/api/accounts/:id/quota", get(routes::get_account_quota))
        .route("/api/accounts/reload", post(routes::reload_accounts))
        .route("/api/accounts/current", get(routes::get_current_account))
        // 配置 API
        .route("/api/config", get(routes::get_config))
        .route("/api/config", post(routes::save_config))
        // 代理服务 API (TODO)
        .route("/api/proxy/start", post(routes::start_proxy))
        .route("/api/proxy/stop", post(routes::stop_proxy))
        .route("/api/proxy/status", get(routes::get_proxy_status))
        
        // 核心代理路由 (Integrated)
        .route("/v1/chat/completions", post(antigravity_server::modules::proxy::server::chat_completions_handler))
        .route("/v1/messages", post(antigravity_server::modules::proxy::server::anthropic_messages_handler))
        .route("/v1/models", get(antigravity_server::modules::proxy::server::list_models_handler))

        // 状态
        .with_state(app_state)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let addr = format!("{}:{}", host, port).parse::<SocketAddr>().unwrap();
    tracing::info!("🚀 Antigravity Server 启动中...");
    tracing::info!("📡 监听地址: http://{}", addr);
    tracing::info!("🔑 API Key: {}...{}", &api_key[..5], &api_key[api_key.len()-5..]);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
