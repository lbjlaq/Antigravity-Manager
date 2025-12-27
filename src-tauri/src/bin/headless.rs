use antigravity_tools_lib::{
    modules::{config::{load_app_config, save_app_config}, logger::init_logger, account::get_data_dir},
    proxy::{AxumServer, TokenManager},
};
use std::sync::Arc;
use tokio::signal;
use tracing::{info, error};

#[tokio::main]
async fn main() {
    // 初始化日志
    init_logger();
    info!("Starting headless proxy server...");

    // 检查配置文件是否存在
    let config_path = match get_data_dir() {
        Ok(dir) => dir.join("gui_config.json"),
        Err(e) => {
            error!("Failed to get data directory: {}", e);
            std::process::exit(1);
        }
    };

    let config_exists = config_path.exists();

    // 加载应用配置
    let mut config = match load_app_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            error!("Failed to load app configuration: {}", e);
            std::process::exit(1);
        }
    };

    // Headless模式自动启用代理（如果未配置）
    if !config.proxy.enabled {
        info!("Proxy is disabled in config, auto-enabling for headless mode");
        config.proxy.enabled = true;
    }

    // 如果配置文件不存在，保存当前配置（包括生成的 API Key）
    if !config_exists {
        info!("Configuration file not found, creating default config...");
        if let Err(e) = save_app_config(&config) {
            error!("Failed to save initial config: {}", e);
            std::process::exit(1);
        }
        info!("✅ Configuration file created at: {}", config_path.display());
    }

    // 获取数据目录
    let app_data_dir = match antigravity_tools_lib::modules::account::get_data_dir() {
        Ok(dir) => dir,
        Err(e) => {
            error!("Failed to get data directory: {}", e);
            std::process::exit(1);
        }
    };

    // 初始化 TokenManager
    let token_manager = Arc::new(TokenManager::new(app_data_dir.clone()));

    // 加载账号
    let active_accounts = match token_manager.load_accounts().await {
        Ok(count) => count,
        Err(e) => {
            // 账号加载失败可能是首次运行，允许继续启动
            tracing::warn!("Failed to load accounts: {}", e);
            tracing::warn!("Starting without accounts - please add accounts via web interface");
            0
        }
    };

    if active_accounts == 0 {
        tracing::warn!("⚠️  No active accounts found!");
        tracing::warn!("📝 Please add accounts via web interface: http://{}:{}/admin",
            config.proxy.get_bind_address(), config.proxy.port);
        tracing::warn!("🔑 API Key: {}", config.proxy.api_key);
    } else {
        info!("✅ Loaded {} active account(s)", active_accounts);
    }

    // 启动 Axum 服务器
    let bind_address = config.proxy.get_bind_address().to_string();
    let port = config.proxy.port;

    let (axum_server, server_handle) = match AxumServer::start(
        bind_address.clone(),
        port,
        token_manager.clone(),
        config.proxy.anthropic_mapping.clone(),
        config.proxy.openai_mapping.clone(),
        config.proxy.custom_mapping.clone(),
        config.proxy.request_timeout,
        config.proxy.upstream_proxy.clone(),
    ).await {
        Ok((server, handle)) => (server, handle),
        Err(e) => {
            error!("Failed to start Axum server: {}", e);
            std::process::exit(1);
        }
    };

    info!("🚀 Proxy server started successfully on http://{}:{}", bind_address, port);
    info!("");
    info!("📊 Web Management Interface:");
    info!("   URL: http://{}:{}/admin", bind_address, port);
    info!("   API Key: {}", config.proxy.api_key);
    info!("");
    info!("🔌 API Endpoints:");
    info!("   OpenAI:  http://{}:{}/v1/chat/completions", bind_address, port);
    info!("   Claude:  http://{}:{}/v1/messages", bind_address, port);
    info!("   Gemini:  http://{}:{}/v1beta/models", bind_address, port);
    info!("");
    if active_accounts == 0 {
        tracing::warn!("⚠️  Add accounts via web interface to start using the proxy");
    }
    info!("Press Ctrl+C to shutdown...");

    // 等待关闭信号
    shutdown_signal().await;
    info!("Shutdown signal received, stopping server...");

    // 优雅停止服务器
    axum_server.stop();
    let _ = server_handle.await;

    info!("Server stopped gracefully.");
}

/// 等待关闭信号 (Ctrl+C 或 SIGTERM)
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
