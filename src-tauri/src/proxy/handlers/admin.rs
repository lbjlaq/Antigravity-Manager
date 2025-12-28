// 管理API处理器 - 用于Web界面

use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Json},
    http::header,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::proxy::server::AppState;
use crate::proxy::admin::models::{AdminError, StatusDto};

/// 管理界面HTML
pub async fn serve_admin_ui() -> impl IntoResponse {
    Html(include_str!("../../../static/admin.html"))
}

/// 服务图标文件
pub async fn serve_icon() -> impl IntoResponse {
    let icon_bytes = include_bytes!("../../../static/icon.png");
    (
        [(header::CONTENT_TYPE, "image/png")],
        icon_bytes.as_slice()
    )
}

/// 获取配置
#[derive(Serialize)]
pub struct ConfigResponse {
    proxy: ProxyConfigData,
    accounts_count: usize,
}

#[derive(Serialize)]
pub struct ProxyConfigData {
    enabled: bool,
    port: u16,
    allow_lan_access: bool,
    request_timeout: u64,
    anthropic_mapping: HashMap<String, String>,
    openai_mapping: HashMap<String, String>,
    custom_mapping: HashMap<String, String>,
}

pub async fn get_config(State(_state): State<AppState>) -> Result<Json<ConfigResponse>, AdminError> {
    // 加载配置文件
    let config = crate::modules::config::load_app_config()
        .map_err(|e| AdminError::internal(format!("Failed to load config: {}", e)))?;

    // 获取账号数量
    let accounts = crate::modules::account::list_accounts()
        .map_err(|e| AdminError::internal(format!("Failed to list accounts: {}", e)))?;

    let response = ConfigResponse {
        proxy: ProxyConfigData {
            enabled: config.proxy.enabled,
            port: config.proxy.port,
            allow_lan_access: config.proxy.allow_lan_access,
            request_timeout: config.proxy.request_timeout,
            anthropic_mapping: config.proxy.anthropic_mapping,
            openai_mapping: config.proxy.openai_mapping,
            custom_mapping: config.proxy.custom_mapping,
        },
        accounts_count: accounts.len(),
    };

    Ok(Json(response))
}

/// 更新配置请求
#[derive(Deserialize)]
pub struct UpdateConfigRequest {
    port: Option<u16>,
    allow_lan_access: Option<bool>,
    request_timeout: Option<u64>,
    anthropic_mapping: Option<HashMap<String, String>>,
    openai_mapping: Option<HashMap<String, String>>,
    custom_mapping: Option<HashMap<String, String>>,
}

pub async fn update_config(
    State(state): State<AppState>,
    Json(req): Json<UpdateConfigRequest>,
) -> Result<Json<serde_json::Value>, AdminError> {
    // 加载当前配置
    let mut config = crate::modules::config::load_app_config()
        .map_err(|e| AdminError::internal(format!("Failed to load config: {}", e)))?;

    // 更新字段
    if let Some(port) = req.port {
        config.proxy.port = port;
    }
    if let Some(allow_lan) = req.allow_lan_access {
        config.proxy.allow_lan_access = allow_lan;
    }
    if let Some(timeout) = req.request_timeout {
        config.proxy.request_timeout = timeout;
    }
    if let Some(mapping) = req.anthropic_mapping {
        config.proxy.anthropic_mapping = mapping;
    }
    if let Some(mapping) = req.openai_mapping {
        config.proxy.openai_mapping = mapping;
    }
    if let Some(mapping) = req.custom_mapping {
        config.proxy.custom_mapping = mapping;
    }

    // 保存配置
    crate::modules::config::save_app_config(&config)
        .map_err(|e| AdminError::internal(format!("Failed to save config: {}", e)))?;

    // 热更新映射（如果服务正在运行）
    {
        let mut anthropic = state.anthropic_mapping.write().await;
        *anthropic = config.proxy.anthropic_mapping.clone();
    }
    {
        let mut openai = state.openai_mapping.write().await;
        *openai = config.proxy.openai_mapping.clone();
    }
    {
        let mut custom = state.custom_mapping.write().await;
        *custom = config.proxy.custom_mapping.clone();
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "配置已更新（部分配置需要重启生效）"
    })))
}

