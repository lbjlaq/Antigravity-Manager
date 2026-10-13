// 上游客户端实现
// 基于高性能通讯接口封装

use dashmap::DashMap;
use once_cell::sync::Lazy;
use rquest::{header, Client, Response, StatusCode};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::Duration;

/// 端点降级尝试的记录信息
#[derive(Debug, Clone)]
pub struct FallbackAttemptLog {
    /// 尝试的端点 URL
    pub endpoint_url: String,
    /// HTTP 状态码 (网络错误时为 None)
    pub status: Option<u16>,
    /// 错误描述
    pub error: String,
}

/// 上游调用结果，包含响应和降级尝试记录
pub struct UpstreamCallResult {
    /// 最终的 HTTP 响应
    pub response: Response,
    /// 降级过程中失败的端点尝试记录 (成功时为空)
    pub fallback_attempts: Vec<FallbackAttemptLog>,
}

/// 邮箱脱敏：只显示前3位 + *** + @域名前2位 + ***
/// 例: "userexample@gmail.com" → "use***@gm***"
pub fn mask_email(email: &str) -> String {
    if let Some(at_pos) = email.find('@') {
        let local = &email[..at_pos];
        let domain = &email[at_pos + 1..];
        let local_prefix: String = local.chars().take(3).collect();
        let domain_prefix: String = domain.chars().take(2).collect();
        format!("{}***@{}***", local_prefix, domain_prefix)
    } else {
        // 不是合法邮箱格式，直接截取前5位
        let prefix: String = email.chars().take(5).collect();
        format!("{}***", prefix)
    }
}

