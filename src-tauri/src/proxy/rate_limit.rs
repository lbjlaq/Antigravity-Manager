use dashmap::DashMap;
use regex::Regex;
use std::time::{Duration, SystemTime};

const MAX_LOCKOUT_SECONDS: u64 = 300;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RetryParserMode {
    Current,
    Baseline,
}

/// 限流原因类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RateLimitReason {
    /// 配额耗尽 (QUOTA_EXHAUSTED)
    QuotaExhausted,
    /// 速率限制 (RATE_LIMIT_EXCEEDED)
    RateLimitExceeded,
    /// 模型容量耗尽 (MODEL_CAPACITY_EXHAUSTED)
    ModelCapacityExhausted,
    /// 服务器错误 (5xx)
    ServerError,
    /// 未知原因
    Unknown,
}

pub(crate) fn normalize_image_model_id(model: &str) -> Option<String> {
    let normalized = crate::proxy::common::model_mapping::normalize_to_standard_id(model)?;
    matches!(
        normalized.as_str(),
        "gemini-3.1-flash-image" | "gemini-3-pro-image"
    )
    .then_some(normalized)
}

pub(crate) fn has_explicit_quota_exhausted(body: &str) -> bool {
    body.to_ascii_uppercase().contains("QUOTA_EXHAUSTED")
}

pub(crate) fn is_active_persisted_long_limit(
    _model_key: &str,
    status: &crate::models::account::LiveLimitStatus,
    now: i64,
) -> bool {
    status.status == 429
        && status.reason == "QuotaExhausted"
        && status.until > now
        && status.until.saturating_sub(status.detected_at) > MAX_LOCKOUT_SECONDS as i64
        && status.message.as_deref().is_some_and(|message| {
            has_explicit_quota_exhausted(message)
                && crate::proxy::upstream::retry::parse_retry_delay(message, None).is_some()
        })
}

pub(crate) fn is_active_persisted_long_image_limit(
    model_key: &str,
    status: &crate::models::account::LiveLimitStatus,
    now: i64,
) -> bool {
    normalize_image_model_id(model_key).is_some()
        && is_active_persisted_long_limit(model_key, status, now)
}

/// 限流信息
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RateLimitInfo {
    /// 限流重置时间
    pub reset_time: SystemTime,
    /// 重试间隔(秒)
    #[allow(dead_code)]
    pub retry_after_sec: u64,
    /// 检测时间
    #[allow(dead_code)]
    pub detected_at: SystemTime,
    /// 限流原因
    #[allow(dead_code)] // Used for logging and diagnostics
    pub reason: RateLimitReason,
    /// 关联的模型 (用于模型级别限流)
    /// None 表示账号级别限流,Some(model) 表示特定模型限流
    #[allow(dead_code)] // Used for model-level rate limiting
    pub model: Option<String>,
}

/// 失败计数过期时间：1小时（超过此时间未失败则重置计数）
const FAILURE_COUNT_EXPIRY_SECONDS: u64 = 3600;

/// 限流跟踪器
struct QuotaBucketLimit {
    observed_at: i64,
    reset_time: Option<SystemTime>,
    weekly: bool,
}

pub struct RateLimitTracker {
    limits: DashMap<String, RateLimitInfo>,
    // Independent official quota windows must survive transient-limit resets.
    quota_limits: DashMap<(String, String), QuotaBucketLimit>,
    /// 连续失败计数（用于智能指数退避），带时间戳用于自动过期
    failure_counts: DashMap<String, (u32, SystemTime)>,
}

impl RateLimitTracker {
    pub fn new() -> Self {
        Self {
            limits: DashMap::new(),
            quota_limits: DashMap::new(),
            failure_counts: DashMap::new(),
        }
    }

    /// 生成限流 Key
    /// - 账号级: "account_id"
    /// - 模型级: "account_id:model_id"
    fn get_limit_key(&self, account_id: &str, model: Option<&str>) -> String {
        match model {
            Some(m) if !m.is_empty() => format!("{}:{}", account_id, m),
            _ => account_id.to_string(),
        }
    }

    /// 获取账号剩余的等待时间(秒)
    /// 支持检查账号级和模型级锁
    pub fn get_remaining_wait(&self, account_id: &str, model: Option<&str>) -> u64 {
        let now = SystemTime::now();
        let model_key = self.get_limit_key(account_id, model);
        let direct_wait = [account_id, model_key.as_str()]
            .into_iter()
            .filter_map(|key| self.limits.get(key))
            .filter_map(|info| info.reset_time.duration_since(now).ok())
            .map(|duration| duration.as_secs().max(1))
            .max()
            .unwrap_or(0)
            .max(self.get_quota_wait(account_id, model, false));

        if direct_wait > 0 {
            return direct_wait;
        }

        // [双重守卫] 若直接匹配未命中，通过归一化标准 ID 兜底检查（确保 gemini-*-pro 等所有变体对齐标准组锁定）
        if let Some(m) = model {
            if let Some(std_id) = crate::proxy::common::model_mapping::normalize_to_standard_id(m) {
                if std_id != m {
                    let std_key = self.get_limit_key(account_id, Some(&std_id));
                    let std_wait = self
                        .limits
                        .get(&std_key)
                        .and_then(|info| info.reset_time.duration_since(now).ok())
                        .map(|duration| duration.as_secs().max(1))
                        .unwrap_or(0)
                        .max(self.get_quota_wait(account_id, Some(&std_id), false));
                    if std_wait > 0 {
                        return std_wait;
                    }
                }
            }
        }

        0
    }

