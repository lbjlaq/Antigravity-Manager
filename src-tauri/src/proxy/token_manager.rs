// 移除冗余的顶层导入，因为这些在代码中已由 full path 或局部导入处理
use axum::http::StatusCode;
use dashmap::DashMap;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use tokio_util::sync::CancellationToken;

use crate::proxy::rate_limit::RateLimitTracker;
use crate::proxy::server::{ImagePermit, ImageScheduler};
use crate::proxy::sticky_config::StickySessionConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnDiskAccountState {
    Enabled,
    Disabled,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackerParserMode {
    Current,
    Baseline,
}

fn classify_rate_limit_reason(error_body: &str) -> crate::proxy::rate_limit::RateLimitReason {
    use crate::proxy::rate_limit::RateLimitReason;

    // 网关自答错误不来自任何账号：一律视为 Unknown，
    // 避免 “no quota accounts” 因包含 quota 关键词被误判为真实配额耗尽而连坐账号。
    if crate::proxy::common::model_gate::is_gateway_self_answer(error_body) {
        return RateLimitReason::Unknown;
    }

    let body = error_body.to_lowercase();
    let generic_resource_exhausted =
        body.contains("resource has been exhausted") || body.contains("resource_exhausted");
    let explicit_quota_exhausted = body.contains("quota_exhausted")
        || body.contains("quotaresetdelay")
        || body.contains("quota reset")
        || body.contains("quota limit")
        || body.contains("per day")
        || body.contains("daily quota")
        || body.contains("credits")
        || body.contains("individual quota")
        || body.contains("resets in");

    if body.contains("model_capacity") {
        RateLimitReason::ModelCapacityExhausted
    } else if body.contains("per minute")
        || body.contains("rate limit")
        || body.contains("too many requests")
        || (generic_resource_exhausted && !explicit_quota_exhausted)
    {
        RateLimitReason::RateLimitExceeded
    } else if explicit_quota_exhausted || body.contains("exhausted") || body.contains("quota") {
        RateLimitReason::QuotaExhausted
    } else {
        RateLimitReason::Unknown
    }
}

const IMAGE_ACCOUNT_RESELECT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);
/// project_id 探测失败/超时时的回退值
const DEFAULT_PROJECT_ID: &str = "bamboo-precept-lgxtn";
/// project_id 探测失败后的负缓存时长
const PROJECT_ID_NEGATIVE_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(300);

async fn wait_for_image_account_change(
    changes: &mut tokio::sync::watch::Receiver<u64>,
    remaining: std::time::Duration,
) -> bool {
    if remaining.is_zero() {
        return false;
    }
    tokio::select! {
        result = changes.changed() => result.is_ok(),
        _ = tokio::time::sleep(remaining.min(IMAGE_ACCOUNT_RESELECT_INTERVAL)) => true,
    }
}

async fn wait_for_image_token_selection<T>(
    deadline: tokio::time::Instant,
    selection: impl std::future::Future<Output = T>,
) -> Option<T> {
    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
    if remaining.is_zero() {
        return None;
    }
    tokio::time::timeout(remaining, selection).await.ok()
}

/// 异步安全的账号 JSON 更新函数
///
/// 使用 `tokio::task::spawn_blocking` 将阻塞的文件 I/O 与 `std::sync::Mutex`
/// 的获取操作转移到 Tokio 的阻塞线程池中，避免占用 Tokio Worker Thread，
/// 防止高并发场景下因同步锁争抢导致 Tokio 运行时饥饿（runtime starvation）。
async fn update_account_json(
    path: &std::path::Path,
    update: impl FnOnce(&mut serde_json::Value) + Send + 'static,
) -> Result<(), String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let _account_write = crate::modules::account::lock_account_file_updates()?;
        let raw = std::fs::read_to_string(&path).map_err(|e| format!("读取文件失败: {}", e))?;
        let mut content: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| format!("解析 JSON 失败: {}", e))?;
        update(&mut content);
        let serialized = serde_json::to_string_pretty(&content)
            .map_err(|e| format!("序列化 JSON 失败: {}", e))?;
        std::fs::write(&path, serialized).map_err(|e| format!("写入文件失败: {}", e))
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))?
}

fn unix_timestamp_ceil(time: std::time::SystemTime) -> Option<i64> {
    let since_epoch = time
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .ok()?;
    let seconds = since_epoch
        .as_secs()
        .saturating_add(u64::from(since_epoch.subsec_nanos() > 0));
    i64::try_from(seconds).ok()
}

#[derive(Debug, Clone)]
pub struct ProxyToken {
    pub account_id: String,
    pub priority: u8,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub timestamp: i64,
    pub email: String,
    pub account_path: PathBuf, // 账号文件路径，用于更新
    pub project_id: Option<String>,
    pub subscription_tier: Option<String>, // "FREE" | "PRO" | "ULTRA"
    pub remaining_quota: Option<i32>,      // [FIX #563] Remaining quota for priority sorting
    pub protected_models: HashSet<String>, // [NEW #621]
    pub health_score: f32,                 // [NEW] 健康分数 (0.0 - 1.0)
    pub reset_time: Option<i64>,           // [NEW] 配额刷新时间戳（用于排序优化）
    pub weekly_reset_time: Option<i64>, // [NEW] 周配额刷新时间戳（用于临期优先调度 EDF，防止周期重置作废配额）
    pub validation_blocked: bool, // [NEW] Check for validation block (VALIDATION_REQUIRED temporary block)
    pub validation_blocked_until: i64, // [NEW] Timestamp until which the account is blocked
    pub validation_url: Option<String>, // [NEW] Validation URL (#1522)
    pub model_quotas: HashMap<String, i32>, // [OPTIMIZATION] In-memory cache for model-specific quotas
    pub model_limits: HashMap<String, u64>, // [NEW] max_output_tokens per model from quota data
    /// [NEW] 账号实际授权模型集合（原始 ID，直接镜像 fetchAvailableModels 下发）。
    /// 用于自适应路由判断：只要模型出现在此集合中，该账号即可承接对应请求。
    /// 无需任何硬编码字典；未来新模型自动覆盖，无需修改代码。
    pub available_models: std::collections::HashSet<String>,
    /// [NEW #3677] 模型元数据缓存 (display_name, supports_images, supports_thinking 等)
    pub model_metadata: HashMap<String, crate::models::quota::ModelListingMeta>,
}

/// 活跃在途请求 RAII Guard：在请求处理或流式传输期间持有，Drop 时原子释放并发计数
#[derive(Debug)]
pub struct InFlightGuard {
    pub account_id: String,
    counter: Arc<AtomicUsize>,
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        let prev = self.counter.fetch_sub(1, Ordering::SeqCst);
        tracing::debug!(
            "📉 [InFlight] Released request for account {}: {} -> {}",
            self.account_id,
            prev,
            prev.saturating_sub(1)
        );
    }
}

pub struct TokenManager {
    tokens: Arc<DashMap<String, ProxyToken>>, // account_id -> ProxyToken
    current_index: Arc<AtomicUsize>,
    last_used_account: Arc<tokio::sync::Mutex<Option<(String, std::time::Instant)>>>,
    data_dir: PathBuf,
    rate_limit_tracker: Arc<RateLimitTracker>, // 新增: 限流跟踪器
    sticky_config: Arc<tokio::sync::RwLock<StickySessionConfig>>, // 新增：调度配置
    session_accounts: Arc<DashMap<String, String>>, // 新增：会话与账号映射 (SessionID -> AccountID)
    preferred_account_id: Arc<tokio::sync::RwLock<Option<String>>>, // [FIX #820] 优先使用的账号ID（固定账号模式）
    health_scores: Arc<DashMap<String, f32>>,                       // account_id -> health_score
    circuit_breaker_config: Arc<tokio::sync::RwLock<crate::models::CircuitBreakerConfig>>, // [NEW] 熔断配置缓存

    // [NEW] 按账号分配的同步刷新锁。
    // 用于实现 Double-Checked Locking，防止并发请求导致单个账号短时间内多次调用 OAuth Refresh。
    refresh_locks: Arc<DashMap<String, Arc<tokio::sync::Mutex<()>>>>,

    // [NEW] 记录账号连续 invalid_grant 失败次数，防止单次偶发网络抖动误停用账号
    invalid_grant_failures: Arc<DashMap<String, u32>>,

    // 按账号分配的 project_id 探测锁，与 refresh_locks 分离，避免被后台 OAuth 刷新阻塞而误触超时
    project_id_locks: Arc<DashMap<String, Arc<tokio::sync::Mutex<()>>>>,

    // project_id 探测失败的负缓存 (account_id -> 失败时刻)，TTL 内直接回退默认值，避免每次请求承受探测延迟
    project_id_failures: Arc<DashMap<String, std::time::Instant>>,

    /// 支持优雅关闭时主动 abort 后台任务
    auto_cleanup_handle: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    proactive_refresh_handle: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    cancel_token: CancellationToken,
    image_scheduler: std::sync::RwLock<Option<Weak<ImageScheduler>>>,

    /// [NEW] 模型级临时避让/熔断记录: (account_id, base_model_family) -> 冷却结束时间戳 (Unix秒)
    /// 当某账号调用高级模型（如 Claude 5.5）返回 404/403 时，仅将该特定模型打入冷却，绝不连坐其他模型
    unsupported_models: Arc<DashMap<(String, String), i64>>,

    /// [NEW] 活跃在途请求跟踪器: account_id -> 实时并发计数
    in_flight_requests: Arc<DashMap<String, Arc<AtomicUsize>>>,

    /// [NEW] QoS 智能熔断与阶梯探针守卫
    pub qos_guardian: Arc<crate::proxy::qos_guardian::QoSGuardian>,
    qos_probe_handle: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl TokenManager {
    fn resolved_data_dir(&self) -> PathBuf {
        #[cfg(test)]
        {
            if !self.data_dir.as_os_str().is_empty() {
                return self.data_dir.clone();
            }
        }
        crate::modules::account::get_data_dir().unwrap_or_else(|_| self.data_dir.clone())
    }

    /// 创建新的 TokenManager
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            tokens: Arc::new(DashMap::new()),
            current_index: Arc::new(AtomicUsize::new(0)),
            last_used_account: Arc::new(tokio::sync::Mutex::new(None)),
            data_dir,
            rate_limit_tracker: Arc::new(RateLimitTracker::new()),
            sticky_config: Arc::new(tokio::sync::RwLock::new(StickySessionConfig::default())),
            session_accounts: Arc::new(DashMap::new()),
            preferred_account_id: Arc::new(tokio::sync::RwLock::new(None)), // [FIX #820]
            health_scores: Arc::new(DashMap::new()),
            circuit_breaker_config: Arc::new(tokio::sync::RwLock::new(
                crate::models::CircuitBreakerConfig::default(),
            )),
            refresh_locks: Arc::new(DashMap::new()),
            invalid_grant_failures: Arc::new(DashMap::new()),
            project_id_locks: Arc::new(DashMap::new()),
            project_id_failures: Arc::new(DashMap::new()),
            auto_cleanup_handle: Arc::new(tokio::sync::Mutex::new(None)),
            proactive_refresh_handle: Arc::new(tokio::sync::Mutex::new(None)),
            cancel_token: CancellationToken::new(),
            image_scheduler: std::sync::RwLock::new(None),
            unsupported_models: Arc::new(DashMap::new()),
            in_flight_requests: Arc::new(DashMap::new()),
            qos_guardian: Arc::new(crate::proxy::qos_guardian::QoSGuardian::new()),
            qos_probe_handle: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// 获取指定账号当前的活跃在途请求数
    pub fn get_in_flight(&self, account_id: &str) -> usize {
        self.in_flight_requests
            .get(account_id)
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// 获取指定账号的在途并发许可 Guard（生命周期结束或 Drop 时自动原子释放）
    pub fn acquire_in_flight(&self, account_id: &str) -> InFlightGuard {
        let counter = self
            .in_flight_requests
            .entry(account_id.to_string())
            .or_insert_with(|| Arc::new(AtomicUsize::new(0)))
            .clone();
        let prev = counter.fetch_add(1, Ordering::SeqCst);
        tracing::debug!(
            "📈 [InFlight] Acquired request for account {}: {} -> {}",
            account_id,
            prev,
            prev + 1
        );
        InFlightGuard {
            account_id: account_id.to_string(),
            counter,
        }
    }

    pub(crate) fn register_image_scheduler(&self, scheduler: &Arc<ImageScheduler>) {
        if let Ok(mut slot) = self.image_scheduler.write() {
            *slot = Some(Arc::downgrade(scheduler));
        }
        scheduler.sync_accounts(self.enabled_account_ids());
    }

    pub(crate) fn enabled_account_ids(&self) -> Vec<String> {
        self.tokens
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    fn sync_image_scheduler_accounts(&self) {
        let scheduler = self
            .image_scheduler
            .read()
            .ok()
            .and_then(|slot| slot.as_ref().and_then(Weak::upgrade));
        if let Some(scheduler) = scheduler {
            scheduler.sync_accounts(self.enabled_account_ids());
        }
    }

    /// 启动限流记录自动清理后台任务（每15秒检查并清除过期记录）
    pub async fn start_rate_limit_cleanup(&self) {
        let tracker = self.rate_limit_tracker.clone();
        let cancel = self.cancel_token.child_token();

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        tracing::info!("Auto-cleanup task received cancel signal");
                        break;
                    }
                    _ = interval.tick() => {
                        let cleaned = tracker.cleanup_expired();
                        if cleaned > 0 {
                            tracing::info!(
                                "Auto-cleanup: Removed {} expired rate limit record(s)",
                                cleaned
                            );
                        }
                    }
                }
            }
        });

        // 先 abort 旧任务（防止任务泄漏），再存储新 handle
        let mut guard = self.auto_cleanup_handle.lock().await;
        if let Some(old) = guard.take() {
            old.abort();
            tracing::warn!("Aborted previous auto-cleanup task");
        }
        *guard = Some(handle);

        tracing::info!("Rate limit auto-cleanup task started (interval: 15s)");
    }

    /// 启动 Token 主动预刷新后台任务（每30秒检查并在过期前10分钟刷新内存中的 Token）
    pub async fn start_proactive_token_refresh(self: &Arc<Self>) {
        let weak_self = Arc::downgrade(self);
        let cancel = self.cancel_token.child_token();

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        tracing::info!("Proactive token refresh task received cancel signal");
                        break;
                    }
                    _ = interval.tick() => {
                        let Some(mgr) = weak_self.upgrade() else {
                            tracing::debug!("TokenManager dropped, stopping proactive refresh loop");
                            break;
                        };
                        mgr.run_proactive_token_refresh_tick().await;
                    }
                }
            }
        });

        let mut guard = self.proactive_refresh_handle.lock().await;
        if let Some(old) = guard.take() {
            old.abort();
            tracing::warn!("Aborted previous proactive token refresh task");
        }
        *guard = Some(handle);

        tracing::info!("Proactive token refresh task started (interval: 30s, buffer: 600s)");
    }

    /// 启动所有后台维护任务（限流清理与 Token 主动预刷新与 QoS 自愈探针巡检）
    pub async fn start_auto_cleanup(self: &Arc<Self>) {
        self.start_rate_limit_cleanup().await;
        self.start_proactive_token_refresh().await;
        self.start_qos_probing_daemon().await;
    }

    /// 执行一次主动预刷新检查轮次
    async fn run_proactive_token_refresh_tick(&self) {
        let now = chrono::Utc::now().timestamp();
        const PROACTIVE_REFRESH_BUFFER_SECS: i64 = 600; // 提前10分钟预刷新

        // 收集需要预刷新的账号列表
        let candidates: Vec<(String, String)> = self
            .tokens
            .iter()
            .filter_map(|entry| {
                let token = entry.value();
                if now >= token.timestamp - PROACTIVE_REFRESH_BUFFER_SECS {
                    Some((token.account_id.clone(), token.email.clone()))
                } else {
                    None
                }
            })
            .collect();

        if candidates.is_empty() {
            return;
        }

        tracing::debug!(
            "Proactive token refresh: scanning {} expiring account(s)...",
            candidates.len()
        );

        for (account_id, email) in candidates {
            let timeout = std::time::Duration::from_secs(10);
            match self
                .refresh_account_token_with_timeout(
                    &account_id,
                    PROACTIVE_REFRESH_BUFFER_SECS,
                    timeout,
                )
                .await
            {
                Ok(_) => {
                    tracing::info!(
                        "Proactive token refresh: successfully refreshed token for {}",
                        email
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Proactive token refresh failed for {} (will retry next interval): {}",
                        email,
                        e
                    );
                }
            }
        }
    }

    /// 刷新单个账号的 Access Token，支持并发互斥锁（Double-Checked Locking）与自定义超时
    ///
    /// # 参数
    /// * `account_id` - 目标账号 ID
    /// * `buffer_secs` - 有效期缓冲时间（秒），若当前时间小于过期时间减去该缓冲，则视为已刷新直接返回
    /// * `timeout_duration` - 调用底层 OAuth 刷新的超时限制
    pub async fn refresh_account_token_with_timeout(
        &self,
        account_id: &str,
        buffer_secs: i64,
        timeout_duration: std::time::Duration,
    ) -> Result<ProxyToken, String> {
        let (token_email, refresh_token, account_path) = {
            let entry = self
                .tokens
                .get(account_id)
                .ok_or_else(|| format!("账号 {} 不存在于内存池中", account_id))?;
            (
                entry.email.clone(),
                entry.refresh_token.clone(),
                entry.account_path.clone(),
            )
        };

        if refresh_token.is_empty() {
            return Err(format!("账号 {} 的 refresh_token 为空", token_email));
        }

        let now = chrono::Utc::now().timestamp();
        // 快速前置检查：若未达到缓冲刷新线，直接返回现有 token
        if let Some(entry) = self.tokens.get(account_id) {
            if now < entry.timestamp - buffer_secs {
                return Ok(entry.clone());
            }
        }

        // 双重检查锁定逻辑 (Double-Checked Locking)，将获取锁与网络刷新全流程纳入超时管控
        let refresh_mu = self
            .refresh_locks
            .entry(account_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();

        let refresh_op = async {
            let _guard = refresh_mu.lock().await;

            let now = chrono::Utc::now().timestamp();
            if let Some(latest) = self.tokens.get(account_id) {
                if now < latest.timestamp - buffer_secs {
                    tracing::debug!(
                        "账号 {} 在等待刷新锁期间已完成刷新，复用最新 Token",
                        token_email
                    );
                    return Ok(latest.clone());
                }
            }

            tracing::debug!(
                "正在执行 OAuth 刷新 (账号: {}, 超时: {:?})...",
                token_email,
                timeout_duration
            );

            let refresh_future =
                crate::modules::oauth::refresh_access_token(&refresh_token, Some(account_id));

            match refresh_future.await {
                Ok(token_response) => {
                    tracing::debug!("账号 {} Token 刷新成功！", token_email);
                    self.invalid_grant_failures.remove(account_id);

                    let updated_token = if let Some(mut entry) = self.tokens.get_mut(account_id) {
                        entry.access_token = token_response.access_token.clone();
                        entry.expires_in = token_response.expires_in;
                        entry.timestamp = now + token_response.expires_in;
                        entry.clone()
                    } else {
                        return Err(format!("账号 {} 在刷新过程中被移除", account_id));
                    };

                    // 写盘操作后台化：内存已更新，磁盘持久化 spawn 到 blocking 线程池
                    // 避免因磁盘 I/O 或锁争抢阻塞当前请求
                    {
                        let write_path = account_path;
                        let access_token = token_response.access_token;
                        let expires_in = token_response.expires_in;
                        let id_token = token_response.id_token;
                        let new_rt = token_response.refresh_token;
                        let write_ts = now + expires_in;
                        tokio::task::spawn_blocking(move || {
                            let Ok(_lk) = crate::modules::account::lock_account_file_updates()
                            else {
                                return;
                            };
                            let Ok(raw) = std::fs::read_to_string(&write_path) else {
                                return;
                            };
                            let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&raw)
                            else {
                                return;
                            };
                            val["token"]["access_token"] = access_token.into();
                            val["token"]["expires_in"] = expires_in.into();
                            val["token"]["expiry_timestamp"] = write_ts.into();
                            if let Some(it) = id_token {
                                val["token"]["id_token"] = it.into();
                            }
                            if let Some(rt) = new_rt {
                                val["token"]["refresh_token"] = rt.into();
                            }
                            if let Ok(s) = serde_json::to_string_pretty(&val) {
                                let _ = std::fs::write(&write_path, s);
                            }
                        });
                    }

                    Ok(updated_token)
                }
                Err(e) => {
                    tracing::error!("Token 刷新失败 ({}): {}", token_email, e);
                    let is_grant_error =
                        e.contains("\"invalid_grant\"") || e.contains("invalid_grant");
                    if is_grant_error {
                        // 计数后立即释放 DashMap 分片写锁：后续 disable_account().await 与
                        // remove() 会再次访问同一分片，持锁跨越将导致自死锁
                        let current_fails = {
                            let mut fail_count = self
                                .invalid_grant_failures
                                .entry(account_id.to_string())
                                .or_insert(0);
                            *fail_count += 1;
                            *fail_count
                        };
                        if current_fails >= 2 {
                            tracing::error!(
                                "账号 {} 连续 {} 次确认为 invalid_grant，正式执行停用",
                                token_email,
                                current_fails
                            );
                            let _ = self
                                .disable_account(account_id, &format!("invalid_grant: {}", e))
                                .await;
                            self.invalid_grant_failures.remove(account_id);
                        } else {
                            tracing::warn!(
                                "账号 {} 首次确认为 invalid_grant (计数 {}/2)，暂不停用，跳过本次调度",
                                token_email,
                                current_fails
                            );
                        }
                    }
                    Err(format!("Token refresh failed: {}", e))
                }
            }
        };

        match tokio::time::timeout(timeout_duration, refresh_op).await {
            Ok(res) => res,
            Err(_) => {
                tracing::error!(
                    "Token 刷新超时 (超过 {:?}) (账号: {})",
                    timeout_duration,
                    token_email
                );
                Err(format!(
                    "Token refresh timed out after {:?}",
                    timeout_duration
                ))
            }
        }
    }

    /// 安全解析/探测 project_id，使用账号专属锁 + 双重检查 + 3.5s 超时管控 + 后台写盘
    async fn resolve_project_id_with_timeout(
        &self,
        account_id: &str,
        access_token: &str,
        account_path: &std::path::Path,
        timeout_duration: std::time::Duration,
    ) -> String {
        // 快速路径：内存中已存在有效 project_id 则直接复用
        if let Some(entry) = self.tokens.get(account_id) {
            if let Some(ref pid) = entry.project_id {
                let trimmed = pid.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }

        if self.is_project_id_negatively_cached(account_id) {
            return DEFAULT_PROJECT_ID.to_string();
        }

        let resolve_mu = self
            .project_id_locks
            .entry(account_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();

        let token_access_token = access_token.to_string();
        let acct_id = account_id.to_string();
        let write_path = account_path.to_path_buf();

        let resolve_op = async {
            let _guard = resolve_mu.lock().await;

            // 获取锁后进行 Double-Check (含负缓存：等锁期间其他请求可能刚刚探测失败)
            if self.is_project_id_negatively_cached(&acct_id) {
                return DEFAULT_PROJECT_ID.to_string();
            }
            let current_access_token = if let Some(entry) = self.tokens.get(&acct_id) {
                if let Some(ref pid) = entry.project_id {
                    let trimmed = pid.trim();
                    if !trimmed.is_empty() {
                        return trimmed.to_string();
                    }
                }
                entry.access_token.clone()
            } else {
                token_access_token.clone()
            };

            let fetch_fut = crate::proxy::project_resolver::fetch_project_id(&current_access_token);
            match fetch_fut.await {
                Ok(pid) => {
                    let trimmed = pid.trim().to_string();
                    self.project_id_failures.remove(&acct_id);
                    if let Some(mut entry) = self.tokens.get_mut(&acct_id) {
                        entry.project_id = Some(trimmed.clone());
                    }
                    // 写盘后台化，避免阻塞热请求路径
                    let pid_clone = trimmed.clone();
                    tokio::task::spawn_blocking(move || {
                        let Ok(_lk) = crate::modules::account::lock_account_file_updates() else {
                            return;
                        };
                        let Ok(raw) = std::fs::read_to_string(&write_path) else {
                            return;
                        };
                        let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&raw) else {
                            return;
                        };
                        val["token"]["project_id"] = pid_clone.into();
                        if let Ok(s) = serde_json::to_string_pretty(&val) {
                            let _ = std::fs::write(&write_path, s);
                        }
                    });
                    trimmed
                }
                Err(e) => {
                    tracing::warn!(
                        "fetch_project_id 失败: {} (账号: {})，回退到默认 project_id",
                        e,
                        acct_id
                    );
                    self.project_id_failures
                        .insert(acct_id.clone(), std::time::Instant::now());
                    DEFAULT_PROJECT_ID.to_string()
                }
            }
        };

        match tokio::time::timeout(timeout_duration, resolve_op).await {
            Ok(pid) => pid,
            Err(_) => {
                tracing::warn!(
                    "fetch_project_id 超时 (超过 {:?}) (账号: {})，回退到默认 project_id",
                    timeout_duration,
                    account_id
                );
                self.project_id_failures
                    .insert(account_id.to_string(), std::time::Instant::now());
                DEFAULT_PROJECT_ID.to_string()
            }
        }
    }

    /// project_id 负缓存是否仍在 TTL 内；过期条目顺带清理
    fn is_project_id_negatively_cached(&self, account_id: &str) -> bool {
        let expired = match self.project_id_failures.get(account_id) {
            Some(failed_at) => failed_at.elapsed() >= PROJECT_ID_NEGATIVE_CACHE_TTL,
            None => return false,
        };
        if expired {
            self.project_id_failures.remove(account_id);
        }
        !expired
    }

    /// 从主应用账号目录加载所有账号
    pub async fn load_accounts(&self) -> Result<usize, String> {
        let accounts_dir = self.resolved_data_dir().join("accounts");

        if !accounts_dir.exists() {
            return Err(format!("账号目录不存在: {:?}", accounts_dir));
        }

        // 读取 accounts.json 索引，过滤掉用户在索引中标记为 disabled 或 proxy_disabled 的账号，防止实体文件残留导致已停用账号渗入代理池
        let index_file = self.resolved_data_dir().join("accounts.json");
        let disabled_in_index: std::collections::HashSet<String> = if index_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&index_file) {
                if let Ok(index_val) = serde_json::from_str::<serde_json::Value>(&content) {
                    index_val
                        .get("accounts")
                        .and_then(|a| a.as_array())
                        .map(|accs| {
                            accs.iter()
                                .filter_map(|acc| {
                                    let is_disabled = acc
                                        .get("disabled")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false);
                                    let is_proxy_disabled = acc
                                        .get("proxy_disabled")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false);
                                    if is_disabled || is_proxy_disabled {
                                        acc.get("id")
                                            .and_then(|v| v.as_str())
                                            .map(|s| s.to_string())
                                    } else {
                                        None
                                    }
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                } else {
                    std::collections::HashSet::new()
                }
            } else {
                std::collections::HashSet::new()
            }
        } else {
            std::collections::HashSet::new()
        };

        let entries =
            std::fs::read_dir(&accounts_dir).map_err(|e| format!("读取账号目录失败: {}", e))?;

        // 阶段 1：在局部集合中加载新账号，严禁提前清空现有活跃池导致并发 503 宕机
        let mut new_tokens = std::collections::HashMap::new();

        for entry in entries {
            let entry = entry.map_err(|e| format!("读取目录项失败: {}", e))?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if disabled_in_index.contains(stem) {
                // 实体文件是账号状态的真实权威来源。先检查实体文件，若实体文件本身未停用（索引数据陈旧），以实体文件为准；
                // 仅当实体文件确实标记为停用或不可读时才跳过。
                let is_truly_disabled = if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(acc_json) = serde_json::from_str::<serde_json::Value>(&content) {
                        let is_disabled = acc_json
                            .get("disabled")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let is_proxy_disabled = acc_json
                            .get("proxy_disabled")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        is_disabled || is_proxy_disabled
                    } else {
                        true
                    }
                } else {
                    true
                };

                if is_truly_disabled {
                    tracing::info!(
                        "[Proxy] Account {} is confirmed disabled/proxy_disabled in both index and entity, skipping from proxy pool",
                        stem
                    );
                    continue;
                } else {
                    tracing::warn!(
                        "[Proxy] Account {} was marked disabled in index but is enabled in entity file; trusting entity file",
                        stem
                    );
                }
            }

            // 尝试加载账号
            match self.load_single_account(&path).await {
                Ok(Some(token)) => {
                    let account_id = token.account_id.clone();
                    new_tokens.insert(account_id, token);
                }
                Ok(None) => {
                    // 跳过无效账号
                }
                Err(e) => {
                    tracing::warn!("加载账号失败 {:?}: {}", path, e);
                }
            }
        }

        let count = new_tokens.len();

        // 阶段 2：零停机平滑原子同步到 self.tokens
        // 剔除已在磁盘删除或停用的账号
        self.tokens.retain(|k, _| new_tokens.contains_key(k));
        // 更新或插入新载入的有效账号
        for (k, v) in new_tokens {
            self.tokens.insert(k, v);
        }

        // 若上次记录的使用账号已不在池中，则安全清理
        {
            let mut last_used = self.last_used_account.lock().await;
            if let Some((ref acc_id, _)) = *last_used {
                if !self.tokens.contains_key(acc_id) {
                    *last_used = None;
                }
            }
        }

        self.sync_image_scheduler_accounts();
        Ok(count)
    }

    /// 重新加载指定账号（用于配额更新后的实时同步）
    pub async fn reload_account(&self, account_id: &str) -> Result<(), String> {
        let path = self
            .data_dir
            .join("accounts")
            .join(format!("{}.json", account_id));
        if !path.exists() {
            self.remove_account(account_id);
            return Err(format!("账号文件不存在: {:?}", path));
        }

        // 实体文件是账号状态的真实权威来源。
        // 若 accounts.json 索引标记了停用，且实体文件也确实停用，则快速从内存池移除；
        // 若实体文件未停用（索引发生漂移），则以实体文件为准继续加载。
        let index_file = self.resolved_data_dir().join("accounts.json");
        if index_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&index_file) {
                if let Ok(index_val) = serde_json::from_str::<serde_json::Value>(&content) {
                    let is_disabled = index_val
                        .get("accounts")
                        .and_then(|a| a.as_array())
                        .map(|accs| {
                            accs.iter().any(|acc| {
                                acc.get("id").and_then(|v| v.as_str()) == Some(account_id)
                                    && (acc
                                        .get("disabled")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false)
                                        || acc
                                            .get("proxy_disabled")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or(false))
                            })
                        })
                        .unwrap_or(false);
                    if is_disabled
                        && Self::get_account_state_on_disk(&path).await
                            == OnDiskAccountState::Disabled
                    {
                        self.remove_account(account_id);
                        return Ok(());
                    }
                }
            }
        }

        match self.load_single_account(&path).await {
            Ok(Some(token)) => {
                self.tokens.insert(account_id.to_string(), token);
                self.sync_image_scheduler_accounts();
                Ok(())
            }
            Ok(None) => {
                // [FIX] 账号被禁用或不可用时，从内存池中彻底移除 (Issue #1565)
                // load_single_account returning None means the account should be skipped in its
                // current state (disabled / proxy_disabled / quota_protection / validation_blocked...).
                self.remove_account(account_id);
                Ok(())
            }
            Err(e) => Err(format!("同步账号失败: {}", e)),
        }
    }

    /// 重新加载所有账号
    pub async fn reload_all_accounts(&self) -> Result<usize, String> {
        self.load_accounts().await
    }

    /// 从内存中彻底移除指定账号及其关联数据 (Issue #1477)
    pub fn remove_account(&self, account_id: &str) {
        // ... (省略原有逻辑)
        if self.tokens.remove(account_id).is_some() {
            tracing::info!("[Proxy] Removed account {} from memory cache", account_id);
        }
        self.health_scores.remove(account_id);
        self.rate_limit_tracker.clear(account_id);
        self.refresh_locks.remove(account_id);
        self.invalid_grant_failures.remove(account_id);
        self.project_id_locks.remove(account_id);
        self.project_id_failures.remove(account_id);
        self.in_flight_requests.remove(account_id);
        self.session_accounts.retain(|_, v| v != account_id);
        if let Ok(mut preferred) = self.preferred_account_id.try_write() {
            if preferred.as_deref() == Some(account_id) {
                *preferred = None;
                tracing::info!(
                    "[Proxy] Cleared preferred account status for {}",
                    account_id
                );
            }
        }
        self.sync_image_scheduler_accounts();
    }

    /// 根据账号 ID 获取完整的 ProxyToken 对象 (v4.1.29)
    pub fn get_token_by_id(&self, account_id: &str) -> Option<ProxyToken> {
        self.tokens.get(account_id).map(|t| t.clone())
    }

    /// Apply a saved priority without resetting sessions or live rate limits.
    pub fn update_account_priority(&self, account_id: &str, priority: u8) {
        if let Some(mut token) = self.tokens.get_mut(account_id) {
            token.priority = priority;
        }
    }

    /// Check if an account has been disabled on disk.
    ///
    /// Safety net: avoids selecting a disabled account when the in-memory pool hasn't been
    /// reloaded yet (e.g. fixed account mode / sticky session).
    ///
    /// Note: this is intentionally tolerant to transient read/parse failures (e.g. concurrent
    /// writes). Failures are reported as `Unknown` so callers can skip without purging the in-memory
    /// token pool.
    async fn get_account_state_on_disk(account_path: &std::path::PathBuf) -> OnDiskAccountState {
        const MAX_RETRIES: usize = 2;
        const RETRY_DELAY_MS: u64 = 5;

        for attempt in 0..=MAX_RETRIES {
            let content = match tokio::fs::read_to_string(account_path).await {
                Ok(c) => c,
                Err(e) => {
                    // If the file is gone, the in-memory token is definitely stale.
                    if e.kind() == std::io::ErrorKind::NotFound {
                        return OnDiskAccountState::Disabled;
                    }
                    if attempt < MAX_RETRIES {
                        tokio::time::sleep(std::time::Duration::from_millis(RETRY_DELAY_MS)).await;
                        continue;
                    }
                    tracing::debug!(
                        "Failed to read account file on disk {:?}: {}",
                        account_path,
                        e
                    );
                    return OnDiskAccountState::Unknown;
                }
            };

            let account = match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(v) => v,
                Err(e) => {
                    if attempt < MAX_RETRIES {
                        tokio::time::sleep(std::time::Duration::from_millis(RETRY_DELAY_MS)).await;
                        continue;
                    }
                    tracing::debug!(
                        "Failed to parse account JSON on disk {:?}: {}",
                        account_path,
                        e
                    );
                    return OnDiskAccountState::Unknown;
                }
            };

            let disabled = account
                .get("disabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
                || account
                    .get("proxy_disabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                || account
                    .get("quota")
                    .and_then(|q| q.get("is_forbidden"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

            return if disabled {
                OnDiskAccountState::Disabled
            } else {
                OnDiskAccountState::Enabled
            };
        }

        OnDiskAccountState::Unknown
    }

    /// 加载单个账号
    async fn load_single_account(&self, path: &PathBuf) -> Result<Option<ProxyToken>, String> {
        let content = std::fs::read_to_string(path).map_err(|e| format!("读取文件失败: {}", e))?;

        let mut account: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| format!("解析 JSON 失败: {}", e))?;

        // [修复 #1344] 先检查账号是否被手动禁用(非配额保护原因)
        let is_proxy_disabled = account
            .get("proxy_disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let disabled_reason = account
            .get("proxy_disabled_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if is_proxy_disabled && disabled_reason != "quota_protection" {
            // Account manually disabled
            tracing::debug!(
                "Account skipped due to manual disable: {:?} (email={}, reason={})",
                path,
                account
                    .get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>"),
                disabled_reason
            );
            return Ok(None);
        }

        // [NEW] Check for validation block (VALIDATION_REQUIRED temporary block)
        if account
            .get("validation_blocked")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            let block_until = account
                .get("validation_blocked_until")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            let now = chrono::Utc::now().timestamp();

            if now < block_until {
                // Still blocked
                tracing::debug!(
                    "Skipping validation-blocked account: {:?} (email={}, blocked until {})",
                    path,
                    account
                        .get("email")
                        .and_then(|v| v.as_str())
                        .unwrap_or("<unknown>"),
                    chrono::DateTime::from_timestamp(block_until, 0)
                        .map(|dt| dt.format("%H:%M:%S").to_string())
                        .unwrap_or_else(|| block_until.to_string())
                );
                return Ok(None);
            } else {
                // Block expired - clear it.
                //
                // 同时清掉两样容易漏的：
                //   - quota.is_forbidden：它是 403 缓存的「账号被封」判定，封禁窗口
                //     一过就该失效；不清它的话刷新会一直跳过这个账号，它永远回不来
                //     （issue #3630）。
                //   - validation_url：与 clear_validation_blocked 的清理口径保持一致，
                //     避免留下「块已清、URL 还在」的残留字段（issue #3631）。
                account["validation_blocked"] = serde_json::json!(false);
                account["validation_blocked_until"] = serde_json::json!(0);
                account["validation_blocked_reason"] = serde_json::Value::Null;
                account["validation_url"] = serde_json::Value::Null;
                if let Some(q) = account.get_mut("quota").and_then(|v| v.as_object_mut()) {
                    q.insert("is_forbidden".to_string(), serde_json::json!(false));
                }

                update_account_json(path, |latest| {
                    latest["validation_blocked"] = serde_json::json!(false);
                    latest["validation_blocked_until"] = serde_json::json!(0);
                    latest["validation_blocked_reason"] = serde_json::Value::Null;
                    latest["validation_url"] = serde_json::Value::Null;
                    if let Some(q) = latest.get_mut("quota").and_then(|v| v.as_object_mut()) {
                        q.insert("is_forbidden".to_string(), serde_json::json!(false));
                    }
                })
                .await?;
                tracing::info!(
                    "Validation block expired and cleared for account: {}",
                    account
                        .get("email")
                        .and_then(|v| v.as_str())
                        .unwrap_or("<unknown>")
                );
            }
        }

        // 最终检查账号主开关
        if account
            .get("disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            tracing::debug!(
                "Skipping disabled account file: {:?} (email={})",
                path,
                account
                    .get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>")
            );
            return Ok(None);
        }

        // Safety check: verify state on disk again to handle concurrent mid-parse writes
        if Self::get_account_state_on_disk(path).await == OnDiskAccountState::Disabled {
            tracing::debug!("Account file {:?} is disabled on disk, skipping.", path);
            return Ok(None);
        }

        // 配额保护检查 - 只处理配额保护逻辑
        // 这样可以在加载时自动恢复配额已恢复的账号
        if self.check_and_protect_quota(&mut account, path).await {
            tracing::debug!(
                "Account skipped due to quota protection: {:?} (email={})",
                path,
                account
                    .get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>")
            );
            return Ok(None);
        }

        // [兼容性] 再次确认最终状态（可能被 check_and_protect_quota 修改）
        if account
            .get("proxy_disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            tracing::debug!(
                "Skipping proxy-disabled account file: {:?} (email={})",
                path,
                account
                    .get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>")
            );
            return Ok(None);
        }

        let account_id = account["id"].as_str().ok_or("缺少 id 字段")?.to_string();

        let email = account["email"]
            .as_str()
            .ok_or("缺少 email 字段")?
            .to_string();

        let token_obj = account["token"].as_object().ok_or("缺少 token 字段")?;

        let access_token = token_obj["access_token"]
            .as_str()
            .ok_or("缺少 access_token")?
            .to_string();

        let refresh_token = token_obj["refresh_token"]
            .as_str()
            .ok_or("缺少 refresh_token")?
            .to_string();

        let expires_in = token_obj["expires_in"].as_i64().ok_or("缺少 expires_in")?;

        let timestamp = token_obj["expiry_timestamp"]
            .as_i64()
            .ok_or("缺少 expiry_timestamp")?;

        // project_id 是可选的
        let project_id = token_obj
            .get("project_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        // 【新增】提取订阅等级 (subscription_tier 为 "FREE" | "PRO" | "ULTRA")
        let subscription_tier = account
            .get("quota")
            .and_then(|q| q.get("subscription_tier"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // [FIX #563] 提取最大剩余配额百分比用于优先级排序 (Option<i32> now)
        let remaining_quota = account
            .get("quota")
            .and_then(|q| self.calculate_quota_stats(q));
        // .filter(|&r| r > 0); // 移除 >0 过滤，因为 0% 也是有效数据，只是优先级低

        // 【新增 #621】提取受限模型列表
        let protected_models: HashSet<String> = account
            .get("protected_models")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let health_score = self
            .health_scores
            .get(&account_id)
            .map(|v| *v)
            .unwrap_or(1.0);

        // [NEW] 提取最近的配额刷新时间（用于排序优化：刷新时间越近优先级越高）
        let reset_time = self.extract_earliest_reset_time(&account);
        // [NEW] 提取周配额刷新时间（来自 retrieveUserQuotaSummary 的 weekly/7d 窗口，用于临期优先消耗）
        let weekly_reset_time = self.extract_weekly_reset_time(&account);

        // [OPTIMIZATION] 构建模型配额内存缓存，避免排序时读取磁盘
        let mut model_quotas = HashMap::new();
        // [NEW] 构建模型输出限额内存缓存 (max_output_tokens)
        let mut model_limits: HashMap<String, u64> = HashMap::new();
        // [NEW] 构建账号授权模型集合（原始 ID，直接镜像 fetchAvailableModels 下发，用于自适应路由）
        let mut available_models = std::collections::HashSet::new();
        // [NEW #3677] 构建模型能力与元数据内存缓存
        let mut model_metadata = HashMap::new();
        if let Some(models) = account
            .get("quota")
            .and_then(|q| q.get("models"))
            .and_then(|m| m.as_array())
        {
            for model in models {
                if model.get("disabled").and_then(|v| v.as_bool()) == Some(true) {
                    continue;
                }
                let raw_name = model.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let raw_name_lower = raw_name.trim().to_lowercase();
                if raw_name_lower.contains("-disabled") || raw_name_lower.ends_with("_disabled") {
                    continue;
                }

                if let (Some(name), Some(pct)) = (
                    model.get("name").and_then(|v| v.as_str()),
                    model.get("percentage").and_then(|v| v.as_i64()),
                ) {
                    // Normalize name to standard ID
                    let standard_id =
                        crate::proxy::common::model_mapping::normalize_to_standard_id(name)
                            .unwrap_or_else(|| name.to_string());
                    model_quotas.insert(standard_id, pct as i32);
                    // [NEW] 同时记录原始模型名（小写），用于精确 / 基名路由匹配
                    available_models.insert(name.to_lowercase());
                }
                // [NEW] 解析并缓存 max_output_tokens (按原始 model name，不归一化)
                if let (Some(name), Some(limit)) = (
                    model.get("name").and_then(|v| v.as_str()),
                    model.get("max_output_tokens").and_then(|v| v.as_u64()),
                ) {
                    model_limits.insert(name.to_string(), limit);
                }
                // [NEW #3677] 提取模型元数据并缓存 (按原始 model name 小写存储)
                if let Some(name) = model.get("name").and_then(|v| v.as_str()) {
                    let normalized = name.trim().to_lowercase();
                    if !normalized.is_empty() {
                        let meta =
                            serde_json::from_value::<crate::models::quota::ModelListingMeta>(
                                model.clone(),
                            )
                            .unwrap_or_default();
                        model_metadata.insert(normalized, meta);
                    }
                }
            }
        }

        // [NEW] 检查 quota_groups 是否存在 Argon 专属分组，为该账号注入 Argon 授权、配额与限额
        let quota_val = account.get("quota");
        let groups_val = quota_val
            .and_then(|q| q.get("quota_groups").or_else(|| q.get("quotaGroups")))
            .and_then(|g| g.as_array());

        if let Some(groups) = groups_val {
            let argon_bucket = groups.iter().find_map(|g| {
                let name = g
                    .get("display_name")
                    .or_else(|| g.get("displayName"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                let buckets = g.get("buckets").and_then(|b| b.as_array())?;
                if name.contains("argon") {
                    return buckets.first();
                }
                buckets.iter().find(|b| {
                    let b_id = b
                        .get("bucket_id")
                        .or_else(|| b.get("bucketId"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let b_name = b
                        .get("display_name")
                        .or_else(|| b.get("displayName"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_lowercase();
                    b_id.contains("argon") || b_name.contains("argon")
                })
            });

            let is_ultra = subscription_tier
                .as_deref()
                .map(|s| s.to_uppercase() == "ULTRA")
                .unwrap_or(false);

            if let Some(bucket) = argon_bucket {
                let frac = bucket
                    .get("remaining_fraction")
                    .or_else(|| bucket.get("remainingFraction"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1.0);
                let pct = (frac * 100.0) as i32;
                model_quotas.insert("gemini-4-argon".to_string(), pct);
                model_quotas.insert("gemini-4-argon-high".to_string(), pct);
                model_quotas.insert("gemini-4-argon-medium".to_string(), pct);
                model_quotas.insert("gemini-4-argon-low".to_string(), pct);

                available_models.insert("gemini-4-argon".to_string());
                available_models.insert("gemini-4-argon-high".to_string());
                available_models.insert("gemini-4-argon-medium".to_string());
                available_models.insert("gemini-4-argon-low".to_string());

                model_limits.insert("gemini-4-argon".to_string(), 65536);
                model_limits.insert("gemini-4-argon-high".to_string(), 65536);
                model_limits.insert("gemini-4-argon-medium".to_string(), 65536);
                model_limits.insert("gemini-4-argon-low".to_string(), 65536);
            } else if is_ultra {
                // Ultra 账号在官方架构下未拆分专属 Argon 分组，其 Argon 额度共享 Gemini 通用配额池
                let gemini_bucket = groups.iter().find_map(|g| {
                    let name = g
                        .get("display_name")
                        .or_else(|| g.get("displayName"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_lowercase();
                    if !name.contains("claude") && !name.contains("gpt") && !name.contains("3p") {
                        g.get("buckets")
                            .and_then(|b| b.as_array())
                            .and_then(|bs| bs.first())
                    } else {
                        None
                    }
                });
                let frac = gemini_bucket
                    .and_then(|b| {
                        b.get("remaining_fraction")
                            .or_else(|| b.get("remainingFraction"))
                    })
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1.0);
                let pct = (frac * 100.0) as i32;
                model_quotas.insert("gemini-4-argon".to_string(), pct);
                model_quotas.insert("gemini-4-argon-high".to_string(), pct);
                model_quotas.insert("gemini-4-argon-medium".to_string(), pct);
                model_quotas.insert("gemini-4-argon-low".to_string(), pct);

                available_models.insert("gemini-4-argon".to_string());
                available_models.insert("gemini-4-argon-high".to_string());
                available_models.insert("gemini-4-argon-medium".to_string());
                available_models.insert("gemini-4-argon-low".to_string());

                model_limits.insert("gemini-4-argon".to_string(), 65536);
                model_limits.insert("gemini-4-argon-high".to_string(), 65536);
                model_limits.insert("gemini-4-argon-medium".to_string(), 65536);
                model_limits.insert("gemini-4-argon-low".to_string(), 65536);
            } else {
                // 配额组中明确不存在 Argon 分组且非 Ultra 账号，防呆清理（防止历史残存虚假配额注入调度器）
                model_quotas.remove("gemini-4-argon");
                model_quotas.remove("gemini-4-argon-high");
                model_quotas.remove("gemini-4-argon-medium");
                model_quotas.remove("gemini-4-argon-low");

                available_models.remove("gemini-4-argon");
                available_models.remove("gemini-4-argon-high");
                available_models.remove("gemini-4-argon-medium");
                available_models.remove("gemini-4-argon-low");

                model_limits.remove("gemini-4-argon");
                model_limits.remove("gemini-4-argon-high");
                model_limits.remove("gemini-4-argon-medium");
                model_limits.remove("gemini-4-argon-low");
            }
        }

        // 若 model_quotas 中已有 gemini-4-argon（例如通过 models 列表或上游配额注入），确保同步到 available_models 与 limits
        if model_quotas.contains_key("gemini-4-argon") {
            available_models.insert("gemini-4-argon".to_string());
            available_models.insert("gemini-4-argon-high".to_string());
            available_models.insert("gemini-4-argon-medium".to_string());
            available_models.insert("gemini-4-argon-low".to_string());
            model_limits
                .entry("gemini-4-argon".to_string())
                .or_insert(65536);
            model_limits
                .entry("gemini-4-argon-high".to_string())
                .or_insert(65536);
            model_limits
                .entry("gemini-4-argon-medium".to_string())
                .or_insert(65536);
            model_limits
                .entry("gemini-4-argon-low".to_string())
                .or_insert(65536);
        }

        if let Some(live_limits) = account
            .get("live_limited_models")
            .and_then(|value| value.as_object())
        {
            let now = chrono::Utc::now().timestamp();
            for (model_key, status) in live_limits {
                let Ok(status) = serde_json::from_value::<crate::models::account::LiveLimitStatus>(
                    status.clone(),
                ) else {
                    continue;
                };
                if !crate::proxy::rate_limit::is_active_persisted_long_limit(
                    model_key, &status, now,
                ) {
                    continue;
                }
                let (Ok(until_seconds), Ok(detected_at_seconds)) = (
                    u64::try_from(status.until),
                    u64::try_from(status.detected_at),
                ) else {
                    continue;
                };
                self.rate_limit_tracker.restore_persisted_long_limit(
                    &account_id,
                    std::time::SystemTime::UNIX_EPOCH
                        + std::time::Duration::from_secs(until_seconds),
                    std::time::SystemTime::UNIX_EPOCH
                        + std::time::Duration::from_secs(detected_at_seconds),
                    model_key,
                );
            }
        }

        // [NEW] 启动时自动同步持久化的淘汰模型路由表，注入热更新拦截器
        if let Some(rules) = account
            .get("quota")
            .and_then(|q| q.get("model_forwarding_rules"))
            .and_then(|r| r.as_object())
        {
            for (k, v) in rules {
                if let Some(new_model) = v.as_str() {
                    // Register dynamic forwarding rules (including those mapping to gemini-pro-agent)
                    crate::proxy::common::model_mapping::update_dynamic_forwarding_rules(
                        k.to_string(),
                        new_model.to_string(),
                    );
                }
            }
        }

        // Weekly availability is mandatory; the optional switch only controls 5h locks.
        self.sync_zero_quota_circuit_breaker(&account_id, &account);

        Ok(Some(ProxyToken {
            account_id,
            priority: crate::models::account::deserialize_priority(
                account.get("priority").unwrap_or(&serde_json::json!(
                    crate::models::account::default_priority()
                )),
            )
            .map_err(|e| format!("invalid account priority: {}", e))?,
            access_token,
            refresh_token,
            expires_in,
            timestamp,
            email,
            account_path: path.clone(),
            project_id,
            subscription_tier,
            remaining_quota,
            protected_models,
            health_score,
            reset_time,
            weekly_reset_time,
            validation_blocked: account
                .get("validation_blocked")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            validation_blocked_until: account
                .get("validation_blocked_until")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            validation_url: account
                .get("validation_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            model_quotas,
            model_limits,
            available_models,
            model_metadata,
        }))
    }

    /// 检查账号是否应该被配额保护
    /// 如果配额低于阈值，自动禁用账号并返回 true
    async fn check_and_protect_quota(
        &self,
        account_json: &mut serde_json::Value,
        account_path: &PathBuf,
    ) -> bool {
        // 1. 加载配额保护配置
        let config = match crate::modules::config::load_app_config() {
            Ok(cfg) => cfg.quota_protection,
            Err(_) => return false, // 配置加载失败，跳过保护
        };

        if !config.enabled {
            // [FIX] 当配额保护在全局关闭时，清空受保护模型列表，避免遗留锁定显示与调度过滤
            if let Some(arr) = account_json
                .get_mut("protected_models")
                .and_then(|v| v.as_array_mut())
            {
                if !arr.is_empty() {
                    arr.clear();
                    let _ = update_account_json(account_path, |latest| {
                        latest["protected_models"] = serde_json::Value::Array(Vec::new());
                    })
                    .await;
                }
            }
            return false; // 配额保护未启用
        }

        // 2. 获取配额信息
        // 注意：我们需要 clone 配额信息来遍历，避免借用冲突，但修改是针对 account_json 的
        let quota = match account_json.get("quota") {
            Some(q) => q.clone(),
            None => return false, // 无配额信息，跳过
        };

        // 3. [兼容性 #621] 检查是否被旧版账号级配额保护禁用,尝试恢复并转为模型级
        let is_proxy_disabled = account_json
            .get("proxy_disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let reason = account_json
            .get("proxy_disabled_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if is_proxy_disabled && reason == "quota_protection" {
            // 如果是被旧版账号级保护禁用的,尝试恢复并转为模型级
            return self
                .check_and_restore_quota(account_json, account_path, &quota, &config)
                .await;
        }

        // [修复 #1344] 不再处理其他禁用原因,让调用方负责检查手动禁用

        // 4. 获取模型列表
        let models = match quota.get("models").and_then(|m| m.as_array()) {
            Some(m) => m,
            None => return false,
        };

        // 5. [重构] 聚合判定逻辑：按 Standard ID 对账号所有型号进行分组
        // 解决如 Pro-Low (0%) 和 Pro-High (100%) 在同一账号内导致状态冲突的问题
        let mut group_max_percentage: HashMap<String, i32> = HashMap::new();

        for model in models {
            let name = model.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let percentage = model
                .get("percentage")
                .and_then(|v| v.as_i64())
                .unwrap_or(100) as i32;

            if let Some(std_id) =
                crate::proxy::common::model_mapping::normalize_to_standard_id(name)
            {
                let entry = group_max_percentage.entry(std_id).or_insert(-1);
                if percentage > *entry {
                    *entry = percentage;
                }
            }
        }

        // 6. 遍历受监控的 Standard ID，根据组内“最好状态”执行锁定或恢复
        let threshold = config.threshold_percentage as i32;
        let account_id = account_json
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let mut changed = false;

        for std_id in &config.monitored_models {
            // [FIX] 归一化监控模型为标准 ID（例如用户在 UI 选了 gemini-3.7-flash，对齐到 gemini-3-flash）
            let lookup_key = crate::proxy::common::model_mapping::normalize_to_standard_id(std_id)
                .unwrap_or_else(|| std_id.clone());

            // 获取该组的最高百分比，如果账号没该组型号则视为 100%
            let max_pct = group_max_percentage
                .get(&lookup_key)
                .cloned()
                .unwrap_or(100);

            if max_pct < threshold {
                // 只有组内所有模型都不行，才触发全组保护
                if self
                    .trigger_quota_protection(
                        account_json,
                        &account_id,
                        account_path,
                        max_pct,
                        threshold,
                        &lookup_key,
                    )
                    .await
                    .unwrap_or(false)
                {
                    changed = true;
                }
            } else {
                // 只有全组都好（或者没这型号），才尝试从之前受限状态恢复
                let protected_models = account_json
                    .get("protected_models")
                    .and_then(|v| v.as_array());

                let is_protected = protected_models.map_or(false, |arr| {
                    arr.iter().any(|m| m.as_str() == Some(lookup_key.as_str()))
                });

                if is_protected {
                    if self
                        .restore_quota_protection(
                            account_json,
                            &account_id,
                            account_path,
                            &lookup_key,
                        )
                        .await
                        .unwrap_or(false)
                    {
                        changed = true;
                    }
                }
            }
        }

        let _ = changed; // 避免 unused 警告，如果后续逻辑需要可以继续使用

        // 我们不再因为配额原因返回 true（即不再跳过账号），
        // 而是加载并在 get_token 时进行过滤。
        false
    }

    /// 计算账号的最大剩余配额百分比（用于排序）
    /// 返回值: Option<i32> (max_percentage)
    fn calculate_quota_stats(&self, quota: &serde_json::Value) -> Option<i32> {
        let models = match quota.get("models").and_then(|m| m.as_array()) {
            Some(m) => m,
            None => return None,
        };

        let mut max_percentage = 0;
        let mut has_data = false;

        for model in models {
            if let Some(pct) = model.get("percentage").and_then(|v| v.as_i64()) {
                let pct_i32 = pct as i32;
                if pct_i32 > max_percentage {
                    max_percentage = pct_i32;
                }
                has_data = true;
            }
        }

        if has_data {
            Some(max_percentage)
        } else {
            None
        }
    }

    /// 从磁盘读取特定模型的 quota 百分比 [FIX] 排序使用目标模型的 quota 而非 max
    ///
    /// # 参数
    /// * `account_path` - 账号 JSON 文件路径
    /// * `model_name` - 目标模型名称（已标准化）
    #[allow(dead_code)] // 预留给精确配额读取逻辑
    fn get_model_quota_from_json(account_path: &PathBuf, model_name: &str) -> Option<i32> {
        let content = std::fs::read_to_string(account_path).ok()?;
        let account: serde_json::Value = serde_json::from_str(&content).ok()?;
        let models = account.get("quota")?.get("models")?.as_array()?;

        let mut legacy_claude_quota = None;
        for model in models {
            if let Some(name) = model.get("name").and_then(|v| v.as_str()) {
                let norm = crate::proxy::common::model_mapping::normalize_to_standard_id(name)
                    .unwrap_or_else(|| name.to_string());
                if norm == model_name {
                    return model
                        .get("percentage")
                        .and_then(|v| v.as_i64())
                        .map(|p| p as i32);
                }
                // 向后兼容：若查询特定 Claude 家族 (如 claude-opus) 且尚未精确命中，当存在历史统一的 "claude" 配额时暂存作为兜底
                if model_name.starts_with("claude-") && (norm == "claude" || name == "claude") {
                    legacy_claude_quota = model
                        .get("percentage")
                        .and_then(|v| v.as_i64())
                        .map(|p| p as i32);
                }
            }
        }
        legacy_claude_quota
    }

    fn get_available_models_from_json(account_path: &PathBuf) -> Option<HashSet<String>> {
        let content = std::fs::read_to_string(account_path).ok()?;
        let account: serde_json::Value = serde_json::from_str(&content).ok()?;
        let mut result = HashSet::new();
        if let Some(models) = account
            .get("quota")
            .and_then(|q| q.get("models"))
            .and_then(|m| m.as_array())
        {
            for model in models {
                if model.get("disabled").and_then(|v| v.as_bool()) == Some(true) {
                    continue;
                }
                if let Some(name) = model.get("name").and_then(|v| v.as_str()) {
                    let normalized = name.trim().to_lowercase();
                    if normalized.contains("-disabled") || normalized.ends_with("_disabled") {
                        continue;
                    }
                    if !normalized.is_empty() {
                        result.insert(normalized);
                    }
                }
            }
        }
        // [NEW] 检查 quota_groups 是否包含 argon 分组（Ultra 账号自动授权）
        if let Some(groups) = account
            .get("quota")
            .and_then(|q| q.get("quota_groups").or_else(|| q.get("quotaGroups")))
            .and_then(|g| g.as_array())
        {
            let is_ultra = account
                .get("quota")
                .and_then(|q| {
                    q.get("subscription_tier")
                        .or_else(|| q.get("subscriptionTier"))
                })
                .and_then(|t| t.as_str())
                .map(|s| s.to_uppercase() == "ULTRA")
                .unwrap_or(false);

            let has_argon = groups.iter().any(|g| {
                let name = g
                    .get("display_name")
                    .or_else(|| g.get("displayName"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                let buckets = g.get("buckets").and_then(|b| b.as_array());
                name.contains("argon")
                    || buckets.map_or(false, |bs| {
                        bs.iter().any(|b| {
                            let b_id = b
                                .get("bucket_id")
                                .or_else(|| b.get("bucketId"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_lowercase();
                            let b_name = b
                                .get("display_name")
                                .or_else(|| b.get("displayName"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_lowercase();
                            b_id.contains("argon") || b_name.contains("argon")
                        })
                    })
            });
            if has_argon || is_ultra {
                result.insert("gemini-4-argon".to_string());
                result.insert("gemini-4-argon-high".to_string());
                result.insert("gemini-4-argon-medium".to_string());
                result.insert("gemini-4-argon-low".to_string());
            } else {
                result.remove("gemini-4-argon");
                result.remove("gemini-4-argon-high");
                result.remove("gemini-4-argon-medium");
                result.remove("gemini-4-argon-low");
            }
        }
        Some(result)
    }

    fn build_dynamic_model_candidates(model_name: &str) -> Option<Vec<String>> {
        let model = model_name.trim().to_lowercase();
        if model.is_empty() {
            return None;
        }

        // Image models: drift ONLY across versions within the SAME tier
        // (pro-image ↔ pro-image, flash-image ↔ flash-image). Never silently downgrade
        // pro→flash. If the account has no model in the requested tier, the name is left
        // unchanged and upstream returns 404 — which is honest (the account lacks that model).
        // To alias e.g. gemini-3-pro-image to a flash model, use the app's Model Routing Center.
        let pro_image = ["gemini-3-pro-image", "gemini-3.1-pro-image"];
        let flash_image = ["gemini-3-flash-image", "gemini-3.1-flash-image"];
        let is_pro_image = pro_image.contains(&model.as_str());
        let is_flash_image = flash_image.contains(&model.as_str());
        if is_pro_image || is_flash_image {
            let mut out = Vec::new();
            let mut seen = HashSet::new();
            let mut push = |candidate: &str| {
                let c = candidate.to_string();
                if seen.insert(c.clone()) {
                    out.push(c);
                }
            };
            push(&model); // requested first
            if is_pro_image {
                push("gemini-3.1-pro-image");
                push("gemini-3-pro-image");
            } else {
                push("gemini-3.1-flash-image");
                push("gemini-3-flash-image");
            }
            return Some(out);
        }

        let pro_family = [
            "gemini-3-pro",
            "gemini-3-pro-preview",
            "gemini-3-pro-high",
            "gemini-3-pro-low",
            "gemini-3.1-pro",
            "gemini-3.1-pro-preview",
            "gemini-3.1-pro-high",
            "gemini-3.1-pro-low",
            "gemini-pro-agent",
        ];

        if !pro_family.contains(&model.as_str()) {
            return None;
        }

        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut push = |candidate: &str| {
            let c = candidate.to_string();
            if seen.insert(c.clone()) {
                out.push(c);
            }
        };

        // Keep requested model as top priority, then fallback across the same family.
        // [FIX #3215] 优先回退到真实高阶模型 (high/preview)，避免优先选择输出上限仅 2k 的老旧 gemini-pro-agent
        push(&model);
        push("gemini-3.1-pro-high");
        push("gemini-3-pro-high");
        push("gemini-3.1-pro-preview");
        push("gemini-3-pro-preview");
        push("gemini-3.1-pro-low");
        push("gemini-3-pro-low");
        push("gemini-pro-agent");

        Some(out)
    }

    pub async fn resolve_dynamic_model_for_account(
        &self,
        account_id: &str,
        mapped_model: &str,
    ) -> String {
        let account_path = match self.tokens.get(account_id) {
            Some(token) => token.account_path.clone(),
            None => return mapped_model.to_string(),
        };

        let available_models = match Self::get_available_models_from_json(&account_path) {
            Some(models) if !models.is_empty() => models,
            _ => return mapped_model.to_string(),
        };

        let mapped_lower = mapped_model.trim().to_lowercase();
        if available_models.contains(&mapped_lower) {
            return mapped_model.to_string();
        }

        if let Some(candidates) = Self::build_dynamic_model_candidates(mapped_model) {
            for candidate in candidates {
                if available_models.contains(&candidate) {
                    if candidate != mapped_lower {
                        tracing::info!(
                            "[Dynamic-Model-Rewrite] account={} {} -> {}",
                            account_id,
                            mapped_model,
                            candidate
                        );
                    }
                    return candidate;
                }
            }
        }

        // 若传入的是无后缀派生裸模型（如 claude-opus-5-5, gemini-4-argon, claude-opus-4-6 等），
        // 严格在该账号实际支持的同基底档位变体中选择（绝不跨代串池）
        for suffix in ["tiered", "medium", "high", "low", "thinking"] {
            let candidate = format!("{}-{}", mapped_lower, suffix);
            if available_models.contains(&candidate) {
                tracing::info!(
                    "[Dynamic-Model-Rewrite] account={} bare model {} -> {}",
                    account_id,
                    mapped_model,
                    candidate
                );
                return candidate;
            }
        }

        mapped_model.to_string()
    }

    /// 测试辅助函数：公开访问 get_model_quota_from_json
    #[cfg(test)]
    pub fn get_model_quota_from_json_for_test(
        account_path: &PathBuf,
        model_name: &str,
    ) -> Option<i32> {
        Self::get_model_quota_from_json(account_path, model_name)
    }

    /// 触发配额保护，限制特定模型 (Issue #621)
    /// 返回 true 如果发生了改变
    async fn trigger_quota_protection(
        &self,
        account_json: &mut serde_json::Value,
        account_id: &str,
        account_path: &PathBuf,
        current_val: i32,
        threshold: i32,
        model_name: &str,
    ) -> Result<bool, String> {
        // 1. 初始化 protected_models 数组（如果不存在）
        if account_json.get("protected_models").is_none() {
            account_json["protected_models"] = serde_json::Value::Array(Vec::new());
        }

        let protected_models = account_json["protected_models"].as_array_mut().unwrap();

        // 2. 检查是否已存在
        if !protected_models
            .iter()
            .any(|m| m.as_str() == Some(model_name))
        {
            protected_models.push(serde_json::Value::String(model_name.to_string()));

            tracing::info!(
                "账号 {} 的模型 {} 因配额受限（{}% < {}%）已被加入保护列表",
                account_id,
                model_name,
                current_val,
                threshold
            );

            // 3. 写入磁盘
            let model_name_owned = model_name.to_string();
            update_account_json(account_path, move |latest| {
                if latest
                    .get("protected_models")
                    .and_then(|value| value.as_array())
                    .is_none()
                {
                    latest["protected_models"] = serde_json::Value::Array(Vec::new());
                }
                let protected_models = latest["protected_models"].as_array_mut().unwrap();
                if !protected_models
                    .iter()
                    .any(|model| model.as_str() == Some(&model_name_owned))
                {
                    protected_models.push(serde_json::Value::String(model_name_owned));
                }
            })
            .await?;

            // [FIX] 触发 TokenManager 的账号重新加载信号，确保内存中的 protected_models 同步
            crate::proxy::server::trigger_account_reload(account_id);

            return Ok(true);
        }

        Ok(false)
    }

    /// 检查并从账号级保护恢复（迁移至模型级，Issue #621）
    async fn check_and_restore_quota(
        &self,
        account_json: &mut serde_json::Value,
        account_path: &PathBuf,
        quota: &serde_json::Value,
        config: &crate::models::QuotaProtectionConfig,
    ) -> bool {
        // [兼容性] 如果该账号当前处于 proxy_disabled=true 且原因是 quota_protection，
        // 我们将其 proxy_disabled 设为 false，但同时更新其 protected_models 列表。
        tracing::info!(
            "正在迁移账号 {} 从全局配额保护模式至模型级保护模式",
            account_json
                .get("email")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
        );

        account_json["proxy_disabled"] = serde_json::Value::Bool(false);
        account_json["proxy_disabled_reason"] = serde_json::Value::Null;
        account_json["proxy_disabled_at"] = serde_json::Value::Null;

        let threshold = config.threshold_percentage as i32;
        let mut protected_list: Vec<serde_json::Value> = Vec::new();

        if let Some(models) = quota.get("models").and_then(|m| m.as_array()) {
            let mut group_max_percentage: HashMap<String, i32> = HashMap::new();

            for model in models {
                let name = model.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let percentage = model
                    .get("percentage")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;

                if let Some(std_id) =
                    crate::proxy::common::model_mapping::normalize_to_standard_id(name)
                {
                    let entry = group_max_percentage.entry(std_id).or_insert(-1);
                    if percentage > *entry {
                        *entry = percentage;
                    }
                }
            }

            for std_id in &config.monitored_models {
                let lookup_key =
                    crate::proxy::common::model_mapping::normalize_to_standard_id(std_id)
                        .unwrap_or_else(|| std_id.clone());
                let max_pct = group_max_percentage
                    .get(&lookup_key)
                    .cloned()
                    .unwrap_or(100);
                if max_pct < threshold
                    && !protected_list
                        .iter()
                        .any(|v| v.as_str() == Some(lookup_key.as_str()))
                {
                    protected_list.push(serde_json::Value::String(lookup_key));
                }
            }
        }

        account_json["protected_models"] = serde_json::Value::Array(protected_list.clone());

        let _ = update_account_json(account_path, |latest| {
            latest["proxy_disabled"] = serde_json::Value::Bool(false);
            latest["proxy_disabled_reason"] = serde_json::Value::Null;
            latest["proxy_disabled_at"] = serde_json::Value::Null;
            latest["protected_models"] = serde_json::Value::Array(protected_list);
        })
        .await;

        false // 返回 false 表示现在已可以尝试加载该账号（模型级过滤会在 get_token 时发生）
    }

    /// 恢复特定模型的配额保护 (Issue #621)
    /// 返回 true 如果发生了改变
    async fn restore_quota_protection(
        &self,
        account_json: &mut serde_json::Value,
        account_id: &str,
        account_path: &PathBuf,
        model_name: &str,
    ) -> Result<bool, String> {
        if let Some(arr) = account_json
            .get_mut("protected_models")
            .and_then(|v| v.as_array_mut())
        {
            let original_len = arr.len();
            arr.retain(|m| m.as_str() != Some(model_name));

            if arr.len() < original_len {
                tracing::info!(
                    "账号 {} 的模型 {} 配额已恢复，移出保护列表",
                    account_id,
                    model_name
                );
                let model_name_owned = model_name.to_string();
                update_account_json(account_path, move |latest| {
                    if let Some(protected_models) = latest
                        .get_mut("protected_models")
                        .and_then(|value| value.as_array_mut())
                    {
                        protected_models.retain(|model| model.as_str() != Some(&model_name_owned));
                    }
                })
                .await?;
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// 动态检测账号是否支持指定的高阶模型（如 Claude >= 5.0 系列）。
    ///
    /// 匹配策略：
    /// 1. 命中 raw model_limits（由官方配额 API 下发并缓存，包含如 `claude-sonnet-5-5-high` 等具体变体或基底名）；
    /// 2. 命中 model_quotas（归一化标准桶 ID）；
    /// 3. 大小写无关与前后缀模糊匹配（兼容带有 -high, -low, -medium, -thought 或 @default 后缀的变体）。
    pub fn token_supports_advanced_model(token: &ProxyToken, target_model: &str) -> bool {
        let target_lower = target_model.trim().to_lowercase();
        if target_lower.is_empty() {
            return false;
        }

        // 0. [NEW] 优先匹配 available_models（直接来自上游 fetchAvailableModels 的授权列表）
        if token.available_models.contains(&target_lower) {
            return true;
        }

        // 1. 精确匹配 model_limits
        if token.model_limits.contains_key(target_model)
            || token
                .model_limits
                .keys()
                .any(|k| k.eq_ignore_ascii_case(target_model))
        {
            return true;
        }

        // 2. 提取去除前后缀的基础模型前缀（例如 claude-sonnet-5-5-high -> claude-sonnet-5-5）
        let base_pattern = target_lower
            .replace("-extra-low", "")
            .replace("-low", "")
            .replace("-medium", "")
            .replace("-high", "")
            .replace("-xhigh", "")
            .replace("-thinking", "")
            .replace("-tiered", "")
            .replace("@default", "");

        // 3. 检查 model_limits 中是否有同属该模型系列的条目
        if token.model_limits.keys().any(|k| {
            let k_lower = k.to_lowercase();
            k_lower == target_lower
                || k_lower == base_pattern
                || (!base_pattern.is_empty() && k_lower.contains(&base_pattern))
                || (!target_lower.is_empty() && k_lower.contains(&target_lower))
        }) {
            return true;
        }

        // 4. 检查 model_quotas（如果标准化匹配成功）
        let normalized =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());
        if token.model_quotas.contains_key(&normalized) && !base_pattern.is_empty() {
            // 如果归一化后是标准组（如 "claude"），仅当 model_limits 同样存在该高阶模型痕迹或完全未拉取到 limits 时作为辅助参考
            if normalized != "claude" {
                return true;
            }
        }

        false
    }

    /// P2C 算法的候选池大小 - 从前 N 个最优候选中随机选择
    const P2C_POOL_SIZE: usize = 5;

    /// 临期配额提权最低门槛（百分比）：<= 10% 的濒危额度不享受 EDF 提权，保护健康账号
    pub const IMMINENT_QUOTA_FLOOR: i32 = 10;
    /// 周配额临期检测窗口（秒）：24小时
    pub const WEEKLY_IMMINENT_HORIZON_SECS: i64 = 86400;
    /// 短期（5小时滚动）配额临期检测窗口（秒）：2小时
    pub const SHORT_IMMINENT_HORIZON_SECS: i64 = 7200;

    /// 计算账号针对目标模型的临期截止时间戳。
    /// 若配额已处于濒危水平 (<= 10%) 或未进入临期窗口，则返回 None。
    pub fn get_imminent_deadline(t: &ProxyToken, q: i32, now_ts: i64) -> Option<i64> {
        if q <= Self::IMMINENT_QUOTA_FLOOR {
            return None;
        }
        if let Some(w) = t.weekly_reset_time {
            if w > now_ts && (w - now_ts) <= Self::WEEKLY_IMMINENT_HORIZON_SECS {
                return Some(w);
            }
        }
        if let Some(r) = t.reset_time {
            if r > now_ts && (r - now_ts) <= Self::SHORT_IMMINENT_HORIZON_SECS {
                return Some(r);
            }
        }
        None
    }

    /// Power of 2 Choices (P2C) 选择算法
    /// 从前 5 个候选中随机选 2 个，选择配额更高的 -> 避免热点
    /// 返回选中的索引
    ///
    /// # 参数
    /// * `candidates` - 已排序的候选 token 列表
    /// * `attempted` - 已尝试失败的账号 ID 集合
    /// * `normalized_target` - 归一化后的目标模型名
    /// * `quota_protection_enabled` - 是否启用配额保护
    fn select_with_p2c<'a>(
        &self,
        candidates: &'a [ProxyToken],
        attempted: &HashSet<String>,
        normalized_target: &str,
        quota_protection_enabled: bool,
    ) -> Option<&'a ProxyToken> {
        use rand::Rng;

        // 过滤可用 token
        let mut available: Vec<&ProxyToken> = candidates
            .iter()
            .filter(|t| !attempted.contains(&t.account_id))
            .filter(|t| {
                !quota_protection_enabled || !t.protected_models.contains(normalized_target)
            })
            .collect();

        if available.is_empty() {
            return None;
        }

        // [QoS Guardian] 剔除处于小黑屋隔离与正在探针探测中的受限账号
        let before_jail = available.len();
        available.retain(|t| !self.qos_guardian.is_jailed_or_probing(&t.account_id));
        let jailed = before_jail.saturating_sub(available.len());
        if jailed > 0 {
            if available.is_empty() {
                // 全池都被关小黑屋：这是“没号可用”的真正原因，必须进 error.log
                tracing::error!(
                    "[Scheduler] 全部 {} 个候选账号均处于 QoS 小黑屋/探测中 | model={}",
                    jailed,
                    normalized_target
                );
            } else {
                tracing::debug!(
                    "[Scheduler] {} 个账号因 QoS 小黑屋/探测被排除 | model={} 剩余候选={}",
                    jailed,
                    normalized_target,
                    available.len()
                );
            }
        }

        // [QoS Guardian] 优先调度纯 Healthy 账号组，避开 Suspicious（只出不进）与 HalfOpen（缓刑试探）
        let pure_healthy: Vec<&ProxyToken> = available
            .iter()
            .filter(|t| self.qos_guardian.is_healthy(&t.account_id))
            .copied()
            .collect();
        if !pure_healthy.is_empty() {
            available = pure_healthy;
        }

        if available.is_empty() {
            return None;
        }

        // [NEW] 分流账号上限截断 (Max Spillover Accounts Cap)
        // 0 表示不限制；> 0 表示池中最多允许前 N 个健康账号参与并发调度/外溢
        let max_spillover = self
            .sticky_config
            .try_read()
            .map(|c| c.max_spillover_accounts)
            .unwrap_or(0);

        if max_spillover > 0 {
            let mut distinct_account_ids: Vec<String> = Vec::new();
            let mut sorted_pool = available.clone();
            // 排序规则：优先级数字越小越优先；相同优先级下，在途请求数越小越优先；最后按 account_id 稳定排序
            sorted_pool.sort_by(|a, b| {
                a.priority
                    .cmp(&b.priority)
                    .then_with(|| {
                        self.get_in_flight(&a.account_id)
                            .cmp(&self.get_in_flight(&b.account_id))
                    })
                    .then_with(|| a.account_id.cmp(&b.account_id))
            });
            for t in sorted_pool {
                if !distinct_account_ids.contains(&t.account_id) {
                    distinct_account_ids.push(t.account_id.clone());
                    if distinct_account_ids.len() == max_spillover {
                        break;
                    }
                }
            }
            let allowed_set: HashSet<String> = distinct_account_ids.into_iter().collect();
            available.retain(|t| allowed_set.contains(&t.account_id));
        }

        // Keep lower-priority groups for retries; only this draw is restricted.
        // [NEW] 并发感知与次级分流加速 (Concurrency-Aware & Priority Spillover Acceleration)：
        // 1. 获取当前可用池中最高优先级 min_priority
        if let Some(min_priority) = available.iter().map(|t| t.priority).min() {
            // 检查最高优先级是否有完全空闲（在途请求数 == 0）的健康账号
            let min_p_idle: Vec<&ProxyToken> = available
                .iter()
                .filter(|t| t.priority == min_priority && self.get_in_flight(&t.account_id) == 0)
                .copied()
                .collect();

            if !min_p_idle.is_empty() {
                // 最高优先级拥有空闲算力，直接锁定在最高优先级的空闲账号组中
                available = min_p_idle;
            } else {
                // 最高优先级的所有账号均处于在途处理中 (in_flight > 0)！
                // 检查更低优先级 (priority > min_priority) 中是否存在完全空闲 (in_flight == 0) 的健康账号
                let lower_idle: Vec<&ProxyToken> = available
                    .iter()
                    .filter(|t| t.priority > min_priority && self.get_in_flight(&t.account_id) == 0)
                    .copied()
                    .collect();

                if let Some(next_min_p) = lower_idle.iter().map(|t| t.priority).min() {
                    // 🚀 触发多会话分流加速：新会话平滑外溢到次高优先级的空闲账号组，打破单号串行排队阻塞！
                    tracing::info!(
                        "🚀 [Concurrency Spillover] Priority {} accounts are busy (in_flight > 0). Offloading new conversation to idle priority {} account",
                        min_priority, next_min_p
                    );
                    available = lower_idle
                        .into_iter()
                        .filter(|t| t.priority == next_min_p)
                        .collect();
                } else {
                    // 全池账号皆处于忙碌状态，按最低在途并发度 (Least-Loaded) 筛选，防单号被压垮
                    let min_inflight = available
                        .iter()
                        .map(|t| self.get_in_flight(&t.account_id))
                        .min()
                        .unwrap_or(0);
                    available.retain(|t| self.get_in_flight(&t.account_id) == min_inflight);
                    // 在相同最低并发度的账号中，恢复优先级优选
                    if let Some(best_p) = available.iter().map(|t| t.priority).min() {
                        available.retain(|t| t.priority == best_p);
                    }
                }
            }
        }

        // [FIX #3639] 优先保留对目标模型具备正配额 (> 0%) 的账号，防止抽中已知 0% 额度的账号导致 429
        let has_positive_model_quota = available
            .iter()
            .any(|t| t.model_quotas.get(normalized_target).copied().unwrap_or(0) > 0);
        if has_positive_model_quota {
            available.retain(|t| t.model_quotas.get(normalized_target).copied().unwrap_or(0) > 0);
        }

        if available.len() == 1 {
            return Some(available[0]);
        }

        // P2C: 从前 min(P2C_POOL_SIZE, len) 个中随机选 2 个
        let pool_size = available.len().min(Self::P2C_POOL_SIZE);
        let mut rng = rand::thread_rng();

        let pick1 = rng.gen_range(0..pool_size);
        let pick2 = rng.gen_range(0..pool_size);
        // 确保选择不同的两个候选
        let pick2 = if pick2 == pick1 {
            (pick1 + 1) % pool_size
        } else {
            pick2
        };

        let c1 = available[pick1];
        let c2 = available[pick2];

        // 目标模型配额提取
        let q1 = c1
            .model_quotas
            .get(normalized_target)
            .copied()
            .unwrap_or_else(|| c1.remaining_quota.unwrap_or(0));
        let q2 = c2
            .model_quotas
            .get(normalized_target)
            .copied()
            .unwrap_or_else(|| c2.remaining_quota.unwrap_or(0));

        // P2C 临期感知选择：若候选账号存在临期充足配额 (<= 24h 重置且 q > 10%)，优先抽中该临期账号防止配额作废
        let now_ts = chrono::Utc::now().timestamp();
        let imm1 = Self::get_imminent_deadline(c1, q1, now_ts);
        let imm2 = Self::get_imminent_deadline(c2, q2, now_ts);

        // 终审比较：在途并发更少者绝对优先；在途相同时结合临期与剩余配额比较
        let inflight1 = self.get_in_flight(&c1.account_id);
        let inflight2 = self.get_in_flight(&c2.account_id);
        let selected = if inflight1 != inflight2 {
            if inflight1 < inflight2 {
                c1
            } else {
                c2
            }
        } else {
            match (imm1, imm2) {
                (Some(ts1), Some(ts2)) => {
                    if ts1 <= ts2 {
                        c1
                    } else {
                        c2
                    }
                }
                (Some(_), None) => c1,
                (None, Some(_)) => c2,
                (None, None) => {
                    if q1 >= q2 {
                        c1
                    } else {
                        c2
                    }
                }
            }
        };

        tracing::debug!(
            "🎲 [P2C] Selected {} (in_flight={}, quota={}%) from [{}(in_flight={}, quota={}%), {}(in_flight={}, quota={}%)]",
            selected.email,
            self.get_in_flight(&selected.account_id),
            selected.remaining_quota.unwrap_or(0),
            c1.email,
            inflight1,
            c1.remaining_quota.unwrap_or(0),
            c2.email,
            inflight2,
            c2.remaining_quota.unwrap_or(0)
        );

        Some(selected)
    }

    /// 先发送取消信号，再带超时等待任务完成
    ///
    /// # 参数
    /// * `timeout` - 等待任务完成的超时时间
    pub async fn graceful_shutdown(&self, timeout: std::time::Duration) {
        tracing::info!("Initiating graceful shutdown of background tasks...");

        // 发送取消信号给所有后台任务
        self.cancel_token.cancel();

        // 带超时等待任务完成
        match tokio::time::timeout(timeout, self.abort_background_tasks()).await {
            Ok(_) => tracing::info!("All background tasks cleaned up gracefully"),
            Err(_) => tracing::warn!(
                "Graceful cleanup timed out after {:?}, tasks were force-aborted",
                timeout
            ),
        }
    }

    /// 中止并等待所有后台任务完成
    /// abort() 仅设置取消标志，必须 await 确认清理完成
    pub async fn abort_background_tasks(&self) {
        Self::abort_task(&self.auto_cleanup_handle, "Auto-cleanup task").await;
        Self::abort_task(
            &self.proactive_refresh_handle,
            "Proactive token refresh task",
        )
        .await;
        Self::abort_task(&self.qos_probe_handle, "QoS probing daemon task").await;
    }

    /// 中止单个后台任务并记录结果
    ///
    /// # 参数
    /// * `handle` - 任务句柄的 Mutex 引用
    /// * `task_name` - 任务名称（用于日志）
    async fn abort_task(
        handle: &tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
        task_name: &str,
    ) {
        let Some(handle) = handle.lock().await.take() else {
            return;
        };

        handle.abort();
        match handle.await {
            Ok(()) => tracing::debug!("{} completed", task_name),
            Err(e) if e.is_cancelled() => tracing::info!("{} aborted", task_name),
            Err(e) => tracing::warn!("{} error: {}", task_name, e),
        }
    }

    /// 获取当前可用的 Token（支持粘性会话与智能调度）
    /// 参数 `quota_group` 用于区分 "claude" vs "gemini" 组
    /// 参数 `force_rotate` 为 true 时将忽略锁定，强制切换账号
    /// 参数 `session_id` 用于跨请求维持会话粘性
    /// 参数 `target_model` 用于检查配额保护 (Issue #621)
    pub async fn get_token(
        &self,
        quota_group: &str,
        force_rotate: bool,
        session_id: Option<&str>,
        target_model: &str,
    ) -> Result<(String, String, String, String, u64), String> {
        let excluded_accounts = HashSet::new();
        self.get_token_filtered(
            quota_group,
            force_rotate,
            session_id,
            target_model,
            &excluded_accounts,
        )
        .await
    }

    pub async fn get_image_token(
        &self,
        force_rotate: bool,
        session_id: Option<&str>,
        target_model: &str,
        scheduler: &Arc<ImageScheduler>,
        request_timeout: u64,
    ) -> Result<(String, String, String, String, u64, ImagePermit), (StatusCode, String)> {
        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_secs(request_timeout);
        let mut scheduler_changes = scheduler.subscribe_changes();

        loop {
            scheduler_changes.borrow_and_update();
            let mut busy_accounts = HashSet::new();

            loop {
                let selection = wait_for_image_token_selection(
                    deadline,
                    self.get_token_filtered(
                        "image_gen",
                        force_rotate,
                        session_id,
                        target_model,
                        &busy_accounts,
                    ),
                )
                .await;
                match selection {
                    None => {
                        return Err((
                            StatusCode::TOO_MANY_REQUESTS,
                            "图片队列等待超时".to_string(),
                        ));
                    }
                    Some(Ok((access_token, project_id, email, account_id, wait_ms))) => {
                        if let Some(permit) = scheduler.try_acquire(&account_id) {
                            return Ok((
                                access_token,
                                project_id,
                                email,
                                account_id,
                                wait_ms,
                                permit,
                            ));
                        }
                        busy_accounts.insert(account_id);
                    }
                    Some(Err(selection_error)) => {
                        // [FIX #3329, #3409] 图像队列降级与瞬态限流救援：
                        // 如果选号错误是临时的全账号限流冷却（如 "All accounts limited. Wait 2s."），
                        // 且距离请求截止时间 (deadline) 仍有充裕预算，则等待冷却时长后自动重选，
                        // 绝不直接向前端或客户端抛出 503 SERVICE_UNAVAILABLE 导致生图失败！
                        if let Some(wait_sec) =
                            crate::proxy::handlers::common::extract_retry_after_seconds(
                                &selection_error,
                            )
                        {
                            let wait_dur = std::time::Duration::from_secs(wait_sec.clamp(1, 10));
                            let remaining =
                                deadline.saturating_duration_since(tokio::time::Instant::now());
                            if remaining > wait_dur {
                                tracing::info!(
                                    "[ImageScheduler] All accounts in transient cooldown ({}s), waiting {}s within timeout budget to rescue image request",
                                    wait_sec, wait_dur.as_secs()
                                );
                                tokio::time::sleep(wait_dur).await;
                                busy_accounts.clear();
                                continue;
                            }
                        }

                        if busy_accounts.is_empty() {
                            return Err((
                                StatusCode::SERVICE_UNAVAILABLE,
                                format!("Token error: {}", selection_error),
                            ));
                        }
                        break;
                    }
                }
            }

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if !wait_for_image_account_change(&mut scheduler_changes, remaining).await {
                return Err((
                    StatusCode::TOO_MANY_REQUESTS,
                    "图片队列等待超时".to_string(),
                ));
            }
        }
    }

    async fn get_token_filtered(
        &self,
        quota_group: &str,
        force_rotate: bool,
        session_id: Option<&str>,
        target_model: &str,
        excluded_accounts: &HashSet<String>,
    ) -> Result<(String, String, String, String, u64), String> {
        // [FIX] 检查并处理待重新加载的账号（配额保护同步）
        let pending_reload = crate::proxy::server::take_pending_reload_accounts();
        for account_id in pending_reload {
            if let Err(e) = self.reload_account(&account_id).await {
                tracing::warn!("[Quota] Failed to reload account {}: {}", account_id, e);
            } else {
                tracing::info!(
                    "[Quota] Reloaded account {} (protected_models synced)",
                    account_id
                );
            }
        }

        // [FIX #1477] 检查并处理待删除的账号（彻底清理缓存）
        let pending_delete = crate::proxy::server::take_pending_delete_accounts();
        for account_id in pending_delete {
            self.remove_account(&account_id);
            tracing::info!(
                "[Proxy] Purged deleted account {} from all caches",
                account_id
            );
        }

        // 【优化 Issue #284】添加 15 秒超时，防止死锁（为多账号热路径刷新容灾留足窗口）
        let timeout_duration = std::time::Duration::from_secs(15);
        match tokio::time::timeout(
            timeout_duration,
            self.get_token_internal(
                quota_group,
                force_rotate,
                session_id,
                target_model,
                excluded_accounts,
            ),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(
                "Token acquisition timeout (15s) - system too busy or deadlock detected"
                    .to_string(),
            ),
        }
    }

    /// 内部实现：获取 Token 的核心逻辑
    async fn get_token_internal(
        &self,
        quota_group: &str,
        force_rotate: bool,
        session_id: Option<&str>,
        target_model: &str,
        excluded_accounts: &HashSet<String>,
    ) -> Result<(String, String, String, String, u64), String> {
        let mut tokens_snapshot: Vec<ProxyToken> =
            self.tokens.iter().map(|e| e.value().clone()).collect();
        tokens_snapshot.retain(|token| !excluded_accounts.contains(&token.account_id));
        let mut total = tokens_snapshot.len();
        if total == 0 {
            return Err("Token pool is empty".to_string());
        }

        // [NEW] 1. 动态能力过滤 (Capability-First Dynamic Discovery)
        // 彻底摆脱对具体模型名称或静态版本号的依赖：
        // 优先检查池中是否有账号明确具备该模型（基于上游 fetchAvailableModels 下发的 available_models / model_limits）。
        // - 若池中有明确具备真实授权的账号，候选池瞬间精准收敛至具备能力的账号，彻底杜绝路由到未开通账号；
        // - 若池中所有账号尚未拉取到具体模型（如冷启动），则根据动态特征（高级订阅专享分布或通用通配）优雅降级为 PRO/ULTRA 门禁。
        let target_lower = target_model.trim().to_lowercase();
        let base_name = crate::proxy::common::model_mapping::strip_tier_suffix(&target_lower);

        // 1.1 动态探测：池中是否有任何账号明确支持该模型？
        let accounts_with_capability: Vec<_> = tokens_snapshot
            .iter()
            .filter(|t| {
                !self.is_model_unsupported(&t.account_id, target_model)
                    && (t.available_models.contains(&target_lower)
                        || t.available_models.contains(&base_name)
                        || Self::token_supports_advanced_model(t, target_model))
            })
            .cloned()
            .collect();

        let is_advanced_canary = if !accounts_with_capability.is_empty() {
            // 只要池中有明确支持的账号，且并非所有 FREE 账号都普惠支持（例如只在 PRO/ULTRA 账号中出现），
            // 就直接将候选池收敛到这些具备实际能力的账号！
            let only_in_paid_tiers = accounts_with_capability.iter().all(|t| {
                matches!(
                    t.subscription_tier
                        .as_deref()
                        .map(str::to_uppercase)
                        .as_deref(),
                    Some("PRO") | Some("ULTRA")
                )
            });
            // 如果仅在付费账号中出现，或者只有部分账号拥有（灰度特征），均触发精准收敛
            only_in_paid_tiers || accounts_with_capability.len() < tokens_snapshot.len()
        } else {
            // 冷启动或上游未显式列出：使用通用通配规则推导（不再写死具体型号/代号）
            // 匹配非基础轻量模型的高级特征（带有 pro/opus/ultra 或新一代大版本通配）
            let is_generic_advanced = target_lower.contains("opus")
                || target_lower.contains("ultra")
                || target_lower.contains("pro")
                || (!target_lower.contains("flash") && !target_lower.contains("lite"))
                || crate::proxy::model_specs::is_claude_v5_or_above(target_model)
                || crate::proxy::model_specs::is_gemini_v4_or_above(target_model);
            is_generic_advanced
        };

        if is_advanced_canary {
            let before_tier = tokens_snapshot.len();

            // 1.2 若池中有明确具备真实授权的账号，直接精准收敛！彻底杜绝将请求路由到未开通的高阶模型账号！
            if !accounts_with_capability.is_empty() {
                tokens_snapshot = accounts_with_capability;
            } else {
                // 冷启动：先行保留符合基本高级订阅资质的账号 (PRO / ULTRA)
                tokens_snapshot.retain(|t| {
                    matches!(
                        t.subscription_tier
                            .as_deref()
                            .map(str::to_uppercase)
                            .as_deref(),
                        Some("PRO") | Some("ULTRA")
                    )
                });
                if tokens_snapshot.is_empty() {
                    if before_tier > 0 {
                        tracing::warn!(
                            "Model {} requires PRO or ULTRA subscription, but no PRO/ULTRA accounts available in pool",
                            target_model
                        );
                        return Err(format!(
                            "Model {} requires PRO or ULTRA subscription, but no PRO/ULTRA accounts are available",
                            target_model
                        ));
                    }
                    return Err("Token pool is empty".to_string());
                }

                // 排除处于该模型临时熔断期的账号
                tokens_snapshot.retain(|t| !self.is_model_unsupported(&t.account_id, target_model));
                if tokens_snapshot.is_empty() {
                    tracing::warn!(
                        "所有 PRO/ULTRA 账号针对模型 {} 均处于临时熔断期或无配额权限",
                        target_model
                    );
                    return Err(format!(
                        "All candidate accounts temporarily suspended for model: {}",
                        target_model
                    ));
                }
                tracing::debug!(
                    "No accounts explicitly reported '{}' in model_limits yet; falling back to PRO/ULTRA subscription tier pool (excluding model-circuit-broken)",
                    target_model
                );
            }
        }

        // 归一化目标模型名为标准 ID（仅用于配额排序权重，不再作为访问门禁）
        let normalized_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());

        // ── 动态模型授权过滤 ────────────────────────────────────────────────
        // 替代旧的硬编码 model_quotas 门禁，改用三步动态匹配：
        //   Step A: 精确命中 available_models（原始 ID，大小写无关）
        //   Step B: 基名匹配（剥离档位后缀后的基底模型名命中 available_models）
        //   Step C: 归一化桶兜底（available_models 为空时退回旧 model_quotas 逻辑，
        //           覆盖冷启动 / 旧账号 / model_quotas 已覆盖的经典模型）
        // 只要上游 fetchAvailableModels 下发了授权列表，任何新模型（无论名称如何）都
        // 自动命中 Step A/B，无需修改任何硬编码字典。
        let target_lower = target_model.trim().to_lowercase();
        let base_name = crate::proxy::common::model_mapping::strip_tier_suffix(&target_lower);
        // 判断池中是否已有任何账号完成了 available_models 刷新（区分冷启动 vs. 已知空集）
        let any_loaded = tokens_snapshot
            .iter()
            .any(|t| !t.available_models.is_empty());

        let candidate_count_before = tokens_snapshot.len();

        if any_loaded {
            // 至少有一个账号已刷新配额目录 → 严格按账号实际支持的 available_models 过滤（严禁跨世代/跨家族串池，如 5.5 串到 4.6 账号池或反之）
            tokens_snapshot.retain(|t| {
                if t.available_models.is_empty() {
                    // 该账号尚未刷新 available_models（冷启动未刷新的账号）：退回 model_quotas 归一化桶兜底
                    return t.model_quotas.contains_key(&normalized_target);
                }
                // Step A: 精确匹配（含档位后缀的完整名，如 gemini-4-argon-high / claude-opus-5-5-high / claude-opus-4-6-thinking）
                if t.available_models.contains(&target_lower) {
                    return true;
                }
                // Step B: 基名匹配（剥离档位后缀，如 gemini-4-argon-high → gemini-4-argon，或 claude-opus-4-6 → claude-opus-4-6-thinking）
                if !base_name.is_empty() {
                    if t.available_models.contains(&base_name) {
                        return true;
                    }
                    if t.available_models.iter().any(|m| {
                        let m_base = crate::proxy::common::model_mapping::strip_tier_suffix(m);
                        m == &base_name
                            || m_base == base_name
                            || m.starts_with(&format!("{}-", base_name))
                            || m.starts_with(&format!("{}@", base_name))
                    }) {
                        return true;
                    }
                }
                // Step C: 高阶模型与专属独立配额组能力推导（对齐 line 2613 accounts_with_capability 判定，涵盖 Argon 独立配额组）
                if Self::token_supports_advanced_model(t, target_model) {
                    return true;
                }
                // Step D: 独立计费桶归一化匹配（排除泛化宽泛桶如 claude 避免串池，支持专属桶如 gemini-4-argon 等）
                if normalized_target != "claude" && t.model_quotas.contains_key(&normalized_target)
                {
                    return true;
                }
                false
            });
        } else {
            // 冷启动：所有账号均未刷新 available_models
            // 退回 model_quotas 归一化桶逻辑（与旧行为完全一致，保证经典模型正常调度）
            tokens_snapshot.retain(|t| t.model_quotas.contains_key(&normalized_target));
        }

        if tokens_snapshot.is_empty() {
            if candidate_count_before > 0 {
                tracing::warn!(
                    "No accounts available with capability for model: {} (base: {}, normalized: {}, any_loaded: {})",
                    target_model, base_name, normalized_target, any_loaded
                );
                return Err(format!(
                    "No accounts available with quota for model: {}",
                    target_model
                ));
            }
            return Err("Token pool is empty".to_string());
        }

        tokens_snapshot.sort_by(|a, b| {
            let priority_cmp = a.priority.cmp(&b.priority);
            if priority_cmp != std::cmp::Ordering::Equal {
                return priority_cmp;
            }

            // 对于高阶灰度模型 (Claude >= 5.0, Gemini >= 4.0 Argon 等)：明确包含该具体模型（由定时刷新下发）的账号获得最高调度优先级
            if is_advanced_canary {
                let has_cap_a = Self::token_supports_advanced_model(a, target_model);
                let has_cap_b = Self::token_supports_advanced_model(b, target_model);
                if has_cap_a != has_cap_b {
                    return has_cap_b.cmp(&has_cap_a);
                }
            }

            // Priority 0: 严格的订阅等级排序 (ULTRA > PRO > FREE)
            // 用户要求：轮询应当遵循 Ultra -> Pro -> Free
            // 既然已经过滤掉了不支持该模型的账号，剩下的都是支持的
            // 此时我们优先使用高级订阅
            // 统一走 models::quota::tier_priority，保证与 UI / 配额解析使用同一套关键词表。
            // 未知等级一律按 FREE 处理（不再返回 3），否则会出现「UI 显示 FREE、
            // 调度器却把它排在 FREE 之后」的隐形档位。
            let tier_priority =
                |tier: &Option<String>| crate::models::quota::tier_priority(tier.as_deref());

            let tier_cmp =
                tier_priority(&a.subscription_tier).cmp(&tier_priority(&b.subscription_tier));
            if tier_cmp != std::cmp::Ordering::Equal {
                return tier_cmp;
            }

            // 提取目标模型配额
            let quota_a = a.model_quotas.get(&normalized_target).copied().unwrap_or(0);
            let quota_b = b.model_quotas.get(&normalized_target).copied().unwrap_or(0);

            // [NEW] Priority 0.5: 临期配额优先消耗 (Earliest Deadline First / Use-It-Or-Lose-It)
            // 针对 Gemini / Claude 账号池的核心经济学优化：
            // Google 对 Pro/Ultra 账号设置了周额度上限（7d 窗口）和 5h 滚动窗口。
            // 当一个账号的周额度即将重置（例如 < 24 小时）且该账号仍有充足可用额度 (> 10%) 时，
            // 若不尽快消耗该账号，未用配额将在周期重置时被彻底清零作废！
            // 相反，重置时间还遥遥无期（如 5~7 天后）的账号即使配额多，也应当退居其后，让临期配额先行释放。
            // 注意：若账号剩余额度已濒危（<= 10%），绝不可被提权抢占健康账号，
            // 否则会快速耗尽单次请求的 429 容错重试配额，导致健康账号未被尝试即整体报错。
            let now_ts = chrono::Utc::now().timestamp();
            let imminent_a = Self::get_imminent_deadline(a, quota_a, now_ts);
            let imminent_b = Self::get_imminent_deadline(b, quota_b, now_ts);

            match (imminent_a, imminent_b) {
                (Some(ts_a), Some(ts_b)) => {
                    // 两者皆处于临期窗口：最早过期者优先使用 (Earliest Deadline First)
                    let deadline_cmp = ts_a.cmp(&ts_b);
                    if deadline_cmp != std::cmp::Ordering::Equal {
                        return deadline_cmp;
                    }
                }
                (Some(_), None) => return std::cmp::Ordering::Less, // a 临期，优先消耗 a
                (None, Some(_)) => return std::cmp::Ordering::Greater, // b 临期，优先消耗 b
                (None, None) => {}                                  // 皆非临期，走正常配额比对
            }

            // Priority 1: 目标模型的 quota (higher is better) -> 保护低配额账号
            // 经过过滤，key 肯定存在
            let quota_cmp = quota_b.cmp(&quota_a);
            if quota_cmp != std::cmp::Ordering::Equal {
                return quota_cmp;
            }

            // Priority 2: Health score (higher is better)
            let health_cmp = b
                .health_score
                .partial_cmp(&a.health_score)
                .unwrap_or(std::cmp::Ordering::Equal);
            if health_cmp != std::cmp::Ordering::Equal {
                return health_cmp;
            }

            // Priority 3: Reset time (earlier is better) [Fix #3570]
            let reset_a = a.reset_time.unwrap_or(i64::MAX);
            let reset_b = b.reset_time.unwrap_or(i64::MAX);
            let reset_cmp = reset_a.cmp(&reset_b);
            if reset_cmp != std::cmp::Ordering::Equal {
                return reset_cmp;
            }

            // Priority 4: Deterministic fallback (Tie-breaker for strict total order)
            a.account_id.cmp(&b.account_id)
        });

        // 【调试日志】打印排序后的账号顺序（显示目标模型的 quota）
        tracing::debug!(
            "🔄 [Token Rotation] target={} Accounts: {:?}",
            normalized_target,
            tokens_snapshot
                .iter()
                .map(|t| format!(
                    "{}(quota={}%, reset={:?}, health={:.2})",
                    t.email,
                    t.model_quotas.get(&normalized_target).copied().unwrap_or(0),
                    t.reset_time.map(|ts| {
                        let now = chrono::Utc::now().timestamp();
                        let diff_secs = ts - now;
                        if diff_secs > 0 {
                            format!("{}m", diff_secs / 60)
                        } else {
                            "now".to_string()
                        }
                    }),
                    t.health_score
                ))
                .collect::<Vec<_>>()
        );

        // 0. 读取当前调度配置
        let scheduling = self.sticky_config.read().await.clone();
        use crate::proxy::sticky_config::SchedulingMode;

        // 【新增】检查配额保护是否启用（如果关闭，则忽略 protected_models 检查）
        let quota_protection_enabled = crate::modules::config::load_app_config()
            .map(|cfg| cfg.quota_protection.enabled)
            .unwrap_or(false);

        // ===== [FIX #820] 固定账号模式：优先使用指定账号 =====
        let preferred_id = self.preferred_account_id.read().await.clone();
        if !force_rotate {
            if let Some(ref pref_id) = preferred_id {
                // 查找优先账号
                if let Some(preferred_token) = tokens_snapshot
                    .iter()
                    .find(|t| &t.account_id == pref_id)
                    .cloned()
                {
                    // 检查账号是否可用（未限流、未被配额保护）
                    match Self::get_account_state_on_disk(&preferred_token.account_path).await {
                        OnDiskAccountState::Disabled => {
                            tracing::warn!(
                                "🔒 [FIX #820] Preferred account {} is disabled on disk, purging and falling back",
                                preferred_token.email
                            );
                            self.remove_account(&preferred_token.account_id);
                            tokens_snapshot.retain(|t| t.account_id != preferred_token.account_id);
                            total = tokens_snapshot.len();

                            {
                                let mut preferred = self.preferred_account_id.write().await;
                                if preferred.as_deref() == Some(pref_id.as_str()) {
                                    *preferred = None;
                                }
                            }

                            if total == 0 {
                                return Err("Token pool is empty".to_string());
                            }
                        }
                        OnDiskAccountState::Unknown => {
                            tracing::warn!(
                                "🔒 [FIX #820] Preferred account {} state on disk is unavailable, falling back",
                                preferred_token.email
                            );
                            // Don't purge on transient read/parse failures; just skip this token for this request.
                            tokens_snapshot.retain(|t| t.account_id != preferred_token.account_id);
                            total = tokens_snapshot.len();
                            if total == 0 {
                                return Err("Token pool is empty".to_string());
                            }
                        }
                        OnDiskAccountState::Enabled => {
                            let normalized_target =
                                crate::proxy::common::model_mapping::normalize_to_standard_id(
                                    target_model,
                                )
                                .unwrap_or_else(|| target_model.to_string());

                            let is_rate_limited = self
                                .is_rate_limited(
                                    &preferred_token.account_id,
                                    Some(&normalized_target),
                                )
                                .await;
                            let is_quota_protected = quota_protection_enabled
                                && preferred_token
                                    .protected_models
                                    .contains(&normalized_target);

                            // [FIX #3639] 检查优先账号的目标模型配额：若已知为 0% 且池中有具备正配额的替代账号，自动逃逸切换
                            let has_zero_quota = preferred_token
                                .model_quotas
                                .get(&normalized_target)
                                .copied()
                                .unwrap_or(100)
                                <= 0;
                            let has_healthy_alternative = tokens_snapshot.iter().any(|t| {
                                t.account_id != preferred_token.account_id
                                    && !self
                                        .rate_limit_tracker
                                        .is_rate_limited(&t.account_id, Some(&normalized_target))
                                    && t.model_quotas.get(&normalized_target).copied().unwrap_or(0)
                                        > 0
                            });

                            if has_zero_quota && has_healthy_alternative {
                                tracing::warn!(
                                    "🔒 [Auto-Switch #3639] Preferred account {} has 0% quota for model {}, seamlessly falling back to available account in pool",
                                    preferred_token.email,
                                    normalized_target
                                );
                            } else if !is_rate_limited && !is_quota_protected {
                                tracing::info!(
                                    "🔒 [FIX #820] Using preferred account: {} (fixed mode)",
                                    preferred_token.email
                                );

                                // 直接使用优先账号，跳过轮询逻辑
                                let mut token = preferred_token.clone();

                                // 检查 token 是否过期（提前 300 秒平滑刷新，并应用 3.5s 超时安全管控）
                                const TOKEN_REFRESH_BUFFER_SECS: i64 = 300;
                                let now = chrono::Utc::now().timestamp();
                                if now >= token.timestamp - TOKEN_REFRESH_BUFFER_SECS {
                                    let hot_path_timeout = std::time::Duration::from_millis(3500);
                                    match self
                                        .refresh_account_token_with_timeout(
                                            &token.account_id,
                                            TOKEN_REFRESH_BUFFER_SECS,
                                            hot_path_timeout,
                                        )
                                        .await
                                    {
                                        Ok(refreshed) => {
                                            token = refreshed;
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                "Preferred account token refresh failed ({}): {}，继续尝试或回退",
                                                token.email,
                                                e
                                            );
                                        }
                                    }
                                }

                                // 确保有 project_id
                                let hot_path_timeout = std::time::Duration::from_millis(3500);
                                let project_id = self
                                    .resolve_project_id_with_timeout(
                                        &token.account_id,
                                        &token.access_token,
                                        &token.account_path,
                                        hot_path_timeout,
                                    )
                                    .await;

                                return Ok((
                                    token.access_token,
                                    project_id,
                                    token.email,
                                    token.account_id,
                                    0,
                                ));
                            } else {
                                if is_rate_limited {
                                    tracing::warn!("🔒 [FIX #820] Preferred account {} is rate-limited, falling back to round-robin", preferred_token.email);
                                } else {
                                    tracing::warn!("🔒 [FIX #820] Preferred account {} is quota-protected for {}, falling back to round-robin", preferred_token.email, target_model);
                                }
                            }
                        }
                    }
                } else {
                    tracing::warn!("🔒 [FIX #820] Preferred account {} not found in pool, falling back to round-robin", pref_id);
                }
            }
        }
        // ===== [END FIX #820] =====

        // 【优化 Issue #284】将锁操作移到循环外，避免重复获取锁
        // 预先获取 last_used_account 的快照，避免在循环中多次加锁
        let last_used_account_id = if quota_group != "image_gen" {
            let last_used = self.last_used_account.lock().await;
            last_used.clone()
        } else {
            None
        };

        let mut attempted: HashSet<String> = HashSet::new();
        let mut last_error: Option<String> = None;
        let mut need_update_last_used: Option<(String, std::time::Instant)> = None;

        for attempt in 0..total {
            let rotate = force_rotate || attempt > 0;

            // ===== 【核心】粘性会话与智能调度逻辑 =====
            let mut target_token: Option<ProxyToken> = None;

            // 归一化目标模型名为标准 ID，用于配额保护检查
            let normalized_target =
                crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                    .unwrap_or_else(|| target_model.to_string());

            // 模式 A: 粘性会话处理 (CacheFirst 或 Balance 且有 session_id)
            if !rotate
                && session_id.is_some()
                && scheduling.mode != SchedulingMode::PerformanceFirst
            {
                let sid = session_id.unwrap();

                // 1. 检查会话是否已绑定账号
                if let Some(bound_id) = self.session_accounts.get(sid).map(|v| v.clone()) {
                    // 【修复】先通过 account_id 找到对应的账号，获取其 email
                    // 2. 转换 email -> account_id 检查绑定的账号是否限流
                    if let Some(bound_token) =
                        tokens_snapshot.iter().find(|t| t.account_id == bound_id)
                    {
                        let key = self
                            .email_to_account_id(&bound_token.email)
                            .unwrap_or_else(|| bound_token.account_id.clone());
                        // [FIX] 传入目标模型标准化 ID，检查该模型是否已被熔断器精准锁定
                        let reset_sec = self
                            .rate_limit_tracker
                            .get_remaining_wait(&key, Some(&normalized_target));
                        let bound_zero_quota = bound_token
                            .model_quotas
                            .get(&normalized_target)
                            .copied()
                            .unwrap_or(100)
                            <= 0;
                        let has_healthy_alternative = tokens_snapshot.iter().any(|t| {
                            t.account_id != bound_token.account_id
                                && !attempted.contains(&t.account_id)
                                && !self
                                    .rate_limit_tracker
                                    .is_rate_limited(&t.account_id, Some(&normalized_target))
                                && t.model_quotas.get(&normalized_target).copied().unwrap_or(0) > 0
                        });

                        let is_qos_jailed = self
                            .qos_guardian
                            .is_jailed_or_probing(&bound_token.account_id);
                        if is_qos_jailed {
                            tracing::warn!(
                                "Sticky Session: Bound account {} is JAILED in QoS Guardian, unbinding and switching.",
                                bound_token.email
                            );
                            self.session_accounts.remove(sid);
                        } else if reset_sec > 0 {
                            // 【修复 Issue #284】立即解绑并切换账号，不再阻塞等待
                            // 原因：阻塞等待会导致并发请求时客户端 socket 超时 (UND_ERR_SOCKET)
                            tracing::debug!(
                                "Sticky Session: Bound account {} is rate-limited for {} ({}s), unbinding and switching.",
                                bound_token.email, normalized_target, reset_sec
                            );
                            self.session_accounts.remove(sid);
                        } else if bound_zero_quota && has_healthy_alternative {
                            // [FIX #3639] 粘性绑定的账号目标模型额度已归零且号池有可用账号，无缝解绑并自动迁移至健康账号
                            tracing::info!(
                                "Sticky Session: Bound account {} has 0% quota for model {}, seamlessly unbinding and migrating to healthy account in pool (Fixes #3639)",
                                bound_token.email,
                                normalized_target
                            );
                            self.session_accounts.remove(sid);
                        } else if !attempted.contains(&bound_id)
                            && !(quota_protection_enabled
                                && bound_token.protected_models.contains(&normalized_target))
                        {
                            // 3. 账号可用且未被标记为尝试失败，优先复用
                            tracing::info!("Sticky Session: Successfully reusing bound account {} for session {}", bound_token.email, sid);
                            target_token = Some(bound_token.clone());
                            need_update_last_used =
                                Some((bound_token.account_id.clone(), std::time::Instant::now()));
                        } else if quota_protection_enabled
                            && bound_token.protected_models.contains(&normalized_target)
                        {
                            tracing::debug!("Sticky Session: Bound account {} is quota-protected for model {} [{}], unbinding and switching.", bound_token.email, normalized_target, target_model);
                            self.session_accounts.remove(sid);
                        } else if attempted.contains(&bound_id) {
                            // [FIX] 绑定的账号在当前轮次请求中已尝试失败，立即解绑避免死锁
                            tracing::debug!("Sticky Session: Bound account {} already attempted in current request, unbinding", bound_token.email);
                            self.session_accounts.remove(sid);
                        }
                    } else {
                        // 绑定的账号已不存在（可能被删除），解绑
                        tracing::debug!(
                            "Sticky Session: Bound account not found for session {}, unbinding",
                            sid
                        );
                        self.session_accounts.remove(sid);
                    }
                }
            }

            // 模式 B: 原子化 60s 全局锁定 (针对无 session_id 情况的默认保护)
            // 【修复】性能优先模式应跳过 60s 锁定；
            if target_token.is_none()
                && !rotate
                && quota_group != "image_gen"
                && scheduling.mode != SchedulingMode::PerformanceFirst
            {
                // 仅针对无 session_id 的无状态请求，使用 60s 全局锁定保底避免轮换
                if session_id.is_none() {
                    if let Some((account_id, last_time)) = &last_used_account_id {
                        // [FIX #3] 60s 锁定逻辑应检查 `attempted` 集合，避免重复尝试失败的账号
                        if last_time.elapsed().as_secs() < 60 && !attempted.contains(account_id) {
                            if let Some(found) =
                                tokens_snapshot.iter().find(|t| &t.account_id == account_id)
                            {
                                // 【修复】检查限流状态和配额保护，避免复用已被锁定的账号
                                let last_zero_quota = found
                                    .model_quotas
                                    .get(&normalized_target)
                                    .copied()
                                    .unwrap_or(100)
                                    <= 0;
                                let has_healthy_alt = tokens_snapshot.iter().any(|t| {
                                    &t.account_id != account_id
                                        && !attempted.contains(&t.account_id)
                                        && !self.rate_limit_tracker.is_rate_limited(
                                            &t.account_id,
                                            Some(&normalized_target),
                                        )
                                        && t.model_quotas
                                            .get(&normalized_target)
                                            .copied()
                                            .unwrap_or(0)
                                            > 0
                                });

                                let is_qos_jailed =
                                    self.qos_guardian.is_jailed_or_probing(&found.account_id);
                                if is_qos_jailed {
                                    tracing::debug!(
                                        "60s Window: Last account {} is JAILED in QoS Guardian, skipping reuse",
                                        found.email
                                    );
                                } else if last_zero_quota && has_healthy_alt {
                                    tracing::debug!(
                                        "60s Window: Last account {} has 0% quota for model {}, skipping reuse (Fixes #3639)",
                                        found.email, normalized_target
                                    );
                                } else if !self
                                    .is_rate_limited(&found.account_id, Some(&normalized_target))
                                    .await
                                    && !(quota_protection_enabled
                                        && found.protected_models.contains(&normalized_target))
                                {
                                    tracing::debug!(
                                        "60s Window: Force reusing last account: {}",
                                        found.email
                                    );
                                    target_token = Some(found.clone());
                                    need_update_last_used =
                                        Some((found.account_id.clone(), std::time::Instant::now()));
                                } else {
                                    if self
                                        .is_rate_limited(
                                            &found.account_id,
                                            Some(&normalized_target),
                                        )
                                        .await
                                    {
                                        tracing::debug!(
                                            "60s Window: Last account {} is rate-limited, skipping",
                                            found.email
                                        );
                                    } else {
                                        tracing::debug!("60s Window: Last account {} is quota-protected for model {} [{}], skipping", found.email, normalized_target, target_model);
                                    }
                                }
                            }
                        }
                    }
                }

                // 若无锁定或带有 session_id（会话首次分配），使用 P2C 均衡选择账号
                if target_token.is_none() {
                    // 先过滤出未限流的账号
                    let mut non_limited: Vec<ProxyToken> = Vec::new();
                    for t in &tokens_snapshot {
                        if !self
                            .is_rate_limited(&t.account_id, Some(&normalized_target))
                            .await
                            && !self.qos_guardian.is_jailed_or_probing(&t.account_id)
                        {
                            non_limited.push(t.clone());
                        }
                    }

                    if let Some(selected) = self.select_with_p2c(
                        &non_limited,
                        &attempted,
                        &normalized_target,
                        quota_protection_enabled,
                    ) {
                        target_token = Some(selected.clone());
                        need_update_last_used =
                            Some((selected.account_id.clone(), std::time::Instant::now()));
                    }
                }
            } else if target_token.is_none() {
                // 模式 C: P2C 选择 (替代纯轮询)
                tracing::debug!("🔄 [Mode C] P2C selection from {} candidates", total);

                // 先过滤出未限流的账号
                let mut non_limited: Vec<ProxyToken> = Vec::new();
                for t in &tokens_snapshot {
                    if !self
                        .is_rate_limited(&t.account_id, Some(&normalized_target))
                        .await
                    {
                        non_limited.push(t.clone());
                    }
                }

                if let Some(selected) = self.select_with_p2c(
                    &non_limited,
                    &attempted,
                    &normalized_target,
                    quota_protection_enabled,
                ) {
                    tracing::debug!("  {} - SELECTED via P2C", selected.email);
                    target_token = Some(selected.clone());

                    if rotate {
                        tracing::debug!("Force Rotation: Switched to account: {}", selected.email);
                    }
                }
            }

            // 同一会话的并发第一次绑定归到已经写下的账号，后到的请求改用赢家。
            if let Some(ref selected) = target_token {
                if let Some(sid) = session_id {
                    if scheduling.mode != SchedulingMode::PerformanceFirst && !rotate {
                        let bound = {
                            self.session_accounts
                                .entry(sid.to_string())
                                .or_insert_with(|| selected.account_id.clone())
                                .clone()
                        };
                        if bound != selected.account_id {
                            if let Some(winner) =
                                tokens_snapshot.iter().find(|t| t.account_id == bound)
                            {
                                let winner_limited = self
                                    .is_rate_limited(&winner.account_id, Some(&normalized_target))
                                    .await;
                                if !winner_limited {
                                    tracing::info!(
                                        "Sticky Session: Adopted concurrent binding {} for session {}",
                                        winner.email,
                                        sid
                                    );
                                    target_token = Some(winner.clone());
                                }
                            }
                        } else {
                            tracing::info!(
                                "Sticky Session: Ensured binding account {} to session {}",
                                selected.email,
                                sid
                            );
                        }
                    }
                }
            }

            let mut token = match target_token {
                Some(t) => t,
                None => {
                    // 乐观重置策略: 双层防护机制
                    // 计算最短等待时间
                    let min_wait = tokens_snapshot
                        .iter()
                        .filter_map(|t| {
                            let wait = self
                                .rate_limit_tracker
                                .get_remaining_wait(&t.account_id, Some(&normalized_target));
                            if wait > 0 {
                                Some(wait)
                            } else {
                                None
                            }
                        })
                        .min();

                    // Layer 1: 如果最短等待时间 <= 5秒, 执行物理冷却延迟（留出冷却时间，杜绝频繁反复触发）
                    if let Some(wait_sec) = min_wait {
                        if wait_sec <= 5 {
                            let wait_ms = (wait_sec as f64 * 1000.0) as u64 + 100;
                            tracing::warn!(
                                "All accounts rate-limited but shortest wait is {}s. Applying {}ms physical cooldown delay...",
                                wait_sec, wait_ms
                            );

                            // 物理冷却延迟
                            tokio::time::sleep(tokio::time::Duration::from_millis(wait_ms)).await;

                            // 重新尝试选择账号
                            let mut retry_token = None;
                            for token in &tokens_snapshot {
                                if attempted.contains(&token.account_id)
                                    || self
                                        .is_rate_limited(
                                            &token.account_id,
                                            Some(&normalized_target),
                                        )
                                        .await
                                    || (quota_protection_enabled
                                        && token.protected_models.contains(&normalized_target))
                                {
                                    continue;
                                }
                                retry_token = Some(token);
                                break;
                            }

                            if let Some(t) = retry_token {
                                tracing::info!(
                                    "✅ Cooldown delay successful! Found available account: {}",
                                    t.email
                                );
                                t.clone()
                            } else {
                                tracing::warn!(
                                    "Cooldown delay elapsed ({}s), but accounts still rate-limited. Demanding client cooldown backoff.",
                                    wait_sec
                                );
                                return Err(format!(
                                    "All accounts rate-limited. Cooldown wait {}s.",
                                    wait_sec
                                ));
                            }
                        } else {
                            // 当等待时间 > 5s 时，不进行本地死等，坚决返回冷却等待错误，
                            // 让上层协议（Claude Code / HTTP 429 Retry-After）主动进行客户端退避，
                            // 严禁 0ms 乐观重置暴力清空限流标记导致多对话瞬态打穿 Google 上游（Fixes #3506）
                            tracing::warn!(
                                "All accounts rate-limited with wait {}s > 5s. Demanding client cooldown backoff.",
                                wait_sec
                            );
                            return Err(format!(
                                "All accounts rate-limited. Cooldown wait {}s.",
                                wait_sec
                            ));
                        }
                    } else {
                        return Err("All accounts failed or unhealthy.".to_string());
                    }
                }
            };

            // Safety net: avoid selecting an account that has been disabled on disk but still
            // exists in the in-memory snapshot (e.g. stale cache + sticky session binding).
            match Self::get_account_state_on_disk(&token.account_path).await {
                OnDiskAccountState::Disabled => {
                    tracing::warn!(
                        "Selected account {} is disabled on disk, purging and retrying",
                        token.email
                    );
                    attempted.insert(token.account_id.clone());
                    self.remove_account(&token.account_id);
                    continue;
                }
                OnDiskAccountState::Unknown => {
                    tracing::warn!(
                        "Selected account {} state on disk is unavailable, skipping",
                        token.email
                    );
                    attempted.insert(token.account_id.clone());
                    continue;
                }
                OnDiskAccountState::Enabled => {}
            }

            // 3. [ENHANCED] 检查 token 是否过期（提前 300 秒/5分钟平滑刷新，保障高可用与并发重试）
            let now = chrono::Utc::now().timestamp();
            const TOKEN_REFRESH_BUFFER_SECS: i64 = 300;
            if now >= token.timestamp - TOKEN_REFRESH_BUFFER_SECS {
                tracing::debug!(
                    "账号 {} 的 token 即将过期，正在执行主路径刷新 (限时 3.5s)...",
                    token.email
                );
                // [Hot-Path 快速容灾] 限制主请求路径上的单账号刷新耗时为 3.5 秒
                // 若遇上游 OAuth 网络卡顿（如常见 9s DNS/TCP 挂起），3.5s 立即超时跳过并轮换到下一个账号
                let hot_path_timeout = std::time::Duration::from_millis(3500);
                match self
                    .refresh_account_token_with_timeout(
                        &token.account_id,
                        TOKEN_REFRESH_BUFFER_SECS,
                        hot_path_timeout,
                    )
                    .await
                {
                    Ok(refreshed) => {
                        token = refreshed;
                        tracing::debug!("Token 刷新成功！");
                    }
                    Err(e) => {
                        tracing::error!(
                            "Token 刷新失败或超时 ({}): {}，尝试下一个账号",
                            token.email,
                            e
                        );
                        last_error = Some(format!("Token refresh failed: {}", e));
                        attempted.insert(token.account_id.clone());
                        if let Some(sid) = session_id {
                            self.abandon_session(sid, &token.account_id);
                        }
                        if quota_group != "image_gen"
                            && matches!(&last_used_account_id, Some((id, _)) if id == &token.account_id)
                        {
                            need_update_last_used =
                                Some((String::new(), std::time::Instant::now()));
                        }
                        continue;
                    }
                }
            }

            // 4. [ENHANCED] 确保有 project_id (使用账号专属锁 + 双重检查 + 3.5s 超时管控)
            let hot_path_timeout = std::time::Duration::from_millis(3500);
            let project_id = self
                .resolve_project_id_with_timeout(
                    &token.account_id,
                    &token.access_token,
                    &token.account_path,
                    hot_path_timeout,
                )
                .await;

            // 【优化】在成功返回前，统一更新 last_used_account（如果需要）
            if let Some((new_account_id, new_time)) = need_update_last_used {
                if quota_group != "image_gen" {
                    let mut last_used = self.last_used_account.lock().await;
                    if new_account_id.is_empty() {
                        // 空字符串表示需要清除锁定
                        *last_used = None;
                    } else {
                        *last_used = Some((new_account_id, new_time));
                    }
                }
            }

            return Ok((
                token.access_token,
                project_id,
                token.email,
                token.account_id,
                0,
            ));
        }

        Err(last_error.unwrap_or_else(|| "All accounts failed".to_string()))
    }

    async fn disable_account(&self, account_id: &str, reason: &str) -> Result<(), String> {
        let path = if let Some(entry) = self.tokens.get(account_id) {
            entry.account_path.clone()
        } else {
            self.resolved_data_dir()
                .join("accounts")
                .join(format!("{}.json", account_id))
        };

        let now = chrono::Utc::now().timestamp();
        let reason_owned = reason.to_string();
        update_account_json(&path, move |content| {
            content["disabled"] = serde_json::Value::Bool(true);
            content["disabled_at"] = serde_json::Value::Number(now.into());
            content["disabled_reason"] =
                serde_json::Value::String(truncate_reason(&reason_owned, 800));
        })
        .await?;

        // 【修复 Issue #3】从内存中移除禁用的账号，防止被60s锁定逻辑继续使用
        self.remove_account(account_id);

        tracing::warn!("Account disabled: {} ({:?})", account_id, path);
        Ok(())
    }

    /// 保存刷新后的 token 到账号文件
    async fn save_refreshed_token(
        &self,
        account_id: &str,
        token_response: &crate::modules::oauth::TokenResponse,
    ) -> Result<(), String> {
        let path = self
            .tokens
            .get(account_id)
            .ok_or("账号不存在")?
            .account_path
            .clone();
        let now = chrono::Utc::now().timestamp();
        let access_token = token_response.access_token.clone();
        let expires_in = token_response.expires_in;
        let id_token = token_response.id_token.clone();
        let refresh_token = token_response.refresh_token.clone();
        let expiry_timestamp = now + expires_in;
        update_account_json(&path, move |content| {
            content["token"]["access_token"] = serde_json::Value::String(access_token);
            content["token"]["expires_in"] = serde_json::Value::Number(expires_in.into());
            content["token"]["expiry_timestamp"] =
                serde_json::Value::Number(expiry_timestamp.into());

            // 如果获取到了新的 id_token，则保存它
            if let Some(it) = id_token {
                content["token"]["id_token"] = serde_json::Value::String(it);
            }

            // 如果获取到了新的 refresh_token（Token 轮转），也一并保存
            if let Some(rt) = refresh_token {
                content["token"]["refresh_token"] = serde_json::Value::String(rt);
            }
        })
        .await?;

        tracing::debug!("已保存刷新后的 token 到账号 {}", account_id);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// 通过 email 获取指定账号的 Token（用于预热等需要指定账号的场景）
    /// 此方法会自动刷新过期的 token
    pub async fn get_token_by_email(
        &self,
        email: &str,
    ) -> Result<(String, String, String, String, u64), String> {
        // 查找账号信息
        let token_info = {
            let mut found = None;
            for entry in self.tokens.iter() {
                let token = entry.value();
                if token.email == email {
                    found = Some((
                        token.account_id.clone(),
                        token.access_token.clone(),
                        token.refresh_token.clone(),
                        token.timestamp,
                        token.expires_in,
                        chrono::Utc::now().timestamp(),
                        token.project_id.clone(),
                    ));
                    break;
                }
            }
            found
        };

        let (
            account_id,
            current_access_token,
            refresh_token,
            timestamp,
            expires_in,
            now,
            project_id_opt,
        ) = match token_info {
            Some(info) => info,
            None => return Err(format!("未找到账号: {}", email)),
        };

        let project_id = project_id_opt
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "bamboo-precept-lgxtn".to_string());

        // 检查是否过期 (提前5分钟)
        if now < timestamp + expires_in - 300 {
            return Ok((
                current_access_token,
                project_id,
                email.to_string(),
                account_id,
                0,
            ));
        }

        tracing::info!("[Warmup] Token for {} is expiring, refreshing...", email);

        // 调用 OAuth 刷新 token
        match crate::modules::oauth::refresh_access_token(&refresh_token, Some(&account_id)).await {
            Ok(token_response) => {
                tracing::info!("[Warmup] Token refresh successful for {}", email);
                let new_now = chrono::Utc::now().timestamp();

                // 更新缓存
                if let Some(mut entry) = self.tokens.get_mut(&account_id) {
                    entry.access_token = token_response.access_token.clone();
                    entry.expires_in = token_response.expires_in;
                    entry.timestamp = new_now;
                }

                // 保存到磁盘
                let _ = self
                    .save_refreshed_token(&account_id, &token_response)
                    .await;

                Ok((
                    token_response.access_token,
                    project_id,
                    email.to_string(),
                    account_id,
                    0,
                ))
            }
            Err(e) => Err(format!(
                "[Warmup] Token refresh failed for {}: {}",
                email, e
            )),
        }
    }

    // ===== 限流管理方法 =====

    /// 标记账号限流(从外部调用,通常在 handler 中)
    /// 参数为 email，内部会自动转换为 account_id
    pub async fn mark_rate_limited(
        &self,
        email: &str,
        status: u16,
        retry_after_header: Option<&str>,
        error_body: &str,
    ) {
        // 网关自答（model not exist / no quota accounts）不得计入任何账号限流。
        if crate::proxy::common::model_gate::is_gateway_self_answer(error_body) {
            return;
        }

        // [NEW] 检查熔断是否启用 (使用内存缓存，极快)
        let config = self.circuit_breaker_config.read().await.clone();
        if !config.enabled {
            return;
        }

        // 【替代方案】转换 email -> account_id
        let key = self
            .email_to_account_id(email)
            .unwrap_or_else(|| email.to_string());

        self.rate_limit_tracker.parse_from_error(
            &key,
            status,
            retry_after_header,
            error_body,
            None,
            &config.backoff_steps, // [NEW] 传入配置
        );
    }

    /// 检查账号是否在限流中 (支持模型级)
    pub async fn is_rate_limited(&self, account_id: &str, model: Option<&str>) -> bool {
        // [NEW] 检查熔断是否启用
        let config = self.circuit_breaker_config.read().await;
        if !config.enabled {
            return self
                .rate_limit_tracker
                .get_quota_wait(account_id, model, true)
                > 0;
        }
        self.rate_limit_tracker.is_rate_limited(account_id, model)
    }

    /// 获取距离限流重置还有多少秒
    #[allow(dead_code)]
    pub fn get_rate_limit_reset_seconds(&self, account_id: &str) -> Option<u64> {
        self.rate_limit_tracker.get_reset_seconds(account_id)
    }

    /// 手动解除账号的小黑屋限制 (QoS 隔离与冷却) 并清除限速记录
    pub fn release_account_qos_jail(&self, account_id: &str) {
        self.qos_guardian.release_from_jail(account_id);
        self.rate_limit_tracker.clear(account_id);
        crate::modules::logger::log_info(&format!(
            "[QoS] Account {} manually released from QoS jail",
            account_id
        ));
    }

    /// 清除过期的限流记录
    #[allow(dead_code)]
    pub fn clean_expired_rate_limits(&self) {
        self.rate_limit_tracker.cleanup_expired();
    }

    /// 将指定账号的特定高阶模型标记为临时不支持（例如调用 Claude 5.5 返回 404/403）
    /// 默认冷却 15 分钟 (900s)，期间仅该模型跳过此账号，其他模型 100% 正常调用。
    pub fn mark_model_unsupported(
        &self,
        account_id: &str,
        model: &str,
        cooldown_secs: Option<i64>,
    ) {
        let base_key = model
            .trim()
            .to_lowercase()
            .replace("-extra-low", "")
            .replace("-low", "")
            .replace("-medium", "")
            .replace("-high", "")
            .replace("-xhigh", "")
            .replace("-thinking", "")
            .replace("-tiered", "")
            .replace("@default", "");
        let cooldown = cooldown_secs.unwrap_or(900);
        let until = chrono::Utc::now().timestamp() + cooldown;
        tracing::warn!(
            "账号 {} 对模型 {} ({}) 标记临时不可用/熔断 {} 秒 (至 {})",
            account_id,
            model,
            base_key,
            cooldown,
            until
        );
        self.unsupported_models
            .insert((account_id.to_string(), base_key), until);
    }

    /// 检查指定账号对特定模型是否处于临时熔断状态
    pub fn is_model_unsupported(&self, account_id: &str, model: &str) -> bool {
        let base_key = model
            .trim()
            .to_lowercase()
            .replace("-extra-low", "")
            .replace("-low", "")
            .replace("-medium", "")
            .replace("-high", "")
            .replace("-xhigh", "")
            .replace("-thinking", "")
            .replace("-tiered", "")
            .replace("@default", "");
        let now = chrono::Utc::now().timestamp();
        if let Some(entry) = self
            .unsupported_models
            .get(&(account_id.to_string(), base_key.clone()))
        {
            if now < *entry {
                return true;
            }
        }
        false
    }

    /// 【替代方案】通过 email 查找对应的 account_id
    /// 用于将 handlers 传入的 email 转换为 tracker 使用的 account_id
    fn email_to_account_id(&self, email: &str) -> Option<String> {
        self.tokens
            .iter()
            .find(|entry| entry.value().email == email)
            .map(|entry| entry.value().account_id.clone())
    }

    /// 清除指定账号的限流记录
    pub fn clear_rate_limit(&self, account_id: &str) -> bool {
        let cleared = self.rate_limit_tracker.clear(account_id);
        let persisted_cleared = self.clear_all_persisted_live_limits(account_id);
        cleared || persisted_cleared
    }

    pub fn clear_rate_limit_memory(&self, account_id: &str) -> bool {
        self.rate_limit_tracker.clear(account_id)
    }

    /// 清除所有限流记录
    pub fn clear_all_rate_limits(&self) {
        self.rate_limit_tracker.clear_all();
        let accounts_dir = self.resolved_data_dir().join("accounts");
        if let Ok(entries) = std::fs::read_dir(accounts_dir) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|value| value.to_str()) == Some("json") {
                    if let Some(account_id) =
                        entry.path().file_stem().and_then(|value| value.to_str())
                    {
                        self.clear_all_persisted_live_limits(account_id);
                    }
                }
            }
        }
    }

    fn clear_all_persisted_live_limits(&self, account_id: &str) -> bool {
        let path = self
            .data_dir
            .join("accounts")
            .join(format!("{}.json", account_id));
        let Ok(_account_write) = crate::modules::account::lock_account_file_updates() else {
            return false;
        };
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return false;
        };
        let Ok(mut content) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return false;
        };
        let Some(live_limits) = content
            .get_mut("live_limited_models")
            .and_then(|value| value.as_object_mut())
        else {
            return false;
        };
        if live_limits.is_empty() {
            return false;
        }
        live_limits.clear();
        let Ok(serialized) = serde_json::to_string_pretty(&content) else {
            return false;
        };
        std::fs::write(path, serialized).is_ok()
    }

    /// 标记账号请求成功，重置连续失败计数
    ///
    /// 在请求成功完成后调用，将该账号的失败计数归零，
    /// 下次失败时从最短的锁定时间开始（智能限流）。
    pub fn mark_account_success(&self, account_id: &str) {
        let resolved_id = self
            .email_to_account_id(account_id)
            .unwrap_or_else(|| account_id.to_string());
        self.rate_limit_tracker.mark_success(&resolved_id);
    }

    /// 标记账号发生短时网络故障或超时，施加短时冷却并将粘性会话即时解绑迁移
    pub fn mark_account_transient_failure(
        &self,
        account_id: &str,
        cooldown_secs: u64,
        reason: &str,
    ) {
        // 网关自答（model not exist / no quota accounts）不对应任何账号，
        // 绝不能触发冷却锁定与粘性会话解绑。
        if crate::proxy::common::model_gate::is_gateway_self_answer(reason) {
            tracing::debug!(
                "[ModelGate] Skipped transient-failure lock for gateway self-answer: {}",
                reason
            );
            return;
        }

        let resolved_id = self
            .email_to_account_id(account_id)
            .unwrap_or_else(|| account_id.to_string());
        let effective_secs = cooldown_secs.clamp(10, 300);
        let reset_time =
            std::time::SystemTime::now() + std::time::Duration::from_secs(effective_secs);
        self.rate_limit_tracker.set_lockout_until(
            &resolved_id,
            reset_time,
            crate::proxy::rate_limit::RateLimitReason::ServerError,
            None,
        );
        // 主动解除绑定到该故障账号的粘性会话，使后续请求即刻迁移至其他健康账号
        self.session_accounts.retain(|_, v| *v != resolved_id);
        tracing::warn!(
            account_id = %resolved_id,
            cooldown_secs = %effective_secs,
            reason = %reason,
            "Account marked with transient failure cooldown in rate limit tracker and unbound from sticky sessions"
        );
    }

    /// 返回所有账号上「真实配额 > 0」的模型名（原始 ID，含尺寸/档位变体，不做归一）。
    ///
    /// 供模型列表归一使用：只有真实有额度的变体才会把基底模型带入列表。
    pub fn models_with_positive_quota(&self) -> Vec<String> {
        let mut positive: HashSet<String> = HashSet::new();
        for entry in self.tokens.iter() {
            for (model, quota) in entry.value().model_quotas.iter() {
                if *quota > 0 {
                    positive.insert(model.clone());
                }
            }
        }
        positive.into_iter().collect()
    }

    /// 统计「对目标模型拥有正配额（或已授权可用）」的账号数量。
    ///
    /// 用于入站模型的配额闸门：返回 0 表示整池都没有该模型额度，
    /// 应当由网关直接 502 拒答，而不是把请求发往上游换回一个 429。
    pub fn accounts_with_positive_quota(&self, model: &str) -> usize {
        let normalized = crate::proxy::common::model_mapping::normalize_to_standard_id(model)
            .unwrap_or_else(|| model.to_string());
        // 生图基底：若账号只上报了尺寸变体（-4k / -2k / -preview …），同族变体配额即代表基底可用
        let image_base = crate::proxy::common::model_mapping::strip_image_variant_suffix(model);
        let is_image = image_base.contains("image") || image_base.starts_with("imagen");

        self.tokens
            .iter()
            .filter(|entry| {
                let token = entry.value();
                if token.validation_blocked {
                    return false;
                }
                if let Some(quota) = token
                    .model_quotas
                    .get(&normalized)
                    .or_else(|| token.model_quotas.get(model))
                {
                    return *quota > 0;
                }
                if is_image
                    && token.model_quotas.iter().any(|(known, quota)| {
                        *quota > 0
                            && crate::proxy::common::model_mapping::strip_image_variant_suffix(
                                known,
                            ) == image_base
                    })
                {
                    return true;
                }
                token.available_models.contains(model)
                    || token.available_models.contains(&normalized)
            })
            .count()
    }

    /// 检查是否有可用的 Google 账号
    ///
    /// 用于"仅兜底"模式的智能判断:当所有 Google 账号不可用时才使用外部提供商。
    ///
    /// # 参数
    /// - `quota_group`: 配额组("claude" 或 "gemini"),暂未使用但保留用于未来扩展
    /// - `target_model`: 目标模型名称(已归一化),用于配额保护检查
    ///
    /// # 返回值
    /// - `true`: 至少有一个可用账号(未限流且未被配额保护)
    /// - `false`: 所有账号都不可用(被限流或被配额保护)
    ///
    /// # 示例
    /// ```ignore
    /// // 检查是否有可用账号处理 claude-sonnet 请求
    /// let has_available = token_manager.has_available_account("claude", "claude-sonnet-4-20250514").await;
    /// if !has_available {
    ///     // 切换到外部提供商
    /// }
    /// ```
    pub async fn has_available_account(&self, _quota_group: &str, target_model: &str) -> bool {
        // 检查配额保护是否启用
        let quota_protection_enabled = crate::modules::config::load_app_config()
            .map(|cfg| cfg.quota_protection.enabled)
            .unwrap_or(false);

        let normalized_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());
        let target_lower = target_model.trim().to_lowercase();
        let base_name = crate::proxy::common::model_mapping::strip_tier_suffix(&target_lower);
        let any_loaded = self.tokens.iter().any(|t| !t.available_models.is_empty());

        // 遍历所有账号,检查是否有可用的
        for entry in self.tokens.iter() {
            let token = entry.value();

            // 0. 验证锁定中或禁用状态
            if token.validation_blocked {
                let now = chrono::Utc::now().timestamp();
                if token.validation_blocked_until == 0 || token.validation_blocked_until > now {
                    tracing::debug!(
                        "[Fallback Check] Account {} is validation blocked until {}, skipping",
                        token.email,
                        token.validation_blocked_until
                    );
                    continue;
                }
            }

            // 1. 检查是否被限流（兼顾全账号级与特定模型级 QuotaExhausted）
            if self
                .is_rate_limited(&token.account_id, Some(&normalized_target))
                .await
                || (target_model != normalized_target
                    && self
                        .is_rate_limited(&token.account_id, Some(target_model))
                        .await)
            {
                tracing::debug!(
                    "[Fallback Check] Account {} is rate-limited for model {}, skipping",
                    token.email,
                    target_model
                );
                continue;
            }

            // 2. 检查是否被配额保护(如果启用)
            if quota_protection_enabled
                && (token.protected_models.contains(target_model)
                    || token.protected_models.contains(&normalized_target))
            {
                tracing::debug!(
                    "[Fallback Check] Account {} is quota-protected for model {}, skipping",
                    token.email,
                    target_model
                );
                continue;
            }

            // 3. 检查模型能力支持（严格按 available_models 实际支持列表隔离，严禁跨代串池）
            if any_loaded {
                let has_cap = if token.available_models.is_empty() {
                    token.model_quotas.contains_key(&normalized_target)
                } else {
                    token.available_models.contains(&target_lower)
                        || (!base_name.is_empty() && token.available_models.contains(&base_name))
                        || (!base_name.is_empty()
                            && token.available_models.iter().any(|m| {
                                let m_base =
                                    crate::proxy::common::model_mapping::strip_tier_suffix(m);
                                m == &base_name
                                    || m_base == base_name
                                    || m.starts_with(&format!("{}-", base_name))
                                    || m.starts_with(&format!("{}@", base_name))
                            }))
                        || Self::token_supports_advanced_model(token, target_model)
                        || (normalized_target != "claude"
                            && token.model_quotas.contains_key(&normalized_target))
                };
                if !has_cap {
                    tracing::debug!(
                        "[Fallback Check] Account {} lacks capability for model {}, skipping",
                        token.email,
                        target_model
                    );
                    continue;
                }
            } else if !token.model_quotas.is_empty()
                && !token.model_quotas.contains_key(&normalized_target)
            {
                tracing::debug!(
                    "[Fallback Check] Account {} has no quota bucket for model {}, skipping",
                    token.email,
                    target_model
                );
                continue;
            }

            // 4. 检查是否已知配额归零
            if let Some(&quota) = token.model_quotas.get(&normalized_target) {
                if quota <= 0 {
                    tracing::debug!(
                        "[Fallback Check] Account {} has 0% quota for model {}, skipping",
                        token.email,
                        target_model
                    );
                    continue;
                }
            }

            // 找到至少一个可用账号
            tracing::debug!(
                "[Fallback Check] Found available account: {} for model {}",
                token.email,
                target_model
            );
            return true;
        }

        // 所有账号都不可用
        tracing::info!(
            "[Fallback Check] No available Google accounts for model {}, fallback should be triggered",
            target_model
        );
        false
    }

    /// 从账号文件获取配额刷新时间
    ///
    /// 若指定了 model，精准提取该模型（且 percentage <= 0）的专属 reset_time，杜绝误用其他耗尽模型的长周期刷新时间；
    /// 若未指定 model，仅在全部可用模型均为 0% 时才返回最早的 reset_time。
    ///
    /// # 参数
    /// - `account_id`: 账号 ID（用于查找账号文件）
    /// - `model`: 可选的目标模型名称，用于模型级别配额重置时间匹配
    pub fn get_quota_reset_time(&self, account_id: &str, model: Option<&str>) -> Option<String> {
        // 直接用 account_id 查找账号文件（文件名是 {account_id}.json）
        let account_path = self
            .data_dir
            .join("accounts")
            .join(format!("{}.json", account_id));

        let content = std::fs::read_to_string(&account_path).ok()?;
        let account: serde_json::Value = serde_json::from_str(&content).ok()?;
        let models = account
            .get("quota")
            .and_then(|q| q.get("models"))
            .and_then(|m| m.as_array())?;

        if let Some(target) = model {
            let norm_target = crate::proxy::common::model_mapping::normalize_to_standard_id(target)
                .unwrap_or_else(|| target.to_string());

            models
                .iter()
                .find(|m| {
                    let name = m.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let norm_name =
                        crate::proxy::common::model_mapping::normalize_to_standard_id(name)
                            .unwrap_or_else(|| name.to_string());
                    norm_name == norm_target || name == target
                })
                .and_then(|m| {
                    let pct = m.get("percentage").and_then(|v| v.as_i64()).unwrap_or(100);
                    // 关键保护：仅在该模型配额真正耗尽 (<= 0) 时才返回其 reset_time；
                    // 若配额尚存，说明 429 属于瞬态速率超限，严禁误读为长周期配额重置！
                    if pct <= 0 {
                        m.get("reset_time")
                            .and_then(|r| r.as_str())
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string())
                    } else {
                        None
                    }
                })
        } else {
            // 全账号级：仅当所有模型均为 0% 时才提取最早 reset_time
            let all_zero = !models.is_empty()
                && models
                    .iter()
                    .all(|m| m.get("percentage").and_then(|v| v.as_i64()).unwrap_or(100) <= 0);

            if all_zero {
                models
                    .iter()
                    .filter_map(|m| m.get("reset_time").and_then(|r| r.as_str()))
                    .filter(|s| !s.is_empty())
                    .min()
                    .map(|s| s.to_string())
            } else {
                None
            }
        }
    }

    /// 使用配额刷新时间精确锁定账号
    ///
    /// 当 API 返回 429 但没有 quotaResetDelay 时,尝试使用账号的配额刷新时间
    ///
    /// # 参数
    /// - `account_id`: 账号 ID
    /// - `reason`: 限流原因（QuotaExhausted/ServerError 等）
    /// - `model`: 可选的模型名称,用于模型级别限流
    pub fn set_precise_lockout(
        &self,
        account_id: &str,
        reason: crate::proxy::rate_limit::RateLimitReason,
        model: Option<String>,
    ) -> bool {
        // [FIX #2209] 统一归一化模型名称
        let normalized_model = model
            .as_deref()
            .and_then(|m| crate::proxy::common::model_mapping::normalize_to_standard_id(m));
        let model_to_lock = normalized_model.or(model);

        let cap = if let Ok(cfg) = self.circuit_breaker_config.try_read() {
            !cfg.lock_on_zero_quota
        } else {
            true
        };

        if let Some(reset_time_str) =
            self.get_quota_reset_time(account_id, model_to_lock.as_deref())
        {
            tracing::info!(
                "找到账号 {} [模型 {:?}] 的配额刷新时间: {} (cap_to_max: {})",
                account_id,
                model_to_lock,
                reset_time_str,
                cap
            );
            if reason == crate::proxy::rate_limit::RateLimitReason::QuotaExhausted {
                self.mark_model_quota_depleted(account_id, model_to_lock.as_deref());
            }
            self.rate_limit_tracker.set_lockout_until_iso_with_cap(
                account_id,
                &reset_time_str,
                reason,
                model_to_lock,
                cap,
            )
        } else {
            tracing::debug!(
                "未找到账号 {} [模型 {:?}] 的配额刷新时间，将使用默认退避策略",
                account_id,
                model_to_lock
            );
            false
        }
    }

    /// 实时刷新配额并精确锁定账号
    ///
    /// 当 429 发生时调用此方法:
    /// 1. 实时调用配额刷新 API 获取最新的 reset_time
    /// 2. 使用最新的 reset_time 精确锁定账号
    /// 3. 如果获取失败,返回 false 让调用方使用回退策略
    ///
    /// # 参数
    /// - `model`: 可选的模型名称,用于模型级别限流
    pub async fn fetch_and_lock_with_realtime_quota(
        &self,
        email: &str,
        reason: crate::proxy::rate_limit::RateLimitReason,
        model: Option<String>,
    ) -> bool {
        // 1. 从 tokens 中获取该账号的 access_token 和 account_id
        // 同时获取 account_id，确保锁定 key 与检查 key 一致
        let (access_token, account_id) = {
            let mut found: Option<(String, String)> = None;
            for entry in self.tokens.iter() {
                if entry.value().email == email {
                    found = Some((
                        entry.value().access_token.clone(),
                        entry.value().account_id.clone(),
                    ));
                    break;
                }
            }
            found
        }
        .unzip();

        let (access_token, account_id) = match (access_token, account_id) {
            (Some(token), Some(id)) => (token, id),
            _ => {
                tracing::warn!("无法找到账号 {} 的 access_token,无法实时刷新配额", email);
                return false;
            }
        };

        // 2. 调用配额刷新 API
        tracing::info!("账号 {} 正在实时刷新配额...", email);
        match crate::modules::quota::fetch_quota(&access_token, email, Some(&account_id)).await {
            Ok((quota_data, _project_id)) => {
                // [NEW] 实时刷新成功后，立即同步最新配额数据与 available_models 至内存缓存
                if let Some(mut token) = self.tokens.get_mut(&account_id) {
                    if !quota_data.models.is_empty() {
                        token.available_models.clear();
                    }
                    for m in &quota_data.models {
                        let norm_name =
                            crate::proxy::common::model_mapping::normalize_to_standard_id(&m.name)
                                .unwrap_or_else(|| m.name.clone());
                        token.model_quotas.insert(norm_name, m.percentage as i32);
                        token
                            .model_quotas
                            .insert(m.name.clone(), m.percentage as i32);
                        token.available_models.insert(m.name.to_lowercase());
                    }
                    // [NEW] 同步 Argon 配额组能力
                    let argon_bucket = quota_data.quota_groups.as_ref().and_then(|groups| {
                        groups.iter().find_map(|g| {
                            let name = g.display_name.to_lowercase();
                            if name.contains("argon") {
                                return g.buckets.first();
                            }
                            g.buckets.iter().find(|b| {
                                let bid = b.bucket_id.to_lowercase();
                                let bname = b.display_name.as_deref().unwrap_or("").to_lowercase();
                                bid.contains("argon") || bname.contains("argon")
                            })
                        })
                    });
                    let is_ultra = token
                        .subscription_tier
                        .as_deref()
                        .map(|s| s.to_uppercase() == "ULTRA")
                        .unwrap_or(false);

                    if let Some(bucket) = argon_bucket {
                        let pct = (bucket.remaining_fraction * 100.0) as i32;
                        token.model_quotas.insert("gemini-4-argon".to_string(), pct);
                        token
                            .model_quotas
                            .insert("gemini-4-argon-high".to_string(), pct);
                        token
                            .model_quotas
                            .insert("gemini-4-argon-medium".to_string(), pct);
                        token
                            .model_quotas
                            .insert("gemini-4-argon-low".to_string(), pct);
                        token.available_models.insert("gemini-4-argon".to_string());
                        token
                            .available_models
                            .insert("gemini-4-argon-high".to_string());
                        token
                            .available_models
                            .insert("gemini-4-argon-medium".to_string());
                        token
                            .available_models
                            .insert("gemini-4-argon-low".to_string());
                        token
                            .model_limits
                            .insert("gemini-4-argon".to_string(), 65536);
                        token
                            .model_limits
                            .insert("gemini-4-argon-high".to_string(), 65536);
                        token
                            .model_limits
                            .insert("gemini-4-argon-medium".to_string(), 65536);
                        token
                            .model_limits
                            .insert("gemini-4-argon-low".to_string(), 65536);
                    } else if is_ultra {
                        // Ultra 账号在官方架构下未拆分专属 Argon 分组，其 Argon 额度共享 Gemini 通用配额池
                        let gemini_bucket = quota_data.quota_groups.as_ref().and_then(|groups| {
                            groups.iter().find_map(|g| {
                                let name = g.display_name.to_lowercase();
                                if !name.contains("claude")
                                    && !name.contains("gpt")
                                    && !name.contains("3p")
                                {
                                    g.buckets.first()
                                } else {
                                    None
                                }
                            })
                        });
                        let pct = gemini_bucket
                            .map(|b| (b.remaining_fraction * 100.0) as i32)
                            .unwrap_or(100);
                        token.model_quotas.insert("gemini-4-argon".to_string(), pct);
                        token
                            .model_quotas
                            .insert("gemini-4-argon-high".to_string(), pct);
                        token
                            .model_quotas
                            .insert("gemini-4-argon-medium".to_string(), pct);
                        token
                            .model_quotas
                            .insert("gemini-4-argon-low".to_string(), pct);
                        token.available_models.insert("gemini-4-argon".to_string());
                        token
                            .available_models
                            .insert("gemini-4-argon-high".to_string());
                        token
                            .available_models
                            .insert("gemini-4-argon-medium".to_string());
                        token
                            .available_models
                            .insert("gemini-4-argon-low".to_string());
                        token
                            .model_limits
                            .insert("gemini-4-argon".to_string(), 65536);
                        token
                            .model_limits
                            .insert("gemini-4-argon-high".to_string(), 65536);
                        token
                            .model_limits
                            .insert("gemini-4-argon-medium".to_string(), 65536);
                        token
                            .model_limits
                            .insert("gemini-4-argon-low".to_string(), 65536);
                    } else {
                        // 若刷新后无 Argon 专属分组且非 Ultra，清理该账号在内存中的 Argon 缓存
                        token.model_quotas.remove("gemini-4-argon");
                        token.model_quotas.remove("gemini-4-argon-high");
                        token.model_quotas.remove("gemini-4-argon-medium");
                        token.model_quotas.remove("gemini-4-argon-low");
                        token.available_models.remove("gemini-4-argon");
                        token.available_models.remove("gemini-4-argon-high");
                        token.available_models.remove("gemini-4-argon-medium");
                        token.available_models.remove("gemini-4-argon-low");
                        token.model_limits.remove("gemini-4-argon");
                        token.model_limits.remove("gemini-4-argon-high");
                        token.model_limits.remove("gemini-4-argon-medium");
                        token.model_limits.remove("gemini-4-argon-low");
                    }
                }

                // 3. 从最新配额中提取匹配模型的 reset_time
                let normalized_model = model
                    .as_deref()
                    .and_then(|m| crate::proxy::common::model_mapping::normalize_to_standard_id(m));
                let model_to_lock = normalized_model.or(model);

                let matched_reset = if let Some(ref target) = model_to_lock {
                    let norm_target =
                        crate::proxy::common::model_mapping::normalize_to_standard_id(target)
                            .unwrap_or_else(|| target.to_string());

                    // 精准查找当前报错的模型
                    quota_data
                        .models
                        .iter()
                        .find(|m| {
                            let norm_m =
                                crate::proxy::common::model_mapping::normalize_to_standard_id(
                                    &m.name,
                                )
                                .unwrap_or_else(|| m.name.clone());
                            norm_m == norm_target || m.name == *target
                        })
                        .and_then(|m| {
                            // 仅当该模型配额确实耗尽 (percentage <= 0) 时才采用其长周期 reset_time；
                            // 若配额尚存，说明 429 属于瞬态限流，严禁锁入长周期刷新时间！
                            if m.percentage <= 0 && !m.reset_time.is_empty() {
                                Some(m.reset_time.as_str())
                            } else {
                                None
                            }
                        })
                } else {
                    // 全账号级锁定：仅在所有模型配额均耗尽时才提取最早 reset_time
                    let all_zero = !quota_data.models.is_empty()
                        && quota_data.models.iter().all(|m| m.percentage <= 0);
                    if all_zero {
                        quota_data
                            .models
                            .iter()
                            .filter_map(|m| {
                                if !m.reset_time.is_empty() {
                                    Some(m.reset_time.as_str())
                                } else {
                                    None
                                }
                            })
                            .min()
                    } else {
                        None
                    }
                };

                if let Some(reset_time_str) = matched_reset {
                    tracing::info!(
                        "账号 {} 实时配额刷新成功,模型 {:?} 匹配到 reset_time: {}",
                        email,
                        model_to_lock,
                        reset_time_str
                    );

                    let cap = if let Ok(cfg) = self.circuit_breaker_config.try_read() {
                        !cfg.lock_on_zero_quota
                    } else {
                        true
                    };

                    // [FIX] 使用 account_id 作为 key，与 is_rate_limited 检查一致
                    self.rate_limit_tracker.set_lockout_until_iso_with_cap(
                        &account_id,
                        reset_time_str,
                        reason,
                        model_to_lock,
                        cap,
                    )
                } else {
                    tracing::warn!(
                        "账号 {} 配额刷新成功，但模型 {:?} 配额未耗尽或未找到专属 reset_time，将使用默认退避策略",
                        email,
                        model_to_lock
                    );
                    false
                }
            }
            Err(e) => {
                tracing::warn!("账号 {} 实时配额刷新失败: {:?}", email, e);
                false
            }
        }
    }

    fn has_explicit_retry_time(
        parser_mode: TrackerParserMode,
        retry_after_header: Option<&str>,
        error_body: &str,
    ) -> bool {
        match parser_mode {
            TrackerParserMode::Current => {
                crate::proxy::upstream::retry::parse_retry_delay(error_body, retry_after_header)
                    .is_some()
            }
            TrackerParserMode::Baseline => {
                retry_after_header.is_some() || error_body.contains("quotaResetDelay")
            }
        }
    }

    fn parse_rate_limit_with_mode(
        &self,
        account_id: &str,
        status: u16,
        retry_after_header: Option<&str>,
        error_body: &str,
        model: Option<&str>,
        backoff_steps: &[u64],
        parser_mode: TrackerParserMode,
    ) -> Option<crate::proxy::rate_limit::RateLimitInfo> {
        match parser_mode {
            TrackerParserMode::Current => self.rate_limit_tracker.parse_from_error(
                account_id,
                status,
                retry_after_header,
                error_body,
                model.map(str::to_string),
                backoff_steps,
            ),
            TrackerParserMode::Baseline => self.rate_limit_tracker.parse_from_error_baseline(
                account_id,
                status,
                retry_after_header,
                error_body,
                model.map(str::to_string),
                backoff_steps,
            ),
        }
    }

    fn record_rate_limit_atomic(
        &self,
        account_id: &str,
        status: u16,
        retry_after_header: Option<&str>,
        error_body: &str,
        model: Option<&str>,
        backoff_steps: &[u64],
        parser_mode: TrackerParserMode,
    ) -> Option<crate::proxy::rate_limit::RateLimitInfo> {
        if status != 429 && status != 529 {
            return None;
        }
        if model
            .and_then(crate::proxy::rate_limit::normalize_image_model_id)
            .is_some()
        {
            match crate::modules::account::lock_account_file_updates() {
                Ok(_account_write) => {
                    let info = self.parse_rate_limit_with_mode(
                        account_id,
                        status,
                        retry_after_header,
                        error_body,
                        model,
                        backoff_steps,
                        parser_mode,
                    );
                    if parser_mode == TrackerParserMode::Current {
                        if let Some(ref info) = info {
                            self.persist_live_limit_locked(
                                account_id,
                                model,
                                status,
                                retry_after_header,
                                error_body,
                                info,
                            );
                        }
                    }
                    return info;
                }
                Err(error) => {
                    tracing::debug!(
                        "Failed to serialize live limit update for {}: {}",
                        account_id,
                        error
                    );
                }
            }
        }

        self.parse_rate_limit_with_mode(
            account_id,
            status,
            retry_after_header,
            error_body,
            model,
            backoff_steps,
            parser_mode,
        )
    }

    /// Register the in-memory image exclusion before releasing its account permit.
    /// Returns whether a slower quota refresh is still useful after the permit is released.
    pub async fn mark_rate_limited_fast(
        &self,
        email: &str,
        status: u16,
        retry_after_header: Option<&str>,
        error_body: &str,
        model: Option<&str>,
    ) -> bool {
        // 网关自答（model not exist / no quota accounts）不属于任何账号，
        // 绝不参与限流记账与零配额标记，否则会连坐惩罚无辜账号。
        if crate::proxy::common::model_gate::is_gateway_self_answer(error_body) {
            return false;
        }

        let normalized_model =
            model.and_then(crate::proxy::common::model_mapping::normalize_to_standard_id);
        let model_to_track = normalized_model.as_deref().or(model);
        let config = self.circuit_breaker_config.read().await.clone();
        if !config.enabled {
            return false;
        }

        let account_id = self
            .email_to_account_id(email)
            .unwrap_or_else(|| email.to_string());
        let has_explicit_retry_time = Self::has_explicit_retry_time(
            TrackerParserMode::Current,
            retry_after_header,
            error_body,
        );
        let reason = classify_rate_limit_reason(error_body);
        let recorded = self.record_rate_limit_atomic(
            &account_id,
            status,
            retry_after_header,
            error_body,
            model_to_track,
            &config.backoff_steps,
            TrackerParserMode::Current,
        );

        if reason == crate::proxy::rate_limit::RateLimitReason::QuotaExhausted {
            self.mark_model_quota_depleted(&account_id, model_to_track);
        }

        status == 429
            && recorded.is_some()
            && !has_explicit_retry_time
            && reason == crate::proxy::rate_limit::RateLimitReason::QuotaExhausted
    }

    /// [NEW] 将指定账号的目标模型配额在内存中即时标记为 0%
    /// 当上游返回明确的配额耗尽 (QuotaExhausted / 429) 时，立即响应式更新内存缓存，
    /// 确保后续轮次的粘性会话与固定账号无感自动切换 (Fixes #3639) 能够立刻生效。
    pub fn mark_model_quota_depleted(&self, account_or_email: &str, model: Option<&str>) {
        let account_id = self
            .email_to_account_id(account_or_email)
            .unwrap_or_else(|| account_or_email.to_string());
        if let Some(mut token) = self.tokens.get_mut(&account_id) {
            if let Some(m) = model {
                let norm = crate::proxy::common::model_mapping::normalize_to_standard_id(m)
                    .unwrap_or_else(|| m.to_string());
                token.model_quotas.insert(norm, 0);
                token.model_quotas.insert(m.to_string(), 0);
            } else {
                for v in token.model_quotas.values_mut() {
                    *v = 0;
                }
            }
            tracing::debug!(
                "[Auto-Switch #3639] Reactively marked model quota depleted (0%) for account {} [model: {:?}]",
                account_id,
                model
            );
        }
    }

    pub async fn refresh_quota_lock_after_fast_mark(&self, email: &str, model: Option<&str>) {
        let normalized_model =
            model.and_then(crate::proxy::common::model_mapping::normalize_to_standard_id);
        let model_to_track = normalized_model.as_deref().or(model);
        let account_id = self
            .email_to_account_id(email)
            .unwrap_or_else(|| email.to_string());
        let reason = crate::proxy::rate_limit::RateLimitReason::QuotaExhausted;

        if self
            .fetch_and_lock_with_realtime_quota(email, reason, model_to_track.map(str::to_string))
            .await
        {
            tracing::info!("账号 {} 已使用实时配额精确锁定", email);
            return;
        }
        if self.set_precise_lockout(&account_id, reason, model_to_track.map(str::to_string)) {
            tracing::info!("账号 {} 已使用本地缓存配额锁定", account_id);
        }
    }

    pub async fn mark_rate_limited_async(
        &self,
        email: &str,
        status: u16,
        retry_after_header: Option<&str>,
        error_body: &str,
        model: Option<&str>,
    ) {
        self.mark_rate_limited_async_with_mode(
            email,
            status,
            retry_after_header,
            error_body,
            model,
            TrackerParserMode::Current,
        )
        .await;
    }

    pub async fn mark_rate_limited_async_baseline(
        &self,
        email: &str,
        status: u16,
        retry_after_header: Option<&str>,
        error_body: &str,
        model: Option<&str>,
    ) {
        self.mark_rate_limited_async_with_mode(
            email,
            status,
            retry_after_header,
            error_body,
            model,
            TrackerParserMode::Baseline,
        )
        .await;
    }

    async fn mark_rate_limited_async_with_mode(
        &self,
        email: &str,
        status: u16,
        retry_after_header: Option<&str>,
        error_body: &str,
        model: Option<&str>,
        parser_mode: TrackerParserMode,
    ) {
        // 关键门禁 1：仅对真正的上游 429 (配额耗尽/速率限制) 和 529 (Overloaded) 记录限流；500/503/404 等绝对不打入冷却池！
        if status != 429 && status != 529 {
            return;
        }

        // 关键门禁 2：内部错误文字（All accounts limited / No accounts available / Token pool is empty 等）严禁递归自锁！
        // 网关自答哨兵（model not exist / no quota accounts）同样必须豁免，否则会连坐无辜账号。
        let lower_err = error_body.to_lowercase();
        if crate::proxy::common::model_gate::is_gateway_self_answer(error_body)
            || lower_err.contains("all accounts limited")
            || lower_err.contains("no accounts available")
            || lower_err.contains("all accounts failed")
            || lower_err.contains("token pool is empty")
            || lower_err.contains("all accounts exhausted")
            || lower_err.contains("all accounts unhealthy")
        {
            return;
        }

        let normalized_model =
            model.and_then(crate::proxy::common::model_mapping::normalize_to_standard_id);
        let model_to_track = normalized_model.as_deref().or(model);
        let config = self.circuit_breaker_config.read().await.clone();
        if !config.enabled {
            return;
        }

        let account_id = self
            .email_to_account_id(email)
            .unwrap_or_else(|| email.to_string());
        let reason = classify_rate_limit_reason(error_body);
        if reason == crate::proxy::rate_limit::RateLimitReason::QuotaExhausted {
            self.mark_model_quota_depleted(&account_id, model_to_track);
        }

        if Self::has_explicit_retry_time(parser_mode, retry_after_header, error_body) {
            self.record_rate_limit_atomic(
                &account_id,
                status,
                retry_after_header,
                error_body,
                model_to_track,
                &config.backoff_steps,
                parser_mode,
            );
            return;
        }
        if reason != crate::proxy::rate_limit::RateLimitReason::QuotaExhausted {
            self.record_rate_limit_atomic(
                &account_id,
                status,
                retry_after_header,
                error_body,
                model_to_track,
                &config.backoff_steps,
                parser_mode,
            );
            return;
        }

        if self
            .fetch_and_lock_with_realtime_quota(email, reason, model_to_track.map(str::to_string))
            .await
        {
            tracing::info!("账号 {} 已使用实时配额精确锁定", email);
            return;
        }
        if self.set_precise_lockout(&account_id, reason, model_to_track.map(str::to_string)) {
            tracing::info!("账号 {} 已使用本地缓存配额锁定", account_id);
            return;
        }

        tracing::warn!("账号 {} 无法获取配额刷新时间,使用指数退避策略", account_id);
        self.record_rate_limit_atomic(
            &account_id,
            status,
            retry_after_header,
            error_body,
            model_to_track,
            &config.backoff_steps,
            parser_mode,
        );
    }

    fn persist_live_limit_locked(
        &self,
        account_id: &str,
        model: Option<&str>,
        status: u16,
        retry_after_header: Option<&str>,
        error_body: &str,
        info: &crate::proxy::rate_limit::RateLimitInfo,
    ) {
        let Some(model_key) = model.and_then(crate::proxy::rate_limit::normalize_image_model_id)
        else {
            return;
        };
        let Some(explicit_delay_ms) =
            crate::proxy::upstream::retry::parse_retry_delay(error_body, retry_after_header)
        else {
            return;
        };
        if status != 429
            || info.reason != crate::proxy::rate_limit::RateLimitReason::QuotaExhausted
            || info.retry_after_sec <= 300
            || !crate::proxy::rate_limit::has_explicit_quota_exhausted(error_body)
        {
            return;
        }
        let (Some(until), Some(detected_at)) = (
            unix_timestamp_ceil(info.reset_time),
            unix_timestamp_ceil(info.detected_at),
        ) else {
            return;
        };

        let path = if let Some(entry) = self.tokens.get(account_id) {
            entry.account_path.clone()
        } else {
            self.resolved_data_dir()
                .join("accounts")
                .join(format!("{}.json", account_id))
        };

        let Ok(raw) = std::fs::read_to_string(&path) else {
            return;
        };
        let Ok(mut content) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return;
        };

        if !content
            .get("live_limited_models")
            .and_then(|v| v.as_object())
            .is_some()
        {
            content["live_limited_models"] = serde_json::Value::Object(serde_json::Map::new());
        }

        content["live_limited_models"][&model_key] = serde_json::json!({
            "model": model_key,
            "status": status,
            "reason": format!("{:?}", info.reason),
            "until": until,
            "detected_at": detected_at,
            "message": format!(
                "QUOTA_EXHAUSTED; retry after {}ms; {}",
                explicit_delay_ms,
                truncate_reason(error_body, 400)
            ),
        });

        let Ok(serialized) = serde_json::to_string_pretty(&content) else {
            return;
        };
        if let Err(e) = std::fs::write(&path, serialized) {
            tracing::debug!("Failed to persist live limit for {}: {}", account_id, e);
        }
    }

    pub fn clear_persisted_live_limit(&self, account_id: &str, model: Option<&str>) {
        let Some(raw_model) = model.filter(|m| !m.is_empty()) else {
            return;
        };
        let model_key = crate::proxy::common::model_mapping::normalize_to_standard_id(raw_model)
            .unwrap_or_else(|| raw_model.to_string());

        let path = if let Some(entry) = self.tokens.get(account_id) {
            entry.account_path.clone()
        } else {
            self.resolved_data_dir()
                .join("accounts")
                .join(format!("{}.json", account_id))
        };

        let Ok(_account_write) = crate::modules::account::lock_account_file_updates() else {
            return;
        };

        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.rate_limit_tracker.clear_model(account_id, &model_key);
                return;
            }
            Err(error) => {
                tracing::debug!("Failed to read live limit for {}: {}", account_id, error);
                return;
            }
        };
        let Ok(mut content) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return;
        };

        let Some(live_limits) = content
            .get_mut("live_limited_models")
            .and_then(|value| value.as_object_mut())
        else {
            self.rate_limit_tracker.clear_model(account_id, &model_key);
            return;
        };

        let mut changed = live_limits.remove(&model_key).is_some();
        if model_key != raw_model {
            changed |= live_limits.remove(raw_model).is_some();
        }

        if changed {
            let Ok(serialized) = serde_json::to_string_pretty(&content) else {
                return;
            };
            if let Err(error) = std::fs::write(&path, serialized) {
                tracing::debug!("Failed to clear live limit for {}: {}", account_id, error);
                return;
            }
        }
        self.rate_limit_tracker.clear_model(account_id, &model_key);
    }

    // ===== 调度配置相关方法 =====

    /// 获取当前调度配置
    pub async fn get_sticky_config(&self) -> StickySessionConfig {
        self.sticky_config.read().await.clone()
    }

    /// 更新调度配置
    pub async fn update_sticky_config(&self, new_config: StickySessionConfig) {
        let mut config = self.sticky_config.write().await;
        *config = new_config;
        tracing::debug!("Scheduling configuration updated: {:?}", *config);
    }

    /// [NEW] 更新熔断器配置
    pub async fn update_circuit_breaker_config(&self, config: crate::models::CircuitBreakerConfig) {
        let mut lock = self.circuit_breaker_config.write().await;
        *lock = config;
        tracing::debug!("Circuit breaker configuration updated");
    }

    /// [NEW] 获取熔断器配置
    pub async fn get_circuit_breaker_config(&self) -> crate::models::CircuitBreakerConfig {
        self.circuit_breaker_config.read().await.clone()
    }

    /// 清除特定会话的粘性映射
    #[allow(dead_code)]
    pub fn clear_session_binding(&self, session_id: &str) {
        self.session_accounts.remove(session_id);
    }

    /// 比较并删除：只有映射里仍然是这个账号才清掉当前会话。
    pub fn abandon_session(&self, session_id: &str, account_id: &str) -> bool {
        let still_bound = self
            .session_accounts
            .get(session_id)
            .map(|bound| bound.as_str() == account_id)
            .unwrap_or(false);
        if still_bound {
            self.session_accounts.remove(session_id);
            true
        } else {
            false
        }
    }

    /// 上游成功后写下真正用过的账号。轮换成功的新账号也从这里进入粘性表。
    pub fn commit_session(&self, session_id: &str, account_id: &str) {
        if session_id.is_empty() || account_id.is_empty() {
            return;
        }
        self.session_accounts
            .insert(session_id.to_string(), account_id.to_string());
    }

    /// 解绑当前会话。不清全局 last_used，避免一个租户的失败打散别人的 60 秒窗口。
    pub async fn unbind_session_and_clear_last_used(&self, session_id: Option<&str>) {
        if let Some(sid) = session_id {
            self.session_accounts.remove(sid);
        }
    }

    /// 获取当前 Token 池内有效账号数量
    pub fn tokens_count(&self) -> usize {
        self.tokens.len()
    }

    /// 获取当前生效的负载均衡调度模式（从内存中安全读取，无需触碰磁盘）
    pub async fn get_scheduling_mode(&self) -> crate::proxy::sticky_config::SchedulingMode {
        self.sticky_config.read().await.mode
    }

    /// 清除所有会话的粘性映射
    pub fn clear_all_sessions(&self) {
        self.session_accounts.clear();
    }

    // ===== [FIX #820] 固定账号模式相关方法 =====

    /// 设置优先使用的账号ID（固定账号模式）
    /// 传入 Some(account_id) 启用固定账号模式，传入 None 恢复轮询模式
    pub async fn set_preferred_account(&self, account_id: Option<String>) {
        let mut preferred = self.preferred_account_id.write().await;
        if let Some(ref id) = account_id {
            tracing::info!("🔒 [FIX #820] Fixed account mode enabled: {}", id);
        } else {
            tracing::info!("🔄 [FIX #820] Round-robin mode enabled (no preferred account)");
        }
        *preferred = account_id;
    }

    /// 获取当前优先使用的账号ID
    pub async fn get_preferred_account(&self) -> Option<String> {
        self.preferred_account_id.read().await.clone()
    }

    /// 使用 Authorization Code 交换 Refresh Token (Web OAuth)
    pub async fn exchange_code(&self, code: &str, redirect_uri: &str) -> Result<String, String> {
        crate::modules::oauth::exchange_code(code, redirect_uri)
            .await
            .and_then(|t| {
                t.refresh_token
                    .ok_or_else(|| "No refresh token returned by Google".to_string())
            })
    }

    /// 获取 OAuth URL (支持自定义 Redirect URI)
    pub fn get_oauth_url_with_redirect(&self, redirect_uri: &str, state: &str) -> String {
        crate::modules::oauth::get_auth_url(redirect_uri, state)
    }

    /// 获取用户信息 (Email 等)
    pub async fn get_user_info(
        &self,
        refresh_token: &str,
    ) -> Result<crate::modules::oauth::UserInfo, String> {
        // 先获取 Access Token
        let token = crate::modules::oauth::refresh_access_token(refresh_token, None)
            .await
            .map_err(|e| format!("刷新 Access Token 失败: {}", e))?;

        crate::modules::oauth::get_user_info(&token.access_token, None).await
    }

    /// 添加新账号 (纯后端实现，不依赖 Tauri AppHandle)
    pub async fn add_account(&self, email: &str, refresh_token: &str) -> Result<(), String> {
        // 1. 获取 Access Token (验证 refresh_token 有效性)
        let token_info = crate::modules::oauth::refresh_access_token(refresh_token, None)
            .await
            .map_err(|e| format!("Invalid refresh token: {}", e))?;

        // 2. 获取项目 ID (Project ID)
        let project_id = crate::proxy::project_resolver::fetch_project_id(&token_info.access_token)
            .await
            .unwrap_or_else(|_| "bamboo-precept-lgxtn".to_string()); // Fallback

        // 3. 委托给 modules::account::add_account 处理 (包含文件写入、索引更新、锁)
        let email_clone = email.to_string();
        let refresh_token_clone = refresh_token.to_string();

        tokio::task::spawn_blocking(move || {
            let token_data = crate::models::TokenData::new(
                token_info.access_token,
                refresh_token_clone,
                token_info.expires_in,
                Some(email_clone.clone()),
                Some(project_id),
                None,  // session_id
                false, // 默认不开启
                token_info.id_token,
            )
            .with_oauth_client_key(token_info.oauth_client_key.clone());

            crate::modules::account::upsert_account(email_clone, None, token_data)
        })
        .await
        .map_err(|e| format!("Task join error: {}", e))?
        .map_err(|e| format!("Failed to save account: {}", e))?;

        // 4. 重新加载 (更新内存)
        self.reload_all_accounts().await.map(|_| ())
    }

    /// 记录请求成功，增加健康分
    pub fn record_success(&self, account_id: &str) {
        self.health_scores
            .entry(account_id.to_string())
            .and_modify(|s| *s = (*s + 0.05).min(1.0))
            .or_insert(1.0);
        tracing::debug!("📈 Health score increased for account {}", account_id);
    }

    /// 记录请求失败，降低健康分
    pub fn record_failure(&self, account_id: &str) {
        self.health_scores
            .entry(account_id.to_string())
            .and_modify(|s| *s = (*s - 0.2).max(0.0))
            .or_insert(0.8);
        tracing::warn!("📉 Health score decreased for account {}", account_id);
    }

    /// 获取当前 QoS 守卫实例
    pub fn get_qos_guardian(&self) -> Arc<crate::proxy::qos_guardian::QoSGuardian> {
        self.qos_guardian.clone()
    }

    /// 获取指定账号当前的订阅等级
    pub fn get_account_tier(&self, account_id: &str) -> Option<String> {
        self.tokens
            .get(account_id)
            .and_then(|t| t.subscription_tier.clone())
    }

    /// 强制解绑指定会话与账号的绑定关系
    pub fn unbind_session(&self, session_id: &str) {
        if self.session_accounts.remove(session_id).is_some() {
            tracing::info!(
                "🔗 [QoS Session Handover] Successfully unbound session {}",
                session_id
            );
        }
    }

    /// 评估单次请求的 TTFT 并驱动 QoS 状态机流转
    pub fn record_qos_ttft(
        &self,
        account_id: &str,
        session_id: Option<&str>,
        tier: &str,
        ttft_ms: f64,
        is_deep_thinking: bool,
        input_tokens: Option<u32>,
    ) {
        let all_tokens: Vec<ProxyToken> = self
            .tokens
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        let rate_limiter = self.rate_limit_tracker.clone();

        let action = self.qos_guardian.record_and_evaluate(
            account_id,
            session_id,
            tier,
            ttft_ms,
            is_deep_thinking,
            input_tokens,
            &all_tokens,
            |acc_id| rate_limiter.is_rate_limited(acc_id, None),
        );

        match action {
            crate::proxy::qos_guardian::QoSAction::TriggerJailedAndUnbind {
                level,
                wait_duration,
            } => {
                if let Some(sid) = session_id {
                    self.unbind_session(sid);
                }
                tracing::warn!(
                    "🚨 [QoS Guardian] Account {} jailed to Level {} (wait {:?}), active session unbound",
                    account_id,
                    level,
                    wait_duration
                );
            }
            crate::proxy::qos_guardian::QoSAction::HalfOpenFailed {
                escalated_level,
                wait_duration,
            } => {
                if let Some(sid) = session_id {
                    self.unbind_session(sid);
                }
                tracing::warn!(
                    "💥 [QoS Guardian] Account {} Half-Open probe probation failed. Escalated to Level {} ({:?})",
                    account_id,
                    escalated_level,
                    wait_duration
                );
            }
            _ => {}
        }
    }

    /// 启动 QoS 自动阶梯探针巡检协程（每 5 秒扫描一次）
    pub async fn start_qos_probing_daemon(self: &Arc<Self>) {
        let weak_self = Arc::downgrade(self);
        let cancel = self.cancel_token.child_token();

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        tracing::info!("QoS probing daemon received cancel signal");
                        break;
                    }
                    _ = interval.tick() => {
                        let Some(mgr) = weak_self.upgrade() else {
                            break;
                        };
                        mgr.run_qos_probing_tick().await;
                    }
                }
            }
        });

        let mut guard = self.qos_probe_handle.lock().await;
        if let Some(old) = guard.take() {
            old.abort();
            tracing::warn!("Aborted previous QoS probing task");
        }
        *guard = Some(handle);
        tracing::info!("QoS probing daemon started (interval: 5s)");
    }

    /// 执行一次 QoS 阶梯自愈扫描轮次
    async fn run_qos_probing_tick(&self) {
        let expired_jails = self.qos_guardian.take_expired_jails_for_probing();
        for (account_id, level) in expired_jails {
            let guardian = self.qos_guardian.clone();
            if let Some(token) = self.tokens.get(&account_id).map(|r| r.value().clone()) {
                tokio::spawn(async move {
                    tracing::info!(
                        "🔍 [QoS Probe] Sending synthetic 'Hello' probe to account {} (Level {})...",
                        account_id,
                        level
                    );
                    let probe_res = Self::send_pure_hello_probe_internal(&token).await;
                    match probe_res {
                        Ok(ttft_ms) if ttft_ms < 8000.0 => {
                            tracing::info!(
                                "🎉 [QoS Probe] Account {} PASSED probe ({:.2}s < 8s)! Moving to Half-Open probation.",
                                account_id,
                                ttft_ms / 1000.0
                            );
                            guardian.mark_probe_passed(&account_id, level);
                        }
                        Ok(ttft_ms) => {
                            tracing::warn!(
                                "⏳ [QoS Probe] Account {} probe TTFT was {:.2}s >= 8s. Escalating jail level.",
                                account_id,
                                ttft_ms / 1000.0
                            );
                            let (next_lv, wait) = guardian.mark_probe_failed(&account_id, level);
                            tracing::warn!(
                                "Account {} escalated to Level {} (wait {:?})",
                                account_id,
                                next_lv,
                                wait
                            );
                        }
                        Err(err) => {
                            tracing::warn!(
                                "⏳ [QoS Probe] Account {} probe failed: {}. Escalating jail level.",
                                account_id,
                                err
                            );
                            let (next_lv, wait) = guardian.mark_probe_failed(&account_id, level);
                            tracing::warn!(
                                "Account {} escalated to Level {} (wait {:?})",
                                account_id,
                                next_lv,
                                wait
                            );
                        }
                    }
                });
            }
        }
    }

    /// 发起纯净“你好”测速探针
    pub async fn send_pure_hello_probe(&self, account_id: &str) -> Result<f64, String> {
        let token = self
            .tokens
            .get(account_id)
            .map(|t| t.value().clone())
            .ok_or_else(|| format!("Account {} not found", account_id))?;
        Self::send_pure_hello_probe_internal(&token).await
    }

    /// 选择探针模型：只挑账号真实可用、且配额为正的 Gemini 文本 Flash 档。
    ///
    /// 探针的唯一目的是测量「账号 → 上游」的链路首包时延，因此必须满足：
    /// 1. 模型确实归属该账号（`model_quotas` / `available_models`），否则上游直接 400/429，
    ///    探针永远失败、刑期只能逐级升级，账号永远回不到 Healthy；
    /// 2. 走 Flash 档（单次成本最低、最不容易撞容量类 429，且不消耗 Pro / Argon 配额）。
    fn select_probe_model(token: &ProxyToken) -> Option<String> {
        let mut candidates: Vec<(String, i32)> = token
            .model_quotas
            .iter()
            .map(|(name, quota)| (name.clone(), *quota))
            .collect();
        // 历史账号可能尚未同步配额表，用 available_models 兜底补全（未知配额视为 0，排在末位）
        for name in &token.available_models {
            if !candidates.iter().any(|(known, _)| known == name) {
                candidates.push((name.clone(), 0));
            }
        }

        candidates.sort_by_key(|(name, quota)| {
            let lower = name.to_lowercase();
            let gemini_text =
                lower.starts_with("gemini") && !lower.contains("image") && !lower.contains("argon");
            (
                if gemini_text { 0 } else { 1 },
                if lower.contains("flash") { 0 } else { 1 },
                if *quota > 0 { 0 } else { 1 },
                lower.clone(),
            )
        });

        candidates.into_iter().next().map(|(name, _)| name)
    }

    /// 构造对齐官方 v1internal 信封的极简探针报文。
    ///
    /// [FIX] 修复历史缺陷：旧实现直接向 `v1internal:streamGenerateContent` 投递裸
    /// `{contents, generationConfig}`，缺少 `project` / `requestId` / `request` 包装，
    /// 上游恒定返回 `400 INVALID_ARGUMENT (Unknown name "contents")`，
    /// 使 QoS 阶梯探针永远失败、账号刑期只能逐级升级而无法自愈。
    fn build_probe_envelope(token: &ProxyToken, model: &str) -> serde_json::Value {
        let project_id = token
            .project_id
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .unwrap_or(DEFAULT_PROJECT_ID);

        serde_json::json!({
            "project": project_id,
            "requestId": crate::proxy::mappers::common_utils::build_official_request_id(
                &format!("qos-probe-{}", token.account_id),
                0,
            ),
            "request": {
                "contents": [{
                    "role": "user",
                    "parts": [{ "text": "你好" }]
                }],
                // 探针只测首包：输出预算与温度压到最低，避免占用真实配额
                "generationConfig": {
                    "maxOutputTokens": 10,
                    "temperature": 0.0
                }
            },
            "model": model,
            "userAgent": "antigravity"
        })
    }

    async fn send_pure_hello_probe_internal(token: &ProxyToken) -> Result<f64, String> {
        let model = Self::select_probe_model(token)
            .ok_or_else(|| format!("探针跳过：账号 {} 缺少可用模型元数据", token.email))?;
        let body = Self::build_probe_envelope(token, &model);

        let client = rquest::Client::builder()
            .timeout(std::time::Duration::from_millis(8000))
            .build()
            .map_err(|e| format!("Client builder error: {}", e))?;
        let hub_ua = crate::modules::hub_version::get_current_hub_user_agent();

        // 与主链路共用端点优先级：Daily 主力 → Sandbox 灾备
        let mut last_error = String::from("Probe attempted no endpoint");
        for base in crate::proxy::upstream::client::V1_INTERNAL_BASE_URL_FALLBACKS {
            let endpoint = format!("{}:streamGenerateContent?alt=sse", base);
            let start = std::time::Instant::now();

            let resp = match client
                .post(endpoint.as_str())
                .header("Authorization", format!("Bearer {}", token.access_token))
                .header("Content-Type", "application/json")
                .header("User-Agent", hub_ua.clone())
                .json(&body)
                .send()
                .await
            {
                Ok(resp) => resp,
                Err(e) => {
                    // 网络层故障属于端点级问题，换下一个候选端点继续探
                    last_error = format!("{} -> {}", endpoint, e);
                    continue;
                }
            };

            let status = resp.status();
            if !status.is_success() {
                last_error = format!("{} -> HTTP {} (model={})", endpoint, status, model);
                // 与 should_try_next_endpoint 保持一致的终止语义：408 / 404 / 5xx 才回退，
                // 400 / 401 / 403 / 429 属于账号或报文级结论，直接判失败，避免重试放大。
                let code = status.as_u16();
                if code == 408 || code == 404 || (500..600).contains(&code) {
                    continue;
                }
                return Err(last_error);
            }

            use futures::StreamExt;
            let mut stream = resp.bytes_stream();
            if let Some(chunk_res) = stream.next().await {
                let _ = chunk_res.map_err(|e| format!("{} -> stream error: {}", endpoint, e))?;
                let ttft_ms = start.elapsed().as_micros() as f64 / 1000.0;
                tracing::debug!(
                    "🔍 [QoS Probe] {} via {} (model={}) TTFT={:.0}ms",
                    token.email,
                    endpoint,
                    model,
                    ttft_ms
                );
                return Ok(ttft_ms);
            }

            last_error = format!("{} -> empty stream (model={})", endpoint, model);
        }

        Err(last_error)
    }

    /// [NEW] 从账号配额信息中提取最近的刷新时间戳
    ///
    /// Claude 模型（sonnet/opus）共用同一个刷新时间，只需取 claude 系列的 reset_time
    /// 返回 Unix 时间戳（秒），用于排序时比较
    fn extract_earliest_reset_time(&self, account: &serde_json::Value) -> Option<i64> {
        let models = account
            .get("quota")
            .and_then(|q| q.get("models"))
            .and_then(|m| m.as_array())?;

        let mut earliest_ts: Option<i64> = None;

        for model in models {
            // 优先取 claude 系列的 reset_time（sonnet/opus 共用）
            let model_name = model.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if !model_name.contains("claude") {
                continue;
            }

            if let Some(reset_time_str) = model.get("reset_time").and_then(|r| r.as_str()) {
                if reset_time_str.is_empty() {
                    continue;
                }
                // 解析 ISO 8601 时间字符串为时间戳
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(reset_time_str) {
                    let ts = dt.timestamp();
                    if earliest_ts.is_none() || ts < earliest_ts.unwrap() {
                        earliest_ts = Some(ts);
                    }
                }
            }
        }

        // 如果没有 claude 模型的时间，尝试取任意模型的最近时间
        if earliest_ts.is_none() {
            for model in models {
                if let Some(reset_time_str) = model.get("reset_time").and_then(|r| r.as_str()) {
                    if reset_time_str.is_empty() {
                        continue;
                    }
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(reset_time_str) {
                        let ts = dt.timestamp();
                        if earliest_ts.is_none() || ts < earliest_ts.unwrap() {
                            earliest_ts = Some(ts);
                        }
                    }
                }
            }
        }

        earliest_ts
    }

    /// [NEW] 从账号配额信息中提取周配额重置时间戳 (来自 retrieveUserQuotaSummary 的 7d/week 桶)
    /// 用于 Earliest Deadline First (EDF) 临期优先调度，防止周周期重置作废剩余配额
    fn extract_weekly_reset_time(&self, account: &serde_json::Value) -> Option<i64> {
        let groups = account
            .get("quota")
            .and_then(|q| q.get("quota_groups"))
            .and_then(|g| g.as_array())?;

        let mut earliest_weekly_ts: Option<i64> = None;

        for group in groups {
            if let Some(buckets) = group.get("buckets").and_then(|b| b.as_array()) {
                for bucket in buckets {
                    let window = bucket.get("window").and_then(|w| w.as_str()).unwrap_or("");
                    let bucket_id = bucket
                        .get("bucket_id")
                        .or_else(|| bucket.get("bucketId"))
                        .and_then(|b| b.as_str())
                        .unwrap_or("");
                    let win_lower = window.to_lowercase();
                    let bid_lower = bucket_id.to_lowercase();

                    let is_weekly = win_lower.contains("week")
                        || bid_lower.contains("week")
                        || win_lower.contains("7d")
                        || bid_lower.contains("7d");

                    if is_weekly {
                        let reset_val = bucket
                            .get("reset_time")
                            .or_else(|| bucket.get("resetTime"))
                            .and_then(|r| r.as_str());
                        if let Some(reset_str) = reset_val {
                            if !reset_str.is_empty() {
                                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(reset_str) {
                                    let ts = dt.timestamp();
                                    if earliest_weekly_ts.is_none()
                                        || ts < earliest_weekly_ts.unwrap()
                                    {
                                        earliest_weekly_ts = Some(ts);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        earliest_weekly_ts
    }

    /// Restore official quota windows without replacing independent upstream limits.
    fn sync_zero_quota_circuit_breaker(&self, account_id: &str, account: &serde_json::Value) {
        let lock_on_zero = if let Ok(cfg) = self.circuit_breaker_config.try_read() {
            cfg.enabled && cfg.lock_on_zero_quota
        } else {
            false
        };

        let quota = match account.get("quota") {
            Some(q) => q,
            None => return,
        };

        let observed_at = quota
            .get("last_updated")
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
            .saturating_mul(1000);
        if let Some(groups) = quota.get("quota_groups").and_then(|g| g.as_array()) {
            for group in groups {
                let group_name = group
                    .get("display_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let group_name_lower = group_name.to_lowercase();
                let is_argon_group = group_name_lower.contains("argon");
                let is_claude_group = !is_argon_group
                    && (group_name_lower.contains("claude")
                        || group_name_lower.contains("gpt")
                        || group_name_lower.contains("3p"));
                let is_gemini_group = !is_argon_group
                    && !is_claude_group
                    && (group_name_lower.contains("gemini")
                        || (!group_name_lower.contains("claude")
                            && !group_name_lower.contains("gpt")
                            && !group_name_lower.contains("3p")));

                if let Some(buckets) = group.get("buckets").and_then(|b| b.as_array()) {
                    for bucket in buckets {
                        let bucket_id = bucket
                            .get("bucket_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let window = bucket.get("window").and_then(|v| v.as_str()).unwrap_or("");
                        let window_key = format!("{} {}", bucket_id, window).to_lowercase();
                        let weekly = window_key.contains("week") || window_key.contains("7d");
                        if !weekly
                            && (!lock_on_zero
                                || !(window_key.contains("5h") || window_key.contains("hour")))
                        {
                            continue;
                        }
                        let Some(fraction) =
                            bucket.get("remaining_fraction").and_then(|v| v.as_f64())
                        else {
                            continue;
                        };
                        let exhausted_until = if fraction <= 0.001 {
                            let Some(reset) = bucket
                                .get("reset_time")
                                .and_then(|v| v.as_str())
                                .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
                            else {
                                continue;
                            };
                            Some(std::time::SystemTime::from(reset))
                        } else {
                            None
                        };
                        let bucket_id_lower = bucket_id.to_lowercase();
                        let is_argon_bucket = is_argon_group || bucket_id_lower.contains("argon");
                        let third_party =
                            !is_argon_bucket && (is_claude_group || bucket_id_lower.contains("3p"));
                        let gemini = !is_argon_bucket
                            && !third_party
                            && (is_gemini_group || bucket_id_lower.contains("gemini"));

                        let mut models: Vec<&str> = if is_argon_bucket {
                            vec![
                                "gemini-4-argon",
                                "gemini-4-argon-high",
                                "gemini-4-argon-medium",
                                "gemini-4-argon-low",
                            ]
                        } else if third_party {
                            vec!["claude", "claude-sonnet-4-6", "gpt-oss-120b-medium"]
                        } else if gemini {
                            vec![
                                "gemini-3-flash",
                                "gemini-3.1-pro-high",
                                "gemini-3.1-flash-image",
                                "gemini-3-pro-image",
                            ]
                        } else {
                            Vec::new()
                        };
                        if let Some(available) = quota.get("models").and_then(|v| v.as_array()) {
                            models.extend(
                                available
                                    .iter()
                                    .filter_map(|m| m.get("name")?.as_str())
                                    .filter(|name| {
                                        let n_lower = name.to_lowercase();
                                        if is_argon_bucket {
                                            n_lower.contains("argon")
                                        } else if third_party {
                                            !n_lower.contains("argon")
                                                && (name.starts_with("claude")
                                                    || name.starts_with("gpt"))
                                        } else {
                                            gemini
                                                && name.starts_with("gemini")
                                                && !n_lower.contains("argon")
                                        }
                                    }),
                            );
                        }
                        for model in models {
                            let normalized =
                                crate::proxy::common::model_mapping::normalize_to_standard_id(
                                    model,
                                )
                                .unwrap_or_else(|| model.to_string());
                            self.rate_limit_tracker.sync_quota_bucket(
                                account_id,
                                &normalized,
                                bucket_id,
                                bucket
                                    .get("observed_at")
                                    .and_then(|v| v.as_i64())
                                    .unwrap_or(observed_at),
                                exhausted_until,
                                weekly,
                            );
                        }
                    }
                }
            }
            if !groups.is_empty() {
                return;
            }
        }

        // 2. 回退到 models 配额检查
        if !lock_on_zero {
            return;
        }
        if let Some(models) = quota.get("models").and_then(|m| m.as_array()) {
            // 只要受监控核心模型或全部模型为 0%，且有有效 reset_time
            let all_zero = models
                .iter()
                .all(|m| m.get("percentage").and_then(|p| p.as_i64()).unwrap_or(100) == 0);

            if all_zero && !models.is_empty() {
                if let Some(reset_time_str) = self.get_quota_reset_time(account_id, None) {
                    if !chrono::DateTime::parse_from_rfc3339(&reset_time_str)
                        .is_ok_and(|reset| reset > chrono::Utc::now())
                    {
                        return;
                    }
                    tracing::warn!(
                        "[CircuitBreaker] 账号 {} 的模型配额已全部为 0%, 持续锁定至 {}",
                        account_id,
                        reset_time_str
                    );
                    self.rate_limit_tracker.set_lockout_until_iso_with_cap(
                        account_id,
                        &reset_time_str,
                        crate::proxy::rate_limit::RateLimitReason::QuotaExhausted,
                        None,
                        false,
                    );
                }
            }
        }
    }

    /// 获取当前所有可用账号中收集到的官方下发的所有动态模型集合
    pub fn get_all_collected_models(&self) -> std::collections::HashSet<String> {
        let mut all_models = std::collections::HashSet::new();
        for entry in self.tokens.iter() {
            let token = entry.value();

            for model_id in &token.available_models {
                all_models.insert(model_id.clone());
            }

            // Keep the raw quota model IDs for /v1/models discovery. `model_quotas`
            // intentionally stores normalized protection buckets (e.g. gemini-3-flash),
            // but clients need concrete usable IDs such as gemini-3-flash-agent.
            if let Some(raw_models) = Self::get_available_models_from_json(&token.account_path) {
                for model_id in raw_models {
                    all_models.insert(model_id);
                }
            }

            // Also keep normalized bucket IDs for existing quota/protection behavior.
            for model_id in token.model_quotas.keys() {
                all_models.insert(model_id.clone());
            }
        }
        all_models
    }

    /// [NEW #3677] 获取当前所有可用账号中收集到的官方动态模型能力与元数据集合
    pub fn get_all_model_metadata(
        &self,
    ) -> std::collections::HashMap<String, crate::models::quota::ModelListingMeta> {
        let mut all_meta = std::collections::HashMap::new();
        for entry in self.tokens.iter() {
            let token = entry.value();
            for (name, meta) in &token.model_metadata {
                all_meta
                    .entry(name.clone())
                    .or_insert_with(crate::models::quota::ModelListingMeta::default)
                    .merge_from(meta);
            }
        }
        all_meta
    }

    /// [NEW] 从指定账号的动态额度数据中获取特定模型的 max_output_tokens
    ///
    /// # 返回
    /// - `Some(u64)`: 找到了动态限额数据
    /// - `None`: 账号不存在或该模型无数据（调用方应继续查静态默认表）
    pub fn get_model_output_limit_for_account(
        &self,
        account_id: &str,
        model_name: &str,
    ) -> Option<u64> {
        self.tokens
            .get(account_id)
            .and_then(|token| token.model_limits.get(model_name).copied())
    }

    /// Helper to find account ID by email
    pub fn get_account_id_by_email(&self, email: &str) -> Option<String> {
        for entry in self.tokens.iter() {
            if entry.value().email == email {
                return Some(entry.key().clone());
            }
        }
        None
    }

    /// Set validation blocked status for an account (internal)
    pub async fn set_validation_block(
        &self,
        account_id: &str,
        block_until: i64,
        reason: &str,
    ) -> Result<(), String> {
        // 1. Update memory
        if let Some(mut token) = self.tokens.get_mut(account_id) {
            token.validation_blocked = true;
            token.validation_blocked_until = block_until;
        }

        // 2. Persist to disk
        let path = self
            .data_dir
            .join("accounts")
            .join(format!("{}.json", account_id));
        if !path.exists() {
            return Err(format!("Account file not found: {:?}", path));
        }

        // [NEW] 尝试从消息中提取验证链接 (#1522)
        let extracted_url = if let Ok(parsed_json) =
            serde_json::from_str::<serde_json::Value>(reason)
        {
            // 尝试从特定的 Google RPC error 结构中取
            let mut url = None;
            if let Some(details) = parsed_json.pointer("/error/details") {
                if let Some(arr) = details.as_array() {
                    for detail in arr {
                        if let Some(meta) = detail.get("metadata") {
                            if let Some(v_url) = meta.get("validation_url").and_then(|v| v.as_str())
                            {
                                url = Some(v_url.to_string());
                                break;
                            }
                            if let Some(a_url) = meta.get("appeal_url").and_then(|v| v.as_str()) {
                                url = Some(a_url.to_string());
                                break;
                            }
                        }
                    }
                }
            }
            url
        } else {
            // 回退方案：通过更严格的正则及反序列化解码可能的 \u0026
            let url_regex = regex::Regex::new(r#"https://[^\s"'\\]+"#).unwrap();
            url_regex.find(reason).map(|m| {
                let raw_url = m.as_str().to_string();
                raw_url.replace("\\u0026", "&")
            })
        };

        if let Some(ref url) = extracted_url {
            if let Some(mut token) = self.tokens.get_mut(account_id) {
                token.validation_url = Some(url.clone());
            }
        }

        let reason_owned = reason.to_string();
        update_account_json(&path, move |account| {
            account["validation_blocked"] = serde_json::Value::Bool(true);
            account["validation_blocked_until"] =
                serde_json::Value::Number(serde_json::Number::from(block_until));
            account["validation_blocked_reason"] = serde_json::Value::String(reason_owned);
            if let Some(url) = extracted_url {
                account["validation_url"] = serde_json::Value::String(url);
            }
        })
        .await?;

        // Clear sticky session if blocked
        self.session_accounts.retain(|_, v| *v != account_id);

        tracing::info!(
            "🚫 Account {} validation blocked until {} (reason: {})",
            account_id,
            block_until,
            reason
        );

        Ok(())
    }

    /// Public method to set validation block (called from handlers)
    pub async fn set_validation_block_public(
        &self,
        account_id: &str,
        block_until: i64,
        reason: &str,
    ) -> Result<(), String> {
        self.set_validation_block(account_id, block_until, reason)
            .await
    }

    /// Set is_forbidden status for an account (called when proxy encounters 403)
    pub async fn set_forbidden(&self, account_id: &str, reason: &str) -> Result<(), String> {
        // [FIX] 调用封装好的模块函数，确保线程安全地更新账号文件和索引
        crate::modules::account::mark_account_forbidden(account_id, reason)?;

        // Clear sticky session if forbidden
        self.session_accounts.retain(|_, v| *v != account_id);

        // [FIX] 从内存池中移除账号，避免重试时再次选中
        self.remove_account(account_id);

        tracing::warn!(
            "🚫 Account {} marked as forbidden (403): {}",
            account_id,
            truncate_reason(reason, 1000)
        );

        Ok(())
    }
}

/// 截断过长的原因字符串
fn truncate_reason(reason: &str, max_len: usize) -> String {
    if reason.len() <= max_len {
        reason.to_string()
    } else {
        let budget = max_len.saturating_sub(3);
        let end = crate::proxy::mappers::common_utils::safe_truncate_str(reason, budget);
        format!("{}...", end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;
    use std::time::Duration;

    fn weekly_quota_account(now: i64) -> serde_json::Value {
        let reset = |seconds| {
            chrono::DateTime::from_timestamp(now + seconds, 0)
                .unwrap()
                .to_rfc3339()
        };
        serde_json::json!({
            "id": "weekly-test", "email": "quota@test.invalid", "created_at": now, "last_used": now,
            "token": {"access_token": "test", "refresh_token": "test", "token_type": "Bearer",
                "expires_in": 3600, "expiry_timestamp": now + 3600, "project_id": "test"},
            "quota": {"last_updated": now, "models": [
                {"name": "gemini-3.1-pro-high", "percentage": 0, "reset_time": reset(7200)},
                {"name": "claude-sonnet-4-6", "percentage": 100, "reset_time": reset(1800)}
            ], "quota_groups": [
                {"display_name": "Gemini Models", "buckets": [
                    {"bucket_id": "gemini-weekly", "window": "weekly", "remaining_fraction": 0.0, "reset_time": reset(7200)},
                    {"bucket_id": "gemini-5h", "window": "5h", "remaining_fraction": 1.0, "reset_time": reset(1800)}
                ]},
                {"display_name": "Claude and GPT models", "buckets": [
                    {"bucket_id": "3p-weekly", "window": "weekly", "remaining_fraction": 1.0, "reset_time": reset(7200)}
                ]}
            ]}
        })
    }

    #[test]
    fn project_id_negative_cache_honors_ttl() {
        let manager = TokenManager::new(PathBuf::new());
        assert!(!manager.is_project_id_negatively_cached("acct"));

        manager
            .project_id_failures
            .insert("acct".to_string(), std::time::Instant::now());
        assert!(manager.is_project_id_negatively_cached("acct"));

        let expired_at = std::time::Instant::now()
            .checked_sub(PROJECT_ID_NEGATIVE_CACHE_TTL + Duration::from_secs(1))
            .unwrap();
        manager
            .project_id_failures
            .insert("acct".to_string(), expired_at);
        assert!(!manager.is_project_id_negatively_cached("acct"));
        assert!(!manager.project_id_failures.contains_key("acct"));
    }

    #[tokio::test]
    async fn project_id_negative_cache_short_circuits_without_waiting_on_locks() {
        let manager = TokenManager::new(PathBuf::new());
        manager
            .project_id_failures
            .insert("acct".to_string(), std::time::Instant::now());

        // 同时占住 OAuth 刷新锁与 project_id 探测锁：命中负缓存时不得等待任何一把锁
        let refresh_mu = Arc::new(tokio::sync::Mutex::new(()));
        manager
            .refresh_locks
            .insert("acct".to_string(), refresh_mu.clone());
        let resolve_mu = Arc::new(tokio::sync::Mutex::new(()));
        manager
            .project_id_locks
            .insert("acct".to_string(), resolve_mu.clone());
        let _refresh_guard = refresh_mu.lock().await;
        let _resolve_guard = resolve_mu.lock().await;

        let pid = tokio::time::timeout(
            Duration::from_millis(200),
            manager.resolve_project_id_with_timeout(
                "acct",
                "token",
                std::path::Path::new("/nonexistent/acct.json"),
                Duration::from_secs(5),
            ),
        )
        .await
        .expect("negative cache hit must not block");
        assert_eq!(pid, DEFAULT_PROJECT_ID);
    }

    #[test]
    fn remove_account_clears_per_account_lock_and_failure_state() {
        let manager = TokenManager::new(PathBuf::new());
        let id = "acct".to_string();
        manager
            .refresh_locks
            .insert(id.clone(), Arc::new(tokio::sync::Mutex::new(())));
        manager.invalid_grant_failures.insert(id.clone(), 1);
        manager
            .project_id_locks
            .insert(id.clone(), Arc::new(tokio::sync::Mutex::new(())));
        manager
            .project_id_failures
            .insert(id.clone(), std::time::Instant::now());

        manager.remove_account(&id);

        assert!(!manager.refresh_locks.contains_key(&id));
        assert!(!manager.invalid_grant_failures.contains_key(&id));
        assert!(!manager.project_id_locks.contains_key(&id));
        assert!(!manager.project_id_failures.contains_key(&id));
    }

    #[tokio::test]
    async fn weekly_quota_blocks_by_default_and_survives_reload_and_resets() {
        let _data_dir = crate::proxy::monitor::prompt_log_tests::TestDataDir::new();
        let data_dir = crate::modules::account::get_data_dir().unwrap();
        let accounts = data_dir.join("accounts");
        std::fs::create_dir(&accounts).unwrap();
        let mut snapshot = weekly_quota_account(chrono::Utc::now().timestamp());
        snapshot["quota"]["quota_groups"][0]["buckets"][0]["remaining_fraction"] =
            serde_json::json!(0.0005);
        std::fs::write(accounts.join("weekly-test.json"), snapshot.to_string()).unwrap();
        let manager = TokenManager::new(data_dir);
        manager.load_accounts().await.unwrap();
        assert!(
            manager
                .is_rate_limited("weekly-test", Some("gemini-3-pro-high"))
                .await
        );
        assert!(!manager.is_rate_limited("weekly-test", Some("claude")).await);
        manager.circuit_breaker_config.write().await.enabled = false;
        manager.reload_account("weekly-test").await.unwrap();
        manager.load_accounts().await.unwrap();
        manager.rate_limit_tracker.clear_for_optimistic_reset();
        manager.clear_all_rate_limits();
        assert!(
            manager
                .is_rate_limited("weekly-test", Some("gemini-3-pro-high"))
                .await
        );
        assert!(manager
            .get_token("gemini", false, None, "gemini-3.1-pro-high")
            .await
            .is_err());
        assert!(manager
            .get_token("claude", false, None, "claude-sonnet-4-6")
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn test_zero_downtime_atomic_account_reload() {
        let _data_dir = crate::proxy::monitor::prompt_log_tests::TestDataDir::new();
        let data_dir = crate::modules::account::get_data_dir().unwrap();
        let accounts = data_dir.join("accounts");
        std::fs::create_dir_all(&accounts).unwrap();

        let now = chrono::Utc::now().timestamp();
        let mut acc1 = weekly_quota_account(now);
        acc1["id"] = serde_json::json!("acc-1");
        std::fs::write(accounts.join("acc-1.json"), acc1.to_string()).unwrap();

        let manager = TokenManager::new(data_dir.clone());
        let count = manager.load_accounts().await.unwrap();
        assert_eq!(count, 1);
        assert!(manager.tokens.contains_key("acc-1"));

        // 模拟磁盘写入新账号 acc-2
        let mut acc2 = weekly_quota_account(now);
        acc2["id"] = serde_json::json!("acc-2");
        std::fs::write(accounts.join("acc-2.json"), acc2.to_string()).unwrap();

        // 重新加载：平滑同步，包含两账号
        let count2 = manager.load_accounts().await.unwrap();
        assert_eq!(count2, 2);
        assert!(manager.tokens.contains_key("acc-1"));
        assert!(manager.tokens.contains_key("acc-2"));

        // 模拟磁盘删除 acc-1
        std::fs::remove_file(accounts.join("acc-1.json")).unwrap();

        // 重新加载：acc-1 被安全剔除，acc-2 保留
        let count3 = manager.load_accounts().await.unwrap();
        assert_eq!(count3, 1);
        assert!(!manager.tokens.contains_key("acc-1"));
        assert!(manager.tokens.contains_key("acc-2"));
    }

    #[tokio::test]
    async fn weekly_quota_recovery_requires_new_same_bucket_and_preserves_other_limits() {
        let manager = TokenManager::new(PathBuf::new());
        let now = chrono::Utc::now().timestamp();
        let mut snapshot = weekly_quota_account(now);
        snapshot["quota"]["quota_groups"][0]["buckets"][0]["remaining_fraction"] =
            serde_json::json!(0.001);
        let tracker = &manager.rate_limit_tracker;
        manager
            .circuit_breaker_config
            .write()
            .await
            .lock_on_zero_quota = true;
        snapshot["quota"]["quota_groups"][0]["buckets"][1]["remaining_fraction"] =
            serde_json::json!(0);
        manager.sync_zero_quota_circuit_breaker("a", &snapshot);
        tracker.set_lockout_until_with_cap(
            "a",
            std::time::SystemTime::now() + Duration::from_secs(10800),
            crate::proxy::rate_limit::RateLimitReason::QuotaExhausted,
            Some("gemini-3-pro-image".into()),
            false,
        );
        tracker.set_lockout_until(
            "a",
            std::time::SystemTime::now() + Duration::from_secs(120),
            crate::proxy::rate_limit::RateLimitReason::RateLimitExceeded,
            None,
        );
        let mut positive = snapshot.clone();
        positive["quota"]["quota_groups"][0]["buckets"][0]["remaining_fraction"] =
            serde_json::json!(0.0011);
        manager.sync_zero_quota_circuit_breaker("a", &positive); // Same old snapshot cannot unlock.
        manager.sync_zero_quota_circuit_breaker("a", &serde_json::json!({"quota": {"models": []}}));
        assert!(tracker.get_quota_wait("a", Some("gemini-3-pro-high"), true) > 7000);
        for (offset, fraction) in [(1, 0.0005), (2, 0.001)] {
            positive["quota"]["last_updated"] = serde_json::json!(now + offset);
            positive["quota"]["quota_groups"][0]["buckets"][0]["remaining_fraction"] =
                serde_json::json!(fraction);
            manager.sync_zero_quota_circuit_breaker("a", &positive);
            assert!(tracker.get_quota_wait("a", Some("gemini-3-pro-high"), true) > 7000);
        }
        positive["quota"]["last_updated"] = serde_json::json!(now + 3);
        positive["quota"]["quota_groups"][0]["buckets"][0]["remaining_fraction"] =
            serde_json::json!(0.0011);
        manager.sync_zero_quota_circuit_breaker("a", &positive);
        assert_eq!(
            tracker.get_quota_wait("a", Some("gemini-3-pro-high"), true),
            0
        );
        assert!(tracker.get_remaining_wait("a", Some("gemini-3-pro-high")) > 1700); // 5h still exhausted.
        assert!(tracker.get_remaining_wait("a", Some("gemini-3-pro-image")) > 10000);
        assert!(tracker.is_rate_limited("a", None)); // Independent account-level upstream limit.
        manager.sync_zero_quota_circuit_breaker("a", &snapshot); // Older zero must not relock.
        assert_eq!(
            tracker.get_quota_wait("a", Some("gemini-3-pro-high"), true),
            0
        );
    }

    #[test]
    fn weekly_quota_missing_data_persists_for_restart_without_renewing_observation() {
        let now = chrono::Utc::now().timestamp();
        let mut account: crate::models::Account =
            serde_json::from_value(weekly_quota_account(now)).unwrap();
        let mut refresh = account.quota.clone().unwrap();
        refresh.last_updated += 1;
        refresh.quota_groups.as_mut().unwrap()[0].buckets.remove(0); // Partial summary / 5h recovery.
        account.update_quota(refresh);
        let mut failed = account.quota.clone().unwrap();
        failed.last_updated += 1;
        failed.quota_groups = None;
        account.update_quota(failed);
        let snapshot = serde_json::to_value(&account).unwrap();
        let bucket = &snapshot["quota"]["quota_groups"][0]["buckets"][1];
        assert_eq!(bucket["bucket_id"], "gemini-weekly");
        assert_eq!(bucket["observed_at"], now * 1000);
        let restarted = TokenManager::new(PathBuf::new());
        restarted.sync_zero_quota_circuit_breaker("a", &snapshot);
        assert!(restarted
            .rate_limit_tracker
            .is_rate_limited("a", Some("gemini-3-pro-high")));
    }

    #[tokio::test]
    async fn weekly_quota_windows_are_order_independent_and_expired_snapshots_stay_expired() {
        let now = chrono::Utc::now().timestamp();
        for reverse in [false, true] {
            let manager = TokenManager::new(PathBuf::new());
            manager
                .circuit_breaker_config
                .write()
                .await
                .lock_on_zero_quota = true;
            let mut snapshot = weekly_quota_account(now);
            let buckets = snapshot["quota"]["quota_groups"][0]["buckets"]
                .as_array_mut()
                .unwrap();
            buckets[1]["remaining_fraction"] = serde_json::json!(0);
            if reverse {
                buckets.reverse();
            }
            manager.sync_zero_quota_circuit_breaker("a", &snapshot);
            for model in [
                "gemini-3-pro-high",
                "gemini-3-flash",
                "gemini-3.1-flash-image",
                "gemini-3-pro-image",
            ] {
                assert!(
                    manager
                        .rate_limit_tracker
                        .get_remaining_wait("a", Some(model))
                        > 7000
                );
            }
            let expired = weekly_quota_account(now - 8000);
            manager.sync_zero_quota_circuit_breaker("expired", &expired);
            manager.sync_zero_quota_circuit_breaker("expired", &expired);
            assert!(!manager
                .rate_limit_tracker
                .is_rate_limited("expired", Some("gemini-3-pro-high")));
        }
    }

    #[test]
    fn test_build_dynamic_model_candidates_agent() {
        let candidates = TokenManager::build_dynamic_model_candidates("gemini-pro-agent").unwrap();
        assert_eq!(candidates[0], "gemini-pro-agent");
        assert!(candidates.contains(&"gemini-3.1-pro-low".to_string()));

        let candidates_high =
            TokenManager::build_dynamic_model_candidates("gemini-3.1-pro-high").unwrap();
        assert_eq!(candidates_high[0], "gemini-3.1-pro-high");
        assert!(candidates_high.contains(&"gemini-pro-agent".to_string()));
    }

    #[tokio::test]
    async fn task_reload_account_preserves_live_limit_and_syncs_disabled_state() {
        let tmp_root = std::env::temp_dir().join(format!(
            "antigravity-token-manager-test-{}",
            uuid::Uuid::new_v4()
        ));
        let accounts_dir = tmp_root.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();

        let account_id = "acc1";
        let model = "gemini-3-pro-image";
        let email = "a@test.com";
        let now = chrono::Utc::now().timestamp();
        let account_path = accounts_dir.join(format!("{}.json", account_id));

        let account_json = serde_json::json!({
            "id": account_id,
            "email": email,
            "token": {
                "access_token": "atk",
                "refresh_token": "rtk",
                "expires_in": 3600,
                "expiry_timestamp": now + 3600
            },
            "disabled": false,
            "proxy_disabled": false,
            "created_at": now,
            "last_used": now,
            "live_limited_models": {
                model: {
                    "model": model,
                    "status": 429,
                    "reason": "QuotaExhausted",
                    "until": now + 7200,
                    "detected_at": now,
                    "message": "{\"error\":{\"details\":[{\"reason\":\"QUOTA_EXHAUSTED\",\"metadata\":{\"quotaResetDelay\":\"2h\"}}]}}"
                },
                "gemini-3.1-flash-image": {
                    "model": "gemini-3.1-flash-image",
                    "status": 429,
                    "reason": "QuotaExhausted",
                    "until": now + 7200,
                    "detected_at": now,
                    "message": "QUOTA_EXHAUSTED"
                },
                "gemini-2.5-pro": {
                    "model": "gemini-2.5-pro",
                    "status": 429,
                    "reason": "QuotaExhausted",
                    "until": now + 7200,
                    "detected_at": now,
                    "message": "QUOTA_EXHAUSTED"
                }
            }
        });
        std::fs::write(
            &account_path,
            serde_json::to_string_pretty(&account_json).unwrap(),
        )
        .unwrap();

        let manager = TokenManager::new(tmp_root.clone());
        manager.load_accounts().await.unwrap();
        assert!(manager.tokens.get(account_id).is_some());
        assert!(manager
            .rate_limit_tracker
            .is_rate_limited(account_id, Some(model)));
        assert!(!manager
            .rate_limit_tracker
            .is_rate_limited(account_id, Some("gemini-3.1-flash-image")));
        assert!(!manager
            .rate_limit_tracker
            .is_rate_limited(account_id, Some("gemini-2.5-pro")));

        let temporary_model = "gemini-3.1-flash-image";
        manager.rate_limit_tracker.parse_from_error(
            account_id,
            429,
            Some("60"),
            "temporary rate limit",
            Some(temporary_model.to_string()),
            &[60, 300],
        );
        assert!(manager
            .rate_limit_tracker
            .is_rate_limited(account_id, Some(temporary_model)));
        assert!(manager.clear_rate_limit_memory(account_id));
        assert!(!manager
            .rate_limit_tracker
            .is_rate_limited(account_id, Some(model)));

        manager.reload_account(account_id).await.unwrap();
        assert!(manager
            .rate_limit_tracker
            .is_rate_limited(account_id, Some(model)));
        assert!(!manager
            .rate_limit_tracker
            .is_rate_limited(account_id, Some(temporary_model)));

        // Prime extra caches to ensure remove_account() is really called.
        manager
            .session_accounts
            .insert("sid1".to_string(), account_id.to_string());
        {
            let mut preferred = manager.preferred_account_id.write().await;
            *preferred = Some(account_id.to_string());
        }

        // Mark account as proxy-disabled on disk (manual disable).
        let mut disabled_json = account_json.clone();
        disabled_json["proxy_disabled"] = serde_json::Value::Bool(true);
        disabled_json["proxy_disabled_reason"] = serde_json::Value::String("manual".to_string());
        disabled_json["proxy_disabled_at"] = serde_json::Value::Number(now.into());
        std::fs::write(
            &account_path,
            serde_json::to_string_pretty(&disabled_json).unwrap(),
        )
        .unwrap();

        manager.reload_account(account_id).await.unwrap();

        assert!(manager.tokens.get(account_id).is_none());
        assert!(manager.session_accounts.get("sid1").is_none());
        assert!(manager.preferred_account_id.read().await.is_none());

        disabled_json["proxy_disabled"] = serde_json::Value::Bool(false);
        std::fs::write(
            &account_path,
            serde_json::to_string_pretty(&disabled_json).unwrap(),
        )
        .unwrap();
        manager.reload_account(account_id).await.unwrap();
        assert!(manager
            .rate_limit_tracker
            .is_rate_limited(account_id, Some(model)));

        let restarted = TokenManager::new(tmp_root.clone());
        restarted.load_accounts().await.unwrap();
        assert!(restarted
            .rate_limit_tracker
            .is_rate_limited(account_id, Some(model)));

        let _ = std::fs::remove_dir_all(&tmp_root);
    }

    #[tokio::test]
    async fn task_account_json_update_preserves_live_limits() {
        let tmp_root = std::env::temp_dir().join(format!(
            "antigravity-account-update-test-{}",
            uuid::Uuid::new_v4()
        ));
        let accounts_dir = tmp_root.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();
        let account_id = "acc-update";
        let account_path = accounts_dir.join(format!("{}.json", account_id));
        let live_limit = serde_json::json!({
            "model": "gemini-3-pro-image",
            "status": 429,
            "reason": "QuotaExhausted",
            "until": chrono::Utc::now().timestamp() + 7200,
            "detected_at": chrono::Utc::now().timestamp(),
            "message": "QUOTA_EXHAUSTED; reset after 2h"
        });
        std::fs::write(
            &account_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "id": account_id,
                "live_limited_models": {
                    "gemini-3-pro-image": live_limit
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let manager = TokenManager::new(tmp_root.clone());
        manager.disable_account(account_id, "test").await.unwrap();

        let updated: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&account_path).unwrap()).unwrap();
        assert_eq!(
            updated["live_limited_models"]["gemini-3-pro-image"],
            live_limit
        );
        assert_eq!(updated["disabled"], true);

        let mut account_snapshot = updated;
        assert!(manager
            .trigger_quota_protection(
                &mut account_snapshot,
                account_id,
                &account_path,
                0,
                10,
                "gemini-3-flash",
            )
            .await
            .unwrap());
        assert!(manager
            .restore_quota_protection(
                &mut account_snapshot,
                account_id,
                &account_path,
                "gemini-3-flash",
            )
            .await
            .unwrap());

        account_snapshot["proxy_disabled"] = serde_json::Value::Bool(true);
        account_snapshot["proxy_disabled_reason"] =
            serde_json::Value::String("quota_protection".to_string());
        let quota = serde_json::json!({
            "models": [{ "name": "gemini-3-flash", "percentage": 0 }]
        });
        let config = crate::models::QuotaProtectionConfig {
            enabled: true,
            threshold_percentage: 10,
            monitored_models: vec!["gemini-3-flash".to_string()],
        };
        manager
            .check_and_restore_quota(&mut account_snapshot, &account_path, &quota, &config)
            .await;

        update_account_json(&account_path, |latest| {
            latest["validation_blocked"] = serde_json::Value::Bool(true);
            latest["validation_blocked_until"] =
                serde_json::Value::Number((chrono::Utc::now().timestamp() - 1).into());
            latest["validation_blocked_reason"] = serde_json::Value::String("expired".to_string());
        })
        .await
        .unwrap();
        manager.load_single_account(&account_path).await.unwrap();

        let updated: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&account_path).unwrap()).unwrap();
        assert_eq!(
            updated["live_limited_models"]["gemini-3-pro-image"],
            live_limit
        );
        assert_eq!(updated["validation_blocked"], false);
        assert_eq!(
            updated["protected_models"],
            serde_json::json!(["gemini-3-flash"])
        );

        let _ = std::fs::remove_dir_all(&tmp_root);
    }

    #[tokio::test]
    async fn task_image_selection_respects_queue_deadline() {
        let result = wait_for_image_token_selection(
            tokio::time::Instant::now() + std::time::Duration::from_millis(20),
            std::future::pending::<()>(),
        )
        .await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn task_image_queue_reselects_without_scheduler_notification() {
        let (_sender, mut changes) = tokio::sync::watch::channel(0);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            wait_for_image_account_change(&mut changes, std::time::Duration::from_secs(1)),
        )
        .await;
        assert_eq!(result, Ok(true));
    }

    #[tokio::test]
    async fn task_short_limit_buffer_reselects_without_blocking_runtime() {
        let tmp_root = std::env::temp_dir().join(format!(
            "antigravity-short-limit-test-{}",
            uuid::Uuid::new_v4()
        ));
        let accounts_dir = tmp_root.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();
        let account_id = "acc-short-limit";
        let model = "gemini-3-flash";
        let now = chrono::Utc::now().timestamp();
        std::fs::write(
            accounts_dir.join(format!("{}.json", account_id)),
            serde_json::to_string_pretty(&serde_json::json!({
                "id": account_id,
                "email": "short-limit@test.com",
                "token": {
                    "access_token": "atk",
                    "refresh_token": "rtk",
                    "expires_in": 3600,
                    "expiry_timestamp": now + 3600,
                    "project_id": "pid"
                },
                "quota": {
                    "models": [{ "name": model, "percentage": 100 }]
                },
                "disabled": false,
                "proxy_disabled": false,
                "created_at": now,
                "last_used": now
            }))
            .unwrap(),
        )
        .unwrap();

        let manager = TokenManager::new(tmp_root.clone());
        manager.load_accounts().await.unwrap();
        manager.rate_limit_tracker.parse_from_error(
            account_id,
            429,
            Some("1"),
            r#"{"error":{"details":[{"reason":"QUOTA_EXHAUSTED"}]}}"#,
            Some(model.to_string()),
            &[60, 300],
        );

        let selected = tokio::time::timeout(
            std::time::Duration::from_secs(4),
            manager.get_token("gemini", false, None, model),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(selected.3, account_id);
        assert!(!manager.is_rate_limited(account_id, Some(model)).await);

        let _ = std::fs::remove_dir_all(&tmp_root);
    }

    #[tokio::test]
    async fn task_concurrent_image_limits_persist_and_clear_exact_bucket() {
        let tmp_root = std::env::temp_dir().join(format!(
            "antigravity-live-limit-test-{}",
            uuid::Uuid::new_v4()
        ));
        let accounts_dir = tmp_root.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();
        let account_id = "acc-concurrent";
        let account_path = accounts_dir.join(format!("{}.json", account_id));
        std::fs::write(
            &account_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "id": account_id,
                "email": "concurrent@test.com",
                "live_limited_models": {}
            }))
            .unwrap(),
        )
        .unwrap();

        let manager = TokenManager::new(tmp_root.clone());
        let body = r#"{"error":{"details":[{"reason":"QUOTA_EXHAUSTED","metadata":{"quotaResetDelay":"72h"}}]}}"#;
        assert!(!manager
            .mark_rate_limited_fast(
                account_id,
                429,
                None,
                body,
                Some("gemini-3.1-flash-image"),
            )
            .await);
        assert!(manager
            .rate_limit_tracker
            .is_rate_limited(account_id, Some("gemini-3.1-flash-image")));
        let persisted: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&account_path).unwrap()).unwrap();
        let flash_limit = &persisted["live_limited_models"]["gemini-3.1-flash-image"];
        assert_eq!(flash_limit["status"], 429);
        assert!(
            flash_limit["until"].as_i64().unwrap() - chrono::Utc::now().timestamp() > 71 * 3600
        );

        manager.clear_persisted_live_limit(account_id, Some("gemini-3.1-flash-image-4k"));
        std::thread::scope(|scope| {
            for model in ["gemini-3.1-flash-image", "gemini-3-pro-image"] {
                let manager = &manager;
                scope.spawn(move || {
                    manager.record_rate_limit_atomic(
                        account_id,
                        429,
                        None,
                        body,
                        Some(model),
                        &[60, 300],
                        TrackerParserMode::Current,
                    );
                });
            }
        });

        let persisted: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&account_path).unwrap()).unwrap();
        let limits = persisted["live_limited_models"].as_object().unwrap();
        assert!(limits.contains_key("gemini-3.1-flash-image"));
        assert!(limits.contains_key("gemini-3-pro-image"));

        manager.clear_persisted_live_limit(account_id, Some("gemini-3.1-flash-image-4k"));
        let persisted: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&account_path).unwrap()).unwrap();
        let limits = persisted["live_limited_models"].as_object().unwrap();
        assert!(!limits.contains_key("gemini-3.1-flash-image"));
        assert!(limits.contains_key("gemini-3-pro-image"));

        std::thread::scope(|scope| {
            scope.spawn(|| {
                manager.record_rate_limit_atomic(
                    account_id,
                    429,
                    None,
                    body,
                    Some("gemini-3-pro-image"),
                    &[60, 300],
                    TrackerParserMode::Current,
                );
            });
            scope.spawn(|| {
                manager.clear_persisted_live_limit(account_id, Some("gemini-3-pro-image"));
            });
        });
        let persisted: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&account_path).unwrap()).unwrap();
        let disk_has_limit = persisted["live_limited_models"]
            .as_object()
            .unwrap()
            .contains_key("gemini-3-pro-image");
        assert_eq!(
            disk_has_limit,
            manager
                .rate_limit_tracker
                .is_rate_limited(account_id, Some("gemini-3-pro-image"))
        );

        let _ = std::fs::remove_dir_all(&tmp_root);
    }

    #[tokio::test]
    async fn test_fixed_account_mode_skips_preferred_when_disabled_on_disk_without_reload() {
        let tmp_root = std::env::temp_dir().join(format!(
            "antigravity-token-manager-test-fixed-mode-{}",
            uuid::Uuid::new_v4()
        ));
        let accounts_dir = tmp_root.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();

        let now = chrono::Utc::now().timestamp();

        let write_account = |id: &str, email: &str, proxy_disabled: bool| {
            let account_path = accounts_dir.join(format!("{}.json", id));
            let json = serde_json::json!({
                "id": id,
                "email": email,
                "token": {
                    "access_token": format!("atk-{}", id),
                    "refresh_token": format!("rtk-{}", id),
                    "expires_in": 3600,
                    "expiry_timestamp": now + 3600,
                    "project_id": format!("pid-{}", id)
                },
                "quota": {
                    "models": [
                        { "name": "gemini-1.5-flash", "percentage": 100 }
                    ]
                },
                "disabled": false,
                "proxy_disabled": proxy_disabled,
                "proxy_disabled_reason": if proxy_disabled { "manual" } else { "" },
                "quota": {
                    "models": [
                        { "name": "gemini-3-flash", "percentage": 100 }
                    ]
                },
                "created_at": now,
                "last_used": now
            });
            std::fs::write(&account_path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
        };

        // Two accounts in pool.
        write_account("acc1", "a@test.com", false);
        write_account("acc2", "b@test.com", false);

        let manager = TokenManager::new(tmp_root.clone());
        manager.load_accounts().await.unwrap();

        // Enable fixed account mode for acc1.
        manager
            .set_preferred_account(Some("acc1".to_string()))
            .await;

        // Disable acc1 on disk WITHOUT reloading the in-memory pool (simulates stale cache).
        write_account("acc1", "a@test.com", true);

        let (_token, _project_id, email, account_id, _wait_ms) = manager
            .get_token("gemini", false, Some("sid1"), "gemini-1.5-flash")
            .await
            .unwrap();

        // Should fall back to another account instead of using the disabled preferred one.
        assert_eq!(account_id, "acc2");
        assert_eq!(email, "b@test.com");
        assert!(manager.tokens.get("acc1").is_none());
        assert!(manager.get_preferred_account().await.is_none());

        let _ = std::fs::remove_dir_all(&tmp_root);
    }

    #[tokio::test]
    async fn test_collected_models_preserve_raw_quota_model_names_for_model_listing() {
        let tmp_root = std::env::temp_dir().join(format!(
            "antigravity-token-manager-test-raw-models-{}",
            uuid::Uuid::new_v4()
        ));
        let accounts_dir = tmp_root.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();

        let now = chrono::Utc::now().timestamp();
        let account_path = accounts_dir.join("acc1.json");
        let account_json = serde_json::json!({
            "id": "acc1",
            "email": "a@test.com",
            "token": {
                "access_token": "atk",
                "refresh_token": "rtk",
                "expires_in": 3600,
                "expiry_timestamp": now + 3600
            },
            "quota": {
                "models": [
                    { "name": "gemini-3-flash-agent", "percentage": 88 }
                ]
            },
            "disabled": false,
            "proxy_disabled": false,
            "created_at": now,
            "last_used": now
        });
        std::fs::write(
            &account_path,
            serde_json::to_string_pretty(&account_json).unwrap(),
        )
        .unwrap();

        let manager = TokenManager::new(tmp_root.clone());
        manager.load_accounts().await.unwrap();

        let collected_models = manager.get_all_collected_models();
        assert!(collected_models.contains("gemini-3-flash-agent"));
        // Keep the normalized quota bucket too; it is used for quota/protection checks.
        assert!(collected_models.contains("gemini-3-flash"));

        let _ = std::fs::remove_dir_all(&tmp_root);
    }

    #[tokio::test]
    async fn test_sticky_session_skips_bound_account_when_disabled_on_disk_without_reload() {
        let tmp_root = std::env::temp_dir().join(format!(
            "antigravity-token-manager-test-sticky-disabled-{}",
            uuid::Uuid::new_v4()
        ));
        let accounts_dir = tmp_root.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();

        let now = chrono::Utc::now().timestamp();

        let write_account = |id: &str, email: &str, percentage: i64, proxy_disabled: bool| {
            let account_path = accounts_dir.join(format!("{}.json", id));
            let json = serde_json::json!({
                "id": id,
                "email": email,
                "token": {
                    "access_token": format!("atk-{}", id),
                    "refresh_token": format!("rtk-{}", id),
                    "expires_in": 3600,
                    "expiry_timestamp": now + 3600,
                    "project_id": format!("pid-{}", id)
                },
                "quota": {
                    "models": [
                        { "name": "gemini-1.5-flash", "percentage": percentage }
                    ]
                },
                "disabled": false,
                "proxy_disabled": proxy_disabled,
                "proxy_disabled_reason": if proxy_disabled { "manual" } else { "" },
                "created_at": now,
                "last_used": now
            });
            std::fs::write(&account_path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
        };

        // Two accounts in pool. acc1 has higher quota -> should be selected and bound first.
        write_account("acc1", "a@test.com", 90, false);
        write_account("acc2", "b@test.com", 10, false);

        let manager = TokenManager::new(tmp_root.clone());
        manager.load_accounts().await.unwrap();

        // Prime: first request should bind the session to acc1.
        let (_token, _project_id, _email, account_id, _wait_ms) = manager
            .get_token("gemini", false, Some("sid1"), "gemini-1.5-flash")
            .await
            .unwrap();
        assert_eq!(account_id, "acc1");
        assert_eq!(
            manager.session_accounts.get("sid1").map(|v| v.clone()),
            Some("acc1".to_string())
        );

        // Disable acc1 on disk WITHOUT reloading the in-memory pool (simulates stale cache).
        write_account("acc1", "a@test.com", 90, true);

        let (_token, _project_id, email, account_id, _wait_ms) = manager
            .get_token("gemini", false, Some("sid1"), "gemini-1.5-flash")
            .await
            .unwrap();

        // Should fall back to another account instead of reusing the disabled bound one.
        assert_eq!(account_id, "acc2");
        assert_eq!(email, "b@test.com");
        assert!(manager.tokens.get("acc1").is_none());
        assert_ne!(
            manager.session_accounts.get("sid1").map(|v| v.clone()),
            Some("acc1".to_string())
        );

        let _ = std::fs::remove_dir_all(&tmp_root);
    }

    /// 创建测试用的 ProxyToken
    fn create_test_token(
        email: &str,
        tier: Option<&str>,
        health_score: f32,
        reset_time: Option<i64>,
        remaining_quota: Option<i32>,
    ) -> ProxyToken {
        ProxyToken {
            account_id: email.to_string(),
            priority: crate::models::account::default_priority(),
            access_token: "test_token".to_string(),
            refresh_token: "test_refresh".to_string(),
            expires_in: 3600,
            timestamp: chrono::Utc::now().timestamp() + 3600,
            email: email.to_string(),
            account_path: PathBuf::from("/tmp/test"),
            project_id: None,
            subscription_tier: tier.map(|s| s.to_string()),
            remaining_quota,
            protected_models: HashSet::new(),
            health_score,
            reset_time,
            weekly_reset_time: None,
            validation_blocked: false,
            validation_blocked_until: 0,
            validation_url: None,
            model_quotas: HashMap::new(),
            model_limits: HashMap::new(),
            available_models: std::collections::HashSet::new(),
            model_metadata: HashMap::new(),
        }
    }

    /// 测试排序比较函数（与 get_token_internal 中的逻辑一致）
    fn compare_tokens(a: &ProxyToken, b: &ProxyToken) -> Ordering {
        // 统一走 models::quota::tier_priority（与生产排序逻辑共用同一实现）
        let tier_priority =
            |tier: &Option<String>| crate::models::quota::tier_priority(tier.as_deref());

        // First: compare by subscription tier
        let tier_cmp =
            tier_priority(&a.subscription_tier).cmp(&tier_priority(&b.subscription_tier));
        if tier_cmp != Ordering::Equal {
            return tier_cmp;
        }

        // Second: compare by health score (higher is better)
        let health_cmp = b
            .health_score
            .partial_cmp(&a.health_score)
            .unwrap_or(Ordering::Equal);
        if health_cmp != Ordering::Equal {
            return health_cmp;
        }

        // Third: compare by reset time (earlier/closer is better) [Fix #3570]
        let reset_a = a.reset_time.unwrap_or(i64::MAX);
        let reset_b = b.reset_time.unwrap_or(i64::MAX);
        let reset_cmp = reset_a.cmp(&reset_b);
        if reset_cmp != Ordering::Equal {
            return reset_cmp;
        }

        // Fourth: compare by remaining quota percentage (higher is better)
        let quota_a = a.remaining_quota.unwrap_or(0);
        let quota_b = b.remaining_quota.unwrap_or(0);
        let quota_cmp = quota_b.cmp(&quota_a);
        if quota_cmp != Ordering::Equal {
            return quota_cmp;
        }

        // Tie-breaker
        a.account_id.cmp(&b.account_id)
    }

    #[test]
    fn test_sorting_tier_priority() {
        // ULTRA > PRO > FREE
        let ultra = create_test_token("ultra@test.com", Some("ULTRA"), 1.0, None, Some(50));
        let pro = create_test_token("pro@test.com", Some("PRO"), 1.0, None, Some(50));
        let premium = create_test_token(
            "premium@test.com",
            Some("Google One AI Premium"),
            1.0,
            None,
            Some(50),
        );
        let advanced = create_test_token(
            "advanced@test.com",
            Some("Gemini Advanced"),
            1.0,
            None,
            Some(50),
        );
        let free = create_test_token("free@test.com", Some("FREE"), 1.0, None, Some(50));

        assert_eq!(compare_tokens(&ultra, &pro), Ordering::Less);
        assert_eq!(compare_tokens(&ultra, &premium), Ordering::Less);
        assert_eq!(compare_tokens(&pro, &free), Ordering::Less);
        assert_eq!(compare_tokens(&premium, &free), Ordering::Less);
        assert_eq!(compare_tokens(&advanced, &free), Ordering::Less);
        assert_eq!(compare_tokens(&ultra, &free), Ordering::Less);
        assert_eq!(compare_tokens(&free, &ultra), Ordering::Greater);
    }

    #[test]
    fn test_sorting_health_score_priority() {
        // 同等级下，健康分高的优先
        let high_health = create_test_token("high@test.com", Some("PRO"), 1.0, None, Some(50));
        let low_health = create_test_token("low@test.com", Some("PRO"), 0.5, None, Some(50));

        assert_eq!(compare_tokens(&high_health, &low_health), Ordering::Less);
        assert_eq!(compare_tokens(&low_health, &high_health), Ordering::Greater);
    }

    #[test]
    fn test_sorting_reset_time_priority() {
        let now = chrono::Utc::now().timestamp();

        // 刷新时间更近（30分钟后）的优先于更远（5小时后）的
        let soon_reset = create_test_token(
            "soon@test.com",
            Some("PRO"),
            1.0,
            Some(now + 1800),
            Some(50),
        ); // 30分钟后
        let late_reset = create_test_token(
            "late@test.com",
            Some("PRO"),
            1.0,
            Some(now + 18000),
            Some(50),
        ); // 5小时后

        assert_eq!(compare_tokens(&soon_reset, &late_reset), Ordering::Less);
        assert_eq!(compare_tokens(&late_reset, &soon_reset), Ordering::Greater);
    }

    /// [Fix #3570] 验证重置时间早的优先，且相同重置时间时按配额排序
    #[test]
    fn test_sorting_reset_time_direct_comparison() {
        let now = chrono::Utc::now().timestamp();

        // 刷新时间较早的账号优先于刷新时间较晚的账号
        let reset_a = create_test_token("a@test.com", Some("PRO"), 1.0, Some(now + 1800), Some(20)); // 30分钟后, 20%配额
        let reset_b = create_test_token("b@test.com", Some("PRO"), 1.0, Some(now + 2100), Some(80)); // 35分钟后, 80%配额

        // 即使 a 配额低，但重置时间更早，所以 a 优先于 b
        assert_eq!(compare_tokens(&reset_a, &reset_b), Ordering::Less);
        assert_eq!(compare_tokens(&reset_b, &reset_a), Ordering::Greater);

        // 当重置时间完全相同时，按配额降序排序（高配额优先）
        let reset_c = create_test_token("c@test.com", Some("PRO"), 1.0, Some(now + 1800), Some(90)); // 30分钟后, 90%配额
        assert_eq!(compare_tokens(&reset_c, &reset_a), Ordering::Less);
    }

    /// [Fix #3570] 验证重置时间排序满足严格全序（Total Order），防止 Rust driftsort/smallsort panic
    /// 典型构造反例：A (T), B (T+300), C (T+600)
    /// 旧逻辑中 |A-B|<600 => Equal, |B-C|<600 => Equal, 但 |A-C|>=600 => Less，违反传递性导致 panic
    #[test]
    fn test_sorting_transitivity_no_panic_issue_3570() {
        let now = chrono::Utc::now().timestamp();
        let token_a = create_test_token("a@test.com", Some("FREE"), 1.0, Some(now), Some(100));
        let token_b =
            create_test_token("b@test.com", Some("FREE"), 1.0, Some(now + 300), Some(100));
        let token_c =
            create_test_token("c@test.com", Some("FREE"), 1.0, Some(now + 600), Some(100));

        let cmp_ab = compare_tokens(&token_a, &token_b);
        let cmp_bc = compare_tokens(&token_b, &token_c);
        let cmp_ac = compare_tokens(&token_a, &token_c);

        // 传递性要求：若 a < b 且 b < c，则必须 a < c
        assert_eq!(cmp_ab, Ordering::Less);
        assert_eq!(cmp_bc, Ordering::Less);
        assert_eq!(cmp_ac, Ordering::Less);

        // 反对称性要求：cmp(b, a) == cmp(a, b).reverse()
        assert_eq!(compare_tokens(&token_b, &token_a), Ordering::Greater);
        assert_eq!(compare_tokens(&token_c, &token_b), Ordering::Greater);
        assert_eq!(compare_tokens(&token_c, &token_a), Ordering::Greater);
    }

    /// [Fix #3570] 模拟 Issue 报告中的真实场景：200+ 个高密度账号池排序测试，绝不触发 panic
    #[test]
    fn test_sorting_large_account_pool_dense_reset_times_issue_3570() {
        let now = chrono::Utc::now().timestamp();
        let mut tokens: Vec<ProxyToken> = (0..205)
            .map(|i| {
                // 每隔 60 秒一个 reset_time，密集分布在 0 ~ 204*60 秒
                let reset_offset = (i as i64) * 60;
                create_test_token(
                    &format!("user_{:03}@test.com", i),
                    Some("FREE"),
                    1.0,
                    Some(now + reset_offset),
                    Some(100),
                )
            })
            .collect();

        // 逆序排列模拟无序输入
        tokens.reverse();

        // 执行排序（旧逻辑在 driftsort/smallsort 遇到密集 reset_time 必定 panic）
        tokens.sort_by(compare_tokens);

        // 验证排序结果正确性：reset_time 必须严格递增
        for window in tokens.windows(2) {
            let t1 = window[0].reset_time.unwrap();
            let t2 = window[1].reset_time.unwrap();
            assert!(t1 <= t2, "Tokens must be sorted by reset_time ascending");
        }
    }

    #[test]
    fn test_sorting_quota_fallback() {
        // 其他条件相同时，配额高的优先
        let high_quota = create_test_token("high@test.com", Some("PRO"), 1.0, None, Some(80));
        let low_quota = create_test_token("low@test.com", Some("PRO"), 1.0, None, Some(20));

        assert_eq!(compare_tokens(&high_quota, &low_quota), Ordering::Less);
        assert_eq!(compare_tokens(&low_quota, &high_quota), Ordering::Greater);
    }

    #[test]
    fn test_sorting_missing_reset_time() {
        let now = chrono::Utc::now().timestamp();

        // 没有 reset_time 的账号应该排在有 reset_time 的后面
        let with_reset = create_test_token(
            "with@test.com",
            Some("PRO"),
            1.0,
            Some(now + 1800),
            Some(50),
        );
        let without_reset = create_test_token("without@test.com", Some("PRO"), 1.0, None, Some(50));

        assert_eq!(compare_tokens(&with_reset, &without_reset), Ordering::Less);
    }

    #[test]
    fn test_full_sorting_integration() {
        let now = chrono::Utc::now().timestamp();

        let mut tokens = vec![
            create_test_token(
                "free_high@test.com",
                Some("FREE"),
                1.0,
                Some(now + 1800),
                Some(90),
            ),
            create_test_token(
                "pro_low_health@test.com",
                Some("PRO"),
                0.5,
                Some(now + 1800),
                Some(90),
            ),
            create_test_token(
                "pro_soon@test.com",
                Some("PRO"),
                1.0,
                Some(now + 1800),
                Some(50),
            ), // 30分钟后
            create_test_token(
                "pro_late@test.com",
                Some("PRO"),
                1.0,
                Some(now + 18000),
                Some(90),
            ), // 5小时后
            create_test_token(
                "ultra@test.com",
                Some("ULTRA"),
                1.0,
                Some(now + 36000),
                Some(10),
            ),
        ];

        tokens.sort_by(compare_tokens);

        // 预期顺序:
        // 1. ULTRA (最高等级，即使刷新时间最远)
        // 2. PRO + 高健康分 + 30分钟后刷新
        // 3. PRO + 高健康分 + 5小时后刷新
        // 4. PRO + 低健康分
        // 5. FREE (最低等级，即使配额最高)
        assert_eq!(tokens[0].email, "ultra@test.com");
        assert_eq!(tokens[1].email, "pro_soon@test.com");
        assert_eq!(tokens[2].email, "pro_late@test.com");
        assert_eq!(tokens[3].email, "pro_low_health@test.com");
        assert_eq!(tokens[4].email, "free_high@test.com");
    }

    #[test]
    fn test_realistic_scenario() {
        // 模拟用户描述的场景:
        // a 账号 claude 4h55m 后刷新
        // b 账号 claude 31m 后刷新
        // 应该优先使用 b（31分钟后刷新）
        let now = chrono::Utc::now().timestamp();

        let account_a = create_test_token(
            "a@test.com",
            Some("PRO"),
            1.0,
            Some(now + 295 * 60),
            Some(80),
        ); // 4h55m
        let account_b = create_test_token(
            "b@test.com",
            Some("PRO"),
            1.0,
            Some(now + 31 * 60),
            Some(30),
        ); // 31m

        // b 应该排在 a 前面（刷新时间更近）
        assert_eq!(compare_tokens(&account_b, &account_a), Ordering::Less);

        let mut tokens = vec![account_a.clone(), account_b.clone()];
        tokens.sort_by(compare_tokens);

        assert_eq!(tokens[0].email, "b@test.com");
        assert_eq!(tokens[1].email, "a@test.com");
    }

    #[test]
    fn test_extract_earliest_reset_time() {
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        // 测试包含 claude 模型的 reset_time 提取
        let account_with_claude = serde_json::json!({
            "quota": {
                "models": [
                    {"name": "gemini-flash", "reset_time": "2025-01-31T10:00:00Z"},
                    {"name": "claude-sonnet", "reset_time": "2025-01-31T08:00:00Z"},
                    {"name": "claude-opus", "reset_time": "2025-01-31T08:00:00Z"}
                ]
            }
        });

        let result = manager.extract_earliest_reset_time(&account_with_claude);
        assert!(result.is_some());
        // 应该返回 claude 的时间（08:00）而不是 gemini 的（10:00）
        let expected_ts = chrono::DateTime::parse_from_rfc3339("2025-01-31T08:00:00Z")
            .unwrap()
            .timestamp();
        assert_eq!(result.unwrap(), expected_ts);
    }

    #[test]
    fn test_extract_reset_time_no_claude() {
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        // 没有 claude 模型时，应该取任意模型的最近时间
        let account_no_claude = serde_json::json!({
            "quota": {
                "models": [
                    {"name": "gemini-flash", "reset_time": "2025-01-31T10:00:00Z"},
                    {"name": "gemini-pro", "reset_time": "2025-01-31T08:00:00Z"}
                ]
            }
        });

        let result = manager.extract_earliest_reset_time(&account_no_claude);
        assert!(result.is_some());
        let expected_ts = chrono::DateTime::parse_from_rfc3339("2025-01-31T08:00:00Z")
            .unwrap()
            .timestamp();
        assert_eq!(result.unwrap(), expected_ts);
    }

    #[test]
    fn test_extract_reset_time_missing_quota() {
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        // 没有 quota 字段时应返回 None
        let account_no_quota = serde_json::json!({
            "email": "test@test.com"
        });

        assert!(manager
            .extract_earliest_reset_time(&account_no_quota)
            .is_none());
    }

    // ===== P2C 算法测试 =====

    /// 创建带 protected_models 的测试 Token
    fn create_test_token_with_protected(
        email: &str,
        remaining_quota: Option<i32>,
        protected_models: HashSet<String>,
    ) -> ProxyToken {
        ProxyToken {
            account_id: email.to_string(),
            priority: crate::models::account::default_priority(),
            access_token: "test_token".to_string(),
            refresh_token: "test_refresh".to_string(),
            expires_in: 3600,
            timestamp: chrono::Utc::now().timestamp() + 3600,
            email: email.to_string(),
            account_path: PathBuf::from("/tmp/test"),
            project_id: None,
            subscription_tier: Some("PRO".to_string()),
            remaining_quota,
            protected_models,
            health_score: 1.0,
            reset_time: None,
            weekly_reset_time: None,
            validation_blocked: false,
            validation_blocked_until: 0,
            validation_url: None,
            model_quotas: HashMap::new(),
            model_limits: HashMap::new(),
            available_models: std::collections::HashSet::new(),
            model_metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_p2c_selects_higher_quota() {
        // P2C 应选择配额更高的账号
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        let low_quota = create_test_token("low@test.com", Some("PRO"), 1.0, None, Some(20));
        let high_quota = create_test_token("high@test.com", Some("PRO"), 1.0, None, Some(80));

        let candidates = vec![low_quota, high_quota];
        let attempted: HashSet<String> = HashSet::new();

        // 运行多次确保选择高配额账号
        for _ in 0..10 {
            let result = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet", false);
            assert!(result.is_some());
            // P2C 从两个候选中选择配额更高的
            // 由于只有两个候选，应该总是选择 high_quota
            assert_eq!(result.unwrap().email, "high@test.com");
        }
    }

    #[test]
    fn test_p2c_depleted_imminent_account_does_not_beat_healthy_account() {
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));
        let now_ts = chrono::Utc::now().timestamp();

        let mut depleted = create_test_token("depleted@test.com", Some("PRO"), 1.0, None, Some(2));
        depleted.weekly_reset_time = Some(now_ts + 3600); // 1 小时后重置

        let mut healthy = create_test_token("healthy@test.com", Some("PRO"), 1.0, None, Some(80));
        healthy.weekly_reset_time = Some(now_ts + 432_000); // 5 天后重置

        let candidates = vec![depleted, healthy];
        let attempted: HashSet<String> = HashSet::new();

        for _ in 0..10 {
            let result = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet", false);
            assert!(result.is_some());
            assert_eq!(
                result.unwrap().email,
                "healthy@test.com",
                "濒危配额(<=10%)即使临期重置也绝不得在 P2C 中胜出健康账号"
            );
        }
    }

    #[test]
    fn account_priority_p2c_stays_in_highest_available_group() {
        let manager = TokenManager::new(PathBuf::new());
        let mut high = create_test_token("high", Some("FREE"), 0.5, None, Some(1));
        high.priority = 1;
        let low = create_test_token("low", Some("ULTRA"), 1.0, None, Some(100));
        let mut candidates = vec![low, high]; // Deliberately unsorted.
        let mut attempted = HashSet::new();
        for _ in 0..20 {
            let selected = manager
                .select_with_p2c(&candidates, &attempted, "claude", true)
                .unwrap();
            assert_eq!(selected.account_id, "high");
        }
        attempted.insert("high".to_string());
        let selected = manager
            .select_with_p2c(&candidates, &attempted, "claude", true)
            .unwrap();
        assert_eq!(selected.account_id, "low");
        attempted.clear();
        candidates[1].protected_models.insert("claude".to_string());
        let selected = manager
            .select_with_p2c(&candidates, &attempted, "claude", true)
            .unwrap();
        assert_eq!(selected.account_id, "low");
    }

    #[tokio::test]
    async fn account_priority_save_reselects_preserving_sessions_and_limits() {
        async fn select(manager: &TokenManager, group: &str, session: Option<&str>) -> String {
            manager
                .get_token(group, false, session, "claude-sonnet-4-6")
                .await
                .unwrap()
                .3
        }
        let _dir = crate::proxy::monitor::prompt_log_tests::TestDataDir::new();
        let data_dir = crate::modules::account::get_data_dir().unwrap();
        let manager = TokenManager::new(data_dir.clone());
        for (id, priority) in [("high", 1), ("low", 100)] {
            let mut account = weekly_quota_account(chrono::Utc::now().timestamp());
            account["id"] = serde_json::json!(id);
            account["email"] = serde_json::json!(format!("{id}@test.invalid"));
            account["priority"] = serde_json::json!(priority);
            let account: crate::models::Account = serde_json::from_value(account).unwrap();
            crate::modules::account::save_account(&account).unwrap();
            manager.reload_account(id).await.unwrap();
        }
        assert_eq!(select(&manager, "claude", Some("existing")).await, "high");
        crate::modules::account::update_account_priority("high", 100).unwrap();
        manager.update_account_priority("high", 100);
        crate::modules::account::update_account_priority("low", 1).unwrap();
        manager.update_account_priority("low", 1);
        assert_eq!(
            crate::modules::account::load_account("low")
                .unwrap()
                .priority,
            1
        );
        assert_eq!(select(&manager, "claude", Some("existing")).await, "high");
        assert_eq!(select(&manager, "claude", Some("new")).await, "low");
        manager
            .set_preferred_account(Some("high".to_string()))
            .await;
        assert_eq!(select(&manager, "image_gen", None).await, "high");
        manager.set_preferred_account(None).await;
        manager.rate_limit_tracker.set_lockout_until(
            "low",
            std::time::SystemTime::now() + Duration::from_secs(60),
            crate::proxy::rate_limit::RateLimitReason::QuotaExhausted,
            Some("claude".to_string()),
        );
        manager.update_account_priority("low", 2);
        assert!(manager
            .rate_limit_tracker
            .is_rate_limited("low", Some("claude")));
        assert_eq!(select(&manager, "image_gen", None).await, "high");
        manager.rate_limit_tracker.clear("low");
        // A stale/failed disk candidate must not remove the lower-priority fallback.
        std::fs::write(data_dir.join("accounts/low.json"), "invalid JSON").unwrap();
        assert_eq!(select(&manager, "image_gen", None).await, "high");
    }

    #[test]
    fn test_p2c_skips_attempted() {
        // P2C 应跳过已尝试的账号
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        let token_a = create_test_token("a@test.com", Some("PRO"), 1.0, None, Some(80));
        let token_b = create_test_token("b@test.com", Some("PRO"), 1.0, None, Some(50));

        let candidates = vec![token_a, token_b];
        let mut attempted: HashSet<String> = HashSet::new();
        attempted.insert("a@test.com".to_string());

        let result = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet", false);
        assert!(result.is_some());
        assert_eq!(result.unwrap().email, "b@test.com");
    }

    #[test]
    fn test_p2c_skips_protected_models() {
        // P2C 应跳过对目标模型有保护的账号 (quota_protection_enabled = true)
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        let mut protected = HashSet::new();
        protected.insert("claude-sonnet".to_string());

        let protected_account =
            create_test_token_with_protected("protected@test.com", Some(90), protected);
        let normal_account =
            create_test_token_with_protected("normal@test.com", Some(50), HashSet::new());

        let candidates = vec![protected_account, normal_account];
        let attempted: HashSet<String> = HashSet::new();

        let result = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet", true);
        assert!(result.is_some());
        assert_eq!(result.unwrap().email, "normal@test.com");
    }

    #[test]
    fn test_p2c_single_candidate() {
        // 单候选时直接返回
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        let token = create_test_token("single@test.com", Some("PRO"), 1.0, None, Some(50));
        let candidates = vec![token];
        let attempted: HashSet<String> = HashSet::new();

        let result = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet", false);
        assert!(result.is_some());
        assert_eq!(result.unwrap().email, "single@test.com");
    }

    #[test]
    fn test_p2c_empty_candidates() {
        // 空候选返回 None
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        let candidates: Vec<ProxyToken> = vec![];
        let attempted: HashSet<String> = HashSet::new();

        let result = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet", false);
        assert!(result.is_none());
    }

    #[test]
    fn test_p2c_all_attempted() {
        // 所有账号都已尝试时返回 None
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        let token_a = create_test_token("a@test.com", Some("PRO"), 1.0, None, Some(80));
        let token_b = create_test_token("b@test.com", Some("PRO"), 1.0, None, Some(50));

        let candidates = vec![token_a, token_b];
        let mut attempted: HashSet<String> = HashSet::new();
        attempted.insert("a@test.com".to_string());
        attempted.insert("b@test.com".to_string());

        let result = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet", false);
        assert!(result.is_none());
    }

    // ===== Ultra 优先逻辑测试 =====

    /// 测试 is_ultra_required_model 辅助函数
    #[test]
    fn test_is_ultra_required_model() {
        // 需要 Ultra 账号的高端模型
        const ULTRA_REQUIRED_MODELS: &[&str] = &["claude-opus-4-6", "claude-opus-4-5", "opus"];

        fn is_ultra_required_model(model: &str) -> bool {
            let lower = model.to_lowercase();
            ULTRA_REQUIRED_MODELS.iter().any(|m| lower.contains(m))
        }

        // 应该识别为高端模型
        assert!(is_ultra_required_model("claude-opus-4-6"));
        assert!(is_ultra_required_model("claude-opus-4-5"));
        assert!(is_ultra_required_model("Claude-Opus-4-6")); // 大小写不敏感
        assert!(is_ultra_required_model("CLAUDE-OPUS-4-5")); // 大小写不敏感
        assert!(is_ultra_required_model("opus")); // 通配匹配
        assert!(is_ultra_required_model("opus-4-6-latest"));
        assert!(is_ultra_required_model("models/claude-opus-4-6"));

        // 应该识别为普通模型
        assert!(!is_ultra_required_model("claude-sonnet-4-5"));
        assert!(!is_ultra_required_model("claude-sonnet"));
        assert!(!is_ultra_required_model("gemini-1.5-flash"));
        assert!(!is_ultra_required_model("gemini-2.0-pro"));
        assert!(!is_ultra_required_model("claude-haiku"));
    }

    /// 测试高端模型排序：Ultra 账号优先于 Pro 账号（即使 Pro 配额更高）
    #[test]
    fn test_ultra_priority_for_high_end_models() {
        // 模拟高端模型排序逻辑
        fn compare_tokens_for_model(
            a: &ProxyToken,
            b: &ProxyToken,
            target_model: &str,
        ) -> Ordering {
            const ULTRA_REQUIRED_MODELS: &[&str] = &["claude-opus-4-6", "claude-opus-4-5", "opus"];
            let requires_ultra = {
                let lower = target_model.to_lowercase();
                ULTRA_REQUIRED_MODELS.iter().any(|m| lower.contains(m))
            };

            // 直接复用生产实现，避免测试里另写一份「简化版关键词表」
            // （旧版只匹配 "pro"，漏掉 premium/advanced，导致测试通过但生产行为未经验证）
            let tier_priority =
                |tier: &Option<String>| crate::models::quota::tier_priority(tier.as_deref());

            // Priority 0: 高端模型时，订阅等级优先
            if requires_ultra {
                let tier_cmp =
                    tier_priority(&a.subscription_tier).cmp(&tier_priority(&b.subscription_tier));
                if tier_cmp != Ordering::Equal {
                    return tier_cmp;
                }
            }

            // Priority 1: Quota (higher is better)
            let quota_a = a.remaining_quota.unwrap_or(0);
            let quota_b = b.remaining_quota.unwrap_or(0);
            let quota_cmp = quota_b.cmp(&quota_a);
            if quota_cmp != Ordering::Equal {
                return quota_cmp;
            }

            // Priority 2: Health score
            let health_cmp = b
                .health_score
                .partial_cmp(&a.health_score)
                .unwrap_or(Ordering::Equal);
            if health_cmp != Ordering::Equal {
                return health_cmp;
            }

            // Priority 3: Tier (for non-high-end models)
            if !requires_ultra {
                let tier_cmp =
                    tier_priority(&a.subscription_tier).cmp(&tier_priority(&b.subscription_tier));
                if tier_cmp != Ordering::Equal {
                    return tier_cmp;
                }
            }

            Ordering::Equal
        }

        // 创建测试账号：Ultra 低配额 vs Pro 高配额
        let ultra_low_quota =
            create_test_token("ultra@test.com", Some("ULTRA"), 1.0, None, Some(20));
        let pro_high_quota = create_test_token("pro@test.com", Some("PRO"), 1.0, None, Some(80));

        // 高端模型 (Opus 4.6): Ultra 应该优先，即使配额低
        assert_eq!(
            compare_tokens_for_model(&ultra_low_quota, &pro_high_quota, "claude-opus-4-6"),
            Ordering::Less, // Ultra 排在前面
            "Opus 4.6 should prefer Ultra account over Pro even with lower quota"
        );

        // 高端模型 (Opus 4.5): Ultra 应该优先
        assert_eq!(
            compare_tokens_for_model(&ultra_low_quota, &pro_high_quota, "claude-opus-4-5"),
            Ordering::Less,
            "Opus 4.5 should prefer Ultra account over Pro"
        );

        // 普通模型 (Sonnet): 高配额 Pro 应该优先
        assert_eq!(
            compare_tokens_for_model(&ultra_low_quota, &pro_high_quota, "claude-sonnet-4-5"),
            Ordering::Greater, // Pro (高配额) 排在前面
            "Sonnet should prefer high-quota Pro over low-quota Ultra"
        );

        // 普通模型 (Flash): 高配额 Pro 应该优先
        assert_eq!(
            compare_tokens_for_model(&ultra_low_quota, &pro_high_quota, "gemini-1.5-flash"),
            Ordering::Greater,
            "Flash should prefer high-quota Pro over low-quota Ultra"
        );
    }

    /// 测试排序：同为 Ultra 时按配额排序
    #[test]
    fn test_ultra_accounts_sorted_by_quota() {
        fn compare_tokens_for_model(
            a: &ProxyToken,
            b: &ProxyToken,
            target_model: &str,
        ) -> Ordering {
            const ULTRA_REQUIRED_MODELS: &[&str] = &["claude-opus-4-6", "claude-opus-4-5", "opus"];
            let requires_ultra = {
                let lower = target_model.to_lowercase();
                ULTRA_REQUIRED_MODELS.iter().any(|m| lower.contains(m))
            };

            // 直接复用生产实现，避免测试里另写一份「简化版关键词表」
            // （旧版只匹配 "pro"，漏掉 premium/advanced，导致测试通过但生产行为未经验证）
            let tier_priority =
                |tier: &Option<String>| crate::models::quota::tier_priority(tier.as_deref());

            if requires_ultra {
                let tier_cmp =
                    tier_priority(&a.subscription_tier).cmp(&tier_priority(&b.subscription_tier));
                if tier_cmp != Ordering::Equal {
                    return tier_cmp;
                }
            }

            let quota_a = a.remaining_quota.unwrap_or(0);
            let quota_b = b.remaining_quota.unwrap_or(0);
            quota_b.cmp(&quota_a)
        }

        let ultra_high =
            create_test_token("ultra_high@test.com", Some("ULTRA"), 1.0, None, Some(80));
        let ultra_low = create_test_token("ultra_low@test.com", Some("ULTRA"), 1.0, None, Some(20));

        // Opus 4.6: 同为 Ultra，高配额优先
        assert_eq!(
            compare_tokens_for_model(&ultra_high, &ultra_low, "claude-opus-4-6"),
            Ordering::Less, // ultra_high 排在前面
            "Among Ultra accounts, higher quota should come first"
        );
    }

    /// 测试完整排序场景：混合账号池
    #[test]
    fn test_full_sorting_mixed_accounts() {
        fn sort_tokens_for_model(tokens: &mut Vec<ProxyToken>, target_model: &str) {
            const ULTRA_REQUIRED_MODELS: &[&str] = &["claude-opus-4-6", "claude-opus-4-5", "opus"];
            let requires_ultra = {
                let lower = target_model.to_lowercase();
                ULTRA_REQUIRED_MODELS.iter().any(|m| lower.contains(m))
            };

            tokens.sort_by(|a, b| {
                // 直接复用生产实现
                let tier_priority =
                    |tier: &Option<String>| crate::models::quota::tier_priority(tier.as_deref());

                if requires_ultra {
                    let tier_cmp = tier_priority(&a.subscription_tier)
                        .cmp(&tier_priority(&b.subscription_tier));
                    if tier_cmp != Ordering::Equal {
                        return tier_cmp;
                    }
                }

                let quota_a = a.remaining_quota.unwrap_or(0);
                let quota_b = b.remaining_quota.unwrap_or(0);
                let quota_cmp = quota_b.cmp(&quota_a);
                if quota_cmp != Ordering::Equal {
                    return quota_cmp;
                }

                if !requires_ultra {
                    let tier_cmp = tier_priority(&a.subscription_tier)
                        .cmp(&tier_priority(&b.subscription_tier));
                    if tier_cmp != Ordering::Equal {
                        return tier_cmp;
                    }
                }

                Ordering::Equal
            });
        }

        // 创建混合账号池
        let ultra_high =
            create_test_token("ultra_high@test.com", Some("ULTRA"), 1.0, None, Some(80));
        let ultra_low = create_test_token("ultra_low@test.com", Some("ULTRA"), 1.0, None, Some(20));
        let pro_high = create_test_token("pro_high@test.com", Some("PRO"), 1.0, None, Some(90));
        let pro_low = create_test_token("pro_low@test.com", Some("PRO"), 1.0, None, Some(30));
        let free = create_test_token("free@test.com", Some("FREE"), 1.0, None, Some(100));

        // 高端模型 (Opus 4.6) 排序
        let mut tokens_opus = vec![
            pro_high.clone(),
            free.clone(),
            ultra_low.clone(),
            pro_low.clone(),
            ultra_high.clone(),
        ];
        sort_tokens_for_model(&mut tokens_opus, "claude-opus-4-6");

        let emails_opus: Vec<&str> = tokens_opus.iter().map(|t| t.email.as_str()).collect();
        // 期望顺序: Ultra(高配额) > Ultra(低配额) > Pro(高配额) > Pro(低配额) > Free
        assert_eq!(
            emails_opus,
            vec![
                "ultra_high@test.com",
                "ultra_low@test.com",
                "pro_high@test.com",
                "pro_low@test.com",
                "free@test.com"
            ],
            "Opus 4.6 should sort Ultra first, then by quota within each tier"
        );

        // 普通模型 (Sonnet) 排序
        let mut tokens_sonnet = vec![
            pro_high.clone(),
            free.clone(),
            ultra_low.clone(),
            pro_low.clone(),
            ultra_high.clone(),
        ];
        sort_tokens_for_model(&mut tokens_sonnet, "claude-sonnet-4-5");

        let emails_sonnet: Vec<&str> = tokens_sonnet.iter().map(|t| t.email.as_str()).collect();
        // 期望顺序: Free(100%) > Pro(90%) > Ultra(80%) > Pro(30%) > Ultra(20%) - 按配额优先
        assert_eq!(
            emails_sonnet,
            vec![
                "free@test.com",
                "pro_high@test.com",
                "ultra_high@test.com",
                "pro_low@test.com",
                "ultra_low@test.com"
            ],
            "Sonnet should sort by quota first, then by tier as tiebreaker"
        );
    }

    #[test]
    fn test_sync_zero_quota_circuit_breaker_later_deadline_and_recovery() {
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));

        // 1. 周配额为 0，5H 配额为 0，两者均耗尽
        // 周配额 reset_time 为 5天后，5H reset_time 为 2小时后
        let now = chrono::Utc::now();
        let base_timestamp = now.timestamp();
        let reset_5h = (now + chrono::Duration::hours(2)).to_rfc3339();
        let reset_weekly = (now + chrono::Duration::days(5)).to_rfc3339();

        let account = serde_json::json!({
            "quota": {
                "last_updated": base_timestamp,
                "quota_groups": [
                    {
                        "display_name": "Claude & 3P Models",
                        "buckets": [
                            {
                                "bucket_id": "3p-5h",
                                "window": "5h",
                                "remaining_fraction": 0.0,
                                "reset_time": reset_5h
                            },
                            {
                                "bucket_id": "3p-weekly",
                                "window": "7d",
                                "remaining_fraction": 0.0,
                                "reset_time": reset_weekly
                            }
                        ]
                    }
                ]
            }
        });

        // 即使 lock_on_zero 为 false，周配额耗尽依然无条件锁定至周截止时间
        manager.sync_zero_quota_circuit_breaker("acc1", &account);
        assert!(manager
            .rate_limit_tracker
            .is_rate_limited("acc1", Some("claude-sonnet-4-6")));
        let wait = manager
            .rate_limit_tracker
            .get_remaining_wait("acc1", Some("claude-sonnet-4-6"));
        assert!(
            wait > 4 * 86400,
            "Should be locked for > 4 days due to weekly constraint"
        );

        // 2. 模拟 provider 提前重置：周配额恢复为 100%，5H 配额仍为 0
        // 若开启 lock_on_zero，应自动对齐到较短的 5H 截止时间 (2小时)
        {
            let mut cfg = manager.circuit_breaker_config.blocking_write();
            cfg.enabled = true;
            cfg.lock_on_zero_quota = true;
        }

        let account_recovered_weekly = serde_json::json!({
            "quota": {
                "last_updated": base_timestamp + 1,
                "quota_groups": [
                    {
                        "display_name": "Claude & 3P Models",
                        "buckets": [
                            {
                                "bucket_id": "3p-5h",
                                "window": "5h",
                                "remaining_fraction": 0.0,
                                "reset_time": reset_5h
                            },
                            {
                                "bucket_id": "3p-weekly",
                                "window": "7d",
                                "remaining_fraction": 1.0,
                                "reset_time": reset_weekly
                            }
                        ]
                    }
                ]
            }
        });

        manager.sync_zero_quota_circuit_breaker("acc1", &account_recovered_weekly);
        let wait_5h = manager
            .rate_limit_tracker
            .get_remaining_wait("acc1", Some("claude-sonnet-4-6"));
        assert!(
            wait_5h <= 2 * 3600 && wait_5h > 0,
            "Should reconcile to 5h deadline (<= 2h)"
        );

        // 3. 模拟 5H 也完全恢复 (全部配额为正)
        let account_fully_recovered = serde_json::json!({
            "quota": {
                "last_updated": base_timestamp + 2,
                "quota_groups": [
                    {
                        "display_name": "Claude & 3P Models",
                        "buckets": [
                            {
                                "bucket_id": "3p-5h",
                                "window": "5h",
                                "remaining_fraction": 1.0,
                                "reset_time": reset_5h
                            },
                            {
                                "bucket_id": "3p-weekly",
                                "window": "7d",
                                "remaining_fraction": 1.0,
                                "reset_time": reset_weekly
                            }
                        ]
                    }
                ]
            }
        });

        manager.sync_zero_quota_circuit_breaker("acc1", &account_fully_recovered);
        assert!(!manager
            .rate_limit_tracker
            .is_rate_limited("acc1", Some("claude-sonnet-4-6")));
    }

    #[tokio::test]
    async fn test_claude_5_5_dynamic_capability_filtering_and_priority() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        // 账号 1: PRO 订阅，尚未拉取到 5.5 模型配额 (model_limits 中仅有普通模型)
        let mut quotas1 = HashMap::new();
        quotas1.insert("claude".to_string(), 100);
        let mut limits1 = HashMap::new();
        limits1.insert("claude-sonnet-4-6".to_string(), 64000);
        let token1 = ProxyToken {
            account_id: "acc_pro_no_55".to_string(),
            access_token: "token1".to_string(),
            refresh_token: "refresh1".to_string(),
            expires_in: 3600,
            timestamp: chrono::Utc::now().timestamp() + 3600,
            email: "pro_no_55@example.com".to_string(),
            account_path: temp_dir.path().join("acc1.json"),
            project_id: None,
            subscription_tier: Some("PRO".to_string()),
            remaining_quota: Some(100),
            priority: 0,
            protected_models: HashSet::new(),
            health_score: 1.0,
            reset_time: None,
            weekly_reset_time: None,
            validation_blocked: false,
            validation_blocked_until: 0,
            validation_url: None,
            model_quotas: quotas1,
            model_limits: limits1,
            available_models: HashSet::new(),
            model_metadata: HashMap::new(),
        };

        // 账号 2: PRO 订阅，经定时刷新成功上报 Claude 5.5 模型配额
        let mut quotas2 = HashMap::new();
        quotas2.insert("claude".to_string(), 80);
        let mut limits2 = HashMap::new();
        limits2.insert("claude-sonnet-5-5-high".to_string(), 128000);
        let token2 = ProxyToken {
            account_id: "acc_pro_has_55".to_string(),
            access_token: "token2".to_string(),
            refresh_token: "refresh2".to_string(),
            expires_in: 3600,
            timestamp: chrono::Utc::now().timestamp() + 3600,
            email: "pro_has_55@example.com".to_string(),
            account_path: temp_dir.path().join("acc2.json"),
            project_id: None,
            subscription_tier: Some("PRO".to_string()),
            remaining_quota: Some(80),
            priority: 0,
            protected_models: HashSet::new(),
            health_score: 1.0,
            reset_time: None,
            weekly_reset_time: None,
            validation_blocked: false,
            validation_blocked_until: 0,
            validation_url: None,
            model_quotas: quotas2,
            model_limits: limits2,
            available_models: {
                let mut s = HashSet::new();
                s.insert("claude-sonnet-5-5-high".to_string());
                s
            },
            model_metadata: HashMap::new(),
        };

        // 验证辅助函数检测
        assert!(!TokenManager::token_supports_advanced_model(
            &token1,
            "claude-sonnet-5-5-high"
        ));
        assert!(TokenManager::token_supports_advanced_model(
            &token2,
            "claude-sonnet-5-5-high"
        ));
        // 对裸模型或带后缀模型具有模糊识别
        assert!(TokenManager::token_supports_advanced_model(
            &token2,
            "claude-sonnet-5-5"
        ));

        // 写入虚拟的 account json 文件以防磁盘检查失败
        std::fs::write(
            &token1.account_path,
            serde_json::json!({"disabled": false}).to_string(),
        )
        .unwrap();
        std::fs::write(
            &token2.account_path,
            serde_json::json!({"disabled": false}).to_string(),
        )
        .unwrap();

        let account_id_2 = token2.account_id.clone();

        // 插入账号池
        manager.tokens.insert(token1.account_id.clone(), token1);
        manager.tokens.insert(token2.account_id.clone(), token2);

        // 发起 Claude 5.5 请求：应收敛并优先调度具备实际能力的账号 2 (即使账号 1 剩余 quota 100% 更高)
        let (chosen_token, _, _, _, _) = manager
            .get_token("claude", false, None, "claude-sonnet-5-5-high")
            .await
            .unwrap();

        assert_eq!(
            chosen_token, "token2",
            "账号 2 具备 Claude 5.5 真实能力，应当突破普通配额排序被优先调度"
        );

        // [NEW 测试] 模拟账号 2 遭遇 404/403 并被标记单模型临时熔断
        manager.mark_model_unsupported(&account_id_2, "claude-sonnet-5-5-high", Some(900));
        assert!(manager.is_model_unsupported(&account_id_2, "claude-sonnet-5-5-high"));
        // 关键断言：该账号调用其他正常模型（如 claude-sonnet-4-6）绝对不受熔断影响！
        assert!(!manager.is_model_unsupported(&account_id_2, "claude-sonnet-4-6"));

        // 关键调度断言：账号 2 被临时熔断后，再次请求 5.5 必须精准避开账号 2 并回退至账号 1
        let (fallback_token, _, _, _, _) = manager
            .get_token("claude", false, None, "claude-sonnet-5-5-high")
            .await
            .unwrap();
        assert_eq!(
            fallback_token, "token1",
            "账号 2 发生单模型临时熔断后，调度器必须自动避开账号 2"
        );
    }

    #[tokio::test]
    async fn test_optimistic_reset_rescues_all_accounts_transiently_rate_limited() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        let mut quotas = HashMap::new();
        quotas.insert("gemini-3-pro-high".to_string(), 100);
        let mut limits = HashMap::new();
        limits.insert("gemini-3-pro-high".to_string(), 64000);

        let token1 = ProxyToken {
            account_id: "acc_transient_1".to_string(),
            access_token: "token1".to_string(),
            refresh_token: "refresh1".to_string(),
            expires_in: 3600,
            timestamp: chrono::Utc::now().timestamp() + 3600,
            email: "acc1@example.com".to_string(),
            account_path: temp_dir.path().join("acc1.json"),
            project_id: None,
            subscription_tier: Some("PRO".to_string()),
            remaining_quota: Some(100),
            priority: 0,
            protected_models: HashSet::new(),
            health_score: 1.0,
            reset_time: None,
            weekly_reset_time: None,
            validation_blocked: false,
            validation_blocked_until: 0,
            validation_url: None,
            model_quotas: quotas,
            model_limits: limits,
            available_models: HashSet::new(),
            model_metadata: HashMap::new(),
        };

        std::fs::write(
            &token1.account_path,
            serde_json::json!({"disabled": false}).to_string(),
        )
        .unwrap();

        manager.tokens.insert(token1.account_id.clone(), token1);

        // 模拟遭遇单次 429 导致账号被锁定 5 秒 (wait_sec = 5 > 2s)
        manager.rate_limit_tracker.parse_from_error(
            "acc_transient_1",
            429,
            None,
            r#"{"error":{"code":429,"message":"Resource has been exhausted (e.g. check quota)."}}"#,
            Some("gemini-3-pro-high".to_string()),
            &[60, 300],
        );

        assert!(manager
            .rate_limit_tracker
            .is_rate_limited("acc_transient_1", Some("gemini-3-pro-high")));
        let wait = manager
            .rate_limit_tracker
            .get_remaining_wait("acc_transient_1", Some("gemini-3-pro-high"));
        assert!(
            wait > 2,
            "Wait must be > 2s to test Layer 2 optimistic reset trigger"
        );

        // [Issue #3506 核心断言] 发起新请求时，TokenManager 检测到全池受限但仅为瞬态 RateLimitExceeded，
        // 必须成功触发乐观重置并解救账号，严禁直接抛出 503 "All accounts limited"
        let res = manager
            .get_token("gemini", false, None, "gemini-3.1-pro-high")
            .await;
        assert!(
            res.is_ok(),
            "Optimistic reset must rescue transiently rate limited account instead of returning 503 error"
        );
        let (token_str, _, _, _, _) = res.unwrap();
        assert_eq!(token_str, "token1");
    }

    #[tokio::test]
    async fn test_mark_account_success_resolves_email_and_resets_tracker() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        let token = ProxyToken {
            account_id: "acc_tracked_1".to_string(),
            access_token: "token1".to_string(),
            refresh_token: "refresh1".to_string(),
            expires_in: 3600,
            timestamp: chrono::Utc::now().timestamp() + 3600,
            email: "dev@example.com".to_string(),
            account_path: temp_dir.path().join("acc.json"),
            project_id: None,
            subscription_tier: Some("PRO".to_string()),
            remaining_quota: Some(100),
            priority: 0,
            protected_models: HashSet::new(),
            health_score: 1.0,
            reset_time: None,
            weekly_reset_time: None,
            validation_blocked: false,
            validation_blocked_until: 0,
            validation_url: None,
            model_quotas: HashMap::new(),
            model_limits: HashMap::new(),
            available_models: std::collections::HashSet::new(),
            model_metadata: HashMap::new(),
        };

        manager.tokens.insert(token.account_id.clone(), token);

        // 记录失败导致锁定与递增 failure_counts
        manager.rate_limit_tracker.parse_from_error(
            "acc_tracked_1",
            429,
            None,
            r#"{"error":{"code":429,"message":"Resource exhausted"}}"#,
            None,
            &[60, 300],
        );

        assert!(manager
            .rate_limit_tracker
            .is_rate_limited("acc_tracked_1", None));

        // 关键断言：即使上游调用者传入的是 email，mark_account_success 也必须能将其防御性解析为 account_id 并清除限制
        manager.mark_account_success("dev@example.com");

        assert!(
            !manager.rate_limit_tracker.is_rate_limited("acc_tracked_1", None),
            "Calling mark_account_success with email must resolve to account_id and clear tracker limit"
        );
    }

    #[test]
    fn test_sticky_session_failover_abandon_and_commit() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        let session_id = "test-session-123";
        let acc1 = "acc_failing";
        let acc2 = "acc_healthy";

        // 初始绑定到 acc1
        manager.commit_session(session_id, acc1);
        assert_eq!(
            manager.session_accounts.get(session_id).unwrap().as_str(),
            acc1
        );

        // CAS 放弃会话：如果不是 acc1，不能被误解绑
        let abandoned_wrong = manager.abandon_session(session_id, "different_acc");
        assert!(
            !abandoned_wrong,
            "CAS check must reject mismatching account"
        );
        assert_eq!(
            manager.session_accounts.get(session_id).unwrap().as_str(),
            acc1
        );

        // 匹配 acc1 时正常解除粘性
        let abandoned_correct = manager.abandon_session(session_id, acc1);
        assert!(
            abandoned_correct,
            "CAS check must succeed and unbind matching account"
        );
        assert!(manager.session_accounts.get(session_id).is_none());

        // 故障转移到 acc2 成功后 commit
        manager.commit_session(session_id, acc2);
        assert_eq!(
            manager.session_accounts.get(session_id).unwrap().as_str(),
            acc2
        );
    }

    #[test]
    fn test_extract_weekly_reset_time_from_quota_groups() {
        let manager = TokenManager::new(PathBuf::from("/tmp/test"));
        let account_json = serde_json::json!({
            "quota": {
                "quota_groups": [
                    {
                        "displayName": "Gemini",
                        "buckets": [
                            {
                                "bucketId": "gemini_5h",
                                "window": "5h",
                                "remainingFraction": 0.8,
                                "resetTime": "2026-10-11T16:00:00Z"
                            },
                            {
                                "bucketId": "gemini_weekly",
                                "window": "7d",
                                "remainingFraction": 0.95,
                                "resetTime": "2026-10-14T03:00:00Z"
                            }
                        ]
                    }
                ]
            }
        });

        let weekly_ts = manager.extract_weekly_reset_time(&account_json);
        assert!(
            weekly_ts.is_some(),
            "必须成功从 7d/weekly 桶中提取周配额刷新时间戳"
        );
        let dt = chrono::DateTime::parse_from_rfc3339("2026-10-14T03:00:00Z").unwrap();
        assert_eq!(weekly_ts.unwrap(), dt.timestamp());
    }

    #[test]
    fn test_get_quota_reset_time_isolates_unrelated_model_reset_issue_3627() {
        let temp_dir = tempfile::tempdir().unwrap();
        let accounts_dir = temp_dir.path().join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();

        let account_id = "acc_mixed_quota";
        let account_json = serde_json::json!({
            "id": account_id,
            "email": "mixed@example.com",
            "quota": {
                "models": [
                    {
                        "name": "claude-opus-4-6-thinking",
                        "percentage": 0,
                        "reset_time": "2026-10-13T20:45:00Z"
                    },
                    {
                        "name": "gemini-3.8-flash-high",
                        "percentage": 100,
                        "reset_time": ""
                    }
                ]
            }
        });

        std::fs::write(
            accounts_dir.join(format!("{}.json", account_id)),
            serde_json::to_string_pretty(&account_json).unwrap(),
        )
        .unwrap();

        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        // 1. 请求 gemini-3.8-flash-high 时，即便 Opus 耗尽且存在 50+ 小时重置时间，
        // 绝不得将 Opus 的 reset_time 盗用套在额度充足的 Gemini 模型上（根治 Issue #3627 误报 182805s）
        let gemini_reset = manager.get_quota_reset_time(account_id, Some("gemini-3.8-flash-high"));
        assert_eq!(
            gemini_reset, None,
            "额度充足的模型绝不得盗用其他模型的配额重置时间"
        );

        let locked = manager.set_precise_lockout(
            account_id,
            crate::proxy::rate_limit::RateLimitReason::QuotaExhausted,
            Some("gemini-3.8-flash-high".to_string()),
        );
        assert!(
            !locked,
            "额度充足的模型 set_precise_lockout 必须返回 false 以便走指数退避，严禁锁入数十小时"
        );

        // 2. 真正耗尽的模型（Opus 0%）必须能精准匹配到专属的 reset_time
        let opus_reset = manager.get_quota_reset_time(account_id, Some("claude-opus-4-6-thinking"));
        assert_eq!(
            opus_reset.as_deref(),
            Some("2026-10-13T20:45:00Z"),
            "真正耗尽的模型必须精准获得专属 reset_time"
        );

        // 3. 全账号未指定 model 时，由于 Gemini 依然有 100% 额度，绝不得连坐封锁全账号
        let global_reset = manager.get_quota_reset_time(account_id, None);
        assert_eq!(
            global_reset, None,
            "存在额度充足模型的账号绝不得触发全账号级长周期锁定"
        );
    }

    #[tokio::test]
    async fn test_sticky_session_auto_failover_when_bound_account_quota_hits_zero_issue_3639() {
        let temp_dir = tempfile::tempdir().unwrap();
        let accounts_dir = temp_dir.path().join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();

        let acc1_id = "acc_exhausted";
        let acc2_id = "acc_healthy";

        let acc1_path = accounts_dir.join(format!("{}.json", acc1_id));
        let acc2_path = accounts_dir.join(format!("{}.json", acc2_id));

        std::fs::write(&acc1_path, r#"{"disabled":false}"#).unwrap();
        std::fs::write(&acc2_path, r#"{"disabled":false}"#).unwrap();

        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        let target_model = "claude-sonnet-4-6";
        let norm_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());
        let mut quotas1 = HashMap::new();
        quotas1.insert(norm_target.clone(), 0); // 0% 额度

        let mut quotas2 = HashMap::new();
        quotas2.insert(norm_target, 100); // 100% 额度

        let mut token1 = create_test_token("acc1@test.com", Some("PRO"), 1.0, None, Some(0));
        token1.account_id = acc1_id.to_string();
        token1.account_path = acc1_path;
        token1.model_quotas = quotas1;

        let mut token2 = create_test_token("acc2@test.com", Some("PRO"), 1.0, None, Some(100));
        token2.account_id = acc2_id.to_string();
        token2.account_path = acc2_path;
        token2.model_quotas = quotas2;

        manager.tokens.insert(acc1_id.to_string(), token1);
        manager.tokens.insert(acc2_id.to_string(), token2);

        let session_id = "agent-coding-session-42";
        // 初始将会话绑定至 acc1
        manager.commit_session(session_id, acc1_id);
        assert_eq!(
            manager.session_accounts.get(session_id).unwrap().as_str(),
            acc1_id
        );

        // 获取 Token：因为 acc1 的 claude-sonnet-4-6 已为 0%，而号池存在 100% 的 acc2，
        // 必须无缝自动解绑 acc1，故障转移至 acc2，并自动将会话重新绑定至 acc2 (Fixes #3639)
        let excluded = HashSet::new();
        let (_token, _project_id, email, selected_id, _wait) = manager
            .get_token_filtered("chat", false, Some(session_id), target_model, &excluded)
            .await
            .expect("Token acquisition must seamlessly failover to healthy account");

        assert_eq!(selected_id, acc2_id, "必须自动选择具备正额度的账号 acc2");
        assert_eq!(email, "acc2@test.com");

        // 验证会话已被原子重新绑定到健康的 acc2
        assert_eq!(
            manager.session_accounts.get(session_id).unwrap().as_str(),
            acc2_id,
            "会话必须被自动 commit 绑定至新健康的账号 acc2"
        );
    }

    #[tokio::test]
    async fn test_preferred_account_auto_failover_when_zero_quota_issue_3639() {
        let temp_dir = tempfile::tempdir().unwrap();
        let accounts_dir = temp_dir.path().join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();

        let acc_pref_id = "acc_preferred_zero";
        let acc_alt_id = "acc_fallback_full";

        let pref_path = accounts_dir.join(format!("{}.json", acc_pref_id));
        let alt_path = accounts_dir.join(format!("{}.json", acc_alt_id));

        std::fs::write(&pref_path, r#"{"disabled":false}"#).unwrap();
        std::fs::write(&alt_path, r#"{"disabled":false}"#).unwrap();

        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        let target_model = "gemini-3.8-flash-high";
        let norm_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());
        let mut quotas1 = HashMap::new();
        quotas1.insert(norm_target.clone(), 0); // 优先账号为 0% 额度

        let mut quotas2 = HashMap::new();
        quotas2.insert(norm_target, 80); // 备选账号具备 80% 额度

        let mut token1 = create_test_token("pref@test.com", Some("PRO"), 1.0, None, Some(0));
        token1.account_id = acc_pref_id.to_string();
        token1.account_path = pref_path;
        token1.model_quotas = quotas1;

        let mut token2 = create_test_token("alt@test.com", Some("PRO"), 1.0, None, Some(80));
        token2.account_id = acc_alt_id.to_string();
        token2.account_path = alt_path;
        token2.model_quotas = quotas2;

        manager.tokens.insert(acc_pref_id.to_string(), token1);
        manager.tokens.insert(acc_alt_id.to_string(), token2);

        // 设置固定优先账号为 acc_pref_id
        manager
            .set_preferred_account(Some(acc_pref_id.to_string()))
            .await;

        // 获取 Token：优先账号目标模型为 0% 且存在具备 80% 的替代账号时，
        // 绝不得死磕 0% 账号导致 429，必须无感自动回退至池中可用账号 (Fixes #3639)
        let excluded = HashSet::new();
        let (_token, _project_id, email, selected_id, _wait) = manager
            .get_token_filtered("chat", false, None, target_model, &excluded)
            .await
            .expect(
                "Preferred account with 0% quota must seamlessly fallback to available account",
            );

        assert_eq!(selected_id, acc_alt_id, "必须自动选择具备额度的备选账号");
        assert_eq!(email, "alt@test.com");
    }

    #[tokio::test]
    async fn test_edf_sorting_prioritizes_expiring_weekly_account_over_higher_quota_distant_reset()
    {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());
        let now_ts = chrono::Utc::now().timestamp();

        let target_model = "gemini-3.8-flash-high";
        let norm_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());

        // 账号 A: 40% 配额，周配额在 2 小时后重置 (临期，不尽快消耗该配额就会被 Google 作废清零)
        let mut quotas_a = HashMap::new();
        quotas_a.insert(norm_target.clone(), 40);
        let mut token_a = create_test_token("expiring@test.com", Some("PRO"), 1.0, None, Some(40));
        token_a.account_id = "acc_expiring".to_string();
        token_a.model_quotas = quotas_a;
        token_a.weekly_reset_time = Some(now_ts + 7200); // 2 小时后

        // 账号 B: 95% 配额，周配额在 5 天后重置 (非临期，整周还有很多时间可用)
        let mut quotas_b = HashMap::new();
        quotas_b.insert(norm_target, 95);
        let mut token_b = create_test_token("distant@test.com", Some("PRO"), 1.0, None, Some(95));
        token_b.account_id = "acc_distant".to_string();
        token_b.model_quotas = quotas_b;
        token_b.weekly_reset_time = Some(now_ts + 432_000); // 5 天后

        std::fs::write(
            temp_dir.path().join("acc_expiring.json"),
            r#"{"disabled":false}"#,
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("acc_distant.json"),
            r#"{"disabled":false}"#,
        )
        .unwrap();

        token_a.account_path = temp_dir.path().join("acc_expiring.json");
        token_b.account_path = temp_dir.path().join("acc_distant.json");

        manager.tokens.insert(token_a.account_id.clone(), token_a);
        manager.tokens.insert(token_b.account_id.clone(), token_b);

        // 获取 Token 时，EDF 临期优先调度必须超越生硬的「配额越高越先用」，
        // 优先选中 2 小时后即将重置的账号 A，最大化释放用户周期资产！
        let excluded = HashSet::new();
        let (_token, _project_id, email, selected_id, _wait) = manager
            .get_token_filtered("chat", false, None, target_model, &excluded)
            .await
            .expect("Token acquisition should succeed");

        assert_eq!(
            selected_id, "acc_expiring",
            "EDF 调度必须优先调度临期周配额账号，防止配额被周期重置废弃"
        );
        assert_eq!(email, "expiring@test.com");
    }

    #[tokio::test]
    async fn test_edf_sorting_both_imminent_earlier_deadline_wins() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());
        let now_ts = chrono::Utc::now().timestamp();

        let target_model = "gemini-3.8-flash-high";
        let norm_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());

        // 账号 A: 40% 配额，周配额在 2 小时后重置
        let mut quotas_a = HashMap::new();
        quotas_a.insert(norm_target.clone(), 40);
        let mut token_a = create_test_token("sooner@test.com", Some("PRO"), 1.0, None, Some(40));
        token_a.account_id = "acc_sooner".to_string();
        token_a.model_quotas = quotas_a;
        token_a.weekly_reset_time = Some(now_ts + 7200); // 2 小时后

        // 账号 B: 60% 配额，周配额在 8 小时后重置
        let mut quotas_b = HashMap::new();
        quotas_b.insert(norm_target, 60);
        let mut token_b = create_test_token("later@test.com", Some("PRO"), 1.0, None, Some(60));
        token_b.account_id = "acc_later".to_string();
        token_b.model_quotas = quotas_b;
        token_b.weekly_reset_time = Some(now_ts + 28800); // 8 小时后

        std::fs::write(
            temp_dir.path().join("acc_sooner.json"),
            r#"{"disabled":false}"#,
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("acc_later.json"),
            r#"{"disabled":false}"#,
        )
        .unwrap();

        token_a.account_path = temp_dir.path().join("acc_sooner.json");
        token_b.account_path = temp_dir.path().join("acc_later.json");

        manager.tokens.insert(token_a.account_id.clone(), token_a);
        manager.tokens.insert(token_b.account_id.clone(), token_b);

        // 两者都在 24 小时临期窗口内，更早过期的账号 A 必须排在前面
        let excluded = HashSet::new();
        let (_token, _project_id, email, selected_id, _wait) = manager
            .get_token_filtered("chat", false, None, target_model, &excluded)
            .await
            .expect("Token acquisition should succeed");

        assert_eq!(selected_id, "acc_sooner");
        assert_eq!(email, "sooner@test.com");
    }

    #[tokio::test]
    async fn test_edf_sorting_distant_resets_preserves_higher_quota_protection() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());
        let now_ts = chrono::Utc::now().timestamp();

        let target_model = "gemini-3.8-flash-high";
        let norm_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());

        // 账号 A: 40% 配额，周配额在 4 天后重置 (非临期)
        let mut quotas_a = HashMap::new();
        quotas_a.insert(norm_target.clone(), 40);
        let mut token_a =
            create_test_token("low_distant@test.com", Some("PRO"), 1.0, None, Some(40));
        token_a.account_id = "acc_low".to_string();
        token_a.model_quotas = quotas_a;
        token_a.weekly_reset_time = Some(now_ts + 345_600); // 4 天后

        // 账号 B: 95% 配额，周配额在 6 天后重置 (非临期)
        let mut quotas_b = HashMap::new();
        quotas_b.insert(norm_target, 95);
        let mut token_b =
            create_test_token("high_distant@test.com", Some("PRO"), 1.0, None, Some(95));
        token_b.account_id = "acc_high".to_string();
        token_b.model_quotas = quotas_b;
        token_b.weekly_reset_time = Some(now_ts + 518_400); // 6 天后

        std::fs::write(
            temp_dir.path().join("acc_low.json"),
            r#"{"disabled":false}"#,
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("acc_high.json"),
            r#"{"disabled":false}"#,
        )
        .unwrap();

        token_a.account_path = temp_dir.path().join("acc_low.json");
        token_b.account_path = temp_dir.path().join("acc_high.json");

        manager.tokens.insert(token_a.account_id.clone(), token_a);
        manager.tokens.insert(token_b.account_id.clone(), token_b);

        // 两者皆非临期：保持 Priority 1 规则，高配额账号 B (95%) 优先于低配额账号 A (40%)，保护低配额账号
        let excluded = HashSet::new();
        let (_token, _project_id, email, selected_id, _wait) = manager
            .get_token_filtered("chat", false, None, target_model, &excluded)
            .await
            .expect("Token acquisition should succeed");

        assert_eq!(
            selected_id, "acc_high",
            "非临期时应保持高配额优先以保护低配额账号"
        );
        assert_eq!(email, "high_distant@test.com");
    }

    #[tokio::test]
    async fn test_edf_zero_quota_never_elevated() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());
        let now_ts = chrono::Utc::now().timestamp();

        let target_model = "gemini-3.8-flash-high";
        let norm_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());

        // 账号 A: 0% 配额，即便 1 小时后重置，也绝不能因为临期被选出（没有额度）
        let mut quotas_a = HashMap::new();
        quotas_a.insert(norm_target.clone(), 0);
        let mut token_a =
            create_test_token("zero_expiring@test.com", Some("PRO"), 1.0, None, Some(0));
        token_a.account_id = "acc_zero".to_string();
        token_a.model_quotas = quotas_a;
        token_a.weekly_reset_time = Some(now_ts + 3600); // 1 小时后

        // 账号 B: 80% 配额，周配额在 5 天后重置
        let mut quotas_b = HashMap::new();
        quotas_b.insert(norm_target, 80);
        let mut token_b = create_test_token("has_quota@test.com", Some("PRO"), 1.0, None, Some(80));
        token_b.account_id = "acc_has".to_string();
        token_b.model_quotas = quotas_b;
        token_b.weekly_reset_time = Some(now_ts + 432_000); // 5 天后

        std::fs::write(
            temp_dir.path().join("acc_zero.json"),
            r#"{"disabled":false}"#,
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("acc_has.json"),
            r#"{"disabled":false}"#,
        )
        .unwrap();

        token_a.account_path = temp_dir.path().join("acc_zero.json");
        token_b.account_path = temp_dir.path().join("acc_has.json");

        manager.tokens.insert(token_a.account_id.clone(), token_a);
        manager.tokens.insert(token_b.account_id.clone(), token_b);

        let excluded = HashSet::new();
        let (_token, _project_id, email, selected_id, _wait) = manager
            .get_token_filtered("chat", false, None, target_model, &excluded)
            .await
            .expect("Token acquisition should succeed");

        assert_eq!(
            selected_id, "acc_has",
            "0% 配额账号即便临期也绝不得被提权选中"
        );
        assert_eq!(email, "has_quota@test.com");
    }

    #[test]
    fn test_mark_model_quota_depleted_updates_memory_cache() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        let account_id = "acc_depletion_test";
        let target_model = "gemini-3.8-flash-high";
        let norm_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());

        let mut quotas = HashMap::new();
        quotas.insert(norm_target.clone(), 100);
        quotas.insert(target_model.to_string(), 100);

        let mut token = create_test_token("deplete@test.com", Some("PRO"), 1.0, None, Some(100));
        token.account_id = account_id.to_string();
        token.model_quotas = quotas;

        manager.tokens.insert(account_id.to_string(), token);

        // Verify initial state
        assert_eq!(
            manager
                .tokens
                .get(account_id)
                .unwrap()
                .model_quotas
                .get(&norm_target)
                .copied(),
            Some(100)
        );

        // Mark depleted reactively
        manager.mark_model_quota_depleted(account_id, Some(target_model));

        // Verify that memory cache immediately reflects 0% for both canonical and raw names
        assert_eq!(
            manager
                .tokens
                .get(account_id)
                .unwrap()
                .model_quotas
                .get(&norm_target)
                .copied(),
            Some(0),
            "Normalized standard model quota must be reactively updated to 0%"
        );
        assert_eq!(
            manager
                .tokens
                .get(account_id)
                .unwrap()
                .model_quotas
                .get(target_model)
                .copied(),
            Some(0),
            "Raw requested model quota must be reactively updated to 0%"
        );
    }

    #[test]
    fn test_in_flight_guard_lifecycle() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());
        let acc_id = "test_acc_1";

        assert_eq!(manager.get_in_flight(acc_id), 0);

        {
            let _guard1 = manager.acquire_in_flight(acc_id);
            assert_eq!(manager.get_in_flight(acc_id), 1);

            {
                let _guard2 = manager.acquire_in_flight(acc_id);
                assert_eq!(manager.get_in_flight(acc_id), 2);
            }

            assert_eq!(manager.get_in_flight(acc_id), 1);
        }

        assert_eq!(manager.get_in_flight(acc_id), 0);
    }

    #[test]
    fn test_concurrency_spillover_acceleration() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        let mut t1 = create_test_token("pri0@example.com", Some("PRO"), 1.0, None, Some(90));
        t1.account_id = "acc_priority_0".to_string();
        t1.priority = 0;

        let mut t2 = create_test_token("pri1@example.com", Some("PRO"), 1.0, None, Some(85));
        t2.account_id = "acc_priority_1".to_string();
        t2.priority = 1;

        let candidates = vec![t1.clone(), t2.clone()];
        let attempted = HashSet::new();

        // 场景 1: acc_priority_0 空闲 (in_flight == 0)，必须优先选 acc_priority_0
        let sel1 = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet-4-6", false);
        assert_eq!(sel1.unwrap().account_id, "acc_priority_0");

        // 场景 2: acc_priority_0 忙碌 (in_flight > 0)，acc_priority_1 空闲 (in_flight == 0)
        // 必须触发分流外溢加速，选中 acc_priority_1！
        let guard_p0 = manager.acquire_in_flight("acc_priority_0");
        assert_eq!(manager.get_in_flight("acc_priority_0"), 1);

        let sel2 = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet-4-6", false);
        assert_eq!(sel2.unwrap().account_id, "acc_priority_1");

        // 场景 3: 两个账号都在忙，但 acc_priority_1 并发更高 (2)，acc_priority_0 并发为 1
        let _guard_p1_a = manager.acquire_in_flight("acc_priority_1");
        let _guard_p1_b = manager.acquire_in_flight("acc_priority_1");
        assert_eq!(manager.get_in_flight("acc_priority_1"), 2);

        // 此时 acc_priority_0 并发度更低 (1 < 2)，选并发度最低的 acc_priority_0
        let sel3 = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet-4-6", false);
        assert_eq!(sel3.unwrap().account_id, "acc_priority_0");

        drop(guard_p0);
        assert_eq!(manager.get_in_flight("acc_priority_0"), 0);
        // acc_priority_0 重新空闲后，再次选中 acc_priority_0
        let sel4 = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet-4-6", false);
        assert_eq!(sel4.unwrap().account_id, "acc_priority_0");
    }

    #[tokio::test]
    async fn test_max_spillover_accounts_cap() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        let mut t0 = create_test_token("acc0@example.com", Some("PRO"), 1.0, None, Some(90));
        t0.account_id = "acc_0".to_string();
        t0.priority = 0;

        let mut t1 = create_test_token("acc1@example.com", Some("PRO"), 1.0, None, Some(85));
        t1.account_id = "acc_1".to_string();
        t1.priority = 1;

        let mut t2 = create_test_token("acc2@example.com", Some("PRO"), 1.0, None, Some(80));
        t2.account_id = "acc_2".to_string();
        t2.priority = 2;

        let candidates = vec![t0.clone(), t1.clone(), t2.clone()];
        let attempted = HashSet::new();

        // 1. 设置分流上限为 2 个账号
        {
            let mut cfg = manager.get_sticky_config().await;
            cfg.max_spillover_accounts = 2;
            manager.update_sticky_config(cfg).await;
        }

        // 场景 1: acc_0 空闲，选中 acc_0
        let sel1 = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet-4-6", false);
        assert_eq!(sel1.unwrap().account_id, "acc_0");

        // 场景 2: acc_0 忙碌，acc_1 空闲，此时使用的总账号数为 2 <= 上限 2，顺利外溢选中 acc_1
        let guard0 = manager.acquire_in_flight("acc_0");
        let sel2 = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet-4-6", false);
        assert_eq!(sel2.unwrap().account_id, "acc_1");

        // 场景 3: acc_0 忙碌，acc_1 也忙碌。虽然 acc_2 完全空闲，但因为设置了上限 2，严禁触碰第 3 个账号 acc_2！
        let guard1 = manager.acquire_in_flight("acc_1");
        let sel3 = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet-4-6", false);
        // 只能在 acc_0 和 acc_1 中选 (由于并发度相同 1==1，优先选高优先级 acc_0)
        assert_eq!(sel3.unwrap().account_id, "acc_0");

        // 场景 4: 将上限调为 3 或 0 (不限制)，此时允许唤醒第 3 个空闲账号 acc_2
        {
            let mut cfg = manager.get_sticky_config().await;
            cfg.max_spillover_accounts = 3;
            manager.update_sticky_config(cfg).await;
        }
        let sel4 = manager.select_with_p2c(&candidates, &attempted, "claude-sonnet-4-6", false);
        assert_eq!(sel4.unwrap().account_id, "acc_2");

        drop(guard0);
        drop(guard1);
    }

    #[tokio::test]
    async fn test_edf_sorting_depleted_imminent_quota_does_not_usurp_healthy_account() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());
        let now_ts = chrono::Utc::now().timestamp();

        let target_model = "gemini-3.8-flash-high";
        let norm_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());

        // 账号 A: 残留濒危配额 (2%)，即便 1 小时后重置，也绝不能抢占高配额健康账号（避免耗尽重试预算导致假性 429）
        let mut quotas_a = HashMap::new();
        quotas_a.insert(norm_target.clone(), 2);
        let mut token_a = create_test_token(
            "depleted_expiring@test.com",
            Some("PRO"),
            1.0,
            None,
            Some(2),
        );
        token_a.account_id = "acc_depleted".to_string();
        token_a.model_quotas = quotas_a;
        token_a.weekly_reset_time = Some(now_ts + 3600); // 1 小时后重置

        // 账号 B: 95% 健康配额，周配额在 5 天后重置
        let mut quotas_b = HashMap::new();
        quotas_b.insert(norm_target, 95);
        let mut token_b =
            create_test_token("healthy_distant@test.com", Some("PRO"), 1.0, None, Some(95));
        token_b.account_id = "acc_healthy".to_string();
        token_b.model_quotas = quotas_b;
        token_b.weekly_reset_time = Some(now_ts + 432_000); // 5 天后

        std::fs::write(
            temp_dir.path().join("acc_depleted.json"),
            r#"{"disabled":false}"#,
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("acc_healthy.json"),
            r#"{"disabled":false}"#,
        )
        .unwrap();

        token_a.account_path = temp_dir.path().join("acc_depleted.json");
        token_b.account_path = temp_dir.path().join("acc_healthy.json");

        manager.tokens.insert(token_a.account_id.clone(), token_a);
        manager.tokens.insert(token_b.account_id.clone(), token_b);

        let excluded = HashSet::new();
        let (_token, _project_id, email, selected_id, _wait) = manager
            .get_token_filtered("chat", false, None, target_model, &excluded)
            .await
            .expect("Token acquisition should succeed");

        assert_eq!(
            selected_id, "acc_healthy",
            "濒危低配额(<=10%)账号即便临期也绝不得抢占健康高配额账号"
        );
        assert_eq!(email, "healthy_distant@test.com");
    }

    #[tokio::test]
    async fn test_get_all_model_metadata_collection_and_bare_derivation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        let mut meta = HashMap::new();
        meta.insert(
            "gemini-3.7-flash-high".to_string(),
            crate::models::quota::ModelListingMeta {
                display_name: Some("Gemini 3.7 Flash (High)".to_string()),
                supports_images: Some(true),
                supports_thinking: Some(true),
                thinking_budget: Some(2048),
                recommended: Some(true),
                max_tokens: Some(1048576),
                max_output_tokens: Some(65536),
                supported_mime_types: None,
            },
        );

        let token = ProxyToken {
            account_id: "acc_test_meta".to_string(),
            priority: 0,
            access_token: "token1".to_string(),
            refresh_token: "refresh1".to_string(),
            expires_in: 3600,
            timestamp: chrono::Utc::now().timestamp() + 3600,
            email: "meta@example.com".to_string(),
            account_path: temp_dir.path().join("acc.json"),
            project_id: None,
            subscription_tier: Some("PRO".to_string()),
            remaining_quota: Some(100),
            protected_models: HashSet::new(),
            health_score: 1.0,
            reset_time: None,
            weekly_reset_time: None,
            validation_blocked: false,
            validation_blocked_until: 0,
            validation_url: None,
            model_quotas: HashMap::new(),
            model_limits: HashMap::new(),
            available_models: HashSet::new(),
            model_metadata: meta,
        };

        manager.tokens.insert(token.account_id.clone(), token);

        // 1. 测试 TokenManager 聚合
        let collected = manager.get_all_model_metadata();
        assert!(collected.contains_key("gemini-3.7-flash-high"));
        assert_eq!(
            collected["gemini-3.7-flash-high"].supports_images,
            Some(true)
        );

        // 2. 测试 model_mapping 派生裸模型元数据继承
        let custom = tokio::sync::RwLock::new(HashMap::new());
        let all_meta = crate::proxy::common::model_mapping::get_all_dynamic_models_metadata(
            &custom,
            Some(&manager),
            true,
        )
        .await;

        let bare = all_meta
            .get("gemini-3.7-flash")
            .expect("gemini-3.7-flash 裸模型必须自动派生并存在于元数据表中");
        assert_eq!(bare.display_name.as_deref(), Some("Gemini 3.7 Flash"));
        assert_eq!(bare.supports_images, Some(true));
        assert_eq!(bare.supports_thinking, Some(true));
        assert_eq!(bare.thinking_budget, Some(2048));
        assert_eq!(bare.max_output_tokens, Some(65536));
    }

    #[tokio::test]
    async fn test_mark_account_transient_failure_locks_and_unbinds_session() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        let acc_id = "test_transient_fail_acc";
        let email = "fail_user@example.com";
        let mut token = create_test_token(email, Some("PRO"), 1.0, None, Some(100));
        token.account_id = acc_id.to_string();
        manager.tokens.insert(acc_id.to_string(), token);

        let sid = "session-test-sticky-123";
        manager.commit_session(sid, acc_id);
        assert_eq!(manager.session_accounts.get(sid).unwrap().as_str(), acc_id);
        assert!(!manager.rate_limit_tracker.is_rate_limited(acc_id, None));

        // 模拟调用短时故障标记
        manager.mark_account_transient_failure(email, 30, "HTTP timeout");

        // 账号进入限流冷却
        assert!(manager.rate_limit_tracker.is_rate_limited(acc_id, None));
        // 粘性绑定立即被解除
        assert!(manager.session_accounts.get(sid).is_none());
    }

    /// 生图变体归一：账号只上报 `-4k` 变体且真实配额 > 0 时，基底生图模型视为可用；
    /// 0 配额变体不得把基底模型带入列表或配额闸门。
    #[test]
    fn image_variant_quota_counts_for_base_model() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        let mut with_quota =
            create_test_token("img1@example.com", Some("PRO"), 1.0, None, Some(100));
        with_quota.account_id = "img_acc_quota".into();
        with_quota
            .model_quotas
            .insert("gemini-3.1-flash-image-4k".into(), 33);
        manager.tokens.insert("img_acc_quota".into(), with_quota);

        let mut zero_quota =
            create_test_token("img2@example.com", Some("PRO"), 1.0, None, Some(100));
        zero_quota.account_id = "img_acc_zero".into();
        zero_quota
            .model_quotas
            .insert("gemini-3.1-flash-image-2k".into(), 0);
        manager.tokens.insert("img_acc_zero".into(), zero_quota);

        assert_eq!(
            manager.accounts_with_positive_quota("gemini-3.1-flash-image"),
            1,
            "只有真实配额 > 0 的变体账号才计入基底生图模型"
        );
        let positive = manager.models_with_positive_quota();
        assert!(
            positive.contains(&"gemini-3.1-flash-image-4k".to_string()),
            "配额 > 0 的变体应被统计: {positive:?}"
        );
        assert!(
            !positive.contains(&"gemini-3.1-flash-image-2k".to_string()),
            "0 配额变体不得进入列表推导: {positive:?}"
        );
    }

    /// 网关自答必须对全部惩罚入口免疫：不得锁号、不得解绑粘性会话、不得写入零配额。
    #[tokio::test]
    async fn gateway_self_answers_never_penalize_accounts() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        let acc_id = "test_gate_exempt_acc";
        let email = "gate@example.com";
        let mut token = create_test_token(email, Some("PRO"), 1.0, None, Some(100));
        token.account_id = acc_id.to_string();
        manager.tokens.insert(acc_id.to_string(), token);

        let sid = "sid-gate-exempt";
        manager.commit_session(sid, acc_id);

        // 1) 短时故障冷却入口：两个哨兵均不得触发冷却 / 解绑
        manager.mark_account_transient_failure(acc_id, 30, "model not exist");
        manager.mark_account_transient_failure(acc_id, 30, "no quota accounts");
        assert!(
            !manager.rate_limit_tracker.is_rate_limited(acc_id, None),
            "网关自答不得计入账号冷却"
        );
        assert_eq!(
            manager.session_accounts.get(sid).unwrap().as_str(),
            acc_id,
            "网关自答不得解绑粘性会话"
        );

        // 2) 快速限流标记入口：不得返回需要刷新配额，也不得写入限流记录
        let needs_refresh = manager
            .mark_rate_limited_fast(email, 404, None, "model not exist", Some("gemini-2.5-pro"))
            .await;
        assert!(!needs_refresh, "网关自答不得触发配额刷新锁定");
        assert!(
            !manager
                .rate_limit_tracker
                .is_rate_limited(acc_id, Some("gemini-2.5-pro")),
            "网关自答不得为任何模型写入冷却"
        );
    }

    /// 探针必须挑选账号真实可用且配额为正的 Gemini 文本 Flash 档，
    /// 绝不能拿 Pro / Image / Argon 去探活（会直接撞 429 并把账号打进长冷却）。
    #[test]
    fn test_select_probe_model_prefers_positive_flash() {
        let mut token = create_test_token("probe@example.com", Some("PRO"), 1.0, None, Some(100));
        token
            .model_quotas
            .insert("gemini-3.8-flash-high".to_string(), 33);
        token
            .model_quotas
            .insert("gemini-4-argon-medium".to_string(), 0);
        token
            .model_quotas
            .insert("gemini-3.1-pro-high".to_string(), 80);
        token
            .model_quotas
            .insert("gemini-3.1-flash-image".to_string(), 33);

        assert_eq!(
            TokenManager::select_probe_model(&token).as_deref(),
            Some("gemini-3.8-flash-high"),
            "必须优先选择正配额的 Gemini 文本 Flash 模型"
        );
    }

    #[test]
    fn test_select_probe_model_falls_back_to_available_models() {
        let mut token = create_test_token("probe2@example.com", Some("PRO"), 1.0, None, Some(100));
        // 历史账号未同步配额表时，用 available_models 兜底
        token
            .available_models
            .insert("gemini-3.8-flash-tiered".to_string());

        assert_eq!(
            TokenManager::select_probe_model(&token).as_deref(),
            Some("gemini-3.8-flash-tiered")
        );
    }

    #[test]
    fn test_select_probe_model_without_metadata_skips_probe() {
        let token = create_test_token("probe3@example.com", Some("PRO"), 1.0, None, Some(100));
        assert!(
            TokenManager::select_probe_model(&token).is_none(),
            "无模型元数据时必须放弃探针，而不是盲发任意模型"
        );
    }

    /// [FIX] 探针报文必须是完整的 v1internal 官方信封；
    /// 裸 {contents, generationConfig} 会被上游 400 拒绝，导致阶梯自愈永远失败。
    #[test]
    fn test_build_probe_envelope_matches_official_shape() {
        let mut token = create_test_token("probe4@example.com", Some("PRO"), 1.0, None, Some(100));
        token.project_id = Some("aicode-consumers".to_string());

        let body = TokenManager::build_probe_envelope(&token, "gemini-3.8-flash-high");

        assert_eq!(body["project"], "aicode-consumers");
        assert_eq!(body["model"], "gemini-3.8-flash-high");
        assert_eq!(body["userAgent"], "antigravity");
        assert!(
            body["requestId"]
                .as_str()
                .unwrap_or("")
                .starts_with("agent/"),
            "requestId 必须对齐官方 agent/ 前缀: {body}"
        );

        let request = &body["request"];
        assert!(
            request.get("contents").is_some(),
            "contents 必须包裹在 request 内: {body}"
        );
        assert_eq!(request["generationConfig"]["maxOutputTokens"], 10);
        assert_eq!(request["generationConfig"]["temperature"], 0.0);

        let contents = request["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "你好");
    }

    #[test]
    fn test_build_probe_envelope_falls_back_to_default_project() {
        let mut token = create_test_token("probe5@example.com", Some("PRO"), 1.0, None, Some(100));
        token.project_id = None;

        let body = TokenManager::build_probe_envelope(&token, "gemini-3-flash");
        assert_eq!(body["project"], DEFAULT_PROJECT_ID);
    }
}
