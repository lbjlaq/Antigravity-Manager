use tauri::State;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};
use crate::proxy::{ProxyConfig, TokenManager};
use tokio::time::Duration;
use crate::proxy::monitor::{ProxyMonitor, ProxyRequestLog, ProxyStats};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenLockSnapshot {
    pub account_id: String,
    pub age_secs: u64,
    pub remaining_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolRuntimeSummary {
    pub active_accounts: usize,
    pub lock: Option<TokenLockSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderFlags {
    pub zai_enabled: bool,
    pub zai_dispatch_mode: crate::proxy::ZaiDispatchMode,
    pub zai_mcp_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyAttributionEvent {
    pub id: String,
    pub timestamp: i64,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub duration: u64,
    pub provider: Option<String>,
    pub resolved_model: Option<String>,
    pub account_id: Option<String>,
    pub account_email_masked: Option<String>,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderAggregate {
    pub provider: String,
    pub requests: u64,
    pub errors: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub last_seen: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountAggregate {
    pub provider: String,
    pub account_id: Option<String>,
    pub account_email_masked: Option<String>,
    pub requests: u64,
    pub errors: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub last_seen: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyRuntimeStatus {
    pub running: bool,
    pub port: u16,
    pub base_url: String,
    pub allow_lan_access: bool,
    pub auth_mode: crate::proxy::config::ProxyAuthMode,
    pub effective_auth_mode: crate::proxy::config::ProxyAuthMode,
    pub logging_enabled: bool,
    pub providers: ProviderFlags,
    pub pool: PoolRuntimeSummary,
    pub recent: Vec<ProxyAttributionEvent>,
    pub per_provider: Vec<ProviderAggregate>,
    pub per_account: Vec<AccountAggregate>,
}


/// 反代服务状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyStatus {
    pub running: bool,
    pub port: u16,
    pub base_url: String,
    pub active_accounts: usize,
}

/// 反代服务全局状态
pub struct ProxyServiceState {
    pub instance: Arc<RwLock<Option<ProxyServiceInstance>>>,
    pub monitor: Arc<RwLock<Option<Arc<ProxyMonitor>>>>,
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
        }
    }
}

/// 启动反代服务
#[tauri::command]
pub async fn start_proxy_service(
    config: ProxyConfig,
    state: State<'_, ProxyServiceState>,
    app_handle: tauri::AppHandle,
) -> Result<ProxyStatus, String> {
    let mut instance_lock = state.instance.write().await;
    
    // 防止重复启动
    if instance_lock.is_some() {
        return Err("服务已在运行中".to_string());
    }

    // Ensure monitor exists
    {
        let mut monitor_lock = state.monitor.write().await;
        if monitor_lock.is_none() {
            *monitor_lock = Some(Arc::new(ProxyMonitor::new(1000, Some(app_handle.clone()))));
        }
        // Sync enabled state from config
        if let Some(monitor) = monitor_lock.as_ref() {
            monitor.set_enabled(config.enable_logging);
        }
    }
    
    let monitor = state.monitor.read().await.as_ref().unwrap().clone();
    
    // 2. 初始化 Token 管理器
    let app_data_dir = crate::modules::account::get_data_dir()?;
    // Ensure accounts dir exists even if the user will only use non-Google providers (e.g. z.ai).
    let _ = crate::modules::account::get_accounts_dir()?;
    let accounts_dir = app_data_dir.clone();
    
    let token_manager = Arc::new(TokenManager::new(accounts_dir));
    // 同步 UI 传递的调度配置
    token_manager.update_sticky_config(config.scheduling.clone()).await;
    
    // 3. 加载账号
    let active_accounts = token_manager.load_accounts().await
        .map_err(|e| format!("加载账号失败: {}", e))?;
    
    if active_accounts == 0 {
        let zai_enabled = config.zai.enabled
            && !matches!(config.zai.dispatch_mode, crate::proxy::ZaiDispatchMode::Off);
        if !zai_enabled {
            return Err("没有可用账号，请先添加账号".to_string());
        }
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
            crate::proxy::ProxySecurityConfig::from_proxy_config(&config),
            config.zai.clone(),
            monitor.clone(),
            config.access_log_enabled,
            config.response_attribution_headers,

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

#[tauri::command]
pub async fn get_proxy_runtime_status(
    state: State<'_, ProxyServiceState>,
    limit: Option<usize>,
) -> Result<ProxyRuntimeStatus, String> {
    let instance_lock = state.instance.read().await;

    let (running, port, base_url, allow_lan_access, auth_mode, effective_auth_mode, logging_enabled, providers, pool_active_accounts) =
        if let Some(instance) = instance_lock.as_ref() {
            let cfg = &instance.config;
            let security = crate::proxy::ProxySecurityConfig::from_proxy_config(cfg);
            (
                true,
                cfg.port,
                format!("http://127.0.0.1:{}", cfg.port),
                cfg.allow_lan_access,
                cfg.auth_mode.clone(),
                security.effective_auth_mode(),
                cfg.enable_logging,
                ProviderFlags {
                    zai_enabled: cfg.zai.enabled,
                    zai_dispatch_mode: cfg.zai.dispatch_mode.clone(),
                    zai_mcp_enabled: cfg.zai.mcp.enabled,
                },
                instance.token_manager.len(),
            )
        } else {
            (
                false,
                0,
                String::new(),
                false,
                crate::proxy::config::ProxyAuthMode::Off,
                crate::proxy::config::ProxyAuthMode::Off,
                false,
                ProviderFlags {
                    zai_enabled: false,
                    zai_dispatch_mode: crate::proxy::ZaiDispatchMode::Off,
                    zai_mcp_enabled: false,
                },
                0,
            )
        };

    let limit = limit.unwrap_or(200);

    let recent_logs: Vec<ProxyRequestLog> = {
        let monitor_lock = state.monitor.read().await;
        if let Some(monitor) = monitor_lock.as_ref() {
            monitor.get_logs(limit).await
        } else {
            Vec::new()
        }
    };

    let mut recent = Vec::with_capacity(recent_logs.len());
    let mut per_provider_map: std::collections::HashMap<String, ProviderAggregate> = std::collections::HashMap::new();
    let mut per_account_map: std::collections::HashMap<(String, Option<String>, Option<String>), AccountAggregate> = std::collections::HashMap::new();

    for l in recent_logs {
        let provider = l.provider.clone();
        let provider_key = provider.clone().unwrap_or_else(|| "unknown".to_string());
        let is_error = l.status < 200 || l.status >= 400;
        let input_tokens = l.input_tokens.unwrap_or(0) as u64;
        let output_tokens = l.output_tokens.unwrap_or(0) as u64;

        recent.push(ProxyAttributionEvent {
            id: l.id.clone(),
            timestamp: l.timestamp,
            method: l.method.clone(),
            path: l.url.clone(),
            status: l.status,
            duration: l.duration,
            provider: provider.clone(),
            resolved_model: l.resolved_model.clone().or(l.model.clone()),
            account_id: l.account_id.clone(),
            account_email_masked: l.account_email_masked.clone(),
            input_tokens: l.input_tokens,
            output_tokens: l.output_tokens,
        });

        let p = per_provider_map.entry(provider_key.clone()).or_insert_with(|| ProviderAggregate {
            provider: provider_key.clone(),
            ..ProviderAggregate::default()
        });
        p.requests += 1;
        if is_error {
            p.errors += 1;
        }
        p.input_tokens += input_tokens;
        p.output_tokens += output_tokens;
        p.last_seen = Some(p.last_seen.map(|v| v.max(l.timestamp)).unwrap_or(l.timestamp));

        let account_key = (provider_key.clone(), l.account_id.clone(), l.account_email_masked.clone());
        let a = per_account_map.entry(account_key.clone()).or_insert_with(|| AccountAggregate {
            provider: provider_key.clone(),
            account_id: account_key.1.clone(),
            account_email_masked: account_key.2.clone(),
            ..AccountAggregate::default()
        });
        a.requests += 1;
        if is_error {
            a.errors += 1;
        }
        a.input_tokens += input_tokens;
        a.output_tokens += output_tokens;
        a.last_seen = Some(a.last_seen.map(|v| v.max(l.timestamp)).unwrap_or(l.timestamp));
    }

    let mut per_provider: Vec<ProviderAggregate> = per_provider_map.into_values().collect();
    per_provider.sort_by(|a, b| b.last_seen.unwrap_or(0).cmp(&a.last_seen.unwrap_or(0)));

    let mut per_account: Vec<AccountAggregate> = per_account_map.into_values().collect();
    per_account.sort_by(|a, b| b.last_seen.unwrap_or(0).cmp(&a.last_seen.unwrap_or(0)));

    Ok(ProxyRuntimeStatus {
        running,
        port,
        base_url,
        allow_lan_access,
        auth_mode,
        effective_auth_mode,
        logging_enabled,
        providers,
        pool: PoolRuntimeSummary {
            active_accounts: pool_active_accounts,
            lock: None,
        },
        recent,
        per_provider,
        per_account,
    })
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
        tracing::debug!("后端服务已接收全量模型映射配置");
    }
    
    // 2. 无论是否运行，都保存到全局配置持久化
    let mut app_config = crate::modules::config::load_app_config().map_err(|e| e)?;
    app_config.proxy.anthropic_mapping = config.anthropic_mapping;
    app_config.proxy.openai_mapping = config.openai_mapping;
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
