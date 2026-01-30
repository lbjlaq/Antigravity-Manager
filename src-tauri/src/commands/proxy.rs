use tauri::State;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use serde::{Serialize, Deserialize};
use crate::proxy::{ProxyConfig, TokenManager};
use tokio::time::Duration;
use crate::proxy::monitor::{ProxyMonitor, ProxyRequestLog, ProxyStats};


/// 反代服务状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyStatus {
    pub running: bool,
    pub port: u16,
    pub base_url: String,
    pub active_accounts: usize,
}

/// 反代服务全局状态
#[derive(Clone)]
pub struct ProxyServiceState {
    pub instance: Arc<RwLock<Option<ProxyServiceInstance>>>,
    pub monitor: Arc<RwLock<Option<Arc<ProxyMonitor>>>>,
    pub admin_server: Arc<RwLock<Option<AdminServerInstance>>>, // [NEW] 常驻管理服务器
    pub starting: Arc<AtomicBool>, // [NEW] 标识是否正在启动中，防止死锁
}

pub struct AdminServerInstance {
    pub axum_server: crate::proxy::AxumServer,
    pub server_handle: tokio::task::JoinHandle<()>,
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
            monitor: Arc::new(RwLock::new(None)),
            admin_server: Arc::new(RwLock::new(None)),
            starting: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// 启动反代服务 (Tauri 命令)
#[tauri::command]
pub async fn start_proxy_service(
    config: ProxyConfig,
    state: State<'_, ProxyServiceState>,
    cf_state: State<'_, crate::commands::cloudflared::CloudflaredState>,
    app_handle: tauri::AppHandle,
) -> Result<ProxyStatus, String> {
    internal_start_proxy_service(
        config,
        &state,
        crate::modules::integration::SystemManager::Desktop(app_handle),
        Arc::new(cf_state.inner().clone()),
    ).await
}

struct StartingGuard(Arc<AtomicBool>);
impl Drop for StartingGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// 内部启动反代服务逻辑 (解耦版本)
pub async fn internal_start_proxy_service(
    config: ProxyConfig,
    state: &ProxyServiceState,
    integration: crate::modules::integration::SystemManager,
    cloudflared_state: Arc<crate::commands::cloudflared::CloudflaredState>,
) -> Result<ProxyStatus, String> {
    // 1. 检查状态并加锁
    {
        let instance_lock = state.instance.read().await;
        if instance_lock.is_some() {
            return Err("服务已在运行中".to_string());
        }
    }

    // 2. 检查是否正在启动中 (防止死锁 & 并发启动)
    if state.starting.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return Err("服务正在启动中，请稍候...".to_string());
    }

    // 使用自定义 Drop guard 确保无论成功失败都会重置 starting 状态
    let _starting_guard = StartingGuard(state.starting.clone());

    // Ensure monitor exists
    {
        let mut monitor_lock = state.monitor.write().await;
        if monitor_lock.is_none() {
            let app_handle = if let crate::modules::integration::SystemManager::Desktop(ref h) = integration {
                Some(h.clone())
            } else {
                None
            };
            *monitor_lock = Some(Arc::new(ProxyMonitor::new(1000, app_handle)));
        }
        // Sync enabled state from config
        if let Some(monitor) = monitor_lock.as_ref() {
            monitor.set_enabled(config.enable_logging);
        }
    }
    
    let _monitor = state.monitor.read().await.as_ref().unwrap().clone();
    
    // 2. 初始化 Token 管理器
    let app_data_dir = crate::modules::account::get_data_dir()?;
    let _ = crate::modules::account::get_accounts_dir()?;
    let accounts_dir = app_data_dir.clone();
    
    let token_manager = Arc::new(TokenManager::new(accounts_dir));
    token_manager.start_auto_cleanup();
    token_manager.update_sticky_config(config.scheduling.clone()).await;
    
    // [NEW] 加载熔断配置 (从主配置加载)
    let app_config = crate::modules::config::load_app_config().unwrap_or_else(|_| crate::models::AppConfig::new());
    token_manager.update_circuit_breaker_config(app_config.circuit_breaker).await;

