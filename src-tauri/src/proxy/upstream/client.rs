// 上游客户端实现
// 基于高性能通讯接口封装

use reqwest::{header, Client, Response, StatusCode};
use serde_json::Value;
use tokio::time::Duration;

use crate::proxy::common::platform;

// Cloud Code v1internal endpoints (fallback order: prod → daily)
// 优先使用稳定的 prod 端点，避免影响缓存命中率
const V1_INTERNAL_BASE_URL_PROD: &str = "https://cloudcode-pa.googleapis.com/v1internal";
const V1_INTERNAL_BASE_URL_DAILY: &str =
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal";
const V1_INTERNAL_BASE_URL_FALLBACKS: [&str; 2] = [
    V1_INTERNAL_BASE_URL_PROD,  // 优先使用生产环境（稳定）
    V1_INTERNAL_BASE_URL_DAILY, // 备用测试环境（新功能）
];

pub struct UpstreamClient {
    http_client: Client,
}

impl UpstreamClient {
    pub fn new(proxy_config: Option<crate::proxy::config::UpstreamProxyConfig>) -> Self {
        // [DEBUG] Allow overriding user agent via environment variable for testing
        // Otherwise, dynamically detect platform/arch for anti-detection compliance
        let user_agent =
            std::env::var("CLAUDE_USER_AGENT").unwrap_or_else(|_| platform::build_user_agent());

        // GAP #5: Validation logging for User-Agent formation
        tracing::info!("🔧 UpstreamClient User-Agent: {}", user_agent);
        tracing::debug!(
            "[Epic-004-Validation] User-Agent: '{}' (platform: {}, arch: {})",
            user_agent,
            platform::get_platform(),
            platform::get_architecture()
        );

        let mut builder = Client::builder()
            // Connection settings (优化连接复用，减少建立开销)
            .connect_timeout(Duration::from_secs(20))
            .pool_max_idle_per_host(16) // 每主机最多 16 个空闲连接
            .pool_idle_timeout(Duration::from_secs(90)) // 空闲连接保持 90 秒
            .tcp_keepalive(Duration::from_secs(60)) // TCP 保活探测 60 秒
            .timeout(Duration::from_secs(600))
            .user_agent(user_agent);

        if let Some(config) = proxy_config {
            if config.enabled && !config.url.is_empty() {
                if let Ok(proxy) = reqwest::Proxy::all(&config.url) {
                    builder = builder.proxy(proxy);
                    tracing::info!("UpstreamClient enabled proxy: {}", config.url);
                }
            }
        }

        let http_client = builder.build().expect("Failed to create HTTP client");

        Self { http_client }
    }

    /// 构建 v1internal URL
    ///
    /// 构建 API 请求地址
    fn build_url(base_url: &str, method: &str, query_string: Option<&str>) -> String {
        if let Some(qs) = query_string {
            format!("{}:{}?{}", base_url, method, qs)
        } else {
            format!("{}:{}", base_url, method)
        }
    }

    /// 判断是否应尝试下一个端点
    ///
    /// 当遇到以下错误时，尝试切换到备用端点：
    /// - 429 Too Many Requests（限流）
    /// - 408 Request Timeout（超时）
    /// - 404 Not Found（端点不存在）
    /// - 5xx Server Error（服务器错误）
    fn should_try_next_endpoint(status: StatusCode) -> bool {
        status == StatusCode::TOO_MANY_REQUESTS
            || status == StatusCode::REQUEST_TIMEOUT
            || status == StatusCode::NOT_FOUND
            || status.is_server_error()
    }

    /// 调用 v1internal API（基础方法）
    ///
    /// 发起基础网络请求，支持多端点自动 Fallback
    pub async fn call_v1_internal(
        &self,
        method: &str,
        access_token: &str,
        body: Value,
        query_string: Option<&str>,
    ) -> Result<Response, String> {
        // 构建 Headers (所有端点复用)
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {}", access_token))
                .map_err(|e| e.to_string())?,
        );

