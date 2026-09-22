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
#[derive(Debug, Clone)]
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
        let allow_grace_retry =
            allow_grace && !self.grace_retried_accounts.contains(account_id);
        let strategy = determine_retry_strategy_inner(
            status_code,
            error_text,
            retry_after,
            retried_without_thinking,
            allow_grace_retry,
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

/// 根据错误状态码和错误信息确定重试策略
pub fn determine_retry_strategy(
    status_code: u16,
    error_text: &str,
    retried_without_thinking: bool,
) -> RetryStrategy {
    if status_code == 429 {
        let lower = error_text.to_lowercase();
        let is_hard_quota_exhausted = lower.contains("resource_exhausted")
            || lower.contains("quota_exhausted")
            || lower.contains("exceeded your current quota")
            || lower.contains("insufficient_quota");

        // [FIX] 硬配额耗尽必须立即轮换账号，绝不走 Grace Retry
        if is_hard_quota_exhausted {
            return RetryStrategy::FixedDelay(Duration::from_millis(50));
        }

        return match crate::proxy::upstream::retry::parse_legacy_retry_delay(error_text) {
            Some(delay_ms) if delay_ms > 0 && delay_ms <= 2000 => {
                let actual_delay = delay_ms.saturating_add(100);
                tracing::info!(
                    "Grace Retry Triggered: Delay {}ms is within window, using same account",
                    actual_delay
                );
                RetryStrategy::GraceRetry(Duration::from_millis(actual_delay))
            }
            Some(delay_ms) => RetryStrategy::FixedDelay(Duration::from_millis(
                delay_ms.saturating_add(200).min(30_000),
            )),
            None => RetryStrategy::LinearBackoff { base_ms: 5000 },
        };
    }

    determine_retry_strategy_inner(
        status_code,
        error_text,
        None,
        retried_without_thinking,
        true,
    )
}

fn determine_retry_strategy_inner(
    status_code: u16,
    error_text: &str,
    retry_after: Option<&str>,
    retried_without_thinking: bool,
    allow_grace_retry: bool,
) -> RetryStrategy {
    // 400 signature errors must be case-insensitive and cover all Google variants.
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
            let is_hard_quota_exhausted = lower.contains("resource_exhausted")
                || lower.contains("quota_exhausted")
                || lower.contains("exceeded your current quota")
                || lower.contains("insufficient_quota")
                || lower.contains("credits");

            // [FIX] 硬配额耗尽或不允许 grace retry (平衡/性能模式且多账号) 必须立即轮换账号，绝不走 Grace Retry 或长退避
            if is_hard_quota_exhausted || !allow_grace_retry {
                return RetryStrategy::FixedDelay(Duration::from_millis(50));
            }

            // 优先使用服务端返回的 Retry-After / quotaResetDelay
            if let Some(parsed_delay) = crate::proxy::upstream::retry::parse_retry_delay_with_source(
                error_text,
                retry_after,
            ) {
                let delay_ms = parsed_delay.raw_ms;
                // 短期原账号重试已使用时，立即回到现有换号逻辑
                if crate::proxy::upstream::retry::should_grace_retry(delay_ms) {
                    if allow_grace_retry {
                        let actual_delay = parsed_delay.actual_wait_ms();
                        tracing::info!(
                            "Grace Retry Triggered: Delay {}ms is within window, using same account",
                            actual_delay
                        );
                        RetryStrategy::GraceRetry(Duration::from_millis(actual_delay))
                    } else {
                        RetryStrategy::FixedDelay(Duration::ZERO)
                    }
                } else {
                    let actual_delay = parsed_delay.actual_wait_ms().min(30_000);
                    RetryStrategy::FixedDelay(Duration::from_millis(actual_delay))
                }
            } else {
                // 否则使用线性退避：起始 5s，逐步增加
                RetryStrategy::LinearBackoff { base_ms: 5000 }
            }
        }

        // 503 服务不可用 / 529 服务器过载
        503 | 529 => {
            // 指数退避：起始 10s，上限 60s (针对 Google 边缘节点过载)
            RetryStrategy::ExponentialBackoff {
                base_ms: 10000,
                max_ms: 60000,
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

            while let Some(attempt) = next_rotation_attempt(
                &mut used_attempts,
                account_count,
                retry_same_account,
            ) {
                retry_same_account = false;
                sends.push(attempt);
                let account_id = format!("account-{}", attempt);
                let strategy =
                    state.determine_strategy(&account_id, 429, body, None, false);
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
        assert_eq!(
            extract_retry_after_seconds("Token pool is empty"),
            None
        );
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
        assert_eq!(
            headers.get("retry-after").unwrap().to_str().unwrap(),
            "45"
        );

        let headers_no_wait = build_token_error_headers(
            Some("gemini-2.5-pro"),
            None,
            "All accounts failed or unhealthy.",
        );
        assert!(headers_no_wait.get("retry-after").is_none());
        assert_eq!(
            headers_no_wait.get("x-mapped-model").unwrap().to_str().unwrap(),
            "gemini-2.5-pro"
        );
    }
}