    // 🆕 [FIX #820] 恢复固定账号模式设置
    if let Some(ref account_id) = config.preferred_account_id {
        token_manager.set_preferred_account(Some(account_id.clone())).await;
        tracing::info!("🔒 [FIX #820] Fixed account mode restored: {}", account_id);
    }

    // 檢查並啟動管理服務器（如果尚未運行）
    ensure_admin_server(config.clone(), state, integration.clone(), cloudflared_state.clone()).await?;

    // 3. 加載賬號
    let active_accounts = token_manager.load_accounts().await
        .unwrap_or(0);
    
    if active_accounts == 0 {
        let zai_enabled = config.zai.enabled
            && !matches!(config.zai.dispatch_mode, crate::proxy::ZaiDispatchMode::Off);
        if !zai_enabled {
            tracing::warn!("沒有可用賬號，反代邏輯將暫停，請通過管理界面添加。");
            return Ok(ProxyStatus {
                running: false,
                port: config.port,
                base_url: format!("http://127.0.0.1:{}", config.port),
                active_accounts: 0,
            });
        }
    }

    let mut instance_lock = state.instance.write().await;
    let admin_lock = state.admin_server.read().await;
    let axum_server = admin_lock.as_ref().unwrap().axum_server.clone();
    
    // 创建服务实例（逻辑启动）
    let instance = ProxyServiceInstance {
        config: config.clone(),
        token_manager: token_manager.clone(),
        axum_server: axum_server.clone(),
        server_handle: tokio::spawn(async {}), // 逻辑上的 handle
    };
    
    // [FIX] Ensure the server is logically running
    axum_server.set_running(true).await;
    
    *instance_lock = Some(instance);
    
    // 成功启动后，guard 在这里结束并重置 starting 是 OK 的
    // 但其实我们可以直接手动掉，或者相信 guard
    Ok(ProxyStatus {
        running: true,
        port: config.port,
        base_url: format!("http://127.0.0.1:{}", config.port),
        active_accounts,
    })
}

/// 确保管理服务器正在运行
pub async fn ensure_admin_server(
    config: ProxyConfig,
    state: &ProxyServiceState,
    integration: crate::modules::integration::SystemManager,
    cloudflared_state: Arc<crate::commands::cloudflared::CloudflaredState>,
) -> Result<(), String> {
    let mut admin_lock = state.admin_server.write().await;
    if admin_lock.is_some() {
        return Ok(());
    }

    // Ensure monitor exists
    let monitor = {
        let mut monitor_lock = state.monitor.write().await;
        if monitor_lock.is_none() {
            let app_handle = if let crate::modules::integration::SystemManager::Desktop(ref h) = integration {
                Some(h.clone())
            } else {
                None
            };
            *monitor_lock = Some(Arc::new(ProxyMonitor::new(1000, app_handle)));
        }
        monitor_lock.as_ref().unwrap().clone()
    };

    // 默认空 TokenManager 用于管理界面
    let app_data_dir = crate::modules::account::get_data_dir()?;
    let token_manager = Arc::new(TokenManager::new(app_data_dir));
    // [NEW] 加载账号数据，否则管理界面统计为 0
    let _ = token_manager.load_accounts().await;

    let (axum_server, server_handle) =
        match crate::proxy::AxumServer::start(
            config.get_bind_address().to_string(),
            config.port,
            token_manager,
            config.custom_mapping.clone(),
            config.request_timeout,
            config.upstream_proxy.clone(),
            crate::proxy::ProxySecurityConfig::from_proxy_config(&config),
            config.zai.clone(),
            monitor,
            config.experimental.clone(),
            config.debug_logging.clone(),
            integration.clone(),
            cloudflared_state,
        ).await {
            Ok((server, handle)) => (server, handle),
            Err(e) => return Err(format!("启动管理服务器失败: {}", e)),
        };

    *admin_lock = Some(AdminServerInstance {
        axum_server,
        server_handle,
    });

    Ok(())
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
    
    // 停止 Axum 服务器 (仅逻辑停止，不杀死进程)
    if let Some(instance) = instance_lock.take() {
        instance.axum_server.set_running(false).await;
        // 已移除 instance.axum_server.stop() 调用，防止杀死 Admin Server
    }
    
    Ok(())
}

