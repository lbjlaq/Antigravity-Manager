use dashmap::DashMap;
use regex::Regex;
use std::time::{Duration, SystemTime};

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
pub struct RateLimitTracker {
    limits: DashMap<String, RateLimitInfo>,
    /// 连续失败计数（用于智能指数退避），带时间戳用于自动过期
    failure_counts: DashMap<String, (u32, SystemTime)>,
}

impl RateLimitTracker {
    pub fn new() -> Self {
        Self {
            limits: DashMap::new(),
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

        // 1. 检查全局账号锁
        if let Some(info) = self.limits.get(account_id) {
            if info.reset_time > now {
                return info
                    .reset_time
                    .duration_since(now)
                    .unwrap_or(Duration::from_secs(0))
                    .as_secs();
            }
        }

        // 2. 如果指定了模型，检查模型级锁
        if let Some(m) = model {
            let key = self.get_limit_key(account_id, Some(m));
            if let Some(info) = self.limits.get(&key) {
                if info.reset_time > now {
                    return info
                        .reset_time
                        .duration_since(now)
                        .unwrap_or(Duration::from_secs(0))
                        .as_secs();
                }
            }
        }

        0
    }

    /// 标记账号请求成功，重置连续失败计数
    ///
    /// 当账号成功完成请求后调用此方法，将其失败计数归零，
    /// 并清除相关的限流锁（包括账号级和指定的模型级锁）。
    pub fn mark_success(&self, account_id: &str, model: Option<&str>) {
        if self.failure_counts.remove(account_id).is_some() {
            tracing::debug!("账号 {} 请求成功，已重置失败计数", account_id);
        }

        // 1. 清除账号级全局限流
        self.limits.remove(account_id);

        // 2. 如果指定了模型，也清除该模型的特定限流锁
        if let Some(m) = model {
            let key = self.get_limit_key(account_id, Some(m));
            if self.limits.remove(&key).is_some() {
                tracing::debug!("账号 {} 的模型 {} 限流锁已清除", account_id, m);
            }
        }
    }

    /// 精确锁定账号到指定时间点
    ///
    /// 使用账号配额中的 reset_time 来精确锁定账号,
    /// 这比指数退避更加精准。
    ///
    /// # 参数
    /// - `model`: 可选的模型名称,用于模型级别限流。None 表示账号级别限流
    pub fn set_lockout_until(
        &self,
        account_id: &str,
        reset_time: SystemTime,
        reason: RateLimitReason,
        model: Option<String>,
    ) {
        let now = SystemTime::now();
        let retry_sec = reset_time
            .duration_since(now)
            .map(|d| d.as_secs())
            .unwrap_or(60); // 如果时间已过,使用默认 60 秒

        let info = RateLimitInfo {
            reset_time,
            retry_after_sec: retry_sec,
            detected_at: now,
            reason,
            model: model.clone(), // 🆕 支持模型级别限流
        };

        let key = self.get_limit_key(account_id, model.as_deref());
        self.limits.insert(key, info);

        if let Some(m) = &model {
            tracing::info!(
                "账号 {} 的模型 {} 已精确锁定到配额刷新时间,剩余 {} 秒",
                account_id,
                m,
                retry_sec
            );
        } else {
            tracing::info!(
                "账号 {} 已精确锁定到配额刷新时间,剩余 {} 秒",
                account_id,
                retry_sec
            );
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
        // 尝试解析 ISO 8601 格式
        match chrono::DateTime::parse_from_rfc3339(reset_time_str) {
            Ok(dt) => {
                let reset_time =
                    SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(dt.timestamp() as u64);
                self.set_lockout_until(account_id, reset_time, reason, model);
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
        // 支持 429 (限流) 以及 500/503/529 (后端故障软避让)
        if status != 429 && status != 500 && status != 503 && status != 529 {
            return None;
        }

        // 1. 解析限流原因类型
        let reason = if status == 429 {
            tracing::warn!("Google 429 Error Body: {}", body);
            self.parse_rate_limit_reason(body)
        } else {
            RateLimitReason::ServerError
        };

        let mut retry_after_sec = None;

        // 2. 从 Retry-After header 提取
        if let Some(retry_after) = retry_after_header {
            if let Ok(seconds) = retry_after.parse::<u64>() {
                retry_after_sec = Some(seconds);
            }
        }

        // 3. 从错误消息提取 (优先尝试 JSON 解析，再试正则)
        if retry_after_sec.is_none() {
            retry_after_sec = self.parse_retry_time_from_body(body);
        }

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
                        tracing::debug!("检测到速率限制 (RATE_LIMIT_EXCEEDED)，使用默认值 5秒");
                        5
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
                        // 5xx 错误
                        tracing::warn!("检测到 5xx 错误 ({}), 执行 8s 软避让...", status);
                        8
                    }
                    RateLimitReason::Unknown => {
                        // 未知原因
                        tracing::debug!("无法解析 429 限流原因, 使用默认值 60秒");
                        60
                    }
                }
            }
        };

        let info = RateLimitInfo {
            reset_time: SystemTime::now() + Duration::from_secs(retry_sec),
            retry_after_sec: retry_sec,
            detected_at: SystemTime::now(),
            reason,
            model: model.clone(),
        };

        // [FIX] 使用复合 Key 存储 (如果是 Quota 且有 Model)
        // 只有 QuotaExhausted 适合做模型隔离，其他如 RateLimitExceeded 通常是全账号的 TPM
        let use_model_key = matches!(reason, RateLimitReason::QuotaExhausted) && model.is_some();
        let key = if use_model_key {
            self.get_limit_key(account_id, model.as_deref())
        } else {
            // 其他情况（如 RateLimitExceeded, ServerError）通常影响整个账号
            // 或者我们也可以根据配置决定是否隔离。
            // 简单起见，只有 QuotaExhausted 做细粒度隔离。
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
        if body_lower.contains("per minute")
            || body_lower.contains("rate limit")
            || body_lower.contains("too many requests")
        {
            RateLimitReason::RateLimitExceeded
        } else if body_lower.contains("exhausted") || body_lower.contains("quota") {
            RateLimitReason::QuotaExhausted
        } else {
            RateLimitReason::Unknown
        }
    }

    /// 通用时间解析函数：支持 "2h1m1s" 等所有格式组合
    fn parse_duration_string(&self, s: &str) -> Option<u64> {
        tracing::debug!("[时间解析] 尝试解析: '{}'", s);

        // 使用正则表达式提取小时、分钟、秒、毫秒
        // 支持格式："2h1m1s", "1h30m", "5m", "30s", "500ms", "510.790006ms" 等
        // 🔧 [FIX] 修改 ms 部分支持小数: (\d+)ms -> (\d+(?:\.\d+)?)ms
        let re = Regex::new(r"(?:(\d+)h)?(?:(\d+)m)?(?:(\d+(?:\.\d+)?)s)?(?:(\d+(?:\.\d+)?)ms)?")
            .ok()?;
        let caps = match re.captures(s) {
            Some(c) => c,
            None => {
                tracing::warn!("[时间解析] 正则未匹配: '{}'", s);
                return None;
            }
        };

        let hours = caps
            .get(1)
            .and_then(|m| m.as_str().parse::<u64>().ok())
            .unwrap_or(0);
        let minutes = caps
            .get(2)
            .and_then(|m| m.as_str().parse::<u64>().ok())
            .unwrap_or(0);
        let seconds = caps
            .get(3)
            .and_then(|m| m.as_str().parse::<f64>().ok())
            .unwrap_or(0.0);
        // 🔧 [FIX] 毫秒也支持小数解析
        let milliseconds = caps
            .get(4)
            .and_then(|m| m.as_str().parse::<f64>().ok())
            .unwrap_or(0.0);

        tracing::debug!(
            "[时间解析] 提取结果: {}h {}m {:.3}s {:.3}ms",
            hours,
            minutes,
            seconds,
            milliseconds
        );

        // 🔧 [FIX] 计算总秒数，毫秒部分向上取整
        let total_seconds = hours * 3600
            + minutes * 60
            + seconds.ceil() as u64
            + (milliseconds / 1000.0).ceil() as u64;

        // 如果总秒数为 0，说明解析失败
        if total_seconds == 0 {
            tracing::warn!("[时间解析] 失败: '{}' (总秒数为0)", s);
            None
        } else {
            tracing::info!(
                "[时间解析] ✓ 成功: '{}' => {}秒 ({}h {}m {:.1}s {:.1}ms)",
                s,
                total_seconds,
                hours,
                minutes,
                seconds,
                milliseconds
            );
            Some(total_seconds)
        }
    }

    /// 从错误消息 body 中解析重置时间
    fn parse_retry_time_from_body(&self, body: &str) -> Option<u64> {
        // A. 优先尝试 JSON 精准解析
        let trimmed = body.trim();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
                // 1. Google 常见的 quotaResetDelay 格式 (支持所有格式："2h1m1s", "1h30m", "42s", "500ms" 等)
                // 路径: error.details[0].metadata.quotaResetDelay
                if let Some(delay_str) = json
                    .get("error")
                    .and_then(|e| e.get("details"))
                    .and_then(|d| d.as_array())
                    .and_then(|a| a.get(0))
                    .and_then(|o| o.get("metadata")) // 添加 metadata 层级
                    .and_then(|m| m.get("quotaResetDelay"))
                    .and_then(|v| v.as_str())
                {
                    tracing::debug!("[JSON解析] 找到 quotaResetDelay: '{}'", delay_str);

                    // 使用通用时间解析函数
                    if let Some(seconds) = self.parse_duration_string(delay_str) {
                        return Some(seconds);
                    }
                }

                // 2. OpenAI 常见的 retry_after 字段 (数字)
                if let Some(retry) = json
                    .get("error")
                    .and_then(|e| e.get("retry_after"))
                    .and_then(|v| v.as_u64())
                {
                    return Some(retry);
                }
            }
        }

