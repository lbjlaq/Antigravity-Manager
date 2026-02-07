// Proxy Config and Mapping Commands

use tauri::State;
use super::types::ProxyServiceState;
use crate::proxy::ProxyConfig;

/// Generate API Key
#[tauri::command]
pub fn generate_api_key() -> String {
    format!("sk-{}", uuid::Uuid::new_v4().simple())
}

/// Update model mapping (hot update)
#[tauri::command]
pub async fn update_model_mapping(
    config: ProxyConfig,
    state: State<'_, ProxyServiceState>,
) -> Result<(), String> {
    let instance_lock = state.instance.read().await;
    
    // 1. If service is running, update mapping in memory
    if let Some(instance) = instance_lock.as_ref() {
        instance.axum_server.update_mapping(&config).await;
        tracing::debug!("后端服务已接收全量模型映射配置");
    }
    
    // 2. Save to global config persistence
    let mut app_config = crate::modules::config::load_app_config().map_err(|e| e)?;
    app_config.proxy.custom_mapping = config.custom_mapping;
    crate::modules::config::save_app_config(&app_config).map_err(|e| e)?;
    
    Ok(())
}

/// Get current scheduling config
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

/// Update scheduling config
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
