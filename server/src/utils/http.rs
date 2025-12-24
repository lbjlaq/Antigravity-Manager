use reqwest::{Client, Proxy};

/// 创建统一配置的 HTTP 客户端
pub fn create_client(timeout_secs: u64) -> Client {
    // 简化版，不自动加载配置，由调用方传入
    create_client_with_proxy(timeout_secs, None)
}

/// 创建带指定代理配置的 HTTP 客户端
pub fn create_client_with_proxy(
    timeout_secs: u64, 
    proxy_config: Option<crate::modules::proxy::config::UpstreamProxyConfig>
) -> Client {
    let mut builder = Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs));

    if let Some(config) = proxy_config {
        if config.enabled && !config.url.is_empty() {
            match Proxy::all(&config.url) {
                Ok(proxy) => {
                    builder = builder.proxy(proxy);
                    tracing::info!("HTTP 客户端已启用上游代理: {}", config.url);
                }
                Err(e) => {
                    tracing::error!("无效的代理地址: {}, 错误: {}", config.url, e);
                }
            }
        }
    }

    builder.build().unwrap_or_else(|_| Client::new())
}
