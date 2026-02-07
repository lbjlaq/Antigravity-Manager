// Proxy Logs Commands (Paginated, Filtered, Export)

use crate::proxy::monitor::ProxyRequestLog;

/// Get proxy request logs (paginated)
#[tauri::command]
pub async fn get_proxy_logs_paginated(
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<ProxyRequestLog>, String> {
    crate::modules::proxy_db::get_logs_summary(
        limit.unwrap_or(20),
        offset.unwrap_or(0)
    )
}

/// Get single log detail
#[tauri::command]
pub async fn get_proxy_log_detail(
    log_id: String,
) -> Result<ProxyRequestLog, String> {
    crate::modules::proxy_db::get_log_detail(&log_id)
}

/// Get total log count
#[tauri::command]
pub async fn get_proxy_logs_count() -> Result<u64, String> {
    crate::modules::proxy_db::get_logs_count()
}

/// Export all logs to file
#[tauri::command]
pub async fn export_proxy_logs(
    file_path: String,
) -> Result<usize, String> {
    let logs = crate::modules::proxy_db::get_all_logs_for_export()?;
    let count = logs.len();
    
    let json = serde_json::to_string_pretty(&logs)
        .map_err(|e| format!("Failed to serialize logs: {}", e))?;
    
    std::fs::write(&file_path, json)
        .map_err(|e| format!("Failed to write file: {}", e))?;
    
    Ok(count)
}

/// Export specified logs JSON to file
#[tauri::command]
pub async fn export_proxy_logs_json(
    file_path: String,
    json_data: String,
) -> Result<usize, String> {
    // Parse to count items
    let logs: Vec<serde_json::Value> = serde_json::from_str(&json_data)
        .map_err(|e| format!("Failed to parse JSON: {}", e))?;
    let count = logs.len();
    
    // Pretty print
    let pretty_json = serde_json::to_string_pretty(&logs)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    
    std::fs::write(&file_path, pretty_json)
        .map_err(|e| format!("Failed to write file: {}", e))?;
    
    Ok(count)
}

/// Get log count with filter
#[tauri::command]
pub async fn get_proxy_logs_count_filtered(
    filter: String,
    errors_only: bool,
) -> Result<u64, String> {
    crate::modules::proxy_db::get_logs_count_filtered(&filter, errors_only)
}

/// Get filtered paginated logs
#[tauri::command]
pub async fn get_proxy_logs_filtered(
    filter: String,
    errors_only: bool,
    limit: usize,
    offset: usize,
) -> Result<Vec<ProxyRequestLog>, String> {
    crate::modules::proxy_db::get_logs_filtered(&filter, errors_only, limit, offset)
}
