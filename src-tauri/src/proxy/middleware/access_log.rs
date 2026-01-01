use axum::{
    extract::State,
    extract::Request,
    middleware::Next,
    response::Response,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;

/// Lightweight access log middleware (method/path/status/latency).
/// - Does not log query strings.
/// - Does not log headers or bodies (avoids leaking secrets).
pub async fn access_log_middleware(
    State(enabled): State<Arc<AtomicBool>>,
    request: Request,
    next: Next,
) -> Response {
    if !enabled.load(Ordering::Relaxed) {
        return next.run(request).await;
    }

    // Skip CORS preflight noise even when access logging is enabled.
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }

    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let start = Instant::now();

    let response = next.run(request).await;
    let status = response.status().as_u16();
    let latency_ms = start.elapsed().as_millis();

    // The handler may attach an extension for upstream routing info.
    let upstream = response
        .extensions()
        .get::<crate::proxy::observability::UpstreamRoute>()
        .map(|r| r.0)
        .unwrap_or("unknown");

    tracing::info!(
        target: "antigravity_tools_lib::proxy::access",
        method = %method,
        path = %path,
        status = status,
        latency_ms = latency_ms,
        upstream = upstream,
        "access"
    );

    response
}
