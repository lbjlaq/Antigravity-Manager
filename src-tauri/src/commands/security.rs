// File: src-tauri/src/commands/security.rs
//! Tauri commands for IP security management.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::modules::security_db::{
    self, AccessLogEntry, IpBlacklistEntry, IpWhitelistEntry, SecurityStats,
};
use crate::proxy::config::SecurityMonitorConfig;

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, serde::Serialize)]
pub struct SecurityError {
    pub message: String,
}

impl From<String> for SecurityError {
    fn from(s: String) -> Self {
        Self { message: s }
    }
}

// Note: Tauri v2 auto-implements From<T: Serialize> for InvokeError,
// so we don't need an explicit impl here.

// ============================================================================
// REQUEST/RESPONSE TYPES
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddToBlacklistRequest {
    pub ip_pattern: String,
    pub reason: String,
    #[serde(default)]
    pub expires_in_seconds: Option<i64>,
    #[serde(default = "default_created_by")]
    pub created_by: String,
}

fn default_created_by() -> String {
    "user".to_string()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddToWhitelistRequest {
    pub ip_pattern: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_created_by")]
    pub created_by: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAccessLogsRequest {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub blocked_only: bool,
    #[serde(default)]
    pub ip_filter: Option<String>,
}

fn default_limit() -> i64 {
    100
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResult {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
}

// ============================================================================
// BLACKLIST COMMANDS
// ============================================================================

/// Initialize security database
#[tauri::command]
pub async fn security_init_db() -> Result<(), SecurityError> {
    security_db::init_db().map_err(SecurityError::from)
}

/// Get all blacklist entries
#[tauri::command]
pub async fn security_get_blacklist() -> Result<Vec<IpBlacklistEntry>, SecurityError> {
    security_db::get_blacklist().map_err(SecurityError::from)
}

/// Add IP to blacklist
#[tauri::command]
pub async fn security_add_to_blacklist(
    request: AddToBlacklistRequest,
) -> Result<OperationResult, SecurityError> {
    let expires_at = request.expires_in_seconds.map(|seconds| {
        chrono::Utc::now().timestamp() + seconds
    });

    let id = security_db::add_to_blacklist(
        &request.ip_pattern,
        &request.reason,
        expires_at,
        &request.created_by,
    )
    .map_err(SecurityError::from)?;

    Ok(OperationResult {
        success: true,
        message: format!("Added {} to blacklist", request.ip_pattern),
        id: Some(id),
    })
}

/// Remove IP from blacklist by pattern
#[tauri::command]
pub async fn security_remove_from_blacklist(ip_pattern: String) -> Result<OperationResult, SecurityError> {
    let removed = security_db::remove_from_blacklist(&ip_pattern).map_err(SecurityError::from)?;

    Ok(OperationResult {
        success: removed,
        message: if removed {
            format!("Removed {} from blacklist", ip_pattern)
        } else {
            format!("{} not found in blacklist", ip_pattern)
        },
        id: None,
    })
}

/// Remove IP from blacklist by ID
#[tauri::command]
pub async fn security_remove_from_blacklist_by_id(id: i64) -> Result<OperationResult, SecurityError> {
    let removed = security_db::remove_from_blacklist_by_id(id).map_err(SecurityError::from)?;

    Ok(OperationResult {
        success: removed,
        message: if removed {
            format!("Removed entry {} from blacklist", id)
        } else {
            format!("Entry {} not found in blacklist", id)
        },
        id: None,
    })
}

/// Check if IP is blacklisted
#[tauri::command]
pub async fn security_is_ip_blacklisted(ip: String) -> Result<bool, SecurityError> {
    security_db::is_ip_in_blacklist(&ip).map_err(SecurityError::from)
}

// ============================================================================
// WHITELIST COMMANDS
// ============================================================================

/// Get all whitelist entries
#[tauri::command]
pub async fn security_get_whitelist() -> Result<Vec<IpWhitelistEntry>, SecurityError> {
    security_db::get_whitelist().map_err(SecurityError::from)
}

/// Add IP to whitelist
#[tauri::command]
pub async fn security_add_to_whitelist(
    request: AddToWhitelistRequest,
) -> Result<OperationResult, SecurityError> {
    let id = security_db::add_to_whitelist(
        &request.ip_pattern,
        &request.description,
        &request.created_by,
    )
    .map_err(SecurityError::from)?;

    Ok(OperationResult {
        success: true,
        message: format!("Added {} to whitelist", request.ip_pattern),
        id: Some(id),
    })
}

/// Remove IP from whitelist by pattern
#[tauri::command]
pub async fn security_remove_from_whitelist(ip_pattern: String) -> Result<OperationResult, SecurityError> {
    let removed = security_db::remove_from_whitelist(&ip_pattern).map_err(SecurityError::from)?;

    Ok(OperationResult {
        success: removed,
        message: if removed {
            format!("Removed {} from whitelist", ip_pattern)
        } else {
            format!("{} not found in whitelist", ip_pattern)
        },
        id: None,
    })
}

/// Remove IP from whitelist by ID
#[tauri::command]
pub async fn security_remove_from_whitelist_by_id(id: i64) -> Result<OperationResult, SecurityError> {
    let removed = security_db::remove_from_whitelist_by_id(id).map_err(SecurityError::from)?;

    Ok(OperationResult {
        success: removed,
        message: if removed {
            format!("Removed entry {} from whitelist", id)
        } else {
            format!("Entry {} not found in whitelist", id)
        },
        id: None,
    })
}

/// Check if IP is whitelisted
#[tauri::command]
pub async fn security_is_ip_whitelisted(ip: String) -> Result<bool, SecurityError> {
    security_db::is_ip_in_whitelist(&ip).map_err(SecurityError::from)
}

// ============================================================================
// ACCESS LOG COMMANDS
// ============================================================================

/// Get access logs
#[tauri::command]
pub async fn security_get_access_logs(
    request: GetAccessLogsRequest,
) -> Result<Vec<AccessLogEntry>, SecurityError> {
    security_db::get_access_logs(
        request.limit,
        request.offset,
        request.blocked_only,
        request.ip_filter.as_deref(),
    )
    .map_err(SecurityError::from)
}

/// Clear old access logs
#[tauri::command]
pub async fn security_cleanup_logs(days: i64) -> Result<OperationResult, SecurityError> {
    let deleted = security_db::cleanup_old_logs(days).map_err(SecurityError::from)?;

    Ok(OperationResult {
        success: true,
        message: format!("Deleted {} old log entries", deleted),
        id: None,
    })
}

/// Clear all access logs
#[tauri::command]
pub async fn security_clear_all_logs() -> Result<OperationResult, SecurityError> {
    let deleted = security_db::clear_all_logs().map_err(SecurityError::from)?;

    Ok(OperationResult {
        success: true,
        message: format!("Cleared {} log entries", deleted),
        id: None,
    })
}

// ============================================================================
// STATISTICS COMMANDS
// ============================================================================

/// Get security statistics
#[tauri::command]
pub async fn security_get_stats() -> Result<SecurityStats, SecurityError> {
    security_db::get_stats().map_err(SecurityError::from)
}

// ============================================================================
// CLEAR COMMANDS (compatibility with Original API)
// ============================================================================

/// Clear all blacklist entries
#[tauri::command]
pub async fn security_clear_blacklist() -> Result<OperationResult, SecurityError> {
    let entries = security_db::get_blacklist().map_err(SecurityError::from)?;
    let mut removed_count = 0;

    for entry in entries {
        if security_db::remove_from_blacklist(&entry.ip_pattern).map_err(SecurityError::from)? {
            removed_count += 1;
        }
    }

    Ok(OperationResult {
        success: true,
        message: format!("Cleared {} blacklist entries", removed_count),
        id: None,
    })
}

/// Clear all whitelist entries
#[tauri::command]
pub async fn security_clear_whitelist() -> Result<OperationResult, SecurityError> {
    let entries = security_db::get_whitelist().map_err(SecurityError::from)?;
    let mut removed_count = 0;

    for entry in entries {
        if security_db::remove_from_whitelist(&entry.ip_pattern).map_err(SecurityError::from)? {
            removed_count += 1;
        }
    }

    Ok(OperationResult {
        success: true,
        message: format!("Cleared {} whitelist entries", removed_count),
        id: None,
    })
}

/// Get IP token consumption statistics
#[tauri::command]
pub async fn security_get_ip_token_stats(
    limit: Option<usize>,
    hours: Option<i64>,
) -> Result<Vec<crate::modules::proxy_db::IpTokenStats>, SecurityError> {
    crate::modules::proxy_db::get_token_usage_by_ip(
        limit.unwrap_or(100),
        hours.unwrap_or(720),
    )
    .map_err(SecurityError::from)
}

// ============================================================================
// CONFIG COMMANDS
// ============================================================================

/// Get security monitor config
#[tauri::command]
pub async fn get_security_config(
    app_state: State<'_, crate::commands::proxy::ProxyServiceState>,
) -> Result<SecurityMonitorConfig, SecurityError> {
    let instance_lock = app_state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        Ok(instance.config.security_monitor.clone())
    } else {
        // Return from saved config
        let app_config = crate::modules::config::load_app_config()
            .map_err(|e| SecurityError::from(format!("Failed to load config: {}", e)))?;
        Ok(app_config.proxy.security_monitor)
    }
}

/// Update security monitor config
#[tauri::command]
pub async fn update_security_config(
    config: SecurityMonitorConfig,
    app_state: State<'_, crate::commands::proxy::ProxyServiceState>,
) -> Result<(), SecurityError> {
    // 1. Save to config file
    let mut app_config = crate::modules::config::load_app_config()
        .map_err(|e| SecurityError::from(format!("Failed to load config: {}", e)))?;
    app_config.proxy.security_monitor = config.clone();
    crate::modules::config::save_app_config(&app_config)
        .map_err(|e| SecurityError::from(format!("Failed to save config: {}", e)))?;

    // 2. Hot-reload if proxy is running
    {
        let mut instance_lock = app_state.instance.write().await;
        if let Some(instance) = instance_lock.as_mut() {
            instance.config.security_monitor = config.clone();
            // Update middleware state via AxumServer method
            instance.axum_server.update_security_monitor(&instance.config).await;
            tracing::info!("[Security] Configuration hot-reloaded");
        }
    }

    Ok(())
}
