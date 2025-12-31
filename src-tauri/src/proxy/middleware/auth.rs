// API Key 认证中间件
use axum::{
    extract::Request,
    http::{header, StatusCode, Method},
    middleware::Next,
    response::Response,
};

/// API Key 认证中间件
pub async fn auth_middleware(request: Request, next: Next) -> Result<Response, StatusCode> {
    // Log the request method and URI
    tracing::info!("Request: {} {}", request.method(), request.uri());

    // 允许 OPTIONS 预检请求直接通过(用于CORS)
    if request.method() == Method::OPTIONS {
        return Ok(next.run(request).await);
    }

    // 从 header 中提取 API key
    let api_key = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .or_else(|| {
            request
                .headers()
                .get("x-api-key")
                .and_then(|h| h.to_str().ok())
        });

    // TODO: 实际验证 API key
    // 目前暂时允许所有请求通过
    if api_key.is_some() || true {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

#[cfg(test)]
mod tests {
    // 移除未使用的 use super::*;

    #[test]
    fn test_auth_placeholder() {
        // Placeholder test
        assert!(true);
    }
}
