// File: src-tauri/src/commands/import.rs
//! Import and migration Tauri commands
//! Handles importing accounts from various sources

use crate::error::{AppError, AppResult};
use crate::models::Account;
use crate::modules;
use super::account::internal_refresh_account_quota;
use tauri::Manager;

// ============================================================================
// Import Commands
// ============================================================================

/// Import accounts from v1 format
#[tauri::command]
pub async fn import_v1_accounts(app: tauri::AppHandle) -> AppResult<Vec<Account>> {
    let accounts = modules::migration::import_from_v1()
        .await
        .map_err(AppError::Account)?;

    // Refresh quota for imported accounts
    for mut account in accounts.clone() {
        let _ = internal_refresh_account_quota(&app, &mut account).await;
    }

    // [FIX] Reload proxy accounts after import
    let proxy_state = app.state::<crate::commands::proxy::ProxyServiceState>();
    let _ = crate::commands::proxy::reload_proxy_accounts(proxy_state).await;

    Ok(accounts)
}

/// Import account from IDE database
#[tauri::command]
pub async fn import_from_db(app: tauri::AppHandle) -> AppResult<Account> {
    let mut account = modules::migration::import_from_db()
        .await
        .map_err(AppError::Account)?;

    // Set as current account since it's from IDE's active session
    let account_id = account.id.clone();
    modules::account::set_current_account_id(&account_id)
        .map_err(AppError::Account)?;

    // Auto-refresh quota
    let _ = internal_refresh_account_quota(&app, &mut account).await;

    // Update tray
    crate::modules::tray::update_tray_menus(&app);

    // [FIX] Reload proxy accounts after import
    let proxy_state = app.state::<crate::commands::proxy::ProxyServiceState>();
    let _ = crate::commands::proxy::reload_proxy_accounts(proxy_state).await;

    Ok(account)
}

/// Import from custom database path
#[tauri::command]
#[allow(dead_code)]
pub async fn import_custom_db(app: tauri::AppHandle, path: String) -> AppResult<Account> {
    let mut account = modules::migration::import_from_custom_db_path(path)
        .await
        .map_err(AppError::Account)?;

    // Set as current account
    let account_id = account.id.clone();
    modules::account::set_current_account_id(&account_id)
        .map_err(AppError::Account)?;

    // Auto-refresh quota
    let _ = internal_refresh_account_quota(&app, &mut account).await;

    // Update tray
    crate::modules::tray::update_tray_menus(&app);

    // [FIX] Reload proxy accounts after import
    let proxy_state = app.state::<crate::commands::proxy::ProxyServiceState>();
    let _ = crate::commands::proxy::reload_proxy_accounts(proxy_state).await;

    Ok(account)
}

/// Sync account from IDE database (periodic check)
#[tauri::command]
pub async fn sync_account_from_db(app: tauri::AppHandle) -> AppResult<Option<Account>> {
    // Get refresh token from DB
    let db_refresh_token = match modules::migration::get_refresh_token_from_db() {
        Ok(token) => token,
        Err(e) => {
            modules::logger::log_info(&format!("Auto-sync skipped: {}", e));
            return Ok(None);
        }
    };

    // Get current Manager account
    let curr_account = modules::account::get_current_account()
        .map_err(AppError::Account)?;

    // Compare: if refresh token matches, no need to import
    if let Some(acc) = curr_account {
        if acc.token.refresh_token == db_refresh_token {
            // Account unchanged, skip to save API quota
            return Ok(None);
        }
        modules::logger::log_info(&format!(
            "Account switch detected ({} -> new DB account), syncing...",
            acc.email
        ));
    } else {
        modules::logger::log_info("New login detected, auto-syncing...");
    }

    // Execute full import
    let account = import_from_db(app).await?;
    Ok(Some(account))
}
