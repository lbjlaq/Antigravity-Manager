//! 健康检查路由

use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;

/// 健康检查
pub async fn health_check() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}