/// [NEW] 错误日志脱敏：抹除报错信息中的 access_token, proxy_url 等敏感凭证
pub fn sanitize_error_for_log(error_text: &str) -> String {
    // 抹除常见敏感 key 的值
    let re = regex::Regex::new(r#"(?i)(access_token|refresh_token|id_token|authorization|api_key|secret|password|proxy_url|http_proxy|https_proxy)\s*[:=]\s*[^"'\\\s,}\]]+"#).unwrap();
    let redacted = re.replace_all(error_text, "$1=<redacted>");

    // 抹除 Bearer token
    let re_bearer = regex::Regex::new(r#"(?i)(bearer\s+)[^"'\\\s,}\]]+"#).unwrap();
    let redacted = re_bearer.replace_all(&redacted, "$1<redacted>");

    // 限制长度防止日志炸弹 (UTF-8 字符边界安全保护)
    if redacted.len() > 1000 {
        format!(
            "{}... (truncated)",
            crate::proxy::mappers::common_utils::safe_truncate_str(&redacted, 1000)
        )
    } else {
        redacted.into_owned()
    }
}

// Cloud Code v1internal endpoints (fallback order: Daily → Sandbox → Prod)
//
// Daily 优先 —— 它是官方 IDE 原生唯一主力端点（`language_server` 的启动参数即指向它），
// 稳定支持思维链与工具调用；Sandbox 为沙箱备用、Prod 为生产兜底，
// 后两者均易触发 Prod 环境的 429（Ref: Issue #1176, Issue #3523）。
//
// [FIX Issue #3525 / PR #3526] 顺序同时是**正确性**要求，不只是可用性偏好：
// 同一账号 / 模型 / 代理下，Sandbox 对部分地区返回**终止性 400**
// `User location is not supported for the API use.`，而 `should_try_next_endpoint`
// 只对 408 / 404 / 5xx 回退 → 400 不触发回退，故 Sandbox 排首位会让已验证可用的
// Daily 永远不被尝试。Sandbox 保留为回退项、回退判定规则不变 ——
// **不对所有 400 无条件重试**。
const V1_INTERNAL_BASE_URL_PROD: &str = "https://cloudcode-pa.googleapis.com/v1internal";
const V1_INTERNAL_BASE_URL_DAILY: &str = "https://daily-cloudcode-pa.googleapis.com/v1internal";
const V1_INTERNAL_BASE_URL_SANDBOX: &str =
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal";

pub const V1_INTERNAL_BASE_URL_FALLBACKS: [&str; 3] = [
    V1_INTERNAL_BASE_URL_DAILY, // 优先级 1: Daily (官方 IDE 原生唯一主力端点，稳定支持思维链与工具调用)
    V1_INTERNAL_BASE_URL_SANDBOX, // 优先级 2: Sandbox (沙箱备用；部分地区对合规账号返回终止性 400)
    V1_INTERNAL_BASE_URL_PROD,  // 优先级 3: Prod (生产兜底，易触发 429)
];

/// 提取端点的主机名或规范标识，使不同 API 路径（如 :loadCodeAssist、:fetchAvailableModels、v1internal）
/// 能共享底层主机的健康度状态，避免同一死端点在不同业务模块重复超时。
pub fn extract_endpoint_host(url_or_host: &str) -> &str {
    let s = if let Some(stripped) = url_or_host.strip_prefix("https://") {
        stripped
    } else if let Some(stripped) = url_or_host.strip_prefix("http://") {
        stripped
    } else {
        url_or_host
    };
    let host_part = s.split('/').next().unwrap_or(s);
    host_part.split(':').next().unwrap_or(host_part)
}

/// 判断 HTTP 状态码是否属于应当尝试下一个候选端点的临时/可重试故障。
pub fn should_try_next_endpoint(status: StatusCode) -> bool {
    UpstreamClient::should_try_next_endpoint(status)
}

/// 端点健康追踪器：记录故障端点与冷却期，动态优化上游端点 Fallback 顺序，避免无状态重复踩坑。
#[derive(Debug, Clone)]
pub struct EndpointHealthTracker {
    /// 记录最近一次故障的时间戳 (host -> Instant)
    failures: Arc<DashMap<String, std::time::Instant>>,
    /// 冷却观察期 (默认 60s)
    cooldown: std::time::Duration,
}

static GLOBAL_ENDPOINT_HEALTH: Lazy<EndpointHealthTracker> =
    Lazy::new(EndpointHealthTracker::default);

/// 获取全局端点健康追踪器实例（跨模块共享，包括主网关请求、项目解析 loadCodeAssist、配额查询）
pub fn get_global_endpoint_health() -> &'static EndpointHealthTracker {
    &GLOBAL_ENDPOINT_HEALTH
}

impl EndpointHealthTracker {
    pub fn new(cooldown: std::time::Duration) -> Self {
        Self {
            failures: Arc::new(DashMap::new()),
            cooldown,
        }
    }

    /// 标记端点调用失败（记录当前时间戳，以 host 聚合）
    pub fn mark_failure(&self, endpoint: &str) {
        let host = extract_endpoint_host(endpoint);
        let now = std::time::Instant::now();
        self.failures.insert(host.to_string(), now);
        tracing::warn!(
            host = %host,
            endpoint = %endpoint,
            cooldown_secs = ?self.cooldown.as_secs(),
            "Marked upstream endpoint host as unhealthy (entering cooldown)"
        );
    }

    /// 标记端点调用成功（清除故障标记，立即恢复健康）
    pub fn mark_success(&self, endpoint: &str) {
        let host = extract_endpoint_host(endpoint);
        if self.failures.remove(host).is_some() {
            tracing::info!(
                host = %host,
                endpoint = %endpoint,
                "Upstream endpoint host recovered and marked healthy"
            );
        }
    }

    /// 检查指定端点当前是否健康（无故障记录，或已超出冷却期）
    #[allow(dead_code)]
    pub fn is_healthy(&self, endpoint: &str) -> bool {
        let host = extract_endpoint_host(endpoint);
        if let Some(entry) = self.failures.get(host) {
            let last_fail = *entry.value();
            if last_fail.elapsed() >= self.cooldown {
                drop(entry);
                self.failures.remove(host);
                true
            } else {
                false
            }
        } else {
            true
        }
    }

    /// 根据各端点健康状况与冷却期，动态排列优先级顺序。
    /// - 健康端点排在最前，保持原有的相对优先级；
    /// - 处于冷却期内的端点排在后部，按故障发生时间由远及近（最早失败的相对排在前面，优先探活）；
    /// - 永远保留所有端点，确保在全故障极端情况下仍能兜底尝试。
    pub fn get_ordered_endpoints<'a>(&self, default_endpoints: &[&'a str]) -> Vec<&'a str> {
        let mut healthy = Vec::new();
        let mut unhealthy = Vec::new();

        for &ep in default_endpoints {
            let host = extract_endpoint_host(ep);
            if let Some(entry) = self.failures.get(host) {
                let last_fail = *entry.value();
                if last_fail.elapsed() >= self.cooldown {
                    drop(entry);
                    self.failures.remove(host);
                    healthy.push(ep);
                } else {
                    unhealthy.push((ep, last_fail));
                }
            } else {
                healthy.push(ep);
            }
        }

        // 不健康的端点：按照失败时间从早到晚排序（先失败的更早尝试探活）
        unhealthy.sort_by_key(|&(_, time)| time);

        let mut result = healthy;
        for (ep, _) in unhealthy {
            result.push(ep);
        }

        result
    }
}

impl Default for EndpointHealthTracker {
    fn default() -> Self {
        Self::new(std::time::Duration::from_secs(60))
    }
}

pub struct UpstreamClient {
    default_client: RwLock<Client>,
    proxy_pool: Option<Arc<crate::proxy::proxy_pool::ProxyPoolManager>>,
    client_cache: DashMap<String, Client>, // proxy_id -> Client
    user_agent_override: RwLock<Option<String>>,
    endpoint_health: EndpointHealthTracker,
}