    pub fn get_quota_wait(&self, account_id: &str, model: Option<&str>, weekly_only: bool) -> u64 {
        let key = self.get_limit_key(account_id, model);
        let now = SystemTime::now();
        self.quota_limits
            .iter()
            .filter(|entry| {
                (entry.key().0 == key || entry.key().0 == account_id)
                    && (!weekly_only || entry.weekly)
            })
            .filter_map(|entry| entry.reset_time?.duration_since(now).ok())
            .map(|duration| duration.as_secs().max(1))
            .max()
            .unwrap_or(0)
    }

    pub fn sync_quota_bucket(
        &self,
        account_id: &str,
        model: &str,
        bucket_id: &str,
        observed_at: i64,
        exhausted_until: Option<SystemTime>,
        weekly: bool,
    ) {
        let key = (
            self.get_limit_key(account_id, Some(model)),
            bucket_id.to_string(),
        );
        // Keep recovered observations too: reloading an older snapshot must not relock/unlock.
        self.quota_limits
            .entry(key)
            .and_modify(|current| {
                if observed_at > current.observed_at {
                    *current = QuotaBucketLimit {
                        observed_at,
                        reset_time: exhausted_until,
                        weekly,
                    };
                }
            })
            .or_insert(QuotaBucketLimit {
                observed_at,
                reset_time: exhausted_until,
                weekly,
            });
    }

    /// 标记账号请求成功，重置连续失败计数
    ///
    /// 当账号成功完成请求后调用此方法，将其失败计数归零，
    /// 这样下次失败时会从最短的锁定时间（60秒）开始。
    pub fn mark_success(&self, account_id: &str) {
        if self.failure_counts.remove(account_id).is_some() {
            tracing::debug!("账号 {} 请求成功，已重置失败计数", account_id);
        }
        // 清除账号级限流
        self.limits.remove(account_id);
        // 注意：我们暂时无法清除该账号下的所有模型级锁，因为我们不知道哪些模型被锁了
        // 除非遍历 limits。考虑到模型级锁通常是 QuotaExhausted，让其自然过期也是可以接受的。
        // 或者我们可以引入索引，但为了简单，暂时只清除 Account 级锁。
    }

    /// 精确锁定账号到指定时间点 (支持是否遵循 MAX_LOCKOUT_SECONDS 上限)
    ///
    /// # 参数
    /// - `model`: 可选的模型名称,用于模型级别限流。None 表示账号级别限流
    /// - `cap_to_max`: 是否将锁定时长限制在 MAX_LOCKOUT_SECONDS (300s) 以内。
    ///   如果为 false，则直接锁定到真实的 reset_time（用于零配额持续熔断）。
    pub fn set_lockout_until_with_cap(
        &self,
        account_id: &str,
        reset_time: SystemTime,
        reason: RateLimitReason,
        model: Option<String>,
        cap_to_max: bool,
    ) {
        let now = SystemTime::now();
        let (mut retry_sec, mut effective_reset_time) = reset_time
            .duration_since(now)
            .map(|duration| (duration.as_secs(), reset_time))
            .unwrap_or((60, now + Duration::from_secs(60)));

        if cap_to_max && retry_sec > MAX_LOCKOUT_SECONDS {
            tracing::info!(
                "Capping lockout time for {} from {}s to 300s (5 minutes)",
                account_id,
                retry_sec
            );
            retry_sec = MAX_LOCKOUT_SECONDS;
            effective_reset_time = now + Duration::from_secs(retry_sec);
        }

        let info = RateLimitInfo {
            reset_time: effective_reset_time,
            retry_after_sec: retry_sec,
            detected_at: now,
            reason,
            model: model.clone(), // 🆕 支持模型级别限流
        };

        let key = self.get_limit_key(account_id, model.as_deref());

        // [防倒退保护] 若已有更长、未过期的锁定时间，防止被后续较短的重置时间覆盖（如周配额 5 天不被 5H 窗口覆盖）
        if let Some(existing) = self.limits.get(&key) {
            if existing.reset_time > now && existing.reset_time > effective_reset_time {
                tracing::info!(
                    "Retaining existing longer lockout for {} (existing: {}s > new: {}s)",
                    key,
                    existing.retry_after_sec,
                    retry_sec
                );
                return;
            }
        }

        self.limits.insert(key, info);

        if let Some(m) = &model {
            tracing::info!(
                "账号 {} 的模型 {} 已精确锁定到配额刷新时间,剩余 {} 秒 (cap_to_max: {})",
                account_id,
                m,
                retry_sec,
                cap_to_max
            );
        } else {
            tracing::info!(
                "账号 {} 已精确锁定到配额刷新时间,剩余 {} 秒 (cap_to_max: {})",
                account_id,
                retry_sec,
                cap_to_max
            );
        }
    }

    /// 精确锁定账号到指定时间点 (默认限制在 300s 内)
    pub fn set_lockout_until(
        &self,
        account_id: &str,
        reset_time: SystemTime,
        reason: RateLimitReason,
        model: Option<String>,
    ) {
        self.set_lockout_until_with_cap(account_id, reset_time, reason, model, true);
    }

