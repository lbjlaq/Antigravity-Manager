//! 测试 RateLimitTracker 对 404 / 500 / 503 与 429 的处理基准：
//! - 404 (模型不存在) 与 500/503 (服务端异常) 绝对不打入账号冷却池 (返回 None)
//! - 真正的 429 (限流) 准确打入冷却池，且提供 model 时严格进行模型级隔离，禁止连坐全账号

use crate::proxy::rate_limit::RateLimitTracker;

#[test]
fn test_404_and_5xx_never_lock_account() {
    let tracker = RateLimitTracker::new();
    let backoff_steps = vec![60, 300, 1800, 7200];

    // 404 模型不存在：绝对禁止打入冷却池
    let info_404 = tracker.parse_from_error(
        "acc_test",
        404,
        None,
        "models/nonexistent is not found",
        Some("nonexistent".to_string()),
        &backoff_steps,
    );
    assert!(
        info_404.is_none(),
        "404 must return None (no account lockout)"
    );
    assert!(!tracker.is_rate_limited("acc_test", Some("nonexistent")));
    assert!(!tracker.is_rate_limited("acc_test", None));

    // 500 服务器内部错误：服务故障，绝不作为账号限流
    let info_500 = tracker.parse_from_error(
        "acc_test",
        500,
        None,
        "Internal server error",
        Some("gemini-2.5-pro".to_string()),
        &backoff_steps,
    );
    assert!(info_500.is_none(), "500 must return None");
    assert!(!tracker.is_rate_limited("acc_test", Some("gemini-2.5-pro")));

    // 503 服务不可用：瞬时故障，绝不作为账号限流
    let info_503 = tracker.parse_from_error(
        "acc_test",
        503,
        None,
        "Service Unavailable",
        Some("gemini-2.5-pro".to_string()),
        &backoff_steps,
    );
    assert!(info_503.is_none(), "503 must return None");
    assert!(!tracker.is_rate_limited("acc_test", Some("gemini-2.5-pro")));
}

#[test]
fn test_rate_limit_with_model_isolates_fine_grained() {
    let tracker = RateLimitTracker::new();
    let backoff_steps = vec![60, 300, 1800, 7200];

    // 429 速率限制触发（带具体 model）
    let info_429 = tracker.parse_from_error(
        "acc_isolated",
        429,
        None,
        r#"{"error":{"message":"Resource has been exhausted (e.g. check quota)."}}"#,
        Some("claude-test-model".to_string()),
        &backoff_steps,
    );
    assert!(info_429.is_some(), "429 should return Some");

    // 该账号针对 claude-test-model 处于限流
    assert!(tracker.is_rate_limited("acc_isolated", Some("claude-test-model")));

    // 关键验证：该账号针对其他正常模型（如 gemini-2.5-flash / claude-sonnet-4-6）绝对不被连坐！
    assert!(
        !tracker.is_rate_limited("acc_isolated", Some("gemini-2.5-flash")),
        "Other models must NOT be locked when rate limit specifies a model"
    );
    assert!(
        !tracker.is_rate_limited("acc_isolated", Some("claude-sonnet-4-6")),
        "Healthy models must remain available"
    );
}
