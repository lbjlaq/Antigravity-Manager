// Proxy Account Operations Commands

use tauri::State;
use super::types::ProxyServiceState;

/// Reload accounts (called when main app adds/deletes accounts)
#[tauri::command]
pub async fn reload_proxy_accounts(
    state: State<'_, ProxyServiceState>,
) -> Result<usize, String> {
    let instance_lock = state.instance.read().await;

    if let Some(instance) = instance_lock.as_ref() {
        // [FIX #820] Clear stale session bindings before reloading accounts
        instance.token_manager.clear_all_sessions();

        // Reload accounts
        let count = instance.token_manager.load_accounts().await
            .map_err(|e| format!("重新加载账号失败: {}", e))?;
        Ok(count)
    } else {
        Err("服务未运行".to_string())
    }
}

/// Clear all session sticky bindings
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

// ===== [FIX #820] Fixed Account Mode Commands =====

/// Set preferred account (fixed account mode)
/// Pass account_id to enable fixed mode, pass null/empty to restore round-robin
#[tauri::command]
pub async fn set_preferred_account(
    state: State<'_, ProxyServiceState>,
    account_id: Option<String>,
) -> Result<(), String> {
    let instance_lock = state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        // Filter empty strings to None
        let cleaned_id = account_id.filter(|s| !s.trim().is_empty());

        // 1. Update memory state
        instance.token_manager.set_preferred_account(cleaned_id.clone()).await;

        // 2. Persist to config file (fix Issue #820 auto-close problem)
        let mut app_config = crate::modules::config::load_app_config()
            .map_err(|e| format!("Failed to load config: {}", e))?;
        app_config.proxy.preferred_account_id = cleaned_id.clone();
        crate::modules::config::save_app_config(&app_config)
            .map_err(|e| format!("Failed to save config: {}", e))?;

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

/// Get current preferred account ID
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

/// Clear rate limit for specific account
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

/// Clear all rate limit records
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
