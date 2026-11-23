use crate::proxy::server::AppState;
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};

pub fn is_service_status_bypassed(path: &str, is_direct_loopback: bool) -> bool {
    // 管理接口、OAuth 回调和健康检查在停服后仍要可达（以便用户通过 Web 界面配置账号）。
    // 前端静态资源由外层 fallback_service 提供，不经过本中间件，因此不得按路径后缀放行，
    // 否则 `/v1beta/models/x.js` 之类的路由会在停服后绕过状态检查。
    // /internal/ 只放行本机回环，避免停服后公网仍能打到预热端点。
    let internal_from_loopback = path.starts_with("/internal/") && is_direct_loopback;
    path.starts_with("/api/")
        || internal_from_loopback
        || path == "/auth/callback"
        || path == "/health"
        || path == "/healthz"
}

pub async fn service_status_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    let is_loopback = crate::proxy::middleware::ip_filter::is_direct_loopback(&request);

    if is_service_status_bypassed(path, is_loopback) {
        return next.run(request).await;
    }

    let running = {
        let r = state.is_running.read().await;
        *r
    };

    if !running {
        tracing::warn!(
            path = %request.uri().path(),
            "Proxy service is currently disabled; returning 503 (service_status, not upstream)"
        );
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Proxy service is currently disabled".to_string(),
        )
            .into_response();
    }

    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_status_bypass_paths() {
        assert!(is_service_status_bypassed("/health", false));
        assert!(is_service_status_bypassed("/healthz", false));
        assert!(is_service_status_bypassed("/api/accounts", false));
        assert!(is_service_status_bypassed("/auth/callback", false));
        assert!(!is_service_status_bypassed("/v1/chat/completions", false));
        assert!(!is_service_status_bypassed("/v1/models", false));
        assert!(is_service_status_bypassed("/internal/warmup", true));
        assert!(!is_service_status_bypassed("/internal/warmup", false));
    }

    #[test]
    fn test_service_status_does_not_bypass_static_suffixes() {
        // 静态资源由外层 fallback_service 托管；后缀不得让代理/管理路由绕过停服检查
        for path in [
            "/v1beta/models/gemini-x.js",
            "/v1/chat/completions.css",
            "/auth/callback.css",
            "/assets/index.js",
            "/favicon.ico",
        ] {
            assert!(!is_service_status_bypassed(path, false), "{path}");
            assert!(!is_service_status_bypassed(path, true), "{path}");
        }
    }
}
