//! HTTP API 模块
//! 提供本地 HTTP 接口供外部程序（如 VS Code 插件）调用
//! 
//! 端点：
//! - GET  /health                    健康检查
//! - GET  /accounts                  获取所有账号及配额
//! - GET  /accounts/current          获取当前账号
//! - POST /accounts/switch           切换账号（异步执行）
//! - POST /accounts/refresh          刷新所有配额
//! - POST /accounts/:id/bind-device  绑定设备指纹

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

use crate::modules::{account, logger, proxy_db};

/// HTTP API 服务器默认端口
pub const DEFAULT_PORT: u16 = 19527;

// ============================================================================
// Settings
// ============================================================================

/// HTTP API 设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpApiSettings {
    /// 是否启用 HTTP API 服务
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// 监听端口
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_enabled() -> bool {
    true
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

impl Default for HttpApiSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            port: DEFAULT_PORT,
        }
    }
}

/// 加载 HTTP API 设置
pub fn load_settings() -> Result<HttpApiSettings, String> {
    let data_dir = crate::modules::account::get_data_dir()
        .map_err(|e| format!("Failed to get data dir: {}", e))?;
    let settings_path = data_dir.join("http_api_settings.json");

    if !settings_path.exists() {
        return Ok(HttpApiSettings::default());
    }

    let content = std::fs::read_to_string(&settings_path)
        .map_err(|e| format!("Failed to read settings file: {}", e))?;

    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings: {}", e))
}

/// 保存 HTTP API 设置
pub fn save_settings(settings: &HttpApiSettings) -> Result<(), String> {
    let data_dir = crate::modules::account::get_data_dir()
        .map_err(|e| format!("Failed to get data dir: {}", e))?;
    let settings_path = data_dir.join("http_api_settings.json");

    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    std::fs::write(&settings_path, content)
        .map_err(|e| format!("Failed to write settings file: {}", e))
}

/// 服务器状态
#[derive(Clone)]
pub struct ApiState {
    /// 当前是否有切换操作正在进行
    switching: Arc<RwLock<bool>>,
}

impl ApiState {
    pub fn new() -> Self {
        Self {
            switching: Arc::new(RwLock::new(false)),
        }
    }
}

// ============================================================================
// Response Types
// ============================================================================

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: String,
}

#[derive(Serialize)]
struct AccountResponse {
    id: String,
    email: String,
    name: Option<String>,
    is_current: bool,
    disabled: bool,
    quota: Option<QuotaResponse>,
    device_bound: bool,
    last_used: i64,
}

#[derive(Serialize)]
struct QuotaResponse {
    models: Vec<ModelQuota>,
    updated_at: Option<i64>,
    subscription_tier: Option<String>,
}

#[derive(Serialize)]
struct ModelQuota {
    name: String,
    percentage: i32,
    reset_time: String,
}

#[derive(Serialize)]
struct AccountListResponse {
    accounts: Vec<AccountResponse>,
    current_account_id: Option<String>,
}

#[derive(Serialize)]
struct CurrentAccountResponse {
    account: Option<AccountResponse>,
}

#[derive(Serialize)]
struct SwitchResponse {
    success: bool,
    message: String,
}

#[derive(Serialize)]
struct RefreshResponse {
    success: bool,
    message: String,
    refreshed_count: usize,
}

#[derive(Serialize)]
struct BindDeviceResponse {
    success: bool,
    message: String,
    device_profile: Option<DeviceProfileResponse>,
}

#[derive(Serialize)]
struct DeviceProfileResponse {
    machine_id: String,
    mac_machine_id: String,
    dev_device_id: String,
    sqm_id: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
struct LogsResponse {
    total: u64,
    logs: Vec<crate::proxy::monitor::ProxyRequestLog>,
}

// ============================================================================
// Request Types
// ============================================================================

#[derive(Deserialize)]
struct SwitchRequest {
    account_id: String,
}

#[derive(Deserialize)]
struct BindDeviceRequest {
    #[serde(default = "default_bind_mode")]
    mode: String,
}

fn default_bind_mode() -> String {
    "generate".to_string()
}

#[derive(Deserialize)]
struct LogsRequest {
    #[serde(default)]
    limit: usize,
    #[serde(default)]
    offset: usize,
    #[serde(default)]
    filter: String,
    #[serde(default)]
    errors_only: bool,
}

// ============================================================================
// Handlers
// ============================================================================

/// GET /health - 健康检查
async fn health() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// GET /accounts - 获取所有账号
async fn list_accounts() -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let accounts = account::list_accounts().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
    })?;

    let current_id = account::get_current_account_id()
        .ok()
        .flatten();

    let account_responses: Vec<AccountResponse> = accounts
        .into_iter()
        .map(|acc| {
            let is_current = current_id.as_ref().map(|id| id == &acc.id).unwrap_or(false);
            let quota = acc.quota.map(|q| QuotaResponse {
                models: q.models.into_iter().map(|m| ModelQuota {
                    name: m.name,
                    percentage: m.percentage,
                    reset_time: m.reset_time,
                }).collect(),
                updated_at: Some(q.last_updated),
                subscription_tier: q.subscription_tier,
            });
            
            AccountResponse {
                id: acc.id,
                email: acc.email,
                name: acc.name,
                is_current,
                disabled: acc.disabled,
                quota,
                device_bound: acc.device_profile.is_some(),
                last_used: acc.last_used,
            }
        })
        .collect();

    Ok(Json(AccountListResponse {
        current_account_id: current_id,
        accounts: account_responses,
    }))
}