        // B. 正则匹配模式 (兜底)
        // 模式 1: "Try again in 2m 30s"
        if let Ok(re) = Regex::new(r"(?i)try again in (\d+)m\s*(\d+)s") {
            if let Some(caps) = re.captures(body) {
                if let (Ok(m), Ok(s)) = (caps[1].parse::<u64>(), caps[2].parse::<u64>()) {
                    return Some(m * 60 + s);
                }
            }
        }

        // 模式 2: "Try again in 30s" 或 "backoff for 42s"
        if let Ok(re) = Regex::new(r"(?i)(?:try again in|backoff for|wait)\s*(\d+)s") {
            if let Some(caps) = re.captures(body) {
                if let Ok(s) = caps[1].parse::<u64>() {
                    return Some(s);
                }
            }
        }

        // 模式 3: "quota will reset in X seconds"
        if let Ok(re) = Regex::new(r"(?i)quota will reset in (\d+) second") {
            if let Some(caps) = re.captures(body) {
                if let Ok(s) = caps[1].parse::<u64>() {
                    return Some(s);
                }
            }
        }

        // 模式 4: OpenAI 风格的 "Retry after (\d+) seconds"
        if let Ok(re) = Regex::new(r"(?i)retry after (\d+) second") {
            if let Some(caps) = re.captures(body) {
                if let Ok(s) = caps[1].parse::<u64>() {
                    return Some(s);
                }
            }
        }

