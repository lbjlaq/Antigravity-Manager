// 统计追踪中间件
use axum::{
    extract::Request,
    middleware::Next,
    response::Response,
};
use std::time::Instant;

/// 请求统计追踪中间件
pub async fn stats_middleware(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();

    // 只统计 /v1 代理 API 请求
    let should_track = path.starts_with("/v1");

    let start = Instant::now();
    let response = next.run(request).await;

    if should_track {
        let duration_ms = start.elapsed().as_millis() as u64;
        let status = response.status().as_u16();
        let success = status >= 200 && status < 400;

        let stats = crate::proxy::admin::global_stats();
        stats.record_request(success, duration_ms).await;
    }

    response
}