impl UpstreamClient {
    pub fn new(
        proxy_config: Option<crate::proxy::config::UpstreamProxyConfig>,
        proxy_pool: Option<Arc<crate::proxy::proxy_pool::ProxyPoolManager>>,
    ) -> Self {
        let default_client = match Self::build_client_internal(proxy_config.clone()) {
            Ok(client) => client,
            Err(err_with_proxy) => {
                tracing::error!(
                    error = %err_with_proxy,
                    "Failed to create default HTTP client with configured upstream proxy; retrying without proxy"
                );
                match Self::build_client_internal(None) {
                    Ok(client) => client,
                    Err(err_without_proxy) => {
                        tracing::error!(
                            error = %err_without_proxy,
                            "Failed to create default HTTP client without proxy; falling back to bare client"
                        );
                        Client::new()
                    }
                }
            }
        };

        Self {
            default_client: RwLock::new(default_client),
            proxy_pool,
            client_cache: DashMap::new(),
            user_agent_override: RwLock::new(None),
            endpoint_health: get_global_endpoint_health().clone(),
        }
    }

    /// [HOT-RELOAD] Rebuild the default HTTP client using the supplied upstream
    /// proxy config. Called from `update_proxy` so changes to the upstream proxy
    /// take effect without restarting the app.
    pub async fn rebuild_default_client(
        &self,
        proxy_config: Option<crate::proxy::config::UpstreamProxyConfig>,
    ) {
        let new_client = match Self::build_client_internal(proxy_config.clone()) {
            Ok(c) => c,
            Err(err_with_proxy) => {
                tracing::error!(
                    error = %err_with_proxy,
                    "Hot-reload: failed to rebuild default HTTP client with configured upstream proxy; retrying without proxy"
                );
                match Self::build_client_internal(None) {
                    Ok(c) => c,
                    Err(err_without_proxy) => {
                        tracing::error!(
                            error = %err_without_proxy,
                            "Hot-reload: failed to rebuild default HTTP client without proxy; keeping previous client"
                        );
                        return;
                    }
                }
            }
        };
        let mut guard = self.default_client.write().await;
        *guard = new_client;
        tracing::info!("UpstreamClient default_client rebuilt (upstream proxy hot-reloaded)");
    }

    /// [HOT-RELOAD] Drop all per-proxy cached clients. Call after the pool
    /// configuration changes (proxy URL/credentials edited, proxy removed,
    /// bindings changed) so the next request rebuilds with fresh settings.
    pub fn clear_client_cache(&self) {
        let size = self.client_cache.len();
        self.client_cache.clear();
        if size > 0 {
            tracing::info!("UpstreamClient cleared {} cached per-proxy clients", size);
        }
    }

    /// Base client builder configured with common connection pool, timeouts, and TCP keep-alive
    fn base_client_builder() -> rquest::ClientBuilder {
        let builder = Client::builder()
            .emulation(rquest_util::Emulation::Chrome123)
            // Connection settings (优化连接复用，减少建立开销)
            // [OPTIMIZATION] 建连仅涉及 TCP/TLS 握手，从 20s 缩短至 5s，快速识别死端点，防止多级重试累积 40s 超时
            .connect_timeout(Duration::from_secs(5))
            .pool_max_idle_per_host(20) // 每主机最多 20 个空闲连接 (对齐官方指纹)
            // [FIX] 空闲连接从 90s 缩短至 45s，避免 GFE 在 45~60s 静默掐断空闲 TCP 后客户端复用死连接引发 error reading a body
            .pool_idle_timeout(Duration::from_secs(45))
            // [FIX #3593 / #3571] 彻底恢复 Chrome 123 原生协议行为：
            // 1. 实测证实 Google GFE 对客户端 HTTP/2 PING 帧静默丢弃、不回复 PING ACK，
            //    导致 hyper2 在 keep_alive_timeout 到期时本地自杀式掐死连接并抛出 error reading a body from connection；
            // 2. 移除破坏性的底层 HTTP/2 PING 配置，回归 4.8.8 稳态基线；
            // 3. 依赖标准 L4 TCP Keep-Alive (60s) 保活，下游已有 PR #3578 的 3s SSE 注释心跳防客户端超时。
            .tcp_keepalive(Duration::from_secs(60)) // TCP 稳态保活探测 (60秒)
            // 强制开启 HTTP/2 协议，并支持在 SOCKS/HTTPS 代理下通过 ALPN 强制降级/协商
            .timeout(Duration::from_secs(600));

        Self::apply_default_user_agent(builder)
    }

    /// Internal helper to build a client with optional upstream proxy config
    fn build_client_internal(
        proxy_config: Option<crate::proxy::config::UpstreamProxyConfig>,
    ) -> Result<Client, rquest::Error> {
        let mut builder = Self::base_client_builder();

        if let Some(config) = proxy_config {
            if config.enabled && !config.url.is_empty() {
                let url = crate::proxy::config::normalize_proxy_url(&config.url);
                if let Ok(proxy) = rquest::Proxy::all(&url) {
                    builder = builder.proxy(proxy);
                    tracing::info!("UpstreamClient enabled proxy: {}", url);
                }
            }
        }

        builder.build()
    }

