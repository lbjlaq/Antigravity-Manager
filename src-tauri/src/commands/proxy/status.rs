// Proxy Status and Stats Commands

use tauri::State;
use std::sync::atomic::Ordering;
use crate::proxy::monitor::{ProxyStats, ProxyRequestLog};
use super::types::{ProxyStatus, ProxyServiceState};

/// Get proxy service status
#[tauri::command]
pub async fn get_proxy_status(
    state: State<'_, ProxyServiceState>,
) -> Result<ProxyStatus, String> {
    // Check starting flag first to avoid being blocked by write lock
    if state.starting.load(Ordering::SeqCst) {
        return Ok(ProxyStatus {
            running: false,
            port: 0,
            base_url: "starting".to_string(),
            active_accounts: 0,
        });
    }

    // Use try_read to avoid queuing delay
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
            // If can't get lock, a write operation is in progress
            Ok(ProxyStatus {
                running: false,
                port: 0,
                base_url: "busy".to_string(),
                active_accounts: 0,
            })
        }
    }
}

/// Get proxy service stats
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

/// Get proxy request logs
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

/// Set monitor enabled state
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

/// Clear proxy request logs
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
