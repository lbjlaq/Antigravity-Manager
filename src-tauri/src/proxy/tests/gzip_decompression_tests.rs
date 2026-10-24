//! Gzip Inbound Request Decompression Integration Tests
//! 验证网关 Inbound Gzip 解压缩功能及洋葱模型安全层级

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        extract::{Json, Request, State},
        http::{header, StatusCode},
        middleware::Next,
        response::Response,
        routing::post,
        Router,
    };
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use serde_json::{json, Value};
    use std::io::Write;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tower::ServiceExt;

    use crate::proxy::{middleware::auth::auth_middleware, ProxyAuthMode, ProxySecurityConfig};

    /// 辅助函数：将字符串压缩为 gzip 二进制字节数组
    fn compress_gzip(data: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    /// 模拟测试用 Monitor 中间件：记录收到的 request body 文本并提取 model
    #[derive(Clone, Default)]
    struct CapturedInfo {
        body_str: Arc<RwLock<Option<String>>>,
        extracted_model: Arc<RwLock<Option<String>>>,
    }

    async fn mock_monitor_middleware(
        State(captured): State<CapturedInfo>,
        request: Request,
        next: Next,
    ) -> Response {
        let (parts, body) = request.into_parts();
        let bytes = axum::body::to_bytes(body, 10 * 1024 * 1024).await.unwrap();

        let (body_str, model) = if let Ok(s) = std::str::from_utf8(&bytes) {
            let m = serde_json::from_slice::<Value>(&bytes).ok().and_then(|v| {
                v.get("model")
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string())
            });
            (Some(s.to_string()), m)
        } else {
            (Some("[Binary Request Data]".to_string()), None)
        };

        *captured.body_str.write().await = body_str;
        *captured.extracted_model.write().await = model;

        let request = Request::from_parts(parts, Body::from(bytes));
        next.run(request).await
    }

    /// 构建具有真实层级结构的测试 Router
    /// 请求顺序：auth -> decompression -> monitor -> handler
    fn build_test_app(
        security: Arc<RwLock<ProxySecurityConfig>>,
        captured: CapturedInfo,
    ) -> Router {
        Router::new()
            .route(
                "/v1/messages",
                post(|Json(payload): Json<Value>| async move {
                    let model = payload.get("model").and_then(|m| m.as_str()).unwrap_or("");
                    (StatusCode::OK, format!("Echo model: {}", model))
                }),
            )
            .layer(axum::middleware::from_fn_with_state(
                captured,
                mock_monitor_middleware,
            ))
            .layer(tower_http::decompression::RequestDecompressionLayer::new())
            .layer(axum::middleware::from_fn_with_state(
                security,
                auth_middleware,
            ))
    }

    #[tokio::test]
    async fn test_inbound_gzip_decompression_and_model_extraction() {
        let security = Arc::new(RwLock::new(ProxySecurityConfig {
            auth_mode: ProxyAuthMode::Strict,
            api_key: "sk-ant-test-key".to_string(),
            admin_password: None,
            allow_lan_access: true,
            port: 8045,
            security_monitor: crate::proxy::config::SecurityMonitorConfig::default(),
        }));

        let captured = CapturedInfo::default();
        let app = build_test_app(security, captured.clone());

        // 构造 Claude 请求体并进行 Gzip 压缩
        let raw_json = json!({
            "model": "claude-3-7-sonnet-20250219",
            "messages": [
                {"role": "user", "content": "Hello from gzipped Claude Code!"}
            ]
        })
        .to_string();
        let gzipped_bytes = compress_gzip(raw_json.as_bytes());

        // 发送带 Content-Encoding: gzip 的请求
        let req = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header(header::AUTHORIZATION, "Bearer sk-ant-test-key")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_ENCODING, "gzip")
            .body(Body::from(gzipped_bytes))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp_body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let resp_text = String::from_utf8(resp_body.to_vec()).unwrap();
        assert_eq!(resp_text, "Echo model: claude-3-7-sonnet-20250219");

        // 验证 Monitor 中间件成功捕获到了解压后的明文文本，而非 [Binary Request Data]
        let captured_str = captured.body_str.read().await.clone();
        assert!(captured_str.is_some());
        let body_str = captured_str.unwrap();
        assert!(body_str.contains("Hello from gzipped Claude Code!"));
        assert!(!body_str.contains("[Binary Request Data]"));

        // 验证 Monitor 成功提取出模型名称
        let extracted_model = captured.extracted_model.read().await.clone();
        assert_eq!(
            extracted_model,
            Some("claude-3-7-sonnet-20250219".to_string())
        );
    }

    #[tokio::test]
    async fn test_unauthenticated_gzip_request_rejected_before_decompression() {
        let _ = crate::modules::user_token_db::init_db();

        let security = Arc::new(RwLock::new(ProxySecurityConfig {
            auth_mode: ProxyAuthMode::Strict,
            api_key: "valid-key".to_string(),
            admin_password: None,
            allow_lan_access: true,
            port: 8045,
            security_monitor: crate::proxy::config::SecurityMonitorConfig::default(),
        }));

        let captured = CapturedInfo::default();
        let app = build_test_app(security, captured.clone());

        // 构造任意 Gzip 数据（包括可能的大体积解压缩炸弹数据）
        let gzipped_bytes = compress_gzip(b"{\"model\":\"decompression-bomb-or-attack\"}");

        // 1. 无凭证请求发送 -> 鉴权层在外层，应直接返回 401 Unauthorized
        let req_no_auth = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_ENCODING, "gzip")
            .body(Body::from(gzipped_bytes.clone()))
            .unwrap();

        let resp = app.clone().oneshot(req_no_auth).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // 验证解压层和监控层根本未被触发
        assert!(captured.body_str.read().await.is_none());
        assert!(captured.extracted_model.read().await.is_none());

        // 2. 携带无效 API Key 发送 -> 鉴权层拒绝 (403 Forbidden)
        let req_invalid_auth = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header(header::AUTHORIZATION, "Bearer invalid-wrong-key")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_ENCODING, "gzip")
            .body(Body::from(gzipped_bytes))
            .unwrap();

        let resp = app.oneshot(req_invalid_auth).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // 验证解压层和监控层依然未被触发
        assert!(captured.body_str.read().await.is_none());
        assert!(captured.extracted_model.read().await.is_none());
    }

    #[tokio::test]
    async fn test_plain_uncompressed_request_still_works_transparently() {
        let security = Arc::new(RwLock::new(ProxySecurityConfig {
            auth_mode: ProxyAuthMode::Strict,
            api_key: "sk-ant-test-key".to_string(),
            admin_password: None,
            allow_lan_access: true,
            port: 8045,
            security_monitor: crate::proxy::config::SecurityMonitorConfig::default(),
        }));

        let captured = CapturedInfo::default();
        let app = build_test_app(security, captured.clone());

        let raw_json = json!({
            "model": "claude-3-5-sonnet-20241022",
            "messages": [{"role": "user", "content": "plain text"}]
        })
        .to_string();

        let req = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header(header::AUTHORIZATION, "Bearer sk-ant-test-key")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(raw_json))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp_body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let resp_text = String::from_utf8(resp_body.to_vec()).unwrap();
        assert_eq!(resp_text, "Echo model: claude-3-5-sonnet-20241022");

        let captured_str = captured.body_str.read().await.clone().unwrap();
        assert!(captured_str.contains("plain text"));
        assert_eq!(
            captured.extracted_model.read().await.clone(),
            Some("claude-3-5-sonnet-20241022".to_string())
        );
    }
}