    /// Build a client with a specific PoolProxyConfig (from ProxyPool)
    fn build_client_with_proxy(
        &self,
        proxy_config: crate::proxy::proxy_pool::PoolProxyConfig,
    ) -> Result<Client, rquest::Error> {
        // Reuse base settings similar to default client but with specific proxy
        let builder = Self::base_client_builder().proxy(proxy_config.proxy);
        builder.build()
    }

    fn apply_default_user_agent(builder: rquest::ClientBuilder) -> rquest::ClientBuilder {
        let ua = crate::constants::USER_AGENT.as_str();
        if header::HeaderValue::from_str(ua).is_ok() {
            builder.user_agent(ua)
        } else {
            tracing::warn!(
                user_agent = %ua,
                "Invalid default User-Agent value, using fallback"
            );
            builder.user_agent("antigravity")
        }
    }

    /// Set dynamic User-Agent override
    pub async fn set_user_agent_override(&self, ua: Option<String>) {
        let mut lock = self.user_agent_override.write().await;
        *lock = ua;
        tracing::debug!("UpstreamClient User-Agent override updated: {:?}", lock);
    }

    /// Get current User-Agent (sanitized with safety floor >= 4.3.0)
    pub async fn get_user_agent(&self) -> String {
        let ua_override = self.user_agent_override.read().await;
        match ua_override.as_ref() {
            Some(ua) => crate::constants::sanitize_egress_user_agent(ua),
            None => crate::constants::USER_AGENT.clone(),
        }
    }

    /// Get client for a specific account (or default if no proxy bound)
    pub async fn get_client(&self, account_id: Option<&str>) -> Client {
        if let Some(pool) = &self.proxy_pool {
            if let Some(acc_id) = account_id {
                // Try to get per-account proxy
                match pool.get_proxy_for_account(acc_id).await {
                    Ok(Some(proxy_cfg)) => {
                        // Check cache
                        if let Some(client) = self.client_cache.get(&proxy_cfg.entry_id) {
                            return client.clone();
                        }
                        // Build new client and cache it
                        match self.build_client_with_proxy(proxy_cfg.clone()) {
                            Ok(client) => {
                                self.client_cache
                                    .insert(proxy_cfg.entry_id.clone(), client.clone());
                                tracing::info!(
                                    "Using ProxyPool proxy ID: {} for account: {}",
                                    proxy_cfg.entry_id,
                                    acc_id
                                );
                                return client;
                            }
                            Err(e) => {
                                tracing::error!("Failed to build client for proxy {}: {}, falling back to default", proxy_cfg.entry_id, e);
                            }
                        }
                    }
                    Ok(None) => {
                        // No proxy found or required for this account, use default
                    }
                    Err(e) => {
                        tracing::error!(
                            "Error getting proxy for account {}: {}, falling back to default",
                            acc_id,
                            e
                        );
                    }
                }
            }
        }
        // Fallback to default client
        self.default_client.read().await.clone()
    }

    /// Build v1internal URL
    fn build_url(base_url: &str, method: &str, query_string: Option<&str>) -> String {
        if let Some(qs) = query_string {
            format!("{}:{}?{}", base_url, method, qs)
        } else {
            format!("{}:{}", base_url, method)
        }
    }

    /// Determine if we should try next endpoint (fallback logic)
    pub fn should_try_next_endpoint(status: StatusCode) -> bool {
        status == StatusCode::REQUEST_TIMEOUT
            || status == StatusCode::NOT_FOUND
            || status.is_server_error()
    }

    /// Call v1internal API (Basic Method)
    ///
    /// Initiates a basic network request, supporting multi-endpoint auto-fallback.
    /// [UPDATED] Takes optional account_id for per-account proxy selection.
    pub async fn call_v1_internal(
        &self,
        method: &str,
        access_token: &str,
        body: Value,
        query_string: Option<&str>,
        account_id: Option<&str>, // [NEW] Account ID for proxy selection
    ) -> Result<UpstreamCallResult, String> {
        self.call_v1_internal_with_headers(
            method,
            access_token,
            body,
            query_string,
            std::collections::HashMap::new(),
            account_id,
        )
        .await
    }

