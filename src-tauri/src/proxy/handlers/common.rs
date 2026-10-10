use crate::proxy::server::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::collections::HashSet;
use tokio::time::{sleep, Duration};
use tracing::{debug, info};

// ===== 统一重试与退避策略 =====

/// 重试策略枚举
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryStrategy {
    /// 不重试，直接返回错误
    NoRetry,
    /// 固定延迟
    FixedDelay(Duration),
    /// 线性退避：base_ms * (attempt + 1)
    LinearBackoff { base_ms: u64 },
    /// 指数退避：base_ms * 2^attempt，上限 max_ms
    ExponentialBackoff { base_ms: u64, max_ms: u64 },
    /// [NEW] 原地重试 (Grace Retry)：在当前账号上小窗口等待后直接重试，不计入常规切换
    GraceRetry(Duration),
}

#[derive(Debug, Default)]
pub struct RequestRetryState {
    grace_retried_accounts: HashSet<String>,
}

impl RequestRetryState {
    pub fn determine_strategy(
        &mut self,
        account_id: &str,
        status_code: u16,
        error_text: &str,
        retry_after: Option<&str>,
        retried_without_thinking: bool,
    ) -> RetryStrategy {
        self.determine_strategy_with_grace(
            account_id,
            status_code,
            error_text,
            retry_after,
            retried_without_thinking,
            true,
        )
    }

    pub fn determine_strategy_with_grace(
        &mut self,
        account_id: &str,
        status_code: u16,
        error_text: &str,
        retry_after: Option<&str>,
        retried_without_thinking: bool,
        allow_grace: bool,
    ) -> RetryStrategy {
        self.determine_strategy_adaptive(
            account_id,
            status_code,
            error_text,
            retry_after,
            retried_without_thinking,
            0,
            if allow_grace { 1 } else { 2 },
        )
    }

    /// [NEW] 自适应感知多账号池轮次与配额重置间隙的重试裁决器
    pub fn determine_strategy_adaptive(
        &mut self,
        account_id: &str,
        status_code: u16,
        error_text: &str,
        retry_after: Option<&str>,
        retried_without_thinking: bool,
        attempt: usize,
        pool_size: usize,
    ) -> RetryStrategy {
        let allow_grace_retry = !self.grace_retried_accounts.contains(account_id);
        let strategy = determine_retry_strategy_adaptive(
            status_code,
            error_text,
            retry_after,
            retried_without_thinking,
            allow_grace_retry,
            attempt,
            pool_size,
        );
        if matches!(strategy, RetryStrategy::GraceRetry(_)) {
            self.grace_retried_accounts.insert(account_id.to_string());
        }
        strategy
    }
}

pub fn next_rotation_attempt(
    used_attempts: &mut usize,
    max_attempts: usize,
    retry_same_account: bool,
) -> Option<usize> {
    if retry_same_account {
        return used_attempts.checked_sub(1);
    }
    if *used_attempts >= max_attempts {
        return None;
    }

    let attempt = *used_attempts;
    *used_attempts += 1;
    Some(attempt)
}

#[derive(Debug, Default)]
pub struct FailureStatusTracker {
    saw_failure: bool,
    last_non_rate_limit: Option<StatusCode>,
}

impl FailureStatusTracker {
    pub fn record(&mut self, status: StatusCode) {
        self.saw_failure = true;
        if status != StatusCode::TOO_MANY_REQUESTS {
            self.last_non_rate_limit = Some(status);
        }
    }

    pub fn final_status(&self) -> StatusCode {
        self.last_non_rate_limit.unwrap_or_else(|| {
            if self.saw_failure {
                StatusCode::TOO_MANY_REQUESTS
            } else {
                StatusCode::BAD_GATEWAY
            }
        })
    }
}

/// 自适应计算最大重试次数
/// - 单账号 (pool_size <= 1): 允许初次请求 + 2次基于 quotaResetDelay 的退避重试 (共 3 次)，绝不 50ms 刷死
/// - 多账号 (pool_size > 1): 保证整池账号至少能完整轮换 2 轮 (Round 1 闪电快切 + Round 2 退避小等重试)
///   最小 4 次，上限 12 次 (既能把 2~6 个账号试满 2 轮，又防止超长等待导致客户端超时)
pub fn calculate_max_retry_attempts(pool_size: usize) -> usize {
    if pool_size <= 1 {
        3
    } else {
        (pool_size * 2).clamp(4, 12)
    }
}

/// 根据错误状态码和错误信息确定重试策略
pub fn determine_retry_strategy(
    status_code: u16,
    error_text: &str,
    retried_without_thinking: bool,
) -> RetryStrategy {
    determine_retry_strategy_with_grace(status_code, error_text, retried_without_thinking, true)
}

