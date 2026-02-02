// 上游客户端实现
// 基于高性能通讯接口封装

use reqwest::{header, Client, Response, StatusCode};
use serde_json::Value;
use tokio::time::Duration;
use tokio::sync::RwLock;

// Cloud Code v1internal endpoints (fallback order: Sandbox → Daily → Prod)
// 优先使用 Sandbox/Daily 环境以避免 Prod环境的 429/403 错误 (Ref: Issue #1176)
const V1_INTERNAL_BASE_URL_PROD: &str = "https://cloudcode-pa.googleapis.com/v1internal";
const V1_INTERNAL_BASE_URL_DAILY: &str = "https://daily-cloudcode-pa.googleapis.com/v1internal";
const V1_INTERNAL_BASE_URL_SANDBOX: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal";

const V1_INTERNAL_BASE_URL_FALLBACKS: [&str; 3] = [
    V1_INTERNAL_BASE_URL_SANDBOX, // 优先级 1: Sandbox (已知有效且稳定，避免 VALIDATION_REQUIRED)
    V1_INTERNAL_BASE_URL_DAILY,   // 优先级 2: Daily (备用)
    V1_INTERNAL_BASE_URL_PROD,    // 优先级 3: Prod (仅作为兜底)
];

pub struct UpstreamClient {
    http_client: RwLock<Client>,
    user_agent_override: RwLock<Option<String>>,
}

impl UpstreamClient {
    pub fn new(proxy_config: Option<crate::proxy::config::UpstreamProxyConfig>) -> Self {
        let client = Self::build_http_client(proxy_config);
        Self { 
            http_client: RwLock::new(client),
            user_agent_override: RwLock::new(None),
        }
    }

    /// [NEW] 设置动态 User-Agent 覆盖
    pub async fn set_user_agent_override(&self, ua: Option<String>) {
        let mut lock = self.user_agent_override.write().await;
        *lock = ua.clone();
        tracing::info!("UpstreamClient User-Agent override updated: {:?}", ua);
    }

    /// [NEW] 获取当前生效的 User-Agent
    async fn get_effective_user_agent(&self) -> String {
        let ua_override = self.user_agent_override.read().await;
        ua_override.as_ref().cloned().unwrap_or_else(|| crate::constants::USER_AGENT.clone())
    }

    /// [NEW] 重建并热更新内部 HTTP 客户端
    pub async fn rebuild_client(&self, proxy_config: Option<crate::proxy::config::UpstreamProxyConfig>) {
        let new_client = Self::build_http_client(proxy_config);
        let mut writer = self.http_client.write().await;
        *writer = new_client;
        tracing::info!("UpstreamClient underlying HTTP client has been reloaded");
    }

    /// 内部构建 HTTP Client 的逻辑
    fn build_http_client(proxy_config: Option<crate::proxy::config::UpstreamProxyConfig>) -> Client {
        // [PERF] Connection pool size configurable via env, default 64 for high-load scenarios
        let pool_size: usize = std::env::var("ABV_POOL_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(64);

        let mut builder = Client::builder()
            // Connection settings (optimized for high concurrency)
            .connect_timeout(Duration::from_secs(20))
            .pool_max_idle_per_host(pool_size)             // [PERF] Increased from 16 to 64
            .pool_idle_timeout(Duration::from_secs(90))    // Keep idle connections for 90s
            .tcp_keepalive(Duration::from_secs(60))        // TCP keepalive probe every 60s
            .timeout(Duration::from_secs(600))
            .user_agent(crate::constants::USER_AGENT.as_str());

        if let Some(config) = proxy_config {
            if config.enabled && !config.url.is_empty() {
                if let Ok(proxy) = reqwest::Proxy::all(&config.url) {
                    builder = builder.proxy(proxy);
                    tracing::info!("UpstreamClient enabled proxy: {}", config.url);
                }
            }
        }

        builder.build().expect("Failed to create HTTP client")
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
        self.call_v1_internal_with_headers(method, access_token, body, query_string, std::collections::HashMap::new()).await
    }

    /// [FIX #765] 调用 v1internal API，支持透传额外的 Headers
    pub async fn call_v1_internal_with_headers(
        &self,
        method: &str,
        access_token: &str,
        body: Value,
        query_string: Option<&str>,
        extra_headers: std::collections::HashMap<String, String>,
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
        
        // [NEW] Используем динамический User-Agent с поддержкой override
        let effective_ua = self.get_effective_user_agent().await;
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_str(&effective_ua)
                .unwrap_or_else(|e| {
                    tracing::warn!("Invalid User-Agent header value, using fallback: {}", e);
                    header::HeaderValue::from_static("antigravity")
                }),
        );

        // 注入额外的 Headers (如 anthropic-beta)
        for (k, v) in extra_headers {
            if let Ok(hk) = header::HeaderName::from_bytes(k.as_bytes()) {
                if let Ok(hv) = header::HeaderValue::from_str(&v) {
                    headers.insert(hk, hv);
                }
            }
        }

        let mut last_err: Option<String> = None;

        // 获取 Client 读锁
        let client_guard = self.http_client.read().await;

        // 遍历所有端点，失败时自动切换
        for (idx, base_url) in V1_INTERNAL_BASE_URL_FALLBACKS.iter().enumerate() {
            let url = Self::build_url(base_url, method, query_string);
            let has_next = idx + 1 < V1_INTERNAL_BASE_URL_FALLBACKS.len();

            let response = client_guard
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
                                "✓ Upstream fallback succeeded | Endpoint: {} | Status: {} | Attempt: {}/{}",
                                base_url,
                                status,
                                idx + 1,
                                V1_INTERNAL_BASE_URL_FALLBACKS.len()
                            );
                        } else {
                            tracing::debug!("✓ Upstream request succeeded | Endpoint: {} | Status: {}", base_url, status);
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

    /// 获取可用模型列表
    /// 
    /// 获取远端模型列表，支持多端点自动 Fallback
    #[allow(dead_code)] // API ready for future model discovery feature
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
        
        // [NEW] Используем динамический User-Agent с поддержкой override
        let effective_ua = self.get_effective_user_agent().await;
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_str(&effective_ua)
                .unwrap_or_else(|e| {
                    tracing::warn!("Invalid User-Agent header value, using fallback: {}", e);
                    header::HeaderValue::from_static("antigravity")
                }),
        );

        let mut last_err: Option<String> = None;
        let client_guard = self.http_client.read().await;

        // 遍历所有端点，失败时自动切换
        for (idx, base_url) in V1_INTERNAL_BASE_URL_FALLBACKS.iter().enumerate() {
            let url = Self::build_url(base_url, "fetchAvailableModels", None);

            let response = client_guard
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
                            tracing::debug!("✓ fetchAvailableModels succeeded | Endpoint: {}", base_url);
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