    /// [FIX #765] 调用 v1internal API，支持透传额外的 Headers
    /// [ENHANCED] 返回 UpstreamCallResult，包含降级尝试记录，用于 debug 日志
    pub async fn call_v1_internal_with_headers(
        &self,
        method: &str,
        access_token: &str,
        mut body: Value,
        query_string: Option<&str>,
        extra_headers: std::collections::HashMap<String, String>,
        account_id: Option<&str>, // [NEW] Account ID
    ) -> Result<UpstreamCallResult, String> {
        // [DEFENSE] 全局终极防御拦截：净化所有发往上游报文中的损坏/空 inlineData 以及触发 Google WAF 拦截的违规计费元数据，并最终统一对齐官方信封与前缀拓扑
        if let Some(inner) = body.get_mut("request") {
            crate::proxy::mappers::common_utils::sanitize_gemini_payload_inline_data(inner);
            crate::proxy::mappers::prompt_sanitizer::PromptSanitizer::sanitize_gemini_payload(
                inner,
            );
            crate::proxy::mappers::common_utils::ensure_gemini_payload_ends_with_user(inner);
        } else {
            crate::proxy::mappers::common_utils::sanitize_gemini_payload_inline_data(&mut body);
            crate::proxy::mappers::prompt_sanitizer::PromptSanitizer::sanitize_gemini_payload(
                &mut body,
            );
            crate::proxy::mappers::common_utils::ensure_gemini_payload_ends_with_user(&mut body);
        }
        crate::proxy::pipeline::InboundThinkingPipeline::align_official_envelope(&mut body);

        // [NEW] Get client based on account (cached in proxy pool manager)
        let client = self.get_client(account_id).await;

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

        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_str(&self.get_user_agent().await).unwrap_or_else(|e| {
                tracing::warn!("Invalid User-Agent header value, using fallback: {}", e);
                header::HeaderValue::from_str(crate::constants::USER_AGENT.as_str())
                    .unwrap_or_else(|_| header::HeaderValue::from_static("antigravity"))
            }),
        );

        // Keep body.project for content requests, but omit the quota-project header.
        let is_content_request = matches!(method, "generateContent" | "streamGenerateContent");
        if !is_content_request {
            if let Some(proj) = body.get("project").and_then(|v| v.as_str()) {
                if !proj.is_empty() && proj != "test-project" && proj != "project-id" {
                    if let Ok(hv) = header::HeaderValue::from_str(proj) {
                        headers.insert("x-goog-user-project", hv);
                    }
                }
            }
        }

        // 注入业务透传 Headers (如 anthropic-beta)
        // 严格过滤客户端特征伪头，确保出站请求头 100% 对齐官方 Antigravity Hub
        for (k, v) in extra_headers {
            let k_lower = k.to_ascii_lowercase();
            if k_lower == "user-agent"
                || k_lower == "x-session-id"
                || k_lower == "x-client-name"
                || k_lower == "x-client-version"
                || k_lower == "x-machine-id"
                || k_lower == "x-vscode-sessionid"
                || k_lower.starts_with("x-jeikcode")
            {
                continue;
            }
            if let Ok(hk) = header::HeaderName::from_bytes(k.as_bytes()) {
                if let Ok(hv) = header::HeaderValue::from_str(&v) {
                    headers.insert(hk, hv);
                }
            }
        }
        if is_content_request {
            headers.remove("x-goog-user-project");
        }

        // [PIPELINE ALIGNMENT] 统一对齐官方上游特权头：所有 Claude 系列模型出站统一注入 anthropic-beta 声明
        let target_model_str = body
            .get("model")
            .and_then(|v| v.as_str())
            .or_else(|| {
                body.get("request")
                    .and_then(|r| r.get("model"))
                    .and_then(|v| v.as_str())
            })
            .unwrap_or("");
        if target_model_str.to_lowercase().contains("claude")
            && !headers.contains_key("anthropic-beta")
        {
            headers.insert(
                header::HeaderName::from_static("anthropic-beta"),
                header::HeaderValue::from_static("claude-code-20250219"),
            );
        }

        // [DEBUG] Log headers for verification
        tracing::debug!(?headers, "Final Upstream Request Headers");

        let _ = crate::proxy::monitor::CURRENT_UPSTREAM_CAPTURE.try_with(|holder| {
            let pairs: Vec<(&str, &str)> = headers
                .iter()
                .filter_map(|(k, v)| v.to_str().ok().map(|s| (k.as_str(), s)))
                .collect();
            holder.set_headers_json(crate::proxy::payload_audit::header_pairs_to_redacted_json(
                pairs,
            ));
        });

        let mut has_triggered_downgrade = false;

