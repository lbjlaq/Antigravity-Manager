//! Proxy control admin handlers
//!
//! Handles proxy service control, session management, rate limiting, and monitoring.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use crate::modules::logger;
use crate::proxy::server::types::{
    AppState, ErrorResponse, LogsFilterQuery, UpdateMappingWrapper,
};

// ============================================================================
// Proxy Service Control
// ============================================================================

pub async fn get_proxy_status(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let active_accounts = state.token_manager.len();
    let is_running = { *state.is_running.read().await };
    
    Ok(Json(serde_json::json!({
        "running": is_running,
        "port": state.port,
        "base_url": format!("http://127.0.0.1:{}", state.port),
        "active_accounts": active_accounts,
    })))
}

pub async fn start_proxy_service(State(state): State<AppState>) -> impl IntoResponse {
    // 1. Persist config (fix #1166)
    if let Ok(mut config) = crate::modules::config::load_app_config() {
        config.proxy.auto_start = true;
        let _ = crate::modules::config::save_app_config(&config);
    }

    // 2. Load accounts if first start
    if let Err(e) = state.token_manager.load_accounts().await {
        logger::log_error(&format!("[API] Failed to load accounts on start: {}", e));
    }

    let mut running = state.is_running.write().await;
    *running = true;
    logger::log_info("[API] Proxy service enabled (persisted)");
    StatusCode::OK
}

pub async fn stop_proxy_service(State(state): State<AppState>) -> impl IntoResponse {
    // 1. Persist config (fix #1166)
    if let Ok(mut config) = crate::modules::config::load_app_config() {
        config.proxy.auto_start = false;
        let _ = crate::modules::config::save_app_config(&config);
    }

    let mut running = state.is_running.write().await;
    *running = false;
    logger::log_info("[API] Proxy service disabled (Axum mode / persisted)");
    StatusCode::OK
}

pub async fn update_model_mapping(
    State(state): State<AppState>,
    Json(payload): Json<UpdateMappingWrapper>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let config = payload.config;

    // 1. Hot-update memory state
    {
        let mut mapping = state.custom_mapping.write().await;
        *mapping = config.custom_mapping.clone();
    }

    // 2. Persist to disk (fix #1149)
    let mut app_config = crate::modules::config::load_app_config().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
    })?;

    app_config.proxy.custom_mapping = config.custom_mapping;

    crate::modules::config::save_app_config(&app_config).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
    })?;

    logger::log_info("[API] Model mapping hot-updated and saved via API");
    Ok(StatusCode::OK)
}

pub async fn generate_api_key() -> impl IntoResponse {
    let new_key = format!("sk-{}", uuid::Uuid::new_v4().to_string().replace("-", ""));
    Json(new_key)
}

// ============================================================================
// Session & Rate Limit Management
// ============================================================================

pub async fn clear_proxy_session_bindings(State(state): State<AppState>) -> impl IntoResponse {
    state.token_manager.clear_all_sessions();
    logger::log_info("[API] Cleared all session bindings");
    StatusCode::OK
}

pub async fn clear_all_rate_limits(State(state): State<AppState>) -> impl IntoResponse {
    state.token_manager.clear_all_rate_limits();
    logger::log_info("[API] Cleared all rate limit records");
    StatusCode::OK
}

