use crate::proxy::monitor::{
    DetailedViolationMetrics, ProxyMonitor, ProxyRequestLog, ProxyStats, ViolationRates,
};
use crate::proxy::{ProxyConfig, TokenManager};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;
use tokio::time::Duration;

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
    token_manager
        .update_sticky_config(config.scheduling.clone())
        .await;

    // 3. 加载账号
    let active_accounts = token_manager
        .load_accounts()
        .await
        .map_err(|e| format!("加载账号失败: {}", e))?;

    if active_accounts == 0 {
        let zai_enabled = config.zai.enabled
            && !matches!(config.zai.dispatch_mode, crate::proxy::ZaiDispatchMode::Off);
        if !zai_enabled {
            return Err("没有可用账号，请先添加账号".to_string());
        }
    }

    // Set global AppHandle for event emission from mappers
    crate::proxy::mappers::claude::set_app_handle(app_handle.clone());

    // 启动 Axum 服务器
    let (axum_server, server_handle) = match crate::proxy::AxumServer::start(
        config.get_bind_address().to_string(),
        config.port,
        token_manager.clone(),
        config.custom_mapping.clone(),
        config.request_timeout,
        config.upstream_proxy.clone(),
        crate::proxy::ProxySecurityConfig::from_proxy_config(&config),
        config.zai.clone(),
        monitor.clone(),
        config.experimental.clone(),
    )
    .await
    {
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
    let mut app_config = crate::modules::config::load_app_config()?;
    app_config.proxy = config.clone();
    crate::modules::config::save_app_config(&app_config)?;

    Ok(ProxyStatus {
        running: true,
        port: config.port,
        base_url: format!("http://127.0.0.1:{}", config.port),
        active_accounts,
    })
}

