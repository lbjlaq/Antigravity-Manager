// Proxy Service Lifecycle Commands

use tauri::State;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use crate::proxy::{ProxyConfig, TokenManager};
use crate::proxy::monitor::ProxyMonitor;
use super::types::{ProxyStatus, ProxyServiceState, ProxyServiceInstance, AdminServerInstance, StartingGuard};

/// Start proxy service (Tauri command)
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

/// Internal start proxy service logic (decoupled version)
pub async fn internal_start_proxy_service(
    config: ProxyConfig,
    state: &ProxyServiceState,
    integration: crate::modules::integration::SystemManager,
    cloudflared_state: Arc<crate::commands::cloudflared::CloudflaredState>,
) -> Result<ProxyStatus, String> {
    // 1. Check state and lock
    {
        let instance_lock = state.instance.read().await;
        if instance_lock.is_some() {
            return Err("服务已在运行中".to_string());
        }
    }

    // 2. Check if starting (prevent deadlock & concurrent starts)
    if state.starting.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return Err("服务正在启动中，请稍候...".to_string());
    }

    // Use custom Drop guard to ensure starting flag is reset
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
    
    // 2. Ensure Admin Server is running (it holds the real TokenManager)
    ensure_admin_server(config.clone(), state, integration.clone(), cloudflared_state.clone()).await?;

    // Get the existing AxumServer and TokenManager
    let (axum_server, token_manager) = {
        let admin_lock = state.admin_server.read().await;
        // SAFETY: ensure_admin_server called above guarantees this is Some
        let server = admin_lock.as_ref()
            .ok_or_else(|| "Final check: Admin server not initialized".to_string())?
            .axum_server.clone();
        (server.clone(), server.token_manager.clone())
    };
    
    // Update config on the existing TokenManager
    // safe to call (will restart task if running)
    token_manager.start_auto_cleanup().await;
    token_manager.update_sticky_config(config.scheduling.clone()).await;
    
    // Load circuit breaker config from main config
    let app_config = crate::modules::config::load_app_config().unwrap_or_else(|_| crate::models::AppConfig::new());
    token_manager.update_circuit_breaker_config(app_config.circuit_breaker).await;

    // 3. Load accounts (refresh from disk)
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
    
    // Create service instance (logical start)
    let instance = ProxyServiceInstance {
        config: config.clone(),
        token_manager: token_manager.clone(),
        axum_server: axum_server.clone(),
        server_handle: tokio::spawn(async {}),
    };
    
    // Ensure the server is logically running
    axum_server.set_running(true).await;
    
    *instance_lock = Some(instance);
    
    Ok(ProxyStatus {
        running: true,
        port: config.port,
        base_url: format!("http://127.0.0.1:{}", config.port),
        active_accounts,
    })
}

/// Ensure admin server is running
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

    // Default empty TokenManager for admin interface
    let app_data_dir = crate::modules::account::get_data_dir()?;
    let token_manager = Arc::new(TokenManager::new(app_data_dir));
    // Load account data for admin interface stats
    let _ = token_manager.load_accounts().await;

    let (axum_server, server_handle) =
        match crate::proxy::AxumServer::start(
            config.get_bind_address().to_string(),
            config.port,
            token_manager,
            config.custom_mapping.clone(),
            config.request_timeout,
            config.upstream_proxy.clone(),
            config.user_agent_override.clone(),
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

/// Stop proxy service
#[tauri::command]
pub async fn stop_proxy_service(
    state: State<'_, ProxyServiceState>,
) -> Result<(), String> {
    let mut instance_lock = state.instance.write().await;
    
    if instance_lock.is_none() {
        return Err("服务未运行".to_string());
    }
    
    // Stop Axum server (logical stop only, don't kill process)
    if let Some(instance) = instance_lock.take() {
        instance.axum_server.set_running(false).await;
    }
    
    Ok(())
}