    pub fn restore_persisted_long_limit(
        &self,
        account_id: &str,
        reset_time: SystemTime,
        detected_at: SystemTime,
        model: &str,
    ) -> bool {
        let normalized_model = crate::proxy::common::model_mapping::normalize_to_standard_id(model)
            .unwrap_or_else(|| model.to_string());
        let now = SystemTime::now();
        let Ok(original_duration) = reset_time.duration_since(detected_at) else {
            return false;
        };
        let Ok(remaining) = reset_time.duration_since(now) else {
            return false;
        };
        if original_duration <= Duration::from_secs(MAX_LOCKOUT_SECONDS) {
            return false;
        }

        let info = RateLimitInfo {
            reset_time,
            retry_after_sec: remaining.as_secs(),
            detected_at,
            reason: RateLimitReason::QuotaExhausted,
            model: Some(normalized_model.clone()),
        };
        let key = self.get_limit_key(account_id, Some(&normalized_model));
        self.limits.insert(key, info);
        true
    }

    pub fn restore_persisted_long_image_limit(
        &self,
        account_id: &str,
        reset_time: SystemTime,
        detected_at: SystemTime,
        model: &str,
    ) -> bool {
        if normalize_image_model_id(model).is_none() {
            return false;
        }
        self.restore_persisted_long_limit(account_id, reset_time, detected_at, model)
    }

    pub fn set_lockout_until_iso_with_cap(
        &self,
        account_id: &str,
        reset_time_str: &str,
        reason: RateLimitReason,
        model: Option<String>,
        cap_to_max: bool,
    ) -> bool {
        // 尝试解析 ISO 8601 格式
        match chrono::DateTime::parse_from_rfc3339(reset_time_str) {
            Ok(dt) => {
                let reset_time =
                    SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(dt.timestamp() as u64);
                self.set_lockout_until_with_cap(account_id, reset_time, reason, model, cap_to_max);
                true
            }
            Err(e) => {
                tracing::warn!(
                    "无法解析配额刷新时间 '{}': {},将使用默认退避策略",
                    reset_time_str,
                    e
                );
                false
            }
        }
    }

    /// 使用 ISO 8601 时间字符串精确锁定账号
    ///
    /// 解析类似 "2026-01-08T17:00:00Z" 格式的时间字符串
    ///
    /// # 参数
    /// - `model`: 可选的模型名称,用于模型级别限流
    pub fn set_lockout_until_iso(
        &self,
        account_id: &str,
        reset_time_str: &str,
        reason: RateLimitReason,
        model: Option<String>,
    ) -> bool {
        self.set_lockout_until_iso_with_cap(account_id, reset_time_str, reason, model, true)
    }

    /// 从错误响应解析限流信息
    ///
    /// # Arguments
    /// * `account_id` - 账号 ID
    /// * `status` - HTTP 状态码
    /// * `retry_after_header` - Retry-After header 值
    /// * `body` - 错误响应 body
    pub fn parse_from_error(
        &self,
        account_id: &str,
        status: u16,
        retry_after_header: Option<&str>,
        body: &str,
        model: Option<String>,
        backoff_steps: &[u64], // [NEW] 传入退避配置
    ) -> Option<RateLimitInfo> {
        self.parse_from_error_with_mode(
            account_id,
            status,
            retry_after_header,
            body,
            model,
            backoff_steps,
            RetryParserMode::Current,
        )
    }

    pub fn parse_from_error_baseline(
        &self,
        account_id: &str,
        status: u16,
        retry_after_header: Option<&str>,
        body: &str,
        model: Option<String>,
        backoff_steps: &[u64],
    ) -> Option<RateLimitInfo> {
        self.parse_from_error_with_mode(
            account_id,
            status,
            retry_after_header,
            body,
            model,
            backoff_steps,
            RetryParserMode::Baseline,
        )
    }