/// 获取反代服务状态
#[tauri::command]
pub async fn get_proxy_status(
    state: State<'_, ProxyServiceState>,
) -> Result<ProxyStatus, String> {
    // 优先检查启动标志，避免被写锁阻塞
    if state.starting.load(Ordering::SeqCst) {
        return Ok(ProxyStatus {
            running: false, // 逻辑上还没运行
            port: 0,
            base_url: "starting".to_string(), // 给前端标识
            active_accounts: 0,
        });
    }

    // 使用 try_read 避免在该命令中产生产生排队延迟
    let lock_res = state.instance.try_read();
    
    match lock_res {
        Ok(instance_lock) => {
            match instance_lock.as_ref() {
                Some(instance) => Ok(ProxyStatus {
                    running: true,
                    port: instance.config.port,
                    base_url: format!("http://127.0.0.1:{}", instance.config.port),
                    active_accounts: instance.token_manager.effective_len().await,
                }),
                None => Ok(ProxyStatus {
                    running: false,
                    port: 0,
                    base_url: String::new(),
                    active_accounts: 0,
                }),
            }
        },
        Err(_) => {
            // 如果拿不到锁，说明正在进行写操作（可能是正在启动或停止中）
            Ok(ProxyStatus {
                running: false,
                port: 0,
                base_url: "busy".to_string(),
                active_accounts: 0,
            })
        }
    }
}

/// 获取反代服务统计
#[tauri::command]
pub async fn get_proxy_stats(
    state: State<'_, ProxyServiceState>,
) -> Result<ProxyStats, String> {
    let monitor_lock = state.monitor.read().await;
    if let Some(monitor) = monitor_lock.as_ref() {
        Ok(monitor.get_stats().await)
    } else {
        Ok(ProxyStats::default())
    }
}

/// 获取反代请求日志
#[tauri::command]
pub async fn get_proxy_logs(
    state: State<'_, ProxyServiceState>,
    limit: Option<usize>,
) -> Result<Vec<ProxyRequestLog>, String> {
    let monitor_lock = state.monitor.read().await;
    if let Some(monitor) = monitor_lock.as_ref() {
        Ok(monitor.get_logs(limit.unwrap_or(100)).await)
    } else {
        Ok(Vec::new())
    }
}

/// 设置监控开启状态
#[tauri::command]
pub async fn set_proxy_monitor_enabled(
    state: State<'_, ProxyServiceState>,
    enabled: bool,
) -> Result<(), String> {
    let monitor_lock = state.monitor.read().await;
    if let Some(monitor) = monitor_lock.as_ref() {
        monitor.set_enabled(enabled);
    }
    Ok(())
}

/// 清除反代请求日志
#[tauri::command]
pub async fn clear_proxy_logs(
    state: State<'_, ProxyServiceState>,
) -> Result<(), String> {
    let monitor_lock = state.monitor.read().await;
    if let Some(monitor) = monitor_lock.as_ref() {
        monitor.clear().await;
    }
    Ok(())
}

