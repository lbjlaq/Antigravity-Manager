use crate::proxy::config::{
    effective_proxy_pool_strict_mode, ProxyEntry, ProxyPoolConfig, ProxySelectionStrategy,
};
use dashmap::DashMap;
use futures::{stream, StreamExt};
use rquest::Client;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

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

/// 新增账号 / OAuth 流程可选预选代理及严格模式配置
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AddAccountProxyOptions {
    #[serde(default, alias = "proxy_id")]
    pub proxy_id: Option<String>,
    #[serde(default, alias = "strict_proxy")]
    pub strict_proxy: bool,
}

impl AddAccountProxyOptions {
    pub fn normalized(mut self) -> Self {
        self.proxy_id = self
            .proxy_id
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty());
        self
    }

    pub fn is_empty(&self) -> bool {
        self.proxy_id.is_none() && !self.strict_proxy
    }
}

/// 已解析的 HTTP 客户端及其代理来源描述（供严格模式日志与显式报错）
pub struct ResolvedClient {
    pub client: Client,
    pub proxy_label: Option<String>,
    pub strict: bool,
}

fn describe_entry_label(entry: &ProxyEntry) -> String {
    if entry.name.trim().is_empty() {
        entry.id.clone()
    } else {
        format!("{} ({})", entry.name.trim(), entry.id)
    }
}

fn build_rquest_client_builder(emulated: bool, timeout_secs: u64) -> rquest::ClientBuilder {
    let builder = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .pool_idle_timeout(Duration::from_secs(45))
        .timeout(Duration::from_secs(timeout_secs));
    if emulated {
        builder.emulation(Emulation::Chrome123)
    } else {
        builder
    }
}

async fn load_effective_proxy_pool_config() -> ProxyPoolConfig {
    if let Some(pool) = get_global_proxy_pool() {
        return pool.config.read().await.clone();
    }
    crate::modules::config::load_app_config()
        .map(|cfg| cfg.proxy.proxy_pool)
        .unwrap_or_default()
}

/// 静态构建 rquest::Proxy 配置，供 ProxyPoolManager 与桌面离线场景复用
pub fn build_proxy_config_from_entry(entry: &ProxyEntry) -> Result<PoolProxyConfig, String> {
    let raw_url = crate::proxy::config::normalize_proxy_url(&entry.url);
    if raw_url.trim().is_empty() {
        return Err("Proxy URL is empty".to_string());
    }
    let (clean_url, parsed_auth) = ProxyPoolManager::parse_and_clean_proxy_url(&raw_url);

    let mut proxy = rquest::Proxy::all(&clean_url)
        .or_else(|_| rquest::Proxy::all(&raw_url))
        .map_err(|e| format!("Invalid proxy URL: {}", e))?;

    if let Some(auth) = &entry.auth {
        if !auth.username.is_empty() {
            if auth.password.starts_with("ag_enc_") {
                if let Some((user, pass)) = &parsed_auth {
                    tracing::warn!(
                        "[ProxyPool] Proxy password decryption failed (retains ag_enc_ prefix); falling back to credentials embedded in URL"
                    );
                    proxy = proxy.basic_auth(user, pass);
                } else {
                    tracing::warn!(
                        "[ProxyPool] Proxy password decryption failed (retains ag_enc_ prefix) and no URL credentials available; refusing to send encrypted ciphertext as password"
                    );
                }
            } else {
                proxy = proxy.basic_auth(&auth.username, &auth.password);
            }
        } else if let Some((user, pass)) = parsed_auth {
            proxy = proxy.basic_auth(&user, &pass);
        }
    } else if let Some((user, pass)) = parsed_auth {
        proxy = proxy.basic_auth(&user, &pass);
    }

    Ok(PoolProxyConfig {
        proxy,
        entry_id: entry.id.clone(),
    })
}

fn validate_add_account_proxy_options_with_config(
    options: &AddAccountProxyOptions,
    pool_cfg: &ProxyPoolConfig,
    upstream_proxy: Option<&crate::proxy::config::UpstreamProxyConfig>,
) -> Result<Option<String>, String> {
    let normalized = options.clone().normalized();
    let strict = normalized.strict_proxy || effective_proxy_pool_strict_mode(pool_cfg);

    if let Some(ref id) = normalized.proxy_id {
        let Some(entry) = pool_cfg.proxies.iter().find(|p| p.id == *id) else {
            if strict {
                return Err(format!(
                    "[Strict Proxy] 指定的代理节点 '{}' 不存在或已被删除",
                    id
                ));
            }
            tracing::warn!(
                "[ProxyPool] Pre-selected proxy '{}' not found; falling back to default routing",
                id
            );
            return Ok(None);
        };

        if !entry.enabled {
            if strict {
                return Err(format!(
                    "[Strict Proxy] 指定的代理节点 '{}' 已禁用",
                    describe_entry_label(entry)
                ));
            }
            tracing::warn!(
                "[ProxyPool] Pre-selected proxy '{}' is disabled; falling back to default routing",
                describe_entry_label(entry)
            );
            return Ok(None);
        }

        if let Err(e) = build_proxy_config_from_entry(entry) {
            if strict {
                return Err(format!(
                    "[Strict Proxy] 指定的代理节点 '{}' 配置无效: {}",
                    describe_entry_label(entry),
                    e
                ));
            }
            tracing::warn!(
                "[ProxyPool] Pre-selected proxy '{}' has invalid config ({}); falling back to default routing",
                describe_entry_label(entry),
                e
            );
            return Ok(None);
        }

        return Ok(Some(entry.id.clone()));
    }

    if strict {
        let has_pool_proxy = pool_cfg.enabled && pool_cfg.proxies.iter().any(|p| p.enabled);
        let has_upstream_proxy =
            upstream_proxy.is_some_and(|up| up.enabled && !up.url.trim().is_empty());
        if !has_pool_proxy && !has_upstream_proxy {
            return Err(
                "[Strict Proxy] 严格代理模式已开启，但未选择代理节点且未配置可用的代理池或全局上游代理，已禁止直连回退".to_string(),
            );
        }
    }

    Ok(None)
}