    fn parse_from_error_with_mode(
        &self,
        account_id: &str,
        status: u16,
        retry_after_header: Option<&str>,
        body: &str,
        model: Option<String>,
        backoff_steps: &[u64],
        parser_mode: RetryParserMode,
    ) -> Option<RateLimitInfo> {
        // 关键防御：网关内部自产生的排队/无可用账号错误，绝对禁止解析为上游限流，杜绝自噬死循环
        let lower_body = body.to_lowercase();
        if lower_body.contains("all accounts limited")
            || lower_body.contains("no accounts available")
            || lower_body.contains("all accounts failed")
            || lower_body.contains("token pool is empty")
            || lower_body.contains("all accounts exhausted")
            || lower_body.contains("all accounts unhealthy")
        {
            return None;
        }

        // 仅对真正的上游限流 429 和过载 529 进行账号冷却跟踪；500/503 属于服务瞬时不可用，绝不打入冷却池！
        if status != 429 && status != 529 {
            return None;
        }

        // 1. 解析限流原因类型
        let reason = if status == 429 {
            tracing::warn!("Google 429 Error Body: {}", body);
            self.parse_rate_limit_reason(body)
        } else {
            RateLimitReason::ServerError
        };

        let retry_after_sec = match parser_mode {
            RetryParserMode::Current => {
                crate::proxy::upstream::retry::parse_retry_delay(body, retry_after_header)
                    .map(|delay_ms| delay_ms.saturating_add(999) / 1000)
            }
            RetryParserMode::Baseline => retry_after_header
                .and_then(|value| value.parse::<u64>().ok())
                .or_else(|| self.parse_retry_time_from_body_baseline(body)),
        };
        let has_explicit_retry_time = retry_after_sec.is_some();
        let preserve_explicit_quota = parser_mode == RetryParserMode::Current
            && status == 429
            && reason == RateLimitReason::QuotaExhausted
            && has_explicit_quota_exhausted(body)
            && has_explicit_retry_time;

        // 4. 处理默认值与软避让逻辑（根据限流类型设置不同默认值）
        let retry_sec = match retry_after_sec {
            Some(s) => {
                // 设置安全缓冲区：最小 2 秒，防止极高频无效重试
                if s < 2 {
                    2
                } else {
                    s
                }
            }
            None => {
                // 获取连续失败次数，用于指数退避（带自动过期逻辑）
                // [FIX] ServerError (5xx) 不累加 failure_count，避免污染 429 的退避阶梯
                let failure_count = if reason != RateLimitReason::ServerError {
                    // 只有非 ServerError 才累加失败计数（用于指数退避）
                    let now = SystemTime::now();
                    // 这里我们使用 account_id 作为 key，不区分模型，
                    // 因为这里是为了计算连续"账号级"问题的退避。
                    // 如果需要针对模型的连续失败计数，可能需要改变 failure_counts 的 key。
                    // 暂时保持 account_id，这样如果一个模型一直挂，也会增加计数，符合逻辑。
                    let mut entry = self
                        .failure_counts
                        .entry(account_id.to_string())
                        .or_insert((0, now));

                    let elapsed = now
                        .duration_since(entry.1)
                        .unwrap_or(Duration::from_secs(0))
                        .as_secs();
                    if elapsed > FAILURE_COUNT_EXPIRY_SECONDS {
                        tracing::debug!(
                            "账号 {} 失败计数已过期（{}秒），重置为 0",
                            account_id,
                            elapsed
                        );
                        *entry = (0, now);
                    }
                    entry.0 += 1;
                    entry.1 = now;
                    entry.0
                } else {
                    // ServerError (5xx) 使用固定值 1，不累加，避免污染 429 的退避阶梯
                    1
                };

                match reason {
                    RateLimitReason::QuotaExhausted => {
                        // [智能限流] 根据 failure_count 和配置的 backoff_steps 计算
                        let index = (failure_count as usize).saturating_sub(1);
                        let lockout = if index < backoff_steps.len() {
                            backoff_steps[index]
                        } else {
                            *backoff_steps.last().unwrap_or(&7200)
                        };

                        tracing::warn!(
                            "检测到配额耗尽 (QUOTA_EXHAUSTED)，第{}次连续失败，根据配置锁定 {} 秒",
                            failure_count,
                            lockout
                        );
                        lockout
                    }
                    RateLimitReason::RateLimitExceeded => {
                        // 速率限制 (TPM/RPM)
                        let body_lower = body.to_lowercase();
                        let lockout = if body_lower.contains("resource has been exhausted")
                            || body_lower.contains("resource_exhausted")
                        {
                            30
                        } else {
                            5
                        };
                        tracing::debug!(
                            "检测到速率限制 (RATE_LIMIT_EXCEEDED)，使用默认值 {}秒",
                            lockout
                        );
                        lockout
                    }
                    RateLimitReason::ModelCapacityExhausted => {
                        // 模型容量耗尽
                        let lockout = match failure_count {
                            1 => 5,
                            2 => 10,
                            _ => 15,
                        };
                        tracing::warn!(
                            "检测到模型容量不足 (MODEL_CAPACITY_EXHAUSTED)，第{}次失败，{}秒后重试",
                            failure_count,
                            lockout
                        );
                        lockout
                    }
                    RateLimitReason::ServerError => {
                        let lockout = 8;
                        tracing::warn!("检测到 {} 错误, 执行 {}s 软避让...", status, lockout);
                        lockout
                    }
                    RateLimitReason::Unknown => {
                        // 未知原因
                        tracing::debug!("无法解析 429 限流原因, 使用默认值 60秒");
                        60
                    }
                }
            }
        };

        let mut retry_sec = retry_sec;
        let max_allowed_lockout = backoff_steps
            .iter()
            .copied()
            .max()
            .unwrap_or(MAX_LOCKOUT_SECONDS)
            .max(MAX_LOCKOUT_SECONDS);
        if retry_sec > max_allowed_lockout && !preserve_explicit_quota {
            tracing::info!(
                "Capping retry lockout time for {} from {}s to {}s (max backoff limit)",
                account_id,
                retry_sec,
                max_allowed_lockout
            );
            retry_sec = max_allowed_lockout;
        }

        let info = RateLimitInfo {
            reset_time: SystemTime::now() + Duration::from_secs(retry_sec),
            retry_after_sec: retry_sec,
            detected_at: SystemTime::now(),
            reason,
            model: model.clone(),
        };

        // [FIX] 细粒度模型隔离：只要调用方传入了具体的 model，限流必须针对该 model 进行隔离！
        // 杜绝因某单个模型（或不存在的模型/特定模型限流）而将全账号的所有模型连坐封锁，导致正常账号宕机。
        let key = if let Some(m) = model.as_deref().filter(|s| !s.is_empty()) {
            self.get_limit_key(account_id, Some(m))
        } else {
            account_id.to_string()
        };

        self.limits.insert(key, info.clone());

        tracing::warn!(
            "账号 {} [{}] 限流类型: {:?}, 重置延时: {}秒",
            account_id,
            status,
            reason,
            retry_sec
        );

        Some(info)
    }

