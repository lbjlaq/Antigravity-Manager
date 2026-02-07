// File: src-tauri/src/commands/oauth.rs
//! OAuth authentication Tauri commands
//! Handles OAuth login flow and token management

use crate::error::{AppError, AppResult};
use crate::models::Account;
use crate::modules;
use tauri::Manager;
use super::account::internal_refresh_account_quota;

// ============================================================================
// OAuth Flow Commands
// ============================================================================

/// Start OAuth login flow (opens browser)
#[tauri::command]
pub async fn start_oauth_login(app_handle: tauri::AppHandle) -> AppResult<Account> {
    modules::logger::log_info("Starting OAuth authorization flow...");
    
    let service = modules::account_service::AccountService::new(
        crate::modules::integration::SystemManager::Desktop(app_handle.clone())
    );

    let mut account = service.start_oauth_login()
        .await
        .map_err(AppError::OAuth)?;

    // Auto-refresh quota
    let _ = internal_refresh_account_quota(&app_handle, &mut account).await;

    // Reload token pool
    let _ = crate::commands::proxy::reload_proxy_accounts(
        app_handle.state::<crate::commands::proxy::ProxyServiceState>(),
    ).await;

    Ok(account)
}

/// Complete OAuth login (manual mode, no browser)
#[tauri::command]
pub async fn complete_oauth_login(app_handle: tauri::AppHandle) -> AppResult<Account> {
    modules::logger::log_info("Completing OAuth authorization flow (manual)...");
    
    let service = modules::account_service::AccountService::new(
        crate::modules::integration::SystemManager::Desktop(app_handle.clone())
    );

    let mut account = service.complete_oauth_login()
        .await
        .map_err(AppError::OAuth)?;

    // Auto-refresh quota
    let _ = internal_refresh_account_quota(&app_handle, &mut account).await;

    // Reload token pool
    let _ = crate::commands::proxy::reload_proxy_accounts(
        app_handle.state::<crate::commands::proxy::ProxyServiceState>(),
    ).await;

    Ok(account)
}

/// Pre-generate OAuth URL (without opening browser)
#[tauri::command]
pub async fn prepare_oauth_url(app_handle: tauri::AppHandle) -> AppResult<String> {
    let service = modules::account_service::AccountService::new(
        crate::modules::integration::SystemManager::Desktop(app_handle.clone())
    );
    service.prepare_oauth_url()
        .await
        .map_err(AppError::OAuth)
}

/// Cancel ongoing OAuth flow
#[tauri::command]
pub async fn cancel_oauth_login() -> AppResult<()> {
    modules::oauth_server::cancel_oauth_flow();
    Ok(())
}

/// Manually submit OAuth code (for Docker/remote environments)
#[tauri::command]
pub async fn submit_oauth_code(code: String, state: Option<String>) -> AppResult<()> {
    modules::logger::log_info("Manual OAuth code submission received");
    modules::oauth_server::submit_oauth_code(code, state)
        .await
        .map_err(AppError::OAuth)
}