pub fn determine_retry_strategy_with_grace(
    status_code: u16,
    error_text: &str,
    retried_without_thinking: bool,
    allow_grace: bool,
) -> RetryStrategy {
    determine_retry_strategy_adaptive(
        status_code,
        error_text,
        None,
        retried_without_thinking,
        allow_grace,
        0,
        if allow_grace { 1 } else { 2 },
    )
}

/// [NEW] 自适应多账号池与单账号退避裁决核心逻辑
pub fn determine_retry_strategy_adaptive(
    status_code: u16,
    error_text: &str,
    retry_after: Option<&str>,
    retried_without_thinking: bool,
    allow_grace_retry: bool,
    attempt: usize,
    pool_size: usize,
) -> RetryStrategy {
    // [DEFENSE] 若错误已明确为模型不存在，绝对禁止无效轮换或退避重试！
    if is_model_not_found_error(status_code, error_text) {
        return RetryStrategy::NoRetry;
    }

    let lower = error_text.to_lowercase();
    match status_code {
        // 400 错误：仅在特定 Thinking 签名失败时重试一次
        400 if !retried_without_thinking
            && (lower.contains("invalid thought signature")
                || lower.contains("invalid `signature`")
                || lower.contains("invalid signature")
                || lower.contains("thought_signature")
                || lower.contains("thoughtsignature")
                || lower.contains("thinking.signature")
                || lower.contains("thinking.thinking")
                || lower.contains("corrupted thought signature")) =>
        {
            RetryStrategy::FixedDelay(Duration::from_millis(200))
        }

        // 429 限流错误
        429 => {
            // 1. 优先尝试解析结构化或自然语言的重试延迟 (quotaResetDelay / Retry-After)
            let parsed_delay = crate::proxy::upstream::retry::parse_retry_delay_with_source(
                error_text,
                retry_after,
            );

            // 2. 单账号模式 (pool_size <= 1)：无法切号，若未曾 GraceRetry 则等待，否则降级为固定退避或放弃
            // 【核心防线】单账号判定必须严格位于任何快切或硬配额判定之前，严禁 50ms 闪电空转刷死
            if pool_size <= 1 {
                if let Some(delay) = parsed_delay {
                    let actual_ms = delay.actual_wait_ms();
                    if actual_ms <= 30_000 && allow_grace_retry {
                        tracing::info!(
                            "[Retry] Single account 429: quotaResetDelay detected ({}ms), applying GraceRetry",
                            actual_ms
                        );
                        return RetryStrategy::GraceRetry(Duration::from_millis(actual_ms));
                    } else {
                        return RetryStrategy::FixedDelay(Duration::from_millis(
                            actual_ms.min(30_000),
                        ));
                    }
                } else if allow_grace_retry {
                    // 没有给出明确延迟时的保底退避 (单账号等待 3s~10s，杜绝 50ms 闪电耗尽重试)
                    let backoff_ms = (3000 * (attempt + 1) as u64).min(10_000);
                    tracing::info!(
                        "[Retry] Single account 429 without explicit delay: backing off {}ms",
                        backoff_ms
                    );
                    return RetryStrategy::GraceRetry(Duration::from_millis(backoff_ms));
                } else {
                    let backoff_ms = (3000 * (attempt + 1) as u64).min(10_000);
                    return RetryStrategy::FixedDelay(Duration::from_millis(backoff_ms));
                }
            }

            // 3. 多账号模式 (pool_size > 1)：
            // 账号级硬配额枯竭检测：仅当没有提供重试延迟且包含确定性枯竭关键字时判定
            // 包含明确的账号额度/周期枯竭字样（如 exceeded your current quota / insufficient_quota / weekly quota / credits 等）
            // 注意：Google 标准通用 429 的 "resource has been exhausted (e.g. check quota)" 属于无明确延迟的通用流控/TPM拒绝，
            // 严禁归入硬配额，否则将导致全池快速轮换遍历并引发全池 30 秒级联锁定 (#3506)。
            let is_hard_quota_exhausted = parsed_delay.is_none()
                && (lower.contains("exceeded your current quota")
                    || lower.contains("insufficient_quota")
                    || lower.contains("credits")
                    || lower.contains("zero_quota")
                    || lower.contains("weekly quota")
                    || lower.contains("daily quota")
                    || lower.contains("per day")
                    || (lower.contains("quota_exhausted")
                        && !lower.contains("resource has been exhausted")));

            // 请求级 429 防穿透保护 (无明确重置时间且非账号硬配额耗尽)：
            // [FIX #3506] 若遭遇无明确重置时间的请求级 429，最多允许尝试 2 个账号快切逃逸（min(pool_size, 2)）。
            // 连续 2 个账号失败说明该请求为恶性 Payload、超大 Token 或 IP/提供商级流控，
            // 必须立即终止进一步轮换，严禁打穿全池导致全池健康账号被锁入 RateLimitExceeded。
            let is_request_level_429 = parsed_delay.is_none() && !is_hard_quota_exhausted;
            let request_level_429_max_attempts = pool_size.min(2);
            if is_request_level_429 && attempt >= request_level_429_max_attempts {
                tracing::warn!(
                    "[Retry] Request-level 429 persisted across {} attempts without explicit delay; aborting further rotation to protect remaining {} accounts.",
                    attempt,
                    pool_size.saturating_sub(attempt)
                );
                return RetryStrategy::NoRetry;
            }

            let is_first_round = attempt < pool_size;
            if is_first_round {
                // Round 1 (第一轮)：全池闪电快切，毫秒级逃逸至其他健康账号
                tracing::info!(
                    "[Retry] Multi-account Round 1 (attempt {}/{}): fast rotating to next account in pool (50ms)",
                    attempt + 1, pool_size
                );
                return RetryStrategy::FixedDelay(Duration::from_millis(50));
            }

            // Round 2 (第二轮)：全池在第一轮已全部遭遇 429，进入智能退避与小间隙等待阶段
            if let Some(delay) = parsed_delay {
                let actual_ms = delay.actual_wait_ms();
                // 遇到小间隙 (<= 5s) 时，在当前账号原地小等并重试
                if actual_ms <= 5000 && allow_grace_retry {
                    tracing::info!(
                        "[Retry] Multi-account Round 2 (attempt {}): small reset gap ({}ms <= 5s), applying GraceRetry",
                        attempt + 1, actual_ms
                    );
                    return RetryStrategy::GraceRetry(Duration::from_millis(actual_ms));
                }

                // 延迟较大 (> 5s，如 10s 以上) 且可用账号数 > 2：优先继续闪电轮换寻找其他可能到期的账号
                if pool_size > 2 && attempt + 1 < pool_size * 2 {
                    tracing::info!(
                        "[Retry] Multi-account Round 2 (attempt {}): delay is {}ms (> 5s) with pool_size > 2, fast rotating",
                        attempt + 1, actual_ms
                    );
                    return RetryStrategy::FixedDelay(Duration::from_millis(50));
                }

                // 其余情况 (例如仅 2 账号或第二轮后半段)：等待该延迟 (上限 12s，防止客户端超时)
                let capped_ms = actual_ms.min(12_000);
                tracing::info!(
                    "[Retry] Multi-account Round 2: waiting capped delay {}ms before retry",
                    capped_ms
                );
                return RetryStrategy::FixedDelay(Duration::from_millis(capped_ms));
            }

            // 第二轮未解析出具体 delay：线性温和退避 2s~4s，让瞬时并发高峰平息
            let backoff_ms = (2000 * ((attempt.saturating_sub(pool_size)) + 1) as u64).min(5000);
            RetryStrategy::FixedDelay(Duration::from_millis(backoff_ms))
        }

        // 503 服务不可用 / 529 服务器过载
        503 | 529 => {
            if pool_size > 1 && attempt < pool_size {
                // 多账号第一轮遭遇 503/529: 快速切向其他健康账号 (50ms)
                tracing::info!(
                    "[Retry] 503/529 detected in multi-account pool (attempt {}/{}): fast escaping to other accounts",
                    attempt + 1, pool_size
                );
                RetryStrategy::FixedDelay(Duration::from_millis(50))
            } else {
                // 单账号或已遍历全池: 指数退避 (起始 5s，上限 30s)
                RetryStrategy::ExponentialBackoff {
                    base_ms: 5000,
                    max_ms: 30000,
                }
            }
        }

        // 500 服务器内部错误
        500 => {
            // 线性退避：起始 3s
            RetryStrategy::LinearBackoff { base_ms: 3000 }
        }

        // 401/403 认证/权限错误：切换账号前给予极短缓冲
        401 | 403 => RetryStrategy::FixedDelay(Duration::from_millis(200)),

        // 404 资源未找到：Google Cloud Code API 的 404 通常是账号级别的间歇性问题
        // (灰度发布、账号权限不同步等)，轮换账号往往能解决
        404 => RetryStrategy::FixedDelay(Duration::from_millis(300)),

        // 其他错误：不重试
        _ => RetryStrategy::NoRetry,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_short_429_preserves_rotation_budget_and_structured_status() {
        let body = r#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"1s"}]}}"#;

        let drive_failures = |account_count| {
            let mut state = RequestRetryState::default();
            let mut used_attempts = 0;
            let mut retry_same_account = false;
            let mut sends = Vec::new();

            while let Some(attempt) =
                next_rotation_attempt(&mut used_attempts, account_count, retry_same_account)
            {
                retry_same_account = false;
                sends.push(attempt);
                let account_id = format!("account-{}", attempt);
                let strategy = state.determine_strategy(&account_id, 429, body, None, false);
                if matches!(strategy, RetryStrategy::GraceRetry(_)) {
                    assert!(!should_rotate_account(429, Some(&strategy)));
                    retry_same_account = true;
                } else {
                    assert!(should_rotate_account(429, Some(&strategy)));
                }
            }
            sends
        };

        assert_eq!(drive_failures(1), vec![0, 0]);
        assert_eq!(drive_failures(2), vec![0, 0, 1, 1]);

        let mut all_429 = FailureStatusTracker::default();
        all_429.record(StatusCode::TOO_MANY_REQUESTS);
        all_429.record(StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(all_429.final_status(), StatusCode::TOO_MANY_REQUESTS);

        for non_429 in [StatusCode::FORBIDDEN, StatusCode::SERVICE_UNAVAILABLE] {
            let mut mixed = FailureStatusTracker::default();
            mixed.record(non_429);
            mixed.record(StatusCode::TOO_MANY_REQUESTS);
            assert_eq!(mixed.final_status(), non_429);
        }

        let mut all_503 = FailureStatusTracker::default();
        all_503.record(StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(all_503.final_status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}

/// 执行退避策略并返回是否应该继续重试
pub async fn apply_retry_strategy(
    strategy: RetryStrategy,
    attempt: usize,
    max_attempts: usize,
    status_code: u16,
    trace_id: &str,
) -> bool {
    match strategy {
        RetryStrategy::NoRetry => {
            debug!(
                "[{}] Non-retryable error {}, stopping",
                trace_id, status_code
            );
            false
        }

        RetryStrategy::FixedDelay(duration) => {
            let base_ms = duration.as_millis() as u64;
            info!(
                "[{}] ⏱️ Retry with fixed delay: status={}, attempt={}/{}, delay={}ms",
                trace_id,
                status_code,
                attempt + 1,
                max_attempts,
                base_ms
            );
            sleep(duration).await;
            true
        }

        RetryStrategy::LinearBackoff { base_ms } => {
            let calculated_ms = base_ms * (attempt as u64 + 1);
            info!(
                "[{}] ⏱️ Retry with linear backoff: status={}, attempt={}/{}, delay={}ms",
                trace_id,
                status_code,
                attempt + 1,
                max_attempts,
                calculated_ms
            );
            sleep(Duration::from_millis(calculated_ms)).await;
            true
        }

        RetryStrategy::ExponentialBackoff { base_ms, max_ms } => {
            let calculated_ms = (base_ms * 2_u64.pow(attempt as u32)).min(max_ms);
            info!(
                "[{}] ⏱️ Retry with exponential backoff: status={}, attempt={}/{}, delay={}ms",
                trace_id,
                status_code,
                attempt + 1,
                max_attempts,
                calculated_ms
            );
            sleep(Duration::from_millis(calculated_ms)).await;
            true
        }

        RetryStrategy::GraceRetry(duration) => {
            info!(
                "[{}] ⚡ Grace Retry: Performing micro-wait ({}ms) on current account...",
                trace_id,
                duration.as_millis()
            );
            sleep(duration).await;
            true // 原地重试在 handlers 层面通过 should_rotate_account 判断是否切换
        }
    }
}

/// 判断是否应该轮换账号
pub fn should_rotate_account(status_code: u16, strategy: Option<&RetryStrategy>) -> bool {
    // [NEW] 如果识别为 Grace Retry，则显式要求不轮换账号
    if let Some(RetryStrategy::GraceRetry(_)) = strategy {
        return false;
    }

    match status_code {
        // 这些错误是账号级别或特定节点配额的，需要轮换
        // [FIX #3485] 503/529 边缘节点熔断或特定账号负载过高，多账号时支持轮换逃逸，杜绝死锁死等
        429 | 401 | 403 | 404 | 500 | 503 | 529 => true,
        _ => false,
    }
}

/// 判断是否为模型不存在/不支持的错误
pub fn is_model_not_found_error(status: u16, body: &str) -> bool {
    if status == 404 {
        return true;
    }
    let lower = body.to_lowercase();
    lower.contains("model not found")
        || lower.contains("unknown model")
        || lower.contains("does not exist")
        || lower.contains("is not found")
        || lower.contains("unsupported model")
        || lower.contains("not found for api version")
        || lower.contains("publisher model")
        || lower.contains("model_not_found")
        || lower.contains("no such model")
        || lower.contains("invalid model")
        || lower.contains("model is not available")
}

/// Detects model capabilities and configuration
/// POST /v1/models/detect
pub async fn handle_detect_model(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    let model_name = body.get("model").and_then(|v| v.as_str()).unwrap_or("");

    if model_name.is_empty() {
        return (StatusCode::BAD_REQUEST, "Missing 'model' field").into_response();
    }

    // 1. Resolve mapping
    let mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
        model_name,
        &*state.custom_mapping.read().await,
    );

    // 2. Resolve capabilities
    let config = crate::proxy::mappers::common_utils::resolve_request_config(
        model_name,
        &mapped_model,
        &None, // We don't check tools for static capability detection
        None,  // size
        None,  // quality
        None,  // image_size
        None,  // body (not needed for static detection)
    );

    // 3. Construct response
    let mut response = json!({
        "model": model_name,
        "mapped_model": mapped_model,
        "type": config.request_type,
        "features": {
            "has_web_search": config.inject_google_search,
            "is_image_gen": config.request_type == "image_gen"
        }
    });

    if let Some(img_conf) = config.image_config {
        if let Some(obj) = response.as_object_mut() {
            obj.insert("config".to_string(), img_conf);
        }
    }

    Json(response).into_response()
}

/// [Issue #3414] 从形如 "All accounts limited. Wait 29s." 或其他明确冷却提示中解析等待秒数
pub fn extract_retry_after_seconds(error_text: &str) -> Option<u64> {
    if let Some(pos) = error_text.find("Wait ") {
        let rest = &error_text[pos + 5..];
        if let Some(s_pos) = rest.find('s') {
            if let Ok(sec) = rest[..s_pos].trim().parse::<u64>() {
                if sec > 0 {
                    return Some(sec);
                }
            }
        }
    }
    None
}

/// [Issue #3414] 统一构造带有 X-Mapped-Model、可选 X-Account-Email 以及 Retry-After 的 HeaderMap
pub fn build_token_error_headers<'a>(
    mapped_model: Option<&'a str>,
    account_email: Option<&'a str>,
    error_text: &str,
) -> axum::http::HeaderMap {
    use axum::http::header::{HeaderName, HeaderValue};
    let mut headers = axum::http::HeaderMap::new();

    if let Some(model) = mapped_model {
        if let Ok(val) = HeaderValue::from_str(model) {
            headers.insert(HeaderName::from_static("x-mapped-model"), val);
        }
    }
    if let Some(email) = account_email {
        if let Ok(val) = HeaderValue::from_str(email) {
            headers.insert(HeaderName::from_static("x-account-email"), val);
        }
    }
    if let Some(sec) = extract_retry_after_seconds(error_text) {
        if let Ok(val) = HeaderValue::from_str(&sec.to_string()) {
            headers.insert(axum::http::header::RETRY_AFTER, val);
        }
    }
    headers
}

/// 判定是否属于偶发性/瞬态 Token 获取错误（例如超时、锁争抢、系统繁忙）
pub fn is_transient_token_error(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("timeout") || lower.contains("too busy") || lower.contains("deadlock")
}


/// 深度解析、剥离前缀与反转义上游错误，返回 (上游原始纯文本消息, 结构化解析对象)
pub fn parse_raw_upstream_error(error_text: &str) -> (String, serde_json::Value) {
    let trimmed = error_text.trim();
    // 1. 剥离可能附带的外层 HTTP 前缀，如 "HTTP 400: "、"HTTP 500: "、"All accounts exhausted. Last error: "
    let clean_str = if let Some(pos) = trimmed.find("HTTP ") {
        if let Some(colon_pos) = trimmed[pos..].find(": ") {
            trimmed[pos + colon_pos + 2..].trim()
        } else {
            trimmed
        }
    } else if let Some(stripped) = trimmed.strip_prefix("All accounts exhausted. Last error: ") {
        stripped.trim()
    } else {
        trimmed
    };

    // 2. 深度递归反转义解析 JSON
    let mut current_val: Option<serde_json::Value> = serde_json::from_str(clean_str).ok();
    // 如果解析出来的还是 string 且看起来像 JSON，尝试二次/三次解析（处理被双重转义的情况）
    for _ in 0..3 {
        if let Some(serde_json::Value::String(ref s)) = current_val {
            let s_trim = s.trim();
            if (s_trim.starts_with('{') && s_trim.ends_with('}'))
                || (s_trim.starts_with('[') && s_trim.ends_with(']'))
            {
                if let Ok(nested) = serde_json::from_str(s_trim) {
                    current_val = Some(nested);
                    continue;
                }
            }
        }
        break;
    }

    // 3. 如果当前值是个 Object，且它某个字段（例如 "response" 或 "raw"）又是嵌套 JSON 字符串，递归解析内部
    if let Some(mut parsed) = current_val {
        if let Some(obj) = parsed.as_object_mut() {
            for key in ["response", "raw", "error", "details", "message"] {
                if let Some(serde_json::Value::String(s)) = obj.get(key) {
                    let s_trim = s.trim();
                    if (s_trim.starts_with('{') && s_trim.ends_with('}'))
                        || (s_trim.starts_with('[') && s_trim.ends_with(']'))
                    {
                        if let Ok(nested) = serde_json::from_str::<serde_json::Value>(s_trim) {
                            obj.insert(key.to_string(), nested);
                        }
                    }
                }
            }
        }

        // 从结构化对象中提取原生的 message
        let raw_msg = if let Some(err_obj) = parsed.get("error") {
            if let Some(msg) = err_obj.get("message").and_then(|m| m.as_str()) {
                msg.to_string()
            } else if let Some(msg) = err_obj.as_str() {
                msg.to_string()
            } else {
                clean_str.to_string()
            }
        } else if let Some(msg) = parsed.get("message").and_then(|m| m.as_str()) {
            msg.to_string()
        } else if let Some(msg) = parsed.get("detail").and_then(|m| m.as_str()) {
            msg.to_string()
        } else {
            clean_str.to_string()
        };

        (raw_msg, parsed)
    } else {
        (
            clean_str.to_string(),
            serde_json::json!({ "raw": clean_str }),
        )
    }
}

fn map_status_code_to_gemini_status(status_code: u16) -> &'static str {
    match status_code {
        400 => "INVALID_ARGUMENT",
        401 => "UNAUTHENTICATED",
        403 => "PERMISSION_DENIED",
        404 => "NOT_FOUND",
        429 => "RESOURCE_EXHAUSTED",
        499 => "CANCELLED",
        500 => "INTERNAL",
        501 => "NOT_IMPLEMENTED",
        503 => "UNAVAILABLE",
        504 => "DEADLINE_EXCEEDED",
        _ => "UPSTREAM_ERROR",
    }
}

/// 格式化双轨制错误报文（四大标准协议原始字段散开 + 顶层/UI层不阉割双轨诊断）
pub fn build_dual_track_error(
    protocol: &str, // "claude", "openai", "responses", or "gemini"
    status_code: u16,
    model: &str,
    error_text: &str,
) -> serde_json::Value {
    let lower = error_text.to_lowercase();
    let is_internal_limited = lower.contains("all accounts limited")
        || lower.contains("no accounts available")
        || lower.contains("all accounts failed")
        || lower.contains("token pool is empty")
        || lower.contains("all accounts exhausted")
        || lower.contains("all accounts unhealthy");

    let is_network_error = lower.contains("http request failed")
        || lower.contains("failed to connect")
        || lower.contains("connection refused")
        || lower.contains("connection reset")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("error trying to connect")
        || lower.contains("dns error")
        || lower.contains("all endpoints failed");

    let is_not_found = is_model_not_found_error(status_code, error_text);

    // 1. 深度解析并反转义上游错误
    let (raw_upstream_msg, parsed_upstream) = parse_raw_upstream_error(error_text);

    let (readable_prefix, diagnosis, suggestion, err_type, err_code) = if is_internal_limited {
        (
            "【网关调度受限】".to_string(),
            format!(
                "网关本地账号池当前暂无可用账号或全部可用账号处于限流冷却中。调度详情: {}",
                error_text
            ),
            "请等待冷却结束（参考等待秒数），或在网关中添加更多正常账号。".to_string(),
            "rate_limit_error",
            "all_accounts_limited",
        )
    } else if is_network_error {
        (
            "【网络请求异常（非服务端故障）】".to_string(),
            format!(
                "请求上游接口时网络连接失败（非网关服务端故障，请排查网络或代理）：{}",
                error_text
            ),
            "请检查系统代理、上游节点连通性或本地网络设置，确保能正常连接上游服务。".to_string(),
            "network_error",
            "upstream_network_error",
        )
    } else if is_not_found {
        (
            format!("【模型不存在】[{}]", model),
            format!("模型 [{}] 在上游端点不存在，或当前绑定的账号暂未开通该模型的访问权限。", model),
            "请核对模型名称，或在网关配置中的「自定义模型映射」将其重定向至可用模型（如 gemini-2.5-flash）。".to_string(),
            "invalid_request_error",
            "model_not_found",
        )
    } else if status_code == 429 || status_code == 529 {
        (
            format!("【上游限流 HTTP {}】", status_code),
            format!("模型 [{}] 触发上游配额耗尽或频率限制。", model),
            "请稍候自动恢复，或添加更多账号以分散并发请求。".to_string(),
            "rate_limit_error",
            "rate_limit_exceeded",
        )
    } else {
        (
            format!("【上游错误 HTTP {}】", status_code),
            format!("调用上游模型 [{}] 发生错误 (HTTP {})。", model, status_code),
            "请参考 upstream_error 中的详细字段排查原因。".to_string(),
            "api_error",
            "upstream_error",
        )
    };

    let readable_message = format!(
        "{} 网关诊断: {} 建议: {}",
        readable_prefix, diagnosis, suggestion
    );

    // 2. 确定散开到各自协议标准 message 字段的内容：
    //    若是网关自身限制或网络异常（无有效上游业务报文参与），则使用网关诊断文本；
    //    若是上游发生的真实报错，100% 保持上游原始报错（raw_upstream_msg），绝不被网关硬编码覆盖！
    let effective_message =
        if is_internal_limited || is_network_error || raw_upstream_msg.trim().is_empty() {
            readable_message.clone()
        } else {
            raw_upstream_msg
        };

    let gateway_error_obj = serde_json::json!({
        "error_code": err_code,
        "model": model,
        "is_server_error": false,
        "diagnosis": diagnosis,
        "suggestion": suggestion,
        "readable_summary": readable_message
    });

    let upstream_error_obj = serde_json::json!({
        "status": status_code,
        "is_server_error": false,
        "response": parsed_upstream
    });

    match protocol {
        "claude" => serde_json::json!({
            "type": "error",
            "error": {
                "type": err_type,
                "code": err_code,
                "message": effective_message,
                "is_server_error": false,
                "gateway_error": gateway_error_obj,
                "upstream_error": upstream_error_obj
            }
        }),
        "openai" => serde_json::json!({
            "error": {
                "message": effective_message,
                "type": err_type,
                "param": serde_json::Value::Null,
                "code": err_code,
                "is_server_error": false,
                "gateway_error": gateway_error_obj,
                "upstream_error": upstream_error_obj
            }
        }),
        "responses" => serde_json::json!({
            "error": {
                "message": effective_message,
                "type": err_type,
                "code": err_code,
                "is_server_error": false,
                "gateway_error": gateway_error_obj,
                "upstream_error": upstream_error_obj
            }
        }),
        "gemini" => {
            let upstream_status = parsed_upstream
                .get("error")
                .and_then(|e| e.get("status"))
                .and_then(|s| s.as_str())
                .unwrap_or_else(|| map_status_code_to_gemini_status(status_code));

            serde_json::json!({
                "error": {
                    "code": status_code,
                    "message": effective_message,
                    "status": upstream_status,
                    "is_server_error": false,
                    "gateway_error": gateway_error_obj,
                    "upstream_error": upstream_error_obj
                }
            })
        }
        _ => serde_json::json!({
            "error": {
                "message": effective_message,
                "type": err_type,
                "code": err_code,
                "is_server_error": false,
                "gateway_error": gateway_error_obj,
                "upstream_error": upstream_error_obj
            }
        }),
    }
}

#[cfg(test)]
mod retry_after_tests {
    use super::*;

    #[test]
    fn test_extract_retry_after_seconds() {
        assert_eq!(
            extract_retry_after_seconds("All accounts limited. Wait 29s."),
            Some(29)
        );
        assert_eq!(
            extract_retry_after_seconds("Token error: All accounts limited. Wait 5s."),
            Some(5)
        );
        assert_eq!(extract_retry_after_seconds("Token pool is empty"), None);
        assert_eq!(
            extract_retry_after_seconds("All accounts failed or unhealthy."),
            None
        );
    }

    #[test]
    fn test_build_token_error_headers() {
        let headers = build_token_error_headers(
            Some("gemini-2.5-pro"),
            Some("test@example.com"),
            "All accounts limited. Wait 45s.",
        );
        assert_eq!(
            headers.get("x-mapped-model").unwrap().to_str().unwrap(),
            "gemini-2.5-pro"
        );
        assert_eq!(
            headers.get("x-account-email").unwrap().to_str().unwrap(),
            "test@example.com"
        );
        assert_eq!(headers.get("retry-after").unwrap().to_str().unwrap(), "45");

        let headers_no_wait = build_token_error_headers(
            Some("gemini-2.5-pro"),
            None,
            "All accounts failed or unhealthy.",
        );
        assert!(headers_no_wait.get("retry-after").is_none());
        assert_eq!(
            headers_no_wait
                .get("x-mapped-model")
                .unwrap()
                .to_str()
                .unwrap(),
            "gemini-2.5-pro"
        );
    }

    #[test]
    fn test_parse_raw_upstream_error_with_http_prefix_and_escapes() {
        let raw = r#"HTTP 400: {"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT"}}"#;
        let (msg, parsed) = parse_raw_upstream_error(raw);
        assert_eq!(msg, "API key not valid. Please pass a valid API key.");
        assert_eq!(parsed["error"]["code"], 400);
        assert_eq!(parsed["error"]["status"], "INVALID_ARGUMENT");
    }

    #[test]
    fn test_parse_raw_upstream_error_double_escaped() {
        let raw = r#""{\"error\":{\"code\":403,\"message\":\"User location not supported.\"}}""#;
        let (msg, parsed) = parse_raw_upstream_error(raw);
        assert_eq!(msg, "User location not supported.");
        assert_eq!(parsed["error"]["code"], 403);
    }

    #[test]
    fn test_build_dual_track_error_four_protocols() {
        let raw_err = r#"HTTP 404: {"error":{"code":404,"message":"models/gemini-not-exist is not found","status":"NOT_FOUND"}}"#;

        // 1. Claude
        let claude_res = build_dual_track_error("claude", 404, "gemini-not-exist", raw_err);
        assert_eq!(claude_res["type"], "error");
        // 标准协议底层字段散开为上游原始 message
        assert_eq!(
            claude_res["error"]["message"],
            "models/gemini-not-exist is not found"
        );
        assert_eq!(claude_res["error"]["code"], "model_not_found");
        // UI层双轨制诊断不阉割
        assert!(claude_res["error"]["gateway_error"]["diagnosis"].is_string());
        assert_eq!(claude_res["error"]["upstream_error"]["status"], 404);
        assert_eq!(
            claude_res["error"]["upstream_error"]["response"]["error"]["status"],
            "NOT_FOUND"
        );

        // 2. OpenAI
        let openai_res = build_dual_track_error("openai", 404, "gemini-not-exist", raw_err);
        assert_eq!(
            openai_res["error"]["message"],
            "models/gemini-not-exist is not found"
        );
        assert_eq!(openai_res["error"]["code"], "model_not_found");
        assert!(openai_res["error"]["gateway_error"].is_object());
        assert!(openai_res["error"]["upstream_error"].is_object());

        // 3. Responses
        let resp_res = build_dual_track_error("responses", 404, "gemini-not-exist", raw_err);
        assert_eq!(
            resp_res["error"]["message"],
            "models/gemini-not-exist is not found"
        );
        assert_eq!(resp_res["error"]["code"], "model_not_found");
        assert!(resp_res["error"]["gateway_error"].is_object());

        // 4. Gemini
        let gemini_res = build_dual_track_error("gemini", 404, "gemini-not-exist", raw_err);
        assert_eq!(gemini_res["error"]["code"], 404);
        assert_eq!(
            gemini_res["error"]["message"],
            "models/gemini-not-exist is not found"
        );
        assert_eq!(gemini_res["error"]["status"], "NOT_FOUND");
        assert!(gemini_res["error"]["gateway_error"].is_object());
        assert!(gemini_res["error"]["upstream_error"].is_object());
    }

    #[test]
    fn test_build_dual_track_error_internal_limited() {
        let raw_err = "All accounts limited. Wait 30s.";
        let claude_res = build_dual_track_error("claude", 429, "gemini-2.5-flash", raw_err);
        assert_eq!(claude_res["error"]["code"], "all_accounts_limited");
        assert!(claude_res["error"]["message"]
            .as_str()
            .unwrap()
            .contains("网关调度受限"));
    }

    #[test]
    fn test_build_dual_track_error_network_error() {
        let raw_err =
            "HTTP request failed at https://alkalicognition.googleapis.com: failed to connect";
        let res = build_dual_track_error("claude", 502, "claude-sonnet-4-6", raw_err);
        assert_eq!(res["error"]["code"], "upstream_network_error");
        assert_eq!(res["error"]["gateway_error"]["is_server_error"], false);
        assert_eq!(res["error"]["is_server_error"], false);
        assert!(res["error"]["message"]
            .as_str()
            .unwrap()
            .contains("非服务端故障"));
    }

    #[test]
    fn test_is_transient_token_error() {
        assert!(is_transient_token_error(
            "Token acquisition timeout (15s) - system too busy or deadlock detected"
        ));
        assert!(is_transient_token_error(
            "Token acquisition timeout (5s) - system too busy or deadlock detected"
        ));
        assert!(is_transient_token_error("System too busy"));
        assert!(is_transient_token_error("Potential deadlock detected"));
        assert!(!is_transient_token_error("Token pool is empty"));
        assert!(!is_transient_token_error(
            "invalid_grant: refresh token revoked"
        ));
        assert!(!is_transient_token_error("No available account with quota"));
    }
}
