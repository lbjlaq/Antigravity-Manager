use crate::commands::proxy::ProxyServiceState;
use std::collections::HashMap;
use tauri::State;

/// Bind an account to a specific proxy
#[tauri::command]
pub async fn bind_account_proxy(
    state: State<'_, ProxyServiceState>,
    account_id: String,
    proxy_id: String,
) -> Result<(), String> {
    let servers = state.active_axum_servers().await;
    if servers.is_empty() {
        return Err("Service not running".to_string());
    }
    for server in servers {
        server
            .proxy_pool_manager
            .bind_account_to_proxy(account_id.clone(), proxy_id.clone())
            .await?;
    }
    Ok(())
}

/// Unbind an account from its proxy
#[tauri::command]
pub async fn unbind_account_proxy(
    state: State<'_, ProxyServiceState>,
    account_id: String,
) -> Result<(), String> {
    let servers = state.active_axum_servers().await;
    if servers.is_empty() {
        return Err("Service not running".to_string());
    }
    for server in servers {
        server
            .proxy_pool_manager
            .unbind_account_proxy(account_id.clone())
            .await;
    }
    Ok(())
}

/// Get the proxy binding for a specific account
#[tauri::command]
pub async fn get_account_proxy_binding(
    state: State<'_, ProxyServiceState>,
    account_id: String,
) -> Result<Option<String>, String> {
    if let Some(server) = state.primary_axum_server().await {
        Ok(server.proxy_pool_manager.get_account_binding(&account_id))
    } else {
        Err("Service not running".to_string())
    }
}

/// Get all account proxy bindings
#[tauri::command]
pub async fn get_all_account_bindings(
    state: State<'_, ProxyServiceState>,
) -> Result<HashMap<String, String>, String> {
    if let Some(server) = state.primary_axum_server().await {
        // Since get_all_bindings returns a DashMap ref or clone, we need to convert it to HashMap for serialization
        // Assuming we add a method to ProxyPoolManager to get a snapshot
        Ok(server.proxy_pool_manager.get_all_bindings_snapshot())
    } else {
        Err("Service not running".to_string())
    }
}