        // [TEMPORARY FIX #3074] 针对 403 SERVICE_DISABLED 的自动降级重试逻辑
        // 我们包装一层循环，以便在检测到特定错误时移除 Header 并重试
        loop {
            let mut last_err: Option<String> = None;
            let mut fallback_attempts: Vec<FallbackAttemptLog> = Vec::new();
            let mut should_retry_without_header = false;

            // 动态健康度排序：优先尝试健康端点，跳过处于冷却期的故障端点，避免无状态重复累积 40s 超时
            let candidate_endpoints = self
                .endpoint_health
                .get_ordered_endpoints(&V1_INTERNAL_BASE_URL_FALLBACKS);

            // 遍历候选端点，失败时自动切换
            for (idx, base_url) in candidate_endpoints.iter().enumerate() {
                let url = Self::build_url(base_url, method, query_string);
                let has_next = idx + 1 < candidate_endpoints.len();

                let body_bytes = serde_json::to_vec(&body).map_err(|e| e.to_string())?;

                let mut req_builder = client.post(&url).headers(headers.clone());

                // [FIX] 仅对流式接口 (streamGenerateContent) 使用分块传输仿真
                // 对其他接口 (如 generateContent, loadCodeAssist) 发送正常的固定长度 Body
                // 否则图像生成会因为缺少 Content-Length 而被 Google 服务端拒绝或限流 (429)
                if method == "streamGenerateContent" {
                    let stream_bytes = body_bytes.clone();
                    req_builder = req_builder.body(rquest::Body::wrap_stream(
                        futures::stream::once(async move { Ok::<_, std::io::Error>(stream_bytes) }),
                    ));
                } else {
                    req_builder = req_builder.body(body_bytes.clone());
                }

                let response = req_builder.send().await;

                match response {
                    Ok(resp) => {
                        let status = resp.status();
                        if status.is_success() {
                            self.endpoint_health.mark_success(base_url);
                            if idx > 0 {
                                tracing::info!(
                                    "✓ Upstream fallback succeeded | Endpoint: {} | Status: {} | Next endpoints available: {}",
                                    base_url,
                                    status,
                                    candidate_endpoints.len() - idx - 1
                                );
                            } else {
                                tracing::debug!(
                                    "✓ Upstream request succeeded | Endpoint: {} | Status: {}",
                                    base_url,
                                    status
                                );
                            }
                            return Ok(UpstreamCallResult {
                                response: resp,
                                fallback_attempts,
                            });
                        }

                        // [NEW] 检测 403 错误 (Issue #3074)
                        // 只要带有项目 Header 且返回 403，我们就尝试降级重试一次
                        if status == StatusCode::FORBIDDEN
                            && !has_triggered_downgrade
                            && headers.contains_key("x-goog-user-project")
                        {
                            tracing::warn!(
                                "Detected 403 Forbidden with project header, retrying WITHOUT x-goog-user-project header (Account: {:?})",
                                account_id
                            );
                            should_retry_without_header = true;
                            break;
                        }

                        // 如果有下一个端点且当前错误可重试，则切换
                        if has_next && Self::should_try_next_endpoint(status) {
                            self.endpoint_health.mark_failure(base_url);
                            let err_msg = format!("Upstream {} returned {}", base_url, status);
                            tracing::warn!(
                                "Upstream endpoint returned {} at {} (method={}), trying next endpoint",
                                status,
                                base_url,
                                method
                            );
                            // [NEW] 记录降级尝试
                            fallback_attempts.push(FallbackAttemptLog {
                                endpoint_url: url.clone(),
                                status: Some(status.as_u16()),
                                error: err_msg.clone(),
                            });
                            last_err = Some(err_msg);
                            continue;
                        }

                        // 不可重试的错误或已是最后一个端点，直接返回
                        return Ok(UpstreamCallResult {
                            response: resp,
                            fallback_attempts,
                        });
                    }
                    Err(e) => {
                        self.endpoint_health.mark_failure(base_url);
                        let msg = format!("HTTP request failed at {}: {}", base_url, e);
                        tracing::debug!("{}", msg);
                        // [NEW] 记录网络错误的降级尝试
                        fallback_attempts.push(FallbackAttemptLog {
                            endpoint_url: url.clone(),
                            status: None,
                            error: msg.clone(),
                        });
                        last_err = Some(msg);

                        // 如果是最后一个端点，退出循环
                        if !has_next {
                            break;
                        }
                        continue;
                    }
                }
            }

            // 处理降级逻辑
            if should_retry_without_header {
                headers.remove("x-goog-user-project");
                has_triggered_downgrade = true;
                // 重启外层 loop，从第一个端点再次尝试
                continue;
            }

            // 如果没有触发降级且所有端点都尝试过，返回最后的错误
            let final_err = last_err.unwrap_or_else(|| "All endpoints failed".to_string());
            tracing::error!(
                error = %final_err,
                account = ?account_id,
                method = %method,
                "Upstream network request failed across all endpoints (non-server fault)"
            );
            return Err(final_err);
        }
    }

    /// 获取可用模型列表
    ///
    /// 获取远端模型列表，支持多端点自动 Fallback
    #[allow(dead_code)] // API ready for future model discovery feature
    pub async fn fetch_available_models(
        &self,
        access_token: &str,
        account_id: Option<&str>,
    ) -> Result<Value, String> {
        // 复用 call_v1_internal，然后解析 JSON
        let result = self
            .call_v1_internal(
                "fetchAvailableModels",
                access_token,
                serde_json::json!({}),
                None,
                account_id,
            )
            .await?;
        let json: Value = result
            .response
            .json()
            .await
            .map_err(|e| format!("Parse json failed: {}", e))?;
        Ok(json)
    }

    /// 辅助型 v1internal 调用（非用户请求路径，例如后台上下文摘要）。
    ///
    /// 为什么需要它：这类调用点位于 mapper / 辅助函数里，历史上拿不到 `AppState.upstream`
    /// 便手写 URL —— 结果形状与 host 双双漂移
    /// （`{host}/v1internal/projects/{p}/locations/global/models/{m}:generateContent`
    /// 在 daily / sandbox / prod 三个 host 上一律返回 HTML 404，导致该功能从未成功过）。
    ///
    /// 这里复用与主请求路径**完全相同**的四个来源，避免再次漂移：
    /// - 客户端 `self.get_client(account_id)` —— 同一套按账号代理池选择与 `client_cache`，
    ///   并随上游代理热更新一并生效（见 `rebuild_default_client` / `clear_client_cache`）。
    ///   账号绑定专属代理时辅助请求同样走它，不会从真实 IP 泄漏出去。
    /// - 端点顺序 `V1_INTERNAL_BASE_URL_FALLBACKS`（Daily → Sandbox → Prod）
    /// - URL 形状 `Self::build_url`（`{base}:{method}`，模型名放在 body 里）
    /// - 回退判定 `Self::should_try_next_endpoint`（408 / 404 / 5xx 才换端点）
    ///
    /// 与 `call_v1_internal` 的差异（有意为之）：不注入请求级 Header
    /// （`x-vscode-sessionid` 等）。超时由调用方用 `timeout_secs` 指定 ——
    /// 客户端默认 600s，对摘要过长，故显式收紧。
    pub async fn call_v1_internal_auxiliary(
        &self,
        method: &str,
        access_token: &str,
        mut body: Value,
        account_id: Option<&str>,
        timeout_secs: u64,
    ) -> Result<Value, String> {
        crate::proxy::pipeline::InboundThinkingPipeline::align_official_envelope(&mut body);
        let client = self.get_client(account_id).await;
        let mut last_error = String::new();

        let candidate_endpoints = self
            .endpoint_health
            .get_ordered_endpoints(&V1_INTERNAL_BASE_URL_FALLBACKS);

        for base_url in candidate_endpoints.iter() {
            let url = Self::build_url(base_url, method, None);

            let response = match client
                .post(&url)
                .header("Authorization", format!("Bearer {}", access_token))
                .header("Content-Type", "application/json")
                .timeout(Duration::from_secs(timeout_secs))
                .json(&body)
                .send()
                .await
            {
                Ok(response) => response,
                Err(e) => {
                    self.endpoint_health.mark_failure(base_url);
                    tracing::warn!(
                        endpoint = %url,
                        error = %e,
                        "Auxiliary v1internal request failed, trying next endpoint"
                    );
                    last_error = format!("request to {} failed: {}", url, e);
                    continue;
                }
            };

            let status = response.status();
            if !status.is_success() {
                let text = response.text().await.unwrap_or_default();
                last_error = format!("{} returned {}: {}", url, status, text);

                // 与主请求路径同一判定：仅 408 / 404 / 5xx 换端点；
                // 其余状态（如 400）说明请求本身有问题，直接终止，不做三倍重试。
                if !Self::should_try_next_endpoint(status) {
                    tracing::error!(
                        error = %last_error,
                        "Auxiliary v1internal request failed with non-retryable status (non-server fault)"
                    );
                    return Err(last_error);
                }
                self.endpoint_health.mark_failure(base_url);
                tracing::warn!(
                    endpoint = %url,
                    status = %status,
                    "Auxiliary v1internal request returned retryable status, trying next endpoint"
                );
                continue;
            }

            self.endpoint_health.mark_success(base_url);
            return response
                .json()
                .await
                .map_err(|e| format!("failed to parse response from {}: {}", url, e));
        }

        let final_err = if last_error.is_empty() {
            "no v1internal endpoint available".to_string()
        } else {
            last_error
        };
        tracing::error!(
            error = %final_err,
            "Auxiliary v1internal request failed across all endpoints (non-server fault)"
        );
        Err(final_err)
    }
}