    /// 解析限流原因类型
    fn parse_rate_limit_reason(&self, body: &str) -> RateLimitReason {
        // 尝试从 JSON 中提取 reason 字段
        let trimmed = body.trim();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(reason_str) = json
                    .get("error")
                    .and_then(|e| e.get("details"))
                    .and_then(|d| d.as_array())
                    .and_then(|a| a.get(0))
                    .and_then(|o| o.get("reason"))
                    .and_then(|v| v.as_str())
                {
                    return match reason_str {
                        "QUOTA_EXHAUSTED" => RateLimitReason::QuotaExhausted,
                        "RATE_LIMIT_EXCEEDED" => RateLimitReason::RateLimitExceeded,
                        "MODEL_CAPACITY_EXHAUSTED" => RateLimitReason::ModelCapacityExhausted,
                        _ => RateLimitReason::Unknown,
                    };
                }
                // [NEW] 尝试从 message 字段进行文本匹配（防止 missed reason）
                if let Some(msg) = json
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|v| v.as_str())
                {
                    let msg_lower = msg.to_lowercase();
                    if msg_lower.contains("per minute") || msg_lower.contains("rate limit") {
                        return RateLimitReason::RateLimitExceeded;
                    }
                }
            }
        }

        // 如果无法从 JSON 解析，尝试从消息文本判断
        let body_lower = body.to_lowercase();
        // [FIX] 优先判断分钟级限制，避免将 TPM 误判为 Quota
        let generic_resource_exhausted = body_lower.contains("resource has been exhausted")
            || body_lower.contains("resource_exhausted");
        let explicit_quota_exhausted = body_lower.contains("quotaresetdelay")
            || body_lower.contains("quotareset")
            || body_lower.contains("quota reset")
            || body_lower.contains("quota limit")
            || body_lower.contains("per day")
            || body_lower.contains("daily quota")
            || body_lower.contains("credits")
            || (body_lower.contains("quota_exhausted")
                && crate::proxy::upstream::retry::parse_retry_delay(body, None).is_some());

        if body_lower.contains("per minute")
            || body_lower.contains("rate limit")
            || body_lower.contains("too many requests")
            || (generic_resource_exhausted && !explicit_quota_exhausted)
        {
            RateLimitReason::RateLimitExceeded
        } else if explicit_quota_exhausted {
            RateLimitReason::QuotaExhausted
        } else if body_lower.contains("exhausted") || body_lower.contains("quota") {
            RateLimitReason::RateLimitExceeded
        } else {
            RateLimitReason::Unknown
        }
    }

    /// 从错误消息 body 中解析重置时间
    fn parse_retry_time_from_body(&self, body: &str) -> Option<u64> {
        crate::proxy::upstream::retry::parse_retry_delay(body, None)
            .map(|delay_ms| delay_ms.saturating_add(999) / 1000)
    }

    fn parse_duration_string_baseline(&self, value: &str) -> Option<u64> {
        let re = Regex::new(r"(?:(\d+)h)?(?:(\d+)m)?(?:(\d+(?:\.\d+)?)s)?(?:(\d+(?:\.\d+)?)ms)?")
            .ok()?;
        let captures = re.captures(value)?;
        let hours = captures
            .get(1)
            .and_then(|value| value.as_str().parse::<u64>().ok())
            .unwrap_or(0);
        let minutes = captures
            .get(2)
            .and_then(|value| value.as_str().parse::<u64>().ok())
            .unwrap_or(0);
        let seconds = captures
            .get(3)
            .and_then(|value| value.as_str().parse::<f64>().ok())
            .unwrap_or(0.0);
        let milliseconds = captures
            .get(4)
            .and_then(|value| value.as_str().parse::<f64>().ok())
            .unwrap_or(0.0);
        let total_seconds = hours * 3600
            + minutes * 60
            + seconds.ceil() as u64
            + (milliseconds / 1000.0).ceil() as u64;
        (total_seconds > 0).then_some(total_seconds)
    }

    fn parse_retry_time_from_body_baseline(&self, body: &str) -> Option<u64> {
        let trimmed = body.trim();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(delay) = json
                    .get("error")
                    .and_then(|error| error.get("details"))
                    .and_then(|details| details.as_array())
                    .and_then(|details| details.first())
                    .and_then(|detail| detail.get("metadata"))
                    .and_then(|metadata| metadata.get("quotaResetDelay"))
                    .and_then(|value| value.as_str())
                    .and_then(|value| self.parse_duration_string_baseline(value))
                {
                    return Some(delay);
                }
                if let Some(retry) = json
                    .get("error")
                    .and_then(|error| error.get("retry_after"))
                    .and_then(|value| value.as_u64())
                {
                    return Some(retry);
                }
            }
        }

        for pattern in [
            r"(?i)try again in (\d+)m\s*(\d+)s",
            r"(?i)(?:try again in|backoff for|wait)\s*(\d+)s",
            r"(?i)quota will reset in (\d+) second",
            r"(?i)retry after (\d+) second",
            r"\(wait (\d+)s\)",
        ] {
            let captures = Regex::new(pattern).ok()?.captures(body);
            let Some(captures) = captures else {
                continue;
            };
            if captures.len() == 3 {
                let minutes = captures.get(1)?.as_str().parse::<u64>().ok()?;
                let seconds = captures.get(2)?.as_str().parse::<u64>().ok()?;
                return Some(minutes * 60 + seconds);
            }
            if let Some(seconds) = captures
                .get(1)
                .and_then(|value| value.as_str().parse::<u64>().ok())
            {
                return Some(seconds);
            }
        }
        None
    }

    /// 获取账号的限流信息
    pub fn get(&self, account_id: &str) -> Option<RateLimitInfo> {
        self.limits.get(account_id).map(|r| r.clone())
    }

    pub fn clear_model(&self, account_id: &str, model: &str) -> bool {
        let normalized = crate::proxy::common::model_mapping::normalize_to_standard_id(model)
            .unwrap_or_else(|| model.to_string());
        let mut cleared = self
            .limits
            .remove(&self.get_limit_key(account_id, Some(&normalized)))
            .is_some();
        if normalized != model {
            cleared |= self
                .limits
                .remove(&self.get_limit_key(account_id, Some(model)))
                .is_some();
        }
        cleared
    }

    /// 安全释放因配额耗尽产生的持续锁定：
    /// - 仅清除 reason 为 QuotaExhausted 的记录，严禁误触 429 速率限制 (RateLimitExceeded)、服务器错误等独立限流；
    /// - 针对直接或标准 ID 均做精确比对；
    pub fn reconcile_quota_recovery(&self, account_id: &str, model: &str) -> bool {
        let normalized = crate::proxy::common::model_mapping::normalize_to_standard_id(model)
            .unwrap_or_else(|| model.to_string());

        let keys = if normalized != model {
            vec![
                self.get_limit_key(account_id, Some(&normalized)),
                self.get_limit_key(account_id, Some(model)),
            ]
        } else {
            vec![self.get_limit_key(account_id, Some(model))]
        };

        let mut cleared = false;
        for key in keys {
            if let Some(entry) = self.limits.get(&key) {
                if entry.reason == RateLimitReason::QuotaExhausted {
                    drop(entry);
                    if self.limits.remove(&key).is_some() {
                        cleared = true;
                    }
                }
            }
        }
        cleared
    }

    /// 检查账号是否仍在限流中
    /// 检查账号是否仍在限流中 (支持模型级)
    pub fn is_rate_limited(&self, account_id: &str, model: Option<&str>) -> bool {
        // Checking using get_remaining_wait which handles both global and model keys
        self.get_remaining_wait(account_id, model) > 0
    }

    /// 获取距离限流重置还有多少秒
    pub fn get_reset_seconds(&self, account_id: &str) -> Option<u64> {
        if let Some(info) = self.get(account_id) {
            info.reset_time
                .duration_since(SystemTime::now())
                .ok()
                .map(|d| d.as_secs())
        } else {
            None
        }
    }

    /// 清除过期的限流记录
    #[allow(dead_code)]
    pub fn cleanup_expired(&self) -> usize {
        let now = SystemTime::now();
        let mut count = 0;

        self.limits.retain(|_k, v| {
            if v.reset_time <= now {
                count += 1;
                false
            } else {
                true
            }
        });

        if count > 0 {
            tracing::debug!("清除了 {} 个过期的限流记录", count);
        }

        count
    }

    /// 只清除账号本身的全局限流（不清除具体的模型级配额耗尽锁定）
    pub fn clear_account_only(&self, account_id: &str) -> bool {
        let cleared = self.limits.remove(account_id).is_some();
        self.failure_counts.remove(account_id);
        cleared
    }

    /// 清除指定账号的限流记录
    pub fn clear(&self, account_id: &str) -> bool {
        let prefix = format!("{}:", account_id);
        let before = self.limits.len();
        self.limits
            .retain(|key, _| key != account_id && !key.starts_with(&prefix));
        self.failure_counts.remove(account_id);
        self.limits.len() != before
    }

    pub fn clear_for_optimistic_reset(&self) {
        let now = SystemTime::now();
        self.limits.retain(|_, info| {
            info.reason == RateLimitReason::QuotaExhausted
                && info
                    .reset_time
                    .duration_since(info.detected_at)
                    .is_ok_and(|duration| duration > Duration::from_secs(MAX_LOCKOUT_SECONDS))
                && info.reset_time.duration_since(now).is_ok()
        });
    }

    /// 清除所有限流记录 (乐观重置策略)
    ///
    /// 用于乐观重置机制,当所有账号都被限流但等待时间很短时,
    /// 清除所有限流记录以解决时序竞争条件
    pub fn clear_all(&self) {
        let count = self.limits.len();
        self.limits.clear();
        tracing::warn!(
            "🔄 Optimistic reset: Cleared all {} rate limit record(s)",
            count
        );
    }
}