pub async fn clear_rate_limit(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
) -> impl IntoResponse {
    let cleared = state.token_manager.clear_rate_limit(&account_id);
    if cleared {
        logger::log_info(&format!(
            "[API] Cleared rate limit for account {}",
            account_id
        ));
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

// ============================================================================
// Monitor Control
// ============================================================================

pub async fn set_proxy_monitor_enabled(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let enabled = payload
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // [FIX #1269] Only log when state actually changes
    if state.monitor.is_enabled() != enabled {
        state.monitor.set_enabled(enabled);
        logger::log_info(&format!("[API] Monitor state set to: {}", enabled));
    }

    StatusCode::OK
}

pub async fn get_proxy_stats(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let stats = state.monitor.get_stats().await;
    Ok(Json(stats))
}

// ============================================================================
// Logs Management
// ============================================================================

pub async fn get_proxy_logs_filtered(
    axum::extract::Query(params): axum::extract::Query<LogsFilterQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let res = tokio::task::spawn_blocking(move || {
        crate::modules::proxy_db::get_logs_filtered(
            &params.filter,
            params.errors_only,
            params.limit,
            params.offset,
        )
    })
    .await;

    match res {
        Ok(Ok(logs)) => Ok(Json(logs)),
        Ok(Err(e)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

pub async fn get_proxy_logs_count_filtered(
    axum::extract::Query(params): axum::extract::Query<LogsFilterQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let res = tokio::task::spawn_blocking(move || {
        crate::modules::proxy_db::get_logs_count_filtered(&params.filter, params.errors_only)
    })
    .await;

    match res {
        Ok(Ok(count)) => Ok(Json(count)),
        Ok(Err(e)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

pub async fn clear_proxy_logs() -> impl IntoResponse {
    let _ = tokio::task::spawn_blocking(|| {
        if let Err(e) = crate::modules::proxy_db::clear_logs() {
            logger::log_error(&format!("[API] Failed to clear proxy logs: {}", e));
        }
    })
    .await;
    logger::log_info("[API] Cleared all proxy logs");
    StatusCode::OK
}

pub async fn get_proxy_log_detail(
    Path(log_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let res = tokio::task::spawn_blocking(move || {
        crate::modules::proxy_db::get_log_detail(&log_id)
    })
    .await;

    match res {
        Ok(Ok(log)) => Ok(Json(log)),
        Ok(Err(e)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

// ============================================================================
// z.ai Integration
// ============================================================================

pub async fn fetch_zai_models(
    Path(_id): Path<String>,
    Json(payload): Json<serde_json::Value>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let zai_config = payload.get("zai").ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Missing zai config".to_string(),
            }),
        )
    })?;

    let api_key = zai_config
        .get("api_key")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let base_url = zai_config
        .get("base_url")
        .and_then(|v| v.as_str())
        .unwrap_or("https://api.z.ai");

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/v1/models", base_url))
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;

    let data: serde_json::Value = resp.json().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;

    let models = data
        .get("data")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id").and_then(|id| id.as_str().map(|s| s.to_string())))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    Ok(Json(models))
}

// ============================================================================
// Cloudflared Handlers
// ============================================================================

pub async fn cloudflared_get_status(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    state
        .cloudflared_state
        .ensure_manager()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;

    let lock = state.cloudflared_state.manager.read().await;
    if let Some(manager) = lock.as_ref() {
        let (installed, version) = manager.check_installed().await;
        let mut status = manager.get_status().await;
        status.installed = installed;
        status.version = version;
        if !installed {
            status.running = false;
            status.url = None;
        }
        Ok(Json(status))
    } else {
        Ok(Json(
            crate::modules::cloudflared::CloudflaredStatus::default(),
        ))
    }
}

pub async fn cloudflared_install(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    state
        .cloudflared_state
        .ensure_manager()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;

    let lock = state.cloudflared_state.manager.read().await;
    if let Some(manager) = lock.as_ref() {
        let status = manager.install().await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;
        Ok(Json(status))
    } else {
        Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Manager not initialized".to_string(),
            }),
        ))
    }
}

pub async fn cloudflared_start(
    State(state): State<AppState>,
    Json(payload): Json<crate::proxy::server::types::CloudflaredStartRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    state
        .cloudflared_state
        .ensure_manager()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;

    let lock = state.cloudflared_state.manager.read().await;
    if let Some(manager) = lock.as_ref() {
        let status = manager.start(payload.config).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;
        Ok(Json(status))
    } else {
        Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Manager not initialized".to_string(),
            }),
        ))
    }
}

pub async fn cloudflared_stop(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    state
        .cloudflared_state
        .ensure_manager()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;

    let lock = state.cloudflared_state.manager.read().await;
    if let Some(manager) = lock.as_ref() {
        let status = manager.stop().await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;
        Ok(Json(status))
    } else {
        Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Manager not initialized".to_string(),
            }),
        ))
    }
}

// ============================================================================
// CLI Sync Handlers
// ============================================================================

use crate::proxy::server::types::{
    CliConfigContentRequest, CliRestoreRequest, CliSyncRequest, CliSyncStatusRequest,
};

pub async fn get_cli_sync_status(
    Json(payload): Json<CliSyncStatusRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    crate::proxy::cli_sync::get_cli_sync_status(payload.app_type, payload.proxy_url)
        .await
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })
}

pub async fn execute_cli_sync(
    Json(payload): Json<CliSyncRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    crate::proxy::cli_sync::execute_cli_sync(payload.app_type, payload.proxy_url, payload.api_key)
        .await
        .map(|_| StatusCode::OK)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })
}

pub async fn execute_cli_restore(
    Json(payload): Json<CliRestoreRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    crate::proxy::cli_sync::execute_cli_restore(payload.app_type)
        .await
        .map(|_| StatusCode::OK)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })
}

pub async fn get_cli_config_content(
    Json(payload): Json<CliConfigContentRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    crate::proxy::cli_sync::get_cli_config_content(payload.app_type, payload.file_name)
        .await
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })
}

// ============================================================================
// [FIX #820] Preferred Account Handlers
// ============================================================================

pub async fn get_preferred_account(State(state): State<AppState>) -> impl IntoResponse {
    let pref = state.token_manager.get_preferred_account().await;
    Json(pref)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPreferredAccountRequest {
    pub account_id: Option<String>,
}

pub async fn set_preferred_account(
    State(state): State<AppState>,
    Json(payload): Json<SetPreferredAccountRequest>,
) -> impl IntoResponse {
    state
        .token_manager
        .set_preferred_account(payload.account_id)
        .await;
    StatusCode::OK
}
