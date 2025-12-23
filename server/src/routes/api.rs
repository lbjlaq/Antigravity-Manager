//! 管理 API 路由

use std::sync::Arc;

use axum::{
    Router,
    routing::{get, post, delete},
    extract::{Path, State},
    response::IntoResponse,
    Json,
    http::{HeaderMap, StatusCode},
};
use serde_json::json;

use crate::services::AppState;
use crate::models::{AddAccountRequest, BatchAddAccountRequest, BatchAddResult};
use crate::error::{AppError, AppResult};

/// 创建管理 API 路由
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // 账号管理
        .route("/api/accounts", get(list_accounts))
        .route("/api/accounts", post(add_account))
        .route("/api/accounts/batch", post(batch_add_accounts))
        .route("/api/accounts/reload", post(reload_accounts))
        .route("/api/accounts/:id", get(get_account))
        .route("/api/accounts/:id", delete(delete_account))
        .route("/api/accounts/:id/refresh", post(refresh_account_token))
        .route("/api/accounts/:id/quota", get(get_account_quota))
        
        // 配置管理
        .route("/api/config", get(get_config))
        .route("/api/config", post(update_config))
        
        // 统计信息
        .route("/api/stats", get(get_stats))
}

/// API Key 认证中间件辅助函数
async fn verify_api_key(headers: &HeaderMap, state: &AppState) -> AppResult<()> {
    let config = state.config.read().await;
    let expected_key = &config.proxy.api_key;
    
    // 从 Authorization header 获取 key
    let auth_header = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    
    let provided_key = auth_header
        .strip_prefix("Bearer ")
        .or_else(|| auth_header.strip_prefix("bearer "))
        .unwrap_or(auth_header);
    
    if provided_key.is_empty() || provided_key != expected_key {
        return Err(AppError::Unauthorized("无效的 API Key".to_string()));
    }
    
    Ok(())
}

// ========== 账号管理 API ==========

/// 列出所有账号
async fn list_accounts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state).await {
        return e.into_response();
    }
    
    let accounts = state.account_service.list_summary().await;
    Json(json!({
        "accounts": accounts,
        "total": accounts.len(),
    })).into_response()
}

/// 获取单个账号
async fn get_account(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state).await {
        return e.into_response();
    }
    
    match state.account_service.get(&id).await {
        Some(account) => Json(account).into_response(),
        None => AppError::NotFound(format!("账号不存在: {}", id)).into_response(),
    }
}

/// 添加账号
async fn add_account(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<AddAccountRequest>,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state).await {
        return e.into_response();
    }
    
    match state.account_service.add(request).await {
        Ok(account) => {
            // 重新加载 Token 管理器
            let _ = state.token_manager.reload().await;
            (StatusCode::CREATED, Json(account)).into_response()
        }
        Err(e) => e.into_response(),
    }
}

/// 批量添加账号
async fn batch_add_accounts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<BatchAddAccountRequest>,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state).await {
        return e.into_response();
    }
    
    let mut result = BatchAddResult {
        success: 0,
        failed: 0,
        accounts: Vec::new(),
        errors: Vec::new(),
    };
    
    for refresh_token in request.refresh_tokens {
        let req = AddAccountRequest {
            refresh_token,
            email: None,
            name: None,
        };
        
        match state.account_service.add(req).await {
            Ok(account) => {
                result.success += 1;
                result.accounts.push(account);
            }
            Err(e) => {
                result.failed += 1;
                result.errors.push(e.to_string());
            }
        }
    }
    
    // 重新加载 Token 管理器
    let _ = state.token_manager.reload().await;
    
    Json(result).into_response()
}

/// 删除账号
async fn delete_account(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state).await {
        return e.into_response();
    }
    
    match state.account_service.delete(&id).await {
        Ok(_) => {
            let _ = state.token_manager.reload().await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => e.into_response(),
    }
}

/// 刷新账号 Token
async fn refresh_account_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state).await {
        return e.into_response();
    }
    
    match state.account_service.refresh_token(&id).await {
        Ok(account) => {
            let _ = state.token_manager.reload().await;
            Json(account).into_response()
        }
        Err(e) => e.into_response(),
    }
}

/// 重新加载账号
async fn reload_accounts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state).await {
        return e.into_response();
    }
    
    match state.reload_accounts().await {
        Ok(count) => Json(json!({
            "success": true,
            "accounts_loaded": count,
        })).into_response(),
        Err(e) => e.into_response(),
    }
}

/// 获取账号配额
async fn get_account_quota(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state).await {
        return e.into_response();
    }
    
    match state.account_service.get(&id).await {
        Some(account) => {
            if let Some(quota) = account.quota {
                Json(quota).into_response()
            } else {
                Json(json!({
                    "message": "配额未查询",
                })).into_response()
            }
        }
        None => AppError::NotFound(format!("账号不存在: {}", id)).into_response(),
    }
}

// ========== 配置管理 API ==========

/// 获取配置
async fn get_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state).await {
        return e.into_response();
    }
    
    let config = state.config.read().await;
    Json(json!({
        "server": {
            "port": config.server.port,
            "host": config.server.host,
        },
        "proxy": {
            "request_timeout": config.proxy.request_timeout,
            "model_mapping": config.proxy.model_mapping,
        },
    })).into_response()
}

/// 更新配置
async fn update_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(updates): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state).await {
        return e.into_response();
    }
    
    let mut config = state.config.write().await;
    
    // 更新模型映射
    if let Some(mapping) = updates.get("model_mapping") {
        if let Ok(m) = serde_json::from_value(mapping.clone()) {
            config.proxy.model_mapping = m;
        }
    }
    
    // 更新请求超时
    if let Some(timeout) = updates.get("request_timeout").and_then(|v| v.as_u64()) {
        config.proxy.request_timeout = timeout;
    }
    
    // 保存到文件
    if let Err(e) = config.save() {
        return AppError::Internal(format!("保存配置失败: {}", e)).into_response();
    }
    
    Json(json!({
        "success": true,
        "message": "配置已更新",
    })).into_response()
}

// ========== 统计 API ==========

/// 获取统计信息
async fn get_stats(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = verify_api_key(&headers, &state).await {
        return e.into_response();
    }
    
    let account_count = state.account_service.count().await;
    let token_count = state.token_manager.len();
    
    Json(json!({
        "accounts": {
            "total": account_count,
            "active": token_count,
        },
        "server": {
            "version": env!("CARGO_PKG_VERSION"),
            "uptime": "N/A", // 可以添加启动时间记录
        },
    })).into_response()
}
