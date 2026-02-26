use std::sync::Arc;
use tokio::sync::RwLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use dashmap::DashMap;
use rquest::Client;
use futures::{stream, StreamExt};
use std::time::Duration;
use crate::proxy::config::{ProxyPoolConfig, ProxySelectionStrategy, ProxyEntry};

use rquest_util::Emulation;
use std::sync::OnceLock;

/// 全局代理池管理器单例
pub static GLOBAL_PROXY_POOL: OnceLock<Arc<ProxyPoolManager>> = OnceLock::new();

/// 获取全局代理池管理器
pub fn get_global_proxy_pool() -> Option<Arc<ProxyPoolManager>> {
    GLOBAL_PROXY_POOL.get().cloned()
}

/// 初始化全局代理池管理器
pub fn init_global_proxy_pool(config: Arc<RwLock<ProxyPoolConfig>>) -> Arc<ProxyPoolManager> {
    let manager = Arc::new(ProxyPoolManager::new(config));
    let _ = GLOBAL_PROXY_POOL.set(manager.clone());
    manager
}

/// 代理配置 (用于构建 reqwest Client)
/// 注意：重命名为 PoolProxyConfig 以避免与 config::ProxyConfig 冲突
#[derive(Debug, Clone)]
pub struct PoolProxyConfig {
    pub proxy: rquest::Proxy,
    pub entry_id: String,
}

/// 代理池管理器
pub struct ProxyPoolManager {
    config: Arc<RwLock<ProxyPoolConfig>>,
    
    /// 代理使用计数 (proxy_id -> count)
    usage_counter: Arc<DashMap<String, usize>>,
    
    /// 账号到代理的绑定 (account_id -> proxy_id)
    account_bindings: Arc<DashMap<String, String>>,
    
    /// 轮询索引 (用于 RoundRobin 策略)
    round_robin_index: Arc<AtomicUsize>,
}