/// 预校验添加账号时的代理选择与严格模式约束，返回最终可用作绑定的 `proxy_id`
pub async fn validate_add_account_proxy_options(
    options: &AddAccountProxyOptions,
) -> Result<Option<String>, String> {
    let pool_cfg = load_effective_proxy_pool_config().await;
    let upstream_proxy = crate::modules::config::load_app_config()
        .ok()
        .map(|c| c.proxy.upstream_proxy);
    validate_add_account_proxy_options_with_config(options, &pool_cfg, upstream_proxy.as_ref())
}

fn check_max_accounts_limit(
    entry: &ProxyEntry,
    account_id: &str,
    bindings_iter: impl Iterator<Item = (String, String)>,
) -> Result<(), String> {
    if let Some(max) = entry.max_accounts {
        if max > 0 {
            let current_count = bindings_iter
                .filter(|(acc_id, pid)| *pid == entry.id && acc_id != account_id)
                .count();
            if current_count >= max {
                return Err(format!(
                    "Proxy {} has reached max accounts limit ({}/{})",
                    describe_entry_label(entry),
                    current_count,
                    max
                ));
            }
        }
    }
    Ok(())
}

/// 为账号绑定代理并持久化（同时兼容反代已启动的全局池与桌面未启动反代的离线配置）
pub async fn bind_account_proxy_persistent(
    account_id: String,
    proxy_id: String,
) -> Result<(), String> {
    if let Some(pool) = get_global_proxy_pool() {
        return pool.bind_account_to_proxy(account_id, proxy_id).await;
    }

    let mut app_config = crate::modules::config::load_app_config()
        .map_err(|e| format!("Failed to load config for proxy binding: {}", e))?;
    let entry = app_config
        .proxy
        .proxy_pool
        .proxies
        .iter()
        .find(|p| p.id == proxy_id)
        .ok_or_else(|| format!("Proxy {} not found", proxy_id))?
        .clone();

    check_max_accounts_limit(
        &entry,
        &account_id,
        app_config
            .proxy
            .proxy_pool
            .account_bindings
            .iter()
            .map(|(k, v)| (k.clone(), v.clone())),
    )?;

    app_config
        .proxy
        .proxy_pool
        .account_bindings
        .insert(account_id.clone(), proxy_id.clone());
    crate::modules::config::save_app_config(&app_config)?;
    tracing::info!(
        "[ProxyPool] Bound account {} to proxy {} (offline config persisted)",
        account_id,
        proxy_id
    );
    Ok(())
}

/// 根据添加账号时的预选代理与严格模式解析最终用于请求的 HTTP Client
pub async fn resolve_client_for_add_account(
    options: &AddAccountProxyOptions,
    emulated: bool,
    timeout_secs: u64,
) -> Result<ResolvedClient, String> {
    let normalized = options.clone().normalized();
    let global_pool = get_global_proxy_pool();
    let pool_cfg = match &global_pool {
        Some(pool) => pool.config.read().await.clone(),
        None => crate::modules::config::load_app_config()
            .map(|cfg| cfg.proxy.proxy_pool)
            .unwrap_or_default(),
    };
    let app_upstream = crate::modules::config::load_app_config()
        .ok()
        .map(|cfg| cfg.proxy.upstream_proxy);

    resolve_client_for_add_account_with_config(
        &normalized,
        &pool_cfg,
        global_pool.as_deref(),
        app_upstream.as_ref(),
        emulated,
        timeout_secs,
    )
    .await
}