/// 获取反代请求日志 (分页)
#[tauri::command]
pub async fn get_proxy_logs_paginated(
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<ProxyRequestLog>, String> {
    crate::modules::proxy_db::get_logs_summary(
        limit.unwrap_or(20),
        offset.unwrap_or(0)
    )
}

/// 获取单条日志的完整详情
#[tauri::command]
pub async fn get_proxy_log_detail(
    log_id: String,
) -> Result<ProxyRequestLog, String> {
    crate::modules::proxy_db::get_log_detail(&log_id)
}

/// 获取日志总数
#[tauri::command]
pub async fn get_proxy_logs_count() -> Result<u64, String> {
    crate::modules::proxy_db::get_logs_count()
}

/// 导出所有日志到指定文件
#[tauri::command]
pub async fn export_proxy_logs(
    file_path: String,
) -> Result<usize, String> {
    let logs = crate::modules::proxy_db::get_all_logs_for_export()?;
    let count = logs.len();
    
    let json = serde_json::to_string_pretty(&logs)
        .map_err(|e| format!("Failed to serialize logs: {}", e))?;
    
    std::fs::write(&file_path, json)
        .map_err(|e| format!("Failed to write file: {}", e))?;
    
    Ok(count)
}

/// 导出指定的日志JSON到文件
#[tauri::command]
pub async fn export_proxy_logs_json(
    file_path: String,
    json_data: String,
) -> Result<usize, String> {
    // Parse to count items
    let logs: Vec<serde_json::Value> = serde_json::from_str(&json_data)
        .map_err(|e| format!("Failed to parse JSON: {}", e))?;
    let count = logs.len();
    
    // Pretty print
    let pretty_json = serde_json::to_string_pretty(&logs)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    
    std::fs::write(&file_path, pretty_json)
        .map_err(|e| format!("Failed to write file: {}", e))?;
    
    Ok(count)
}

/// 获取带搜索条件的日志数量
#[tauri::command]
pub async fn get_proxy_logs_count_filtered(
    filter: String,
    errors_only: bool,
) -> Result<u64, String> {
    crate::modules::proxy_db::get_logs_count_filtered(&filter, errors_only)
}

/// 获取带搜索条件的分页日志
#[tauri::command]
pub async fn get_proxy_logs_filtered(
    filter: String,
    errors_only: bool,
    limit: usize,
    offset: usize,
) -> Result<Vec<crate::proxy::monitor::ProxyRequestLog>, String> {
    crate::modules::proxy_db::get_logs_filtered(&filter, errors_only, limit, offset)
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
        // [FIX #820] Clear stale session bindings before reloading accounts
        // This ensures that after switching accounts in the UI, API requests
        // won't be routed to the previously bound (wrong) account
        instance.token_manager.clear_all_sessions();

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
        tracing::debug!("后端服务已接收全量模型映射配置");
    }
    
    // 2. 无论是否运行，都保存到全局配置持久化
    let mut app_config = crate::modules::config::load_app_config().map_err(|e| e)?;
    app_config.proxy.custom_mapping = config.custom_mapping;
    crate::modules::config::save_app_config(&app_config).map_err(|e| e)?;
    
    Ok(())
}

fn join_base_url(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    };
    format!("{}{}", base, path)
}

fn extract_model_ids(value: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();

    fn push_from_item(out: &mut Vec<String>, item: &serde_json::Value) {
        match item {
            serde_json::Value::String(s) => out.push(s.to_string()),
            serde_json::Value::Object(map) => {
                if let Some(id) = map.get("id").and_then(|v| v.as_str()) {
                    out.push(id.to_string());
                } else if let Some(name) = map.get("name").and_then(|v| v.as_str()) {
                    out.push(name.to_string());
                }
            }
            _ => {}
        }
    }

    match value {
        serde_json::Value::Array(arr) => {
            for item in arr {
                push_from_item(&mut out, item);
            }
        }
        serde_json::Value::Object(map) => {
            if let Some(data) = map.get("data") {
                if let serde_json::Value::Array(arr) = data {
                    for item in arr {
                        push_from_item(&mut out, item);
                    }
                }
            }
            if let Some(models) = map.get("models") {
                match models {
                    serde_json::Value::Array(arr) => {
                        for item in arr {
                            push_from_item(&mut out, item);
                        }
                    }
                    other => push_from_item(&mut out, other),
                }
            }
        }
        _ => {}
    }

    out
}

/// Fetch available models from the configured z.ai Anthropic-compatible API (`/v1/models`).
#[tauri::command]
pub async fn fetch_zai_models(
    zai: crate::proxy::ZaiConfig,
    upstream_proxy: crate::proxy::config::UpstreamProxyConfig,
    request_timeout: u64,
) -> Result<Vec<String>, String> {
    if zai.base_url.trim().is_empty() {
        return Err("z.ai base_url is empty".to_string());
    }
    if zai.api_key.trim().is_empty() {
        return Err("z.ai api_key is not set".to_string());
    }

    let url = join_base_url(&zai.base_url, "/v1/models");

    let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(request_timeout.max(5)));
    if upstream_proxy.enabled && !upstream_proxy.url.is_empty() {
        let proxy = reqwest::Proxy::all(&upstream_proxy.url)
            .map_err(|e| format!("Invalid upstream proxy url: {}", e))?;
        builder = builder.proxy(proxy);
    }
    let client = builder
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", zai.api_key))
        .header("x-api-key", zai.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Upstream request failed: {}", e))?;

    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("Failed to read response: {}", e))?;

    if !status.is_success() {
        let preview = if text.len() > 4000 { &text[..4000] } else { &text };
        return Err(format!("Upstream returned {}: {}", status, preview));
    }

    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON response: {}", e))?;
    let mut models = extract_model_ids(&json);
    models.retain(|s| !s.trim().is_empty());
    models.sort();
    models.dedup();
    Ok(models)
}