impl ProxyPoolManager {
    pub fn new(config: Arc<RwLock<ProxyPoolConfig>>) -> Self {
        // 从配置中加载已保存的绑定关系
        let account_bindings = Arc::new(DashMap::new());

        // 使用 blocking 方式读取配置（因为 new 不是 async）
        // 注意：这里使用 try_read 避免死锁
        if let Ok(cfg) = config.try_read() {
            for (account_id, proxy_id) in &cfg.account_bindings {
                account_bindings.insert(account_id.clone(), proxy_id.clone());
            }
            if !cfg.account_bindings.is_empty() {
                tracing::info!("[ProxyPool] Loaded {} account bindings from config", cfg.account_bindings.len());
            }
        }

        Self {
            config,
            usage_counter: Arc::new(DashMap::new()),
            account_bindings,
            round_robin_index: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// [NEW] 为指定账号获取“最终生效”的 HttpClient
    /// 逻辑：
    /// 1. 账号显式绑定代理优先 (Account-Proxy Binding)
    /// 2. 如果无绑定，且开启了“自动全局”，取池中第一个节点
    /// 3. 如果以上均无，则检查全局上游代理 (Upstream Proxy) [由调用方 fallback]
    pub async fn get_effective_client(&self, account_id: Option<&str>, timeout_secs: u64) -> Client {
        let mut builder = Client::builder()
            .emulation(Emulation::Chrome123)
            .timeout(Duration::from_secs(timeout_secs));
        
        // 尝试获取代理配置
        let proxy_opt = if let Some(acc_id) = account_id {
            self.get_proxy_for_account(acc_id).await.ok().flatten()
        } else {
            // 没有 account_id 的通用请求，如果代理池启用，则默认从中选择节点作为出口
            let config = self.config.read().await;
            if config.enabled {
                let res = self.select_proxy_from_pool(&config).await.ok().flatten();
                if let Some(ref p) = res {
                    tracing::info!("[Proxy] Route: Generic Request -> Proxy {} (Pool)", p.entry_id);
                } else {
                    // [FIX #1583] 明确记录池中无可用代理的情况
                    tracing::warn!("[Proxy] Route: Generic Request -> No available proxy in pool, falling back to upstream or direct");
                }
                res
            } else {
                tracing::debug!("[Proxy] Route: Generic Request -> Proxy pool disabled");
                None
            }
        };

        if let Some(proxy_cfg) = proxy_opt {
            builder = builder.proxy(proxy_cfg.proxy);
            // Already logged more detail in get_proxy_for_account or pool selection
        } else {
            // Fallback 到应用配置的单上游代理
            if let Ok(app_cfg) = crate::modules::config::load_app_config() {
                let up = app_cfg.proxy.upstream_proxy;
                if up.enabled && !up.url.is_empty() {
                    if let Ok(p) = rquest::Proxy::all(&up.url) {
                        tracing::info!("[Proxy] Route: {:?} -> Upstream: {} (AppConfig)", account_id.unwrap_or("Generic"), up.url);
                        builder = builder.proxy(p);
                    }
                } else {
                    tracing::info!("[Proxy] Route: {:?} -> Direct", account_id.unwrap_or("Generic"));
                }
            }
        }

        builder.build().unwrap_or_else(|_| Client::new())
    }

    /// [NEW] 为指定账号获取“最终生效”的无特征 Standard HttpClient (专门用于纯净场景，如 OAuth 退还)
    pub async fn get_effective_standard_client(&self, account_id: Option<&str>, timeout_secs: u64) -> Client {
        let mut builder = Client::builder()
            // 无 Emulation 设置，走纯正的基础 TLS 指纹
            .timeout(Duration::from_secs(timeout_secs));
        
        // 尝试获取代理配置
        let proxy_opt = if let Some(acc_id) = account_id {
            self.get_proxy_for_account(acc_id).await.ok().flatten()
        } else {
            // 没有 account_id 的通用请求，如果代理池启用，则默认从中选择节点作为出口
            let config = self.config.read().await;
            if config.enabled {
                let res = self.select_proxy_from_pool(&config).await.ok().flatten();
                if let Some(ref p) = res {
                    tracing::info!("[Proxy] Route: Generic Request (Standard Client) -> Proxy {} (Pool)", p.entry_id);
                } else {
                    tracing::warn!("[Proxy] Route: Generic Request (Standard Client) -> No available proxy in pool, falling back to upstream or direct");
                }
                res
            } else {
                tracing::debug!("[Proxy] Route: Generic Request (Standard Client) -> Proxy pool disabled");
                None
            }
        };

        if let Some(proxy_cfg) = proxy_opt {
            builder = builder.proxy(proxy_cfg.proxy);
        } else {
            // Fallback 到应用配置的单上游代理
            if let Ok(app_cfg) = crate::modules::config::load_app_config() {
                let up = app_cfg.proxy.upstream_proxy;
                if up.enabled && !up.url.is_empty() {
                    if let Ok(p) = rquest::Proxy::all(&up.url) {
                        tracing::info!("[Proxy] Route: {:?} (Standard Client) -> Upstream: {} (AppConfig)", account_id.unwrap_or("Generic"), up.url);
                        builder = builder.proxy(p);
                    }
                } else {
                    tracing::info!("[Proxy] Route: {:?} (Standard Client) -> Direct", account_id.unwrap_or("Generic"));
                }
            }
        }

        builder.build().unwrap_or_else(|_| Client::new())
    }

    /// 为账号获取代理
    pub async fn get_proxy_for_account(
        &self,
        account_id: &str,
    ) -> Result<Option<PoolProxyConfig>, String> {
        let config = self.config.read().await;
        
        if !config.enabled || config.proxies.is_empty() {
            return Ok(None);
        }
        
        // 1. 优先使用账号绑定 (专属 IP)
        if let Some(proxy) = self.get_bound_proxy(account_id, &config).await? {
            tracing::info!("[Proxy] Route: Account {} -> Proxy {} (Bound)", account_id, proxy.entry_id);
            return Ok(Some(proxy));
        }

        // 2. 否则从池中策略选择 (公用池)
        let res = self.select_proxy_from_pool(&config).await?;
        if let Some(ref p) = res {
            tracing::info!("[Proxy] Route: Account {} -> Proxy {} (Pool)", account_id, p.entry_id);
        }
        Ok(res)
    }
    
    /// 获取账号绑定的代理
    async fn get_bound_proxy(
        &self,
        account_id: &str,
        config: &ProxyPoolConfig,
    ) -> Result<Option<PoolProxyConfig>, String> {
        if let Some(proxy_id) = self.account_bindings.get(account_id) {
            if let Some(entry) = config.proxies.iter().find(|p| p.id == *proxy_id.value()) {
                if entry.enabled {
                    // 如果开启了自动故障转移且代理不健康，则返回 None (将回退到其他策略或失败)
                    if config.auto_failover && !entry.is_healthy {
                        return Ok(None);
                    }
                    return Ok(Some(self.build_proxy_config(entry)?));
                }
            }
        }
        Ok(None)
    }
    
    /// 从代理池中选择代理
    async fn select_proxy_from_pool(
        &self,
        config: &ProxyPoolConfig,
    ) -> Result<Option<PoolProxyConfig>, String> {
        // [FIX] 专属隔离逻辑：剔除所有已被绑定的代理，保护专属 IP 账号的安全
        let bound_ids: std::collections::HashSet<String> = self.account_bindings
            .iter()
            .map(|kv| kv.value().clone())
            .collect();

        let healthy_proxies: Vec<_> = config.proxies.iter()
            .filter(|p| {
                if !p.enabled { return false; }
                if config.auto_failover && !p.is_healthy { return false; }
                // 如果该代理已被某个账号“专属绑定”，则不再参与公用轮询
                if bound_ids.contains(&p.id) { return false; }
                true
            })
            .collect();
        
        if healthy_proxies.is_empty() {
             // 如果所有代理都被绑定了，或者池本身为空，尝试返回池中开启了且不依赖绑定的代理
             // (这里可以根据业务进一步调整，目前保持严谨隔离)
            return Ok(None);
        }
        
        let selected = match config.strategy {
            ProxySelectionStrategy::RoundRobin => {
                self.select_round_robin(&healthy_proxies)
            }
            ProxySelectionStrategy::Random => {
                self.select_random(&healthy_proxies)
            }
            ProxySelectionStrategy::Priority => {
                self.select_by_priority(&healthy_proxies)
            }
            ProxySelectionStrategy::LeastConnections => {
                self.select_least_connections(&healthy_proxies)
            }
            ProxySelectionStrategy::WeightedRoundRobin => {
                self.select_weighted(&healthy_proxies)
            }
        };
        
        if let Some(entry) = selected {
            // 更新计数
            *self.usage_counter.entry(entry.id.clone()).or_insert(0) += 1;
            Ok(Some(self.build_proxy_config(entry)?))
        } else {
            Ok(None)
        }
    }
    
    fn select_round_robin<'a>(&self, proxies: &[&'a ProxyEntry]) -> Option<&'a ProxyEntry> {
        if proxies.is_empty() { return None; }
        let index = self.round_robin_index.fetch_add(1, Ordering::Relaxed);
        Some(proxies[index % proxies.len()])
    }

    fn select_random<'a>(&self, proxies: &[&'a ProxyEntry]) -> Option<&'a ProxyEntry> {
        if proxies.is_empty() { return None; }
        use rand::seq::SliceRandom;
        let mut rng = rand::thread_rng();
        proxies.choose(&mut rng).copied()
    }
    
    fn select_by_priority<'a>(&self, proxies: &[&'a ProxyEntry]) -> Option<&'a ProxyEntry> {
        // priority 越小越优先
        proxies.iter().min_by_key(|p| p.priority).copied()
    }
    
    fn select_least_connections<'a>(&self, proxies: &[&'a ProxyEntry]) -> Option<&'a ProxyEntry> {
        proxies.iter().min_by_key(|p| {
            self.usage_counter.get(&p.id).map(|v| *v).unwrap_or(0)
        }).copied()
    }
    
    fn select_weighted<'a>(&self, proxies: &[&'a ProxyEntry]) -> Option<&'a ProxyEntry> {
        // 简单实现: 类似优先级的加权, 这里暂用 Priority 代替
        self.select_by_priority(proxies)
    }

    /// 构建 reqwest::Proxy 配置
    fn build_proxy_config(&self, entry: &ProxyEntry) -> Result<PoolProxyConfig, String> {
        let url = crate::proxy::config::normalize_proxy_url(&entry.url);

        let mut proxy = rquest::Proxy::all(&url)
            .map_err(|e| format!("Invalid proxy URL: {}", e))?;
        
        // 添加认证
        if let Some(auth) = &entry.auth {
            proxy = proxy.basic_auth(&auth.username, &auth.password);
        }
        
        Ok(PoolProxyConfig {
            proxy,
            entry_id: entry.id.clone(),
        })
    }
    
    /// 绑定账号到代理
    pub async fn bind_account_to_proxy(
        &self,
        account_id: String,
        proxy_id: String,
    ) -> Result<(), String> {
        // 检查代理是否存在
        {
            let config = self.config.read().await;
            if !config.proxies.iter().any(|p| p.id == proxy_id) {
                return Err(format!("Proxy {} not found", proxy_id));
            }

            // 检查代理最大账号数限制
            if let Some(entry) = config.proxies.iter().find(|p| p.id == proxy_id) {
                if let Some(max) = entry.max_accounts {
                    if max > 0 {
                        let current_count = self.account_bindings.iter()
                            .filter(|kv| *kv.value() == proxy_id)
                            .count();
                        if current_count >= max {
                            return Err(format!("Proxy {} has reached max accounts limit", proxy_id));
                        }
                    }
                }
            }
        }

        // 更新内存中的绑定
        self.account_bindings.insert(account_id.clone(), proxy_id.clone());

        // 持久化到配置文件
        self.persist_bindings().await;

        tracing::info!("[ProxyPool] Bound account {} to proxy {}", account_id, proxy_id);
        Ok(())
    }

    /// 解绑账号代理
    pub async fn unbind_account_proxy(&self, account_id: String) {
        self.account_bindings.remove(&account_id);

        // 持久化到配置文件
        self.persist_bindings().await;

        tracing::info!("[ProxyPool] Unbound account {}", account_id);
    }

    /// 获取账号当前绑定的代理ID
    pub fn get_account_binding(&self, account_id: &str) -> Option<String> {
        self.account_bindings.get(account_id).map(|v| v.value().clone())
    }

    /// 获取所有绑定关系的快照
    pub fn get_all_bindings_snapshot(&self) -> std::collections::HashMap<String, String> {
        self.account_bindings.iter()
            .map(|kv| (kv.key().clone(), kv.value().clone()))
            .collect()
    }

    /// 持久化绑定关系到配置文件
    async fn persist_bindings(&self) {
        // 获取当前绑定快照
        let bindings = self.get_all_bindings_snapshot();

        // 更新配置中的绑定关系
        {
            let mut config = self.config.write().await;
            config.account_bindings = bindings;
        }

        // 保存到磁盘
        if let Ok(mut app_config) = crate::modules::config::load_app_config() {
            let config = self.config.read().await;
            app_config.proxy.proxy_pool = config.clone();
            if let Err(e) = crate::modules::config::save_app_config(&app_config) {
                tracing::error!("[ProxyPool] Failed to persist bindings: {}", e);
            }
        }
    }

    /// 健康检查
    pub async fn health_check(&self) -> Result<(), String> {
        // 由于需要异步并发检查，且不能锁住 config 太久，
        // 我们先复制一份需要检查的代理列表
        let proxies_to_check: Vec<_> = {
            let config = self.config.read().await;
            config.proxies.iter()
                .filter(|p| p.enabled)
                .cloned()
                .collect()
        };

        let concurrency_limit = 20usize;
        let results = stream::iter(proxies_to_check)
            .map(|proxy| async move {
                let (is_healthy, latency) = self.check_proxy_health(&proxy).await;
                
                let latency_msg = if let Some(ms) = latency {
                    format!("{}ms", ms)
                } else {
                    "-".to_string()
                };

                tracing::info!(
                    "Proxy {} ({}) health check: {} (Latency: {})",
                    proxy.name,
                    proxy.url,
                    if is_healthy { "✓ OK" } else { "✗ FAILED" },
                    latency_msg
                );

                (proxy.id, is_healthy, latency)
            })
            .buffer_unordered(concurrency_limit)
            .collect::<Vec<_>>()
            .await;

        // 统一更新状态
        let mut config = self.config.write().await;
        for (id, is_healthy, latency) in results {
            if let Some(proxy) = config.proxies.iter_mut().find(|p| p.id == id) {
                proxy.is_healthy = is_healthy;
                proxy.latency = latency;
                proxy.last_check_time = Some(chrono::Utc::now().timestamp());
            }
        }
        
        Ok(())
    }
    
    /// 检查单个代理健康状态
    async fn check_proxy_health(&self, entry: &ProxyEntry) -> (bool, Option<u64>) {
        let check_url = if let Some(url) = &entry.health_check_url {
            if url.trim().is_empty() {
                "http://cp.cloudflare.com/generate_204"
            } else {
                url.as_str()
            }
        } else {
            "http://cp.cloudflare.com/generate_204"
        };
        
        // 尝试构建 Client，如果失败直接视为不健康
        let proxy_res = self.build_proxy_config(entry);
        if let Err(e) = proxy_res { 
            tracing::error!("Proxy {} build config failed: {}", entry.url, e);
            return (false, None); 
        }
        let proxy_cfg = proxy_res.unwrap();

        let client_result = Client::builder()
            .proxy(proxy_cfg.proxy)
            .emulation(Emulation::Chrome123)
            .timeout(Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36")
            .build();
        
        let client = match client_result {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Proxy {} build client failed: {}", entry.url, e);
                return (false, None);
            },
        };
        
        let start = std::time::Instant::now();
        match client.get(check_url).send().await {
            Ok(resp) => {
                let latency = start.elapsed().as_millis() as u64;
                if resp.status().is_success() {
                    (true, Some(latency))
                } else {
                    tracing::warn!("Proxy {} health check status error: {}", entry.url, resp.status());
                    (false, None)
                }
            },
            Err(e) => {
                tracing::warn!("Proxy {} health check request failed: {}", entry.url, e);
                (false, None)
            },
        }
    }

    /// 启动健康检查循环
    pub fn start_health_check_loop(self: Arc<Self>) {
        tokio::spawn(async move {
            tracing::info!("Starting proxy pool health check loop...");
            loop {
                // Perform check only if enabled
                let enabled = self.config.read().await.enabled;
                if enabled {
                    if let Err(e) = self.health_check().await {
                        tracing::error!("Proxy pool health check failed: {}", e);
                    }
                }

                // Get interval and sleep AFTER check
                let interval_secs = {
                    let cfg = self.config.read().await;
                    if !cfg.enabled {
                        60 // check every minute if disabled
                    } else {
                        cfg.health_check_interval.max(30) // Back to default min 30s
                    }
                };

                tokio::time::sleep(Duration::from_secs(interval_secs)).await;
            }
        });
    }
}