async fn resolve_client_for_add_account_with_config(
    options: &AddAccountProxyOptions,
    pool_cfg: &ProxyPoolConfig,
    global_pool: Option<&ProxyPoolManager>,
    upstream_proxy: Option<&crate::proxy::config::UpstreamProxyConfig>,
    emulated: bool,
    timeout_secs: u64,
) -> Result<ResolvedClient, String> {
    let strict = options.strict_proxy || effective_proxy_pool_strict_mode(pool_cfg);

    if let Some(ref proxy_id) = options.proxy_id {
        let matched = pool_cfg.proxies.iter().find(|p| p.id == *proxy_id);
        match matched {
            Some(entry) if entry.enabled => {
                // 非严格模式下，若预选节点不健康且启用了自动故障转移且代理池开启，保留现有故障转移习惯
                if !strict && !entry.is_healthy && pool_cfg.auto_failover && pool_cfg.enabled {
                    if let Some(pool) = global_pool {
                        if let Ok(Some(fallback_cfg)) = pool.select_proxy_from_pool(pool_cfg).await
                        {
                            let mut builder = build_rquest_client_builder(emulated, timeout_secs);
                            builder = builder.proxy(fallback_cfg.proxy);
                            let client = builder.build().map_err(|e| {
                                format!("Failed to build fallback proxy client: {}", e)
                            })?;
                            tracing::warn!(
                                "[Proxy] AddAccount pre-selected proxy '{}' is unhealthy; failover to pool proxy {}",
                                describe_entry_label(entry),
                                fallback_cfg.entry_id
                            );
                            return Ok(ResolvedClient {
                                client,
                                proxy_label: Some(fallback_cfg.entry_id),
                                strict,
                            });
                        }
                    }
                }

                let proxy_cfg = build_proxy_config_from_entry(entry).map_err(|e| {
                    format!(
                        "[Strict Proxy] 代理节点 '{}' 配置无效: {}",
                        describe_entry_label(entry),
                        e
                    )
                });
                match proxy_cfg {
                    Ok(cfg) => {
                        let label = describe_entry_label(entry);
                        let builder =
                            build_rquest_client_builder(emulated, timeout_secs).proxy(cfg.proxy);
                        let client = builder.build().map_err(|e| {
                            format!("[Strict Proxy] 构建代理节点 '{}' 客户端失败: {}", label, e)
                        })?;
                        tracing::info!(
                            "[Proxy] Route: AddAccount (strict={}) -> Pre-selected Proxy {}",
                            strict,
                            label
                        );
                        return Ok(ResolvedClient {
                            client,
                            proxy_label: Some(label),
                            strict,
                        });
                    }
                    Err(err_msg) => {
                        if strict {
                            return Err(err_msg);
                        }
                        tracing::warn!(
                            "[Proxy] AddAccount proxy build error ({}); falling back to default routing",
                            err_msg
                        );
                    }
                }
            }
            Some(entry) => {
                if strict {
                    return Err(format!(
                        "[Strict Proxy] 指定的代理节点 '{}' 已禁用",
                        describe_entry_label(entry)
                    ));
                }
                tracing::warn!(
                    "[Proxy] Pre-selected proxy '{}' is disabled; falling back to default routing",
                    describe_entry_label(entry)
                );
            }
            None => {
                if strict {
                    return Err(format!(
                        "[Strict Proxy] 指定的代理节点 '{}' 不存在或已被删除",
                        proxy_id
                    ));
                }
                tracing::warn!(
                    "[Proxy] Pre-selected proxy '{}' not found; falling back to default routing",
                    proxy_id
                );
            }
        }
    }

    // 未指定节点或非严格模式下回退到常规策略
    if let Some(pool) = global_pool {
        if strict {
            let client = pool
                .resolve_effective_client_inner(None, emulated, timeout_secs, true)
                .await?;
            return Ok(ResolvedClient {
                client,
                proxy_label: None,
                strict: true,
            });
        }
        let client = pool
            .resolve_effective_client_inner(None, emulated, timeout_secs, false)
            .await?;
        return Ok(ResolvedClient {
            client,
            proxy_label: None,
            strict: false,
        });
    }

    // 代理服务未启动时，使用静态配置（先查代理池默认节点，再查全局上游代理）
    if pool_cfg.enabled && !pool_cfg.proxies.is_empty() {
        let manager = ProxyPoolManager::new(Arc::new(RwLock::new(pool_cfg.clone())));
        if let Ok(Some(cfg)) = manager.select_proxy_from_pool(pool_cfg).await {
            let builder = build_rquest_client_builder(emulated, timeout_secs).proxy(cfg.proxy);
            let client = builder
                .build()
                .map_err(|e| format!("Failed to build pool proxy client: {}", e))?;
            return Ok(ResolvedClient {
                client,
                proxy_label: Some(cfg.entry_id),
                strict,
            });
        }
    }

    if let Some(up) = upstream_proxy {
        if up.enabled && !up.url.trim().is_empty() {
            match rquest::Proxy::all(&up.url) {
                Ok(p) => {
                    let builder = build_rquest_client_builder(emulated, timeout_secs).proxy(p);
                    let client = builder.build().map_err(|e| {
                        format!("Failed to build upstream proxy client '{}': {}", up.url, e)
                    })?;
                    return Ok(ResolvedClient {
                        client,
                        proxy_label: Some(up.url.clone()),
                        strict,
                    });
                }
                Err(e) if strict => {
                    return Err(format!(
                        "[Strict Proxy] 全局上游代理 '{}' 无效: {}",
                        up.url, e
                    ));
                }
                Err(_) => {}
            }
        }
    }

    if strict {
        return Err(
            "[Strict Proxy] 严格代理模式已开启，但当前无可用代理节点或全局上游代理，已禁止直连回退"
                .to_string(),
        );
    }

    let client = match (emulated, timeout_secs) {
        (true, 15) => crate::utils::http::get_client(),
        (false, 60) => crate::utils::http::get_long_standard_client(),
        (false, 15) => crate::utils::http::get_standard_client(),
        _ => build_rquest_client_builder(emulated, timeout_secs)
            .build()
            .unwrap_or_else(|_| Client::new()),
    };
    Ok(ResolvedClient {
        client,
        proxy_label: None,
        strict: false,
    })
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
                tracing::info!(
                    "[ProxyPool] Loaded {} account bindings from config",
                    cfg.account_bindings.len()
                );
            }
        }

        Self {
            config,
            usage_counter: Arc::new(DashMap::new()),
            account_bindings,
            round_robin_index: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// 当前代理池是否处于严格模式（受配置字段与环境变量共同控制）
    pub async fn is_strict_mode(&self) -> bool {
        let config = self.config.read().await;
        effective_proxy_pool_strict_mode(&config)
    }

    async fn resolve_effective_client_inner(
        &self,
        account_id: Option<&str>,
        emulated: bool,
        timeout_secs: u64,
        force_strict: bool,
    ) -> Result<Client, String> {
        let mut builder = build_rquest_client_builder(emulated, timeout_secs);
        let (strict, proxy_opt) = {
            let config = self.config.read().await;
            let strict = force_strict || effective_proxy_pool_strict_mode(&config);
            drop(config);

            let res = if let Some(acc_id) = account_id {
                self.get_proxy_for_account(acc_id).await
            } else {
                let config = self.config.read().await;
                if config.enabled {
                    let sel = self.select_proxy_from_pool(&config).await;
                    if let Ok(Some(ref p)) = sel {
                        tracing::info!(
                            "[Proxy] Route: Generic Request (emulated={}) -> Proxy {} (Pool)",
                            emulated,
                            p.entry_id
                        );
                    } else if sel.is_ok() {
                        tracing::warn!(
                            "[Proxy] Route: Generic Request -> No available proxy in pool"
                        );
                    }
                    sel
                } else {
                    tracing::debug!("[Proxy] Route: Generic Request -> Proxy pool disabled");
                    Ok(None)
                }
            };
            (strict, res?)
        };

        if let Some(proxy_cfg) = proxy_opt {
            builder = builder.proxy(proxy_cfg.proxy);
            return builder.build().map_err(|e| {
                format!(
                    "Failed to build HTTP client for proxy {}: {}",
                    proxy_cfg.entry_id, e
                )
            });
        }

        // 未命中代理池节点时：先检查单上游代理，严格模式下禁止静默直连回退
        if let Ok(app_cfg) = crate::modules::config::load_app_config() {
            let up = app_cfg.proxy.upstream_proxy;
            if up.enabled && !up.url.trim().is_empty() {
                match rquest::Proxy::all(&up.url) {
                    Ok(p) => {
                        tracing::info!(
                            "[Proxy] Route: {:?} (emulated={}) -> Upstream: {} (AppConfig)",
                            account_id.unwrap_or("Generic"),
                            emulated,
                            up.url
                        );
                        builder = builder.proxy(p);
                        return builder.build().map_err(|e| {
                            format!("Failed to build upstream proxy client '{}': {}", up.url, e)
                        });
                    }
                    Err(e) if strict => {
                        return Err(format!(
                            "[Strict Proxy] 全局上游代理 '{}' 无效: {}",
                            up.url, e
                        ));
                    }
                    Err(e) => {
                        tracing::error!("invalid upstream proxy url '{}': {}", up.url, e);
                    }
                }
            }
        }

        if strict {
            return Err(format!(
                "[Strict Proxy] 严格代理模式已开启，但 {:?} 未解析到可用代理节点或全局上游代理，已禁止直连回退",
                account_id.unwrap_or("Generic")
            ));
        }

        tracing::info!(
            "[Proxy] Route: {:?} (emulated={}) -> Direct",
            account_id.unwrap_or("Generic"),
            emulated
        );
        Ok(builder.build().unwrap_or_else(|_| Client::new()))
    }

    /// [NEW] 为指定账号获取“最终生效”的 HttpClient（可失败，严格模式下禁止静默回退）
    pub async fn try_get_effective_client(
        &self,
        account_id: Option<&str>,
        timeout_secs: u64,
    ) -> Result<Client, String> {
        self.resolve_effective_client_inner(account_id, true, timeout_secs, false)
            .await
    }

    /// [NEW] 为指定账号获取“最终生效”的 HttpClient
    /// 逻辑：
    /// 1. 账号显式绑定代理优先 (Account-Proxy Binding)
    /// 2. 如果无绑定，且开启了“自动全局”，取池中节点
    /// 3. 如果以上均无，则检查全局上游代理 (Upstream Proxy)
    pub async fn get_effective_client(
        &self,
        account_id: Option<&str>,
        timeout_secs: u64,
    ) -> Client {
        match self
            .try_get_effective_client(account_id, timeout_secs)
            .await
        {
            Ok(client) => client,
            Err(e) => {
                tracing::error!(
                    "[Proxy] Fallback triggered on infallible get_effective_client({:?}): {}",
                    account_id,
                    e
                );
                build_rquest_client_builder(true, timeout_secs)
                    .build()
                    .unwrap_or_else(|_| Client::new())
            }
        }
    }

    /// [NEW] 为指定账号获取“最终生效”的无特征 Standard HttpClient（可失败，严格模式下禁止静默回退）
    pub async fn try_get_effective_standard_client(
        &self,
        account_id: Option<&str>,
        timeout_secs: u64,
    ) -> Result<Client, String> {
        self.resolve_effective_client_inner(account_id, false, timeout_secs, false)
            .await
    }

    /// [NEW] 为指定账号获取“最终生效”的无特征 Standard HttpClient (专门用于纯净场景，如 OAuth 退还)
    pub async fn get_effective_standard_client(
        &self,
        account_id: Option<&str>,
        timeout_secs: u64,
    ) -> Client {
        match self
            .try_get_effective_standard_client(account_id, timeout_secs)
            .await
        {
            Ok(client) => client,
            Err(e) => {
                tracing::error!(
                    "[Proxy] Fallback triggered on infallible get_effective_standard_client({:?}): {}",
                    account_id,
                    e
                );
                build_rquest_client_builder(false, timeout_secs)
                    .build()
                    .unwrap_or_else(|_| Client::new())
            }
        }
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
            tracing::info!(
                "[Proxy] Route: Account {} -> Proxy {} (Bound)",
                account_id,
                proxy.entry_id
            );
            return Ok(Some(proxy));
        }

        // 2. 否则从池中策略选择 (公用池)
        let res = self.select_proxy_from_pool(&config).await?;
        if let Some(ref p) = res {
            tracing::info!(
                "[Proxy] Route: Account {} -> Proxy {} (Pool)",
                account_id,
                p.entry_id
            );
        }
        Ok(res)
    }

    /// 获取账号绑定的代理
    async fn get_bound_proxy(
        &self,
        account_id: &str,
        config: &ProxyPoolConfig,
    ) -> Result<Option<PoolProxyConfig>, String> {
        let strict = effective_proxy_pool_strict_mode(config);
        if let Some(proxy_id) = self.account_bindings.get(account_id) {
            let bound_id = proxy_id.value().clone();
            if let Some(entry) = config.proxies.iter().find(|p| p.id == bound_id) {
                if entry.enabled {
                    // 1. 若代理健康，直接使用专属绑定代理
                    if entry.is_healthy {
                        return Ok(Some(self.build_proxy_config(entry)?));
                    }

                    // 2. 非严格模式下，若代理不健康且启用了自动故障转移，优先从公用池选择其他可用代理
                    if !strict && config.auto_failover {
                        if let Ok(Some(fallback_proxy)) = self.select_proxy_from_pool(config).await
                        {
                            tracing::warn!(
                                "[Proxy] Bound proxy {} for account {} is unhealthy; auto-failover to pool proxy {}",
                                entry.id,
                                account_id,
                                fallback_proxy.entry_id
                            );
                            return Ok(Some(fallback_proxy));
                        }
                    }

                    // 3. 严格模式或不回退兜底：保持使用绑定代理，杜绝切到其他节点或 Direct 直连泄露源站 IP
                    tracing::warn!(
                        "[Proxy] Bound proxy {} for account {} is marked unhealthy (strict={}); keeping bound proxy",
                        entry.id,
                        account_id,
                        strict
                    );
                    return Ok(Some(self.build_proxy_config(entry)?));
                }

                if strict {
                    return Err(format!(
                        "[Strict Proxy] 账号 {} 绑定的代理节点 '{}' 已禁用，严格模式已禁止回退其他出口",
                        account_id,
                        describe_entry_label(entry)
                    ));
                }
            } else if strict {
                return Err(format!(
                    "[Strict Proxy] 账号 {} 绑定的代理节点 '{}' 不存在，严格模式已禁止回退其他出口",
                    account_id, bound_id
                ));
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
        let bound_ids: std::collections::HashSet<String> = self
            .account_bindings
            .iter()
            .map(|kv| kv.value().clone())
            .collect();

        let healthy_proxies: Vec<_> = config
            .proxies
            .iter()
            .filter(|p| {
                if !p.enabled {
                    return false;
                }
                if config.auto_failover && !p.is_healthy {
                    return false;
                }
                // 如果该代理已被某个账号“专属绑定”，则不再参与公用轮询
                if bound_ids.contains(&p.id) {
                    return false;
                }
                true
            })
            .collect();

        if healthy_proxies.is_empty() {
            // 如果所有代理都被绑定了，或者池本身为空，尝试返回池中开启了且不依赖绑定的代理
            // (这里可以根据业务进一步调整，目前保持严谨隔离)
            return Ok(None);
        }

        let selected = match config.strategy {
            ProxySelectionStrategy::RoundRobin => self.select_round_robin(&healthy_proxies),
            ProxySelectionStrategy::Random => self.select_random(&healthy_proxies),
            ProxySelectionStrategy::Priority => self.select_by_priority(&healthy_proxies),
            ProxySelectionStrategy::LeastConnections => {
                self.select_least_connections(&healthy_proxies)
            }
            ProxySelectionStrategy::WeightedRoundRobin => self.select_weighted(&healthy_proxies),
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
        if proxies.is_empty() {
            return None;
        }
        let index = self.round_robin_index.fetch_add(1, Ordering::Relaxed);
        Some(proxies[index % proxies.len()])
    }

    fn select_random<'a>(&self, proxies: &[&'a ProxyEntry]) -> Option<&'a ProxyEntry> {
        if proxies.is_empty() {
            return None;
        }
        use rand::seq::SliceRandom;
        let mut rng = rand::thread_rng();
        proxies.choose(&mut rng).copied()
    }

    fn select_by_priority<'a>(&self, proxies: &[&'a ProxyEntry]) -> Option<&'a ProxyEntry> {
        // priority 越小越优先
        proxies.iter().min_by_key(|p| p.priority).copied()
    }

    fn select_least_connections<'a>(&self, proxies: &[&'a ProxyEntry]) -> Option<&'a ProxyEntry> {
        proxies
            .iter()
            .min_by_key(|p| self.usage_counter.get(&p.id).map(|v| *v).unwrap_or(0))
            .copied()
    }

    fn select_weighted<'a>(&self, proxies: &[&'a ProxyEntry]) -> Option<&'a ProxyEntry> {
        // 简单实现: 类似优先级的加权, 这里暂用 Priority 代替
        self.select_by_priority(proxies)
    }

    /// 解析并清理 URL 中的凭据
    pub fn parse_and_clean_proxy_url(raw_url: &str) -> (String, Option<(String, String)>) {
        match url::Url::parse(raw_url) {
            Ok(mut u) => {
                let user = if !u.username().is_empty() {
                    Some(u.username().to_string())
                } else {
                    None
                };
                let pass = u.password().map(|p| p.to_string());

                // 清理 URL 中的凭据以防特定底层库解析异常
                // 注意：必须先清理 password 再清理 username，确保符合 WHATWG URL 规范并杜绝凭据残留
                let _ = u.set_password(None);
                let _ = u.set_username("");

                let auth = if let (Some(user), Some(pass)) = (user, pass) {
                    Some((user, pass))
                } else {
                    None
                };
                (u.to_string(), auth)
            }
            Err(_) => (raw_url.to_string(), None),
        }
    }

    /// 构建 rquest::Proxy 配置
    fn build_proxy_config(&self, entry: &ProxyEntry) -> Result<PoolProxyConfig, String> {
        build_proxy_config_from_entry(entry)
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
            let entry = config
                .proxies
                .iter()
                .find(|p| p.id == proxy_id)
                .ok_or_else(|| format!("Proxy {} not found", proxy_id))?;

            // 检查代理最大账号数限制（同一账号重复绑定不计入占用增量）
            check_max_accounts_limit(
                entry,
                &account_id,
                self.account_bindings
                    .iter()
                    .map(|kv| (kv.key().clone(), kv.value().clone())),
            )?;
        }

        // 更新内存中的绑定
        self.account_bindings
            .insert(account_id.clone(), proxy_id.clone());

        // 持久化到配置文件
        self.persist_bindings().await;

        tracing::info!(
            "[ProxyPool] Bound account {} to proxy {}",
            account_id,
            proxy_id
        );
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
        self.account_bindings
            .get(account_id)
            .map(|v| v.value().clone())
    }

    /// 获取所有绑定关系的快照
    pub fn get_all_bindings_snapshot(&self) -> std::collections::HashMap<String, String> {
        self.account_bindings
            .iter()
            .map(|kv| (kv.key().clone(), kv.value().clone()))
            .collect()
    }

    /// [HOT-RELOAD] Re-sync the in-memory DashMap from `config.account_bindings`.
    /// Called after `update_proxy_pool` so that a wholesale ProxyPoolConfig
    /// replacement (e.g. via `save_config`) does not leave the in-memory
    /// bindings stale or empty.
    pub async fn sync_bindings_from_config(&self) {
        let config = self.config.read().await;
        let snapshot = config.account_bindings.clone();
        drop(config);

        // Reset the DashMap: clear old entries, then insert fresh ones.
        self.account_bindings.clear();
        for (account_id, proxy_id) in &snapshot {
            self.account_bindings
                .insert(account_id.clone(), proxy_id.clone());
        }
        tracing::info!(
            "[ProxyPool] Re-synced {} account bindings from config (hot-reload)",
            snapshot.len()
        );
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

    /// 批量检查代理健康状态
    pub async fn health_check(&self) -> Result<(), String> {
        let proxies_to_check: Vec<ProxyEntry> = {
            let config = self.config.read().await;
            config
                .proxies
                .iter()
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
        const DEFAULT_HEALTH_CHECK_URL: &str = "https://cp.cloudflare.com/generate_204";

        let check_url = if let Some(url) = &entry.health_check_url {
            if url.trim().is_empty() {
                DEFAULT_HEALTH_CHECK_URL
            } else {
                url.as_str()
            }
        } else {
            DEFAULT_HEALTH_CHECK_URL
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
            .connect_timeout(Duration::from_secs(10))
            .pool_idle_timeout(Duration::from_secs(45))
            .timeout(Duration::from_secs(15))
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36")
            .build();

        let client = match client_result {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Proxy {} build client failed: {}", entry.url, e);
                return (false, None);
            }
        };

        let start = std::time::Instant::now();
        match client.get(check_url).send().await {
            Ok(resp) => {
                let latency = start.elapsed().as_millis() as u64;
                if resp.status().is_success() {
                    (true, Some(latency))
                } else {
                    tracing::warn!(
                        "Proxy {} health check status error: {}",
                        entry.url,
                        resp.status()
                    );
                    (false, None)
                }
            }
            Err(e) => {
                tracing::warn!("Proxy {} health check request failed: {}", entry.url, e);
                (false, None)
            }
        }
    }

    /// 启动健康检查循环
    pub fn start_health_check_loop(self: Arc<Self>) {
        tokio::spawn(async move {
            tracing::info!("Starting proxy pool health check loop...");
            loop {
                // Perform check only if enabled and silent mode is off
                let enabled = self.config.read().await.enabled;
                if enabled && !crate::modules::config::is_silent_mode_enabled() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::config::ProxyAuth;

    #[test]
    fn test_build_proxy_config_with_explicit_auth() {
        let pool = ProxyPoolManager::new(Arc::new(RwLock::new(ProxyPoolConfig::default())));
        let entry = ProxyEntry {
            id: "p1".to_string(),
            name: "test".to_string(),
            url: "http://127.0.0.1:8080".to_string(),
            auth: Some(ProxyAuth {
                username: "user".to_string(),
                password: "pass".to_string(),
            }),
            enabled: true,
            priority: 1,
            tags: vec![],
            max_accounts: None,
            health_check_url: None,
            last_check_time: None,
            is_healthy: true,
            latency: None,
        };

        let res = pool.build_proxy_config(&entry);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().entry_id, "p1");
    }

    #[test]
    fn test_build_proxy_config_with_url_auth() {
        let pool = ProxyPoolManager::new(Arc::new(RwLock::new(ProxyPoolConfig::default())));
        let entry = ProxyEntry {
            id: "p2".to_string(),
            name: "test_url_auth".to_string(),
            url: "http://user:pass@127.0.0.1:10080".to_string(),
            auth: None,
            enabled: true,
            priority: 1,
            tags: vec![],
            max_accounts: None,
            health_check_url: None,
            last_check_time: None,
            is_healthy: true,
            latency: None,
        };

        let res = pool.build_proxy_config(&entry);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().entry_id, "p2");
    }

    #[test]
    fn test_build_proxy_config_fallback_to_url_auth_on_decrypt_failure() {
        let pool = ProxyPoolManager::new(Arc::new(RwLock::new(ProxyPoolConfig::default())));
        let entry = ProxyEntry {
            id: "p3".to_string(),
            name: "test_fallback_auth".to_string(),
            url: "http://url_user:url_pass@127.0.0.1:10080".to_string(),
            auth: Some(ProxyAuth {
                username: "struct_user".to_string(),
                password: "ag_enc_v2_failed_decrypt_payload".to_string(),
            }),
            enabled: true,
            priority: 1,
            tags: vec![],
            max_accounts: None,
            health_check_url: None,
            last_check_time: None,
            is_healthy: true,
            latency: None,
        };

        let res = pool.build_proxy_config(&entry);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().entry_id, "p3");
    }

    #[test]
    fn test_parse_and_clean_proxy_url_strips_both_user_and_pass() {
        let raw = "http://myuser:mypassword@127.0.0.1:10080";
        let (clean_url, auth) = ProxyPoolManager::parse_and_clean_proxy_url(raw);
        assert_eq!(clean_url, "http://127.0.0.1:10080/");
        assert_eq!(auth, Some(("myuser".to_string(), "mypassword".to_string())));
    }

    #[test]
    fn test_parse_and_clean_proxy_url_socks5h() {
        let raw = "socks5h://socks_user:socks_pass@127.0.0.1:1080";
        let (clean_url, auth) = ProxyPoolManager::parse_and_clean_proxy_url(raw);
        assert_eq!(clean_url, "socks5h://127.0.0.1:1080");
        assert_eq!(
            auth,
            Some(("socks_user".to_string(), "socks_pass".to_string()))
        );
    }

    #[test]
    fn test_build_proxy_config_blocks_ciphertext_when_no_url_auth() {
        let pool = ProxyPoolManager::new(Arc::new(RwLock::new(ProxyPoolConfig::default())));
        let entry = ProxyEntry {
            id: "p_cipher".to_string(),
            name: "test_cipher".to_string(),
            url: "http://127.0.0.1:10080".to_string(),
            auth: Some(ProxyAuth {
                username: "struct_user".to_string(),
                password: "ag_enc_v2_broken_key_payload".to_string(),
            }),
            enabled: true,
            priority: 1,
            tags: vec![],
            max_accounts: None,
            health_check_url: None,
            last_check_time: None,
            is_healthy: true,
            latency: None,
        };

        let res = pool.build_proxy_config(&entry);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().entry_id, "p_cipher");
    }

    #[tokio::test]
    async fn test_bound_account_prevents_direct_leak_when_unhealthy() {
        let pool = ProxyPoolManager::new(Arc::new(RwLock::new(ProxyPoolConfig {
            enabled: true,
            auto_failover: true,
            proxies: vec![ProxyEntry {
                id: "bound_p1".to_string(),
                name: "test_bound".to_string(),
                url: "http://127.0.0.1:8080".to_string(),
                auth: None,
                enabled: true,
                priority: 1,
                tags: vec![],
                max_accounts: None,
                health_check_url: None,
                last_check_time: None,
                is_healthy: false, // Mark proxy as unhealthy
                latency: None,
            }],
            ..Default::default()
        })));

        let _ = pool
            .bind_account_to_proxy("acc_test".to_string(), "bound_p1".to_string())
            .await;

        let res = pool.get_proxy_for_account("acc_test").await;
        assert!(res.is_ok(), "Expected Ok, got {:?}", res);
        let proxy_opt = res.unwrap();
        assert!(
            proxy_opt.is_some(),
            "Bound account must NOT silently fall back to None (Direct leak)"
        );
        assert_eq!(proxy_opt.unwrap().entry_id, "bound_p1");
    }

    #[tokio::test]
    async fn test_bound_account_fails_over_to_healthy_pool_proxy() {
        let pool = ProxyPoolManager::new(Arc::new(RwLock::new(ProxyPoolConfig {
            enabled: true,
            auto_failover: true,
            proxies: vec![
                ProxyEntry {
                    id: "bound_p1".to_string(),
                    name: "test_bound".to_string(),
                    url: "http://127.0.0.1:8080".to_string(),
                    auth: None,
                    enabled: true,
                    priority: 1,
                    tags: vec![],
                    max_accounts: None,
                    health_check_url: None,
                    last_check_time: None,
                    is_healthy: false, // unhealthy
                    latency: None,
                },
                ProxyEntry {
                    id: "pool_p2".to_string(),
                    name: "test_pool".to_string(),
                    url: "http://127.0.0.1:8081".to_string(),
                    auth: None,
                    enabled: true,
                    priority: 1,
                    tags: vec![],
                    max_accounts: None,
                    health_check_url: None,
                    last_check_time: None,
                    is_healthy: true, // healthy
                    latency: None,
                },
            ],
            ..Default::default()
        })));

        let _ = pool
            .bind_account_to_proxy("acc_test".to_string(), "bound_p1".to_string())
            .await;

        let res = pool.get_proxy_for_account("acc_test").await;
        assert!(res.is_ok());
        let proxy_opt = res.unwrap();
        assert!(proxy_opt.is_some());
        assert_eq!(proxy_opt.unwrap().entry_id, "pool_p2");
    }

    fn make_test_proxy_entry(id: &str, url: &str, enabled: bool, is_healthy: bool) -> ProxyEntry {
        ProxyEntry {
            id: id.to_string(),
            name: format!("node-{}", id),
            url: url.to_string(),
            auth: None,
            enabled,
            priority: 1,
            tags: vec![],
            max_accounts: None,
            health_check_url: None,
            last_check_time: None,
            is_healthy,
            latency: None,
        }
    }

    #[tokio::test]
    async fn test_strict_mode_prevents_failover_and_rejects_disabled_or_missing_bound_proxy() {
        let _guard = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let pool = ProxyPoolManager::new(Arc::new(RwLock::new(ProxyPoolConfig {
            enabled: true,
            auto_failover: true,
            strict_mode: true,
            proxies: vec![
                make_test_proxy_entry("bound_unhealthy", "http://127.0.0.1:18080", true, false),
                make_test_proxy_entry("pool_healthy", "http://127.0.0.1:18081", true, true),
                make_test_proxy_entry("bound_disabled", "http://127.0.0.1:18082", false, true),
            ],
            ..Default::default()
        })));

        // 1. Unhealthy bound proxy must remain pinned under strict_mode even when auto_failover is true
        pool.bind_account_to_proxy("acc_strict_1".to_string(), "bound_unhealthy".to_string())
            .await
            .unwrap();
        let pinned = pool
            .get_proxy_for_account("acc_strict_1")
            .await
            .expect("should stay pinned cleanly without failover");
        assert_eq!(pinned.unwrap().entry_id, "bound_unhealthy");

        // 2. Disabled bound proxy must return explicit error under strict_mode
        pool.bind_account_to_proxy("acc_strict_2".to_string(), "bound_disabled".to_string())
            .await
            .unwrap();
        let disabled_err = pool
            .get_proxy_for_account("acc_strict_2")
            .await
            .unwrap_err();
        assert!(disabled_err.contains("Strict Proxy"));

        // 3. Missing bound proxy must return explicit error under strict_mode
        pool.account_bindings
            .insert("acc_strict_missing".to_string(), "deleted_node".to_string());
        let missing_err = pool
            .get_proxy_for_account("acc_strict_missing")
            .await
            .unwrap_err();
        assert!(missing_err.contains("Strict Proxy"));
    }

    #[tokio::test]
    async fn test_bind_account_allows_rebinding_same_account_at_max_accounts() {
        let _guard = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let mut entry = make_test_proxy_entry("capped_node", "http://127.0.0.1:18083", true, true);
        entry.max_accounts = Some(1);

        let pool = ProxyPoolManager::new(Arc::new(RwLock::new(ProxyPoolConfig {
            enabled: true,
            proxies: vec![entry],
            ..Default::default()
        })));

        pool.bind_account_to_proxy("acc_a".to_string(), "capped_node".to_string())
            .await
            .expect("first account bind within cap should succeed");

        // Re-binding same account must still succeed
        pool.bind_account_to_proxy("acc_a".to_string(), "capped_node".to_string())
            .await
            .expect("re-binding same account should not fail max_accounts check");

        // Second distinct account must still be rejected
        let second_err = pool
            .bind_account_to_proxy("acc_b".to_string(), "capped_node".to_string())
            .await
            .unwrap_err();
        assert!(second_err.contains("max accounts limit"));
    }

    #[tokio::test]
    async fn test_resolve_client_for_add_account_respects_strict_vs_fallback() {
        let pool_cfg = ProxyPoolConfig {
            enabled: true,
            auto_failover: true,
            strict_mode: false,
            proxies: vec![
                make_test_proxy_entry("p_ok", "http://127.0.0.1:19001", true, true),
                make_test_proxy_entry("p_disabled", "http://127.0.0.1:19002", false, true),
            ],
            ..Default::default()
        };

        // Strict mode + missing proxy -> explicit error
        let strict_missing = resolve_client_for_add_account_with_config(
            &AddAccountProxyOptions {
                proxy_id: Some("non_existent".to_string()),
                strict_proxy: true,
            },
            &pool_cfg,
            None,
            None,
            false,
            15,
        )
        .await;
        assert!(strict_missing.is_err());

        // Strict mode + disabled proxy -> explicit error
        let strict_disabled = resolve_client_for_add_account_with_config(
            &AddAccountProxyOptions {
                proxy_id: Some("p_disabled".to_string()),
                strict_proxy: true,
            },
            &pool_cfg,
            None,
            None,
            false,
            15,
        )
        .await;
        assert!(strict_disabled.is_err());

        // Non-strict mode + missing proxy -> falls back cleanly
        let non_strict = resolve_client_for_add_account_with_config(
            &AddAccountProxyOptions {
                proxy_id: Some("non_existent".to_string()),
                strict_proxy: false,
            },
            &pool_cfg,
            None,
            None,
            false,
            15,
        )
        .await
        .expect("non-strict missing proxy should fall back cleanly");
        assert!(!non_strict.strict);

        // Strict mode + valid enabled proxy -> resolved with proxy label
        let strict_ok = resolve_client_for_add_account_with_config(
            &AddAccountProxyOptions {
                proxy_id: Some("p_ok".to_string()),
                strict_proxy: true,
            },
            &pool_cfg,
            None,
            None,
            false,
            15,
        )
        .await
        .expect("valid enabled proxy under strict mode should succeed");
        assert!(strict_ok.strict);
        assert!(strict_ok.proxy_label.as_deref().unwrap().contains("p_ok"));
    }
}