/// 账号列表响应
#[derive(Serialize)]
pub struct AccountInfo {
    id: String,
    email: String,
    name: Option<String>,
    is_active: bool,
    quota: Option<QuotaInfo>,
}

#[derive(Serialize)]
pub struct QuotaInfo {
    models: Vec<ModelQuotaInfo>,
    is_forbidden: bool,
}

#[derive(Serialize)]
pub struct ModelQuotaInfo {
    name: String,
    percentage: i32,
    reset_time: String,
}

pub async fn list_accounts() -> Result<Json<Vec<AccountInfo>>, AdminError> {
    let accounts = crate::modules::account::list_accounts()
        .map_err(|e| AdminError::internal(format!("Failed to list accounts: {}", e)))?;

    let current_id = crate::modules::account::get_current_account_id()
        .ok()
        .flatten();

    let infos: Vec<AccountInfo> = accounts.into_iter().map(|acc| {
        AccountInfo {
            id: acc.id.clone(),
            email: acc.email,
            name: acc.name,
            is_active: current_id.as_ref() == Some(&acc.id),
            quota: acc.quota.map(|q| QuotaInfo {
                models: q.models.iter().map(|m| ModelQuotaInfo {
                    name: m.name.clone(),
                    percentage: m.percentage,
                    reset_time: m.reset_time.clone(),
                }).collect(),
                is_forbidden: q.is_forbidden,
            }),
        }
    }).collect();

    Ok(Json(infos))
}

/// 添加账号请求
#[derive(Deserialize)]
pub struct AddAccountRequest {
    refresh_token: String,
}

pub async fn add_account(
    State(state): State<AppState>,
    Json(req): Json<AddAccountRequest>,
) -> Result<Json<serde_json::Value>, AdminError> {
    // 1. 使用refresh_token获取access_token
    let token_res = crate::modules::oauth::refresh_access_token(&req.refresh_token)
        .await
        .map_err(|e| AdminError::bad_request(format!("Invalid refresh token: {}", e)))?;

    // 2. 获取用户信息
    let user_info = crate::modules::oauth::get_user_info(&token_res.access_token)
        .await
        .map_err(|e| AdminError::bad_request(format!("Failed to get user info: {}", e)))?;

    // 3. 尝试获取project_id
    let project_id = crate::proxy::project_resolver::fetch_project_id(&token_res.access_token)
        .await
        .ok();

    // 4. 构造TokenData
    let token = crate::models::TokenData::new(
        token_res.access_token,
        req.refresh_token,
        token_res.expires_in,
        Some(user_info.email.clone()),
        project_id,
        None,
    );

    // 5. 保存账号
    let account = crate::modules::account::upsert_account(
        user_info.email.clone(),
        user_info.get_display_name(),
        token,
    ).map_err(|e| AdminError::internal(format!("Failed to save account: {}", e)))?;

    // 6. 运行时同步 TokenManager
    if let Err(e) = state.token_manager.reload_accounts().await {
        tracing::warn!("Failed to reload accounts in TokenManager: {}", e);
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "account": {
            "id": account.id,
            "email": account.email
        }
    })))
}

/// 删除账号
pub async fn delete_account(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
) -> Result<Json<serde_json::Value>, AdminError> {
    crate::modules::account::delete_account(&account_id)
        .map_err(|e| AdminError::internal(format!("Failed to delete account: {}", e)))?;

    // 运行时同步 TokenManager
    if let Err(e) = state.token_manager.reload_accounts().await {
        tracing::warn!("Failed to reload accounts in TokenManager: {}", e);
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "账号已删除"
    })))
}