/// 获取当前调度配置
#[tauri::command]
pub async fn get_proxy_scheduling_config(
    state: State<'_, ProxyServiceState>,
) -> Result<crate::proxy::sticky_config::StickySessionConfig, String> {
    let instance_lock = state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        Ok(instance.token_manager.get_sticky_config().await)
    } else {
        Ok(crate::proxy::sticky_config::StickySessionConfig::default())
    }
}

/// 更新调度配置
#[tauri::command]
pub async fn update_proxy_scheduling_config(
    state: State<'_, ProxyServiceState>,
    config: crate::proxy::sticky_config::StickySessionConfig,
) -> Result<(), String> {
    let instance_lock = state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        instance.token_manager.update_sticky_config(config).await;
        Ok(())
    } else {
        Err("服务未运行，无法更新实时配置".to_string())
    }
}

/// 清除所有会话粘性绑定
#[tauri::command]
pub async fn clear_proxy_session_bindings(
    state: State<'_, ProxyServiceState>,
) -> Result<(), String> {
    let instance_lock = state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        instance.token_manager.clear_all_sessions();
        Ok(())
    } else {
        Err("服务未运行".to_string())
    }
}

// ===== [FIX #820] 固定账号模式命令 =====

/// 设置优先使用的账号（固定账号模式）
/// 传入 account_id 启用固定模式，传入 null/空字符串恢复轮询模式
#[tauri::command]
pub async fn set_preferred_account(
    state: State<'_, ProxyServiceState>,
    account_id: Option<String>,
) -> Result<(), String> {
    let instance_lock = state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        // 过滤空字符串为 None
        let cleaned_id = account_id.filter(|s| !s.trim().is_empty());

        // 1. 更新内存状态
        instance.token_manager.set_preferred_account(cleaned_id.clone()).await;

        // 2. 持久化到配置文件 (修复 Issue #820 自动关闭问题)
        let mut app_config = crate::modules::config::load_app_config()
            .map_err(|e| format!("加载配置失败: {}", e))?;
        app_config.proxy.preferred_account_id = cleaned_id.clone();
        crate::modules::config::save_app_config(&app_config)
            .map_err(|e| format!("保存配置失败: {}", e))?;

        if let Some(ref id) = cleaned_id {
            tracing::info!("🔒 [FIX #820] Fixed account mode enabled and persisted: {}", id);
        } else {
            tracing::info!("🔄 [FIX #820] Round-robin mode enabled and persisted");
        }

        Ok(())
    } else {
        Err("服务未运行".to_string())
    }
}

/// 获取当前优先使用的账号ID
#[tauri::command]
pub async fn get_preferred_account(
    state: State<'_, ProxyServiceState>,
) -> Result<Option<String>, String> {
    let instance_lock = state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        Ok(instance.token_manager.get_preferred_account().await)
    } else {
        Ok(None)
    }
}

/// 清除指定账号的限流记录
#[tauri::command]
pub async fn clear_proxy_rate_limit(
    state: State<'_, ProxyServiceState>,
    account_id: String,
) -> Result<bool, String> {
    let instance_lock = state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        Ok(instance.token_manager.clear_rate_limit(&account_id))
    } else {
        Err("服务未运行".to_string())
    }
}

/// 清除所有限流记录
#[tauri::command]
pub async fn clear_all_proxy_rate_limits(
    state: State<'_, ProxyServiceState>,
) -> Result<(), String> {
    let instance_lock = state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        instance.token_manager.clear_all_rate_limits();
        Ok(())
    } else {
        Err("服务未运行".to_string())
    }
}
