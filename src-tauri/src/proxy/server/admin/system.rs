//! System and configuration admin handlers
//!
//! Handles configuration management, update checks, autostart, and file operations.

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::path::Path;

use crate::proxy::server::types::{AppState, ErrorResponse, SaveConfigWrapper, SaveFileRequest};

fn validate_save_path(path: &str) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("File path cannot be empty".to_string());
    }

    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    if normalized.contains("../") || normalized.contains("..\\") || normalized.ends_with("/..") {
        return Err("Path traversal is not allowed".to_string());
    }

    let forbidden_prefixes = [
        "/etc/",
        "/proc/",
        "/sys/",
        "/dev/",
        "/root/",
        "/var/spool/cron",
        "c:/windows",
        "c:/programdata",
    ];

    if forbidden_prefixes
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
    {
        return Err("Access to system-sensitive path is denied".to_string());
    }

    if let Some(parent) = Path::new(path).parent() {
        if !parent.exists() {
            return Err("Target directory does not exist".to_string());
        }
    }

    Ok(())
}

// ============================================================================
// Configuration
// ============================================================================

pub async fn get_config() -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let cfg = crate::modules::config::load_app_config().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
    })?;
    Ok(Json(cfg))
}

pub async fn save_config(
    State(state): State<AppState>,
    Json(payload): Json<SaveConfigWrapper>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let new_config = payload.config;
    
    // 1. Persist to disk
    crate::modules::config::save_app_config(&new_config).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
    })?;

    // 2. Hot-update memory state
    // Update model mapping
    {
        let mut mapping = state.custom_mapping.write().await;
        *mapping = new_config.clone().proxy.custom_mapping;
    }

    // Update upstream proxy
    {
        let mut proxy = state.upstream_proxy.write().await;
        *proxy = new_config.clone().proxy.upstream_proxy;
    }

    // Update security policy
    {
        let mut security = state.security.write().await;
        *security = crate::proxy::ProxySecurityConfig::from_proxy_config(&new_config.proxy);
    }

    // Update z.ai config
    {
        let mut zai = state.zai.write().await;
        *zai = new_config.clone().proxy.zai;
    }

    // Update experimental config
    {
        let mut exp = state.experimental.write().await;
        *exp = new_config.clone().proxy.experimental;
    }

    Ok(StatusCode::OK)
}

// ============================================================================
// Update Management
// ============================================================================

pub async fn get_update_settings() -> impl IntoResponse {
    match crate::modules::update_checker::load_update_settings() {
        Ok(s) => Json(serde_json::to_value(s).unwrap_or_default()),
        Err(_) => Json(serde_json::json!({
            "auto_check": true,
            "last_check_time": 0,
            "check_interval_hours": 24
        })),
    }
}

pub async fn should_check_updates(
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let settings = crate::modules::update_checker::load_update_settings().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;
    let should = crate::modules::update_checker::should_check_for_updates(&settings);
    Ok(Json(should))
}

pub async fn check_for_updates(
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let info = crate::modules::update_checker::check_for_updates()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;
    Ok(Json(info))
}

pub async fn update_last_check_time(
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    crate::modules::update_checker::update_last_check_time().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
    })?;
    Ok(StatusCode::OK)
}

pub async fn save_update_settings(Json(settings): Json<serde_json::Value>) -> impl IntoResponse {
    if let Ok(s) =
        serde_json::from_value::<crate::modules::update_checker::UpdateSettings>(settings)
    {
        let _ = crate::modules::update_checker::save_update_settings(&s);
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    }
}

// ============================================================================
// Autostart
// ============================================================================

pub async fn is_auto_launch_enabled() -> impl IntoResponse {
    // Note: Autostart requires tauri::AppHandle, not available in Axum State easily.
    // Return false in Web mode.
    Json(false)
}

pub async fn toggle_auto_launch(Json(_payload): Json<serde_json::Value>) -> impl IntoResponse {
    // Note: Autostart requires tauri::AppHandle.
    StatusCode::NOT_IMPLEMENTED
}

// ============================================================================
// HTTP API Settings
// ============================================================================

pub async fn get_http_api_settings() -> impl IntoResponse {
    Json(serde_json::json!({ "enabled": true, "port": 8045 }))
}

pub async fn save_http_api_settings(
    Json(payload): Json<crate::modules::http_api::HttpApiSettings>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    crate::modules::http_api::save_settings(&payload).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
    })?;
    Ok(StatusCode::OK)
}

// ============================================================================
// File Operations
// ============================================================================

pub async fn get_data_dir_path() -> impl IntoResponse {
    match crate::modules::account::get_data_dir() {
        Ok(p) => Json(p.to_string_lossy().to_string()),
        Err(e) => Json(format!("Error: {}", e)),
    }
}

pub async fn save_text_file(
    Json(payload): Json<SaveFileRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    if let Err(e) = validate_save_path(&payload.path) {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })));
    }

    let res = tokio::task::spawn_blocking(move || {
        std::fs::write(&payload.path, &payload.content)
    })
    .await;

    match res {
        Ok(Ok(_)) => Ok(StatusCode::OK),
        Ok(Err(e)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

pub async fn open_folder() -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    crate::commands::system::open_data_folder()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;
    Ok(StatusCode::OK)
}

pub async fn get_antigravity_path(
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let path = crate::commands::system::get_antigravity_path(Some(true))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;
    Ok(Json(path))
}

pub async fn get_antigravity_args(
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let args = crate::commands::system::get_antigravity_args()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;
    Ok(Json(args))
}
