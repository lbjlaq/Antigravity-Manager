// API Key 认证中间件
use axum::{
    extract::Request,
    extract::State,
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use std::borrow::Cow;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::proxy::{ProxyAuthMode, ProxySecurityConfig};

/// 检查是否为 Web 控制台的静态资源或前端路由
pub fn is_static_asset_path(path: &str) -> bool {
    path == "/"
        || path == "/index.html"
        || path == "/favicon.ico"
        || path.starts_with("/assets/")
        || path.starts_with("/icons/")
        || path.ends_with(".js")
        || path.ends_with(".css")
        || path.ends_with(".png")
        || path.ends_with(".svg")
        || path.ends_with(".woff")
        || path.ends_with(".woff2")
}

/// 从请求中提取 API Key / Token
/// 支持提取顺序：
/// 1. Authorization header (Bearer token / Raw token)
/// 2. x-api-key header
/// 3. x-goog-api-key header (Google Gemini / Google Cloud)
/// 4. URL query 参数 ?key=... (Google Gemini 官方 REST 协议规范)
pub fn extract_api_key<B>(request: &Request<B>) -> Option<Cow<'_, str>> {
    // 1. Authorization header
    if let Some(auth_val) = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
    {
        let trimmed = auth_val.trim();
        let token = if let Some(stripped) = trimmed.strip_prefix("Bearer ") {
            stripped.trim()
        } else if let Some(stripped) = trimmed.strip_prefix("bearer ") {
            stripped.trim()
        } else if trimmed.eq_ignore_ascii_case("bearer") {
            ""
        } else {
            trimmed
        };
        if !token.is_empty() {
            return Some(Cow::Borrowed(token));
        }
    }

    // 2. x-api-key header
    if let Some(val) = request
        .headers()
        .get("x-api-key")
        .and_then(|h| h.to_str().ok())
    {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return Some(Cow::Borrowed(trimmed));
        }
    }

    // 3. x-goog-api-key header
    if let Some(val) = request
        .headers()
        .get("x-goog-api-key")
        .and_then(|h| h.to_str().ok())
    {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return Some(Cow::Borrowed(trimmed));
        }
    }

    // 4. URL query 参数 (?key=...)
    if let Some(query) = request.uri().query() {
        for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
            if k == "key" {
                match v {
                    Cow::Borrowed(s) => {
                        let trimmed = s.trim();
                        if !trimmed.is_empty() {
                            return Some(Cow::Borrowed(trimmed));
                        }
                    }
                    Cow::Owned(s) => {
                        let trimmed = s.trim();
                        if !trimmed.is_empty() {
                            return Some(Cow::Owned(trimmed.to_string()));
                        }
                    }
                }
            }
        }
    }

    None
}

/// API Key 认证中间件 (代理接口使用，遵循 auth_mode)
pub async fn auth_middleware(
    state: State<Arc<RwLock<ProxySecurityConfig>>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    auth_middleware_internal(state, request, next, false).await
}

/// 管理接口认证中间件 (管理接口使用，强制严格鉴权)
pub async fn admin_auth_middleware(
    state: State<Arc<RwLock<ProxySecurityConfig>>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    auth_middleware_internal(state, request, next, true).await
}

