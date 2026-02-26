use crate::modules::config::load_app_config;
use once_cell::sync::Lazy;
use rquest::{Client, Proxy};
use rquest_util::Emulation;

/// Global shared HTTP client (15s timeout)
/// Client has a built-in connection pool; cloning it is light and shares the pool
pub static SHARED_CLIENT: Lazy<Client> = Lazy::new(|| create_base_client(15));

/// Global shared HTTP client (Long timeout: 60s, for warmup etc.)
pub static SHARED_CLIENT_LONG: Lazy<Client> = Lazy::new(|| create_base_client(60));

/// Global shared standard HTTP client (15s timeout, NO JA3 Emulation)
pub static SHARED_STANDARD_CLIENT: Lazy<Client> = Lazy::new(|| create_standard_client(15));

/// Global shared standard HTTP client (Long timeout: 60s, NO JA3 Emulation)
pub static SHARED_STANDARD_CLIENT_LONG: Lazy<Client> = Lazy::new(|| create_standard_client(60));

/// Base client creation logic with JA3 Emulation
fn create_base_client(timeout_secs: u64) -> Client {
    let mut builder = Client::builder()
        .emulation(Emulation::Chrome123)
        .timeout(std::time::Duration::from_secs(timeout_secs));

    if let Ok(config) = load_app_config() {
        let proxy_config = config.proxy.upstream_proxy;
        if proxy_config.enabled && !proxy_config.url.is_empty() {
            match Proxy::all(&proxy_config.url) {
                Ok(proxy) => {
                    builder = builder.proxy(proxy);
                    tracing::info!(
                        "HTTP shared client enabled upstream proxy: {}",
                        proxy_config.url
                    );
                }
                Err(e) => {
                    tracing::error!("invalid_proxy_url: {}, error: {}", proxy_config.url, e);
                }
            }
        }
    }

    tracing::info!("Initialized JA3/TLS Impersonation (Chrome123)");
    builder.build().unwrap_or_else(|_| Client::new())
}

/// Get uniformly configured HTTP client (15s timeout)
pub fn get_client() -> Client {
    SHARED_CLIENT.clone()
}

/// Get long timeout HTTP client (60s timeout)
pub fn get_long_client() -> Client {
    SHARED_CLIENT_LONG.clone()
}

/// Base client creation logic strictly WITHOUT JA3 Emulation (Pure Native)
fn create_standard_client(timeout_secs: u64) -> Client {
    let mut builder = Client::builder()
        // No .emulation(Emulation::Chrome123) here!
        .timeout(std::time::Duration::from_secs(timeout_secs));

    if let Ok(config) = load_app_config() {
        let proxy_config = config.proxy.upstream_proxy;
        if proxy_config.enabled && !proxy_config.url.is_empty() {
            match Proxy::all(&proxy_config.url) {
                Ok(proxy) => {
                    builder = builder.proxy(proxy);
                    tracing::info!(
                        "HTTP standard client enabled upstream proxy: {}",
                        proxy_config.url
                    );
                }
                Err(e) => {
                    tracing::error!("invalid_proxy_url: {}, error: {}", proxy_config.url, e);
                }
            }
        }
    }

    tracing::info!("Initialized Pure Native Standard Client");
    builder.build().unwrap_or_else(|_| Client::new())
}

/// Get standard HTTP client without JA3 Emulation (15s timeout)
pub fn get_standard_client() -> Client {
    SHARED_STANDARD_CLIENT.clone()
}

/// Get long timeout standard HTTP client without JA3 Emulation (60s timeout)
pub fn get_long_standard_client() -> Client {
    SHARED_STANDARD_CLIENT_LONG.clone()
}
