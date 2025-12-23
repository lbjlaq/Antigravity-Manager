//! HTTP 客户端工厂

use std::time::Duration;
use reqwest::Client;

/// 创建 HTTP 客户端
pub fn create_http_client(timeout_secs: u64) -> Client {
    create_http_client_with_proxy(timeout_secs, None)
}

/// 创建带代理的 HTTP 客户端
pub fn create_http_client_with_proxy(timeout_secs: u64, proxy_url: Option<&str>) -> Client {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .connect_timeout(Duration::from_secs(30))
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(10);

    // 配置代理
    if let Some(proxy_str) = proxy_url {
        if !proxy_str.is_empty() {
            if let Ok(proxy) = reqwest::Proxy::all(proxy_str) {
                builder = builder.proxy(proxy);
                tracing::info!("HTTP 客户端使用代理: {}", proxy_str);
            } else {
                tracing::warn!("无效的代理配置: {}", proxy_str);
            }
        }
    }

    builder.build().unwrap_or_else(|_| Client::new())
}