/// 停止反代服务
#[tauri::command]
pub async fn stop_proxy_service(state: State<'_, ProxyServiceState>) -> Result<(), String> {
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
pub async fn get_proxy_status(state: State<'_, ProxyServiceState>) -> Result<ProxyStatus, String> {
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
pub async fn get_proxy_stats(state: State<'_, ProxyServiceState>) -> Result<ProxyStats, String> {
    let monitor_lock = state.monitor.read().await;
    if let Some(monitor) = monitor_lock.as_ref() {
        Ok(monitor.get_stats().await)
    } else {
        Ok(ProxyStats::default())
    }
}

/// 🆕 Story #8 Step 12: 获取详细的 thinking violation metrics
/// AC7: Detailed violation metrics API for frontend
#[tauri::command]
pub async fn get_violation_metrics(
    state: State<'_, ProxyServiceState>,
) -> Result<DetailedViolationMetrics, String> {
    let monitor_lock = state.monitor.read().await;
    if let Some(monitor) = monitor_lock.as_ref() {
        Ok(monitor.get_violation_metrics().await)
    } else {
        // Return empty metrics if monitor not initialized
        Ok(DetailedViolationMetrics {
            stats: ProxyStats::default(),
            position_histogram: vec![],
            rates: ViolationRates {
                budget_violations_per_second: 0.0,
                position_violations_per_second: 0.0,
            },
        })
    }
}

/// Reset violation metrics (Story-003-12)
#[tauri::command]
pub async fn reset_violation_metrics(state: State<'_, ProxyServiceState>) -> Result<(), String> {
    let monitor_lock = state.monitor.read().await;
    if let Some(monitor) = monitor_lock.as_ref() {
        monitor.reset_violation_metrics().await;
        Ok(())
    } else {
        Err("Proxy monitor not initialized".to_string())
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
pub async fn clear_proxy_logs(state: State<'_, ProxyServiceState>) -> Result<(), String> {
    let monitor_lock = state.monitor.read().await;
    if let Some(monitor) = monitor_lock.as_ref() {
        monitor.clear().await;
    }
    Ok(())
}

/// 生成 API Key
#[tauri::command]
pub fn generate_api_key() -> String {
    format!("sk-{}", uuid::Uuid::new_v4().simple())
}

/// 重新加载账号（当主应用添加/删除账号时调用）
#[tauri::command]
pub async fn reload_proxy_accounts(state: State<'_, ProxyServiceState>) -> Result<usize, String> {
    let instance_lock = state.instance.read().await;

    if let Some(instance) = instance_lock.as_ref() {
        // 重新加载账号
        let count = instance
            .token_manager
            .load_accounts()
            .await
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
    let mut app_config = crate::modules::config::load_app_config()?;
    app_config.proxy.custom_mapping = config.custom_mapping;
    crate::modules::config::save_app_config(&app_config)?;

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
            if let Some(serde_json::Value::Array(arr)) = map.get("data") {
                for item in arr {
                    push_from_item(&mut out, item);
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

    let mut builder =
        reqwest::Client::builder().timeout(Duration::from_secs(request_timeout.max(5)));
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
    let text = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    if !status.is_success() {
        let preview = if text.len() > 4000 {
            &text[..4000]
        } else {
            &text
        };
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

// ========== Story-008-02: Cache Metrics API Commands ==========

/// Get comprehensive cache metrics
/// Story-008-02 AC5: Dashboard integration
#[tauri::command]
pub async fn get_cache_metrics() -> Result<crate::proxy::cache_monitor::CacheMetrics, String> {
    let monitor = crate::proxy::signature_cache::SignatureCache::get_monitor();
    let mut metrics = monitor.export_metrics().await;

    // Load top signatures from database if available
    if let Ok(db_signatures) = crate::modules::proxy_db::load_top_signatures(10) {
        if !db_signatures.is_empty() {
            metrics.top_signatures = db_signatures;
        }
    }

    // Save metrics to database (async, non-blocking)
    let metrics_clone = metrics.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::modules::proxy_db::save_cache_metrics(&metrics_clone) {
            tracing::error!("Failed to save cache metrics to DB: {}", e);
        }

        // Save top signatures separately
        if !metrics_clone.top_signatures.is_empty() {
            if let Err(e) =
                crate::modules::proxy_db::save_signature_stats(&metrics_clone.top_signatures)
            {
                tracing::error!("Failed to save signature stats to DB: {}", e);
            }
        }
    });

    Ok(metrics)
}

/// Get current cache hit rate
/// Story-008-02 AC1: Hit rate monitoring
#[tauri::command]
pub async fn get_cache_hit_rate() -> Result<f32, String> {
    let monitor = crate::proxy::signature_cache::SignatureCache::get_monitor();
    Ok(monitor.get_hit_rate().await)
}

/// Get top N most reused signatures
/// Story-008-02 AC2: Signature reuse analysis
#[tauri::command]
pub async fn get_top_cache_signatures(
    limit: Option<usize>,
) -> Result<Vec<crate::proxy::cache_monitor::SignatureStats>, String> {
    let monitor = crate::proxy::signature_cache::SignatureCache::get_monitor();
    let limit = limit.unwrap_or(10);

    // Try to load from database first for historical data
    if let Ok(db_signatures) = crate::modules::proxy_db::load_top_signatures(limit) {
        if !db_signatures.is_empty() {
            return Ok(db_signatures);
        }
    }

    // Fallback to in-memory data
    Ok(monitor.get_top_signatures(limit).await)
}

/// Get cost savings analysis
/// Story-008-02 AC3: Cost attribution
#[tauri::command]
pub async fn get_cache_cost_savings() -> Result<crate::proxy::cache_monitor::CostSavings, String> {
    let monitor = crate::proxy::signature_cache::SignatureCache::get_monitor();
    Ok(monitor.calculate_cost_savings().await)
}

/// Clear cache metrics (for testing or reset)
/// Story-008-02: Metrics management
#[tauri::command]
pub async fn clear_cache_metrics() -> Result<(), String> {
    let monitor = crate::proxy::signature_cache::SignatureCache::get_monitor();
    monitor.clear().await;
    Ok(())
}

// ===== Story-013-06: Cost Analytics Commands =====

/// Get analytics report for thinking level distribution and costs
/// Story-013-06 AC4: Analytics API
#[tauri::command]
pub async fn get_analytics_report(
    period: String,
) -> Result<crate::proxy::analytics::AnalyticsReport, String> {
    let report = crate::proxy::analytics::ANALYTICS
        .generate_report(&period)
        .await;
    Ok(report)
}

/// Get cost breakdown for a specific model
/// Story-013-06 AC4: Cost breakdown API
#[tauri::command]
pub async fn get_cost_breakdown(
    model: String,
) -> Result<crate::proxy::analytics::CostBreakdown, String> {
    let breakdown = crate::proxy::analytics::ANALYTICS
        .get_cost_breakdown(&model)
        .await;
    Ok(breakdown)
}

/// Reset analytics data
/// Story-013-06: Analytics management
#[tauri::command]
pub async fn reset_analytics() -> Result<(), String> {
    crate::proxy::analytics::ANALYTICS.reset().await;
    Ok(())
}