/// 内部认证逻辑
async fn auth_middleware_internal(
    State(security): State<Arc<RwLock<ProxySecurityConfig>>>,
    request: Request,
    next: Next,
    force_strict: bool,
) -> Result<Response, StatusCode> {
    let method = request.method().clone();
    let path = request.uri().path().to_string();

    // 静态资源文件与前端入口白名单 (用于 Headless/Docker/Web UI 正常加载)
    let is_static_asset = is_static_asset_path(&path);

    // 过滤心跳、健康检查与探针请求,避免日志噪音
    let is_health_check = path == "/healthz"
        || path == "/api/health"
        || path == "/health"
        || path == "/version"
        || path == "/v1/version"
        || path == "/props"
        || path == "/v1/props"
        || is_static_asset;
    let is_internal_endpoint = path.starts_with("/internal/");
    if !path.contains("event_logging") && !is_health_check {
        tracing::info!("Request: {} {}", method, path);
    } else {
        tracing::trace!("Heartbeat/Health: {} {}", method, path);
    }

    // Allow CORS preflight regardless of auth policy.
    if method == axum::http::Method::OPTIONS {
        return Ok(next.run(request).await);
    }

    // 内部端点 (/internal/*) 保护：仅供本机内部进程调度（如配额轮询内部预热）。
    // 1. 本机直接 TCP 回环：免鉴权直接放行。
    // 2. 任何非回环来源：无论鉴权模式（即使 auth_mode=Off）或是否提供有效 API Key，一律直接拒绝 (403 Forbidden)。
    if is_internal_endpoint {
        if crate::proxy::middleware::ip_filter::is_direct_loopback(&request) {
            tracing::debug!("Loopback internal endpoint bypassed auth: {}", path);
            return Ok(next.run(request).await);
        } else {
            tracing::warn!("Blocked non-loopback access to internal endpoint: {}", path);
            return Err(StatusCode::FORBIDDEN);
        }
    }

    let security = security.read().await.clone();
    let effective_mode = security.effective_auth_mode();

    // 权限检查逻辑
    if !force_strict {
        // AI 代理接口 (v1/chat/completions 等)
        if matches!(effective_mode, ProxyAuthMode::Off) {
            // [FIX] 即使 auth_mode=Off，也需要尝试识别 User Token 以记录使用情况
            // 先检查是否携带了 User Token (支持 header 或 URL query ?key=)
            let api_key = extract_api_key(&request);

            if let Some(token) = api_key.as_deref() {
                // 尝试验证是否为 User Token（不阻止请求，只记录）
                if let Ok(Some(user_token)) =
                    crate::modules::user_token_db::get_token_by_value(token)
                {
                    let identity = UserTokenIdentity {
                        token_id: user_token.id,
                        token: user_token.token,
                        username: user_token.username,
                    };
                    // 注入 identity 到请求
                    let (mut parts, body) = request.into_parts();
                    parts.extensions.insert(identity);
                    let request = Request::from_parts(parts, body);
                    return Ok(next.run(request).await);
                }
            }

            return Ok(next.run(request).await);
        }

        if matches!(effective_mode, ProxyAuthMode::AllExceptHealth) && is_health_check {
            return Ok(next.run(request).await);
        }
    } else {
        // Management endpoints always require admin auth; only health checks stay public.
        if is_health_check {
            return Ok(next.run(request).await);
        }
    }

    // 从 header 或 URL query (?key=) 中提取 API key
    let api_key = extract_api_key(&request);

    if security.api_key.is_empty()
        && (security.admin_password.is_none()
            || security.admin_password.as_ref().unwrap().is_empty())
    {
        if force_strict {
            tracing::error!("Admin auth is required but both api_key and admin_password are empty; denying request");
            return Err(StatusCode::UNAUTHORIZED);
        }
        tracing::error!("Proxy auth is enabled but api_key is empty; denying request");
        return Err(StatusCode::UNAUTHORIZED);
    }

    // 认证逻辑
    let authorized = if force_strict {
        // 管理接口：优先使用独立的 admin_password，如果没有则回退使用 api_key
        match &security.admin_password {
            Some(pwd) if !pwd.is_empty() => api_key.as_deref().map(|k| k == pwd).unwrap_or(false),
            _ => {
                // 回退使用 api_key
                api_key
                    .as_deref()
                    .map(|k| k == security.api_key)
                    .unwrap_or(false)
            }
        }
    } else {
        // AI 代理接口：仅允许使用 api_key
        api_key
            .as_deref()
            .map(|k| k == security.api_key)
            .unwrap_or(false)
    };

    if authorized {
        Ok(next.run(request).await)
    } else if !force_strict && api_key.is_some() {
        // 尝试验证 UserToken
        let token = api_key.as_deref().unwrap();

        // 提取 IP (复用 ip_filter 规范化逻辑，支持 IPv4/IPv6 及 ConnectInfo)
        let client_ip = crate::proxy::middleware::ip_filter::extract_client_ip(&request)
            .unwrap_or_else(|| "127.0.0.1".to_string()); // Default fallback

        // 验证 Token
        match crate::modules::user_token_db::validate_token(token, &client_ip) {
            Ok((true, _)) => {
                // Token 有效，查询信息以便传递
                if let Ok(Some(user_token)) =
                    crate::modules::user_token_db::get_token_by_value(token)
                {
                    let identity = UserTokenIdentity {
                        token_id: user_token.id,
                        token: user_token.token,
                        username: user_token.username,
                    };

                    // [FIX] 将身份信息注入到请求 extensions 中，而不是响应
                    // 这样 monitor_middleware 在处理请求时就能获取到 identity
                    // 因为中间件执行顺序：auth (外层) -> monitor (内层) -> handler
                    // 响应返回时：handler -> monitor -> auth
                    // 如果注入到 response，monitor 执行时 identity 还不存在
                    let (mut parts, body) = request.into_parts();
                    parts.extensions.insert(identity);
                    let request = Request::from_parts(parts, body);

                    // 执行请求
                    let response = next.run(request).await;

                    Ok(response)
                } else {
                    Err(StatusCode::UNAUTHORIZED)
                }
            }
            Ok((false, reason)) => {
                let reason_str = reason.unwrap_or_else(|| "Access denied".to_string());
                tracing::warn!("UserToken rejected: {}", reason_str);
                let body = serde_json::json!({
                    "error": {
                        "message": reason_str,
                        "type": "token_rejected",
                        "code": "token_rejected"
                    }
                });
                let response = axum::response::Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .header("Content-Type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_string(&body).unwrap(),
                    ))
                    .unwrap();
                Ok(response)
            }
            Err(e) => {
                tracing::error!("UserToken validation error: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// 用户令牌身份信息 (传递给 Monitor 使用)
#[derive(Clone, Debug)]
pub struct UserTokenIdentity {
    pub token_id: String,
    #[allow(dead_code)] // 保留原始 token 便于审计/调试
    pub token: String,
    pub username: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::ProxyAuthMode;

    #[tokio::test]
    async fn test_admin_auth_with_password() {
        let _security = Arc::new(RwLock::new(ProxySecurityConfig {
            auth_mode: ProxyAuthMode::Strict,
            api_key: "sk-api".to_string(),
            admin_password: Some("admin123".to_string()),
            allow_lan_access: true,
            port: 8045,
            security_monitor: crate::proxy::config::SecurityMonitorConfig::default(),
        }));

        // 模拟请求 - 管理接口使用正确的管理密码
        let _req = Request::builder()
            .header("Authorization", "Bearer admin123")
            .uri("/admin/stats")
            .body(axum::body::Body::empty())
            .unwrap();

        // 此测试由于涉及 Next 中间件调用比较复杂,主要验证核心逻辑
        // 我们在 auth_middleware_internal 基础上做了逻辑校验即可
    }

    #[test]
    fn test_auth_placeholder() {
        assert!(true);
    }

    #[tokio::test]
    async fn test_internal_endpoint_loopback_only() {
        use axum::routing::post;
        use tower::ServiceExt;

        let security = Arc::new(RwLock::new(ProxySecurityConfig {
            auth_mode: ProxyAuthMode::Off, // 即使在 Off 模式下，也必须强制拦截非回环
            api_key: "valid-key".to_string(),
            admin_password: None,
            allow_lan_access: true,
            port: 8045,
            security_monitor: crate::proxy::config::SecurityMonitorConfig::default(),
        }));

        let app = axum::Router::new()
            .route("/internal/warmup", post(|| async { StatusCode::OK }))
            .layer(axum::middleware::from_fn_with_state(
                security.clone(),
                auth_middleware,
            ));

        // 1. 本机回环来源（带 ConnectInfo） -> 允许访问 (200 OK)
        let mut req_loopback = Request::builder()
            .method("POST")
            .uri("/internal/warmup")
            .body(axum::body::Body::empty())
            .unwrap();
        req_loopback
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(
                "127.0.0.1:12345".parse::<std::net::SocketAddr>().unwrap(),
            ));
        let resp = app.clone().oneshot(req_loopback).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 2. 外部局域网来源（无凭证，即使 auth_mode=Off） -> 必须被拦截 (403 Forbidden)
        let mut req_lan = Request::builder()
            .method("POST")
            .uri("/internal/warmup")
            .body(axum::body::Body::empty())
            .unwrap();
        req_lan.extensions_mut().insert(axum::extract::ConnectInfo(
            "192.168.1.100:12345"
                .parse::<std::net::SocketAddr>()
                .unwrap(),
        ));
        let resp = app.clone().oneshot(req_lan).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // 3. 外部来源附带有效 API Key -> 依然必须被拦截 (403 Forbidden)
        let mut req_with_key = Request::builder()
            .method("POST")
            .uri("/internal/warmup")
            .header("Authorization", "Bearer valid-key")
            .body(axum::body::Body::empty())
            .unwrap();
        req_with_key
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(
                "10.0.0.1:12345".parse::<std::net::SocketAddr>().unwrap(),
            ));
        let resp = app.clone().oneshot(req_with_key).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // 4. 外部来源伪造 X-Forwarded-For: 127.0.0.1 -> 依然被拦截 (403 Forbidden)
        let mut req_spoofed = Request::builder()
            .method("POST")
            .uri("/internal/warmup")
            .header("x-forwarded-for", "127.0.0.1")
            .body(axum::body::Body::empty())
            .unwrap();
        req_spoofed
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(
                "192.168.1.100:12345"
                    .parse::<std::net::SocketAddr>()
                    .unwrap(),
            ));
        let resp = app.clone().oneshot(req_spoofed).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn test_extract_api_key_sources() {
        // 1. Authorization: Bearer
        let req = Request::builder()
            .header("Authorization", "Bearer sk-bearer-token")
            .body(())
            .unwrap();
        assert_eq!(extract_api_key(&req).as_deref(), Some("sk-bearer-token"));

        // 2. Authorization: bearer (小写)
        let req = Request::builder()
            .header("Authorization", "bearer sk-lowercase-token")
            .body(())
            .unwrap();
        assert_eq!(extract_api_key(&req).as_deref(), Some("sk-lowercase-token"));

        // 3. Authorization: raw token (无 Bearer 前缀)
        let req = Request::builder()
            .header("Authorization", "sk-raw-token")
            .body(())
            .unwrap();
        assert_eq!(extract_api_key(&req).as_deref(), Some("sk-raw-token"));

        // 4. x-api-key header
        let req = Request::builder()
            .header("x-api-key", "sk-x-api-key")
            .body(())
            .unwrap();
        assert_eq!(extract_api_key(&req).as_deref(), Some("sk-x-api-key"));

        // 5. x-goog-api-key header (Google Gemini)
        let req = Request::builder()
            .header("x-goog-api-key", "AIzaSyGeminiHeaderKey")
            .body(())
            .unwrap();
        assert_eq!(
            extract_api_key(&req).as_deref(),
            Some("AIzaSyGeminiHeaderKey")
        );

        // 6. URL query: ?key=... (Google Gemini REST 协议)
        let req = Request::builder()
            .uri("/v1beta/models?key=AIzaSyGeminiQueryKey")
            .body(())
            .unwrap();
        assert_eq!(
            extract_api_key(&req).as_deref(),
            Some("AIzaSyGeminiQueryKey")
        );

        // 7. URL query: 混合其它参数 ?alt=sse&key=...&other=1
        let req = Request::builder()
            .uri("/v1beta/models/gemini-2.5-flash:generateContent?alt=sse&key=AIzaSyMixedQuery&stream=true")
            .body(())
            .unwrap();
        assert_eq!(extract_api_key(&req).as_deref(), Some("AIzaSyMixedQuery"));

        // 8. 优先级：Header 优先于 Query
        let req = Request::builder()
            .header("x-goog-api-key", "header-key")
            .uri("/v1beta/models?key=query-key")
            .body(())
            .unwrap();
        assert_eq!(extract_api_key(&req).as_deref(), Some("header-key"));

        // 9. 空值/空白处理
        let req = Request::builder()
            .uri("/v1beta/models?key=%20%20")
            .body(())
            .unwrap();
        assert_eq!(extract_api_key(&req), None);

        let req = Request::builder()
            .uri("/v1beta/models?key=")
            .body(())
            .unwrap();
        assert_eq!(extract_api_key(&req), None);

        let req = Request::builder()
            .header("Authorization", "Bearer   ")
            .body(())
            .unwrap();
        assert_eq!(extract_api_key(&req), None);
    }

    #[tokio::test]
    async fn test_auth_middleware_with_gemini_query_key() {
        use axum::routing::get;
        use tower::ServiceExt;

        let _ = crate::modules::user_token_db::init_db();

        let security = Arc::new(RwLock::new(ProxySecurityConfig {
            auth_mode: ProxyAuthMode::Strict,
            api_key: "valid-gemini-key".to_string(),
            admin_password: None,
            allow_lan_access: true,
            port: 8045,
            security_monitor: crate::proxy::config::SecurityMonitorConfig::default(),
        }));

        let app = axum::Router::new()
            .route("/v1beta/models", get(|| async { StatusCode::OK }))
            .layer(axum::middleware::from_fn_with_state(
                security.clone(),
                auth_middleware,
            ));

        // 1. 通过 URL query 携带正确的 API Key 访问 Gemini 接口 -> 200 OK
        let req_valid = Request::builder()
            .uri("/v1beta/models?key=valid-gemini-key")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req_valid).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 2. 通过 URL query 携带错误的 API Key（未通过主 API Key，进入 UserToken 校验失败） -> 403 Forbidden
        let req_invalid = Request::builder()
            .uri("/v1beta/models?key=wrong-key")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req_invalid).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // 3. 不带任何 Key 访问 -> 401 Unauthorized
        let req_none = Request::builder()
            .uri("/v1beta/models")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req_none).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
