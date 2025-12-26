use tauri::State;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};
use crate::proxy::{ProxyConfig, TokenManager};

/// 反代服务状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyStatus {
    pub running: bool,
    pub port: u16,
    pub base_url: String,
    pub active_accounts: usize,
}

/// 反代服务统计
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProxyStats {
    pub total_requests: u64,
    pub success_count: u64,
    pub error_count: u64,
}

/// 反代服务全局状态
pub struct ProxyServiceState {
    pub instance: Arc<RwLock<Option<ProxyServiceInstance>>>,
}

/// 反代服务实例
pub struct ProxyServiceInstance {
    pub config: ProxyConfig,
    pub token_manager: Arc<TokenManager>,
    pub axum_server: crate::proxy::AxumServer,
    pub server_handle: tokio::task::JoinHandle<()>,
}

impl ProxyServiceState {
    pub fn new() -> Self {
        Self {
            instance: Arc::new(RwLock::new(None)),
        }
    }
}

/// 启动反代服务
#[tauri::command]
pub async fn start_proxy_service(
    config: ProxyConfig,
    state: State<'_, ProxyServiceState>,
    _app_handle: tauri::AppHandle,
) -> Result<ProxyStatus, String> {
    let mut instance_lock = state.instance.write().await;
    
    // 防止重复启动
    if instance_lock.is_some() {
        return Err("服务已在运行中".to_string());
    }
    
    // 2. 初始化 Token 管理器
    let app_data_dir = crate::modules::account::get_data_dir()?;
    let accounts_dir = app_data_dir.clone();
    
    let token_manager = Arc::new(TokenManager::new(accounts_dir));
    
    // 3. 加载账号
    let active_accounts = token_manager.load_accounts().await
        .map_err(|e| format!("加载账号失败: {}", e))?;
    
    if active_accounts == 0 {
        return Err("没有可用账号，请先添加账号".to_string());
    }
    
    // 启动 Axum 服务器
    let (axum_server, server_handle) =
        match crate::proxy::AxumServer::start(
            config.get_bind_address().to_string(),
            config.port,
            token_manager.clone(),
            config.anthropic_mapping.clone(),
            config.openai_mapping.clone(),
            config.custom_mapping.clone(),
            config.request_timeout,
            config.upstream_proxy.clone(),
        ).await {
            Ok((server, handle)) => (server, handle),
            Err(e) => return Err(format!("启动 Axum 服务器失败: {}", e)),
        };
    
    // 创建服务实例
    let instance = ProxyServiceInstance {
        config: config.clone(),
        token_manager: token_manager.clone(), // Clone for ProxyServiceInstance
        axum_server,
        server_handle,
    };
    
    *instance_lock = Some(instance);
    

    // 保存配置到全局 AppConfig
    let mut app_config = crate::modules::config::load_app_config().map_err(|e| e)?;
    app_config.proxy = config.clone();
    crate::modules::config::save_app_config(&app_config).map_err(|e| e)?;
    
    Ok(ProxyStatus {
        running: true,
        port: config.port,
        base_url: format!("http://127.0.0.1:{}", config.port),
        active_accounts,
    })
}

/// 停止反代服务
#[tauri::command]
pub async fn stop_proxy_service(
    state: State<'_, ProxyServiceState>,
) -> Result<(), String> {
    let mut instance_lock = state.instance.write().await;
    
    if instance_lock.is_none() {
        return Err("服务未运行".to_string());
    }
    
    // 停止 Axum 服务器
    if let Some(instance) = instance_lock.take() {
        instance.axum_server.stop();
        // 等待服务器任务完成
        instance.server_handle.await.ok();
    }
    
    Ok(())
}

/// 获取反代服务状态
#[tauri::command]
pub async fn get_proxy_status(
    state: State<'_, ProxyServiceState>,
) -> Result<ProxyStatus, String> {
    let instance_lock = state.instance.read().await;
    
    match instance_lock.as_ref() {
        Some(instance) => Ok(ProxyStatus {
            running: true,
            port: instance.config.port,
            base_url: format!("http://127.0.0.1:{}", instance.config.port),
            active_accounts: instance.token_manager.len(),
        }),
        None => Ok(ProxyStatus {
            running: false,
            port: 0,
            base_url: String::new(),
            active_accounts: 0,
        }),
    }
}

/// 获取反代服务统计
#[tauri::command]
pub async fn get_proxy_stats(
    _state: State<'_, ProxyServiceState>,
) -> Result<ProxyStats, String> {
    // TODO: 实现统计收集
    Ok(ProxyStats::default())
}

/// 生成 API Key
#[tauri::command]
pub fn generate_api_key() -> String {
    format!("sk-{}", uuid::Uuid::new_v4().simple())
}

/// 重新加载账号（当主应用添加/删除账号时调用）
#[tauri::command]
pub async fn reload_proxy_accounts(
    state: State<'_, ProxyServiceState>,
) -> Result<usize, String> {
    let instance_lock = state.instance.read().await;
    
    if let Some(instance) = instance_lock.as_ref() {
        // 重新加载账号
        let count = instance.token_manager.load_accounts().await
            .map_err(|e| format!("重新加载账号失败: {}", e))?;
        Ok(count)
    } else {
        Err("服务未运行".to_string())
    }
}

/// 更新模型映射表 (热更新)
#[tauri::command]
pub async fn update_model_mapping(
    config: ProxyConfig,
    state: State<'_, ProxyServiceState>,
) -> Result<(), String> {
    let instance_lock = state.instance.read().await;
    
    // 1. 如果服务正在运行，立即更新内存中的映射 (这里目前只更新了 anthropic_mapping 的 RwLock, 
    // 后续可以根据需要让 resolve_model_route 直接读取全量 config)
    if let Some(instance) = instance_lock.as_ref() {
        instance.axum_server.update_mapping(&config).await;
        tracing::info!("后端服务已接收全量模型映射配置");
    }
    
    // 2. 无论是否运行，都保存到全局配置持久化
    let mut app_config = crate::modules::config::load_app_config().map_err(|e| e)?;
    app_config.proxy.anthropic_mapping = config.anthropic_mapping;
    app_config.proxy.openai_mapping = config.openai_mapping;
    app_config.proxy.custom_mapping = config.custom_mapping;
    crate::modules::config::save_app_config(&app_config).map_err(|e| e)?;
    
    Ok(())
}