/// 切换当前账号（headless 模式使用 pin 机制，无需重启）
pub async fn switch_account(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
) -> Result<Json<serde_json::Value>, AdminError> {
    // 验证账号是否存在
    let accounts = crate::modules::account::list_accounts()
        .map_err(|e| AdminError::internal(format!("Failed to list accounts: {}", e)))?;

    if !accounts.iter().any(|acc| acc.id == account_id) {
        return Err(AdminError::not_found("Account not found"));
    }

    // 使用 pin 机制切换账号（headless 模式专用）
    state.token_manager.pin_account(Some(account_id.clone())).await;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "已切换到指定账号"
    })))
}

/// 刷新账号配额
pub async fn refresh_account_quota(
    Path(account_id): Path<String>,
) -> Result<Json<serde_json::Value>, AdminError> {
    // 加载账号
    let mut account = crate::modules::account::load_account(&account_id)
        .map_err(|e| AdminError::not_found(format!("Account not found: {}", e)))?;

    // 刷新配额
    let quota = crate::modules::account::fetch_quota_with_retry(&mut account)
        .await
        .map_err(|e| AdminError::internal(format!("Failed to fetch quota: {}", e)))?;

    // 更新账号配额
    crate::modules::account::update_account_quota(&account_id, quota)
        .map_err(|e| AdminError::internal(format!("Failed to update quota: {}", e)))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "配额已刷新"
    })))
}

/// 服务状态
pub async fn get_status(State(state): State<AppState>) -> Json<StatusDto> {
    let pinned_account_id = state.token_manager.pinned_account_id().await;
    let active_accounts = state.token_manager.len();

    Json(StatusDto {
        running: true,
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: 0, // TODO: 实现真实uptime
        active_accounts,
        pinned_account_id,
    })
}

// ==================== 请求统计API ====================

use crate::proxy::admin::stats::TimeSeriesData;

#[derive(Serialize)]
pub struct StatsResponse {
    requests_total: u64,
    requests_ok: u64,
    requests_err: u64,
    success_rate: f64,
    latency_ms_avg: f64,
    latency_ms_p95: f64,
    rps: f64,
    time_series: TimeSeriesData,
}

pub async fn get_stats() -> Json<StatsResponse> {
    let snapshot = crate::proxy::admin::global_stats().snapshot().await;
    Json(StatsResponse {
        requests_total: snapshot.requests_total,
        requests_ok: snapshot.requests_ok,
        requests_err: snapshot.requests_err,
        success_rate: snapshot.success_rate,
        latency_ms_avg: snapshot.latency_ms_avg,
        latency_ms_p95: snapshot.latency_ms_p95,
        rps: snapshot.rps,
        time_series: snapshot.time_series,
    })
}

// ==================== 配置导出API ====================

#[derive(Serialize)]
pub struct ConfigExport {
    schema_version: u32,
    exported_at_ms: u64,
    app_version: String,
    config: ConfigExportData,
}

#[derive(Serialize)]
struct ConfigExportData {
    proxy: ProxyConfigData,
}

pub async fn export_config() -> Result<Json<ConfigExport>, AdminError> {
    let config = crate::modules::config::load_app_config()
        .map_err(|e| AdminError::internal(format!("Failed to load config: {}", e)))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    Ok(Json(ConfigExport {
        schema_version: 1,
        exported_at_ms: now,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        config: ConfigExportData {
            proxy: ProxyConfigData {
                enabled: config.proxy.enabled,
                port: config.proxy.port,
                allow_lan_access: config.proxy.allow_lan_access,
                request_timeout: config.proxy.request_timeout,
                anthropic_mapping: config.proxy.anthropic_mapping,
                openai_mapping: config.proxy.openai_mapping,
                custom_mapping: config.proxy.custom_mapping,
            },
        },
    }))
}

// ==================== 配置导入API ====================

