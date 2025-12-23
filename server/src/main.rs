//! Antigravity API Server - 主入口
//! 
//! 一个本地 AI 网关，支持 OpenAI 和 Anthropic 协议代理

mod config;
mod routes;
mod services;
mod proxy;
mod models;
mod error;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::AppConfig;
use crate::services::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 加载 .env 文件（如果存在）
    dotenvy::dotenv().ok();

    // 初始化日志系统
    init_tracing();

    // 加载配置
    let config = AppConfig::load()?;
    tracing::info!("配置加载完成: {:?}", config);

    // 创建应用状态
    let state = AppState::new(config.clone()).await?;
    let state = Arc::new(state);

    // 构建路由
    let app = build_router(state.clone());

    // 启动服务器
    let addr = SocketAddr::new(
        config.server.host.parse()?,
        config.server.port,
    );
    
    tracing::info!("🚀 Antigravity API Server 启动中...");
    tracing::info!("📍 监听地址: http://{}", addr);
    tracing::info!("📖 API 文档: http://{}/api/docs", addr);
    tracing::info!("🔑 API Key: {}", mask_api_key(&config.proxy.api_key));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// 构建应用路由
fn build_router(state: Arc<AppState>) -> Router {
    // CORS 配置
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // API 代理路由 (OpenAI / Anthropic 协议)
        .merge(routes::proxy::router())
        // 管理 API 路由
        .merge(routes::api::router())
        // 健康检查
        .route("/health", axum::routing::get(routes::health::health_check))
        // 静态文件服务 (SPA)
        .fallback_service(routes::static_files::service())
        // 全局中间件
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        // 注入状态
        .with_state(state)
}

/// 初始化日志系统
fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "antigravity_server=info,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}

/// 掩码 API Key 用于日志输出
fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        "****".to_string()
    } else {
        format!("{}...{}", &key[..4], &key[key.len()-4..])
    }
}
