//! HTTP API Module
//! Provides local HTTP interfaces for external programs (e.g., VS Code extension) to call.

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

/// Default port for HTTP API server
pub const DEFAULT_PORT: u16 = 19527;

// ============================================================================
// Settings
// ============================================================================

/// HTTP API Settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpApiSettings {
    /// Whether to enable HTTP API service
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Listening port
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

/// Load HTTP API settings
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

/// Save HTTP API settings
pub fn save_settings(settings: &HttpApiSettings) -> Result<(), String> {
    let data_dir = crate::modules::account::get_data_dir()
        .map_err(|e| format!("Failed to get data dir: {}", e))?;
    let settings_path = data_dir.join("http_api_settings.json");

    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    std::fs::write(&settings_path, content)
        .map_err(|e| format!("Failed to write settings file: {}", e))
}

/// Server State
#[derive(Clone)]
pub struct ApiState {
    /// Whether there is a switch operation currently in progress
    pub switching: Arc<RwLock<bool>>,
    pub integration: crate::modules::integration::SystemManager,
}

impl ApiState {
    pub fn new(integration: crate::modules::integration::SystemManager) -> Self {
        Self {
            switching: Arc::new(RwLock::new(false)),
            integration,
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

/// GET /health - Health check
async fn health() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// GET /accounts - Get all accounts
async fn list_accounts() -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let accounts = account::list_accounts().await.map_err(|e| {
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

/// GET /accounts/current - Get current account
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

/// POST /accounts/switch - Switch account
async fn switch_account(
    State(state): State<ApiState>,
    Json(payload): Json<SwitchRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    // Check if another switch operation is already in progress
    {
        let switching = state.switching.read().await;
        if *switching {
            return Err((
                StatusCode::CONFLICT,
                Json(ErrorResponse {
                    error: "Another switch operation is already in progress".to_string(),
                }),
            ));
        }
    }

    // Mark switch started
    {
        let mut switching = state.switching.write().await;
        *switching = true;
    }

    let account_id = payload.account_id.clone();
    let state_clone = state.clone();

    // Execute switch asynchronously (non-blocking response)
    tokio::spawn(async move {
        logger::log_info(&format!("[HTTP API] Starting account switch: {}", account_id));
        
        match account::switch_account(&account_id, &state_clone.integration).await {
            Ok(()) => {
                logger::log_info(&format!("[HTTP API] Account switch successful: {}", account_id));
            }
            Err(e) => {
                logger::log_error(&format!("[HTTP API] Account switch failed: {}", e));
            }
        }

        // Mark switch ended
        let mut switching = state_clone.switching.write().await;
        *switching = false;
    });

    // Immediately return 202 Accepted
    Ok((
        StatusCode::ACCEPTED,
        Json(SwitchResponse {
            success: true,
            message: format!("Account switch task started: {}", payload.account_id),
        }),
    ))
}

/// POST /accounts/refresh - Refresh all quotas
async fn refresh_all_quotas() -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    logger::log_info("[HTTP API] Starting refresh of all account quotas");

    // Execute refresh asynchronously
    tokio::spawn(async {
        match account::refresh_all_quotas_logic().await {
            Ok(stats) => {
                logger::log_info(&format!(
                    "[HTTP API] Quota refresh completed, successful {}/{} accounts",
                    stats.success, stats.total
                ));
            }
            Err(e) => {
                logger::log_error(&format!("[HTTP API] Quota refresh failed: {}", e));
            }
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(RefreshResponse {
            success: true,
            message: "Quota refresh task started".to_string(),
            refreshed_count: 0,
        }),
    ))
}

/// POST /accounts/:id/bind-device - Bind device fingerprint
async fn bind_device(
    Path(account_id): Path<String>,
    Json(payload): Json<BindDeviceRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    logger::log_info(&format!(
        "[HTTP API] Binding device fingerprint: account={}, mode={}",
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
        message: "Device fingerprint bound successfully".to_string(),
        device_profile: Some(DeviceProfileResponse {
            machine_id: result.machine_id,
            mac_machine_id: result.mac_machine_id,
            dev_device_id: result.dev_device_id,
            sqm_id: result.sqm_id,
        }),
    }))
}

/// GET /logs - Get proxy logs
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

/// Start HTTP API server
pub async fn start_server(port: u16, integration: crate::modules::integration::SystemManager) -> Result<(), String> {
    let state = ApiState::new(integration);

    // CORS config - allow local calls
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
    logger::log_info(&format!("[HTTP API] Starting server: http://{}", addr));

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("failed_to_bind_port: {}", e))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("failed_to_run_server: {}", e))?;

    Ok(())
}

/// Start HTTP API server in background (non-blocking)
pub fn spawn_server(port: u16, integration: crate::modules::integration::SystemManager) {
    // Use tauri::async_runtime::spawn to ensure running within Tauri's runtime
    tauri::async_runtime::spawn(async move {
        if let Err(e) = start_server(port, integration).await {
            logger::log_error(&format!("[HTTP API] Failed to start server: {}", e));
        }
    });
}
