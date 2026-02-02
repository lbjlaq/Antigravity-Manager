// File: src-tauri/src/commands/account.rs
//! Account management Tauri commands
//! CRUD operations for user accounts

use crate::error::{AppError, AppResult};
use crate::models::{Account, AccountExportResponse, QuotaData};
use crate::modules;
use tauri::Manager;

// ============================================================================
// Account CRUD Commands
// ============================================================================

/// List all accounts
#[tauri::command]
pub async fn list_accounts() -> AppResult<Vec<Account>> {
    modules::list_accounts()
        .await
        .map_err(AppError::Account)
}

/// Add a new account via refresh token
#[tauri::command]
pub async fn add_account(
    app: tauri::AppHandle,
    _email: String,
    refresh_token: String,
) -> AppResult<Account> {
    let service = modules::account_service::AccountService::new(
        crate::modules::integration::SystemManager::Desktop(app.clone())
    );

    let mut account = service.add_account(&refresh_token)
        .await
        .map_err(AppError::Account)?;

    // Auto-refresh quota after adding
    let _ = internal_refresh_account_quota(&app, &mut account).await;

    // Reload token pool in proxy service
    let _ = crate::commands::proxy::reload_proxy_accounts(
        app.state::<crate::commands::proxy::ProxyServiceState>(),
    ).await;

    Ok(account)
}

/// Delete a single account
#[tauri::command]
pub async fn delete_account(
    app: tauri::AppHandle,
    proxy_state: tauri::State<'_, crate::commands::proxy::ProxyServiceState>,
    account_id: String,
) -> AppResult<()> {
    let service = modules::account_service::AccountService::new(
        crate::modules::integration::SystemManager::Desktop(app.clone())
    );
    service.delete_account(&account_id)
        .map_err(AppError::Account)?;

    // Reload token pool
    let _ = crate::commands::proxy::reload_proxy_accounts(proxy_state).await;

    Ok(())
}

/// Batch delete multiple accounts
#[tauri::command]
pub async fn delete_accounts(
    app: tauri::AppHandle,
    proxy_state: tauri::State<'_, crate::commands::proxy::ProxyServiceState>,
    account_ids: Vec<String>,
) -> AppResult<()> {
    modules::logger::log_info(&format!(
        "Batch delete request received: {} accounts",
        account_ids.len()
    ));
    
    modules::account::delete_accounts(&account_ids)
        .map_err(|e| {
            modules::logger::log_error(&format!("Batch delete failed: {}", e));
            AppError::Account(e)
        })?;

    // Force tray sync
    crate::modules::tray::update_tray_menus(&app);

    // Reload token pool
    let _ = crate::commands::proxy::reload_proxy_accounts(proxy_state).await;

    Ok(())
}

/// Reorder accounts list
#[tauri::command]
pub async fn reorder_accounts(
    app: tauri::AppHandle,
    account_ids: Vec<String>,
) -> AppResult<()> {
    modules::logger::log_info(&format!(
        "Account reorder request: {} accounts", 
        account_ids.len()
    ));
    
    modules::account::reorder_accounts(&account_ids)
        .map_err(|e| {
            modules::logger::log_error(&format!("Account reorder failed: {}", e));
            AppError::Account(e)
        })?;

    // [FIX] Reload proxy accounts after reorder
    let proxy_state = app.state::<crate::commands::proxy::ProxyServiceState>();
    let _ = crate::commands::proxy::reload_proxy_accounts(proxy_state).await;

    Ok(())
}

/// Switch to a different account
#[tauri::command]
pub async fn switch_account(
    app: tauri::AppHandle,
    proxy_state: tauri::State<'_, crate::commands::proxy::ProxyServiceState>,
    account_id: String,
) -> AppResult<()> {
    let service = modules::account_service::AccountService::new(
        crate::modules::integration::SystemManager::Desktop(app.clone())
    );
    
    service.switch_account(&account_id)
        .await
        .map_err(AppError::Account)?;
    
    // Sync tray
    crate::modules::tray::update_tray_menus(&app);

    // Notify proxy to clear stale session bindings and reload accounts
    let _ = crate::commands::proxy::reload_proxy_accounts(proxy_state).await;
    
    Ok(())
}

