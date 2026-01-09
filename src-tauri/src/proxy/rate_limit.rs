use dashmap::DashMap;
use std::time::{SystemTime, Duration};
use regex::Regex;

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
    pub reason: RateLimitReason,
}

/// 限流跟踪器
pub struct RateLimitTracker {
    limits: DashMap<String, RateLimitInfo>,
    /// 连续失败计数（用于智能指数退避）
    failure_counts: DashMap<String, u32>,
}

impl RateLimitTracker {
    pub fn new() -> Self {
        Self {
            limits: DashMap::new(),
            failure_counts: DashMap::new(),
        }
    }
    
    /// 获取账号剩余的等待时间(秒)
    pub fn get_remaining_wait(&self, account_id: &str) -> u64 {
        if let Some(info) = self.limits.get(account_id) {
            let now = SystemTime::now();
            if info.reset_time > now {
                return info.reset_time.duration_since(now).unwrap_or(Duration::from_secs(0)).as_secs();
            }
        }
        0
    }
    
    /// 标记账号请求成功，重置连续失败计数
    /// 
    /// 当账号成功完成请求后调用此方法，将其失败计数归零，
    /// 这样下次失败时会从最短的锁定时间（60秒）开始。
    pub fn mark_success(&self, account_id: &str) {
        if self.failure_counts.remove(account_id).is_some() {
            tracing::debug!("账号 {} 请求成功，已重置失败计数", account_id);
        }
        // 同时清除限流记录（如果有）
        self.limits.remove(account_id);
    }
    
    /// 精确锁定账号到指定时间点
    /// 
    /// 使用账号配额中的 reset_time 来精确锁定账号，
    /// 这比指数退避更加精准。
    pub fn set_lockout_until(&self, account_id: &str, reset_time: SystemTime, reason: RateLimitReason) {
        let now = SystemTime::now();
        let retry_sec = reset_time
            .duration_since(now)
            .map(|d| d.as_secs())
            .unwrap_or(60); // 如果时间已过，使用默认 60 秒
        
        let info = RateLimitInfo {
            reset_time,
            retry_after_sec: retry_sec,
            detected_at: now,
            reason,
        };
        
        self.limits.insert(account_id.to_string(), info);
        
        tracing::info!(
            "账号 {} 已精确锁定到配额刷新时间，剩余 {} 秒",
            account_id,
            retry_sec
        );
    }
    