        // 模式 5: 括号形式 "(wait (\d+)s)"
        if let Ok(re) = Regex::new(r"\(wait (\d+)s\)") {
            if let Some(caps) = re.captures(body) {
                if let Ok(s) = caps[1].parse::<u64>() {
                    return Some(s);
                }
            }
        }

        None
    }

    /// 获取账号的限流信息
    pub fn get(&self, account_id: &str) -> Option<RateLimitInfo> {
        self.limits.get(account_id).map(|r| r.clone())
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

    /// 清除指定账号的限流记录
    pub fn clear(&self, account_id: &str) -> bool {
        self.limits.remove(account_id).is_some()
    }

    /// [FIX] Clear only expired or nearly-expired rate limit records (optimistic reset)
    ///
    /// This is safer than clear_all() - only clears records that have expired or
    /// will expire within the buffer_seconds threshold. Prevents clearing accounts
    /// with long QUOTA_EXHAUSTED lockouts.
    ///
    /// # Arguments
    /// * `buffer_seconds` - Also clear records expiring within this many seconds
    ///
    /// # Returns
    /// Number of records cleared
    pub fn clear_expired_with_buffer(&self, buffer_seconds: u64) -> usize {
        let now = SystemTime::now();
        let buffer = Duration::from_secs(buffer_seconds);
        let threshold = now + buffer;
        let mut count = 0;

        self.limits.retain(|key, info| {
            if info.reset_time <= threshold {
                tracing::debug!(
                    "🧹 Optimistic reset: Clearing expired/near-expired record for {}",
                    key
                );
                count += 1;
                false
            } else {
                true
            }
        });

        if count > 0 {
            tracing::info!(
                "🔄 Optimistic reset: Cleared {} expired/near-expired rate limit record(s) (buffer: {}s)",
                count,
                buffer_seconds
            );
        }

        count
    }

    /// 清除所有限流记录 (乐观重置策略)
    ///
    /// 用于乐观重置机制,当所有账号都被限流但等待时间很短时,
    /// 清除所有限流记录以解决时序竞争条件
    /// 
    /// WARNING: This clears ALL records including long-term QUOTA_EXHAUSTED locks.
    /// Prefer clear_expired_with_buffer() for safer optimistic reset.
    #[allow(dead_code)]
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
    fn test_tpm_exhausted_is_rate_limit_exceeded() {
        let tracker = RateLimitTracker::new();
        // 模拟真实世界的 TPM 错误，同时包含 "Resource exhausted" 和 "per minute"
        let body = "Resource has been exhausted (e.g. check quota). Quota limit 'Tokens per minute' exceeded.";
        let reason = tracker.parse_rate_limit_reason(body);
        // 应该被识别为 RateLimitExceeded，而不是 QuotaExhausted
        assert_eq!(reason, RateLimitReason::RateLimitExceeded);
    }
}
