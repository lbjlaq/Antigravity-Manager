//! HTTP API 路由处理器

use std::sync::Arc;
use axum::{
    extract::{Path, State, Json},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;

use crate::state::AppState;
use crate::modules::{self, quota};
use crate::models;

/// 健康检查
pub async fn health_check() -> impl IntoResponse {
    Json(json!({"status": "ok", "version": env!("CARGO_PKG_VERSION")}))
}

/// 验证 API Key
/// 支持两种方式：
/// 1. x-api-key 头（用于管理接口）
/// 2. Authorization: Bearer <key>（用于代理接口，兼容 OpenAI/Anthropic 格式）
pub(crate) fn verify_api_key(headers: &HeaderMap, state: &AppState) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    // 方式1: 检查 x-api-key 头
    let key_from_header = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    
    // 方式2: 检查 Authorization: Bearer <key>
    let key_from_auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| {
            if s.starts_with("Bearer ") {
                Some(&s[7..]) // 跳过 "Bearer " 前缀
            } else {
                None
            }
        })
        .unwrap_or("");
    
    // 使用第一个非空的 key
    let key = if !key_from_header.is_empty() {
        key_from_header
    } else {
        key_from_auth
    };
    
    if key.is_empty() {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Missing API key. Please provide x-api-key header or Authorization: Bearer <key>"}))
        ));
    }
    
    if key != state.api_key {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid API key"}))
        ));
    }
    Ok(())
}

/// 获取账号列表
pub async fn list_accounts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state) {
        return e.into_response();
    }

    match state.account_manager.list_accounts().await {
        Ok(accounts) => Json(accounts).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()}))
        ).into_response(),
    }
}

#[derive(Deserialize)]
pub struct AddAccountRequest {
    pub email: String,
    pub refresh_token: String,
}

/// 添加账号
pub async fn add_account(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<AddAccountRequest>,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state) {
        return e.into_response();
    }

    match state.account_manager.add_account(&req.email, &req.refresh_token).await {
        Ok(account) => {
            // 同步到 TokenManager
            if let Err(e) = state.token_manager.sync_from_account(&account).await {
                tracing::warn!("添加账号后同步到 TokenManager 失败: {}", e);
            }
            (StatusCode::CREATED, Json(account)).into_response()
        },
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": e.to_string()}))
        ).into_response(),
    }
}

/// 删除账号
pub async fn delete_account(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state) {
        return e.into_response();
    }

    match state.account_manager.delete_account(&id).await {
        Ok(_) => {
            // 从 TokenManager 移除
            state.token_manager.remove_account(&id);
            Json(json!({"success": true})).into_response()
        },
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": e.to_string()}))
        ).into_response(),
    }
}

/// 切换当前账号
pub async fn switch_account(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state) {
        return e.into_response();
    }

    match state.account_manager.switch_account(&id).await {
        Ok(account) => Json(account).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": e.to_string()}))
        ).into_response(),
    }
}

/// 获取当前账号
pub async fn get_current_account(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state) {
        return e.into_response();
    }

    match state.account_manager.get_current_account().await {
        Some(account) => Json(account).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "No current account"}))
        ).into_response(),
    }
}

/// 获取账号配额
pub async fn get_account_quota(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state) {
        return e.into_response();
    }

    let account = match state.account_manager.get_account(&id).await {
        Some(acc) => acc,
        None => return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Account not found"}))
        ).into_response(),
    };

    match quota::fetch_quota(&account.token.access_token).await {
        Ok(quota_data) => {
            let _ = state.account_manager.update_quota(&id, quota_data.clone()).await;
            Json(quota_data).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()}))
        ).into_response(),
    }
}

/// 重新加载所有账号配额
pub async fn reload_accounts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state) {
        return e.into_response();
    }

    let accounts = match state.account_manager.list_accounts().await {
        Ok(accs) => accs,
        Err(e) => return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()}))
        ).into_response(),
    };

    let mut success = 0;
    let mut failed = 0;
    let mut details = Vec::new();

    for account in &accounts {
        match quota::fetch_quota(&account.token.access_token).await {
            Ok(quota_data) => {
                let _ = state.account_manager.update_quota(&account.id, quota_data).await;
                success += 1;
            }
            Err(e) => {
                failed += 1;
                tracing::warn!("Failed to fetch quota for {}: {}", account.email, e);
                details.push(format!("{}: {}", account.email, e));
            }
        }
    }

    let updated = state.account_manager.list_accounts().await.unwrap_or_default();
    
    // 重新同步 TokenManager
    if let Err(e) = state.token_manager.sync_from_account_manager(&updated).await {
        tracing::warn!("重新加载账号后同步 TokenManager 失败: {}", e);
    }
    
    Json(json!({
        "success": true,
        "accounts_loaded": accounts.len(),
        "quota_success": success,
        "quota_failed": failed,
        "details": details,
        "accounts": updated
    })).into_response()
}

/// 获取配置
pub async fn get_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state) {
        return e.into_response();
    }

    match modules::load_app_config() {
        Ok(config) => Json(config).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e}))
        ).into_response(),
    }
}

/// 保存配置
pub async fn save_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(config): Json<models::AppConfig>,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state) {
        return e.into_response();
    }

    match modules::save_app_config(&config) {
        Ok(_) => Json(json!({"success": true})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e}))
        ).into_response(),
    }
}

/// 启动代理服务
pub async fn start_proxy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state) {
        return e.into_response();
    }
    state.proxy_enabled.store(true, std::sync::atomic::Ordering::Relaxed);
    Json(json!({"success": true, "message": "Proxy service started"})).into_response()
}

/// 停止代理服务
pub async fn stop_proxy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state) {
        return e.into_response();
    }
    state.proxy_enabled.store(false, std::sync::atomic::Ordering::Relaxed);
    Json(json!({"success": true, "message": "Proxy service stopped"})).into_response()
}

/// 获取代理状态
pub async fn get_proxy_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state) {
        return e.into_response();
    }
    let running = state.proxy_enabled.load(std::sync::atomic::Ordering::Relaxed);
    let active_accounts = state.token_manager.len();
    
    // 端口优先级：环境变量 > 配置文件 > 默认值
    let port: u16 = {
        // 1. 先尝试从环境变量读取
        if let Ok(port_str) = std::env::var("PORT") {
            if let Ok(port) = port_str.parse::<u16>() {
                port
            } else {
                // 2. 环境变量无效，尝试从配置文件读取
                if let Ok(config) = modules::load_app_config() {
                    if config.proxy.port > 0 {
                        config.proxy.port
                    } else {
                        8045
                    }
                } else {
                    8045
                }
            }
        } else {
            // 3. 环境变量不存在，尝试从配置文件读取
            if let Ok(config) = modules::load_app_config() {
                if config.proxy.port > 0 {
                    config.proxy.port
                } else {
                    8045
                }
            } else {
                8045
            }
        }
    };
    
    Json(json!({
        "running": running,
        "port": port,
        "base_url": format!("http://localhost:{}", port),
        "active_accounts": active_accounts,
        "pid": std::process::id()
    })).into_response()
}