/// 派生确定性 UUID 格式的客户端窗口 Session ID (RFC 4122 v4 格式)
#[allow(dead_code)]
fn derive_session_uuid(seed: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"antigravity-session-v1:");
    hasher.update(seed.as_bytes());
    let hash = hasher.finalize();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-4{:01x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        hash[0], hash[1], hash[2], hash[3],
        hash[4], hash[5],
        hash[6] & 0x0f, hash[7],
        (hash[8] & 0x3f) | 0x80, hash[9],
        hash[10], hash[11], hash[12], hash[13], hash[14], hash[15]
    )
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

    #[test]
    fn test_endpoint_health_all_healthy_preserves_order() {
        let tracker = EndpointHealthTracker::new(Duration::from_secs(60));
        let endpoints = ["ep1", "ep2", "ep3"];
        let ordered = tracker.get_ordered_endpoints(&endpoints);
        assert_eq!(ordered, vec!["ep1", "ep2", "ep3"]);
    }

    #[test]
    fn test_endpoint_health_single_failure_moves_to_back() {
        let tracker = EndpointHealthTracker::new(Duration::from_secs(60));
        let endpoints = ["daily", "sandbox", "prod"];
        tracker.mark_failure("daily");

        let ordered = tracker.get_ordered_endpoints(&endpoints);
        assert_eq!(ordered, vec!["sandbox", "prod", "daily"]);
    }

    #[test]
    fn test_endpoint_health_two_failures_prioritizes_healthy() {
        let tracker = EndpointHealthTracker::new(Duration::from_secs(60));
        let endpoints = ["daily", "sandbox", "prod"];
        tracker.mark_failure("daily");
        tracker.mark_failure("sandbox");

        let ordered = tracker.get_ordered_endpoints(&endpoints);
        assert_eq!(ordered[0], "prod");
        assert_eq!(ordered.len(), 3);
        assert!(ordered.contains(&"daily"));
        assert!(ordered.contains(&"sandbox"));
    }

    #[test]
    fn test_endpoint_health_success_clears_failure() {
        let tracker = EndpointHealthTracker::new(Duration::from_secs(60));
        let endpoints = ["daily", "sandbox", "prod"];
        tracker.mark_failure("daily");
        assert_eq!(
            tracker.get_ordered_endpoints(&endpoints),
            vec!["sandbox", "prod", "daily"]
        );

        tracker.mark_success("daily");
        assert_eq!(
            tracker.get_ordered_endpoints(&endpoints),
            vec!["daily", "sandbox", "prod"]
        );
    }

    #[test]
    fn test_endpoint_health_all_failed_preserves_all_endpoints() {
        let tracker = EndpointHealthTracker::new(Duration::from_secs(60));
        let endpoints = ["daily", "sandbox", "prod"];
        tracker.mark_failure("daily");
        tracker.mark_failure("sandbox");
        tracker.mark_failure("prod");

        let ordered = tracker.get_ordered_endpoints(&endpoints);
        assert_eq!(ordered.len(), 3);
        assert!(ordered.contains(&"daily"));
        assert!(ordered.contains(&"sandbox"));
        assert!(ordered.contains(&"prod"));
    }

    #[test]
    fn test_endpoint_health_cooldown_expiry() {
        let tracker = EndpointHealthTracker::new(Duration::from_millis(10));
        let endpoints = ["daily", "sandbox", "prod"];
        tracker.mark_failure("daily");
        assert_eq!(
            tracker.get_ordered_endpoints(&endpoints),
            vec!["sandbox", "prod", "daily"]
        );

        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(
            tracker.get_ordered_endpoints(&endpoints),
            vec!["daily", "sandbox", "prod"]
        );
    }

    #[test]
    fn test_extract_endpoint_host() {
        assert_eq!(
            extract_endpoint_host("https://daily-cloudcode-pa.googleapis.com/v1internal"),
            "daily-cloudcode-pa.googleapis.com"
        );
        assert_eq!(
            extract_endpoint_host(
                "https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist"
            ),
            "daily-cloudcode-pa.googleapis.com"
        );
        assert_eq!(
            extract_endpoint_host(
                "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:fetchAvailableModels"
            ),
            "daily-cloudcode-pa.sandbox.googleapis.com"
        );
        assert_eq!(
            extract_endpoint_host(
                "https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse"
            ),
            "cloudcode-pa.googleapis.com"
        );
        assert_eq!(
            extract_endpoint_host("daily-cloudcode-pa.googleapis.com"),
            "daily-cloudcode-pa.googleapis.com"
        );
    }

    #[test]
    fn test_endpoint_health_cross_api_sharing() {
        let tracker = EndpointHealthTracker::new(Duration::from_secs(60));
        let base_endpoints = [
            "https://daily-cloudcode-pa.googleapis.com/v1internal",
            "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal",
            "https://cloudcode-pa.googleapis.com/v1internal",
        ];
        let load_project_endpoints = [
            "https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
            "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:loadCodeAssist",
            "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
        ];

        // 模拟 loadCodeAssist 发现 daily 和 sandbox 故障
        tracker.mark_failure("https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist");
        tracker.mark_failure(
            "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:loadCodeAssist",
        );

        // 验证主聊天请求端点立即动态重排，优先走健康的 prod
        let ordered_base = tracker.get_ordered_endpoints(&base_endpoints);
        assert_eq!(
            ordered_base[0],
            "https://cloudcode-pa.googleapis.com/v1internal"
        );

        // 验证其他 API (如 loadCodeAssist 本身) 也同步重排
        let ordered_load = tracker.get_ordered_endpoints(&load_project_endpoints);
        assert_eq!(
            ordered_load[0],
            "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist"
        );

        // 模拟 prod 成功
        tracker.mark_success("https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist");
        assert!(tracker.is_healthy("https://cloudcode-pa.googleapis.com/v1internal"));
    }
}
