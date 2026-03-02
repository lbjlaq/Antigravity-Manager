use crate::modules::security_db;
use serde::{Deserialize, Serialize};
use tauri::State;

// ==================== 请求/响应结构 ====================

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpAccessLogQuery {
    pub page: usize,
    pub page_size: usize,
    pub search: Option<String>,
    pub blocked_only: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IpAccessLogResponse {
    pub logs: Vec<security_db::IpAccessLog>,
    pub total: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddBlacklistRequest {
    pub ip_pattern: String,
    pub reason: Option<String>,
    pub expires_at: Option<i64>, // Unix timestamp
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddWhitelistRequest {
    pub ip_pattern: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IpStatsResponse {
    pub total_requests: usize,
    pub unique_ips: usize,
    pub blocked_requests: usize,
    pub top_ips: Vec<security_db::IpRanking>,
}

// ==================== IP 访问日志命令 ====================

/// 获取 IP 访问日志列表
#[tauri::command]
pub async fn get_ip_access_logs(query: IpAccessLogQuery) -> Result<IpAccessLogResponse, String> {
    let offset = (query.page.max(1) - 1) * query.page_size;

    let logs = security_db::get_ip_access_logs(
        query.page_size,
        offset,
        query.search.as_deref(),
        query.blocked_only,
    )?;

    // 简单计算总数 (如果需要精确分页,可以添加 count 函数)
    let total = logs.len();

    Ok(IpAccessLogResponse { logs, total })
}

/// 获取 IP 统计信息
#[tauri::command]
pub async fn get_ip_stats() -> Result<IpStatsResponse, String> {
    let stats = security_db::get_ip_stats()?;
    let top_ips = security_db::get_top_ips(10, 24)?; // Top 10 IPs in last 24 hours

    Ok(IpStatsResponse {
        total_requests: stats.total_requests as usize,
        unique_ips: stats.unique_ips as usize,
        blocked_requests: stats.blocked_count as usize,
        top_ips,
    })
}

/// 清空 IP 访问日志
#[tauri::command]
pub async fn clear_ip_access_logs() -> Result<(), String> {
    security_db::clear_ip_access_logs()
}

// ==================== IP 黑名单命令 ====================

/// 获取 IP 黑名单列表
#[tauri::command]
pub async fn get_ip_blacklist() -> Result<Vec<security_db::IpBlacklistEntry>, String> {
    security_db::get_blacklist()
}

/// 添加 IP 到黑名单
#[tauri::command]
pub async fn add_ip_to_blacklist(request: AddBlacklistRequest) -> Result<(), String> {
    // 验证 IP 格式
    if !is_valid_ip_pattern(&request.ip_pattern) {
        return Err(
            "Invalid IP pattern. Use IP address or CIDR notation (e.g., 192.168.1.0/24)"
                .to_string(),
        );
    }

    security_db::add_to_blacklist(
        &request.ip_pattern,
        request.reason.as_deref(),
        request.expires_at,
        "manual",
    )?;
    Ok(())
}

/// 从黑名单移除 IP
#[tauri::command]
pub async fn remove_ip_from_blacklist(ip_pattern: String) -> Result<(), String> {
    // 先获取黑名单列表，找到对应的id
    let entries = security_db::get_blacklist()?;
    let entry = entries.iter().find(|e| e.ip_pattern == ip_pattern);

    if let Some(entry) = entry {
        security_db::remove_from_blacklist(&entry.id)
    } else {
        Err(format!("IP pattern {} not found in blacklist", ip_pattern))
    }
}

/// 清空黑名单
#[tauri::command]
pub async fn clear_ip_blacklist() -> Result<(), String> {
    // 获取所有黑名单条目并逐个删除
    let entries = security_db::get_blacklist()?;
    for entry in entries {
        security_db::remove_from_blacklist(&entry.ip_pattern)?;
    }
    Ok(())
}

/// 检查 IP 是否在黑名单中
#[tauri::command]
pub async fn check_ip_in_blacklist(ip: String) -> Result<bool, String> {
    security_db::is_ip_in_blacklist(&ip)
}

// ==================== IP 白名单命令 ====================

/// 获取 IP 白名单列表
#[tauri::command]
pub async fn get_ip_whitelist() -> Result<Vec<security_db::IpWhitelistEntry>, String> {
    security_db::get_whitelist()
}

/// 添加 IP 到白名单
#[tauri::command]
pub async fn add_ip_to_whitelist(request: AddWhitelistRequest) -> Result<(), String> {
    // 验证 IP 格式
    if !is_valid_ip_pattern(&request.ip_pattern) {
        return Err(
            "Invalid IP pattern. Use IP address or CIDR notation (e.g., 192.168.1.0/24)"
                .to_string(),
        );
    }

    security_db::add_to_whitelist(&request.ip_pattern, request.description.as_deref())?;
    Ok(())
}

/// 从白名单移除 IP
#[tauri::command]
pub async fn remove_ip_from_whitelist(ip_pattern: String) -> Result<(), String> {
    // 先获取白名单列表，找到对应的id
    let entries = security_db::get_whitelist()?;
    let entry = entries.iter().find(|e| e.ip_pattern == ip_pattern);

    if let Some(entry) = entry {
        security_db::remove_from_whitelist(&entry.id)
    } else {
        Err(format!("IP pattern {} not found in whitelist", ip_pattern))
    }
}

/// 清空白名单
#[tauri::command]
pub async fn clear_ip_whitelist() -> Result<(), String> {
    // 获取所有白名单条目并逐个删除
    let entries = security_db::get_whitelist()?;
    for entry in entries {
        security_db::remove_from_whitelist(&entry.ip_pattern)?;
    }
    Ok(())
}

/// 检查 IP 是否在白名单中
#[tauri::command]
pub async fn check_ip_in_whitelist(ip: String) -> Result<bool, String> {
    security_db::is_ip_in_whitelist(&ip)
}

// ==================== 安全配置命令 ====================

/// 获取安全监控配置
#[tauri::command]
pub async fn get_security_config(
    app_state: State<'_, crate::commands::proxy::ProxyServiceState>,
) -> Result<crate::proxy::config::SecurityMonitorConfig, String> {
    // 1. 尝试从运行中的实例获取 (内存中可能由最新的配置)
    let instance_lock = app_state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        return Ok(instance.config.security_monitor.clone());
    }

    // 2. 如果服务未运行，从磁盘加载
    let app_config = crate::modules::config::load_app_config()
        .map_err(|e| format!("Failed to load config: {}", e))?;
    Ok(app_config.proxy.security_monitor)
}

/// 更新安全监控配置
#[tauri::command]
pub async fn update_security_config(
    config: crate::proxy::config::SecurityMonitorConfig,
    app_state: State<'_, crate::commands::proxy::ProxyServiceState>,
) -> Result<(), String> {
    // 1. 同步保存到配置文件
    let mut app_config = crate::modules::config::load_app_config()
        .map_err(|e| format!("Failed to load config: {}", e))?;
    app_config.proxy.security_monitor = config.clone();
    crate::modules::config::save_app_config(&app_config)
        .map_err(|e| format!("Failed to save config: {}", e))?;

    // 2. 更新内存中的配置 (如果服务正在运行)
    {
        let mut instance_lock = app_state.instance.write().await;
        if let Some(instance) = instance_lock.as_mut() {
            instance.config.security_monitor = config.clone();
            // [FIX] 调用 update_security 热更新运行中的中间件配置
            // 这是关键步骤！中间件读取的是 AppState.security (Arc<RwLock<ProxySecurityConfig>>)
            // 必须调用 update_security() 才能使黑白名单配置实时生效
            instance.axum_server.update_security(&instance.config).await;
            tracing::info!("[Security] Runtime security config hot-reloaded");
        }
    }

    tracing::info!("[Security] Security monitor config updated and saved");
    Ok(())
}

// ==================== 统计分析命令 ====================

/// 获取 IP Token 消耗统计
#[tauri::command]
pub async fn get_ip_token_stats(
    limit: Option<usize>,
    hours: Option<i64>,
) -> Result<Vec<crate::modules::proxy_db::IpTokenStats>, String> {
    crate::modules::proxy_db::get_token_usage_by_ip(limit.unwrap_or(100), hours.unwrap_or(720))
}

// ==================== 辅助函数 ====================

/// 验证 IP 模式格式 (支持单个 IP 和 CIDR)
fn is_valid_ip_pattern(pattern: &str) -> bool {
    // 检查是否为 CIDR 格式
    if pattern.contains('/') {
        let parts: Vec<&str> = pattern.split('/').collect();
        if parts.len() != 2 {
            return false;
        }

        // 验证 IP 部分
        if !is_valid_ip(parts[0]) {
            return false;
        }

        // 验证掩码部分
        if let Ok(mask) = parts[1].parse::<u8>() {
            return mask <= 32;
        }
        return false;
    }

    // 单个 IP 地址
    is_valid_ip(pattern)
}

/// 验证 IP 地址格式
fn is_valid_ip(ip: &str) -> bool {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return false;
    }

    for part in parts {
        if part.parse::<u8>().is_err() {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_ip_patterns() {
        assert!(is_valid_ip_pattern("192.168.1.1"));
        assert!(is_valid_ip_pattern("10.0.0.0/8"));
        assert!(is_valid_ip_pattern("172.16.0.0/16"));
        assert!(is_valid_ip_pattern("192.168.1.0/24"));
        assert!(is_valid_ip_pattern("8.8.8.8/32"));
    }

    #[test]
    fn test_invalid_ip_patterns() {
        assert!(!is_valid_ip_pattern("256.1.1.1"));
        assert!(!is_valid_ip_pattern("192.168.1"));
        assert!(!is_valid_ip_pattern("192.168.1.1/33"));
        assert!(!is_valid_ip_pattern("192.168.1.1/"));
        assert!(!is_valid_ip_pattern("invalid"));
    }
}
