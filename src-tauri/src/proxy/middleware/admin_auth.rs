// Admin认证中间件
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{Response, IntoResponse},
    http::{StatusCode, HeaderMap},
    Json,
};
use serde_json::json;

use crate::proxy::server::AppState;

/// Admin面板认证中间件
///
/// 验证方式：
/// 1. Cookie: admin_token=<api_key>
/// 2. Header: Authorization: Bearer <api_key>
/// 3. Query: ?token=<api_key>
pub async fn admin_auth_middleware(
    State(_state): State<AppState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // 加载配置获取API Key
    let config = match crate::modules::config::load_app_config() {
        Ok(cfg) => cfg,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let expected_key = &config.proxy.api_key;
    let mut authenticated = false;

    // 方式1: 检查Cookie
    if let Some(cookie_header) = headers.get("cookie") {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for cookie in cookie_str.split(';') {
                let parts: Vec<&str> = cookie.trim().split('=').collect();
                if parts.len() == 2 && parts[0] == "admin_token" {
                    if parts[1] == expected_key {
                        authenticated = true;
                        break;
                    }
                }
            }
        }
    }

    // 方式2: 检查Authorization Header
    if !authenticated {
        if let Some(auth_header) = headers.get("authorization") {
            if let Ok(auth_str) = auth_header.to_str() {
                if let Some(token) = auth_str.strip_prefix("Bearer ") {
                    if token == expected_key {
                        authenticated = true;
                    }
                }
            }
        }
    }

    // 方式3: 检查Query参数
    if !authenticated {
        let uri = request.uri();
        if let Some(query) = uri.query() {
            for param in query.split('&') {
                let parts: Vec<&str> = param.split('=').collect();
                if parts.len() == 2 && parts[0] == "token" {
                    if parts[1] == expected_key {
                        authenticated = true;
                        break;
                    }
                }
            }
        }
    }

    if authenticated {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// Admin登录处理
pub async fn admin_login(
    State(_state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    // 验证API Key
    let config = crate::modules::config::load_app_config()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if payload.api_key != config.proxy.api_key {
        return Ok((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "success": false,
                "message": "Invalid API Key"
            }))
        ));
    }

    // 返回成功，前端会存储token
    Ok((
        StatusCode::OK,
        Json(json!({
            "success": true,
            "token": config.proxy.api_key,
            "message": "Login successful"
        }))
    ))
}

#[derive(serde::Deserialize)]
pub struct LoginRequest {
    api_key: String,
}
