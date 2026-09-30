use crate::modules::config::load_app_config;
use once_cell::sync::Lazy;
use rquest::{Client, Proxy};
use rquest_util::Emulation;
use std::collections::HashMap;
use std::sync::RwLock;

/// 共享客户端的身份：决定超时长度与是否启用 JA3/TLS 仿真。
///
/// 每个组合只构建一次并长期复用（`Client` 自带连接池，clone 很轻且共享同一个池）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ClientKey {
    timeout_secs: u64,
    emulated: bool,
}

impl ClientKey {
    const fn new(timeout_secs: u64, emulated: bool) -> Self {
        Self {
            timeout_secs,
            emulated,
        }
    }
}

struct SharedClients {
    built: HashMap<ClientKey, Client>,
}

impl SharedClients {
    fn new() -> Self {
        Self {
            built: HashMap::new(),
        }
    }

    fn get(&mut self, key: ClientKey) -> Client {
        if let Some(client) = self.built.get(&key) {
            return client.clone();
        }
        let client = if key.emulated {
            create_base_client(key.timeout_secs)
        } else {
            create_standard_client(key.timeout_secs)
        };
        self.built.insert(key, client.clone());
        client
    }
}

static SHARED_CLIENTS: Lazy<RwLock<SharedClients>> =
    Lazy::new(|| RwLock::new(SharedClients::new()));

fn acquire(key: ClientKey) -> Client {
    let mut guard = SHARED_CLIENTS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.get(key)
}

/// 丢弃所有已构建的共享客户端，令其在下次访问时按当前配置重建。
///
/// **为什么必须存在**：`create_base_client` / `create_standard_client` 只在**构建时**
/// 读取一次 `load_app_config().proxy.upstream_proxy`。这些客户端此前是 `Lazy` 静态值 ——
/// 首次使用即定型、永不重建，于是用户在运行期间改动上游代理后，
/// 主请求路径已热更新（`UpstreamClient::rebuild_default_client`），
/// 但这些共享客户端仍带着**旧代理**继续跑：表现为「改完代理，聊天正常，
/// 但 token 刷新 / 配额刷新 / 项目解析（loadCodeAssist）一直失败」，
/// 必须重启应用才能恢复。
///
/// 调用点：`AxumServer::update_proxy`（上游代理热更新）。
/// 采用惰性重建 —— 若之后无人使用，不付出任何构建成本。
pub fn invalidate_shared_clients() {
    let mut guard = SHARED_CLIENTS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let dropped = guard.built.len();
    guard.built.clear();
    if dropped > 0 {
        tracing::info!(
            dropped,
            "Discarded shared HTTP clients after upstream proxy change; rebuild on next use"
        );
    }
}

/// 测试用：列出当前已构建的客户端 key。
#[cfg(test)]
fn shared_client_keys() -> Vec<ClientKey> {
    let guard = SHARED_CLIENTS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.built.keys().copied().collect()
}

/// 统一的 HTTP 客户端（15s 超时，带 JA3 仿真）
pub fn get_client() -> Client {
    acquire(ClientKey::new(15, true))
}

/// 标准 HTTP 客户端（15s 超时，**不带** JA3 仿真）
pub fn get_standard_client() -> Client {
    acquire(ClientKey::new(15, false))
}

/// 长超时的标准客户端（60s，**不带** JA3 仿真）
pub fn get_long_standard_client() -> Client {
    acquire(ClientKey::new(60, false))
}

/// 应用当前上游代理配置到 builder 上。
fn apply_upstream_proxy(builder: rquest::ClientBuilder, label: &str) -> rquest::ClientBuilder {
    if let Ok(config) = load_app_config() {
        let proxy_config = config.proxy.upstream_proxy;
        if proxy_config.enabled && !proxy_config.url.is_empty() {
            match Proxy::all(&proxy_config.url) {
                Ok(proxy) => {
                    tracing::info!("{} enabled upstream proxy: {}", label, proxy_config.url);
                    return builder.proxy(proxy);
                }
                Err(e) => {
                    tracing::error!("invalid_proxy_url: {}, error: {}", proxy_config.url, e);
                }
            }
        }
    }
    builder
}

/// Base client creation logic with JA3 Emulation
fn create_base_client(timeout_secs: u64) -> Client {
    let builder = Client::builder()
        .emulation(Emulation::Chrome123)
        .timeout(std::time::Duration::from_secs(timeout_secs));
    let builder = apply_upstream_proxy(builder, "HTTP shared client");

    tracing::info!("Initialized JA3/TLS Impersonation (Chrome123)");
    builder.build().unwrap_or_else(|_| Client::new())
}

/// Base client creation logic strictly WITHOUT JA3 Emulation (Pure Native)
fn create_standard_client(timeout_secs: u64) -> Client {
    // No .emulation(Emulation::Chrome123) here!
    let builder = Client::builder().timeout(std::time::Duration::from_secs(timeout_secs));
    let builder = apply_upstream_proxy(builder, "HTTP standard client");

    tracing::info!("Initialized Pure Native Standard Client");
    builder.build().unwrap_or_else(|_| Client::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emulated_15_key() -> ClientKey {
        ClientKey::new(15, true)
    }

    #[test]
    fn shared_client_is_cached_until_invalidated() {
        let _guard = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        invalidate_shared_clients();

        let _ = get_client();
        assert!(
            shared_client_keys().contains(&emulated_15_key()),
            "get_client 应产出对应 key 的客户端"
        );

        // 同一 key 只应存在一份（HashMap 语义），重复获取不得新建实例。
        let _ = get_client();
        let count = shared_client_keys()
            .iter()
            .filter(|k| **k == emulated_15_key())
            .count();
        assert_eq!(count, 1, "重复获取同一 key 不应产生第二个实例");
    }

    /// 这是本次修复的核心语义：上游代理热更新后，已构建的共享客户端必须被丢弃，
    /// 否则它们会带着**旧代理**继续跑（token 刷新 / 配额刷新 / 项目解析）。
    #[test]
    fn invalidate_drops_clients_so_they_rebuild_with_current_proxy() {
        let _guard = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());

        let _ = get_client();
        assert!(shared_client_keys().contains(&emulated_15_key()));

        invalidate_shared_clients();
        assert!(
            !shared_client_keys().contains(&emulated_15_key()),
            "invalidate 必须丢弃已构建的客户端，否则改代理后仍走旧代理直到重启"
        );

        // 下次访问按当前配置重建
        let _ = get_client();
        assert!(
            shared_client_keys().contains(&emulated_15_key()),
            "失效后应重新构建"
        );

        invalidate_shared_clients();
    }
}
