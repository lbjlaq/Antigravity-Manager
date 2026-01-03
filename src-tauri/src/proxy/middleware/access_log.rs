use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use std::time::Instant;

use crate::proxy::server::AppState;

pub async fn access_log_middleware(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let enabled = { *state.access_log_enabled.read().await };
    if !enabled {
        return next.run(request).await;
    }

    let start = Instant::now();
    let method = request.method().to_string();
    let path = request.uri().path().to_string();

    let response = next.run(request).await;
    let status = response.status().as_u16();
    let duration_ms = start.elapsed().as_millis() as u64;

    tracing::info!("[Access] {} {} {} {}ms", method, path, status, duration_ms);
    response
}