    /// 使用 ISO 8601 时间字符串精确锁定账号
    /// 
    /// 解析类似 "2026-01-08T17:00:00Z" 格式的时间字符串
    pub fn set_lockout_until_iso(&self, account_id: &str, reset_time_str: &str, reason: RateLimitReason) -> bool {
        // 尝试解析 ISO 8601 格式
        match chrono::DateTime::parse_from_rfc3339(reset_time_str) {
            Ok(dt) => {
                let reset_time = SystemTime::UNIX_EPOCH + 
                    std::time::Duration::from_secs(dt.timestamp() as u64);
                self.set_lockout_until(account_id, reset_time, reason);
                true
            },
            Err(e) => {
                tracing::warn!(
                    "无法解析配额刷新时间 '{}': {}，将使用默认退避策略",
                    reset_time_str, e
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
                // 引入 PR #28 的安全缓冲区：最小 2 秒，防止极高频无效重试
                if s < 2 { 2 } else { s }
            },
            None => {
                // 获取连续失败次数，用于指数退避
                let failure_count = {
                    let mut count = self.failure_counts.entry(account_id.to_string()).or_insert(0);
                    *count += 1;
                    *count
                };
                
                match reason {
                    RateLimitReason::QuotaExhausted => {
                        // [智能限流] 根据连续失败次数动态调整锁定时间
                        // 第1次: 60s, 第2次: 5min, 第3次: 30min, 第4次+: 2h
                        let lockout = match failure_count {
                            1 => {
                                tracing::warn!("检测到配额耗尽 (QUOTA_EXHAUSTED)，第1次失败，锁定 60秒");
                                60
                            },
                            2 => {
                                tracing::warn!("检测到配额耗尽 (QUOTA_EXHAUSTED)，第2次连续失败，锁定 5分钟");
                                300
                            },
                            3 => {
                                tracing::warn!("检测到配额耗尽 (QUOTA_EXHAUSTED)，第3次连续失败，锁定 30分钟");
                                1800
                            },
                            _ => {
                                tracing::warn!("检测到配额耗尽 (QUOTA_EXHAUSTED)，第{}次连续失败，锁定 2小时", failure_count);
                                7200
                            }
                        };
                        lockout
                    },
                    RateLimitReason::RateLimitExceeded => {
                        // 速率限制：通常是短暂的，使用较短的默认值（30秒）
                        tracing::debug!("检测到速率限制 (RATE_LIMIT_EXCEEDED)，使用默认值 30秒");
                        30
                    },
                    RateLimitReason::ModelCapacityExhausted => {
                        // 模型容量耗尽：服务端暂时无可用 GPU 实例
                        // 这是临时性问题，使用较短的重试时间（15秒）
                        tracing::warn!("检测到模型容量不足 (MODEL_CAPACITY_EXHAUSTED)，服务端暂无可用实例，15秒后重试");
                        15
                    },
                    RateLimitReason::ServerError => {
                        // 服务器错误：执行"软避让"，默认锁定 20 秒
                        tracing::warn!("检测到 5xx 错误 ({}), 执行 20s 软避让...", status);
                        20
                    },
                    RateLimitReason::Unknown => {
                        // 未知原因：使用中等默认值（60秒）
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
        };
        
        // 存储
        self.limits.insert(account_id.to_string(), info.clone());
        
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
                if let Some(reason_str) = json.get("error")
                    .and_then(|e| e.get("details"))
                    .and_then(|d| d.as_array())
                    .and_then(|a| a.get(0))
                    .and_then(|o| o.get("reason"))
                    .and_then(|v| v.as_str()) {
                    
                    return match reason_str {
                        "QUOTA_EXHAUSTED" => RateLimitReason::QuotaExhausted,
                        "RATE_LIMIT_EXCEEDED" => RateLimitReason::RateLimitExceeded,
                        "MODEL_CAPACITY_EXHAUSTED" => RateLimitReason::ModelCapacityExhausted,
                        _ => RateLimitReason::Unknown,
                    };
                }
                // [NEW] 尝试从 message 字段进行文本匹配（防止 missed reason）
                 if let Some(msg) = json.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|v| v.as_str()) {
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
        if body_lower.contains("per minute") || body_lower.contains("rate limit") || body_lower.contains("too many requests") {
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
        // 支持格式："2h1m1s", "1h30m", "5m", "30s", "500ms" 等
        let re = Regex::new(r"(?:(\d+)h)?(?:(\d+)m)?(?:(\d+(?:\.\d+)?)s)?(?:(\d+)ms)?").ok()?;
        let caps = match re.captures(s) {
            Some(c) => c,
            None => {
                tracing::warn!("[时间解析] 正则未匹配: '{}'", s);
                return None;
            }
        };
        
        let hours = caps.get(1)
            .and_then(|m| m.as_str().parse::<u64>().ok())
            .unwrap_or(0);
        let minutes = caps.get(2)
            .and_then(|m| m.as_str().parse::<u64>().ok())
            .unwrap_or(0);
        let seconds = caps.get(3)
            .and_then(|m| m.as_str().parse::<f64>().ok())
            .unwrap_or(0.0);
        let milliseconds = caps.get(4)
            .and_then(|m| m.as_str().parse::<u64>().ok())
            .unwrap_or(0);
        
        tracing::debug!("[时间解析] 提取结果: {}h {}m {:.3}s {}ms", hours, minutes, seconds, milliseconds);
        
        // 计算总秒数
        let total_seconds = hours * 3600 + minutes * 60 + seconds.ceil() as u64 + (milliseconds + 999) / 1000;
        
        // 如果总秒数为 0，说明解析失败
        if total_seconds == 0 {
            tracing::warn!("[时间解析] 失败: '{}' (总秒数为0)", s);
            None
        } else {
            tracing::info!("[时间解析] ✓ 成功: '{}' => {}秒 ({}h {}m {:.1}s)", 
                s, total_seconds, hours, minutes, seconds);
            Some(total_seconds)
        }
    }
    
    /// 从错误消息 body 中解析重置时间
    fn parse_retry_time_from_body(&self, body: &str) -> Option<u64> {
        // A. 优先尝试 JSON 精准解析 (借鉴 PR #28)
        let trimmed = body.trim();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
                // 1. Google 常见的 quotaResetDelay 格式 (支持所有格式："2h1m1s", "1h30m", "42s", "500ms" 等)
                // 路径: error.details[0].metadata.quotaResetDelay
                if let Some(delay_str) = json.get("error")
                    .and_then(|e| e.get("details"))
                    .and_then(|d| d.as_array())
                    .and_then(|a| a.get(0))
                    .and_then(|o| o.get("metadata"))  // 添加 metadata 层级
                    .and_then(|m| m.get("quotaResetDelay"))
                    .and_then(|v| v.as_str()) {
                    
                    tracing::debug!("[JSON解析] 找到 quotaResetDelay: '{}'", delay_str);
                    
                    // 使用通用时间解析函数
                    if let Some(seconds) = self.parse_duration_string(delay_str) {
                        return Some(seconds);
                    }
                }
                
                // 2. OpenAI 常见的 retry_after 字段 (数字)
                if let Some(retry) = json.get("error")
                    .and_then(|e| e.get("retry_after"))
                    .and_then(|v| v.as_u64()) {
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
    pub fn is_rate_limited(&self, account_id: &str) -> bool {
        if let Some(info) = self.get(account_id) {
            info.reset_time > SystemTime::now()
        } else {
            false
        }
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
    #[allow(dead_code)]
    pub fn clear(&self, account_id: &str) -> bool {
        self.limits.remove(account_id).is_some()
    }
    
    /// 清除所有限流记录
    #[allow(dead_code)]
    pub fn clear_all(&self) {
        let count = self.limits.len();
        self.limits.clear();
        tracing::debug!("清除了所有 {} 条限流记录", count);
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
        tracker.parse_from_error("acc1", 429, Some("30"), "");
        let wait = tracker.get_remaining_wait("acc1");
        assert!(wait > 25 && wait <= 30);
    }

    #[test]
    fn test_safety_buffer() {
        let tracker = RateLimitTracker::new();
        // 如果 API 返回 1s，我们强制设为 2s
        tracker.parse_from_error("acc1", 429, Some("1"), "");
        let wait = tracker.get_remaining_wait("acc1");
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
