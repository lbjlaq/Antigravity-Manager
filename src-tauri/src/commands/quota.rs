// File: src-tauri/src/commands/quota.rs
//! Quota management Tauri commands
//! Handles account quota fetching and warmup

use crate::error::{AppError, AppResult};
use crate::models::QuotaData;
use crate::modules;
use tauri::Emitter;

pub use modules::account::RefreshStats;

// ============================================================================
// Quota Commands
// ============================================================================

/// Fetch quota for a single account
#[tauri::command]
pub async fn fetch_account_quota(
    app: tauri::AppHandle,
    proxy_state: tauri::State<'_, crate::commands::proxy::ProxyServiceState>,
    account_id: String,
) -> AppResult<QuotaData> {
    modules::logger::log_info(&format!("Manual quota refresh: {}", account_id));
    
    let mut account = modules::load_account(&account_id)
        .map_err(AppError::Account)?;

    // Use shared retry logic
    let quota = modules::account::fetch_quota_with_retry(&mut account).await?;

    // Update account quota
    modules::update_account_quota(&account_id, quota.clone())
        .map_err(AppError::Account)?;

    crate::modules::tray::update_tray_menus(&app);

    // Sync to running proxy service if started
    let instance_lock = proxy_state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        let _ = instance.token_manager.reload_account(&account_id).await;
    }

    Ok(quota)
}

/// Refresh all accounts' quotas (internal implementation)
pub async fn refresh_all_quotas_internal(
    proxy_state: &crate::commands::proxy::ProxyServiceState,
    app_handle: Option<tauri::AppHandle>,
) -> Result<RefreshStats, String> {
    let stats = modules::account::refresh_all_quotas_logic().await?;

    // Sync to running proxy service
    let instance_lock = proxy_state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        let _ = instance.token_manager.reload_all_accounts().await;
    }

    // Emit global refresh event to UI
    if let Some(handle) = app_handle {
        let _ = handle.emit("accounts://refreshed", ());
    }

    Ok(stats)
}

/// Refresh all accounts' quotas (Tauri Command)
#[tauri::command]
pub async fn refresh_all_quotas(
    proxy_state: tauri::State<'_, crate::commands::proxy::ProxyServiceState>,
    app_handle: tauri::AppHandle,
) -> Result<RefreshStats, String> {
    refresh_all_quotas_internal(&proxy_state, Some(app_handle)).await
}

// ============================================================================
// Warmup Commands
// ============================================================================

/// Warm up all available accounts
#[tauri::command]
pub async fn warm_up_all_accounts() -> AppResult<String> {
    modules::quota::warm_up_all_accounts()
        .await
        .map_err(AppError::Account)
}

/// Warm up a specific account
#[tauri::command]
pub async fn warm_up_account(account_id: String) -> AppResult<String> {
    modules::quota::warm_up_account(&account_id)
        .await
        .map_err(AppError::Account)
}
