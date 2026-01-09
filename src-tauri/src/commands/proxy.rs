use tauri::State;
use crate::proxy::ProxyConfig;
use crate::proxy::monitor::{ProxyRequestLog, ProxyStats};
use crate::services::proxy::{ProxyService, ProxyStatus};
use tokio::time::Duration;
use serde_json::Value;

pub type ProxyServiceState = ProxyService;

#[tauri::command]
pub async fn start_proxy_service(
    config: ProxyConfig,
    state: State<'_, ProxyServiceState>,
    app_handle: tauri::AppHandle,
) -> Result<ProxyStatus, String> {
    state.start(config, Some(app_handle)).await
}

#[tauri::command]
pub async fn stop_proxy_service(
    state: State<'_, ProxyServiceState>,
) -> Result<(), String> {
    state.stop().await
}

#[tauri::command]
pub async fn get_proxy_status(
    state: State<'_, ProxyServiceState>,
) -> Result<ProxyStatus, String> {
    Ok(state.get_status().await)
}

#[tauri::command]
pub async fn get_proxy_stats(
    state: State<'_, ProxyServiceState>,
) -> Result<ProxyStats, String> {
    Ok(state.get_stats().await)
}

#[tauri::command]
pub async fn get_proxy_logs(
    state: State<'_, ProxyServiceState>,
    limit: Option<usize>,
) -> Result<Vec<ProxyRequestLog>, String> {
    Ok(state.get_logs(limit.unwrap_or(100)).await)
}

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

#[tauri::command]
pub async fn clear_proxy_logs(
    state: State<'_, ProxyServiceState>,
) -> Result<(), String> {
    state.clear_logs().await;
    Ok(())
}

#[tauri::command]
pub async fn reload_proxy_accounts(
    state: State<'_, ProxyServiceState>,
) -> Result<usize, String> {
    let instance_lock = state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        let count = instance.token_manager.load_accounts().await
            .map_err(|e| format!("重新加载账号失败: {}", e))?;
        Ok(count)
    } else {
        Err("服务未运行".to_string())
    }
}

#[tauri::command]
pub async fn update_model_mapping(
    config: ProxyConfig,
    state: State<'_, ProxyServiceState>,
) -> Result<(), String> {
    let instance_lock = state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        instance.axum_server.update_mapping(&config).await;
    }
    
    let mut app_config = crate::modules::config::load_app_config().map_err(|e| e)?;
    app_config.proxy.anthropic_mapping = config.anthropic_mapping;
    app_config.proxy.openai_mapping = config.openai_mapping;
    app_config.proxy.custom_mapping = config.custom_mapping;
    app_config.proxy.model_strategies = config.model_strategies;
    crate::modules::config::save_app_config(&app_config).map_err(|e| e)?;
    
    Ok(())
}

#[tauri::command]
pub fn generate_api_key() -> String {
    format!("sk-{}", uuid::Uuid::new_v4().simple())
}

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

// Helpers for z.ai
fn join_base_url(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    };
    format!("{}{}", base, path)
}

fn extract_model_ids(value: &Value) -> Vec<String> {
    let mut out = Vec::new();

    fn push_from_item(out: &mut Vec<String>, item: &Value) {
        match item {
            Value::String(s) => out.push(s.to_string()),
            Value::Object(map) => {
                if let Some(id) = map.get("id").and_then(|v| v.as_str()) {
                    out.push(id.to_string());
                } else if let Some(name) = map.get("name").and_then(|v| v.as_str()) {
                    out.push(name.to_string());
                }
            }
            _ => {}
        }
    }

    match value {
        Value::Array(arr) => {
            for item in arr {
                push_from_item(&mut out, item);
            }
        }
        Value::Object(map) => {
            if let Some(data) = map.get("data") {
                if let Value::Array(arr) = data {
                    for item in arr {
                        push_from_item(&mut out, item);
                    }
                }
            }
            if let Some(models) = map.get("models") {
                match models {
                    Value::Array(arr) => {
                        for item in arr {
                            push_from_item(&mut out, item);
                        }
                    }
                    other => push_from_item(&mut out, other),
                }
            }
        }
        _ => {}
    }

    out
}

#[tauri::command]
pub async fn fetch_zai_models(
    zai: crate::proxy::ZaiConfig,
    upstream_proxy: crate::proxy::config::UpstreamProxyConfig,
    request_timeout: u64,
) -> Result<Vec<String>, String> {
    if zai.base_url.trim().is_empty() {
        return Err("z.ai base_url is empty".to_string());
    }
    if zai.api_key.trim().is_empty() {
        return Err("z.ai api_key is not set".to_string());
    }

    let url = join_base_url(&zai.base_url, "/v1/models");

    let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(request_timeout.max(5)));
    if upstream_proxy.enabled && !upstream_proxy.url.is_empty() {
        let proxy = reqwest::Proxy::all(&upstream_proxy.url)
            .map_err(|e| format!("Invalid upstream proxy url: {}", e))?;
        builder = builder.proxy(proxy);
    }
    let client = builder
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", zai.api_key))
        .header("x-api-key", zai.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Upstream request failed: {}", e))?;

    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("Failed to read response: {}", e))?;

    if !status.is_success() {
        let preview = if text.len() > 4000 { &text[..4000] } else { &text };
        return Err(format!("Upstream returned {}: {}", status, preview));
    }

    let json: Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON response: {}", e))?;
    let mut models = extract_model_ids(&json);
    models.retain(|s| !s.trim().is_empty());
    models.sort();
    models.dedup();
    Ok(models)
}
