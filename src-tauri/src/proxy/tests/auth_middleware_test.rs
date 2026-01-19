#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        middleware::from_fn_with_state,
        routing::get,
        Router,
    };
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tower::util::ServiceExt; // for oneshot
    use crate::proxy::{middleware::auth::auth_middleware, ProxySecurityConfig};

    // Helper to setup the app with the auth middleware
    fn setup_app(api_key: &str) -> Router {
        let config = ProxySecurityConfig {
            api_key: api_key.to_string(),
            auth_mode: crate::proxy::ProxyAuthMode::Strict,
            ..Default::default()
        };
        let state = Arc::new(RwLock::new(config));

        Router::new()
            .route("/", get(|| async { "OK" }))
            .route("/healthz", get(|| async { "OK" }))
            .layer(from_fn_with_state(state, auth_middleware))
    }

    #[tokio::test]
    async fn test_auth_success_with_header() {
        let app = setup_app("valid-key");

        let req = Request::builder()
            .uri("/")
            .header("Authorization", "Bearer valid-key")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auth_success_with_x_api_key() {
        let app = setup_app("valid-key");

        let req = Request::builder()
            .uri("/")
            .header("x-api-key", "valid-key")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auth_failure_no_header() {
        let app = setup_app("valid-key");

        let req = Request::builder()
            .uri("/")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_failure_invalid_key() {
        let app = setup_app("valid-key");

        let req = Request::builder()
            .uri("/")
            .header("Authorization", "Bearer wrong-key")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_bypass_for_healthz_endpoint() {
        use crate::proxy::ProxyAuthMode;

        // Auto mode with allow_lan_access=true resolves to AllExceptHealth
        let config = ProxySecurityConfig {
            api_key: "valid-key".to_string(),
            auth_mode: ProxyAuthMode::Auto,
            allow_lan_access: true, 
        };
        let state = Arc::new(RwLock::new(config));

        let app = Router::new()
            .route("/healthz", get(|| async { "OK" }))
            .layer(from_fn_with_state(state, auth_middleware));

        let req = Request::builder()
            .uri("/healthz")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
