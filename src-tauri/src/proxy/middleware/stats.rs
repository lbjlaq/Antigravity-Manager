// 统计追踪中间件
use axum::{
    extract::Request,
    middleware::Next,
    response::Response,
};
use std::time::Instant;

/// 请求统计追踪中间件
pub async fn stats_middleware(request: Request, next: Next) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_string();

    // 跳过 admin 相关路径的统计
    if path.starts_with("/admin") || path.starts_with("/api/admin") {
        return next.run(request).await;
    }

    let start = Instant::now();
    let response = next.run(request).await;
    let duration_ms = start.elapsed().as_millis() as u64;

    let status = response.status().as_u16();
    let success = status >= 200 && status < 400;

    // 记录统计
    let stats = crate::proxy::admin::global_stats();
    stats.record_request(success, duration_ms).await;

    // 发送日志
    crate::proxy::admin::emit_proxy_log(&method, &path, status, duration_ms, None);

    response
}
