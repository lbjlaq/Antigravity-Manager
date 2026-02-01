// File: src-tauri/src/commands/config.rs
//! Configuration management Tauri commands
//! Load/save application and proxy configuration

use crate::error::{AppError, AppResult};
use crate::models::AppConfig;
use crate::modules;
use tauri::Emitter;

// ============================================================================
// Config Commands
// ============================================================================

/// Load application configuration
#[tauri::command]
pub async fn load_config() -> AppResult<AppConfig> {
    modules::load_app_config()
        .map_err(AppError::Config)
}

/// Save application configuration with hot-reload
#[tauri::command]
pub async fn save_config(
    app: tauri::AppHandle,
    proxy_state: tauri::State<'_, crate::commands::proxy::ProxyServiceState>,
    config: AppConfig,
) -> AppResult<()> {
    modules::save_app_config(&config)
        .map_err(AppError::Config)?;

    // Notify tray that config was updated
    let _ = app.emit("config://updated", ());

    // Hot-reload running service
    let instance_lock = proxy_state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        // Update model mapping
        instance.axum_server.update_mapping(&config.proxy).await;
        // Update upstream proxy
        instance
            .axum_server
            .update_proxy(config.proxy.upstream_proxy.clone())
            .await;
        // Update security (auth)
        instance.axum_server.update_security(&config.proxy).await;
        // Update z.ai config
        instance.axum_server.update_zai(&config.proxy).await;
        // Update experimental config
        instance.axum_server.update_experimental(&config.proxy).await;
        // Update debug logging config
        instance.axum_server.update_debug_logging(&config.proxy).await;
        // Update circuit breaker config
        instance.token_manager.update_circuit_breaker_config(config.circuit_breaker.clone()).await;
        // Update sticky scheduling config
        instance.token_manager.update_sticky_config(config.proxy.scheduling.clone()).await;
        
        tracing::debug!("Hot-reloaded proxy service configuration");
    }

    Ok(())
}

// ============================================================================
// HTTP API Settings Commands
// ============================================================================

/// Get HTTP API settings
#[tauri::command]
pub async fn get_http_api_settings() -> AppResult<crate::modules::http_api::HttpApiSettings> {
    crate::modules::http_api::load_settings()
        .map_err(AppError::Config)
}

/// Save HTTP API settings
#[tauri::command]
pub async fn save_http_api_settings(
    settings: crate::modules::http_api::HttpApiSettings,
) -> AppResult<()> {
    crate::modules::http_api::save_settings(&settings)
        .map_err(AppError::Config)
}
