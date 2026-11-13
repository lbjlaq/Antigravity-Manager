// 移除冗余的顶层导入，因为这些在代码中已由 full path 或局部导入处理
use axum::http::StatusCode;
use dashmap::DashMap;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use tokio_util::sync::CancellationToken;

use crate::models::account::{classify_lock_reason, AccountLockKind};
use crate::proxy::rate_limit::RateLimitTracker;
use crate::proxy::server::{ImagePermit, ImageScheduler};
use crate::proxy::sticky_config::StickySessionConfig;

/// 账号 JSON 模型清单的解析缓存快照（模块级，见 `get_available_models_from_json`）
struct AccountModelFileSnapshot {
    len: u64,
    modified: Option<std::time::SystemTime>,
    models: Option<HashSet<String>>,
}

static ACCOUNT_MODEL_FILE_CACHE: once_cell::sync::Lazy<DashMap<PathBuf, AccountModelFileSnapshot>> =
    once_cell::sync::Lazy::new(DashMap::new);

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

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QuotaWindowStatus {
    pub is_weekly: bool,
    pub remaining_fraction: Option<f64>,
    pub reset_time: Option<i64>,
}

/// 只读配额桶视图：用于测试、诊断与未来 QuotaSnapshot 迁移，不参与调度决策。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct QuotaBucketState {
    pub family: String,
    pub is_weekly: bool,
    pub remaining_fraction: Option<f64>,
    pub reset_at: Option<i64>,
}

/// 由 ProxyToken 现有三套 map 派生出的只读快照；不改变任何调度读写路径。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
pub struct QuotaSnapshot {
    pub buckets: Vec<QuotaBucketState>,
}

impl QuotaSnapshot {
    pub fn family_buckets<'a>(
        &'a self,
        family: &'a str,
    ) -> impl Iterator<Item = &'a QuotaBucketState> + 'a {
        self.buckets.iter().filter(move |b| b.family == family)
    }
}

/// 配额写入入口的中立输入：load / 实时刷新都先归一化为该结构。
#[derive(Debug, Clone, PartialEq)]
struct QuotaBucketInput {
    family: String,
    is_weekly: bool,
    remaining_fraction: Option<f64>,
    reset_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ProxyToken {
    pub account_id: String,
    pub priority: u8,
    pub model_priorities: HashMap<String, u8>,
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
    pub forbidden_models: HashSet<String>, // [NEW] 模型级 403 冻结（只影响对应模型）
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
    /// [NEW] 各模型及各同族配额组具体短期 (5h) 刷新时间戳（用于消除跨模型截止时间污染）
    pub model_reset_times: HashMap<String, i64>,
    /// [NEW] 各模型及各同族配额组具体周周期刷新时间戳（用于 Earliest Deadline First 临期判定）
    pub model_weekly_reset_times: HashMap<String, i64>,
    /// [NEW] 各模型及各同族配额组具体配额窗口快照（包含 is_weekly, remaining_fraction, reset_time）
    pub model_quota_windows: HashMap<String, Vec<QuotaWindowStatus>>,
}

impl ProxyToken {
    /// 根据当前请求的目标模型与归一化模型组，计算该账号的实际生效调度优先级。
    /// 若设置了对应模型组（"gemini" | "claude"）的独立优先级则优先采用，否则回退至账号全局 `priority`。
    pub fn effective_priority(&self, target_model: &str, normalized_target: &str) -> u8 {
        if self.model_priorities.is_empty() {
            return self.priority;
        }
        let group =
            crate::models::account::resolve_model_priority_group(target_model, normalized_target);
        self.model_priorities
            .get(group)
            .copied()
            .unwrap_or(self.priority)
    }

    /// 目标模型是否被模型级 403 冻结。账号级封禁/403 由加载阶段处理。
    pub fn is_model_forbidden(&self, model: &str) -> bool {
        crate::models::account::is_model_forbidden_by_scopes(model, &self.forbidden_models)
    }

    /// 只读配额视图：由现有三套 map 派生，供测试、诊断与后续迁移使用。
    /// 注意：调度器仍读取原始 map，本方法不参与任何调度决策。
    pub fn quota_snapshot(&self) -> QuotaSnapshot {
        let mut buckets = Vec::new();
        for (family, windows) in &self.model_quota_windows {
            for w in windows {
                buckets.push(QuotaBucketState {
                    family: family.clone(),
                    is_weekly: w.is_weekly,
                    remaining_fraction: w.remaining_fraction,
                    reset_at: w.reset_time,
                });
            }
        }

        // 兼容旧数据：窗口快照缺失时，用族级 reset map 补出只读视图。
        for family in ["argon", "3p", "gemini"] {
            if self.model_quota_windows.contains_key(family) {
                continue;
            }
            if let Some(ts) = self.model_reset_times.get(family) {
                buckets.push(QuotaBucketState {
                    family: family.to_string(),
                    is_weekly: false,
                    remaining_fraction: None,
                    reset_at: Some(*ts),
                });
            }
            if let Some(ts) = self.model_weekly_reset_times.get(family) {
                buckets.push(QuotaBucketState {
                    family: family.to_string(),
                    is_weekly: true,
                    remaining_fraction: None,
                    reset_at: Some(*ts),
                });
            }
        }

        buckets.sort_by(|a, b| {
            a.family
                .cmp(&b.family)
                .then_with(|| a.is_weekly.cmp(&b.is_weekly))
                .then_with(|| a.reset_at.cmp(&b.reset_at))
        });
        QuotaSnapshot { buckets }
    }

    /// 在持有 `DashMap::RefMut` 时安全地统一写入三套配额 map。
    fn apply_quota_bucket_inputs(&mut self, inputs: impl IntoIterator<Item = QuotaBucketInput>) {
        let Self {
            model_quota_windows,
            model_reset_times,
            model_weekly_reset_times,
            ..
        } = self;
        TokenManager::apply_quota_bucket_inputs(
            model_quota_windows,
            model_reset_times,
            model_weekly_reset_times,
            inputs,
        );
    }

    /// Debug/测试专用：校验族级窗口快照与 reset map 的最小截止时间一致。
    /// Release 构建不包含该方法，不会影响线上行为。
    #[cfg(debug_assertions)]
    pub fn assert_quota_state_consistent(&self) {
        for (family, windows) in &self.model_quota_windows {
            if windows.is_empty() {
                continue;
            }
            let short_min = windows
                .iter()
                .filter(|w| !w.is_weekly)
                .filter_map(|w| w.reset_time)
                .min();
            let weekly_min = windows
                .iter()
                .filter(|w| w.is_weekly)
                .filter_map(|w| w.reset_time)
                .min();
            debug_assert_eq!(
                self.model_reset_times.get(family).copied(),
                short_min,
                "short reset map drifted for family {family}"
            );
            debug_assert_eq!(
                self.model_weekly_reset_times.get(family).copied(),
                weekly_min,
                "weekly reset map drifted for family {family}"
            );
        }
    }
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

    /// 模型门禁在准备返回 `no quota accounts` 前，强制从磁盘刷新账号池的节流时间戳（Unix 秒）。
    /// 确保无论通过 Tauri / HTTP API / CLI 哪条路径导入新账号，门禁都能在 2 秒内看到它。
    last_gate_pool_refresh: Arc<AtomicI64>,

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
            last_gate_pool_refresh: Arc::new(AtomicI64::new(0)),
            in_flight_requests: Arc::new(DashMap::new()),
            qos_guardian: {
                let qos_auto_jail = crate::modules::config::load_app_config()
                    .map(|c| c.proxy.qos_auto_jail)
                    .unwrap_or(false);
                let guardian = Arc::new(crate::proxy::qos_guardian::QoSGuardian::new());
                guardian.set_auto_jail_enabled(qos_auto_jail);
                guardian
            },
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
                        let client_key = token_response.oauth_client_key;
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
                            if let Some(ck) = client_key {
                                if crate::modules::oauth::is_gcp_enterprise_client(Some(&ck)) {
                                    val["token"]["is_gcp_tos"] = serde_json::Value::Bool(true);
                                }
                                val["token"]["oauth_client_key"] = ck.into();
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

    /// 读取 `accounts.json` 索引，返回 `(全部注册账号 ID, disabled/proxy_disabled 账号 ID)`。
    fn read_accounts_index_sets(
        data_dir: &std::path::Path,
    ) -> (Option<HashSet<String>>, HashSet<String>) {
        let index_file = data_dir.join("accounts.json");
        if !index_file.exists() {
            return (None, HashSet::new());
        }
        let Ok(content) = std::fs::read_to_string(&index_file) else {
            return (None, HashSet::new());
        };
        let Ok(index_val) = serde_json::from_str::<serde_json::Value>(&content) else {
            return (None, HashSet::new());
        };
        let Some(accs) = index_val.get("accounts").and_then(|a| a.as_array()) else {
            return (None, HashSet::new());
        };
        let mut all_ids = HashSet::new();
        let mut disabled_ids = HashSet::new();
        for acc in accs {
            if let Some(id) = acc.get("id").and_then(|v| v.as_str()) {
                all_ids.insert(id.to_string());
                let is_disabled = acc
                    .get("disabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let is_proxy_disabled = acc
                    .get("proxy_disabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if is_disabled || is_proxy_disabled {
                    disabled_ids.insert(id.to_string());
                }
            }
        }
        (Some(all_ids), disabled_ids)
    }

    /// 校验账号文件是否属于有效索引（兼容 `stem != id` 的文件）。
    fn is_account_file_in_index(path: &PathBuf, valid_ids: &HashSet<String>) -> bool {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if valid_ids.contains(stem) {
            return true;
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
            .and_then(|v| v.get("id").and_then(|id| id.as_str()).map(str::to_string))
            .is_some_and(|internal_id| valid_ids.contains(&internal_id))
    }

    /// 从主应用账号目录加载所有账号
    pub async fn load_accounts(&self) -> Result<usize, String> {
        let accounts_dir = self.resolved_data_dir().join("accounts");

        if !accounts_dir.exists() {
            return Err(format!("账号目录不存在: {:?}", accounts_dir));
        }

        // 读取 accounts.json 索引，过滤掉用户在索引中标记为 disabled 或 proxy_disabled 的账号，防止实体文件残留导致已停用账号渗入代理池
        let (valid_in_index, disabled_in_index) =
            Self::read_accounts_index_sets(&self.resolved_data_dir());

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

            // [FIX #3806, #3808] 若 accounts.json 索引存在且有效，仅加载注册表中的合法账号。
            // 孤儿账号（已在界面删除但磁盘残留的 *.json 文件）拒绝载入代理调度池，仅记录 warning 跳过，
            // 严禁在此主动删除文件，避免并发导入或未提交窗口期误删 refresh token。
            if let Some(ref valid_ids) = valid_in_index {
                if !Self::is_account_file_in_index(&path, valid_ids) {
                    tracing::warn!(
                        "[Proxy] Skipping orphan account file {:?} from proxy pool (not present in accounts.json index)",
                        path
                    );
                    continue;
                }
            }

            if disabled_in_index.contains(stem) {
                // 实体文件是账号状态的真实权威来源。先检查实体文件，若实体文件本身未停用（索引数据陈旧），以实体文件为准；
                // 仅当实体文件确实标记为停用或不可读时才跳过。
                let is_truly_disabled = if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(acc_json) = serde_json::from_str::<serde_json::Value>(&content) {
                        let is_disabled = acc_json
                            .get("disabled")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let is_proxy_disabled = if acc_json
                            .get("proxy_disabled")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                        {
                            let manual = acc_json
                                .get("manual_proxy_disabled")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            let reason = acc_json
                                .get("proxy_disabled_reason")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let quota_reason = acc_json
                                .pointer("/quota/forbidden_reason")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            // 审查项 7：若属于可自愈迁移的模型级 403 且非手动禁用，不视为永久禁用，放行至 load_single_account 执行自愈迁移！
                            if !manual
                                && (crate::models::account::forbidden_model_scope(reason).is_some()
                                    || crate::models::account::forbidden_model_scope(quota_reason)
                                        .is_some())
                            {
                                false
                            } else {
                                true
                            }
                        } else {
                            false
                        };
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

        // 全量重载（服务启动 / 导入 / 删除 / 刷新全部）代表一次权威账号快照。
        // 清空内存中的模型级 900s 临时熔断，避免新导入或已恢复配额的账号
        // 继续被历史冷却卡住 15 分钟。
        self.unsupported_models.clear();
        let _ = self.get_all_available_models();

        Ok(count)
    }

    /// 彻底卸载账号并异步清理需要 `.await` 写锁的粘性/优先/末次选中缓存
    pub async fn remove_account_and_clear_locks(&self, account_id: &str) {
        self.remove_account(account_id);
        {
            let mut preferred = self.preferred_account_id.write().await;
            if preferred.as_deref() == Some(account_id) {
                *preferred = None;
                tracing::info!(
                    "[Proxy] Cleared preferred account status for {}",
                    account_id
                );
            }
        }
        {
            let mut last_used = self.last_used_account.lock().await;
            if matches!(&*last_used, Some((ref id, _)) if id == account_id) {
                *last_used = None;
            }
        }
    }

    /// 手动重新启用账号反代时清理运行期拦截态（QoS 隔离、401/project_id 失败计数、模型临时熔断、内存限流锁定）
    pub fn prepare_manual_enable(&self, account_id: &str) {
        self.qos_guardian.release_from_jail(account_id);
        self.invalid_grant_failures.remove(account_id);
        self.project_id_failures.remove(account_id);
        self.clear_unsupported_models_for_account(account_id);
        self.clear_rate_limit_memory(account_id);
    }

    #[cfg(test)]
    pub(crate) fn insert_token_for_test(&self, token: ProxyToken) {
        self.tokens.insert(token.account_id.clone(), token);
    }

    /// 重新加载指定账号（用于配额更新后的实时同步）
    pub async fn reload_account(&self, account_id: &str) -> Result<(), String> {
        let path = self
            .data_dir
            .join("accounts")
            .join(format!("{}.json", account_id));
        if !path.exists() {
            self.remove_account_and_clear_locks(account_id).await;
            return Err(format!("账号文件不存在: {:?}", path));
        }

        // 实体文件是账号状态的真实权威来源。
        // 若 accounts.json 索引标记了停用，且实体文件也确实停用，则快速从内存池移除；
        // 若实体文件未停用（索引发生漂移），则以实体文件为准继续加载。
        let index_file = self.resolved_data_dir().join("accounts.json");
        if index_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&index_file) {
                if let Ok(index_val) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(accs) = index_val.get("accounts").and_then(|a| a.as_array()) {
                        let in_index = accs
                            .iter()
                            .any(|acc| acc.get("id").and_then(|v| v.as_str()) == Some(account_id));
                        // [FIX #3806, #3808] 若 accounts.json 存在但该账号已被删除，仅从内存调度池卸载，不删物理文件，并返回 Ok(())
                        if !in_index {
                            self.remove_account_and_clear_locks(account_id).await;
                            tracing::info!(
                                "[Proxy] Account {} is not in accounts.json index, removed from proxy pool",
                                account_id
                            );
                            return Ok(());
                        }
                    }

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
                        self.remove_account_and_clear_locks(account_id).await;
                        return Ok(());
                    }
                }
            }
        }

        match self.load_single_account(&path).await {
            Ok(Some(token)) => {
                // 配额刷新是权威快照：立即释放该账号的历史模型级 900s 熔断；
                // 再按模型粒度释放已恢复的 QuotaExhausted，严格保留 429 速率限制退避与 0 配额模型的锁 (Ref #3840, #3845)
                self.clear_unsupported_models_for_account(account_id);
                self.reconcile_recovered_quota_locks(account_id, &token);
                self.tokens.insert(account_id.to_string(), token);

                self.sync_image_scheduler_accounts();
                let _ = self.get_all_available_models();
                Ok(())
            }
            Ok(None) => {
                // [FIX] 账号被禁用或不可用时，从内存池中彻底移除 (Issue #1565)
                // load_single_account returning None means the account should be skipped in its
                // current state (disabled / proxy_disabled / quota_protection / validation_blocked...).
                self.remove_account_and_clear_locks(account_id).await;
                Ok(())
            }
            Err(e) => Err(format!("同步账号失败: {}", e)),
        }
    }

    /// 重新加载所有账号
    pub async fn reload_all_accounts(&self) -> Result<usize, String> {
        let count = self.load_accounts().await?;

        // 批量配额刷新的调用方：按模型粒度对已恢复正配额的模型释放旧 QuotaExhausted 锁定，
        // 严格保留 429 速率限制退避与仍为 0 配额模型的锁定 (Ref #3840, #3845, #3852)
        for entry in self.tokens.iter() {
            self.reconcile_recovered_quota_locks(entry.key(), entry.value());
        }
        let _ = self.get_all_available_models();

        Ok(count)
    }

    /// 对一次权威账号快照执行按模型粒度的配额解限：
    /// - 有 per-model 配额：仅对 `> 0` 的模型清 `QuotaExhausted`；
    /// - 无 per-model 配额：回退到账号级聚合配额；
    /// - 任一模型正配额时，额外清账号级“全模型耗尽”锁；
    /// - `RateLimitExceeded` 永不触碰。
    fn reconcile_recovered_quota_locks(&self, account_id: &str, token: &ProxyToken) {
        if token.model_quotas.is_empty() {
            if token.remaining_quota.unwrap_or(0) > 0 {
                self.rate_limit_tracker
                    .reconcile_all_quota_recovery_for_account(account_id);
            }
            return;
        }

        let mut has_positive = false;
        for (model, quota) in &token.model_quotas {
            if *quota > 0 {
                has_positive = true;
                self.rate_limit_tracker
                    .reconcile_quota_recovery(account_id, model);
            }
        }
        if has_positive {
            self.rate_limit_tracker
                .reconcile_account_quota_recovery(account_id);
        }
    }

    /// 处理待重载账号队列。
    ///
    /// 模型门禁在判断 `no quota accounts` 前必须先刷新内存池，否则配额更新/新导入
    /// 产生的 reload 信号只能等到 `get_token_filtered` 才能被消费；而门禁一旦误判，
    /// 请求根本到不了 `get_token_filtered`，形成「越等越不恢复」的死锁。
    pub async fn sync_pending_account_reloads(&self) {
        for account_id in crate::proxy::server::take_pending_reload_accounts() {
            if let Err(e) = self.reload_account(&account_id).await {
                tracing::warn!(
                    "[Quota] Failed to reload account {} before model gate: {}",
                    account_id,
                    e
                );
            }
        }
    }

    /// 模型门禁的最后一道自愈保障：在返回 `no quota accounts` 前强制刷新一次账号池。
    ///
    /// 配额更新/新账号导入可能来自 Tauri、HTTP API、CLI 等不同入口，部分入口只写入
    /// 磁盘而不会向运行中的 TokenManager 发 reload。若不在这里重新读盘，门禁会
    /// 持续用旧快照判死整池，直到 900s 旧熔断过期才偶然恢复。
    ///
    /// 为避免真正无配额时每个请求都触发全量磁盘扫描，这里做 2 秒节流。
    pub async fn refresh_pool_for_gate_if_stale(&self) {
        let now = chrono::Utc::now().timestamp();
        let last = self.last_gate_pool_refresh.load(Ordering::Relaxed);
        // 池子为空时不节流：新账号导入后必须尽快解除 no-quota 门禁。
        if !self.tokens.is_empty() && now.saturating_sub(last) < 2 {
            return;
        }
        if self
            .last_gate_pool_refresh
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return;
        }

        // 先处理精确的待刷新账号（配额更新路径），再做一次全量读盘兜底
        // （覆盖 CLI/headless 导入等未主动触发 reload 的路径）。
        self.sync_pending_account_reloads().await;
        if let Err(e) = self.reload_all_accounts().await {
            tracing::warn!(
                "[ModelGate] Failed to refresh account pool before rejection: {}",
                e
            );
        }
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
        self.invalid_grant_failures.remove(account_id);
        self.in_flight_requests.remove(account_id);
        self.clear_unsupported_models_for_account(account_id);
        self.qos_guardian.release_from_jail(account_id);
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
        let _ = self.get_all_available_models();
    }

    /// 根据账号 ID 获取完整的 ProxyToken 对象 (v4.1.29)
    pub fn get_token_by_id(&self, account_id: &str) -> Option<ProxyToken> {
        self.tokens.get(account_id).map(|t| t.clone())
    }

    /// Apply a saved priority without resetting sessions or live rate limits.
    pub fn update_account_priority(&self, account_id: &str, priority: u8) {
        self.update_account_model_priorities(account_id, priority, None);
    }

    /// Apply saved priority and optional per-model-family priority overrides to in-memory ProxyToken.
    pub fn update_account_model_priorities(
        &self,
        account_id: &str,
        priority: u8,
        model_priorities: Option<HashMap<String, u8>>,
    ) {
        if let Some(mut token) = self.tokens.get_mut(account_id) {
            token.priority = priority;
            if let Some(mp) = model_priorities {
                token.model_priorities = mp;
            }
        }
    }

