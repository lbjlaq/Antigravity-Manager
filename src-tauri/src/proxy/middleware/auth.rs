// API Key 认证中间件
use axum::{
    extract::State,
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::proxy::{ProxyAuthMode, ProxySecurityConfig};

/// API Key 认证中间件
pub async fn auth_middleware(
    State(security): State<Arc<RwLock<ProxySecurityConfig>>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let method = request.method().clone();
    let path = request.uri().path().to_string();

    // 过滤心跳和健康检查请求,避免日志噪音
    if !path.contains("event_logging") && path != "/healthz" {
        tracing::info!("Request: {} {}", method, path);
    } else {
        tracing::trace!("Heartbeat: {} {}", method, path);
    }

    // Allow CORS preflight regardless of auth policy.
    if method == axum::http::Method::OPTIONS {
        return Ok(next.run(request).await);
    }

    let security = security.read().await.clone();
    let effective_mode = security.effective_auth_mode();

    if matches!(effective_mode, ProxyAuthMode::Off) {
        return Ok(next.run(request).await);
    }

    if matches!(effective_mode, ProxyAuthMode::AllExceptHealth) && (path == "/healthz" || path == "/health") {
        return Ok(next.run(request).await);
    }
    
    // 从 header 中提取 API key
    let api_key = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer ").or(Some(s)))
        .or_else(|| {
            request
                .headers()
                .get("x-api-key")
                .and_then(|h| h.to_str().ok())
        });

    if security.api_key.is_empty() {
        tracing::error!("Proxy auth is enabled but api_key is empty; denying request");
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Constant-time compare is unnecessary here, but keep strict equality and avoid leaking values.
    let authorized = api_key.map(|k| k == security.api_key).unwrap_or(false);

    if authorized {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::any, Router};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn test_app(security: ProxySecurityConfig) -> Router {
        let state = Arc::new(RwLock::new(security));
        Router::new()
            .route("/health", any(|| async { "ok" }))
            .route("/healthz", any(|| async { "ok" }))
            .route("/v1/messages", any(|| async { "ok" }))
            .route("/v1/api/event_logging", any(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(state, auth_middleware))
    }

    async fn call(app: Router, method: axum::http::Method, path: &str, headers: Vec<(&str, &str)>) -> StatusCode {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let mut req = Request::builder().method(method).uri(path);
        for (k, v) in headers {
            req = req.header(k, v);
        }
        let req = req.body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        resp.status()
    }

    #[tokio::test]
    async fn off_mode_allows_everything() {
        let app = test_app(ProxySecurityConfig {
            auth_mode: ProxyAuthMode::Off,
            api_key: "sk-test".to_string(),
            allow_lan_access: false,
        });

        assert_eq!(call(app.clone(), axum::http::Method::GET, "/health", vec![]).await, StatusCode::OK);
        assert_eq!(call(app.clone(), axum::http::Method::GET, "/healthz", vec![]).await, StatusCode::OK);
        assert_eq!(call(app, axum::http::Method::POST, "/v1/messages", vec![]).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn strict_mode_requires_auth_everywhere() {
        let key = "sk-test";
        let app = test_app(ProxySecurityConfig {
            auth_mode: ProxyAuthMode::Strict,
            api_key: key.to_string(),
            allow_lan_access: false,
        });

        assert_eq!(call(app.clone(), axum::http::Method::GET, "/health", vec![]).await, StatusCode::UNAUTHORIZED);
        assert_eq!(call(app.clone(), axum::http::Method::GET, "/healthz", vec![]).await, StatusCode::UNAUTHORIZED);
        assert_eq!(call(app.clone(), axum::http::Method::POST, "/v1/messages", vec![]).await, StatusCode::UNAUTHORIZED);

        assert_eq!(
            call(
                app.clone(),
                axum::http::Method::POST,
                "/v1/messages",
                vec![(header::AUTHORIZATION.as_str(), &format!("Bearer {}", key))],
            )
            .await,
            StatusCode::OK
        );

        assert_eq!(
            call(app.clone(), axum::http::Method::POST, "/v1/messages", vec![(header::AUTHORIZATION.as_str(), key)]).await,
            StatusCode::OK
        );

        assert_eq!(
            call(app, axum::http::Method::POST, "/v1/messages", vec![("x-api-key", key)]).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn all_except_health_leaves_health_open() {
        let key = "sk-test";
        let app = test_app(ProxySecurityConfig {
            auth_mode: ProxyAuthMode::AllExceptHealth,
            api_key: key.to_string(),
            allow_lan_access: false,
        });

        assert_eq!(call(app.clone(), axum::http::Method::GET, "/health", vec![]).await, StatusCode::OK);
        assert_eq!(call(app.clone(), axum::http::Method::GET, "/healthz", vec![]).await, StatusCode::OK);
        // Health stays open even if a wrong auth header is present.
        assert_eq!(
            call(
                app.clone(),
                axum::http::Method::GET,
                "/healthz",
                vec![(header::AUTHORIZATION.as_str(), "Bearer sk-wrong")],
            )
            .await,
            StatusCode::OK
        );
        assert_eq!(
            call(
                app.clone(),
                axum::http::Method::GET,
                "/health",
                vec![(header::AUTHORIZATION.as_str(), "Bearer sk-wrong")],
            )
            .await,
            StatusCode::OK
        );
        assert_eq!(call(app.clone(), axum::http::Method::POST, "/v1/messages", vec![]).await, StatusCode::UNAUTHORIZED);
        assert_eq!(
            call(
                app,
                axum::http::Method::POST,
                "/v1/messages",
                vec![(header::AUTHORIZATION.as_str(), &format!("Bearer {}", key))],
            )
            .await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn auto_mode_depends_on_lan_flag() {
        let key = "sk-test";

        let app_local = test_app(ProxySecurityConfig {
            auth_mode: ProxyAuthMode::Auto,
            api_key: key.to_string(),
            allow_lan_access: false,
        });
        assert_eq!(call(app_local.clone(), axum::http::Method::GET, "/health", vec![]).await, StatusCode::OK);
        assert_eq!(call(app_local, axum::http::Method::POST, "/v1/messages", vec![]).await, StatusCode::OK);

        let app_lan = test_app(ProxySecurityConfig {
            auth_mode: ProxyAuthMode::Auto,
            api_key: key.to_string(),
            allow_lan_access: true,
        });
        assert_eq!(call(app_lan.clone(), axum::http::Method::GET, "/health", vec![]).await, StatusCode::OK);
        assert_eq!(call(app_lan.clone(), axum::http::Method::GET, "/healthz", vec![]).await, StatusCode::OK);
        // Health stays open in auto(lan) even if a wrong auth header is present.
        assert_eq!(
            call(
                app_lan.clone(),
                axum::http::Method::GET,
                "/healthz",
                vec![(header::AUTHORIZATION.as_str(), "Bearer sk-wrong")],
            )
            .await,
            StatusCode::OK
        );
        assert_eq!(
            call(
                app_lan.clone(),
                axum::http::Method::GET,
                "/health",
                vec![(header::AUTHORIZATION.as_str(), "Bearer sk-wrong")],
            )
            .await,
            StatusCode::OK
        );
        assert_eq!(call(app_lan, axum::http::Method::POST, "/v1/messages", vec![]).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn options_is_allowed_without_auth() {
        let app = test_app(ProxySecurityConfig {
            auth_mode: ProxyAuthMode::Strict,
            api_key: "sk-test".to_string(),
            allow_lan_access: false,
        });

        assert_eq!(
            call(app, axum::http::Method::OPTIONS, "/v1/messages", vec![]).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn enabled_but_empty_api_key_denies_all_non_health_bypass() {
        let app = test_app(ProxySecurityConfig {
            auth_mode: ProxyAuthMode::Strict,
            api_key: "".to_string(),
            allow_lan_access: false,
        });

        assert_eq!(
            call(
                app,
                axum::http::Method::POST,
                "/v1/messages",
                vec![(header::AUTHORIZATION.as_str(), "Bearer sk-any")],
            )
            .await,
            StatusCode::UNAUTHORIZED
        );
    }
}