#[derive(Deserialize)]
pub struct ConfigImportRequest {
    #[allow(dead_code)]
    schema_version: Option<u32>,
    config: Option<ConfigImportData>,
    // 直接导入proxy字段也支持
    proxy: Option<ProxyImportData>,
}

#[derive(Deserialize)]
struct ConfigImportData {
    proxy: Option<ProxyImportData>,
}

#[derive(Deserialize)]
struct ProxyImportData {
    enabled: Option<bool>,
    port: Option<u16>,
    allow_lan_access: Option<bool>,
    request_timeout: Option<u64>,
    anthropic_mapping: Option<HashMap<String, String>>,
    openai_mapping: Option<HashMap<String, String>>,
    custom_mapping: Option<HashMap<String, String>>,
}

#[derive(Serialize)]
pub struct ImportValidationResult {
    valid: bool,
    errors: Vec<ImportError>,
    warnings: Vec<String>,
}

#[derive(Serialize)]
struct ImportError {
    path: String,
    message: String,
}

pub async fn import_config(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
    Json(req): Json<ConfigImportRequest>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let mode = params.get("mode").map(|s| s.as_str()).unwrap_or("validate");

    // 提取proxy配置
    let proxy_data = req.config
        .and_then(|c| c.proxy)
        .or(req.proxy);

    let proxy_data = match proxy_data {
        Some(p) => p,
        None => return Err(AdminError::bad_request("Missing proxy configuration")),
    };

    // 验证
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if let Some(port) = proxy_data.port {
        if port == 0 {
            errors.push(ImportError {
                path: "proxy.port".to_string(),
                message: "Port must be between 1 and 65535".to_string(),
            });
        }
        if port < 1024 {
            warnings.push("Using privileged port (< 1024) may require root".to_string());
        }
    }

    if let Some(timeout) = proxy_data.request_timeout {
        if timeout < 10 || timeout > 600 {
            errors.push(ImportError {
                path: "proxy.request_timeout".to_string(),
                message: "Timeout must be between 10 and 600 seconds".to_string(),
            });
        }
    }

    let valid = errors.is_empty();

    if mode == "validate" {
        return Ok(Json(serde_json::json!({
            "valid": valid,
            "errors": errors,
            "warnings": warnings
        })));
    }

    // 应用配置
    if !valid {
        return Err(AdminError::bad_request("Configuration validation failed"));
    }

    let mut config = crate::modules::config::load_app_config()
        .map_err(|e| AdminError::internal(format!("Failed to load config: {}", e)))?;

    if let Some(enabled) = proxy_data.enabled {
        config.proxy.enabled = enabled;
    }
    if let Some(port) = proxy_data.port {
        config.proxy.port = port;
    }
    if let Some(allow_lan) = proxy_data.allow_lan_access {
        config.proxy.allow_lan_access = allow_lan;
    }
    if let Some(timeout) = proxy_data.request_timeout {
        config.proxy.request_timeout = timeout;
    }
    if let Some(mapping) = proxy_data.anthropic_mapping {
        config.proxy.anthropic_mapping = mapping;
    }
    if let Some(mapping) = proxy_data.openai_mapping {
        config.proxy.openai_mapping = mapping;
    }
    if let Some(mapping) = proxy_data.custom_mapping {
        config.proxy.custom_mapping = mapping;
    }

    crate::modules::config::save_app_config(&config)
        .map_err(|e| AdminError::internal(format!("Failed to save config: {}", e)))?;

    // 热更新映射
    {
        let mut anthropic = state.anthropic_mapping.write().await;
        *anthropic = config.proxy.anthropic_mapping.clone();
    }
    {
        let mut openai = state.openai_mapping.write().await;
        *openai = config.proxy.openai_mapping.clone();
    }
    {
        let mut custom = state.custom_mapping.write().await;
        *custom = config.proxy.custom_mapping.clone();
    }

    Ok(Json(serde_json::json!({
        "applied": true,
        "restart_required": true,
        "message": "Configuration applied successfully"
    })))
}