    /// Apply a saved GCP project_id dynamically to in-memory ProxyToken.
    pub fn update_account_project_id(&self, account_id: &str, project_id: Option<String>) {
        if let Some(mut token) = self.tokens.get_mut(account_id) {
            token.project_id = project_id
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
        self.project_id_failures.remove(account_id);
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
                    .get("banned")
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

    /// 从模型名称集合中动态提取 Gemini 4+ 专属代号（零硬编码，自动覆盖未来新代号）。
    fn collect_known_g4_codenames<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        let mut codenames = Vec::new();
        for name in names {
            let nl = name.trim().to_lowercase();
            if nl.contains("-disabled") || nl.ends_with("_disabled") {
                continue;
            }
            if let Some(code) = crate::proxy::common::model_mapping::extract_gemini4_codename(&nl) {
                if !codenames.iter().any(|c| c == &code) {
                    codenames.push(code);
                }
            }
        }
        codenames
    }

    fn quota_family_key_with_codenames(
        display_name_lower: &str,
        bucket_id_lower: &str,
        known_codenames: &[String],
    ) -> String {
        if let Some(code) =
            crate::proxy::common::model_mapping::extract_group_g4_codename_with_known(
                display_name_lower,
                bucket_id_lower,
                known_codenames,
            )
        {
            code
        } else if display_name_lower.contains("claude")
            || display_name_lower.contains("gpt")
            || bucket_id_lower.contains("3p")
        {
            "3p".to_string()
        } else {
            "gemini".to_string()
        }
    }

    /// 组级配额族 Key 解析（无模型上下文时的兼容入口）。
    fn quota_family_key(display_name_lower: &str, bucket_id_lower: &str) -> String {
        Self::quota_family_key_with_codenames(display_name_lower, bucket_id_lower, &[])
    }

    fn quota_bucket_is_weekly(window_lower: &str, bucket_id_lower: &str) -> bool {
        window_lower.contains("week")
            || window_lower.contains("7d")
            || bucket_id_lower.contains("week")
            || bucket_id_lower.contains("7d")
    }

    /// 将账号 JSON 中的 `quota_groups` 归一化为中立输入（结合动态代号上下文）。
    fn quota_bucket_inputs_from_json_with_codenames(
        groups: &[serde_json::Value],
        known_codenames: &[String],
    ) -> Vec<QuotaBucketInput> {
        let mut inputs = Vec::new();
        for group in groups {
            let display_name_lower = group
                .get("display_name")
                .or_else(|| group.get("displayName"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            let Some(buckets) = group.get("buckets").and_then(|b| b.as_array()) else {
                continue;
            };
            for bucket in buckets {
                let window_lower = bucket
                    .get("window")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                let bucket_id_lower = bucket
                    .get("bucket_id")
                    .or_else(|| bucket.get("bucketId"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                let b_name_lower = bucket
                    .get("display_name")
                    .or_else(|| bucket.get("displayName"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                let group_or_bucket_name = if !b_name_lower.is_empty() {
                    format!("{} {}", display_name_lower, b_name_lower)
                } else {
                    display_name_lower.clone()
                };
                let remaining_fraction = bucket
                    .get("remaining_fraction")
                    .or_else(|| bucket.get("remainingFraction"))
                    .and_then(|v| v.as_f64());
                let reset_at = bucket
                    .get("reset_time")
                    .or_else(|| bucket.get("resetTime"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.timestamp());
                inputs.push(QuotaBucketInput {
                    family: Self::quota_family_key_with_codenames(
                        &group_or_bucket_name,
                        &bucket_id_lower,
                        known_codenames,
                    ),
                    is_weekly: Self::quota_bucket_is_weekly(&window_lower, &bucket_id_lower),
                    remaining_fraction,
                    reset_at,
                });
            }
        }
        inputs
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn quota_bucket_inputs_from_json(groups: &[serde_json::Value]) -> Vec<QuotaBucketInput> {
        Self::quota_bucket_inputs_from_json_with_codenames(groups, &[])
    }

    /// 将强类型 `QuotaGroup` 归一化为同一份中立输入（结合动态代号上下文）。
    fn quota_bucket_inputs_from_typed_with_codenames(
        groups: &[crate::models::quota::QuotaGroup],
        known_codenames: &[String],
    ) -> Vec<QuotaBucketInput> {
        let mut inputs = Vec::new();
        for group in groups {
            let display_name_lower = group.display_name.to_lowercase();
            for bucket in &group.buckets {
                let window_lower = bucket.window.to_lowercase();
                let bucket_id_lower = bucket.bucket_id.to_lowercase();
                let b_name_lower = bucket.display_name.as_deref().unwrap_or("").to_lowercase();
                let group_or_bucket_name = if !b_name_lower.is_empty() {
                    format!("{} {}", display_name_lower, b_name_lower)
                } else {
                    display_name_lower.clone()
                };
                let reset_at = if bucket.reset_time.is_empty() {
                    None
                } else {
                    chrono::DateTime::parse_from_rfc3339(&bucket.reset_time)
                        .ok()
                        .map(|dt| dt.timestamp())
                };
                inputs.push(QuotaBucketInput {
                    family: Self::quota_family_key_with_codenames(
                        &group_or_bucket_name,
                        &bucket_id_lower,
                        known_codenames,
                    ),
                    is_weekly: Self::quota_bucket_is_weekly(&window_lower, &bucket_id_lower),
                    remaining_fraction: Some(bucket.remaining_fraction),
                    reset_at,
                });
            }
        }
        inputs
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn quota_bucket_inputs_from_typed(
        groups: &[crate::models::quota::QuotaGroup],
    ) -> Vec<QuotaBucketInput> {
        Self::quota_bucket_inputs_from_typed_with_codenames(groups, &[])
    }

    /// 统一写入三套既有 map：窗口快照 + 短期 reset + 周 reset。
    /// 调用方负责在重建前清空旧 map；本函数只做确定性写入。
    fn apply_quota_bucket_inputs(
        model_quota_windows: &mut HashMap<String, Vec<QuotaWindowStatus>>,
        model_reset_times: &mut HashMap<String, i64>,
        model_weekly_reset_times: &mut HashMap<String, i64>,
        inputs: impl IntoIterator<Item = QuotaBucketInput>,
    ) {
        for input in inputs {
            model_quota_windows
                .entry(input.family.clone())
                .or_default()
                .push(QuotaWindowStatus {
                    is_weekly: input.is_weekly,
                    remaining_fraction: input.remaining_fraction,
                    reset_time: input.reset_at,
                });

            let Some(ts) = input.reset_at else {
                continue;
            };
            let target_map: &mut HashMap<String, i64> = if input.is_weekly {
                &mut *model_weekly_reset_times
            } else {
                &mut *model_reset_times
            };
            target_map
                .entry(input.family.clone())
                .and_modify(|existing| *existing = (*existing).min(ts))
                .or_insert(ts);
            if input.family != "3p" && input.family != "gemini" {
                target_map
                    .entry(format!("gemini-4-{}", input.family))
                    .and_modify(|existing| *existing = (*existing).min(ts))
                    .or_insert(ts);
            }
        }
    }

    /// load_single_account 与实时 429 刷新共用的 Gemini 4+ 分组额度对齐入口：
    /// 1. 对历史 `argon` 维持 #3745 专属组/Ultra 准入门禁与默认档位注入；
    /// 2. 对其余动态代号（如 hydrogen/carbon 或未来任何代号），仅当 quota_groups 恰好拆分了对应专属桶时
    ///    同步账号已解锁变体的配额，绝不凭空注入或覆盖上游 `model_limits`。
    fn reconcile_g4_group_quotas(
        is_ultra: bool,
        bucket_inputs: &[QuotaBucketInput],
        model_quotas: &mut HashMap<String, i32>,
        available_models: &mut HashSet<String>,
        model_limits: &mut HashMap<String, u64>,
    ) {
        const ARGON_DEFAULT_MODELS: [&str; 4] = [
            "gemini-4-argon",
            "gemini-4-argon-high",
            "gemini-4-argon-medium",
            "gemini-4-argon-low",
        ];

        let primary_fraction_for = |family: &str| -> Option<f64> {
            bucket_inputs
                .iter()
                .filter(|b| b.family == family)
                .filter_map(|b| b.remaining_fraction)
                .fold(None, |acc: Option<f64>, f| {
                    Some(acc.map_or(f, |cur| cur.min(f)))
                })
        };

        // 1. Argon 保持既有独立桶 / Ultra 兜底与防呆清理语义 (#3745)
        let argon_frac = primary_fraction_for("argon").or_else(|| {
            if is_ultra {
                Some(primary_fraction_for("gemini").unwrap_or(1.0))
            } else {
                None
            }
        });

        if let Some(frac) = argon_frac {
            let pct = (frac * 100.0).round() as i32;
            for m in ARGON_DEFAULT_MODELS {
                model_quotas.insert(m.to_string(), pct);
                available_models.insert(m.to_string());
                model_limits.entry(m.to_string()).or_insert(65536);
            }
            if available_models.contains("gemini-4-argon-xhigh") {
                model_quotas.insert("gemini-4-argon-xhigh".to_string(), pct);
                model_limits
                    .entry("gemini-4-argon-xhigh".to_string())
                    .or_insert(65536);
            }
        } else {
            for m in [
                "gemini-4-argon",
                "gemini-4-argon-xhigh",
                "gemini-4-argon-high",
                "gemini-4-argon-medium",
                "gemini-4-argon-low",
            ] {
                model_quotas.remove(m);
                available_models.remove(m);
                model_limits.remove(m);
            }
        }

        // 2. 其他 Gemini 4+ 动态代号：若上游为此代号拆出了专属桶，则仅对其已授权变体同步额度
        let mut dedicated_codes: Vec<String> = Vec::new();
        for b in bucket_inputs {
            if b.family != "argon"
                && b.family != "3p"
                && b.family != "gemini"
                && !dedicated_codes.iter().any(|c| c == &b.family)
            {
                dedicated_codes.push(b.family.clone());
            }
        }
        for code in dedicated_codes {
            let Some(frac) = primary_fraction_for(&code) else {
                continue;
            };
            let pct = (frac * 100.0).round() as i32;
            let matched_models: Vec<String> = available_models
                .iter()
                .filter(|m| {
                    crate::proxy::common::model_mapping::extract_gemini4_codename(m).as_deref()
                        == Some(code.as_str())
                })
                .cloned()
                .collect();
            for m in matched_models {
                model_quotas.insert(m, pct);
            }
        }
    }

    async fn load_single_account(&self, path: &PathBuf) -> Result<Option<ProxyToken>, String> {
        let content = std::fs::read_to_string(path).map_err(|e| format!("读取文件失败: {}", e))?;

        let mut account: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| format!("解析 JSON 失败: {}", e))?;

        // 账号级封禁是最高优先级：banned 先于 proxy_disabled / validation / 403。
        if account
            .get("banned")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            tracing::debug!(
                "Account skipped due to upstream ban: {:?} (email={})",
                path,
                account
                    .get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>")
            );
            return Ok(None);
        }

        // 验证锁：按 until 判断；过期后只清验证相关状态，不得误清 hard 403。
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
            let last_used = account
                .get("last_used")
                .and_then(|v| v.as_i64())
                .unwrap_or(now);
            let is_val_active = if block_until == 0 {
                now.saturating_sub(last_used) < 86400
            } else {
                now < block_until
            };

            if is_val_active {
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
            }

            let is_proxy_disabled = account
                .get("proxy_disabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let manual_proxy_disabled = account
                .get("manual_proxy_disabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let proxy_reason = account
                .get("proxy_disabled_reason")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let proxy_kind = if is_proxy_disabled {
                classify_lock_reason(proxy_reason)
            } else {
                AccountLockKind::Manual
            };
            let quota_is_forbidden = account
                .pointer("/quota/is_forbidden")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let quota_reason = account
                .pointer("/quota/forbidden_reason")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let quota_kind = classify_lock_reason(quota_reason);
            let keep_hard_lock = matches!(
                proxy_kind,
                AccountLockKind::Banned | AccountLockKind::HardForbidden
            ) || (quota_is_forbidden
                && matches!(
                    quota_kind,
                    AccountLockKind::Banned | AccountLockKind::HardForbidden
                ));

            account["validation_blocked"] = serde_json::json!(false);
            account["validation_blocked_until"] = serde_json::json!(0);
            account["validation_blocked_reason"] = serde_json::Value::Null;
            account["validation_url"] = serde_json::Value::Null;
            if !keep_hard_lock {
                if let Some(q) = account.get_mut("quota").and_then(|v| v.as_object_mut()) {
                    q.insert("is_forbidden".to_string(), serde_json::json!(false));
                    q.insert("forbidden_reason".to_string(), serde_json::Value::Null);
                }
                if proxy_kind == AccountLockKind::Validation {
                    if manual_proxy_disabled {
                        account["proxy_disabled"] = serde_json::json!(true);
                        account["proxy_disabled_reason"] = serde_json::json!("manual");
                    } else {
                        account["proxy_disabled"] = serde_json::json!(false);
                        account["proxy_disabled_reason"] = serde_json::Value::Null;
                        account["proxy_disabled_at"] = serde_json::Value::Null;
                    }
                }
            }

            update_account_json(path, move |latest| {
                latest["validation_blocked"] = serde_json::json!(false);
                latest["validation_blocked_until"] = serde_json::json!(0);
                latest["validation_blocked_reason"] = serde_json::Value::Null;
                latest["validation_url"] = serde_json::Value::Null;
                if !keep_hard_lock {
                    if let Some(q) = latest.get_mut("quota").and_then(|v| v.as_object_mut()) {
                        q.insert("is_forbidden".to_string(), serde_json::json!(false));
                        q.insert("forbidden_reason".to_string(), serde_json::Value::Null);
                    }
                    if proxy_kind == AccountLockKind::Validation {
                        if manual_proxy_disabled {
                            latest["proxy_disabled"] = serde_json::json!(true);
                            latest["proxy_disabled_reason"] = serde_json::json!("manual");
                        } else {
                            latest["proxy_disabled"] = serde_json::json!(false);
                            latest["proxy_disabled_reason"] = serde_json::Value::Null;
                            latest["proxy_disabled_at"] = serde_json::Value::Null;
                        }
                    }
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

        // 残留/不一致的 quota.is_forbidden：验证类软锁可清；hard 403 与未知原因保守跳过。
        let mut did_migrate_quota = false;
        let quota_is_forbidden = account
            .pointer("/quota/is_forbidden")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if quota_is_forbidden {
            let quota_reason = account
                .pointer("/quota/forbidden_reason")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match classify_lock_reason(quota_reason) {
                AccountLockKind::HardForbidden | AccountLockKind::Banned => {
                    // 自愈检查（审查项 7）：如果 HardForbidden 的 reason 实际上能提取出具体模型 scope，
                    // 说明是老版本遗留的模型级 403 被误写成账号级！自动迁移到 forbidden_models，不跳过该账号！
                    let model_scope = crate::models::account::forbidden_model_scope(quota_reason);
                    if let Some(scope) = model_scope {
                        let mut forbidden_list: Vec<String> = account
                            .get("forbidden_models")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();
                        if !forbidden_list.contains(&scope) {
                            forbidden_list.push(scope.clone());
                        }
                        account["forbidden_models"] = serde_json::json!(forbidden_list);
                        if let Some(q) = account.get_mut("quota").and_then(|v| v.as_object_mut()) {
                            q.insert("is_forbidden".to_string(), serde_json::json!(false));
                            q.insert("forbidden_reason".to_string(), serde_json::Value::Null);
                        }
                        let scope_clone = scope.clone();
                        update_account_json(path, move |latest| {
                            let mut fl: Vec<String> = latest
                                .get("forbidden_models")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_str().map(String::from))
                                        .collect()
                                })
                                .unwrap_or_default();
                            if !fl.contains(&scope_clone) {
                                fl.push(scope_clone);
                            }
                            latest["forbidden_models"] = serde_json::json!(fl);
                            if let Some(q) = latest.get_mut("quota").and_then(|v| v.as_object_mut())
                            {
                                q.insert("is_forbidden".to_string(), serde_json::json!(false));
                                q.insert("forbidden_reason".to_string(), serde_json::Value::Null);
                            }
                        })
                        .await?;
                        did_migrate_quota = true;
                        tracing::info!(
                            "TokenManager: Migrated legacy model 403 to forbidden_models for account {}",
                            account
                                .get("email")
                                .and_then(|v| v.as_str())
                                .unwrap_or("<unknown>")
                        );
                    } else {
                        tracing::debug!(
                            "Account skipped due to quota hard 403: {:?} (email={})",
                            path,
                            account
                                .get("email")
                                .and_then(|v| v.as_str())
                                .unwrap_or("<unknown>")
                        );
                        return Ok(None);
                    }
                }
                AccountLockKind::Validation => {
                    if let Some(q) = account.get_mut("quota").and_then(|v| v.as_object_mut()) {
                        q.insert("is_forbidden".to_string(), serde_json::json!(false));
                        q.insert("forbidden_reason".to_string(), serde_json::Value::Null);
                    }
                    update_account_json(path, |latest| {
                        if let Some(q) = latest.get_mut("quota").and_then(|v| v.as_object_mut()) {
                            q.insert("is_forbidden".to_string(), serde_json::json!(false));
                            q.insert("forbidden_reason".to_string(), serde_json::Value::Null);
                        }
                    })
                    .await?;
                }
                _ => {
                    tracing::debug!(
                        "Account skipped due to unresolved quota.is_forbidden: {:?} (email={})",
                        path,
                        account
                            .get("email")
                            .and_then(|v| v.as_str())
                            .unwrap_or("<unknown>")
                    );
                    return Ok(None);
                }
            }
        }

        // proxy_disabled：系统锁（403/验证/封禁）不加载；quota_protection 交给保护逻辑；
        // 其余视为手动禁用。
        let is_proxy_disabled = account
            .get("proxy_disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let manual_proxy_disabled = account
            .get("manual_proxy_disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let disabled_reason = account
            .get("proxy_disabled_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if is_proxy_disabled {
            match classify_lock_reason(disabled_reason) {
                AccountLockKind::QuotaProtection => {}
                AccountLockKind::Banned => {
                    tracing::debug!(
                        "Account skipped due to banned proxy reason: {:?} (email={}, reason={})",
                        path,
                        account
                            .get("email")
                            .and_then(|v| v.as_str())
                            .unwrap_or("<unknown>"),
                        disabled_reason
                    );
                    return Ok(None);
                }
                AccountLockKind::HardForbidden => {
                    // 自愈检查（审查项 1 & 7）：
                    // 仅当 proxy 禁用原因自身明确属于模型级 403 时，才自愈迁移至 forbidden_models。
                    // 关键守卫：若用户此前明确手动禁用了账号 (manual_proxy_disabled == true)，
                    // 即使迁移了模型冻结，账号也必须继续保持 proxy_disabled = true，绝不越权恢复用户手动关闭的账号！
                    let proxy_model_scope =
                        crate::models::account::forbidden_model_scope(disabled_reason);
                    let has_migrated_from_quota = did_migrate_quota;

                    if let Some(scope) = proxy_model_scope {
                        let mut fl: Vec<String> = account
                            .get("forbidden_models")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();
                        if !fl.contains(&scope) {
                            fl.push(scope.clone());
                        }
                        account["forbidden_models"] = serde_json::json!(fl);

                        if manual_proxy_disabled {
                            account["proxy_disabled"] = serde_json::json!(true);
                            account["proxy_disabled_reason"] = serde_json::json!("manual");
                            let scope_clone = scope.clone();
                            update_account_json(path, move |latest| {
                                let mut fl: Vec<String> = latest
                                    .get("forbidden_models")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|v| v.as_str().map(String::from))
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                if !fl.contains(&scope_clone) {
                                    fl.push(scope_clone);
                                }
                                latest["forbidden_models"] = serde_json::json!(fl);
                                latest["proxy_disabled"] = serde_json::json!(true);
                                latest["proxy_disabled_reason"] = serde_json::json!("manual");
                            })
                            .await?;
                            tracing::info!(
                                "TokenManager: Migrated legacy model 403 to forbidden_models but kept manual proxy_disabled for account {}",
                                account
                                    .get("email")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("<unknown>")
                            );
                            return Ok(None);
                        } else {
                            account["proxy_disabled"] = serde_json::json!(false);
                            account["proxy_disabled_reason"] = serde_json::Value::Null;
                            account["proxy_disabled_at"] = serde_json::Value::Null;
                            let scope_clone = scope.clone();
                            update_account_json(path, move |latest| {
                                let mut fl: Vec<String> = latest
                                    .get("forbidden_models")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|v| v.as_str().map(String::from))
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                if !fl.contains(&scope_clone) {
                                    fl.push(scope_clone);
                                }
                                latest["forbidden_models"] = serde_json::json!(fl);
                                latest["proxy_disabled"] = serde_json::json!(false);
                                latest["proxy_disabled_reason"] = serde_json::Value::Null;
                                latest["proxy_disabled_at"] = serde_json::Value::Null;
                            })
                            .await?;
                            if let Some(aid) = account.get("id").and_then(|v| v.as_str()) {
                                let _ = crate::modules::account::set_account_index_proxy_disabled(
                                    aid, false,
                                );
                            }
                            tracing::info!(
                                "TokenManager: Healed legacy proxy_disabled for model-scoped 403 on account {}",
                                account
                                    .get("email")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("<unknown>")
                            );
                        }
                    } else if has_migrated_from_quota && !manual_proxy_disabled {
                        // 审查项 7：proxy_disabled_reason 是泛化的 "Forbidden (403)"，但已在 quota 处成功自愈模型 scope
                        account["proxy_disabled"] = serde_json::json!(false);
                        account["proxy_disabled_reason"] = serde_json::Value::Null;
                        account["proxy_disabled_at"] = serde_json::Value::Null;
                        update_account_json(path, |latest| {
                            latest["proxy_disabled"] = serde_json::json!(false);
                            latest["proxy_disabled_reason"] = serde_json::Value::Null;
                            latest["proxy_disabled_at"] = serde_json::Value::Null;
                        })
                        .await?;
                        if let Some(aid) = account.get("id").and_then(|v| v.as_str()) {
                            let _ = crate::modules::account::set_account_index_proxy_disabled(
                                aid, false,
                            );
                        }
                        tracing::info!(
                            "TokenManager: Healed generic proxy_disabled associated with migrated model 403 on account {}",
                            account
                                .get("email")
                                .and_then(|v| v.as_str())
                                .unwrap_or("<unknown>")
                        );
                    } else {
                        tracing::debug!(
                            "Account skipped due to hard 403 proxy reason: {:?} (email={}, reason={})",
                            path,
                            account
                                .get("email")
                                .and_then(|v| v.as_str())
                                .unwrap_or("<unknown>"),
                            disabled_reason
                        );
                        return Ok(None);
                    }
                }
                AccountLockKind::Validation => {
                    tracing::debug!(
                        "Account skipped due to validation proxy reason: {:?} (email={})",
                        path,
                        account
                            .get("email")
                            .and_then(|v| v.as_str())
                            .unwrap_or("<unknown>")
                    );
                    return Ok(None);
                }
                AccountLockKind::Manual => {
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

        let forbidden_models: HashSet<String> = account
            .get("forbidden_models")
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

        // [NEW] 构建各模型及各组专属刷新时间戳缓存（消灭跨模型截止时间污染）
        let mut model_reset_times: HashMap<String, i64> = HashMap::new();
        let mut model_weekly_reset_times: HashMap<String, i64> = HashMap::new();
        let mut model_quota_windows: HashMap<String, Vec<QuotaWindowStatus>> = HashMap::new();

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
                    let pct_val = pct as i32;
                    // Normalize name to standard ID
                    let standard_id =
                        crate::proxy::common::model_mapping::normalize_to_standard_id(name)
                            .unwrap_or_else(|| name.to_string());
                    // 粗粒度标准组归一化聚合时必须取组内最大可用配额，杜绝单模型 0 配额覆盖整组正额度
                    model_quotas
                        .entry(standard_id.clone())
                        .and_modify(|existing: &mut i32| *existing = (*existing).max(pct_val))
                        .or_insert(pct_val);
                    // 同时按具体模型原始 ID 及小写记录独立配额，确保具名查询精确可用
                    model_quotas.insert(name.to_string(), pct_val);
                    let name_lower = name.to_lowercase();
                    model_quotas.insert(name_lower.clone(), pct_val);
                    // 自动关联同族基底模型名（如 gemini-3.8-flash-high -> gemini-3.8-flash），取组内最高配额
                    let base_name =
                        crate::proxy::common::model_mapping::strip_tier_suffix(&name_lower);
                    if base_name != name_lower {
                        model_quotas
                            .entry(base_name.clone())
                            .and_modify(|existing: &mut i32| *existing = (*existing).max(pct_val))
                            .or_insert(pct_val);
                        available_models.insert(base_name.clone());
                    }
                    // [NEW] 同时记录原始模型名（小写），用于精确 / 基名路由匹配
                    available_models.insert(name_lower.clone());

                    // [NEW] 提取具体模型的官方权威 reset_time
                    if let Some(r_str) = model.get("reset_time").and_then(|r| r.as_str()) {
                        if !r_str.is_empty() {
                            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(r_str) {
                                let ts = dt.timestamp();
                                model_reset_times.insert(name.to_string(), ts);
                                model_reset_times.insert(name_lower.clone(), ts);
                                if base_name != name_lower {
                                    model_reset_times.insert(base_name.clone(), ts);
                                }
                                model_reset_times.insert(standard_id.clone(), ts);
                            }
                        }
                    }
                }
                // [NEW] 解析并缓存 max_output_tokens (动态优先，同时同步小写与剥离档位后缀的派生裸模型)
                if let (Some(name), Some(limit)) = (
                    model.get("name").and_then(|v| v.as_str()),
                    model.get("max_output_tokens").and_then(|v| v.as_u64()),
                ) {
                    model_limits.insert(name.to_string(), limit);
                    let name_lower = name.trim().to_lowercase();
                    if name_lower != name {
                        model_limits.entry(name_lower.clone()).or_insert(limit);
                    }
                    let base_name =
                        crate::proxy::common::model_mapping::strip_tier_suffix(&name_lower);
                    if base_name != name_lower {
                        model_limits
                            .entry(base_name)
                            .and_modify(|existing| *existing = (*existing).max(limit))
                            .or_insert(limit);
                    }
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

        let quota_val = account.get("quota");
        let groups_val = quota_val
            .and_then(|q| q.get("quota_groups").or_else(|| q.get("quotaGroups")))
            .and_then(|g| g.as_array());

        let known_codenames =
            Self::collect_known_g4_codenames(available_models.iter().map(String::as_str));
        let parsed_bucket_inputs = groups_val.map(|groups| {
            Self::quota_bucket_inputs_from_json_with_codenames(groups, &known_codenames)
        });

        if let Some(ref inputs) = parsed_bucket_inputs {
            let is_ultra = subscription_tier
                .as_deref()
                .map(|s| s.to_uppercase() == "ULTRA")
                .unwrap_or(false);
            Self::reconcile_g4_group_quotas(
                is_ultra,
                inputs,
                &mut model_quotas,
                &mut available_models,
                &mut model_limits,
            );
        }

        // 若 model_quotas 中已有 gemini-4-argon（例如通过 models 列表或上游配额注入），确保同步到 available_models 与 limits
        if model_quotas.contains_key("gemini-4-argon") {
            for m in [
                "gemini-4-argon",
                "gemini-4-argon-high",
                "gemini-4-argon-medium",
                "gemini-4-argon-low",
            ] {
                available_models.insert(m.to_string());
                model_limits.entry(m.to_string()).or_insert(65536);
            }
        }

        // [NEW] 从配额桶分组精确提取周周期与短期周期重置时间及窗口快照（模型大类组级隔离，P1-2, P1-3）
        // 写入入口统一收敛到 apply_quota_bucket_inputs，避免 load / 实时刷新各写一套逻辑。
        if let Some(inputs) = parsed_bucket_inputs {
            Self::apply_quota_bucket_inputs(
                &mut model_quota_windows,
                &mut model_reset_times,
                &mut model_weekly_reset_times,
                inputs,
            );
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
            model_priorities: account
                .get("model_priorities")
                .cloned()
                .map(crate::models::account::deserialize_model_priorities)
                .transpose()
                .map_err(|e| format!("invalid account model_priorities: {}", e))?
                .unwrap_or_default(),
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
            forbidden_models,
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
            model_reset_times,
            model_weekly_reset_times,
            model_quota_windows,
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

    /// 账号 JSON 中模型清单的解析缓存（按文件长度 + mtime 失效）。
    ///
    /// 模型门禁与 `/v1/models` 会在**每个请求**上汇总全池模型，历史实现每次都
    /// `read_to_string + serde_json::from_str` 每个账号文件。这里只缓存解析结果，
    /// 文件变化（长度或 mtime）时立即失效，不引入 TTL 语义漂移。
    fn get_available_models_from_json(account_path: &PathBuf) -> Option<HashSet<String>> {
        let meta = std::fs::metadata(account_path).ok()?;
        let len = meta.len();
        let modified = meta.modified().ok();

        if let Some(hit) = ACCOUNT_MODEL_FILE_CACHE.get(account_path) {
            if hit.len == len && hit.modified == modified {
                return hit.models.clone();
            }
        }

        let models = Self::parse_available_models_from_json(account_path);
        if ACCOUNT_MODEL_FILE_CACHE.len() > 4096 {
            ACCOUNT_MODEL_FILE_CACHE.clear();
        }
        ACCOUNT_MODEL_FILE_CACHE.insert(
            account_path.clone(),
            AccountModelFileSnapshot {
                len,
                modified,
                models: models.clone(),
            },
        );
        models
    }

    fn parse_available_models_from_json(account_path: &PathBuf) -> Option<HashSet<String>> {
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
        for suffix in ["tiered", "xhigh", "medium", "high", "low", "thinking"] {
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

    /// 模型族分组 Key 解析（支持未来任意 Gemini 4+ 代号动态分组）
    pub fn resolve_model_group_key(model: &str) -> String {
        let m_lower = model.trim().to_lowercase();
        if let Some(code) = crate::proxy::common::model_mapping::extract_gemini4_codename(&m_lower)
        {
            code
        } else if m_lower.starts_with("claude")
            || m_lower.starts_with("gpt")
            || m_lower.contains("3p")
        {
            "3p".to_string()
        } else {
            "gemini".to_string()
        }
    }

    /// 检查非 Argon 的 Gemini 4+ 代号在当前账号上是否拆分出了专属配额组（避免误落通用池或错失回退）。
    fn has_dedicated_codename_bucket(t: &ProxyToken, codename: &str) -> bool {
        t.model_quota_windows.contains_key(codename)
            || t.model_reset_times.contains_key(codename)
            || t.model_weekly_reset_times.contains_key(codename)
    }

    /// 解析指定模型的重置时间戳（支持短期滚动与周周期窗口）
    ///
    /// 核心契约（防跨模型时间污染，Ref #3840, #3845）：
    /// 1. 两个 per-model map 任一非空时，严格在对应 map 中按「具体模型名 -> 小写 -> 基名 -> 标准 ID -> 族群 Key」匹配；
    ///    若未检索到，绝对不回退全局兜底 reset_time（防止如 Argon 到期污染 Gemini 模型）；
    ///    仅对**未拆分专属桶**的非 Argon Gemini 4+ 模型允许回退到共享的 `gemini` 大桶。
    /// 2. 仅当两个 map 都为空时（旧版数据或无多模型隔离），才允许回退全局兜底时间。
    pub fn resolve_model_reset_time(t: &ProxyToken, model: &str, is_weekly: bool) -> Option<i64> {
        let map = if is_weekly {
            &t.model_weekly_reset_times
        } else {
            &t.model_reset_times
        };
        let any_model_map_populated =
            !t.model_reset_times.is_empty() || !t.model_weekly_reset_times.is_empty();
        let group_key = Self::resolve_model_group_key(model);

        let model_lower = model.trim().to_lowercase();
        let base_name = crate::proxy::common::model_mapping::strip_tier_suffix(&model_lower);
        let standard_id = crate::proxy::common::model_mapping::normalize_to_standard_id(model);

        let matched = map
            .get(model)
            .or_else(|| map.get(&model_lower))
            .or_else(|| {
                if base_name != model_lower {
                    map.get(&base_name)
                } else {
                    None
                }
            })
            .or_else(|| standard_id.as_deref().and_then(|sid| map.get(sid)))
            .or_else(|| map.get(group_key.as_str()))
            .or_else(|| {
                if group_key != "argon"
                    && group_key != "3p"
                    && group_key != "gemini"
                    && !Self::has_dedicated_codename_bucket(t, &group_key)
                {
                    map.get("gemini")
                } else {
                    None
                }
            })
            .copied();

        if matched.is_some() {
            matched
        } else if !any_model_map_populated {
            if is_weekly {
                t.weekly_reset_time
            } else {
                t.reset_time
            }
        } else {
            None
        }
    }

    /// 同时解析指定模型的短期与周周期重置时间
    pub fn resolve_model_reset_times(t: &ProxyToken, model: &str) -> (Option<i64>, Option<i64>) {
        (
            Self::resolve_model_reset_time(t, model, false),
            Self::resolve_model_reset_time(t, model, true),
        )
    }

    /// 解析指定模型的配额桶详细窗口快照
    pub fn resolve_model_quota_windows<'a>(
        t: &'a ProxyToken,
        model: &str,
    ) -> Option<&'a Vec<QuotaWindowStatus>> {
        if t.model_quota_windows.is_empty() {
            return None;
        }
        let group_key = Self::resolve_model_group_key(model);
        let model_lower = model.trim().to_lowercase();
        let base_name = crate::proxy::common::model_mapping::strip_tier_suffix(&model_lower);
        let standard_id = crate::proxy::common::model_mapping::normalize_to_standard_id(model);

        t.model_quota_windows
            .get(model)
            .or_else(|| t.model_quota_windows.get(&model_lower))
            .or_else(|| {
                if base_name != model_lower {
                    t.model_quota_windows.get(&base_name)
                } else {
                    None
                }
            })
            .or_else(|| {
                standard_id
                    .as_deref()
                    .and_then(|sid| t.model_quota_windows.get(sid))
            })
            .or_else(|| t.model_quota_windows.get(group_key.as_str()))
            .or_else(|| {
                if group_key != "argon"
                    && group_key != "3p"
                    && group_key != "gemini"
                    && !Self::has_dedicated_codename_bucket(t, &group_key)
                {
                    t.model_quota_windows.get("gemini")
                } else {
                    None
                }
            })
    }

    /// 检查指定模型的配额耗尽 (0%) 状态是否已经过期（上游已重置进入新周期）
    ///
    /// 严格遵循 P0-1 / P0-2 工程契约：
    /// 1. Weekly 硬配额优先：若 weekly 窗口耗尽 (remaining_fraction <= 0.001)，
    ///    必须等待 weekly reset 到达；即使 5h 短期 reset 到达也严禁自愈；
    /// 2. 模型族隔离无污染：map 非空时不回退到全局 reset，杜绝其他族 reset 误判恢复；
    /// 3. 所有耗尽窗口均越过重置时间戳时，才判定为已恢复。
    pub fn is_model_quota_depleted_expired(t: &ProxyToken, model: &str) -> bool {
        let now_ts = chrono::Utc::now().timestamp();

        // 1. 优先检查详细的窗口快照 (model_quota_windows)
        // 核心契约：筛选出所有已耗尽 (remaining_fraction <= 0.001) 的桶（无论 weekly 还是 5h），
        // 杜绝使用 .find() 只取第一个周桶而漏判其它周桶；必须满足所有耗尽桶的 reset_time 均已越过当前物理时间，才判定自愈。
        if let Some(buckets) = Self::resolve_model_quota_windows(t, model) {
            if !buckets.is_empty() {
                let depleted_buckets: Vec<_> = buckets
                    .iter()
                    .filter(|b| b.remaining_fraction.is_some_and(|f| f <= 0.001))
                    .collect();

                if !depleted_buckets.is_empty() {
                    return depleted_buckets
                        .iter()
                        .all(|b| b.reset_time.is_some_and(|r| now_ts >= r));
                }
            }
        }

        // 2. 回退到 resolve_model_reset_time（基于模型或家族的隔离 reset 时间戳）
        let (short_reset, weekly_reset) = Self::resolve_model_reset_times(t, model);

        if short_reset.is_none() && weekly_reset.is_none() {
            return false;
        }

        // 若存在周重置时间且仍在未来，weekly 硬配额未过，不得自愈
        if let Some(w) = weekly_reset {
            if now_ts < w {
                return false;
            }
        }

        // 若存在短期重置时间且仍在未来，不得自愈
        if let Some(r) = short_reset {
            if now_ts < r {
                return false;
            }
        }

        true
    }

    /// 获取模型原始在存配额（百分比）
    pub fn get_raw_model_quota(t: &ProxyToken, normalized_target: &str) -> i32 {
        if let Some(&q) = t.model_quotas.get(normalized_target) {
            return q;
        }
        if let Some(std_id) =
            crate::proxy::common::model_mapping::normalize_to_standard_id(normalized_target)
        {
            if let Some(&q) = t.model_quotas.get(&std_id) {
                return q;
            }
        }
        let m_lower = normalized_target.trim().to_lowercase();
        if let Some(&q) = t.model_quotas.get(&m_lower) {
            return q;
        }
        let base = crate::proxy::common::model_mapping::strip_tier_suffix(&m_lower);
        if base != m_lower {
            if let Some(&q) = t.model_quotas.get(&base) {
                return q;
            }
        }
        t.remaining_quota.unwrap_or(0)
    }

    /// 判定候选账号是否具备目标模型的调度准入资格（准入与排序解耦，P1-4）
    pub fn has_candidate_quota_admission(t: &ProxyToken, normalized_target: &str) -> bool {
        let q = Self::get_raw_model_quota(t, normalized_target);
        if q > 0 {
            return true;
        }
        if q == 0 && Self::is_model_quota_depleted_expired(t, normalized_target) {
            return true;
        }
        false
    }

    /// 获取候选账号针对目标模型的有效配额（自愈账号赋予正向 sentinel，排序不压制健康高额度账号）
    pub fn get_candidate_quota(t: &ProxyToken, normalized_target: &str) -> i32 {
        let q = Self::get_raw_model_quota(t, normalized_target);
        if q > 0 {
            return q;
        }
        if q == 0 && Self::is_model_quota_depleted_expired(t, normalized_target) {
            // 自愈账号：使用保守正向 sentinel (20)，参与调度竞争但排序不压制健康高配额 (80%~100%) 账号
            return 20;
        }
        q
    }

    /// 精确计算账号针对特定模型大类的临期截止时间戳（彻底杜绝跨模型组时间污染）
    pub fn get_imminent_deadline_for_model(
        t: &ProxyToken,
        model: &str,
        q: i32,
        now_ts: i64,
    ) -> Option<i64> {
        if q <= Self::IMMINENT_QUOTA_FLOOR {
            return None;
        }

        let (short_val, weekly_val) = Self::resolve_model_reset_times(t, model);

        if let Some(w) = weekly_val {
            if w > now_ts && (w - now_ts) <= Self::WEEKLY_IMMINENT_HORIZON_SECS {
                return Some(w);
            }
        }

        if let Some(r) = short_val {
            if r > now_ts && (r - now_ts) <= Self::SHORT_IMMINENT_HORIZON_SECS {
                return Some(r);
            }
        }

        None
    }

    /// 计算账号针对目标模型的临期截止时间戳。
    /// 若配额已处于濒危水平 (<= 10%) 或未进入临期窗口，则返回 None。
    pub fn get_imminent_deadline(t: &ProxyToken, q: i32, now_ts: i64) -> Option<i64> {
        Self::get_imminent_deadline_for_model(t, "", q, now_ts)
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
            // 排序规则：当前模型的生效优先级数字越小越优先；相同优先级下，在途请求数越小越优先；最后按 account_id 稳定排序
            sorted_pool.sort_by(|a, b| {
                a.effective_priority(normalized_target, normalized_target)
                    .cmp(&b.effective_priority(normalized_target, normalized_target))
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

        // [FIX #3840] 目标模型正配额过滤必须**先于**优先级收敛执行。
        //
        // 为什么顺序不能反：下面的优先级收敛会把 available 直接收缩到
        // 「最高优先级的空闲账号组」（available = min_p_idle 是赋值覆盖，
        // 不是 retain）。若该组恰好是一个配额已耗尽的临期账号
        // （蹬额度模式 pedal 会把它调到 priority=1，用户手动设高优先级同样可触发），
        // 就会出现经典的优先级倒置 / 饥饿死锁：
        //   1. 其他 priority=50 但配额 100% 的账号被提前剔除；
        //   2. 随后 has_positive_model_quota 只能看到那个 0% 账号；
        //   3. 于是直接 return None，报「无可用账号」——即使池中还有满额账号。
        //
        // 先做配额过滤，等于确立了调度器的核心不变式：
        // 「优先级只在具备该模型有效正配额的健康账号之间竞争；
        //   配额已知为 0% 的账号从一开始就不具备参与优先级裁决的资格。」
        //
        // 注：max_spillover 截断与 P2C 都在本步之后，语义不受影响——
        // 被提前剔除的本就是不该参与并发调度的零配额账号。
        let has_positive_model_quota = available
            .iter()
            .any(|t| Self::has_candidate_quota_admission(t, normalized_target));
        if has_positive_model_quota {
            available.retain(|t| Self::has_candidate_quota_admission(t, normalized_target));
        } else {
            // 全池候选账号确实均已配额耗尽：本地拦下，杜绝裸奔打爆 Google 429。
            tracing::warn!(
                "[Scheduler] All {} candidate accounts have 0% quota for model {}. Intercepting upstream call to protect pool.",
                available.len(),
                normalized_target
            );
            return None;
        }

        // Keep lower-priority groups for retries; only this draw is restricted.
        // [NEW] 并发感知与次级分流加速 (Concurrency-Aware & Priority Spillover Acceleration)：
        // 1. 获取当前可用池中针对目标模型的最高生效优先级 min_priority
        let eff_p = |t: &ProxyToken| t.effective_priority(normalized_target, normalized_target);
        if let Some(min_priority) = available.iter().map(|t| eff_p(t)).min() {
            // 检查最高优先级是否有完全空闲（在途请求数 == 0）的健康账号
            let min_p_idle: Vec<&ProxyToken> = available
                .iter()
                .filter(|t| eff_p(t) == min_priority && self.get_in_flight(&t.account_id) == 0)
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
                    .filter(|t| eff_p(t) > min_priority && self.get_in_flight(&t.account_id) == 0)
                    .copied()
                    .collect();

                if let Some(next_min_p) = lower_idle.iter().map(|t| eff_p(t)).min() {
                    // 🚀 触发多会话分流加速：新会话平滑外溢到次高优先级的空闲账号组，打破单号串行排队阻塞！
                    tracing::info!(
                        "🚀 [Concurrency Spillover] Priority {} accounts are busy (in_flight > 0). Offloading new conversation to idle priority {} account",
                        min_priority, next_min_p
                    );
                    available = lower_idle
                        .into_iter()
                        .filter(|t| eff_p(t) == next_min_p)
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
                    if let Some(best_p) = available.iter().map(|t| eff_p(t)).min() {
                        available.retain(|t| eff_p(t) == best_p);
                    }
                }
            }
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

        // 目标模型配额提取（具备候选自愈与通用配额兜底）
        let q1 = Self::get_candidate_quota(c1, normalized_target);
        let q2 = Self::get_candidate_quota(c2, normalized_target);

        // P2C 临期感知选择：若候选账号存在临期充足配额 (<= 24h 重置且 q > 10%)，优先抽中该临期账号防止配额作废
        let now_ts = chrono::Utc::now().timestamp();
        let imm1 = Self::get_imminent_deadline_for_model(c1, normalized_target, q1, now_ts);
        let imm2 = Self::get_imminent_deadline_for_model(c2, normalized_target, q2, now_ts);

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
                            let is_cooldown =
                                crate::proxy::pipeline::policy::UpstreamClassification::is_internal_gateway_exhaustion(
                                    &selection_error.to_lowercase(),
                                );
                            return Err((
                                if is_cooldown {
                                    StatusCode::TOO_MANY_REQUESTS
                                } else {
                                    StatusCode::SERVICE_UNAVAILABLE
                                },
                                if is_cooldown {
                                    selection_error
                                } else {
                                    format!("Token error: {}", selection_error)
                                },
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

    pub async fn get_token_filtered(
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

    /// 扫描磁盘上的账号文件，统计封禁/禁用/可用数量（用于池空诊断）
    /// 返回 (banned, disabled, available)
    fn scan_pool_disk_state(&self) -> (usize, usize, usize) {
        let accounts_dir = self.resolved_data_dir().join("accounts");
        let mut banned = 0usize;
        let mut disabled = 0usize;
        let mut available = 0usize;

        let Ok(entries) = std::fs::read_dir(&accounts_dir) else {
            return (0, 0, 0);
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(account) = serde_json::from_str::<serde_json::Value>(&content) else {
                continue;
            };
            // 封禁账号：上游 403 (TOS_VIOLATION 等) 由 mark_account_forbidden 标记 quota.is_forbidden=true
            let is_banned = account
                .pointer("/quota/is_forbidden")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let reason = account
                .get("proxy_disabled_reason")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let is_disabled = account
                .get("disabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
                || (account
                    .get("proxy_disabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                    && reason != "quota_protection");
            if is_banned {
                banned += 1;
            } else if is_disabled {
                disabled += 1;
            } else {
                available += 1;
            }
        }
        (banned, disabled, available)
    }

    /// 池空时返回可区分原因的诊断错误，避免误导用户"等待冷却"
    /// (#3919: 全池 banned 后应明确提示封禁而非 429 "Token pool is empty")
    fn empty_pool_error(&self) -> String {
        let (banned, disabled, available) = self.scan_pool_disk_state();
        if banned > 0 {
            format!(
                "No available accounts: all pool accounts are banned or disabled (available: {}, banned: {}, disabled: {}). Banned accounts (upstream 403, e.g. TOS_VIOLATION) will NOT recover by waiting for cooldown - check the Accounts page and submit an appeal.",
                available, banned, disabled
            )
        } else if available == 0 && disabled > 0 {
            format!(
                "No available accounts: all {} account(s) are disabled (banned: 0, disabled: {}). Re-enable accounts in the Accounts page.",
                disabled, disabled
            )
        } else if available > 0 {
            format!(
                "Token pool is empty: {} usable account(s) exist on disk but none are loaded into the proxy pool - refresh the account pool.",
                available
            )
        } else {
            "Token pool is empty: no accounts configured. Add accounts in the Accounts page."
                .to_string()
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
        let now_ts = chrono::Utc::now().timestamp();
        let mut tokens_snapshot: Vec<ProxyToken> = self
            .tokens
            .iter()
            .map(|e| e.value().clone())
            .filter(|token| {
                if token.validation_blocked
                    && (token.validation_blocked_until == 0
                        || token.validation_blocked_until > now_ts)
                {
                    return false;
                }
                true
            })
            .collect();
        tokens_snapshot.retain(|token| !excluded_accounts.contains(&token.account_id));
        let mut total = tokens_snapshot.len();
        if total == 0 {
            return Err(self.empty_pool_error());
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
                    return Err(self.empty_pool_error());
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

        // 模型级 403 冻结：只排除目标模型，不影响其他模型。
        tokens_snapshot.retain(|t| !t.is_model_forbidden(target_model));
        if tokens_snapshot.is_empty() {
            tracing::warn!(
                "All candidate accounts are forbidden for model: {}",
                target_model
            );
            return Err(format!(
                "All accounts are forbidden for model: {}",
                target_model
            ));
        }

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
            return Err(self.empty_pool_error());
        }

        tokens_snapshot.sort_by(|a, b| {
            let priority_cmp = a
                .effective_priority(target_model, &normalized_target)
                .cmp(&b.effective_priority(target_model, &normalized_target));
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

            // 提取目标模型配额（具备候选自愈与通用配额兜底）
            let quota_a = Self::get_candidate_quota(a, &normalized_target);
            let quota_b = Self::get_candidate_quota(b, &normalized_target);

            // [NEW] Priority 0.5: 临期配额优先消耗 (Earliest Deadline First / Use-It-Or-Lose-It)
            // 针对 Gemini / Claude 账号池的核心经济学优化：
            // Google 对 Pro/Ultra 账号设置了周额度上限（7d 窗口）和 5h 滚动窗口。
            // 当一个账号的周额度即将重置（例如 < 24 小时）且该账号仍有充足可用额度 (> 10%) 时，
            // 若不尽快消耗该账号，未用配额将在周期重置时被彻底清零作废！
            // 相反，重置时间还遥遥无期（如 5~7 天后）的账号即使配额多，也应当退居其后，让临期配额先行释放。
            // 注意：若账号剩余额度已濒危（<= 10%），绝不可被提权抢占健康账号，
            // 否则会快速耗尽单次请求的 429 容错重试配额，导致健康账号未被尝试即整体报错。
            let now_ts = chrono::Utc::now().timestamp();
            let imminent_a =
                Self::get_imminent_deadline_for_model(a, &normalized_target, quota_a, now_ts);
            let imminent_b =
                Self::get_imminent_deadline_for_model(b, &normalized_target, quota_b, now_ts);

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
                                return Err(self.empty_pool_error());
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
                                return Err(self.empty_pool_error());
                            }
                        }
                        OnDiskAccountState::Enabled => {
                            let normalized_target =
                                crate::proxy::common::model_mapping::normalize_to_standard_id(
                                    target_model,
                                )
                                .unwrap_or_else(|| target_model.to_string());

                            let is_validation_blocked = preferred_token.validation_blocked
                                && (preferred_token.validation_blocked_until == 0
                                    || preferred_token.validation_blocked_until
                                        > chrono::Utc::now().timestamp());

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

                            // [FIX #3639] 检查优先账号的目标模型配额：多键容错匹配，防止因 key 差异误判满额
                            let preferred_quota = preferred_token
                                .model_quotas
                                .get(&normalized_target)
                                .or_else(|| preferred_token.model_quotas.get(target_model))
                                .copied();
                            let has_zero_quota = preferred_quota.is_some_and(|q| q <= 0);
                            let has_healthy_alternative = tokens_snapshot.iter().any(|t| {
                                t.account_id != preferred_token.account_id
                                    && !self
                                        .rate_limit_tracker
                                        .is_rate_limited(&t.account_id, Some(&normalized_target))
                                    && t.model_quotas
                                        .get(&normalized_target)
                                        .or_else(|| t.model_quotas.get(target_model))
                                        .copied()
                                        .unwrap_or(0)
                                        > 0
                            });

                            if is_validation_blocked {
                                tracing::warn!(
                                    "🔒 [Validation-Blocked] Preferred account {} is blocked for validation, seamlessly falling back to available account in pool",
                                    preferred_token.email
                                );
                            } else if has_zero_quota && has_healthy_alternative {
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
                        let bound_quota = bound_token
                            .model_quotas
                            .get(&normalized_target)
                            .or_else(|| bound_token.model_quotas.get(target_model))
                            .copied()
                            .unwrap_or(100);
                        let bound_zero_quota = bound_quota <= 0;

                        let has_healthy_alternative = tokens_snapshot.iter().any(|t| {
                            t.account_id != bound_token.account_id
                                && !attempted.contains(&t.account_id)
                                && !self
                                    .rate_limit_tracker
                                    .is_rate_limited(&t.account_id, Some(&normalized_target))
                                && !(quota_protection_enabled
                                    && t.protected_models.contains(&normalized_target))
                                && !self.qos_guardian.is_jailed_or_probing(&t.account_id)
                                && !(t.validation_blocked
                                    && (t.validation_blocked_until == 0
                                        || t.validation_blocked_until
                                            > chrono::Utc::now().timestamp()))
                                && t.model_quotas
                                    .get(&normalized_target)
                                    .or_else(|| t.model_quotas.get(target_model))
                                    .copied()
                                    .unwrap_or(0)
                                    > 0
                        });

                        let is_qos_jailed = self
                            .qos_guardian
                            .is_jailed_or_probing(&bound_token.account_id);
                        let is_validation_blocked = bound_token.validation_blocked
                            && (bound_token.validation_blocked_until == 0
                                || bound_token.validation_blocked_until
                                    > chrono::Utc::now().timestamp());

                        if is_validation_blocked {
                            tracing::warn!(
                                "Sticky Session: Bound account {} is VALIDATION BLOCKED, unbinding and switching.",
                                bound_token.email
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
                            attempted.insert(bound_token.account_id.clone());
                        } else if quota_protection_enabled
                            && bound_token.protected_models.contains(&normalized_target)
                        {
                            tracing::debug!("Sticky Session: Bound account {} is quota-protected for model {} [{}], unbinding and switching.", bound_token.email, normalized_target, target_model);
                            self.session_accounts.remove(sid);
                        } else if attempted.contains(&bound_id) {
                            // [FIX] 绑定的账号在当前轮次请求中已尝试失败，立即解绑避免死锁
                            tracing::debug!("Sticky Session: Bound account {} already attempted in current request, unbinding", bound_token.email);
                            self.session_accounts.remove(sid);
                        } else if is_qos_jailed {
                            if scheduling.mode == SchedulingMode::CacheFirst {
                                // CacheFirst（缓存优先）：触发限速小黑屋时绝不解绑会话换号，按最大等待时长原地精准等待冷却后继续复用，保住昂贵 KV Cache
                                let rem_secs = self
                                    .qos_guardian
                                    .get_jailed_remaining_seconds(&bound_token.account_id)
                                    .unwrap_or(0);
                                let capped_wait = if scheduling.max_wait_seconds > 0 {
                                    rem_secs.min(scheduling.max_wait_seconds)
                                } else {
                                    0
                                };
                                let wait_secs =
                                    match crate::proxy::pipeline::budget::remaining_secs() {
                                        Some(rem_budget) => {
                                            capped_wait.min(rem_budget.saturating_sub(30))
                                        }
                                        None => capped_wait,
                                    };
                                if wait_secs > 0 {
                                    tracing::info!(
                                        "Sticky Session (CacheFirst): Bound account {} is in QoS Jail ({}s remaining), waiting {}s in-place to preserve session binding and KV cache...",
                                        bound_token.email,
                                        rem_secs,
                                        wait_secs
                                    );
                                    tokio::time::sleep(std::time::Duration::from_secs(wait_secs))
                                        .await;
                                }
                                self.qos_guardian
                                    .promote_to_half_open(&bound_token.account_id);
                                tracing::info!(
                                    "Sticky Session: Successfully reusing bound account {} for session {} after QoS wait",
                                    bound_token.email,
                                    sid
                                );
                                target_token = Some(bound_token.clone());
                                need_update_last_used = Some((
                                    bound_token.account_id.clone(),
                                    std::time::Instant::now(),
                                ));
                            } else {
                                tracing::warn!(
                                    "Sticky Session: Bound account {} is JAILED in QoS Guardian, unbinding and switching.",
                                    bound_token.email
                                );
                                self.session_accounts.remove(sid);
                            }
                        } else if reset_sec > 0 {
                            if scheduling.mode == SchedulingMode::CacheFirst
                                && reset_sec <= scheduling.max_wait_seconds
                            {
                                // CacheFirst（缓存优先）：当限流冷却时长 <= max_wait_seconds 时原地精准等待，绝不解绑换号丢失缓存
                                let wait_secs =
                                    match crate::proxy::pipeline::budget::remaining_secs() {
                                        Some(rem_budget) => {
                                            reset_sec.min(rem_budget.saturating_sub(30))
                                        }
                                        None => reset_sec,
                                    };
                                if wait_secs > 0 {
                                    tracing::info!(
                                        "Sticky Session (CacheFirst): Bound account {} is rate-limited for {} ({}s <= max_wait {}s), waiting {}s in-place to preserve session binding and KV cache...",
                                        bound_token.email,
                                        normalized_target,
                                        reset_sec,
                                        scheduling.max_wait_seconds,
                                        wait_secs
                                    );
                                    tokio::time::sleep(std::time::Duration::from_secs(wait_secs))
                                        .await;
                                }
                                self.rate_limit_tracker
                                    .clear_model(&key, &normalized_target);
                                self.rate_limit_tracker.clear_account_only(&key);
                                tracing::info!(
                                    "Sticky Session: Successfully reusing bound account {} for session {} after rate-limit wait",
                                    bound_token.email,
                                    sid
                                );
                                target_token = Some(bound_token.clone());
                                need_update_last_used = Some((
                                    bound_token.account_id.clone(),
                                    std::time::Instant::now(),
                                ));
                            } else {
                                tracing::debug!(
                                    "Sticky Session: Bound account {} is rate-limited for {} ({}s), unbinding and switching.",
                                    bound_token.email, normalized_target, reset_sec
                                );
                                self.session_accounts.remove(sid);
                            }
                        } else {
                            // 3. 账号可用且未被标记为尝试失败，优先复用（无论 Balance 还是 CacheFirst，配额 > 0% 且未被限流、未被封禁/模型冻结时绝不主动解绑，坚决保护上游 KV 缓存）
                            tracing::info!("Sticky Session: Successfully reusing bound account {} for session {}", bound_token.email, sid);
                            target_token = Some(bound_token.clone());
                            need_update_last_used =
                                Some((bound_token.account_id.clone(), std::time::Instant::now()));
                        }
                    } else {
                        // 绑定的账号已不可用（已被删除、被上游封禁、或当前目标模型已被 403 冻结移出候选池），安全自动解绑
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
                                // 多键容错匹配：归一化 ID 与实际模型名不一致时不能当作满额
                                let last_quota = found
                                    .model_quotas
                                    .get(&normalized_target)
                                    .or_else(|| found.model_quotas.get(target_model))
                                    .copied()
                                    .unwrap_or(100);
                                let last_zero_quota = last_quota <= 0;

                                let found_eff_priority =
                                    found.effective_priority(target_model, &normalized_target);
                                let has_healthy_alt = tokens_snapshot.iter().any(|t| {
                                    &t.account_id != account_id
                                        && !attempted.contains(&t.account_id)
                                        && !self.rate_limit_tracker.is_rate_limited(
                                            &t.account_id,
                                            Some(&normalized_target),
                                        )
                                        && !(quota_protection_enabled
                                            && t.protected_models.contains(&normalized_target))
                                        && !self.qos_guardian.is_jailed_or_probing(&t.account_id)
                                        && !(t.validation_blocked
                                            && (t.validation_blocked_until == 0
                                                || t.validation_blocked_until
                                                    > chrono::Utc::now().timestamp()))
                                        && t.model_quotas
                                            .get(&normalized_target)
                                            .or_else(|| t.model_quotas.get(target_model))
                                            .copied()
                                            .unwrap_or(0)
                                            > 0
                                });

                                let has_higher_priority_alt = tokens_snapshot.iter().any(|t| {
                                    &t.account_id != account_id
                                        && !attempted.contains(&t.account_id)
                                        && !self.qos_guardian.is_jailed_or_probing(&t.account_id)
                                        && !self.rate_limit_tracker.is_rate_limited(
                                            &t.account_id,
                                            Some(&normalized_target),
                                        )
                                        && !(quota_protection_enabled
                                            && t.protected_models.contains(&normalized_target))
                                        && Self::has_candidate_quota_admission(
                                            t,
                                            &normalized_target,
                                        )
                                        && t.effective_priority(target_model, &normalized_target)
                                            < found_eff_priority
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
                                    attempted.insert(account_id.clone());
                                } else if has_higher_priority_alt {
                                    tracing::debug!(
                                        "60s Window: Last account {} has lower effective priority ({}) for model {}, yielding to higher-priority account",
                                        found.email, found_eff_priority, normalized_target
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
                    // 先过滤出未限流且未被 QoS 封禁的账号
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

                    // Layer 1: 最短等待时间 <= 配置阈值时执行物理冷却延迟（默认 60s，上限 60s）
                    if let Some(wait_sec) = min_wait {
                        let max_physical_wait_secs = scheduling.max_wait_seconds.clamp(1, 60);
                        if wait_sec <= max_physical_wait_secs {
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
                            // 当等待时间超过配置阈值时，不进行本地死等，返回冷却等待错误，
                            // 让上层协议（Claude Code / HTTP 429 Retry-After）主动进行客户端退避。
                            tracing::warn!(
                                "All accounts rate-limited with wait {}s > {}s. Demanding client cooldown backoff.",
                                wait_sec, max_physical_wait_secs
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
        let oauth_client_key = token_response.oauth_client_key.clone();
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

            if let Some(ck) = oauth_client_key {
                if crate::modules::oauth::is_gcp_enterprise_client(Some(&ck)) {
                    content["token"]["is_gcp_tos"] = serde_json::Value::Bool(true);
                }
                content["token"]["oauth_client_key"] = serde_json::Value::String(ck);
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
        // 检查熔断配置：当自适应熔断器关闭时，依然坚定拦截明确的配额耗尽与每周硬配额
        let config = self.circuit_breaker_config.read().await;
        if !config.enabled {
            return self
                .rate_limit_tracker
                .is_quota_exhausted_or_explicit_limited(account_id, model);
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

    /// 更新 QoS 自动限流进小黑屋机制开关
    pub fn set_qos_auto_jail(&self, enabled: bool) {
        self.qos_guardian.set_auto_jail_enabled(enabled);
    }

    /// 清除过期的限流记录
    #[allow(dead_code)]
    pub fn clean_expired_rate_limits(&self) {
        self.rate_limit_tracker.cleanup_expired();
    }

    /// 模型级临时熔断的归一化 key（去掉档位/思考后缀，归属到同一模型族）。
    fn model_breaker_key(model: &str) -> String {
        model
            .trim()
            .to_lowercase()
            .replace("-extra-low", "")
            .replace("-low", "")
            .replace("-medium", "")
            .replace("-high", "")
            .replace("-xhigh", "")
            .replace("-thinking", "")
            .replace("-tiered", "")
            .replace("@default", "")
    }

    /// 将指定账号的特定高阶模型标记为临时不支持（例如调用 Claude 5.5 返回 404/403）
    /// 默认冷却 15 分钟 (900s)，期间仅该模型跳过此账号，其他模型 100% 正常调用。
    pub fn mark_model_unsupported(
        &self,
        account_id: &str,
        model: &str,
        cooldown_secs: Option<i64>,
    ) {
        let base_key = Self::model_breaker_key(model);
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

    /// 清除指定账号对某个模型的临时熔断。
    ///
    /// 配额刷新 / 模型探活成功都代表上游已经重新可用，必须立即解锁，
    /// 不能让 900s 的旧冷却继续阻塞调度。
    pub fn clear_model_unsupported(&self, account_id: &str, model: &str) -> bool {
        let base_key = Self::model_breaker_key(model);
        self.unsupported_models
            .remove(&(account_id.to_string(), base_key))
            .is_some()
    }

    /// 清除指定账号的全部模型级临时熔断（用于该账号的配额/账号状态刷新）。
    pub fn clear_unsupported_models_for_account(&self, account_id: &str) -> usize {
        let before = self.unsupported_models.len();
        self.unsupported_models
            .retain(|(account_id_key, _), _| account_id_key != account_id);
        before.saturating_sub(self.unsupported_models.len())
    }

    /// 判断指定账号对特定模型是否处于临时熔断状态
    pub fn is_model_unsupported(&self, account_id: &str, model: &str) -> bool {
        let base_key = Self::model_breaker_key(model);
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

    /// 清除指定账号的限流记录（含模型级临时熔断）
    pub fn clear_rate_limit(&self, account_id: &str) -> bool {
        let cleared = self.rate_limit_tracker.clear(account_id);
        let persisted_cleared = self.clear_all_persisted_live_limits(account_id);
        let breakers_cleared = self.clear_unsupported_models_for_account(account_id) > 0;
        cleared || persisted_cleared || breakers_cleared
    }

    /// 清除指定账号在特定模型上的限流锁定（用于模型探活成功后立即恢复）
    pub fn clear_model_rate_limit(&self, account_id: &str, model: &str) -> bool {
        self.rate_limit_tracker.clear_model(account_id, model)
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
        // 非 CacheFirst 模式下主动解除绑定到该故障账号的粘性会话；CacheFirst 模式下保留会话绑定以便后续请求在 max_wait_seconds 内精准等待复用
        let is_cache_first = self
            .sticky_config
            .try_read()
            .map(|c| c.mode == crate::proxy::sticky_config::SchedulingMode::CacheFirst)
            .unwrap_or(false);
        if !is_cache_first {
            self.session_accounts.retain(|_, v| *v != resolved_id);
        }
        tracing::warn!(
            account_id = %resolved_id,
            cooldown_secs = %effective_secs,
            session_unbound = !is_cache_first,
            reason = %reason,
            "Account marked with transient failure cooldown in rate limit tracker"
        );
    }

    /// 返回所有账号上「真实配额 > 0」的模型名（原始 ID，含尺寸/档位变体，不做归一）。
    ///
    /// 供模型列表归一使用：只有真实有额度的变体才会把基底模型带入列表。
    pub fn models_with_positive_quota(&self) -> Vec<String> {
        let mut positive: HashSet<String> = HashSet::new();
        for entry in self.tokens.iter() {
            let token = entry.value();
            for (model, quota) in token.model_quotas.iter() {
                if *quota > 0 && !token.is_model_forbidden(model) {
                    positive.insert(model.clone());
                }
            }
        }
        positive.into_iter().collect()
    }

    /// 统计「对目标模型拥有正配额（或已授权可用）」的账号数量。
    ///
    /// 用于入站模型的配额闸门：返回 0 且池中确实存在支持该模型的活跃账号时，
    /// 应当由网关直接 502 拒答；若当前活跃池中没有任何支持该模型的账号，则由门禁返回 503。
    pub fn accounts_with_positive_quota(&self, model: &str) -> usize {
        self.count_matching_accounts(model, true)
    }

    /// 检查当前活跃代理池（`self.tokens`）中是否存在至少一个“具备该模型资格（含 0% 配额）”的可用账号。
    ///
    /// 排除模型级 403 冻结与验证锁定账号，但不要求当前剩余配额 > 0%，
    /// 以便模型门禁精确区分：
    /// - 503 `no available accounts`（活跃池中没有支持/未禁用此模型的账号）
    /// - 502 `no quota accounts`（活跃池里有支持此模型的账号，但全部剩余配额归零）
    pub fn has_any_capable_account_in_pool(&self, model: &str) -> bool {
        self.count_matching_accounts(model, false) > 0
    }

    /// 共享账号能力/正配额判定 helper：避免双套族判规则漂移。
    fn count_matching_accounts(&self, model: &str, require_positive_quota: bool) -> usize {
        let normalized = crate::proxy::common::model_mapping::normalize_to_standard_id(model)
            .unwrap_or_else(|| model.to_string());
        // 生图基底：若账号只上报了尺寸变体（-4k / -2k / -preview …），同族变体配额即代表基底可用
        let image_base = crate::proxy::common::model_mapping::strip_image_variant_suffix(model);
        let is_image = image_base.contains("image") || image_base.starts_with("imagen");

        let model_lower = model.trim().to_lowercase();
        // 极简两道判定，0 启发式：
        // 1. 名字含 claude 或 gpt -> 3P 第三方大池
        let is_3p = model_lower.contains("claude") || model_lower.contains("gpt");
        // 2. Gemini 4+ 专属代号（如 argon 或未来任意新代号）
        let g4_codename =
            crate::proxy::common::model_mapping::extract_gemini4_codename(&model_lower);
        let is_argon = g4_codename.as_deref() == Some("argon");
        let base_name = crate::proxy::common::model_mapping::strip_tier_suffix(&model_lower);
        let now = chrono::Utc::now().timestamp();

        self.tokens
            .iter()
            .filter(|entry| {
                let token = entry.value();
                // 模型级 403 冻结：该模型不计入可用账号/可用配额
                if token.is_model_forbidden(model) {
                    return false;
                }
                // 验证锁定且未过期的账号跳过
                if token.validation_blocked {
                    if token.validation_blocked_until == 0 || token.validation_blocked_until > now {
                        return false;
                    }
                }

                // 生图独立大池判定
                if is_image {
                    if token.model_quotas.iter().any(|(known, quota)| {
                        (!require_positive_quota || *quota > 0)
                            && crate::proxy::common::model_mapping::strip_image_variant_suffix(
                                known,
                            ) == image_base
                    }) {
                        return true;
                    }
                    return token.available_models.contains(model)
                        || token.available_models.contains(&model_lower)
                        || token.available_models.contains(&image_base);
                }

                // 3P 第三方大池判定（Claude / GPT）
                if is_3p {
                    if !require_positive_quota {
                        let has_3p_bucket = token.model_quotas.contains_key(model)
                            || token.model_quotas.contains_key(&model_lower)
                            || token.model_quotas.contains_key("claude")
                            || token.model_quotas.keys().any(|k| {
                                let kl = k.to_lowercase();
                                kl.contains("claude") || kl.contains("gpt")
                            });
                        let has_3p_available = token.available_models.contains(model)
                            || token.available_models.contains(&model_lower)
                            || token.available_models.contains(&normalized);
                        return has_3p_bucket || has_3p_available;
                    }

                    let has_3p_positive = token
                        .model_quotas
                        .get(model)
                        .copied()
                        .filter(|&q| q > 0)
                        .or_else(|| {
                            token
                                .model_quotas
                                .get(&model_lower)
                                .copied()
                                .filter(|&q| q > 0)
                        })
                        .or_else(|| token.model_quotas.get("claude").copied().filter(|&q| q > 0))
                        .or_else(|| {
                            token
                                .model_quotas
                                .iter()
                                .find(|(k, v)| {
                                    let kl = k.to_lowercase();
                                    **v > 0 && (kl.contains("claude") || kl.contains("gpt"))
                                })
                                .map(|(_, v)| *v)
                        });
                    if has_3p_positive.is_some() {
                        return true;
                    }
                    // 【FIX】3P 桶耗尽时：若模型在 model_quotas 中被显式标记为 0% 且未到期，坚决判定为无配额
                    let is_explicitly_zero = token.model_quotas.get(model).copied() == Some(0)
                        || token.model_quotas.get(&model_lower).copied() == Some(0)
                        || token.model_quotas.get("claude").copied() == Some(0);

                    if is_explicitly_zero && !Self::is_model_quota_depleted_expired(token, model) {
                        return false;
                    }

                    // 若账号在 available_models 中仍持有该模型授权（例如新模型冷启动尚未拉取配额桶）且未被限流
                    let is_available_3p = token.available_models.contains(model)
                        || token.available_models.contains(&model_lower)
                        || token.available_models.contains(&normalized);
                    if is_available_3p {
                        return !self
                            .rate_limit_tracker
                            .is_rate_limited(&token.account_id, Some(model));
                    }
                    // 3P 配额打满（如 3p-5h 归 0），且账号无该模型授权
                    return false;
                }

                // Argon 判定（数据驱动：看账号是否含有独立 Argon 桶或 Ultra 共享大池，#3745）
                if is_argon {
                    let has_argon_cap = token.available_models.iter().any(|m| {
                        crate::proxy::common::model_mapping::extract_gemini4_codename(m).as_deref()
                            == Some("argon")
                    }) || token.model_quotas.keys().any(|k| {
                        crate::proxy::common::model_mapping::extract_gemini4_codename(k).as_deref()
                            == Some("argon")
                    });
                    if !has_argon_cap {
                        return false;
                    }
                    if !require_positive_quota {
                        return true;
                    }
                    let has_argon_positive = token.model_quotas.iter().any(|(k, v)| {
                        *v > 0
                            && crate::proxy::common::model_mapping::extract_gemini4_codename(k)
                                .as_deref()
                                == Some("argon")
                    });
                    if has_argon_positive {
                        return true;
                    }
                    return !self
                        .rate_limit_tracker
                        .is_rate_limited(&token.account_id, Some(model));
                }

                // 非 Argon 的 Gemini 4+ 代号模型（如 hydrogen/carbon 或未来任何代号）：
                // 1. 若账号已加载 available_models 且不包含该代号能力（例如灰度 -disabled），直接排除；
                // 2. 若账号拆分了该代号的专属桶，仅认专属桶配额与冷却状态，不与 Gemini 通用大池串池；
                // 3. 若未拆分专属桶（共享 Gemini 通用池），继续走下方的 Gemini 通用大池判定。
                if let Some(code) = g4_codename.as_deref() {
                    let has_codename_cap = token.available_models.iter().any(|m| {
                        crate::proxy::common::model_mapping::extract_gemini4_codename(m).as_deref()
                            == Some(code)
                    }) || token.model_quotas.keys().any(|k| {
                        crate::proxy::common::model_mapping::extract_gemini4_codename(k).as_deref()
                            == Some(code)
                    });
                    if !token.available_models.is_empty() && !has_codename_cap {
                        return false;
                    }
                    if Self::has_dedicated_codename_bucket(token, code) {
                        if !require_positive_quota {
                            return true;
                        }
                        let has_dedicated_positive = token.model_quotas.iter().any(|(k, v)| {
                            *v > 0
                                && crate::proxy::common::model_mapping::extract_gemini4_codename(k)
                                    .as_deref()
                                    == Some(code)
                        });
                        if has_dedicated_positive {
                            return true;
                        }
                        return Self::is_model_quota_depleted_expired(token, model)
                            && !self
                                .rate_limit_tracker
                                .is_rate_limited(&token.account_id, Some(model));
                    }
                }

                // 除 3P 与专属独立代号桶外的所有 Gemini 模型：一律直接落入 Gemini 通用配额大池！
                let matches_general_gemini_bucket = |k: &str| {
                    let kl = k.to_lowercase();
                    let k_dedicated_g4 =
                        crate::proxy::common::model_mapping::extract_gemini4_codename(&kl)
                            .map(|c| c == "argon" || Self::has_dedicated_codename_bucket(token, &c))
                            .unwrap_or(false);
                    !kl.contains("claude")
                        && !kl.contains("gpt")
                        && !kl.contains("3p")
                        && !k_dedicated_g4
                };

                if !require_positive_quota {
                    let has_gemini_bucket = token.model_quotas.contains_key(model)
                        || token.model_quotas.contains_key(&model_lower)
                        || token.model_quotas.contains_key(&base_name)
                        || token
                            .model_quotas
                            .contains_key(&format!("{}-tiered", base_name))
                        || token.model_quotas.contains_key(&normalized)
                        || token
                            .model_quotas
                            .keys()
                            .any(|k| matches_general_gemini_bucket(k));
                    let has_gemini_available = token.available_models.contains(model)
                        || token.available_models.contains(&model_lower)
                        || (!base_name.is_empty() && token.available_models.contains(&base_name))
                        || token.available_models.contains(&normalized);
                    return has_gemini_bucket || has_gemini_available;
                }

                let has_gemini_positive = token
                    .model_quotas
                    .get(model)
                    .copied()
                    .filter(|&q| q > 0)
                    .or_else(|| {
                        token
                            .model_quotas
                            .get(&model_lower)
                            .copied()
                            .filter(|&q| q > 0)
                    })
                    .or_else(|| {
                        token
                            .model_quotas
                            .get(&base_name)
                            .copied()
                            .filter(|&q| q > 0)
                    })
                    .or_else(|| {
                        token
                            .model_quotas
                            .get(&format!("{}-tiered", base_name))
                            .copied()
                            .filter(|&q| q > 0)
                    })
                    .or_else(|| {
                        token
                            .model_quotas
                            .iter()
                            .find(|(k, v)| **v > 0 && matches_general_gemini_bucket(k))
                            .map(|(_, v)| *v)
                    })
                    .or_else(|| {
                        token
                            .model_quotas
                            .get(&normalized)
                            .copied()
                            .filter(|&q| q > 0)
                    });

                if has_gemini_positive.is_some() {
                    return true;
                }

                // 兜底：账号在 available_models 中有授权且未被限流锁定
                let is_available = token.available_models.contains(model)
                    || token.available_models.contains(&model_lower)
                    || (!base_name.is_empty() && token.available_models.contains(&base_name))
                    || token.available_models.contains(&normalized);

                if is_available {
                    return !self
                        .rate_limit_tracker
                        .is_rate_limited(&token.account_id, Some(model));
                }

                false
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

            // 0. 模型级 403 冻结：若目标模型已被冻结，该账号不可用于此模型
            if token.is_model_forbidden(target_model) {
                tracing::debug!(
                    "[Fallback Check] Account {} is forbidden for model {}, skipping",
                    token.email,
                    target_model
                );
                continue;
            }

            // 0.1 验证锁定中或禁用状态
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
        error_body: Option<&str>,
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
                        token.model_limits.clear();
                    }
                    for m in &quota_data.models {
                        let norm_name =
                            crate::proxy::common::model_mapping::normalize_to_standard_id(&m.name)
                                .unwrap_or_else(|| m.name.clone());
                        let pct_val = m.percentage as i32;
                        token
                            .model_quotas
                            .entry(norm_name)
                            .and_modify(|existing: &mut i32| *existing = (*existing).max(pct_val))
                            .or_insert(pct_val);
                        token.model_quotas.insert(m.name.clone(), pct_val);
                        let m_lower = m.name.to_lowercase();
                        token.model_quotas.insert(m_lower.clone(), pct_val);
                        let base_name =
                            crate::proxy::common::model_mapping::strip_tier_suffix(&m_lower);
                        if base_name != m_lower {
                            token
                                .model_quotas
                                .entry(base_name.clone())
                                .and_modify(|existing: &mut i32| {
                                    *existing = (*existing).max(pct_val)
                                })
                                .or_insert(pct_val);
                            token.available_models.insert(base_name.clone());
                        }
                        token.available_models.insert(m_lower.clone());
                        if let Some(limit) = m.max_output_tokens.filter(|&v| v > 0) {
                            let limit_u64 = limit as u64;
                            token.model_limits.insert(m.name.clone(), limit_u64);
                            if m_lower != m.name {
                                token.model_limits.entry(m_lower).or_insert(limit_u64);
                            }
                            if base_name != m.name.to_lowercase() {
                                token
                                    .model_limits
                                    .entry(base_name)
                                    .and_modify(|existing| *existing = (*existing).max(limit_u64))
                                    .or_insert(limit_u64);
                            }
                        }
                    }

                    // 实时刷新时复用统一 Gemini 4+ 分组同步与三套配额窗口快照写入入口，
                    // 确保 Argon 门禁、未来新代号专属桶（若上游拆分）以及重置时间戳与冷启动加载完全一致。
                    let is_ultra = token
                        .subscription_tier
                        .as_deref()
                        .map(|s| s.to_uppercase() == "ULTRA")
                        .unwrap_or(false);
                    let known_codenames = Self::collect_known_g4_codenames(
                        token.available_models.iter().map(String::as_str),
                    );
                    if let Some(groups) = quota_data.quota_groups.as_ref() {
                        let inputs = Self::quota_bucket_inputs_from_typed_with_codenames(
                            groups,
                            &known_codenames,
                        );
                        {
                            let token_ref = &mut *token;
                            Self::reconcile_g4_group_quotas(
                                is_ultra,
                                &inputs,
                                &mut token_ref.model_quotas,
                                &mut token_ref.available_models,
                                &mut token_ref.model_limits,
                            );
                        }
                        token.model_quota_windows.clear();
                        token.model_reset_times.clear();
                        token.model_weekly_reset_times.clear();
                        token.apply_quota_bucket_inputs(inputs);
                    }

                    for m in &quota_data.models {
                        if !m.reset_time.is_empty() {
                            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&m.reset_time) {
                                let ts = dt.timestamp();
                                let m_lower = m.name.to_lowercase();
                                token.model_reset_times.insert(m.name.clone(), ts);
                                token.model_reset_times.insert(m_lower.clone(), ts);
                                let base = crate::proxy::common::model_mapping::strip_tier_suffix(
                                    &m_lower,
                                );
                                if base != m_lower {
                                    token.model_reset_times.insert(base, ts);
                                }
                            }
                        }
                    }
                }

                // 3. 从最新配额中提取匹配模型的 reset_time
                let normalized_model = model
                    .as_deref()
                    .and_then(|m| crate::proxy::common::model_mapping::normalize_to_standard_id(m));
                let model_to_lock = normalized_model.or(model);

                let mut is_weekly_depleted = false;

                // 【L1】优先采信上游在 429 报文中直接下发的权威重置时间戳。
                // 实测四类桶（Argon 独立桶 / Ultra 共享大桶 / 普通 Gemini 大桶 / 3P 桶）
                // 耗尽时结构一致且无桶标识，但均携带 quotaResetTimeStamp；采信它比
                // “本地桶余量 <= 1.5%”的推算更可靠，也不会因上游改文案而失效。
                // 1.5% 木桶（下列分支）保留为 L3 兜底。
                let upstream_reset = error_body
                    .as_deref()
                    .and_then(crate::proxy::rate_limit::extract_quota_reset_timestamp)
                    .and_then(|ts| chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0))
                    .map(|dt| dt.to_rfc3339());

                if let Some(ref ts) = upstream_reset {
                    tracing::info!(
                        "账号 {} 429 携带上游权威 quotaResetTimeStamp={}，直接锁定（跳过本地推算）",
                        email,
                        ts
                    );
                }

                let matched_reset: Option<&str> = if upstream_reset.is_some() {
                    is_weekly_depleted = true;
                    upstream_reset.as_deref()
                } else if let Some(ref target) = model_to_lock {
                    let norm_target =
                        crate::proxy::common::model_mapping::normalize_to_standard_id(target)
                            .unwrap_or_else(|| target.to_string());

                    // 精准查找当前报错的模型所属分组，并检查**该分组内任意桶**是否低于 1.5%。
                    // 四类桶（Argon 独立桶 / Ultra 共享大桶 / 普通 Gemini 大桶 / 3P 桶）都可能只耗尽
                    // 5h 或 weekly 其中一个窗口，只盯 weekly 会漏判（如 5h=0.4% 反复 300s 重试）。
                    let is_ultra = self
                        .tokens
                        .get(&account_id)
                        .map(|t| {
                            t.subscription_tier
                                .as_deref()
                                .map(|s| s.to_uppercase() == "ULTRA")
                                .unwrap_or(false)
                        })
                        .unwrap_or(false);
                    let known_codenames = Self::collect_known_g4_codenames(
                        quota_data.models.iter().map(|m| m.name.as_str()),
                    );
                    let target_codename =
                        crate::proxy::common::model_mapping::extract_gemini4_codename(target);
                    let l3_group_family = |g: &crate::models::quota::QuotaGroup| -> String {
                        let gname = g.display_name.to_lowercase();
                        for b in &g.buckets {
                            let bid = b.bucket_id.to_lowercase();
                            let bname = b.display_name.as_deref().unwrap_or("").to_lowercase();
                            let combined = if !bname.is_empty() {
                                format!("{} {}", gname, bname)
                            } else {
                                gname.clone()
                            };
                            let fam = Self::quota_family_key_with_codenames(
                                &combined,
                                &bid,
                                &known_codenames,
                            );
                            if fam != "gemini" {
                                return fam;
                            }
                        }
                        Self::quota_family_key_with_codenames(&gname, "", &known_codenames)
                    };
                    let l3_group_matches = |g: &crate::models::quota::QuotaGroup,
                                            groups: &[crate::models::quota::QuotaGroup]|
                     -> bool {
                        let fam = l3_group_family(g);
                        if target_codename.is_none()
                            && (target.contains("claude") || target.contains("gpt"))
                        {
                            return fam == "3p";
                        }
                        if let Some(code) = target_codename.as_deref() {
                            if groups.iter().any(|cand| l3_group_family(cand) == code) {
                                return fam == code;
                            }
                            return if code == "argon" {
                                is_ultra && fam == "gemini"
                            } else {
                                fam == "gemini"
                            };
                        }
                        fam == "gemini"
                    };

                    is_weekly_depleted = quota_data.quota_groups.as_ref().map_or(false, |groups| {
                        groups.iter().any(|g| {
                            l3_group_matches(g, groups)
                                && g.buckets.iter().any(|b| b.remaining_fraction <= 0.015)
                        })
                    });

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
                            // 仅当该模型配额确实耗尽 (percentage <= 1，即低于 1.5%) 或对应周配额已到期时才采用长周期 reset_time；
                            // 若配额尚存，说明 429 属于瞬态限流，严禁锁入长周期刷新时间！
                            if (m.percentage <= 1 || is_weekly_depleted) && !m.reset_time.is_empty()
                            {
                                Some(m.reset_time.as_str())
                            } else {
                                None
                            }
                        })
                        .or_else(|| {
                            // 回退：上游对已耗尽模型可能直接省略 quotaInfo.remainingFraction，
                            // 导致上面的 models 匹配拿不到 reset_time。此时直接从命中的
                            // 配额分组中提取“已耗尽桶”的官方 reset_time（覆盖 Argon/G4 专属桶及共享大池）。
                            if !is_weekly_depleted {
                                return None;
                            }
                            quota_data.quota_groups.as_ref().and_then(|groups| {
                                groups
                                    .iter()
                                    .filter(|g| l3_group_matches(g, groups))
                                    // 【锁定源优先级：硬配额优先于滚动窗口】
                                    // weekly 是硬配额，归零后只有到 reset 才恢复；若此时取
                                    // 更早的 5h reset（例如 30 分钟后）就会造成「反复调度」：
                                    // 5h 一恢复就去试，weekly 仍为 0，于是再 429 一次，
                                    // 每次失败都会污染模型级锁定与失败计数。
                                    // 因此：weekly 耗尽 → 锁 weekly reset；
                                    //       仅 5h 耗尽（weekly 健康）→ 才锁 5h reset，
                                    //       避免把一个几十分钟就能恢复的账号锁上一整天。
                                    .flat_map(|g| g.buckets.iter())
                                    .filter(|b| b.remaining_fraction <= 0.015)
                                    .filter(|b| !b.reset_time.is_empty())
                                    .min_by_key(|b| {
                                        let win =
                                            format!("{} {}", b.window, b.bucket_id).to_lowercase();
                                        let is_weekly = win.contains("week")
                                            || win.contains("7d")
                                            || win.contains("wtu");
                                        // weekly 排在最前（硬配额），其余按 reset 时间排序
                                        if is_weekly {
                                            (0u8, String::new())
                                        } else {
                                            (1u8, b.reset_time.clone())
                                        }
                                    })
                                    .map(|b| b.reset_time.as_str())
                            })
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
                        "账号 {} 实时配额刷新成功,模型 {:?} 匹配到官方权威 reset_time: {}",
                        email,
                        model_to_lock,
                        reset_time_str
                    );

                    // 解析官方时间戳，同步更新内存中 token 的 reset_time
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(reset_time_str) {
                        let ts = dt.timestamp();
                        if let Some(mut t) = self.tokens.get_mut(&account_id) {
                            t.reset_time = Some(ts);
                            if is_weekly_depleted {
                                t.weekly_reset_time = Some(ts);
                            }
                        }
                    }

                    // 广播给前端实时更新账户管理界面的配额进度条与重置倒计时
                    crate::modules::log_bridge::emit_accounts_refreshed();

                    // 权威锁定：以官方返回时间精确锁定（不被 300s 截断），到期自动归还调度池
                    self.rate_limit_tracker.set_lockout_until_iso_with_cap(
                        &account_id,
                        reset_time_str,
                        reason,
                        model_to_lock,
                        false, // 官方权威时间，不进行 300s 截断，到期自动解除
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

        let account_id = self
            .email_to_account_id(email)
            .unwrap_or_else(|| email.to_string());
        let has_explicit_retry_time = Self::has_explicit_retry_time(
            TrackerParserMode::Current,
            retry_after_header,
            error_body,
        );
        let reason = classify_rate_limit_reason(error_body);
        let is_quota_depleted = crate::proxy::rate_limit::is_quota_depleted_signal(error_body)
            || reason == crate::proxy::rate_limit::RateLimitReason::QuotaExhausted;

        // 【物理事实优先】当上游明确返回配额耗尽信号时，
        // 账号已无可用配额是客观物理事实，绝不受自适应熔断器开关限制！
        if is_quota_depleted {
            self.mark_model_quota_depleted(&account_id, model_to_track);
        }

        let config = self.circuit_breaker_config.read().await.clone();
        if !config.enabled && !is_quota_depleted && !has_explicit_retry_time {
            return false;
        }

        let recorded = self.record_rate_limit_atomic(
            &account_id,
            status,
            retry_after_header,
            error_body,
            model_to_track,
            &config.backoff_steps,
            TrackerParserMode::Current,
        );

        status == 429
            && (recorded.is_some() || is_quota_depleted)
            && !has_explicit_retry_time
            && is_quota_depleted
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
                let m_lower = m.trim().to_lowercase();
                token.model_quotas.insert(m_lower.clone(), 0);
                let base_name = crate::proxy::common::model_mapping::strip_tier_suffix(&m_lower);
                if base_name != m_lower {
                    token.model_quotas.insert(base_name, 0);
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
            .fetch_and_lock_with_realtime_quota(
                email,
                reason,
                model_to_track.map(str::to_string),
                None, // 该路径无原始报文，走 L3 本地桶兜底
            )
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
        // 判据统一收敛到 UpstreamClassification::is_internal_gateway_exhaustion，保持单一权威源。
        if crate::proxy::common::model_gate::is_gateway_self_answer(error_body)
            || crate::proxy::pipeline::UpstreamClassification::is_internal_gateway_exhaustion(
                &error_body.to_lowercase(),
            )
        {
            return;
        }

        let normalized_model =
            model.and_then(crate::proxy::common::model_mapping::normalize_to_standard_id);
        let model_to_track = normalized_model.as_deref().or(model);

        let account_id = self
            .email_to_account_id(email)
            .unwrap_or_else(|| email.to_string());
        let reason = classify_rate_limit_reason(error_body);
        let is_quota_depleted_signal =
            crate::proxy::rate_limit::is_quota_depleted_signal(error_body);

        // 【物理事实优先】当上游明确返回配额耗尽或配额到期信号时，
        // 账号已无可用配额是客观物理事实，绝不受自适应熔断器开关限制！
        // 必须立即将目标模型配额标记为 0%，杜绝调度器继续盲目重试该账号。
        if is_quota_depleted_signal
            || reason == crate::proxy::rate_limit::RateLimitReason::QuotaExhausted
        {
            self.mark_model_quota_depleted(&account_id, model_to_track);
        }

        // 若命中 "Individual quota reached" 或 "check quota"，强制触发实时官方配额复核，
        // 确认周配额是否低于 1.5%，并精确锁定为官方重置时间（到期自动归还调度池）
        if is_quota_depleted_signal {
            if self
                .fetch_and_lock_with_realtime_quota(
                    email,
                    crate::proxy::rate_limit::RateLimitReason::QuotaExhausted,
                    model_to_track.map(str::to_string),
                    Some(error_body),
                )
                .await
            {
                tracing::info!(
                    "账号 {} 命中配额到期关键字，已使用实时官方配额精确锁定并同步前端",
                    email
                );
                return;
            }
        }

        let config = self.circuit_breaker_config.read().await.clone();

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
            if !config.enabled {
                return;
            }
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
            .fetch_and_lock_with_realtime_quota(
                email,
                reason,
                model_to_track.map(str::to_string),
                Some(error_body),
            )
            .await
        {
            tracing::info!("账号 {} 已使用实时配额精确锁定", email);
            return;
        }
        if self.set_precise_lockout(&account_id, reason, model_to_track.map(str::to_string)) {
            tracing::info!("账号 {} 已使用本地缓存配额锁定", account_id);
            return;
        }

        if !config.enabled {
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
        crate::proxy::pipeline::rotation::set_request_429_grace_ms(
            new_config.resolve_request_429_grace_ms(),
        );
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
            let is_gcp_tos = crate::modules::oauth::is_gcp_enterprise_client(
                token_info.oauth_client_key.as_deref(),
            );

            let token_data = crate::models::TokenData::new(
                token_info.access_token,
                refresh_token_clone,
                token_info.expires_in,
                Some(email_clone.clone()),
                Some(project_id),
                None,       // session_id
                is_gcp_tos, // 企业或 GCP 客户端自适应开启
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

        let is_cache_first = self
            .sticky_config
            .try_read()
            .map(|c| c.mode == crate::proxy::sticky_config::SchedulingMode::CacheFirst)
            .unwrap_or(false);

        match action {
            crate::proxy::qos_guardian::QoSAction::TriggerJailedAndUnbind {
                level,
                wait_duration,
            } => {
                if !is_cache_first {
                    if let Some(sid) = session_id {
                        self.unbind_session(sid);
                    }
                }
                tracing::warn!(
                    "🚨 [QoS Guardian] Account {} jailed to Level {} (wait {:?}), session_unbound={}",
                    account_id,
                    level,
                    wait_duration,
                    !is_cache_first
                );
            }
            crate::proxy::qos_guardian::QoSAction::HalfOpenFailed {
                escalated_level,
                wait_duration,
            } => {
                if !is_cache_first {
                    if let Some(sid) = session_id {
                        self.unbind_session(sid);
                    }
                }
                tracing::warn!(
                    "💥 [QoS Guardian] Account {} Half-Open probe probation failed. Escalated to Level {} ({:?}), session_unbound={}",
                    account_id,
                    escalated_level,
                    wait_duration,
                    !is_cache_first
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
            .and_then(|q| q.get("quota_groups").or_else(|| q.get("quotaGroups")))
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
        if let Some(groups) = quota
            .get("quota_groups")
            .or_else(|| quota.get("quotaGroups"))
            .and_then(|g| g.as_array())
        {
            let known_codenames = Self::collect_known_g4_codenames(
                quota
                    .get("models")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|m| m.get("name").and_then(|v| v.as_str())),
            );
            // Collect all Gemini 4+ codenames that actually own a dedicated group in this payload
            // so the general Gemini group automatically excludes them without hardcoding names.
            let mut dedicated_codenames: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            for group in groups {
                let gname = group
                    .get("display_name")
                    .or_else(|| group.get("displayName"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                if let Some(code) =
                    crate::proxy::common::model_mapping::extract_group_g4_codename_with_known(
                        &gname,
                        "",
                        &known_codenames,
                    )
                {
                    dedicated_codenames.insert(code);
                }
                if let Some(buckets) = group.get("buckets").and_then(|b| b.as_array()) {
                    for b in buckets {
                        let bid = b
                            .get("bucket_id")
                            .or_else(|| b.get("bucketId"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_lowercase();
                        let bname = b
                            .get("display_name")
                            .or_else(|| b.get("displayName"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_lowercase();
                        let combined = if !bname.is_empty() {
                            format!("{} {}", gname, bname)
                        } else {
                            gname.clone()
                        };
                        if let Some(code) =
                            crate::proxy::common::model_mapping::extract_group_g4_codename_with_known(
                                &combined,
                                &bid,
                                &known_codenames,
                            )
                        {
                            dedicated_codenames.insert(code);
                        }
                    }
                }
            }

            for group in groups {
                let group_name = group
                    .get("display_name")
                    .or_else(|| group.get("displayName"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let group_name_lower = group_name.to_lowercase();
                let group_codename =
                    crate::proxy::common::model_mapping::extract_group_g4_codename_with_known(
                        &group_name_lower,
                        "",
                        &known_codenames,
                    );
                let is_claude_group = group_codename.is_none()
                    && (group_name_lower.contains("claude")
                        || group_name_lower.contains("gpt")
                        || group_name_lower.contains("3p"));
                let is_gemini_group = group_codename.is_none()
                    && !is_claude_group
                    && (group_name_lower.contains("gemini")
                        || (!group_name_lower.contains("claude")
                            && !group_name_lower.contains("gpt")
                            && !group_name_lower.contains("3p")));

                if let Some(buckets) = group.get("buckets").and_then(|b| b.as_array()) {
                    let all_buckets_positive = buckets.iter().all(|b| {
                        b.get("remaining_fraction")
                            .or_else(|| b.get("remainingFraction"))
                            .and_then(|v| v.as_f64())
                            .map(|f| f > 0.001)
                            .unwrap_or(false)
                    });
                    for bucket in buckets {
                        let bucket_id = bucket
                            .get("bucket_id")
                            .or_else(|| bucket.get("bucketId"))
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
                        let Some(fraction) = bucket
                            .get("remaining_fraction")
                            .or_else(|| bucket.get("remainingFraction"))
                            .and_then(|v| v.as_f64())
                        else {
                            continue;
                        };
                        let exhausted_until = if fraction <= 0.001 {
                            let Some(reset) = bucket
                                .get("reset_time")
                                .or_else(|| bucket.get("resetTime"))
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
                        let bucket_display_lower = bucket
                            .get("display_name")
                            .or_else(|| bucket.get("displayName"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_lowercase();
                        let bucket_name_ctx = if !bucket_display_lower.is_empty() {
                            format!("{} {}", group_name_lower, bucket_display_lower)
                        } else {
                            group_name_lower.clone()
                        };
                        let bucket_codename = group_codename.clone().or_else(|| {
                            crate::proxy::common::model_mapping::extract_group_g4_codename_with_known(
                                &bucket_name_ctx,
                                &bucket_id_lower,
                                &known_codenames,
                            )
                        });
                        let third_party = bucket_codename.is_none()
                            && (is_claude_group || bucket_id_lower.contains("3p"));
                        let gemini = bucket_codename.is_none()
                            && !third_party
                            && (is_gemini_group || bucket_id_lower.contains("gemini"));

                        let mut models: Vec<String> = if let Some(ref code) = bucket_codename {
                            if code == "argon" {
                                vec![
                                    "gemini-4-argon".to_string(),
                                    "gemini-4-argon-high".to_string(),
                                    "gemini-4-argon-medium".to_string(),
                                    "gemini-4-argon-low".to_string(),
                                ]
                            } else {
                                vec![format!("gemini-4-{}", code)]
                            }
                        } else if third_party {
                            let has_3p = quota
                                .get("models")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter().any(|m| {
                                        let n = m
                                            .get("name")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_lowercase();
                                        !n.contains("-disabled")
                                            && !n.ends_with("_disabled")
                                            && (n.starts_with("claude") || n.starts_with("gpt"))
                                    })
                                })
                                .unwrap_or(true);
                            if has_3p {
                                vec!["claude".to_string()]
                            } else {
                                Vec::new()
                            }
                        } else if gemini {
                            vec![
                                "gemini-3-flash".to_string(),
                                "gemini-3.1-pro-high".to_string(),
                                "gemini-3.1-flash-image".to_string(),
                                "gemini-3-pro-image".to_string(),
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
                                        if n_lower.contains("-disabled")
                                            || n_lower.ends_with("_disabled")
                                        {
                                            return false;
                                        }
                                        let m_code =
                                            crate::proxy::common::model_mapping::extract_gemini4_codename(
                                                &n_lower,
                                            );
                                        if let Some(ref code) = bucket_codename {
                                            m_code.as_deref() == Some(code.as_str())
                                        } else if third_party {
                                            m_code.is_none()
                                                && (n_lower.starts_with("claude")
                                                    || n_lower.starts_with("gpt"))
                                        } else {
                                            gemini
                                                && n_lower.starts_with("gemini")
                                                && match m_code {
                                                    None => true,
                                                    Some(ref c) => {
                                                        c != "argon"
                                                            && !dedicated_codenames.contains(c)
                                                    }
                                                }
                                        }
                                    })
                                    .map(str::to_string),
                            );
                        }
                        for model in models {
                            let normalized =
                                crate::proxy::common::model_mapping::normalize_to_standard_id(
                                    &model,
                                )
                                .unwrap_or(model);
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
                            if all_buckets_positive {
                                self.rate_limit_tracker
                                    .reconcile_quota_recovery(account_id, &normalized);
                            }
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

    /// 获取当前所有可用账号中收集到的官方下发模型集合
    ///
    /// 用于对外模型列表（`/v1/models` 等）：**不包含** `model_quotas` 里的内部保护桶名，
    /// 也不包含任何未在 `fetchAvailableModels` 可用集合里的字典残留。
    pub fn get_all_available_models(&self) -> std::collections::HashSet<String> {
        let mut all_models = std::collections::HashSet::new();
        for entry in self.tokens.iter() {
            let token = entry.value();

            for model_id in &token.available_models {
                if !token.is_model_forbidden(model_id) {
                    all_models.insert(model_id.clone());
                }
            }

            // 持久化文件里的原始 quota 模型 ID（官方 fetchAvailableModels 可用集合）
            if let Some(raw_models) = Self::get_available_models_from_json(&token.account_path) {
                for model_id in raw_models {
                    if !token.is_model_forbidden(&model_id) {
                        all_models.insert(model_id);
                    }
                }
            }
        }
        // 同步到全局快照：一键配置（CLI/opencode/dsh 等）只列真实可用模型
        crate::proxy::cli_sync::update_available_model_ids(all_models.iter().cloned());
        all_models
    }

    /// 获取当前所有可用账号中收集到的官方下发的所有动态模型集合
    ///
    /// 注意：本方法用于模型闸门动态感知号池上报过的模型特征并集（供冷启动与结构发现使用）；
    /// 对外暴露的标准可用模型列表请使用已严格排除 forbidden_models 冻结模型的 `get_all_available_models`。
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

    /// 扫描磁盘 `accounts/*.json` 中所有有效（非孤儿）账号曾上报过的模型并集，
    /// **包含**当前被标记为 `disabled` / `proxy_disabled` 的已导入账号，用于模型门禁判定
    /// “该模型确实在号池/目录生态中被识别过，只是当前没有启用/可用的配额账号 (503)”。
    ///
    /// 注意：严格复用 `ACCOUNT_MODEL_FILE_CACHE` 缓存与 `accounts.json` 索引校验，
    /// 避免在请求路径引入重复磁盘解析或渗入已删除孤儿文件。
    pub fn get_all_disk_discovered_models(&self) -> std::collections::HashSet<String> {
        let mut discovered = std::collections::HashSet::new();
        let data_dir = self.resolved_data_dir();
        let accounts_dir = data_dir.join("accounts");
        if !accounts_dir.exists() {
            return discovered;
        }
        let (valid_in_index, _) = Self::read_accounts_index_sets(&data_dir);
        let Ok(entries) = std::fs::read_dir(&accounts_dir) else {
            return discovered;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            if let Some(ref valid_ids) = valid_in_index {
                if !Self::is_account_file_in_index(&path, valid_ids) {
                    continue;
                }
            }
            if let Some(raw_models) = Self::get_available_models_from_json(&path) {
                discovered.extend(raw_models);
            }
        }
        discovered
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

        // [FIX] 403 人机验证阻断时，同步标记 proxy_disabled = true，确保前端直接展示禁用反代状态并停止参与调度
        // 关键防护：此为系统锁调用，manual_intent 必须传 false，严禁把验证阻断写成“用户手动禁用”！
        let _ = crate::modules::account::toggle_proxy_status_with_intent(
            account_id,
            false,
            Some("validation_required"),
            false,
        );

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
        self.set_forbidden_with_model(account_id, reason, None)
            .await
    }

    pub async fn set_forbidden_with_model(
        &self,
        account_id: &str,
        reason: &str,
        target_model: Option<&str>,
    ) -> Result<(), String> {
        self.set_forbidden_with_models(account_id, reason, target_model, None)
            .await
    }

    pub async fn set_forbidden_with_models(
        &self,
        account_id: &str,
        reason: &str,
        target_model: Option<&str>,
        alt_model: Option<&str>,
    ) -> Result<(), String> {
        // [FIX] 调用封装好的模块函数，确保线程安全地更新账号文件和索引
        let outcome = crate::modules::account::mark_account_forbidden_with_models(
            account_id,
            reason,
            target_model,
            alt_model,
        )?;

        // Clear sticky session if forbidden
        self.session_accounts.retain(|_, v| *v != account_id);

        match outcome {
            crate::modules::account::ForbiddenApplyResult::AccountLocked => {
                // 账号级 403/封禁：从内存池中移除，避免重试时再次选中
                self.remove_account(account_id);
            }
            crate::modules::account::ForbiddenApplyResult::ModelLocked(scope) => {
                // 模型级 403：保留账号供其他模型使用，立即冻结当前模型。
                // mark_account_forbidden 已经触发 reload；这里同步内存 token，
                // 让本次请求的后续重试立即避开该模型。
                if let Some(mut token) = self.tokens.get_mut(account_id) {
                    token.forbidden_models.insert(scope);
                    if let Some(target) = target_model {
                        if let Some(target_cleaned) =
                            crate::models::account::clean_model_token(target)
                        {
                            let target_base =
                                crate::proxy::common::model_mapping::strip_tier_suffix(
                                    &target_cleaned,
                                )
                                .to_lowercase();
                            token.forbidden_models.insert(target_base);
                        }
                    }
                    if let Some(alt) = alt_model {
                        if let Some(alt_cleaned) = crate::models::account::clean_model_token(alt) {
                            let alt_base = crate::proxy::common::model_mapping::strip_tier_suffix(
                                &alt_cleaned,
                            )
                            .to_lowercase();
                            token.forbidden_models.insert(alt_base);
                        }
                    }
                }
            }
        }

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

    #[tokio::test]
    async fn test_empty_pool_error_reports_banned_accounts_instead_of_generic_429() {
        // Issue #3919: 全池 banned 后应返回明确的封禁诊断，而非误导性的 "Token pool is empty"
        let tmp_root = std::env::temp_dir().join(format!(
            "antigravity-token-manager-test-empty-pool-{}",
            uuid::Uuid::new_v4()
        ));
        let accounts_dir = tmp_root.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();
        let now = chrono::Utc::now().timestamp();

        // 2 banned (403 TOS_VIOLATION), 1 disabled (manual), 0 available.
        let banned_json = serde_json::json!({
            "id": "acc1",
            "email": "a@test.com",
            "token": {
                "access_token": "atk",
                "refresh_token": "rtk",
                "expires_in": 3600,
                "expiry_timestamp": now + 3600
            },
            "disabled": false,
            "proxy_disabled": true,
            "proxy_disabled_reason": "Forbidden (403): TOS_VIOLATION",
            "quota": { "is_forbidden": true, "forbidden_reason": "TOS_VIOLATION" },
            "created_at": now,
            "last_used": now
        });
        std::fs::write(
            accounts_dir.join("acc1.json"),
            serde_json::to_string_pretty(&banned_json).unwrap(),
        )
        .unwrap();
        let banned2 = {
            let mut j = banned_json.clone();
            j["id"] = serde_json::Value::String("acc2".into());
            j["email"] = serde_json::Value::String("b@test.com".into());
            j
        };
        std::fs::write(
            accounts_dir.join("acc2.json"),
            serde_json::to_string_pretty(&banned2).unwrap(),
        )
        .unwrap();
        let disabled_json = serde_json::json!({
            "id": "acc3",
            "email": "c@test.com",
            "token": {
                "access_token": "atk3",
                "refresh_token": "rtk3",
                "expires_in": 3600,
                "expiry_timestamp": now + 3600
            },
            "disabled": false,
            "proxy_disabled": true,
            "proxy_disabled_reason": "manual",
            "created_at": now,
            "last_used": now
        });
        std::fs::write(
            accounts_dir.join("acc3.json"),
            serde_json::to_string_pretty(&disabled_json).unwrap(),
        )
        .unwrap();

        let manager = TokenManager::new(tmp_root.clone());
        // All accounts are banned/disabled on disk, so nothing loads into the pool.
        manager.load_accounts().await.unwrap();
        assert_eq!(manager.tokens.len(), 0);

        let err = manager
            .get_token("gemini", false, None, "gemini-1.5-flash")
            .await
            .unwrap_err();

        assert!(
            err.contains("No available accounts"),
            "error should classify the pool state, got: {}",
            err
        );
        assert!(
            err.contains("banned: 2"),
            "error should report banned count, got: {}",
            err
        );
        assert!(
            err.contains("disabled: 1"),
            "error should report disabled count, got: {}",
            err
        );
        assert!(
            err.contains("will NOT recover by waiting"),
            "error should warn that cooldown will not help, got: {}",
            err
        );
        assert!(
            !err.starts_with("Token pool is empty"),
            "banned-pool error must not be the generic empty message, got: {}",
            err
        );

        let _ = std::fs::remove_dir_all(&tmp_root);
    }

    #[tokio::test]
    async fn test_empty_pool_error_reports_disabled_only_pool() {
        // Pool with only manually disabled accounts: should point to re-enabling, not cooldown.
        let tmp_root = std::env::temp_dir().join(format!(
            "antigravity-token-manager-test-disabled-pool-{}",
            uuid::Uuid::new_v4()
        ));
        let accounts_dir = tmp_root.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();
        let now = chrono::Utc::now().timestamp();
        let disabled_json = serde_json::json!({
            "id": "acc1",
            "email": "a@test.com",
            "token": {
                "access_token": "atk",
                "refresh_token": "rtk",
                "expires_in": 3600,
                "expiry_timestamp": now + 3600
            },
            "disabled": false,
            "proxy_disabled": true,
            "proxy_disabled_reason": "manual",
            "created_at": now,
            "last_used": now
        });
        std::fs::write(
            accounts_dir.join("acc1.json"),
            serde_json::to_string_pretty(&disabled_json).unwrap(),
        )
        .unwrap();

        let manager = TokenManager::new(tmp_root.clone());
        manager.load_accounts().await.unwrap();

        let err = manager
            .get_token("gemini", false, None, "gemini-1.5-flash")
            .await
            .unwrap_err();
        assert!(
            err.contains("all 1 account(s) are disabled"),
            "error should report disabled-only pool, got: {}",
            err
        );
        assert!(
            err.contains("banned: 0"),
            "disabled-only pool must not be reported as banned, got: {}",
            err
        );

        let _ = std::fs::remove_dir_all(&tmp_root);
    }

    #[tokio::test]
    async fn test_internal_gateway_exhaustion_classifier_recognizes_new_banned_pool_error() {
        // 防递归自锁门禁必须识别新的池空诊断文案（policy.rs 同步扩展）
        let lower = "no available accounts: all pool accounts are banned or disabled (banned: 3)"
            .to_lowercase();
        assert!(
            crate::proxy::pipeline::UpstreamClassification::is_internal_gateway_exhaustion(&lower)
        );
        assert!(
            !crate::proxy::pipeline::UpstreamClassification::is_internal_gateway_exhaustion(
                "some totally unrelated upstream error"
            )
        );
    }

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
            model_priorities: HashMap::new(),
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
            forbidden_models: std::collections::HashSet::new(),
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
            model_reset_times: HashMap::new(),
            model_weekly_reset_times: HashMap::new(),
            model_quota_windows: HashMap::new(),
        }
    }

    #[test]
    fn test_proxy_token_model_forbidden_matching() {
        let mut token = create_test_token("forbidden@test.com", Some("PRO"), 1.0, None, Some(100));
        token
            .forbidden_models
            .insert("claude-3-5-sonnet".to_string());

        assert!(token.is_model_forbidden("claude-3-5-sonnet"));
        assert!(token.is_model_forbidden("claude-3-5-sonnet-thinking"));
        assert!(!token.is_model_forbidden("gemini-3-pro"));
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
            model_priorities: HashMap::new(),
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
            forbidden_models: std::collections::HashSet::new(),
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
            model_reset_times: HashMap::new(),
            model_weekly_reset_times: HashMap::new(),
            model_quota_windows: HashMap::new(),
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
            model_priorities: HashMap::new(),
            protected_models: HashSet::new(),
            forbidden_models: std::collections::HashSet::new(),
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
            model_reset_times: HashMap::new(),
            model_weekly_reset_times: HashMap::new(),
            model_quota_windows: HashMap::new(),
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
            model_priorities: HashMap::new(),
            protected_models: HashSet::new(),
            forbidden_models: std::collections::HashSet::new(),
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
            model_reset_times: HashMap::new(),
            model_weekly_reset_times: HashMap::new(),
            model_quota_windows: HashMap::new(),
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
    async fn test_gate_pool_refresh_sees_newly_imported_mapped_model_account() {
        let temp_dir = tempfile::tempdir().unwrap();
        let tmp_root = temp_dir.path().to_path_buf();
        let accounts_dir = tmp_root.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();
        let now = chrono::Utc::now().timestamp();

        // Start with an empty indexed pool (the exact state that yields no quota accounts).
        let empty_index = serde_json::json!({
            "version": "2.0",
            "accounts": [],
            "current_account_id": null
        });
        std::fs::write(
            tmp_root.join("accounts.json"),
            serde_json::to_string_pretty(&empty_index).unwrap(),
        )
        .unwrap();

        let manager = TokenManager::new(tmp_root.clone());
        manager.load_accounts().await.unwrap();
        assert_eq!(
            manager.accounts_with_positive_quota("gemini-3.8-flash-high"),
            0
        );

        // Simulate an external import (e.g. CLI/headless) that only writes the entity + index.
        let account_id = "acc_newly_imported";
        let account = serde_json::json!({
            "id": account_id,
            "email": "newly@test.com",
            "token": {
                "access_token": "atk-new",
                "refresh_token": "rtk-new",
                "expires_in": 3600,
                "expiry_timestamp": now + 3600,
                "project_id": "pid-new"
            },
            "quota": {
                "models": [{ "name": "gemini-3.8-flash-high", "percentage": 100 }],
                "last_updated": now
            },
            "disabled": false,
            "proxy_disabled": false,
            "created_at": now,
            "last_used": now
        });
        std::fs::write(
            accounts_dir.join(format!("{}.json", account_id)),
            serde_json::to_string_pretty(&account).unwrap(),
        )
        .unwrap();
        let index = serde_json::json!({
            "version": "2.0",
            "accounts": [{
                "id": account_id,
                "email": "newly@test.com",
                "name": null,
                "disabled": false,
                "proxy_disabled": false,
                "protected_models": [],
                "created_at": now,
                "last_used": now
            }],
            "current_account_id": null
        });
        std::fs::write(
            tmp_root.join("accounts.json"),
            serde_json::to_string_pretty(&index).unwrap(),
        )
        .unwrap();

        // The in-memory pool is still stale and would return `no quota accounts` here.
        assert_eq!(
            manager.accounts_with_positive_quota("gemini-3.8-flash-high"),
            0
        );

        // The gate's self-healing refresh must pick up the newly imported account immediately.
        manager.refresh_pool_for_gate_if_stale().await;
        assert_eq!(
            manager.accounts_with_positive_quota("gemini-3.8-flash-high"),
            1,
            "newly imported account with mapped-model quota must release the no-quota gate"
        );
    }

    #[test]
    fn test_model_breaker_clear_after_quota_recovery() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        manager.mark_model_unsupported("acc_recovered", "claude-sonnet-5-5-high", Some(900));
        manager.mark_model_unsupported("acc_recovered", "gemini-3.8-flash-high", Some(900));
        manager.mark_model_unsupported("acc_other", "claude-sonnet-5-5-high", Some(900));

        assert!(manager.is_model_unsupported("acc_recovered", "claude-sonnet-5-5"));
        assert!(manager.is_model_unsupported("acc_recovered", "gemini-3.8-flash"));

        // Quota refresh for one account must release only this account's breakers.
        assert_eq!(
            manager.clear_unsupported_models_for_account("acc_recovered"),
            2
        );
        assert!(!manager.is_model_unsupported("acc_recovered", "claude-sonnet-5-5"));
        assert!(!manager.is_model_unsupported("acc_recovered", "gemini-3.8-flash"));
        assert!(
            manager.is_model_unsupported("acc_other", "claude-sonnet-5-5"),
            "clearing one account must not release other accounts"
        );

        // Successful single-model probe clears exactly that model family.
        manager.mark_model_unsupported("acc_probe", "claude-sonnet-5-5-low", Some(900));
        assert!(manager.clear_model_unsupported("acc_probe", "claude-sonnet-5-5-high"));
        assert!(!manager.is_model_unsupported("acc_probe", "claude-sonnet-5-5"));
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
            model_priorities: HashMap::new(),
            protected_models: HashSet::new(),
            forbidden_models: std::collections::HashSet::new(),
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
            model_reset_times: HashMap::new(),
            model_weekly_reset_times: HashMap::new(),
            model_quota_windows: HashMap::new(),
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
            model_priorities: HashMap::new(),
            protected_models: HashSet::new(),
            forbidden_models: std::collections::HashSet::new(),
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
            model_reset_times: HashMap::new(),
            model_weekly_reset_times: HashMap::new(),
            model_quota_windows: HashMap::new(),
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
    async fn test_sticky_session_preserves_bound_account_while_quota_positive() {
        let temp_dir = tempfile::tempdir().unwrap();
        let accounts_dir = temp_dir.path().join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();

        let acc1_id = "acc_depleted_50";
        let acc2_id = "acc_pristine_100";

        let acc1_path = accounts_dir.join(format!("{}.json", acc1_id));
        let acc2_path = accounts_dir.join(format!("{}.json", acc2_id));

        std::fs::write(&acc1_path, r#"{"disabled":false}"#).unwrap();
        std::fs::write(&acc2_path, r#"{"disabled":false}"#).unwrap();

        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        let target_model = "gemini-4-argon";
        let norm_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());

        let mut quotas1 = HashMap::new();
        quotas1.insert(norm_target.clone(), 50); // 50% 额度

        let mut quotas2 = HashMap::new();
        quotas2.insert(norm_target.clone(), 100); // 100% 额度

        let mut token1 = create_test_token("acc1@test.com", Some("PRO"), 1.0, None, Some(50));
        token1.account_id = acc1_id.to_string();
        token1.account_path = acc1_path;
        token1.model_quotas = quotas1;

        let mut token2 = create_test_token("acc2@test.com", Some("PRO"), 1.0, None, Some(100));
        token2.account_id = acc2_id.to_string();
        token2.account_path = acc2_path;
        token2.model_quotas = quotas2;

        manager.tokens.insert(acc1_id.to_string(), token1);
        manager.tokens.insert(acc2_id.to_string(), token2);

        let session_id = "test-session-quota-preservation";
        // 初始将会话绑定至 acc1
        manager.commit_session(session_id, acc1_id);
        assert_eq!(
            manager.session_accounts.get(session_id).unwrap().as_str(),
            acc1_id
        );

        // 即使 acc1 为 50%，池中有 100% 的 acc2，依然坚守 acc1 复用以保住前缀缓存，绝对不主动迁移换号
        let excluded = HashSet::new();
        let (_token, _project_id, email, selected_id, _wait) = manager
            .get_token_filtered("chat", false, Some(session_id), target_model, &excluded)
            .await
            .expect("Token acquisition must reuse bound account as long as quota > 0");

        assert_eq!(
            selected_id, acc1_id,
            "绑定账号配额大于 0% 且未限流时，必须坚决复用原绑定账号，绝不得因配额差距主动解绑"
        );
        assert_eq!(email, "acc1@test.com");
        assert_eq!(
            manager.session_accounts.get(session_id).unwrap().as_str(),
            acc1_id,
            "会话绑定状态必须持续保留"
        );

        // 绑定账号额度为 80% 时，依然保持复用
        let session_high_quota = "test-session-cache-preserved";
        if let Some(mut t1) = manager.tokens.get_mut(acc1_id) {
            t1.model_quotas.insert(norm_target.clone(), 80);
        }
        manager.commit_session(session_high_quota, acc1_id);
        let (_token, _project_id, email, selected_id, _wait) = manager
            .get_token_filtered(
                "chat",
                false,
                Some(session_high_quota),
                target_model,
                &excluded,
            )
            .await
            .expect("Token acquisition must preserve cache when bound account has >65% quota");
        assert_eq!(
            selected_id, acc1_id,
            "配额高于 65% 时必须坚守复用以最大化前缀缓存命中率"
        );
        assert_eq!(email, "acc1@test.com");
    }

    #[tokio::test]
    async fn test_sticky_session_unbinds_and_fails_over_when_bound_account_forbidden_or_banned() {
        let temp_dir = tempfile::tempdir().unwrap();
        let accounts_dir = temp_dir.path().join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();

        let acc1_id = "acc_bound_forbidden";
        let acc2_id = "acc_healthy_alt";

        let acc1_path = accounts_dir.join(format!("{}.json", acc1_id));
        let acc2_path = accounts_dir.join(format!("{}.json", acc2_id));

        std::fs::write(&acc1_path, r#"{"disabled":false}"#).unwrap();
        std::fs::write(&acc2_path, r#"{"disabled":false}"#).unwrap();

        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        let target_model = "gemini-4-argon";
        let norm_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());

        let mut quotas1 = HashMap::new();
        quotas1.insert(norm_target.clone(), 80); // 即使有 80% 配额
        let mut quotas2 = HashMap::new();
        quotas2.insert(norm_target.clone(), 90);

        let mut token1 =
            create_test_token("bound_forbidden@test.com", Some("PRO"), 1.0, None, Some(80));
        token1.account_id = acc1_id.to_string();
        token1.account_path = acc1_path.clone();
        token1.model_quotas = quotas1;

        let mut token2 =
            create_test_token("healthy_alt@test.com", Some("PRO"), 1.0, None, Some(90));
        token2.account_id = acc2_id.to_string();
        token2.account_path = acc2_path;
        token2.model_quotas = quotas2;

        manager.tokens.insert(acc1_id.to_string(), token1);
        manager.tokens.insert(acc2_id.to_string(), token2);

        let session_id = "test-session-failover-forbidden";
        // 初始将会话绑定至 acc1
        manager.commit_session(session_id, acc1_id);

        // 场景 1：acc1 遭遇模型级 403 冻结（进入 forbidden_models），即使配额为 80% 也必须立即解绑并切号到 acc2
        if let Some(mut t1) = manager.tokens.get_mut(acc1_id) {
            t1.forbidden_models.insert(norm_target.clone());
        }

        let excluded = HashSet::new();
        let (_token, _project_id, email, selected_id, _wait) = manager
            .get_token_filtered("chat", false, Some(session_id), target_model, &excluded)
            .await
            .expect("Token acquisition must failover to healthy account when bound account is forbidden");

        assert_eq!(
            selected_id, acc2_id,
            "绑定账号即使配额 > 0%，一旦目标模型被 403 冻结，必须无缝解绑并故障转移至健康账号"
        );
        assert_eq!(email, "healthy_alt@test.com");
        assert_eq!(
            manager.session_accounts.get(session_id).unwrap().as_str(),
            acc2_id,
            "会话必须被自动重新绑定至健康的候选账号 acc2"
        );

        // 场景 2：acc1 在运行期被上游 TOS 封禁（写入 banned）并从内存池移除
        // 再次将新会话绑定至 acc1
        let session_id_banned = "test-session-failover-banned";
        manager.commit_session(session_id_banned, acc1_id);

        // 模拟账号被踢出内存调度池且磁盘标记 banned
        manager.remove_account(acc1_id);
        std::fs::write(&acc1_path, r#"{"disabled":false,"banned":true}"#).unwrap();

        let (_token2, _project_id2, email2, selected_id2, _wait2) = manager
            .get_token_filtered(
                "chat",
                false,
                Some(session_id_banned),
                target_model,
                &excluded,
            )
            .await
            .expect("Token acquisition must failover when bound account is purged/banned");

        assert_eq!(
            selected_id2, acc2_id,
            "绑定账号被封禁移出号池后，必须无缝解绑并切换到下一个可用账号"
        );
        assert_eq!(email2, "healthy_alt@test.com");
        assert_eq!(
            manager
                .session_accounts
                .get(session_id_banned)
                .unwrap()
                .as_str(),
            acc2_id
        );
    }

    #[tokio::test]
    async fn test_60s_window_preserves_last_account_while_quota_positive() {
        let temp_dir = tempfile::tempdir().unwrap();
        let accounts_dir = temp_dir.path().join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();

        let acc1_id = "acc_mode_b_50";
        let acc2_id = "acc_mode_b_100";

        let acc1_path = accounts_dir.join(format!("{}.json", acc1_id));
        let acc2_path = accounts_dir.join(format!("{}.json", acc2_id));

        std::fs::write(&acc1_path, r#"{"disabled":false}"#).unwrap();
        std::fs::write(&acc2_path, r#"{"disabled":false}"#).unwrap();

        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        let target_model = "gemini-4-argon";
        let norm_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());

        let mut quotas1 = HashMap::new();
        quotas1.insert(norm_target.clone(), 50); // 50% 额度

        let mut quotas2 = HashMap::new();
        quotas2.insert(norm_target.clone(), 100); // 100% 额度

        let mut token1 = create_test_token("acc1@test.com", Some("PRO"), 1.0, None, Some(50));
        token1.account_id = acc1_id.to_string();
        token1.account_path = acc1_path;
        token1.model_quotas = quotas1;

        let mut token2 = create_test_token("acc2@test.com", Some("PRO"), 1.0, None, Some(100));
        token2.account_id = acc2_id.to_string();
        token2.account_path = acc2_path;
        token2.model_quotas = quotas2;

        manager.tokens.insert(acc1_id.to_string(), token1);
        manager.tokens.insert(acc2_id.to_string(), token2);

        // 模拟 60s 窗口内上一请求使用了 acc1
        {
            let mut last_used = manager.last_used_account.lock().await;
            *last_used = Some((acc1_id.to_string(), std::time::Instant::now()));
        }

        // 无 session_id 请求（触发模式 B 60s 全局锁定）
        // 只要 acc1 配额 > 0% 且未限流，60s 窗口必须坚守 acc1 复用，不得因 acc2 额度更高而切号
        let excluded = HashSet::new();
        let (_token, _project_id, email, selected_id, _wait) = manager
            .get_token_filtered("chat", false, None, target_model, &excluded)
            .await
            .expect("60s window should reuse last account as long as quota > 0");

        assert_eq!(
            selected_id, acc1_id,
            "60s 窗口必须优先复用最近账号以保住缓存，不得因其他账号配额更高主动解绑"
        );
        assert_eq!(email, "acc1@test.com");
    }

    #[tokio::test]
    async fn test_cache_first_mode_preserves_sticky_session_and_handles_qos_wait() {
        use crate::proxy::sticky_config::{SchedulingMode, StickySessionConfig};

        let temp_dir = tempfile::tempdir().unwrap();
        let accounts_dir = temp_dir.path().join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();

        let acc1_id = "acc_bound_50";
        let acc2_id = "acc_idle_100";

        let acc1_path = accounts_dir.join(format!("{}.json", acc1_id));
        let acc2_path = accounts_dir.join(format!("{}.json", acc2_id));

        std::fs::write(&acc1_path, r#"{"disabled":false}"#).unwrap();
        std::fs::write(&acc2_path, r#"{"disabled":false}"#).unwrap();

        let manager = TokenManager::new(temp_dir.path().to_path_buf());
        manager
            .update_sticky_config(StickySessionConfig {
                mode: SchedulingMode::CacheFirst,
                max_wait_seconds: 60,
                request_429_grace_ms: 1500,
                max_spillover_accounts: 0,
                affinity_scope: crate::proxy::sticky_config::AffinityScope::Session,
            })
            .await;

        let target_model = "gemini-4-argon";
        let norm_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());

        let mut quotas1 = HashMap::new();
        quotas1.insert(norm_target.clone(), 50);

        let mut quotas2 = HashMap::new();
        quotas2.insert(norm_target.clone(), 100);

        let mut token1 = create_test_token("acc1@test.com", Some("PRO"), 1.0, None, Some(50));
        token1.account_id = acc1_id.to_string();
        token1.account_path = acc1_path;
        token1.model_quotas = quotas1;

        let mut token2 = create_test_token("acc2@test.com", Some("PRO"), 1.0, None, Some(100));
        token2.account_id = acc2_id.to_string();
        token2.account_path = acc2_path;
        token2.model_quotas = quotas2;

        manager.tokens.insert(acc1_id.to_string(), token1);
        manager.tokens.insert(acc2_id.to_string(), token2);

        let session_id = "test-session-cache-first-strict";
        manager.commit_session(session_id, acc1_id);

        let excluded = HashSet::new();
        let (_token, _project_id, email, selected_id, _wait) = manager
            .get_token_filtered("chat", false, Some(session_id), target_model, &excluded)
            .await
            .expect("CacheFirst must reuse bound account even when pool has higher quota account");

        assert_eq!(
            selected_id, acc1_id,
            "CacheFirst 模式下即使绑定账号配额 50% <= 65% 且池中有 100% 账号，也必须坚守绑定账号以保护 KV 缓存"
        );
        assert_eq!(email, "acc1@test.com");
        assert_eq!(
            manager.session_accounts.get(session_id).unwrap().as_str(),
            acc1_id
        );

        // 进一步验证：CacheFirst 模式下即使触发限速小黑屋 (QoS Jailed)，也绝不解绑会话换下一个账号，而是原地等待后继续复用
        manager.set_qos_auto_jail(true);
        manager
            .update_sticky_config(StickySessionConfig {
                mode: SchedulingMode::CacheFirst,
                max_wait_seconds: 1,
                request_429_grace_ms: 1500,
                max_spillover_accounts: 0,
                affinity_scope: crate::proxy::sticky_config::AffinityScope::Session,
            })
            .await;
        // 连续两次慢首字触发 QoS Jailed
        manager.record_qos_ttft(acc1_id, Some(session_id), "PRO", 50_000.0, false, Some(100));
        manager.record_qos_ttft(acc1_id, Some(session_id), "PRO", 50_000.0, false, Some(100));
        assert!(
            manager.qos_guardian.is_jailed_or_probing(acc1_id),
            "acc1 应已进入 QoS 小黑屋"
        );
        assert_eq!(
            manager
                .session_accounts
                .get(session_id)
                .map(|v| v.clone())
                .as_deref(),
            Some(acc1_id),
            "CacheFirst 模式下进入 QoS 小黑屋时绝不能解除会话绑定"
        );

        let (_token, _project_id, email_after_jail, selected_after_jail, _wait) = manager
            .get_token_filtered("chat", false, Some(session_id), target_model, &excluded)
            .await
            .expect("CacheFirst must wait in-place and reuse bound account when QoS jailed");
        assert_eq!(
            selected_after_jail, acc1_id,
            "CacheFirst 模式下触发限速小黑屋后必须原地等待并继续复用原绑定账号，不得切换到 acc2"
        );
        assert_eq!(email_after_jail, "acc1@test.com");
        assert!(
            manager.qos_guardian.is_half_open(acc1_id),
            "CacheFirst 原地等待结束后账号应转入 HalfOpen 观察缓刑期继续服务本会话"
        );
    }

    #[tokio::test]
    async fn test_balance_mode_preserves_bound_account_regardless_of_higher_quota_accounts() {
        let temp_dir = tempfile::tempdir().unwrap();
        let accounts_dir = temp_dir.path().join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();

        let acc_bound_51 = "acc_1_bound_51";
        let acc_low_49 = "acc_2_low_49";
        let acc_busy_82 = "acc_3_busy_82";

        for id in [acc_bound_51, acc_low_49, acc_busy_82] {
            let p = accounts_dir.join(format!("{}.json", id));
            std::fs::write(&p, r#"{"disabled":false}"#).unwrap();
        }

        let manager = TokenManager::new(temp_dir.path().to_path_buf());
        let target_model = "gemini-4-argon";
        let norm_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());

        let make_tok = |id: &str, email: &str, q: i32| {
            let mut quotas = HashMap::new();
            quotas.insert(norm_target.clone(), q);
            let mut t = create_test_token(email, Some("PRO"), 1.0, None, Some(q));
            t.account_id = id.to_string();
            t.account_path = accounts_dir.join(format!("{}.json", id));
            t.model_quotas = quotas;
            t
        };

        manager.tokens.insert(
            acc_bound_51.to_string(),
            make_tok(acc_bound_51, "mcl51@test.com", 51),
        );
        manager.tokens.insert(
            acc_low_49.to_string(),
            make_tok(acc_low_49, "che49@test.com", 49),
        );
        manager.tokens.insert(
            acc_busy_82.to_string(),
            make_tok(acc_busy_82, "ity82@test.com", 82),
        );

        let session_id = "test-session-no-ping-pong";
        manager.commit_session(session_id, acc_bound_51);
        let excluded = HashSet::new();

        // 1. 当 82% 的账号处于 in_flight > 0 时，绝不能解绑 51% 账号
        {
            let _busy_guard = manager.acquire_in_flight(acc_busy_82);
            let (_tok, _pid, email, selected_id, _w) = manager
                .get_token_filtered("chat", false, Some(session_id), target_model, &excluded)
                .await
                .unwrap();
            assert_eq!(
                selected_id, acc_bound_51,
                "当 82% 账号正忙 (in_flight > 0) 时不得解绑 51% 账号去换 49% 账号引发踢皮球"
            );
            assert_eq!(email, "mcl51@test.com");
        }

        // 2. 当 82% 的账号释放并发锁 (in_flight == 0) 且处于 Healthy 时，
        // 只要 51% 账号配额 > 0% 且未限流，Balance 模式依然坚决复用原绑定账号保住 KV 缓存，绝不因号池存在更高额度账号而跳号。
        let (_tok, _pid, email, selected_id, _w) = manager
            .get_token_filtered("chat", false, Some(session_id), target_model, &excluded)
            .await
            .unwrap();
        assert_eq!(
            selected_id, acc_bound_51,
            "只要绑定账号有额度，Balance 模式必须坚守原绑定账号以保护 KV 缓存，绝不主动解绑"
        );
        assert_eq!(email, "mcl51@test.com");
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
    fn test_accounts_with_positive_quota_3p_isolation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        let account_id = "acc_3p_depleted";
        let mut quotas = HashMap::new();
        // 3P 模型（Claude）5H 耗尽为 0
        quotas.insert("claude".to_string(), 0);
        quotas.insert("claude-sonnet-4-6".to_string(), 0);
        // 但 Gemini 通用大池拥有 92% 配额
        quotas.insert("gemini-3.8-flash-tiered".to_string(), 92);
        quotas.insert("gemini-3.8-flash-high".to_string(), 92);

        let mut token = create_test_token("hybrid@test.com", Some("PRO"), 1.0, None, Some(92));
        token.account_id = account_id.to_string();
        token.model_quotas = quotas;
        token
            .available_models
            .insert("gemini-3.8-flash-high".to_string());
        token
            .available_models
            .insert("gemini-3.8-flash-tiered".to_string());
        token
            .available_models
            .insert("claude-sonnet-4-6".to_string());

        manager.tokens.insert(account_id.to_string(), token);

        // 1. 3P 模型应准确返回 0 配额
        assert_eq!(
            manager.accounts_with_positive_quota("claude-sonnet-4-6"),
            0,
            "Depleted 3P model must return 0 quota"
        );

        // 2. 核心大池隔离验证：Gemini 通用大池的请求绝不受 3P 耗尽影响，必须判定为具备正配额
        assert_eq!(
            manager.accounts_with_positive_quota("gemini-3.8-flash-high"),
            1,
            "Gemini request must NOT be blocked when 3P quota is depleted"
        );
        assert_eq!(
            manager.accounts_with_positive_quota("gemini-3.8-flash-tiered"),
            1
        );
    }

    #[test]
    fn test_accounts_with_positive_quota_argon_heterogeneous_dispatch() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        // 1. 无 Argon 的 Pro 账号：仅有 Gemini 通用配额，无 Argon 授权
        let mut token_pro_no_argon =
            create_test_token("pro_no_argon@test.com", Some("PRO"), 1.0, None, Some(80));
        token_pro_no_argon.account_id = "pro_no_argon".to_string();
        token_pro_no_argon
            .model_quotas
            .insert("gemini-3.8-flash-high".to_string(), 80);
        token_pro_no_argon
            .available_models
            .insert("gemini-3.8-flash-high".to_string());
        manager
            .tokens
            .insert("pro_no_argon".to_string(), token_pro_no_argon);

        // 2. 有独立 Argon 桶的 Pro 账号：有独立的 gemini-4-argon 配额 50%
        let mut token_pro_with_argon =
            create_test_token("pro_with_argon@test.com", Some("PRO"), 1.0, None, Some(80));
        token_pro_with_argon.account_id = "pro_with_argon".to_string();
        token_pro_with_argon
            .model_quotas
            .insert("gemini-4-argon".to_string(), 50);
        token_pro_with_argon
            .model_quotas
            .insert("gemini-3.8-flash-high".to_string(), 80);
        token_pro_with_argon
            .available_models
            .insert("gemini-4-argon".to_string());
        token_pro_with_argon
            .available_models
            .insert("gemini-3.8-flash-high".to_string());
        manager
            .tokens
            .insert("pro_with_argon".to_string(), token_pro_with_argon);

        // 3. Ultra 账号：无独立桶，但享有 Argon 能力，配额共享通用大池 95%
        let mut token_ultra =
            create_test_token("ultra@test.com", Some("ULTRA"), 1.0, None, Some(95));
        token_ultra.account_id = "ultra".to_string();
        token_ultra
            .model_quotas
            .insert("gemini-4-argon".to_string(), 95);
        token_ultra
            .model_quotas
            .insert("gemini-3.8-flash-high".to_string(), 95);
        token_ultra
            .available_models
            .insert("gemini-4-argon".to_string());
        token_ultra
            .available_models
            .insert("gemini-3.8-flash-high".to_string());
        manager.tokens.insert("ultra".to_string(), token_ultra);

        // 验证 Argon 请求能力探测：
        // 无 Argon 的 Pro 被精确排除，具备能力的 2 个账号（有独立桶 Pro + Ultra）通过闸门
        assert_eq!(
            manager.accounts_with_positive_quota("gemini-4-argon"),
            2,
            "Only Pro with Argon bucket and Ultra account should be counted for Argon"
        );

        // 验证通用 Gemini 请求：3 个账号全部合格放行
        assert_eq!(
            manager.accounts_with_positive_quota("gemini-3.8-flash-high"),
            3,
            "All three accounts should be eligible for general Gemini requests"
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
    fn test_zero_quota_high_priority_account_must_not_starve_pool() {
        // [FIX #3840] 回归：优先级倒置 / 饥饿死锁
        //
        // 场景（蹬额度模式 pedal 或用户手动设高优先级均可复现）：
        //   A: priority=1（最高优先级，空闲 in_flight=0），但目标模型配额 = 0%
        //   B: priority=50（默认优先级），目标模型配额 = 100%
        //
        // 旧行为（配额过滤在优先级收敛之后）：
        //   优先级收敛先执行 available = min_p_idle → 只剩 A；
        //   随后配额检查只看到 A 是 0%，has_positive_model_quota=false
        //   → return None，报「无可用账号」，B 被活活饿死。
        //
        // 新行为（配额过滤前置）：
        //   A 因 0 配额先被剔除，B 接管 → 必须返回 B。
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());
        let model = "claude-sonnet-4-6";

        let mut boosted = create_test_token("pedal@example.com", Some("PRO"), 1.0, None, Some(0));
        boosted.account_id = "acc_pedal_drained".to_string();
        boosted.priority = 1; // 蹬额度模式把它提到最高优先级
        boosted.model_quotas.insert(model.to_string(), 0); // 但配额已耗尽

        let mut healthy =
            create_test_token("healthy@example.com", Some("PRO"), 1.0, None, Some(100));
        healthy.account_id = "acc_healthy".to_string();
        healthy.priority = 50; // 默认优先级
        healthy.model_quotas.insert(model.to_string(), 100);

        let candidates = vec![boosted.clone(), healthy.clone()];
        let attempted = HashSet::new();

        let selected = manager
            .select_with_p2c(&candidates, &attempted, model, false)
            .expect("必须回退到有正配额的账号，而不是返回 None");
        assert_eq!(
            selected.account_id, "acc_healthy",
            "高优先级但 0 配额的账号绝不能饿死全池"
        );

        // 反向验证：当高优先级账号真正有配额时，优先级仍然生效
        let mut boosted_with_quota = boosted.clone();
        boosted_with_quota
            .model_quotas
            .insert(model.to_string(), 80);
        let both_healthy = vec![boosted_with_quota, healthy.clone()];
        let selected2 = manager
            .select_with_p2c(&both_healthy, &attempted, model, false)
            .expect("两者都有配额时应正常选中");
        assert_eq!(
            selected2.account_id, "acc_pedal_drained",
            "配额健康时，最高优先级仍应优先命中"
        );

        // 安全网：全池 0 配额时仍必须坚定拦截（不打爆 Google）
        let mut all_zero_a = boosted.clone();
        all_zero_a.model_quotas.insert(model.to_string(), 0);
        let mut all_zero_b = healthy.clone();
        all_zero_b.model_quotas.insert(model.to_string(), 0);
        let all_zero = vec![all_zero_a, all_zero_b];
        assert!(
            manager
                .select_with_p2c(&all_zero, &attempted, model, false)
                .is_none(),
            "全池均为 0 配额时必须本地拦截"
        );
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
            "gemini-3.8-flash-high".to_string(),
            crate::models::quota::ModelListingMeta {
                display_name: Some("Gemini 3.8 Flash (High)".to_string()),
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
            model_priorities: HashMap::new(),
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
            forbidden_models: std::collections::HashSet::new(),
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
            model_reset_times: HashMap::new(),
            model_weekly_reset_times: HashMap::new(),
            model_quota_windows: HashMap::new(),
        };

        manager.tokens.insert(token.account_id.clone(), token);

        // 1. 测试 TokenManager 聚合
        let collected = manager.get_all_model_metadata();
        assert!(collected.contains_key("gemini-3.8-flash-high"));
        assert_eq!(
            collected["gemini-3.8-flash-high"].supports_images,
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

        // 裸模型派生：模型清单随版本演进，改为“至少存在一个裸模型且元数据完整”
        assert!(!all_meta.is_empty(), "派生元数据表不得为空");
        assert!(
            all_meta
                .values()
                .next()
                .map(|m| m.display_name.is_some())
                .unwrap_or(false),
            "派生条目必须有 display_name"
        );
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

    /// 账号模型清单缓存必须以「长度 + mtime」失效，避免门禁热路径每请求 read+parse。
    #[test]
    fn test_available_models_file_cache_invalidates_on_change() {
        let path = std::env::temp_dir().join(format!(
            "am-account-model-cache-{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&path, r#"{"quota":{"models":[{"name":"Model-A"}]}}"#).unwrap();

        let first =
            TokenManager::get_available_models_from_json(&path).expect("first parse must succeed");
        assert!(first.contains("model-a"));

        // 文件未变化：命中缓存，结果一致
        let second = TokenManager::get_available_models_from_json(&path).unwrap();
        assert_eq!(first, second);

        // 内容变化（长度不同）：必须立即失效
        std::fs::write(
            &path,
            r#"{"quota":{"models":[{"name":"Model-B"},{"name":"Model-C"}]}}"#,
        )
        .unwrap();
        let third = TokenManager::get_available_models_from_json(&path).unwrap();
        assert!(third.contains("model-b") && third.contains("model-c"));
        assert!(!third.contains("model-a"));

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_load_accounts_skips_and_cleans_orphan_files_not_in_index() {
        let temp_dir =
            std::env::temp_dir().join(format!("test_orphan_accounts_{}", uuid::Uuid::new_v4()));
        let accounts_dir = temp_dir.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();

        let now = chrono::Utc::now().timestamp();
        let mut acc_valid = weekly_quota_account(now);
        acc_valid["id"] = serde_json::json!("acc-valid-1");
        acc_valid["email"] = serde_json::json!("valid@example.com");
        std::fs::write(accounts_dir.join("acc-valid-1.json"), acc_valid.to_string()).unwrap();

        let mut acc_orphan = weekly_quota_account(now);
        acc_orphan["id"] = serde_json::json!("acc-orphan-ghost");
        acc_orphan["email"] = serde_json::json!("orphan@example.com");
        std::fs::write(
            accounts_dir.join("acc-orphan-ghost.json"),
            acc_orphan.to_string(),
        )
        .unwrap();

        // 写入 accounts.json 索引，只包含 acc-valid-1，不包含 acc-orphan-ghost
        let index_json = serde_json::json!({
            "version": "2.0",
            "accounts": [
                {
                    "id": "acc-valid-1",
                    "email": "valid@example.com",
                    "name": null,
                    "disabled": false,
                    "proxy_disabled": false,
                    "created_at": now,
                    "last_used": now
                }
            ],
            "current_account_id": "acc-valid-1"
        });
        std::fs::write(temp_dir.join("accounts.json"), index_json.to_string()).unwrap();

        let manager = TokenManager::new(temp_dir.clone());
        let count = manager.load_accounts().await.unwrap();

        // [FIX #3806, #3808] 仅有效账号载入，孤儿账号被跳过载入调度池，但保留物理文件零副作用
        assert_eq!(count, 1);
        assert!(manager.tokens.contains_key("acc-valid-1"));
        assert!(!manager.tokens.contains_key("acc-orphan-ghost"));
        assert!(accounts_dir.join("acc-valid-1.json").exists());
        assert!(accounts_dir.join("acc-orphan-ghost.json").exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_hard_quota_depleted_locks_and_rotates_even_with_circuit_breaker_disabled() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        // 用户在配置界面关闭了自适应熔断器 (circuit_breaker.enabled = false)
        manager.circuit_breaker_config.write().await.enabled = false;

        let target_model = "claude-opus-5-5-high";
        let norm_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());

        let mut quotas1 = HashMap::new();
        quotas1.insert(norm_target.clone(), 50);
        quotas1.insert(target_model.to_string(), 50);

        let mut quotas2 = HashMap::new();
        quotas2.insert(norm_target.clone(), 40);
        quotas2.insert(target_model.to_string(), 40);

        let acc1_path = temp_dir.path().join("acc1.json");
        let acc2_path = temp_dir.path().join("acc2.json");
        std::fs::write(
            &acc1_path,
            serde_json::json!({"disabled": false}).to_string(),
        )
        .unwrap();
        std::fs::write(
            &acc2_path,
            serde_json::json!({"disabled": false}).to_string(),
        )
        .unwrap();

        let mut token1 = create_test_token("acc1@example.com", Some("PRO"), 1.0, None, Some(50));
        token1.account_id = "acc1".to_string();
        token1.account_path = acc1_path;
        token1.model_quotas = quotas1;
        token1.available_models.insert(target_model.to_string());
        token1.available_models.insert(norm_target.clone());

        let mut token2 = create_test_token("acc2@example.com", Some("PRO"), 1.0, None, Some(40));
        token2.account_id = "acc2".to_string();
        token2.account_path = acc2_path;
        token2.model_quotas = quotas2;
        token2.available_models.insert(target_model.to_string());
        token2.available_models.insert(norm_target.clone());

        manager.tokens.insert("acc1".to_string(), token1);
        manager.tokens.insert("acc2".to_string(), token2);

        // 首次调度：acc1 (50%) 优于 acc2 (40%)，被选中
        let (_token, _proj, email1, id1, _) = manager
            .get_token("claude", false, None, target_model)
            .await
            .expect("First selection should succeed");
        assert_eq!(id1, "acc1");
        assert_eq!(email1, "acc1@example.com");

        // 上游返回 429 Individual quota reached
        let upstream_429 = r#"{
            "error": {
                "code": 429,
                "message": "Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 4h52m54s.",
                "status": "RESOURCE_EXHAUSTED",
                "details": [
                    {
                        "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                        "reason": "QUOTA_EXHAUSTED",
                        "metadata": {
                            "quotaResetDelay": "17574s",
                            "quotaResetTimeStamp": "2026-11-06T04:00:00Z"
                        }
                    }
                ]
            }
        }"#;

        // 标记限流
        manager
            .mark_rate_limited_async_baseline(
                "acc1@example.com",
                429,
                None,
                upstream_429,
                Some(target_model),
            )
            .await;

        // 核心断言：即使 circuit_breaker.enabled == false，
        // 上游明确下发的配额耗尽 (QUOTA_EXHAUSTED / Individual quota reached) 依然必须标记 0 配额并精确锁定，
        // 随后调度轮换必须自动剔除 acc1 并命中健康的 acc2！
        let (_token, _proj, email2, id2, _) = manager
            .get_token("claude", true, None, target_model)
            .await
            .expect("Rotation after 429 must succeed with acc2");
        assert_eq!(id2, "acc2", "429 配额耗尽后绝不能重复选中已耗尽的 acc1");
        assert_eq!(email2, "acc2@example.com");
    }

    #[tokio::test]
    async fn test_get_token_filtered_excludes_attempted_accounts_in_retry_loop() {
        let temp_dir = tempfile::tempdir().unwrap();
        let acc1_file = temp_dir.path().join("acc1.json");
        let acc2_file = temp_dir.path().join("acc2.json");
        std::fs::write(&acc1_file, r#"{"disabled": false}"#).unwrap();
        std::fs::write(&acc2_file, r#"{"disabled": false}"#).unwrap();

        let manager = TokenManager::new(temp_dir.path().to_path_buf());
        let target_model = "claude-opus-5-5-high";
        let norm_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());

        let mut token1 = create_test_token("acc1@example.com", Some("PRO"), 1.0, None, Some(80));
        token1.account_id = "acc1".to_string();
        token1.account_path = acc1_file;
        token1.available_models.insert(target_model.to_string());
        token1.available_models.insert(norm_target.clone());
        token1.model_quotas.insert(target_model.to_string(), 80);
        token1.model_quotas.insert(norm_target.clone(), 80);
        token1.remaining_quota = Some(80);

        let mut token2 = create_test_token("acc2@example.com", Some("PRO"), 1.0, None, Some(40));
        token2.account_id = "acc2".to_string();
        token2.account_path = acc2_file;
        token2.available_models.insert(target_model.to_string());
        token2.available_models.insert(norm_target.clone());
        token2.model_quotas.insert(target_model.to_string(), 40);
        token2.model_quotas.insert(norm_target.clone(), 40);
        token2.remaining_quota = Some(40);

        manager.tokens.insert("acc1".to_string(), token1);
        manager.tokens.insert("acc2".to_string(), token2);

        let mut attempted = std::collections::HashSet::new();
        // 第一次选号：没有被排除，acc1 额度更高 (80%) 优先被选中
        let (_, _, _, id1, _) = manager
            .get_token_filtered("claude", false, None, target_model, &attempted)
            .await
            .unwrap();
        assert_eq!(id1, "acc1");

        // 模拟第 1 次尝试失败并将 acc1 记入 attempted_accounts
        attempted.insert(id1);

        // 第二次重试选号：传入已尝试账号，调度器必须排除 acc1 并选中 acc2
        let (_, _, _, id2, _) = manager
            .get_token_filtered("claude", true, None, target_model, &attempted)
            .await
            .unwrap();
        assert_eq!(id2, "acc2", "重试循环中必须排除当前请求中已尝试失败的账号");

        // 当所有账号都被尝试过时，返回池空错误
        attempted.insert(id2);
        let res3 = manager
            .get_token_filtered("claude", true, None, target_model, &attempted)
            .await;
        assert!(res3.is_err());
    }

    #[tokio::test]
    async fn test_quota_weight_recovery_after_reset_and_snapshot_sync() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());
        let target_model = "gemini-3.8-flash-high";
        let norm_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());

        let acc1_file = temp_dir.path().join("acc1.json");
        let acc2_file = temp_dir.path().join("acc2.json");
        std::fs::write(&acc1_file, r#"{"disabled": false}"#).unwrap();
        std::fs::write(&acc2_file, r#"{"disabled": false}"#).unwrap();

        let now_ts = chrono::Utc::now().timestamp();

        // 账号 1: 原本 90% 配额，但被标记为 0%（模拟 429 耗尽），设重置时间为 1 小时后（确定性时间，P1-5）
        let mut token1 = create_test_token("acc1@example.com", Some("PRO"), 1.0, None, Some(90));
        token1.account_id = "acc1".to_string();
        token1.account_path = acc1_file;
        token1.available_models.insert(target_model.to_string());
        token1.available_models.insert(norm_target.clone());
        token1.model_quotas.insert(target_model.to_string(), 0);
        token1.model_quotas.insert(norm_target.clone(), 0);
        token1
            .model_reset_times
            .insert("gemini".to_string(), now_ts + 3600);

        // 账号 2: 保持 40% 的正常配额
        let mut token2 = create_test_token("acc2@example.com", Some("PRO"), 1.0, None, Some(40));
        token2.account_id = "acc2".to_string();
        token2.account_path = acc2_file;
        token2.available_models.insert(target_model.to_string());
        token2.available_models.insert(norm_target.clone());
        token2.model_quotas.insert(target_model.to_string(), 40);
        token2.model_quotas.insert(norm_target.clone(), 40);

        manager.tokens.insert("acc1".to_string(), token1.clone());
        manager.tokens.insert("acc2".to_string(), token2.clone());

        // 阶段 1：重置时间尚未到达（now_ts < reset_time），acc1 处于 0% 耗尽中
        // P2C 必须过滤掉 acc1，仅能选中 acc2
        let candidates = vec![
            manager.tokens.get("acc1").unwrap().clone(),
            manager.tokens.get("acc2").unwrap().clone(),
        ];
        let attempted = HashSet::new();
        let sel_before = manager.select_with_p2c(&candidates, &attempted, target_model, false);
        assert_eq!(
            sel_before.unwrap().account_id,
            "acc2",
            "重置前耗尽账号绝对不能抢占正常账号"
        );

        // 阶段 2：重置时间到达（通过过期自愈检测）
        // 模拟时间流逝（重置时间设在过去 10 秒）
        let mut t1_expired = manager.tokens.get_mut("acc1").unwrap();
        t1_expired
            .model_reset_times
            .insert("gemini".to_string(), now_ts - 10);
        drop(t1_expired);

        let t1_ref = manager.tokens.get("acc1").unwrap();
        assert!(
            TokenManager::is_model_quota_depleted_expired(&t1_ref, target_model),
            "达到重置时间后必须自动识别为已过期可自愈"
        );
        let has_adm = TokenManager::has_candidate_quota_admission(&t1_ref, &norm_target);
        assert!(has_adm, "达到重置时间后必须具备准入资格");
        let recovered_quota = TokenManager::get_candidate_quota(&t1_ref, &norm_target);
        assert_eq!(recovered_quota, 20, "自愈检测应赋予保守正向 sentinel (20)");
        drop(t1_ref);

        // 此时 P2C 调度：自愈账号 (sentinel 20) 面对耗尽账号 (0%) 可以胜出
        let mut token3 = create_test_token("acc3@example.com", Some("PRO"), 1.0, None, Some(0));
        token3.account_id = "acc3".to_string();
        token3.available_models.insert(target_model.to_string());
        token3.model_quotas.insert(target_model.to_string(), 0);
        manager.tokens.insert("acc3".to_string(), token3);

        let candidates_with_exhausted = vec![
            manager.tokens.get("acc1").unwrap().clone(),
            manager.tokens.get("acc3").unwrap().clone(),
        ];
        let sel_self_healed =
            manager.select_with_p2c(&candidates_with_exhausted, &attempted, target_model, false);
        assert_eq!(
            sel_self_healed.unwrap().account_id,
            "acc1",
            "自愈账号赋予正向 sentinel 后面对完全耗尽账号必须胜出"
        );

        // 阶段 3：上游下发全量正配额 snapshot（新周期快照同步）
        // 验证 sync_zero_quota_circuit_breaker 能够精准清理 QuotaExhausted 限制，同时严格保留 RateLimitExceeded (429)
        manager.rate_limit_tracker.set_lockout_until_with_cap(
            "acc1",
            std::time::SystemTime::now() + std::time::Duration::from_secs(3600),
            crate::proxy::rate_limit::RateLimitReason::QuotaExhausted,
            Some(target_model.to_string()),
            false,
        );
        manager.rate_limit_tracker.set_lockout_until_with_cap(
            "acc1",
            std::time::SystemTime::now() + std::time::Duration::from_secs(3600),
            crate::proxy::rate_limit::RateLimitReason::RateLimitExceeded,
            Some("other-model".to_string()),
            false,
        );
        assert!(manager
            .rate_limit_tracker
            .is_rate_limited("acc1", Some(target_model)));
        assert!(manager
            .rate_limit_tracker
            .is_rate_limited("acc1", Some("other-model")));

        let recovery_snapshot = serde_json::json!({
            "quota": {
                "last_updated": now_ts + 10,
                "quota_groups": [
                    {
                        "displayName": "Gemini",
                        "buckets": [
                            {
                                "bucketId": "gemini_5h",
                                "window": "5h",
                                "remainingFraction": 0.85,
                                "resetTime": (chrono::Utc::now() + chrono::Duration::hours(5)).to_rfc3339()
                            },
                            {
                                "bucketId": "gemini_weekly",
                                "window": "7d",
                                "remainingFraction": 0.90,
                                "resetTime": (chrono::Utc::now() + chrono::Duration::days(6)).to_rfc3339()
                            }
                        ]
                    }
                ]
            }
        });

        manager.sync_zero_quota_circuit_breaker("acc1", &recovery_snapshot);
        assert!(
            !manager
                .rate_limit_tracker
                .is_rate_limited("acc1", Some(target_model)),
            "收到正配额快照后，针对该模型的配额锁定必须被立刻安全解除"
        );
        assert!(
            manager
                .rate_limit_tracker
                .is_rate_limited("acc1", Some("other-model")),
            "配额快照解除 QuotaExhausted 时，绝对不能解除 RateLimitExceeded 限制 (Ref #3840, #3845)"
        );
    }

    #[test]
    fn test_weekly_hard_quota_depleted_blocks_self_healing_until_weekly_reset() {
        let now_ts = chrono::Utc::now().timestamp();
        let mut token =
            create_test_token("weekly_depleted@test.com", Some("PRO"), 1.0, None, Some(0));
        token
            .model_quotas
            .insert("gemini-3.8-flash-high".to_string(), 0);

        // 周窗口 0% (reset 在 3 天后)，短期 5h 窗口 0% (reset 在 10 分钟前)
        let windows = vec![
            QuotaWindowStatus {
                is_weekly: true,
                remaining_fraction: Some(0.0),
                reset_time: Some(now_ts + 3 * 86400),
            },
            QuotaWindowStatus {
                is_weekly: false,
                remaining_fraction: Some(0.0),
                reset_time: Some(now_ts - 600),
            },
        ];
        token
            .model_quota_windows
            .insert("gemini".to_string(), windows);

        // P0-1 核心断言：虽然短期 5h 重置时间已过，但因周硬配额耗尽且未到重置时间，绝对不可自愈
        assert!(
            !TokenManager::is_model_quota_depleted_expired(&token, "gemini-3.8-flash-high"),
            "周配额耗尽未重置时严禁短期自愈"
        );
        assert!(
            !TokenManager::has_candidate_quota_admission(&token, "gemini-3.8-flash-high"),
            "周配额耗尽未重置时严禁准入"
        );

        // 当周重置时间也跨过（模拟时间到达 3 天后）
        let mut token_recovered = token.clone();
        token_recovered
            .model_quota_windows
            .get_mut("gemini")
            .unwrap()[0]
            .reset_time = Some(now_ts - 10);
        assert!(
            TokenManager::is_model_quota_depleted_expired(
                &token_recovered,
                "gemini-3.8-flash-high"
            ),
            "周配额与短期配额重置时间均到达后必须允许自愈"
        );
        assert!(
            TokenManager::has_candidate_quota_admission(&token_recovered, "gemini-3.8-flash-high"),
            "全重置到达后准入必须开放"
        );
    }

    #[test]
    fn test_model_family_isolation_prevents_argon_reset_contaminating_gemini() {
        let now_ts = chrono::Utc::now().timestamp();
        let mut token =
            create_test_token("argon_isolated@test.com", Some("PRO"), 1.0, None, Some(0));
        token
            .model_quotas
            .insert("gemini-3.8-flash-high".to_string(), 0);
        token
            .model_quotas
            .insert("gemini-4-argon-high".to_string(), 0);

        // Argon 族短期重置已过期，但 Gemini 族没有任何重置记录
        token
            .model_reset_times
            .insert("argon".to_string(), now_ts - 100);
        token.reset_time = Some(now_ts - 100); // 全局 fallback 时间

        // P0-2 核心断言：由于 model_reset_times 非空，查询 Gemini 模型严禁回退到全局 fallback 或被 Argon 污染
        let gemini_reset =
            TokenManager::resolve_model_reset_time(&token, "gemini-3.8-flash-high", false);
        assert_eq!(
            gemini_reset, None,
            "多模型隔离映射非空时，未索引的 Gemini 模型严禁回退全局兜底重置时间"
        );
        assert!(
            !TokenManager::is_model_quota_depleted_expired(&token, "gemini-3.8-flash-high"),
            "Gemini 模型绝对不可被 Argon 重置时间误触发自愈"
        );

        // 相比之下，查询 Argon 模型必须精准命中其专属过期时间
        let argon_reset =
            TokenManager::resolve_model_reset_time(&token, "gemini-4-argon-high", false);
        assert_eq!(argon_reset, Some(now_ts - 100));
        assert!(
            TokenManager::is_model_quota_depleted_expired(&token, "gemini-4-argon-high"),
            "Argon 模型可正常命中专属自愈"
        );
    }

    #[test]
    fn test_multi_weekly_bucket_depleted_requires_all_depleted_to_expire() {
        let now_ts = chrono::Utc::now().timestamp();
        let mut token = create_test_token("multi_weekly@test.com", Some("PRO"), 1.0, None, Some(0));
        token
            .model_quotas
            .insert("gemini-4-argon-high".to_string(), 0);

        // 场景 1：两个周桶，桶 A 剩余 85%（重置在 1 天后），桶 B 耗尽 0%（重置在 3 天后）
        // 杜绝 .find() 提前命中非耗尽桶或遗漏次级耗尽桶导致的静默泄露
        let windows = vec![
            QuotaWindowStatus {
                is_weekly: true,
                remaining_fraction: Some(0.85),
                reset_time: Some(now_ts - 100), // 虽然桶 A 重置已过，但它不是耗尽桶
            },
            QuotaWindowStatus {
                is_weekly: true,
                remaining_fraction: Some(0.0),        // 桶 B 耗尽
                reset_time: Some(now_ts + 3 * 86400), // 重置在 3 天后
            },
        ];
        token
            .model_quota_windows
            .insert("argon".to_string(), windows);

        assert!(
            !TokenManager::is_model_quota_depleted_expired(&token, "gemini-4-argon-high"),
            "次级周桶耗尽且重置时间未到时，绝对不可提前判定为过期自愈"
        );

        // 场景 2：两个周桶均耗尽，桶 A 重置已过 (1 小时前)，桶 B 重置未过 (3 天后)
        let windows_both_depleted = vec![
            QuotaWindowStatus {
                is_weekly: true,
                remaining_fraction: Some(0.0),
                reset_time: Some(now_ts - 3600),
            },
            QuotaWindowStatus {
                is_weekly: true,
                remaining_fraction: Some(0.0),
                reset_time: Some(now_ts + 3 * 86400),
            },
        ];
        token
            .model_quota_windows
            .insert("argon".to_string(), windows_both_depleted);

        assert!(
            !TokenManager::is_model_quota_depleted_expired(&token, "gemini-4-argon-high"),
            "所有耗尽桶中只要有任意一个未到重置时间，均不可恢复准入"
        );

        // 场景 3：所有耗尽周桶的重置时间均已到达
        let windows_all_expired = vec![
            QuotaWindowStatus {
                is_weekly: true,
                remaining_fraction: Some(0.0),
                reset_time: Some(now_ts - 3600),
            },
            QuotaWindowStatus {
                is_weekly: true,
                remaining_fraction: Some(0.0),
                reset_time: Some(now_ts - 10),
            },
        ];
        token
            .model_quota_windows
            .insert("argon".to_string(), windows_all_expired);

        assert!(
            TokenManager::is_model_quota_depleted_expired(&token, "gemini-4-argon-high"),
            "所有耗尽桶重置时间均到达后必须允许自愈"
        );
    }

    #[test]
    fn test_per_model_lockout_reconciliation_preserves_depleted_model_and_rate_limit() {
        let manager = TokenManager::new(std::path::PathBuf::from("/tmp/test_tokens"));
        let account_id = "test_acc_reconcile";

        // 设置三种锁：
        // 1. gemini: QuotaExhausted
        // 2. claude: QuotaExhausted
        // 3. gemini: RateLimitExceeded (429)
        manager.rate_limit_tracker.set_lockout_until_with_cap(
            account_id,
            std::time::SystemTime::now() + std::time::Duration::from_secs(3600),
            crate::proxy::rate_limit::RateLimitReason::QuotaExhausted,
            Some("gemini-2.5-pro".to_string()),
            false,
        );
        manager.rate_limit_tracker.set_lockout_until_with_cap(
            account_id,
            std::time::SystemTime::now() + std::time::Duration::from_secs(3600),
            crate::proxy::rate_limit::RateLimitReason::QuotaExhausted,
            Some("claude-sonnet-4-6".to_string()),
            false,
        );
        manager.rate_limit_tracker.set_lockout_until_with_cap(
            account_id,
            std::time::SystemTime::now() + std::time::Duration::from_secs(3600),
            crate::proxy::rate_limit::RateLimitReason::RateLimitExceeded,
            Some("gemini-2.5-flash".to_string()),
            false,
        );

        // 模拟账号刷新后：gemini 恢复正配额 (100)，claude 仍耗尽 (0)
        let mut model_quotas = std::collections::HashMap::new();
        model_quotas.insert("gemini-2.5-pro".to_string(), 100);
        model_quotas.insert("claude-sonnet-4-6".to_string(), 0);

        // 仅对 quota > 0 的模型调用 reconcile_quota_recovery
        for (model, quota) in &model_quotas {
            if *quota > 0 {
                manager
                    .rate_limit_tracker
                    .reconcile_quota_recovery(account_id, model);
            }
        }

        assert!(
            !manager
                .rate_limit_tracker
                .is_rate_limited(account_id, Some("gemini-2.5-pro")),
            "正配额模型必须精准解除 QuotaExhausted 锁定"
        );
        assert!(
            manager
                .rate_limit_tracker
                .is_rate_limited(account_id, Some("claude-sonnet-4-6")),
            "零配额模型绝对不可因其他模型的正配额被误解禁"
        );
        assert!(
            manager
                .rate_limit_tracker
                .is_rate_limited(account_id, Some("gemini-2.5-flash")),
            "RateLimitExceeded (429) 限流绝对不可被配额解禁逻辑冲掉"
        );
    }

    #[test]
    fn test_camel_case_quota_groups_support() {
        let manager = TokenManager::new(std::path::PathBuf::from("/tmp/test_tokens"));
        let future_time = chrono::Utc::now() + chrono::Duration::days(5);
        let future_rfc3339 = future_time.to_rfc3339();

        let account_val = serde_json::json!({
            "quota": {
                "last_updated": 1700000000,
                "quotaGroups": [
                    {
                        "displayName": "Gemini",
                        "buckets": [
                            {
                                "bucketId": "gemini_weekly",
                                "window": "7d",
                                "remainingFraction": 0.5,
                                "resetTime": future_rfc3339
                            }
                        ]
                    }
                ]
            }
        });

        let extracted_ts = manager.extract_weekly_reset_time(&account_val);
        assert_eq!(
            extracted_ts,
            Some(future_time.timestamp()),
            "camelCase quotaGroups 与 resetTime 必须能正确解析出周重置时间戳"
        );
    }

    #[test]
    fn test_reconcile_recovered_quota_locks_clears_account_level_and_preserves_zero_model() {
        let manager = TokenManager::new(std::path::PathBuf::from("/tmp/test_tokens"));
        let account_id = "acc_recovered_helper";
        let target_time = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        use crate::proxy::rate_limit::RateLimitReason;

        // 账号级“全模型耗尽”锁
        manager.rate_limit_tracker.set_lockout_until_with_cap(
            account_id,
            target_time,
            RateLimitReason::QuotaExhausted,
            None,
            false,
        );
        // 模型级配额锁：gemini 已恢复，claude 仍为 0
        manager.rate_limit_tracker.set_lockout_until_with_cap(
            account_id,
            target_time,
            RateLimitReason::QuotaExhausted,
            Some("gemini".to_string()),
            false,
        );
        manager.rate_limit_tracker.set_lockout_until_with_cap(
            account_id,
            target_time,
            RateLimitReason::QuotaExhausted,
            Some("claude".to_string()),
            false,
        );
        // 独立 429 退避，必须保留
        manager.rate_limit_tracker.set_lockout_until_with_cap(
            account_id,
            target_time,
            RateLimitReason::RateLimitExceeded,
            Some("gemini-2.5-flash".to_string()),
            false,
        );

        let mut token = create_test_token("recovered@test.com", Some("PRO"), 1.0, None, Some(100));
        token.account_id = account_id.to_string();
        token.model_quotas.insert("gemini".to_string(), 100);
        token.model_quotas.insert("claude".to_string(), 0);

        manager.reconcile_recovered_quota_locks(account_id, &token);

        assert!(
            !manager
                .rate_limit_tracker
                .is_rate_limited(account_id, Some("gemini")),
            "正配额模型必须精准解除 QuotaExhausted"
        );
        assert!(
            !manager.rate_limit_tracker.is_rate_limited(account_id, None),
            "任一模型恢复后，账号级全模型耗尽锁必须解除"
        );
        assert!(
            manager
                .rate_limit_tracker
                .is_rate_limited(account_id, Some("claude")),
            "仍为 0 配额的模型不可被其他模型恢复连带解禁"
        );
        assert!(
            manager
                .rate_limit_tracker
                .is_rate_limited(account_id, Some("gemini-2.5-flash")),
            "RateLimitExceeded (429) 退避必须保留"
        );
    }

    #[test]
    fn test_resolve_model_reset_time_requires_both_model_maps_empty_for_global_fallback() {
        let now_ts = chrono::Utc::now().timestamp();
        let mut token = create_test_token("fallback@test.com", Some("PRO"), 1.0, None, Some(0));
        token.reset_time = Some(now_ts - 100);
        token.weekly_reset_time = Some(now_ts - 100);

        // 仅 short map 非空：weekly 查询也不得回退全局
        token
            .model_reset_times
            .insert("argon".to_string(), now_ts - 100);
        assert_eq!(
            TokenManager::resolve_model_reset_time(&token, "gemini-3.8-flash-high", false),
            None
        );
        assert_eq!(
            TokenManager::resolve_model_reset_time(&token, "gemini-3.8-flash-high", true),
            None
        );

        // 仅 weekly map 非空：short 查询也不得回退全局
        token.model_reset_times.clear();
        token
            .model_weekly_reset_times
            .insert("argon".to_string(), now_ts - 100);
        assert_eq!(
            TokenManager::resolve_model_reset_time(&token, "gemini-3.8-flash-high", false),
            None
        );
        assert_eq!(
            TokenManager::resolve_model_reset_time(&token, "gemini-3.8-flash-high", true),
            None
        );

        // 两个 map 都为空：才允许全局回退
        token.model_weekly_reset_times.clear();
        assert_eq!(
            TokenManager::resolve_model_reset_time(&token, "gemini-3.8-flash-high", false),
            Some(now_ts - 100)
        );
        assert_eq!(
            TokenManager::resolve_model_reset_time(&token, "gemini-3.8-flash-high", true),
            Some(now_ts - 100)
        );
    }

    #[test]
    fn test_quota_bucket_inputs_json_and_typed_are_equivalent() {
        let json_groups = serde_json::json!([
            {
                "displayName": "Gemini Models",
                "buckets": [
                    {
                        "bucketId": "gemini_weekly",
                        "window": "7d",
                        "remainingFraction": 0.0,
                        "resetTime": "2026-11-10T00:00:00Z"
                    },
                    {
                        "bucketId": "gemini_5h",
                        "window": "5h",
                        "remainingFraction": 0.75,
                        "resetTime": "2026-11-07T08:00:00Z"
                    }
                ]
            }
        ]);
        let json_inputs =
            TokenManager::quota_bucket_inputs_from_json(json_groups.as_array().unwrap());

        let typed_groups: Vec<crate::models::quota::QuotaGroup> =
            serde_json::from_value(serde_json::json!([
                {
                    "display_name": "Gemini Models",
                    "buckets": [
                        {
                            "bucket_id": "gemini_weekly",
                            "window": "7d",
                            "remaining_fraction": 0.0,
                            "reset_time": "2026-11-10T00:00:00Z"
                        },
                        {
                            "bucket_id": "gemini_5h",
                            "window": "5h",
                            "remaining_fraction": 0.75,
                            "reset_time": "2026-11-07T08:00:00Z"
                        }
                    ]
                }
            ]))
            .unwrap();
        let typed_inputs = TokenManager::quota_bucket_inputs_from_typed(&typed_groups);

        assert_eq!(json_inputs, typed_inputs);
    }

    #[test]
    fn test_apply_quota_bucket_inputs_builds_consistent_view_and_min_reset() {
        let mut windows = HashMap::new();
        let mut short = HashMap::new();
        let mut weekly = HashMap::new();
        let inputs = vec![
            QuotaBucketInput {
                family: "gemini".to_string(),
                is_weekly: true,
                remaining_fraction: Some(0.0),
                reset_at: Some(300),
            },
            QuotaBucketInput {
                family: "gemini".to_string(),
                is_weekly: true,
                remaining_fraction: Some(0.0),
                reset_at: Some(200),
            },
            QuotaBucketInput {
                family: "gemini".to_string(),
                is_weekly: false,
                remaining_fraction: Some(0.5),
                reset_at: Some(100),
            },
            QuotaBucketInput {
                family: "argon".to_string(),
                is_weekly: true,
                remaining_fraction: Some(0.8),
                reset_at: Some(400),
            },
        ];

        TokenManager::apply_quota_bucket_inputs(&mut windows, &mut short, &mut weekly, inputs);

        assert_eq!(weekly.get("gemini"), Some(&200));
        assert_eq!(short.get("gemini"), Some(&100));
        assert_eq!(weekly.get("argon"), Some(&400));
        assert_eq!(weekly.get("gemini-4-argon"), Some(&400));
        assert_eq!(windows.get("gemini").unwrap().len(), 3);
        assert_eq!(windows.get("argon").unwrap().len(), 1);

        let mut token = create_test_token("snapshot@test.com", Some("PRO"), 1.0, None, Some(0));
        token.model_quota_windows = windows;
        token.model_reset_times = short;
        token.model_weekly_reset_times = weekly;
        token.assert_quota_state_consistent();

        let snapshot = token.quota_snapshot();
        assert_eq!(snapshot.buckets.len(), 4);
        assert_eq!(snapshot.family_buckets("gemini").count(), 3);
    }

    #[test]
    fn test_readonly_quota_snapshot_falls_back_to_reset_maps() {
        let mut token = create_test_token("legacy@test.com", Some("PRO"), 1.0, None, Some(0));
        token.model_reset_times.insert("gemini".to_string(), 100);
        token
            .model_weekly_reset_times
            .insert("gemini".to_string(), 200);

        let snapshot = token.quota_snapshot();
        assert_eq!(snapshot.family_buckets("gemini").count(), 2);
        assert!(snapshot
            .family_buckets("gemini")
            .any(|b| !b.is_weekly && b.reset_at == Some(100)));
        assert!(snapshot
            .family_buckets("gemini")
            .any(|b| b.is_weekly && b.reset_at == Some(200)));
        token.assert_quota_state_consistent();
    }

    #[tokio::test]
    async fn account_model_priorities_route_claude_and_gemini_independently() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = TokenManager::new(temp_dir.path().to_path_buf());

        let acc_personal_file = temp_dir.path().join("acc_personal.json");
        let acc_pool_file = temp_dir.path().join("acc_pool.json");
        std::fs::write(&acc_personal_file, r#"{"disabled": false}"#).unwrap();
        std::fs::write(&acc_pool_file, r#"{"disabled": false}"#).unwrap();

        let claude_model = "claude-sonnet-4-6";
        let gemini_model = "gemini-3.1-pro-high";

        // Personal account: default priority 50, Claude priority 1 (use first), Gemini priority 80 (use after other Pro accounts)
        let mut personal =
            create_test_token("personal@example.com", Some("PRO"), 1.0, None, Some(80));
        personal.account_id = "acc_personal".to_string();
        personal.access_token = "tok_personal".to_string();
        personal.account_path = acc_personal_file;
        personal.priority = 50;
        personal.model_priorities.insert("claude".to_string(), 1);
        personal.model_priorities.insert("gemini".to_string(), 80);
        for m in [claude_model, gemini_model, "claude", "gemini"] {
            personal.available_models.insert(m.to_string());
            personal.model_quotas.insert(m.to_string(), 80);
        }

        // Pool Pro account: default priority 50, no per-model overrides
        let mut pool_pro = create_test_token("pool@example.com", Some("PRO"), 1.0, None, Some(90));
        pool_pro.account_id = "acc_pool".to_string();
        pool_pro.access_token = "tok_pool".to_string();
        pool_pro.account_path = acc_pool_file;
        pool_pro.priority = 50;
        for m in [claude_model, gemini_model, "claude", "gemini"] {
            pool_pro.available_models.insert(m.to_string());
            pool_pro.model_quotas.insert(m.to_string(), 90);
        }

        assert_eq!(personal.effective_priority(claude_model, claude_model), 1);
        assert_eq!(personal.effective_priority(gemini_model, gemini_model), 80);
        assert_eq!(pool_pro.effective_priority(claude_model, claude_model), 50);
        assert_eq!(pool_pro.effective_priority(gemini_model, gemini_model), 50);

        manager.tokens.insert("acc_personal".to_string(), personal);
        manager.tokens.insert("acc_pool".to_string(), pool_pro);

        // 1. Requesting Claude selects `acc_personal` (effective priority 1 < 50)
        let (tok_claude, _, _, id_claude, _) = manager
            .get_token("claude", false, None, claude_model)
            .await
            .unwrap();
        assert_eq!(id_claude, "acc_personal");
        assert_eq!(tok_claude, "tok_personal");

        // 2. Immediately requesting Gemini (even within the 60s stateless window) yields to `acc_pool`
        // because `acc_personal` has effective Gemini priority 80 > `acc_pool` priority 50.
        let (tok_gemini, _, _, id_gemini, _) = manager
            .get_token("gemini", false, None, gemini_model)
            .await
            .unwrap();
        assert_eq!(id_gemini, "acc_pool");
        assert_eq!(tok_gemini, "tok_pool");

        // 3. Hot-updating model priorities via TokenManager flips Gemini priority to 1 on `acc_personal`
        let mut updated_map = HashMap::new();
        updated_map.insert("claude".to_string(), 1);
        updated_map.insert("gemini".to_string(), 1);
        manager.update_account_model_priorities("acc_personal", 50, Some(updated_map));

        let (_, _, _, id_gemini_after, _) = manager
            .get_token("gemini", false, None, gemini_model)
            .await
            .unwrap();
        assert_eq!(id_gemini_after, "acc_personal");
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "short reset map drifted")]
    fn test_quota_state_consistency_assertion_detects_drift() {
        let mut token = create_test_token("drift@test.com", Some("PRO"), 1.0, None, Some(0));
        token.model_quota_windows.insert(
            "gemini".to_string(),
            vec![QuotaWindowStatus {
                is_weekly: false,
                remaining_fraction: Some(0.0),
                reset_time: Some(100),
            }],
        );
        token.model_reset_times.insert("gemini".to_string(), 999);
        token.assert_quota_state_consistent();
    }

    #[tokio::test]
    async fn test_load_accounts_preserves_hydrogen_and_dynamic_limits_for_non_ultra() {
        let tmp_root = std::env::temp_dir().join(format!(
            "antigravity-token-manager-test-hydrogen-pro-{}",
            uuid::Uuid::new_v4()
        ));
        let accounts_dir = tmp_root.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();
        let now = chrono::Utc::now().timestamp();
        let future_reset = chrono::DateTime::from_timestamp(now + 3600, 0)
            .unwrap()
            .to_rfc3339();

        let pro_json = serde_json::json!({
            "id": "acc_pro_hydrogen",
            "email": "pro_hydrogen@test.com",
            "token": {
                "access_token": "atk_h",
                "refresh_token": "rtk_h",
                "expires_in": 3600,
                "expiry_timestamp": now + 3600
            },
            "quota": {
                "subscription_tier": "PRO",
                "models": [
                    {
                        "name": "gemini-4-hydrogen-high",
                        "percentage": 85,
                        "reset_time": future_reset,
                        "max_output_tokens": 262144
                    },
                    {
                        "name": "gemini-4-argon-high",
                        "percentage": 85,
                        "reset_time": future_reset,
                        "max_output_tokens": 65536
                    }
                ],
                "quota_groups": [
                    {
                        "display_name": "Gemini",
                        "buckets": [
                            {
                                "bucket_id": "gemini-5h",
                                "window": "5h",
                                "remaining_fraction": 0.85,
                                "reset_time": future_reset
                            }
                        ]
                    }
                ]
            },
            "created_at": now,
            "last_used": now
        });

        std::fs::write(
            accounts_dir.join("acc_pro_hydrogen.json"),
            serde_json::to_string_pretty(&pro_json).unwrap(),
        )
        .unwrap();

        let manager = TokenManager::new(tmp_root.clone());
        manager.load_accounts().await.unwrap();

        let token = manager
            .tokens
            .get("acc_pro_hydrogen")
            .expect("pro account should load");
        assert!(
            token.available_models.contains("gemini-4-hydrogen-high"),
            "hydrogen-high returned by upstream must not be wiped for non-Ultra accounts"
        );
        assert!(
            token.available_models.contains("gemini-4-hydrogen"),
            "derived bare gemini-4-hydrogen must be preserved"
        );
        assert_eq!(
            token.model_quotas.get("gemini-4-hydrogen-high").copied(),
            Some(85)
        );
        assert_eq!(
            token.model_quotas.get("gemini-4-hydrogen").copied(),
            Some(85)
        );
        assert_eq!(
            token.model_limits.get("gemini-4-hydrogen-high").copied(),
            Some(262144)
        );
        assert_eq!(
            token.model_limits.get("gemini-4-hydrogen").copied(),
            Some(262144)
        );

        // Legacy Argon still cleans up on non-Ultra accounts without an Argon bucket (#3745).
        assert!(
            !token.available_models.contains("gemini-4-argon-high"),
            "argon without argon bucket must still be excluded on non-Ultra accounts"
        );
        assert!(!token.available_models.contains("gemini-4-argon"));

        let _ = std::fs::remove_dir_all(&tmp_root);
    }

    #[tokio::test]
    async fn test_load_accounts_ultra_does_not_fabricate_ungranted_hydrogen_or_carbon() {
        let tmp_root = std::env::temp_dir().join(format!(
            "antigravity-token-manager-test-ultra-no-fab-{}",
            uuid::Uuid::new_v4()
        ));
        let accounts_dir = tmp_root.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();
        let now = chrono::Utc::now().timestamp();
        let future_reset = chrono::DateTime::from_timestamp(now + 3600, 0)
            .unwrap()
            .to_rfc3339();

        let ultra_json = serde_json::json!({
            "id": "acc_ultra_only_3x",
            "email": "ultra@test.com",
            "token": {
                "access_token": "atk_u",
                "refresh_token": "rtk_u",
                "expires_in": 3600,
                "expiry_timestamp": now + 3600
            },
            "quota": {
                "subscription_tier": "ULTRA",
                "models": [
                    {
                        "name": "gemini-3.8-flash-high",
                        "percentage": 90,
                        "reset_time": future_reset,
                        "max_output_tokens": 65536
                    }
                ],
                "quota_groups": [
                    {
                        "display_name": "Gemini",
                        "buckets": [
                            {
                                "bucket_id": "gemini-5h",
                                "window": "5h",
                                "remaining_fraction": 0.90,
                                "reset_time": future_reset
                            }
                        ]
                    }
                ]
            },
            "created_at": now,
            "last_used": now
        });

        std::fs::write(
            accounts_dir.join("acc_ultra_only_3x.json"),
            serde_json::to_string_pretty(&ultra_json).unwrap(),
        )
        .unwrap();

        let manager = TokenManager::new(tmp_root.clone());
        manager.load_accounts().await.unwrap();

        let token = manager
            .tokens
            .get("acc_ultra_only_3x")
            .expect("ultra account should load");
        // Ultra keeps Argon fallback compatibility...
        assert!(token.available_models.contains("gemini-4-argon"));
        // ...without fabricating ungranted hydrogen/carbon models not in quota.models.
        assert!(!token.available_models.contains("gemini-4-hydrogen"));
        assert!(!token.available_models.contains("gemini-4-carbon"));

        let _ = std::fs::remove_dir_all(&tmp_root);
    }

    #[test]
    fn test_dynamic_g4_codename_quota_window_isolation_and_shared_fallback() {
        let now_ts = chrono::Utc::now().timestamp();
        let mut token = create_test_token("g4_isolation@test.com", Some("PRO"), 1.0, None, Some(0));

        // Shared Gemini pool is healthy/expired, while dedicated Helium bucket is depleted and unreset.
        token.model_quota_windows.insert(
            "gemini".to_string(),
            vec![QuotaWindowStatus {
                is_weekly: true,
                remaining_fraction: Some(0.0),
                reset_time: Some(now_ts - 60),
            }],
        );
        token.model_quota_windows.insert(
            "helium".to_string(),
            vec![QuotaWindowStatus {
                is_weekly: true,
                remaining_fraction: Some(0.0),
                reset_time: Some(now_ts + 86400),
            }],
        );
        token
            .model_weekly_reset_times
            .insert("gemini".to_string(), now_ts - 60);
        token
            .model_weekly_reset_times
            .insert("helium".to_string(), now_ts + 86400);
        token
            .model_quotas
            .insert("gemini-4-helium-xhigh".to_string(), 0);
        token
            .model_quotas
            .insert("gemini-4-hydrogen-xhigh".to_string(), 0);

        // Dedicated Helium must strictly consult its own bucket and block expiry self-heal.
        assert!(
            !TokenManager::is_model_quota_depleted_expired(&token, "gemini-4-helium-xhigh"),
            "dedicated codename bucket depletion must stay isolated from expired Gemini pool"
        );
        assert_eq!(
            TokenManager::resolve_model_reset_time(&token, "gemini-4-helium-xhigh", true),
            Some(now_ts + 86400)
        );

        // Hydrogen without a dedicated bucket shares the general Gemini pool.
        assert!(
            TokenManager::is_model_quota_depleted_expired(&token, "gemini-4-hydrogen-xhigh"),
            "non-dedicated G4 codename without a dedicated bucket should fall back to Gemini windows"
        );
        assert_eq!(
            TokenManager::resolve_model_reset_time(&token, "gemini-4-hydrogen-xhigh", true),
            Some(now_ts - 60)
        );
    }

    #[test]
    fn test_accounts_with_positive_quota_filters_canary_g4_and_isolates_dedicated_groups() {
        let now_ts = chrono::Utc::now().timestamp();
        let manager = TokenManager::new(std::path::PathBuf::from("/tmp/test_g4_positive"));

        // Account A: Pro without Hydrogen capability, healthy Gemini pool
        let mut acc_no_cap = create_test_token("nocap@test.com", Some("PRO"), 1.0, None, Some(100));
        acc_no_cap
            .available_models
            .insert("gemini-3.8-flash-high".to_string());
        acc_no_cap
            .model_quotas
            .insert("gemini-3.8-flash-high".to_string(), 90);

        // Account B: Pro with Hydrogen sharing Gemini pool (no dedicated hydrogen bucket)
        let mut acc_shared =
            create_test_token("shared@test.com", Some("PRO"), 1.0, None, Some(100));
        acc_shared
            .available_models
            .insert("gemini-4-hydrogen-xhigh".to_string());
        acc_shared
            .model_quotas
            .insert("gemini-4-hydrogen-xhigh".to_string(), 80);

        // Account C: Dedicated Helium bucket depleted (0%), Gemini pool healthy (90%)
        let mut acc_dedicated_zero =
            create_test_token("ded_zero@test.com", Some("PRO"), 1.0, None, Some(100));
        acc_dedicated_zero
            .available_models
            .insert("gemini-4-helium-xhigh".to_string());
        acc_dedicated_zero
            .available_models
            .insert("gemini-3.8-flash-high".to_string());
        acc_dedicated_zero
            .model_quotas
            .insert("gemini-4-helium-xhigh".to_string(), 0);
        acc_dedicated_zero
            .model_quotas
            .insert("gemini-3.8-flash-high".to_string(), 90);
        acc_dedicated_zero.model_quota_windows.insert(
            "helium".to_string(),
            vec![QuotaWindowStatus {
                is_weekly: true,
                remaining_fraction: Some(0.0),
                reset_time: Some(now_ts + 3600),
            }],
        );

        manager.tokens.insert("nocap".to_string(), acc_no_cap);
        manager.tokens.insert("shared".to_string(), acc_shared);
        manager
            .tokens
            .insert("ded_zero".to_string(), acc_dedicated_zero);

        // Only acc_shared holds Hydrogen capability.
        assert_eq!(
            manager.accounts_with_positive_quota("gemini-4-hydrogen-xhigh"),
            1
        );
        // Dedicated Helium is 0% despite positive Gemini 3.8 pool.
        assert_eq!(
            manager.accounts_with_positive_quota("gemini-4-helium-xhigh"),
            0
        );
        // Standard Gemini sees acc_no_cap + acc_dedicated_zero (+ acc_shared non-3p non-dedicated key).
        assert_eq!(
            manager.accounts_with_positive_quota("gemini-3.8-flash-high"),
            3
        );
    }

    #[test]
    fn test_sync_zero_quota_circuit_breaker_isolates_dynamic_g4_groups() {
        let now_ts = chrono::Utc::now().timestamp();
        let future_reset = chrono::DateTime::from_timestamp(now_ts + 7200, 0)
            .unwrap()
            .to_rfc3339();
        let manager = TokenManager::new(std::path::PathBuf::from("/tmp/test_g4_cb"));

        // Scenario 1: Helium has its own depleted group while Gemini is healthy;
        // Hydrogen is in models with NO dedicated Hydrogen group (shares Gemini).
        let account_json = serde_json::json!({
            "quota": {
                "last_updated": now_ts,
                "models": [
                    { "name": "gemini-4-helium-xhigh", "percentage": 0 },
                    { "name": "gemini-4-hydrogen-xhigh", "percentage": 90 },
                    { "name": "gemini-3.8-flash-high", "percentage": 90 }
                ],
                "quota_groups": [
                    {
                        "display_name": "Gemini Models",
                        "buckets": [
                            {
                                "bucket_id": "gemini-weekly",
                                "window": "weekly",
                                "remaining_fraction": 0.90,
                                "reset_time": future_reset
                            }
                        ]
                    },
                    {
                        "display_name": "Helium",
                        "buckets": [
                            {
                                "bucket_id": "helium-weekly",
                                "window": "weekly",
                                "remaining_fraction": 0.0,
                                "reset_time": future_reset
                            }
                        ]
                    }
                ]
            }
        });

        manager.sync_zero_quota_circuit_breaker("acc1", &account_json);

        assert!(
            manager
                .rate_limit_tracker
                .is_rate_limited("acc1", Some("gemini-4-helium-xhigh")),
            "dedicated exhausted G4 group must lock its codename models"
        );
        assert!(
            !manager
                .rate_limit_tracker
                .is_rate_limited("acc1", Some("gemini-4-hydrogen-xhigh")),
            "shared non-dedicated G4 model must stay unlocked when Gemini is positive"
        );
        assert!(
            !manager
                .rate_limit_tracker
                .is_rate_limited("acc1", Some("gemini-3.8-flash-high")),
            "dedicated G4 exhaustion must not lock general Gemini models"
        );

        // Scenario 2: When general Gemini weekly bucket hits 0%, shared Hydrogen locks with Gemini.
        let gemini_exhausted_json = serde_json::json!({
            "quota": {
                "last_updated": now_ts + 10,
                "models": [
                    { "name": "gemini-4-hydrogen-xhigh", "percentage": 0 },
                    { "name": "gemini-3.8-flash-high", "percentage": 0 }
                ],
                "quota_groups": [
                    {
                        "display_name": "Gemini Models",
                        "buckets": [
                            {
                                "bucket_id": "gemini-weekly",
                                "window": "weekly",
                                "remaining_fraction": 0.0,
                                "reset_time": future_reset
                            }
                        ]
                    }
                ]
            }
        });
        manager.sync_zero_quota_circuit_breaker("acc2", &gemini_exhausted_json);
        assert!(manager
            .rate_limit_tracker
            .is_rate_limited("acc2", Some("gemini-4-hydrogen-xhigh")));
        assert!(manager
            .rate_limit_tracker
            .is_rate_limited("acc2", Some("gemini-3.8-flash-high")));
    }

    #[tokio::test]
    async fn test_capable_account_vs_positive_quota_and_disk_discovery() {
        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().to_path_buf();
        let accounts_dir = data_dir.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();

        // Disk account 1: proxy_disabled = true, carries gemini-4-hydrogen-high
        let acc_disabled = serde_json::json!({
            "id": "acc-disabled-h2",
            "email": "h2-disabled@example.com",
            "proxy_disabled": true,
            "token": {
                "access_token": "ya29.test",
                "refresh_token": "1//test",
                "expires_in": 3600,
                "expiry_timestamp": chrono::Utc::now().timestamp() + 3600
            },
            "quota": {
                "models": [
                    { "name": "gemini-4-hydrogen-high", "percentage": 100 }
                ]
            }
        });
        std::fs::write(
            accounts_dir.join("acc-disabled-h2.json"),
            acc_disabled.to_string(),
        )
        .unwrap();

        // Disk account 2: orphan (not in accounts.json), carries fake-orphan-model
        let acc_orphan = serde_json::json!({
            "id": "acc-orphan",
            "email": "orphan@example.com",
            "quota": {
                "models": [
                    { "name": "fake-orphan-model", "percentage": 100 }
                ]
            }
        });
        std::fs::write(accounts_dir.join("acc-orphan.json"), acc_orphan.to_string()).unwrap();

        std::fs::write(
            data_dir.join("accounts.json"),
            serde_json::json!({
                "version": "2.0",
                "accounts": [
                    {
                        "id": "acc-disabled-h2",
                        "email": "h2-disabled@example.com",
                        "disabled": false,
                        "proxy_disabled": true
                    }
                ]
            })
            .to_string(),
        )
        .unwrap();

        let manager = TokenManager::new(data_dir.clone());
        let disk_models = manager.get_all_disk_discovered_models();
        assert!(
            disk_models.contains("gemini-4-hydrogen-high"),
            "proxy_disabled valid account models must remain discoverable on disk"
        );
        assert!(
            !disk_models.contains("fake-orphan-model"),
            "orphan files outside accounts.json must not leak into disk discovery"
        );

        // Pool token without hydrogen capability -> neither capable nor positive for hydrogen,
        // while capable for its own 0%-quota gemini-3.8-flash-high.
        let mut quotas = HashMap::new();
        quotas.insert("gemini-3.8-flash-high".to_string(), 0);
        let mut available = HashSet::new();
        available.insert("gemini-3.8-flash-high".to_string());
        let tok = ProxyToken {
            account_id: "acc-active".to_string(),
            priority: 0,
            model_priorities: HashMap::new(),
            access_token: "ya29.active".to_string(),
            refresh_token: "1//active".to_string(),
            expires_in: 3600,
            timestamp: chrono::Utc::now().timestamp() + 3600,
            email: "active@example.com".to_string(),
            account_path: accounts_dir.join("acc-active.json"),
            project_id: None,
            subscription_tier: None,
            remaining_quota: Some(0),
            protected_models: HashSet::new(),
            forbidden_models: HashSet::new(),
            health_score: 1.0,
            reset_time: None,
            weekly_reset_time: None,
            validation_blocked: false,
            validation_blocked_until: 0,
            validation_url: None,
            model_quotas: quotas,
            model_limits: HashMap::new(),
            available_models: available,
            model_metadata: HashMap::new(),
            model_reset_times: HashMap::new(),
            model_weekly_reset_times: HashMap::new(),
            model_quota_windows: HashMap::new(),
        };
        manager.tokens.insert("acc-active".to_string(), tok);
        let future_reset =
            chrono::DateTime::from_timestamp(chrono::Utc::now().timestamp() + 3600, 0)
                .unwrap()
                .to_rfc3339();
        let active_quota_json = serde_json::json!({
            "quota": {
                "last_updated": chrono::Utc::now().timestamp(),
                "models": [
                    { "name": "gemini-3.8-flash-high", "percentage": 0, "reset_time": future_reset }
                ],
                "quota_groups": [
                    {
                        "display_name": "Gemini Models",
                        "buckets": [
                            {
                                "bucket_id": "gemini-weekly",
                                "window": "weekly",
                                "remaining_fraction": 0.0,
                                "reset_time": future_reset
                            }
                        ]
                    }
                ]
            }
        });
        manager.sync_zero_quota_circuit_breaker("acc-active", &active_quota_json);

        assert!(
            !manager.has_any_capable_account_in_pool("gemini-4-hydrogen-high"),
            "pool lacking hydrogen authorization must report no capable account (503)"
        );
        assert!(
            manager.has_any_capable_account_in_pool("gemini-3.8-flash-high"),
            "pool with 0%-quota authorized model still has capable account (502)"
        );
        assert_eq!(
            manager.accounts_with_positive_quota("gemini-3.8-flash-high"),
            0
        );
    }

    #[tokio::test]
    async fn test_disable_and_manual_enable_clear_locks_and_jail() {
        let data_dir =
            std::env::temp_dir().join(format!("abv_tm_toggle_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&data_dir).unwrap();
        let manager = TokenManager::new(data_dir.clone());

        let mut available = HashSet::new();
        available.insert("gemini-4-hydrogen-high".to_string());
        let tok = ProxyToken {
            account_id: "acc-toggle".to_string(),
            priority: 0,
            model_priorities: HashMap::new(),
            access_token: "ya29.toggle".to_string(),
            refresh_token: "1//toggle".to_string(),
            expires_in: 3600,
            timestamp: chrono::Utc::now().timestamp() + 3600,
            email: "toggle@example.com".to_string(),
            account_path: data_dir.join("acc-toggle.json"),
            project_id: Some("proj-toggle".to_string()),
            subscription_tier: None,
            remaining_quota: Some(100),
            protected_models: HashSet::new(),
            forbidden_models: HashSet::new(),
            health_score: 1.0,
            reset_time: None,
            weekly_reset_time: None,
            validation_blocked: false,
            validation_blocked_until: 0,
            validation_url: None,
            model_quotas: HashMap::new(),
            model_limits: HashMap::new(),
            available_models: available,
            model_metadata: HashMap::new(),
            model_reset_times: HashMap::new(),
            model_weekly_reset_times: HashMap::new(),
            model_quota_windows: HashMap::new(),
        };
        manager.tokens.insert("acc-toggle".to_string(), tok);
        manager
            .set_preferred_account(Some("acc-toggle".to_string()))
            .await;
        {
            let mut last_used = manager.last_used_account.lock().await;
            *last_used = Some(("acc-toggle".to_string(), std::time::Instant::now()));
        }
        manager
            .session_accounts
            .insert("sess-1".to_string(), "acc-toggle".to_string());
        manager.mark_model_unsupported("acc-toggle", "gemini-4-hydrogen-high", Some(900));
        manager
            .invalid_grant_failures
            .insert("acc-toggle".to_string(), 1);
        manager
            .project_id_failures
            .insert("acc-toggle".to_string(), std::time::Instant::now());

        // Async removal must clear tokens, last_used_account, preferred_account_id,
        // session_accounts, and unsupported breakers cleanly.
        manager.remove_account_and_clear_locks("acc-toggle").await;
        assert!(!manager.tokens.contains_key("acc-toggle"));
        assert!(manager.get_preferred_account().await.is_none());
        assert!(manager.last_used_account.lock().await.is_none());
        assert!(!manager.session_accounts.contains_key("sess-1"));
        assert!(!manager.is_model_unsupported("acc-toggle", "gemini-4-hydrogen-high"));
        assert!(!manager.invalid_grant_failures.contains_key("acc-toggle"));
        assert!(!manager.project_id_failures.contains_key("acc-toggle"));

        // Manual re-enable must immediately release QoS jail, transient rate limits, failure counters, and model breakers.
        manager.qos_guardian.set_auto_jail_enabled(true);
        manager.qos_guardian.states.insert(
            "acc-toggle".to_string(),
            crate::proxy::qos_guardian::AccountQoSState::Jailed {
                level: 1,
                jailed_at: std::time::Instant::now(),
                current_wait: std::time::Duration::from_secs(600),
            },
        );
        manager.mark_model_unsupported("acc-toggle", "gemini-4-hydrogen-high", Some(900));
        manager.rate_limit_tracker.set_lockout_until(
            "acc-toggle",
            std::time::SystemTime::now() + std::time::Duration::from_secs(120),
            crate::proxy::rate_limit::RateLimitReason::QuotaExhausted,
            Some("gemini-4-hydrogen-high".to_string()),
        );
        manager
            .invalid_grant_failures
            .insert("acc-toggle".to_string(), 2);
        manager
            .project_id_failures
            .insert("acc-toggle".to_string(), std::time::Instant::now());

        assert!(manager.qos_guardian.is_jailed_or_probing("acc-toggle"));
        manager.prepare_manual_enable("acc-toggle");
        assert!(!manager.qos_guardian.is_jailed_or_probing("acc-toggle"));
        assert!(!manager.is_model_unsupported("acc-toggle", "gemini-4-hydrogen-high"));
        assert!(!manager
            .rate_limit_tracker
            .is_rate_limited("acc-toggle", Some("gemini-4-hydrogen-high")));
        assert!(!manager.invalid_grant_failures.contains_key("acc-toggle"));
        assert!(!manager.project_id_failures.contains_key("acc-toggle"));

        let _ = std::fs::remove_dir_all(&data_dir);
    }
}