impl Default for RateLimitTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_retry_time_minutes_seconds() {
        let tracker = RateLimitTracker::new();
        let body = "Rate limit exceeded. Try again in 2m 30s";
        let time = tracker.parse_retry_time_from_body(body);
        assert_eq!(time, Some(150));
    }

    #[test]
    fn test_parse_google_json_delay() {
        let tracker = RateLimitTracker::new();
        let body = r#"{
            "error": {
                "details": [
                    { 
                        "metadata": {
                            "quotaResetDelay": "42s" 
                        }
                    }
                ]
            }
        }"#;
        let time = tracker.parse_retry_time_from_body(body);
        assert_eq!(time, Some(42));
    }

    #[test]
    fn test_parse_retry_after_ignore_case() {
        let tracker = RateLimitTracker::new();
        let body = "Quota limit hit. Retry After 99 Seconds";
        let time = tracker.parse_retry_time_from_body(body);
        assert_eq!(time, Some(99));
    }

    #[test]
    fn test_get_remaining_wait() {
        let tracker = RateLimitTracker::new();
        tracker.parse_from_error("acc1", 429, Some("30"), "", None, &[]);
        let wait = tracker.get_remaining_wait("acc1", None);
        assert!(wait > 25 && wait <= 30);
    }

    #[test]
    fn test_safety_buffer() {
        let tracker = RateLimitTracker::new();
        // 如果 API 返回 1s，我们强制设为 2s
        tracker.parse_from_error("acc1", 429, Some("1"), "", None, &[]);
        let wait = tracker.get_remaining_wait("acc1", None);
        // Due to time passing, it might be 1 or 2
        assert!(wait >= 1 && wait <= 2);
    }

    #[test]
    fn task_preserves_explicit_long_image_quota_deadline() {
        let tracker = RateLimitTracker::new();
        let body = r#"{
            "error": {
                "details": [{
                    "reason": "QUOTA_EXHAUSTED",
                    "metadata": {"quotaResetDelay": "58h24m53s"}
                }]
            }
        }"#;
        let info = tracker
            .parse_from_error(
                "acc-long",
                429,
                None,
                body,
                Some("gemini-3-pro-image".to_string()),
                &[60, 300],
            )
            .unwrap();
        assert_eq!(info.retry_after_sec, 210_293);

        tracker.clear_for_optimistic_reset();
        assert!(tracker.is_rate_limited("acc-long", Some("gemini-3-pro-image")));

        let now = SystemTime::now();
        assert!(tracker.restore_persisted_long_image_limit(
            "acc-expiring",
            now + Duration::from_secs(30),
            now - Duration::from_secs(301),
            "gemini-3.1-flash-image",
        ));
        tracker.clear_for_optimistic_reset();
        assert!(tracker.is_rate_limited("acc-expiring", Some("gemini-3.1-flash-image")));

        let inferred = tracker
            .parse_from_error(
                "acc-inferred",
                429,
                None,
                "Quota limit hit; reset after 72h",
                Some("gemini-3-pro-image".to_string()),
                &[60, 300],
            )
            .unwrap();
        assert_eq!(inferred.retry_after_sec, 300);

        let text_model = tracker
            .parse_from_error(
                "acc-text",
                429,
                Some("72h"),
                r#"{"error":{"details":[{"reason":"QUOTA_EXHAUSTED"}]}}"#,
                Some("gemini-2.5-pro".to_string()),
                &[60, 300],
            )
            .unwrap();
        assert_eq!(text_model.retry_after_sec, 300);

        let broad_quota = tracker
            .parse_from_error(
                "acc-broad",
                429,
                None,
                "Quota limit hit; reset after 72h",
                Some("gemini-3.1-flash-image".to_string()),
                &[60, 300],
            )
            .unwrap();
        assert_eq!(broad_quota.retry_after_sec, 300);

        let inferred_reset = SystemTime::now() + Duration::from_secs(72 * 3600);
        tracker.set_lockout_until(
            "acc-inferred-reset",
            inferred_reset,
            RateLimitReason::QuotaExhausted,
            Some("gemini-3-pro-image".to_string()),
        );
        assert!(
            tracker.get_remaining_wait("acc-inferred-reset", Some("gemini-3-pro-image")) <= 300
        );
    }

    #[test]
    fn task_claude_baseline_ignores_retry_info_delay() {
        for (index, delay) in ["1s", "3s"].into_iter().enumerate() {
            let body = format!(
                r#"{{"error":{{"details":[{{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"{}"}}]}}}}"#,
                delay
            );
            let current = RateLimitTracker::new()
                .parse_from_error(
                    &format!("current-{}", index),
                    429,
                    None,
                    &body,
                    None,
                    &[60, 300],
                )
                .unwrap();
            assert_eq!(current.retry_after_sec, if index == 0 { 2 } else { 3 });

            let baseline = RateLimitTracker::new()
                .parse_from_error_baseline(
                    &format!("baseline-{}", index),
                    429,
                    None,
                    &body,
                    None,
                    &[60, 300],
                )
                .unwrap();
            assert_eq!(baseline.retry_after_sec, 60);
        }
    }

    #[test]
    fn test_tpm_exhausted_is_rate_limit_exceeded() {
        let tracker = RateLimitTracker::new();
        // 模拟真实世界的 TPM 错误，同时包含 "Resource exhausted" 和 "per minute"
        let body = "Resource has been exhausted (e.g. check quota). Quota limit 'Tokens per minute' exceeded.";
        let reason = tracker.parse_rate_limit_reason(body);
        // 应该被识别为 RateLimitExceeded，而不是 QuotaExhausted
        assert_eq!(reason, RateLimitReason::RateLimitExceeded);
    }

    #[test]
    fn test_generic_resource_exhausted_is_short_rate_limit() {
        let tracker = RateLimitTracker::new();
        let body = r#"{
            "error": {
                "code": 429,
                "message": "Resource has been exhausted (e.g. check quota).",
                "status": "RESOURCE_EXHAUSTED"
            }
        }"#;
        let reason = tracker.parse_rate_limit_reason(body);
        assert_eq!(reason, RateLimitReason::RateLimitExceeded);
    }

    #[test]
    fn test_server_error_does_not_accumulate_failure_count() {
        let tracker = RateLimitTracker::new();
        let backoff_steps = vec![60, 300, 1800, 7200];

        // 模拟连续 5 次 5xx 错误
        for i in 1..=5 {
            let info = tracker.parse_from_error(
                "acc1",
                503,
                None,
                "Service Unavailable",
                None,
                &backoff_steps,
            );
            assert!(info.is_some(), "第 {} 次 5xx 应该返回 RateLimitInfo", i);
            let info = info.unwrap();
            // 5xx 应该始终锁定 8 秒，不受 failure_count 影响
            assert_eq!(info.retry_after_sec, 8, "5xx 第 {} 次应该锁定 8 秒", i);
        }

        // 现在触发一次 429 QuotaExhausted（没有 quotaResetDelay）
        let quota_body = r#"{"error":{"details":[{"reason":"QUOTA_EXHAUSTED"}]}}"#;
        let info = tracker.parse_from_error("acc1", 429, None, quota_body, None, &backoff_steps);
        assert!(info.is_some());
        let info = info.unwrap();

        // 关键断言：429 应该从第 1 次开始（锁 60 秒），而不是继承 5xx 的计数
        assert_eq!(
            info.retry_after_sec, 60,
            "429 应该从第 1 次退避开始(60秒),而不是被 5xx 污染"
        );
    }

    #[test]
    fn test_quota_exhausted_does_accumulate_failure_count() {
        let tracker = RateLimitTracker::new();
        let backoff_steps = vec![60, 300, 1800, 7200];
        let quota_body = r#"{"error":{"details":[{"reason":"QUOTA_EXHAUSTED"}]}}"#;

        // 第 1 次 429 → 60 秒
        let info = tracker.parse_from_error("acc2", 429, None, quota_body, None, &backoff_steps);
        assert_eq!(info.unwrap().retry_after_sec, 60);

        // 第 2 次 429 → 300 秒
        let info = tracker.parse_from_error("acc2", 429, None, quota_body, None, &backoff_steps);
        assert_eq!(info.unwrap().retry_after_sec, 300);

        // 第 3 次 429 → 1800 秒
        let info = tracker.parse_from_error("acc2", 429, None, quota_body, None, &backoff_steps);
        assert_eq!(info.unwrap().retry_after_sec, 1800);

        // 第 4 次 429 → 7200 秒
        let info = tracker.parse_from_error("acc2", 429, None, quota_body, None, &backoff_steps);
        assert_eq!(info.unwrap().retry_after_sec, 7200);
    }

    #[test]
    fn test_set_lockout_until_with_cap() {
        let tracker = RateLimitTracker::new();
        let target_time = SystemTime::now() + Duration::from_secs(5 * 3600); // 5 hours

        // Capped: should be capped to 300s
        tracker.set_lockout_until_with_cap(
            "acc_cap",
            target_time,
            RateLimitReason::QuotaExhausted,
            None,
            true,
        );
        let wait_capped = tracker.get_remaining_wait("acc_cap", None);
        assert!(wait_capped <= 300 && wait_capped >= 290);

        // Uncapped (Zero Quota): should retain full 5 hours duration
        tracker.set_lockout_until_with_cap(
            "acc_uncap",
            target_time,
            RateLimitReason::QuotaExhausted,
            None,
            false,
        );
        let wait_uncapped = tracker.get_remaining_wait("acc_uncap", None);
        assert!(wait_uncapped > 300 && wait_uncapped <= 5 * 3600);
    }

    #[test]
    fn test_reconcile_quota_recovery_clears_quota_exhausted_but_keeps_rate_limit_exceeded() {
        let tracker = RateLimitTracker::new();
        let target_time = SystemTime::now() + Duration::from_secs(3600);

        // 1. 设置一个 QuotaExhausted 锁定
        tracker.set_lockout_until_with_cap(
            "acc_test",
            target_time,
            RateLimitReason::QuotaExhausted,
            Some("claude-sonnet-4-6".to_string()),
            false,
        );
        assert!(tracker.is_rate_limited("acc_test", Some("claude-sonnet-4-6")));

        // 恢复配额：应该成功解除
        assert!(tracker.reconcile_quota_recovery("acc_test", "claude-sonnet-4-6"));
        assert!(!tracker.is_rate_limited("acc_test", Some("claude-sonnet-4-6")));

        // 2. 设置一个 RateLimitExceeded (429 速率限制)
        tracker.set_lockout_until_with_cap(
            "acc_test",
            target_time,
            RateLimitReason::RateLimitExceeded,
            Some("claude-sonnet-4-6".to_string()),
            false,
        );
        assert!(tracker.is_rate_limited("acc_test", Some("claude-sonnet-4-6")));

        // 配额恢复：严禁清除独立 429 速率限制！
        assert!(!tracker.reconcile_quota_recovery("acc_test", "claude-sonnet-4-6"));
        assert!(tracker.is_rate_limited("acc_test", Some("claude-sonnet-4-6")));
    }

    #[test]
    fn test_restore_persisted_long_limit_supports_text_models() {
        let tracker = RateLimitTracker::new();
        let now = SystemTime::now();
        let detected_at = now - Duration::from_secs(60);
        let reset_time = now + Duration::from_secs(86400); // 24小时显式长锁定

        // 文本模型应该成功恢复长锁定
        assert!(tracker.restore_persisted_long_limit(
            "acc_text",
            reset_time,
            detected_at,
            "gemini-2.5-pro",
        ));
        assert!(tracker.is_rate_limited("acc_text", Some("gemini-2.5-pro")));
        // 归一化标准 ID 也能探测到该锁定
        assert!(tracker.is_rate_limited("acc_text", Some("gemini-3-pro-high")));
    }
}
