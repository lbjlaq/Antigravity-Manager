use crate::proxy::server::AppState;
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};

pub async fn service_status_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();

    // 管理接口、OAuth 回调、健康检查和前端静态资源在停服后仍要可达（以便用户通过 Web 界面配置账号）。
    // /internal/ 只放行本机回环，避免停服后公网仍能打到预热端点。
    let internal_from_loopback = path.starts_with("/internal/")
        && crate::proxy::middleware::ip_filter::is_direct_loopback(&request);
    if path.starts_with("/api/")
        || internal_from_loopback
        || path == "/auth/callback"
        || path == "/health"
        || crate::proxy::middleware::is_static_asset_path(path)
    {
        return next.run(request).await;
    }

    let running = {
        let r = state.is_running.read().await;
        *r
    };

    if !running {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Proxy service is currently disabled".to_string(),
        )
            .into_response();
    }

    next.run(request).await
}