/// Get the currently active account
#[tauri::command]
pub async fn get_current_account() -> AppResult<Option<Account>> {
    modules::logger::log_info("Backend Command: get_current_account called");

    let account_id = modules::get_current_account_id()
        .map_err(AppError::Account)?;

    if let Some(id) = account_id {
        modules::load_account(&id)
            .map(Some)
            .map_err(AppError::Account)
    } else {
        modules::logger::log_info("   No current account set");
        Ok(None)
    }
}

/// Export accounts with refresh_token (for backup/migration)
#[tauri::command]
pub async fn export_accounts(account_ids: Vec<String>) -> Result<AccountExportResponse, String> {
    modules::account::export_accounts_by_ids(&account_ids)
}

/// Toggle account proxy status (enable/disable for proxy service)
#[tauri::command]
pub async fn toggle_proxy_status(
    app: tauri::AppHandle,
    proxy_state: tauri::State<'_, crate::commands::proxy::ProxyServiceState>,
    account_id: String,
    enable: bool,
    reason: Option<String>,
) -> AppResult<()> {
    modules::logger::log_info(&format!(
        "Toggle account proxy status: {} -> {}",
        account_id,
        if enable { "enabled" } else { "disabled" }
    ));

    // Read account file
    let data_dir = modules::account::get_data_dir()
        .map_err(AppError::Account)?;
    let account_path = data_dir.join("accounts").join(format!("{}.json", account_id));

    if !account_path.exists() {
        return Err(AppError::AccountNotFound(account_id));
    }

    let content = std::fs::read_to_string(&account_path)
        .map_err(|e| AppError::Io(e))?;

    let mut account_json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| AppError::Validation(format!("Failed to parse account file: {}", e)))?;

    // Update proxy_disabled field
    if enable {
        account_json["proxy_disabled"] = serde_json::Value::Bool(false);
        account_json["proxy_disabled_reason"] = serde_json::Value::Null;
        account_json["proxy_disabled_at"] = serde_json::Value::Null;
    } else {
        let now = chrono::Utc::now().timestamp();
        account_json["proxy_disabled"] = serde_json::Value::Bool(true);
        account_json["proxy_disabled_at"] = serde_json::Value::Number(now.into());
        account_json["proxy_disabled_reason"] = serde_json::Value::String(
            reason.unwrap_or_else(|| "Manually disabled by user".to_string())
        );
    }

    // Save to disk
    std::fs::write(&account_path, serde_json::to_string_pretty(&account_json).unwrap())
        .map_err(|e| AppError::Io(e))?;

    modules::logger::log_info(&format!(
        "Account proxy status updated: {} ({})",
        account_id,
        if enable { "enabled" } else { "disabled" }
    ));

    // Reload proxy accounts if service is running
    let _ = crate::commands::proxy::reload_proxy_accounts(proxy_state).await;

    // Update tray menu
    crate::modules::tray::update_tray_menus(&app);

    Ok(())
}

// ============================================================================
// Internal Helper Functions
// ============================================================================

/// Internal helper: Auto-refresh quota after adding or importing account
pub(crate) async fn internal_refresh_account_quota(
    app: &tauri::AppHandle,
    account: &mut Account,
) -> Result<QuotaData, String> {
    modules::logger::log_info(&format!("Auto-refresh quota triggered: {}", account.email));

    // Use shared retry logic
    match modules::account::fetch_quota_with_retry(account).await {
        Ok(quota) => {
            // Update account quota
            let _ = modules::update_account_quota(&account.id, quota.clone());
            // Update tray menu
            crate::modules::tray::update_tray_menus(app);
            Ok(quota)
        }
        Err(e) => {
            modules::logger::log_warn(&format!(
                "Auto-refresh quota failed ({}): {}", 
                account.email, 
                e
            ));
            Err(e.to_string())
        }
    }
}

// ============================================================================
// Re-exports for backward compatibility
// ============================================================================

pub use modules::account::RefreshStats;
