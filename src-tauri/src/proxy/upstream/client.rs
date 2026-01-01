// 上游客户端实现
// 基于高性能通讯接口封装

use reqwest::{header, Client, Response, StatusCode};
use serde_json::Value;
use tokio::time::Duration;

// Cloud Code v1internal endpoints (fallback order: daily -> prod)
const V1_INTERNAL_BASE_URL_DAILY: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal";
const V1_INTERNAL_BASE_URL_PROD: &str = "https://cloudcode-pa.googleapis.com/v1internal";
const V1_INTERNAL_BASE_URL_FALLBACKS: [&str; 2] = [V1_INTERNAL_BASE_URL_DAILY, V1_INTERNAL_BASE_URL_PROD];

pub struct UpstreamClient {
    http_client: Client,
}

impl UpstreamClient {
    pub fn new(proxy_config: Option<crate::proxy::config::UpstreamProxyConfig>) -> Self {
        let mut builder = Client::builder()
            // Connection settings (WSL/Windows benefit from stable keepalive + connection reuse).
            .connect_timeout(Duration::from_secs(20))
            .pool_max_idle_per_host(16)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .timeout(Duration::from_secs(600))
            .user_agent("antigravity/1.11.9 windows/amd64");

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

    fn should_try_next_endpoint(status: StatusCode) -> bool {
        status == StatusCode::TOO_MANY_REQUESTS
            || status == StatusCode::REQUEST_TIMEOUT
            || status == StatusCode::NOT_FOUND
            || status.is_server_error()
    }

    /// 调用 v1internal API（基础方法）
    /// 
    /// 发起基础网络请求
    pub async fn call_v1_internal(
        &self,
        method: &str,
        access_token: &str,
        body: Value,
        query_string: Option<&str>,
    ) -> Result<Response, String> {
        // 构建 Headers (reused across endpoint attempts)
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
        // 设置自定义 User-Agent
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_static("antigravity/1.11.9 windows/amd64"),
        );

        let mut last_err: Option<String> = None;

        for (idx, base_url) in V1_INTERNAL_BASE_URL_FALLBACKS.iter().enumerate() {
            let url = Self::build_url(base_url, method, query_string);
            let has_next = idx + 1 < V1_INTERNAL_BASE_URL_FALLBACKS.len();

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
                    if status.is_success() {
                        if idx > 0 {
                            tracing::info!(
                                "Upstream endpoint fallback succeeded: {} (status={})",
                                base_url,
                                status
                            );
                        }
                        return Ok(resp);
                    }

                    if has_next && Self::should_try_next_endpoint(status) {
                        tracing::warn!(
                            "Upstream endpoint returned {} at {} (method={}), trying next if available",
                            status,
                            base_url,
                            method
                        );
                        last_err = Some(format!("Upstream {} returned {}", base_url, status));
                        continue;
                    }

                    return Ok(resp);
                }
                Err(e) => {
                    let msg = format!("HTTP request failed at {}: {}", base_url, e);
                    tracing::warn!("{}", msg);
                    last_err = Some(msg);

                    // On the last endpoint, surface the error.
                    if idx + 1 >= V1_INTERNAL_BASE_URL_FALLBACKS.len() {
                        break;
                    }
                    continue;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| "All endpoints failed".to_string()))
    }

    /// 调用 v1internal API（带 429 重试,支持闭包）
    /// 
    /// 带容错和重试的核心请求逻辑
    /// 
    /// # Arguments
    /// * `method` - API method (e.g., "generateContent")
    /// * `query_string` - Optional query string (e.g., "?alt=sse")
    /// * `get_credentials` - 闭包，获取凭证（支持账号轮换）
    /// * `build_body` - 闭包，接收 project_id 构建请求体
    /// * `max_attempts` - 最大重试次数
    /// 
    /// # Returns
    /// HTTP Response
    // 已移除弃用的重试方法 (call_v1_internal_with_retry)

    // 已移除弃用的辅助方法 (parse_retry_delay)

    // 已移除弃用的辅助方法 (parse_duration_ms)

    /// 获取可用模型列表
    /// 
    /// 获取远端模型列表
    pub async fn fetch_available_models(&self, access_token: &str) -> Result<Value, String> {
        let mut headers = header::HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, header::HeaderValue::from_static("application/json"));
        headers.insert(header::AUTHORIZATION, header::HeaderValue::from_str(&format!("Bearer {}", access_token)).map_err(|e| e.to_string())?);
        headers.insert(header::USER_AGENT, header::HeaderValue::from_static("antigravity/1.11.9 windows/amd64"));

        let mut last_err: Option<String> = None;

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
                                "Upstream endpoint fallback succeeded for fetchAvailableModels: {}",
                                base_url
                            );
                        }
                        let json: Value = resp
                            .json()
                            .await
                            .map_err(|e| format!("Parse json failed: {}", e))?;
                        return Ok(json);
                    }

                    if Self::should_try_next_endpoint(status) {
                        tracing::warn!(
                            "fetchAvailableModels failed at {} (status={}); trying next",
                            base_url,
                            status
                        );
                        last_err = Some(format!("Upstream {} returned {}", base_url, status));
                        continue;
                    }

                    return Err(format!("Upstream error: {}", status));
                }
                Err(e) => {
                    let msg = format!("Request failed at {}: {}", base_url, e);
                    tracing::warn!("{}", msg);
                    last_err = Some(msg);
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
        let url1 = UpstreamClient::build_url(V1_INTERNAL_BASE_URL_PROD, "generateContent", None);
        assert_eq!(
            url1,
            "https://cloudcode-pa.googleapis.com/v1internal:generateContent"
        );

        let url2 = UpstreamClient::build_url(
            V1_INTERNAL_BASE_URL_PROD,
            "streamGenerateContent",
            Some("alt=sse"),
        );
        assert_eq!(
            url2,
            "https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn test_should_try_next_endpoint() {
        assert!(UpstreamClient::should_try_next_endpoint(
            StatusCode::TOO_MANY_REQUESTS
        ));
        assert!(UpstreamClient::should_try_next_endpoint(StatusCode::BAD_GATEWAY));
        assert!(UpstreamClient::should_try_next_endpoint(
            StatusCode::SERVICE_UNAVAILABLE
        ));
        assert!(!UpstreamClient::should_try_next_endpoint(StatusCode::BAD_REQUEST));
        assert!(!UpstreamClient::should_try_next_endpoint(StatusCode::UNAUTHORIZED));
        assert!(!UpstreamClient::should_try_next_endpoint(StatusCode::FORBIDDEN));
        assert!(UpstreamClient::should_try_next_endpoint(StatusCode::NOT_FOUND));
    }

}