/// GET /accounts/current - 获取当前账号
async fn get_current_account() -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let current = account::get_current_account().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
    })?;

    let response = current.map(|acc| {
        let quota = acc.quota.map(|q| QuotaResponse {
            models: q.models.into_iter().map(|m| ModelQuota {
                name: m.name,
                percentage: m.percentage,
                reset_time: m.reset_time,
            }).collect(),
            updated_at: Some(q.last_updated),
            subscription_tier: q.subscription_tier,
        });

        AccountResponse {
            id: acc.id,
            email: acc.email,
            name: acc.name,
            is_current: true,
            disabled: acc.disabled,
            quota,
            device_bound: acc.device_profile.is_some(),
            last_used: acc.last_used,
        }
    });

    Ok(Json(CurrentAccountResponse { account: response }))
}

/// POST /accounts/switch - 切换账号
async fn switch_account(
    State(state): State<ApiState>,
    Json(payload): Json<SwitchRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    // 检查是否已有切换操作在进行
    {
        let switching = state.switching.read().await;
        if *switching {
            return Err((
                StatusCode::CONFLICT,
                Json(ErrorResponse {
                    error: "另一个切换操作正在进行中".to_string(),
                }),
            ));
        }
    }

    // 标记切换开始
    {
        let mut switching = state.switching.write().await;
        *switching = true;
    }

    let account_id = payload.account_id.clone();
    let state_clone = state.clone();

    // 异步执行切换（不阻塞响应）
    tokio::spawn(async move {
        logger::log_info(&format!("[HTTP API] 开始切换账号: {}", account_id));
        
        match account::switch_account(&account_id).await {
            Ok(()) => {
                logger::log_info(&format!("[HTTP API] 账号切换成功: {}", account_id));
            }
            Err(e) => {
                logger::log_error(&format!("[HTTP API] 账号切换失败: {}", e));
            }
        }

        // 标记切换结束
        let mut switching = state_clone.switching.write().await;
        *switching = false;
    });

    // 立即返回 202 Accepted
    Ok((
        StatusCode::ACCEPTED,
        Json(SwitchResponse {
            success: true,
            message: format!("账号切换任务已启动: {}", payload.account_id),
        }),
    ))
}

/// POST /accounts/refresh - 刷新所有配额
async fn refresh_all_quotas() -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    logger::log_info("[HTTP API] 开始刷新所有账号配额");

    // 异步执行刷新
    tokio::spawn(async {
        match account::refresh_all_quotas_logic().await {
            Ok(stats) => {
                logger::log_info(&format!(
                    "[HTTP API] 配额刷新完成，成功 {}/{} 个账号",
                    stats.success, stats.total
                ));
            }
            Err(e) => {
                logger::log_error(&format!("[HTTP API] 配额刷新失败: {}", e));
            }
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(RefreshResponse {
            success: true,
            message: "配额刷新任务已启动".to_string(),
            refreshed_count: 0,
        }),
    ))
}

/// POST /accounts/:id/bind-device - 绑定设备指纹
async fn bind_device(
    Path(account_id): Path<String>,
    Json(payload): Json<BindDeviceRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    logger::log_info(&format!(
        "[HTTP API] 绑定设备指纹: account={}, mode={}",
        account_id, payload.mode
    ));

    let result = account::bind_device_profile(&account_id, &payload.mode).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
    })?;

    Ok(Json(BindDeviceResponse {
        success: true,
        message: "设备指纹绑定成功".to_string(),
        device_profile: Some(DeviceProfileResponse {
            machine_id: result.machine_id,
            mac_machine_id: result.mac_machine_id,
            dev_device_id: result.dev_device_id,
            sqm_id: result.sqm_id,
        }),
    }))
}

/// GET /logs - 获取代理日志
async fn get_logs(
    Query(params): Query<LogsRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let limit = if params.limit == 0 { 50 } else { params.limit };

    let total = proxy_db::get_logs_count_filtered(&params.filter, params.errors_only)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e })))?;

    let logs = proxy_db::get_logs_filtered(&params.filter, params.errors_only, limit, params.offset)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e })))?;

    Ok(Json(LogsResponse {
        total,
        logs,
    }))
}

// ============================================================================
// Server
// ============================================================================

/// 启动 HTTP API 服务器
pub async fn start_server(port: u16) -> Result<(), String> {
    let state = ApiState::new();

    // CORS 配置 - 允许本地调用
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/health", get(health))
        .route("/accounts", get(list_accounts))
        .route("/accounts/current", get(get_current_account))
        .route("/accounts/switch", post(switch_account))
        .route("/accounts/refresh", post(refresh_all_quotas))
        .route("/accounts/{id}/bind-device", post(bind_device))
        .route("/logs", get(get_logs))
        .layer(cors)
        .with_state(state);

    let addr = format!("127.0.0.1:{}", port);
    logger::log_info(&format!("[HTTP API] 启动服务器: http://{}", addr));

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("绑定端口失败: {}", e))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("服务器运行失败: {}", e))?;

    Ok(())
}

/// 在后台启动 HTTP API 服务器（非阻塞）
pub fn spawn_server(port: u16) {
    // 使用 tauri::async_runtime::spawn 以确保在 Tauri 的 runtime 中运行
    tauri::async_runtime::spawn(async move {
        if let Err(e) = start_server(port).await {
            logger::log_error(&format!("[HTTP API] 服务器启动失败: {}", e));
        }
    });
}