        // [DEBUG] Allow overriding user agent via environment variable
        // Updated to 1.13.3 to match current Google Antigravity version
        let user_agent = std::env::var("CLAUDE_USER_AGENT")
            .unwrap_or_else(|_| "antigravity/1.13.3 darwin/arm64".to_string());
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_str(&user_agent).map_err(|e| e.to_string())?,
        );

        // [DEBUG] Add additional headers for testing
        if let Ok(test_variant) = std::env::var("X_TEST_VARIANT") {
            headers.insert(
                header::HeaderName::from_static("x-test-variant"),
                header::HeaderValue::from_str(&test_variant)
                    .unwrap_or_else(|_| header::HeaderValue::from_static("unknown")),
            );
        }

        let mut last_err: Option<String> = None;

        // 遍历所有端点，失败时自动切换
        for (idx, base_url) in V1_INTERNAL_BASE_URL_FALLBACKS.iter().enumerate() {
            let url = Self::build_url(base_url, method, query_string);
            let has_next = idx + 1 < V1_INTERNAL_BASE_URL_FALLBACKS.len();

            // [DEBUG] Log full request details before sending
            tracing::error!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            tracing::error!("🔍 UPSTREAM REQUEST DEBUG");
            tracing::error!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            tracing::error!("📍 URL: {}", url);
            tracing::error!("🔧 Method: POST");
            tracing::error!("📋 Headers:");
            for (key, value) in headers.iter() {
                if let Ok(v) = value.to_str() {
                    if key.as_str().to_lowercase() == "authorization" {
                        let masked = if v.len() > 20 {
                            format!("Bearer {}...{}", &v[7..17], &v[v.len() - 4..])
                        } else {
                            "Bearer ***".to_string()
                        };
                        tracing::error!("  {}: {}", key, masked);
                    } else {
                        tracing::error!("  {}: {}", key, v);
                    }
                }
            }
            tracing::error!("📦 Body Structure:");
            if let Some(obj) = body.as_object() {
                tracing::error!("  project: {:?}", obj.get("project"));
                tracing::error!("  requestId: {:?}", obj.get("requestId"));
                tracing::error!("  model: {:?}", obj.get("model"));
                tracing::error!("  userAgent: {:?}", obj.get("userAgent"));
                tracing::error!("  requestType: {:?}", obj.get("requestType"));
                if let Some(req) = obj.get("request").and_then(|r| r.as_object()) {
                    tracing::error!(
                        "  request.contents.len: {:?}",
                        req.get("contents")
                            .and_then(|c| c.as_array())
                            .map(|a| a.len())
                    );
                    tracing::error!(
                        "  request.systemInstruction: {}",
                        req.contains_key("systemInstruction")
                    );
                    tracing::error!(
                        "  request.generationConfig: {}",
                        req.contains_key("generationConfig")
                    );
                    tracing::error!("  request.tools: {}", req.contains_key("tools"));
                }
            }
            tracing::error!("📄 Full Body (first 500 chars):");
            let body_str = serde_json::to_string_pretty(&body).unwrap_or_default();
            let preview = if body_str.len() > 500 {
                // Find safe UTF-8 character boundary
                let mut end = 500.min(body_str.len());
                while end > 0 && !body_str.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}[...]", &body_str[..end])
            } else {
                body_str
            };
            tracing::error!("{}", preview);
            tracing::error!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

            let response = self
                .http_client
                .post(&url)
                .headers(headers.clone())
                .json(&body)
                .send()
                .await;

            match response {
                Ok(resp) => {
                    let status = resp.status();

                    // [DEBUG] Log response details
                    tracing::error!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                    tracing::error!("📥 UPSTREAM RESPONSE DEBUG");
                    tracing::error!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                    tracing::error!("🔢 Status: {}", status);
                    tracing::error!("📋 Response Headers:");
                    for (key, value) in resp.headers().iter() {
                        if let Ok(v) = value.to_str() {
                            tracing::error!("  {}: {}", key, v);
                        }
                    }

                    // [DEBUG] For 429 errors, capture error body
                    if status == StatusCode::TOO_MANY_REQUESTS {
                        tracing::error!("❌❌❌ 429 RATE LIMIT ERROR DETECTED! ❌❌❌");
                        // Try to read error body
                        let body_bytes = resp.bytes().await.unwrap_or_default();
                        let body_str = String::from_utf8_lossy(&body_bytes);
                        tracing::error!("📄 Error Body: {}", body_str);
                        tracing::error!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

                        // Return error since we consumed body
                        return Err(format!("429 Rate Limit: {}", body_str));
                    }

                    // [DEBUG] For 400 errors, capture error body
                    if status == StatusCode::BAD_REQUEST {
                        tracing::error!("❌❌❌ 400 BAD REQUEST ERROR DETECTED! ❌❌❌");
                        // Try to read error body
                        let body_bytes = resp.bytes().await.unwrap_or_default();
                        let body_str = String::from_utf8_lossy(&body_bytes);
                        tracing::error!("📄 Error Body: {}", body_str);
                        tracing::error!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

                        // Return error since we consumed body
                        return Err(format!("400 Bad Request: {}", body_str));
                    }
                    tracing::error!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

                    if status.is_success() {
                        if idx > 0 {
                            tracing::info!(
                                "✓ Upstream fallback succeeded | Endpoint: {} | Status: {} | Attempt: {}/{}",
                                base_url,
                                status,
                                idx + 1,
                                V1_INTERNAL_BASE_URL_FALLBACKS.len()
                            );

                            // [Epic-005-Retry] Location 3: Retry success after endpoint failover
                            tracing::info!(
                                target: "retry_event",
                                "[Epic-005-Retry] Retry succeeded on attempt {}: {} (endpoint: {})",
                                idx + 1,
                                status,
                                base_url
                            );
                        } else {
                            tracing::debug!(
                                "✓ Upstream request succeeded | Endpoint: {} | Status: {}",
                                base_url,
                                status
                            );
                        }
                        return Ok(resp);
                    }

                    // 如果有下一个端点且当前错误可重试，则切换
                    if has_next && Self::should_try_next_endpoint(status) {
                        tracing::warn!(
                            "Upstream endpoint returned {} at {} (method={}), trying next endpoint",
                            status,
                            base_url,
                            method
                        );

                        // [Epic-005-Retry] Location 4: HTTP retry with endpoint failover
                        tracing::warn!(
                            target: "retry_event",
                            "[Epic-005-Retry] HTTP retry attempt {}/{}: {} (switching endpoint: {} → {})",
                            idx + 1,
                            V1_INTERNAL_BASE_URL_FALLBACKS.len(),
                            status,
                            base_url,
                            V1_INTERNAL_BASE_URL_FALLBACKS.get(idx + 1).unwrap_or(&"<none>")
                        );

                        last_err = Some(format!("Upstream {} returned {}", base_url, status));
                        continue;
                    }

                    // 不可重试的错误或已是最后一个端点，直接返回
                    return Ok(resp);
                }
                Err(e) => {
                    let msg = format!("HTTP request failed at {}: {}", base_url, e);
                    tracing::debug!("{}", msg);
                    last_err = Some(msg);

                    // 如果是最后一个端点，退出循环
                    if !has_next {
                        break;
                    }
                    continue;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| "All endpoints failed".to_string()))
    }

    // 调用 v1internal API（带 429 重试,支持闭包）
    //
    // 带容错和重试的核心请求逻辑
    //
    // # Arguments
    // * `method` - API method (e.g., "generateContent")
    // * `query_string` - Optional query string (e.g., "?alt=sse")
    // * `get_credentials` - 闭包，获取凭证（支持账号轮换）
    // * `build_body` - 闭包，接收 project_id 构建请求体
    // * `max_attempts` - 最大重试次数
    //
    // # Returns
    // HTTP Response
    // 已移除弃用的重试方法 (call_v1_internal_with_retry)
    // 已移除弃用的辅助方法 (parse_retry_delay)
    // 已移除弃用的辅助方法 (parse_duration_ms)

    /// 获取可用模型列表
    ///
    /// 获取远端模型列表，支持多端点自动 Fallback
    pub async fn fetch_available_models(&self, access_token: &str) -> Result<Value, String> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {}", access_token))
                .map_err(|e| e.to_string())?,
        );
        // Updated to 1.13.3 to match current Google Antigravity version
        let user_agent = std::env::var("CLAUDE_USER_AGENT")
            .unwrap_or_else(|_| "antigravity/1.13.3 darwin/arm64".to_string());
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_str(&user_agent)
                .map_err(|e| format!("Invalid user agent: {}", e))?,
        );

        let mut last_err: Option<String> = None;

        // 遍历所有端点，失败时自动切换
        for (idx, base_url) in V1_INTERNAL_BASE_URL_FALLBACKS.iter().enumerate() {
            let url = Self::build_url(base_url, "fetchAvailableModels", None);

            let response = self
                .http_client
                .post(&url)
                .headers(headers.clone())
                .json(&serde_json::json!({}))
                .send()
                .await;

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        if idx > 0 {
                            tracing::info!(
                                "✓ Upstream fallback succeeded for fetchAvailableModels | Endpoint: {} | Status: {}",
                                base_url,
                                status
                            );
                        } else {
                            tracing::debug!(
                                "✓ fetchAvailableModels succeeded | Endpoint: {}",
                                base_url
                            );
                        }
                        let json: Value = resp
                            .json()
                            .await
                            .map_err(|e| format!("Parse json failed: {}", e))?;
                        return Ok(json);
                    }

                    // 如果有下一个端点且当前错误可重试，则切换
                    let has_next = idx + 1 < V1_INTERNAL_BASE_URL_FALLBACKS.len();
                    if has_next && Self::should_try_next_endpoint(status) {
                        tracing::warn!(
                            "fetchAvailableModels returned {} at {}, trying next endpoint",
                            status,
                            base_url
                        );
                        last_err = Some(format!("Upstream error: {}", status));
                        continue;
                    }

                    // 不可重试的错误或已是最后一个端点
                    return Err(format!("Upstream error: {}", status));
                }
                Err(e) => {
                    let msg = format!("Request failed at {}: {}", base_url, e);
                    tracing::debug!("{}", msg);
                    last_err = Some(msg);

                    // 如果是最后一个端点，退出循环
                    if idx + 1 >= V1_INTERNAL_BASE_URL_FALLBACKS.len() {
                        break;
                    }
                    continue;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| "All endpoints failed".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_url() {
        let base_url = "https://cloudcode-pa.googleapis.com/v1internal";

        let url1 = UpstreamClient::build_url(base_url, "generateContent", None);
        assert_eq!(
            url1,
            "https://cloudcode-pa.googleapis.com/v1internal:generateContent"
        );

        let url2 = UpstreamClient::build_url(base_url, "streamGenerateContent", Some("alt=sse"));
        assert_eq!(
            url2,
            "https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse"
        );
    }
}
