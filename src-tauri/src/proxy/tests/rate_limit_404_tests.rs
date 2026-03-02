//! 测试 RateLimitTracker::parse_from_error 对 404 状态码的处理逻辑：
//! - 短时锁定（5s）
//! - 不累加失败计数
//! - 与 5xx 锁定时长的差异

use crate::proxy::rate_limit::{RateLimitReason, RateLimitTracker};

#[test]
fn test_parse_from_error_404_short_lockout() {
    let tracker = RateLimitTracker::new();
    let backoff_steps = vec![60, 300, 1800, 7200];

    let info = tracker.parse_from_error("acc_404", 404, None, "Not Found", None, &backoff_steps);
    assert!(info.is_some(), "404 should return Some(RateLimitInfo)");
    let info = info.unwrap();
    assert_eq!(info.retry_after_sec, 5, "404 should lock out for 5 seconds");
    assert_eq!(
        info.reason,
        RateLimitReason::ServerError,
        "404 reason should be ServerError"
    );
}

#[test]
fn test_404_does_not_accumulate_failure_count() {
    let tracker = RateLimitTracker::new();
    let backoff_steps = vec![60, 300, 1800, 7200];

    // 连续多次 404，锁定时间应始终为 5s（不像 429 QuotaExhausted 那样递增）
    for i in 1..=5 {
        // 清除上一次的限流记录，模拟轮换后再次遇到 404
        tracker.clear("acc_404_repeat");
        let info = tracker.parse_from_error(
            "acc_404_repeat",
            404,
            None,
            "Not Found",
            None,
            &backoff_steps,
        );
        assert!(info.is_some(), "404 attempt {} should return Some", i);
        assert_eq!(
            info.unwrap().retry_after_sec,
            5,
            "404 attempt {} should still lock for 5s, not escalate",
            i
        );
    }
}

#[test]
fn test_404_vs_5xx_lockout_duration() {
    let tracker = RateLimitTracker::new();
    let backoff_steps = vec![60, 300, 1800, 7200];

    // 404 → 5s lockout
    let info_404 =
        tracker.parse_from_error("acc_cmp_404", 404, None, "Not Found", None, &backoff_steps);
    assert_eq!(
        info_404.unwrap().retry_after_sec,
        5,
        "404 should lock for 5s"
    );

    // 503 → 8s lockout
    let info_503 = tracker.parse_from_error(
        "acc_cmp_503",
        503,
        None,
        "Service Unavailable",
        None,
        &backoff_steps,
    );
    assert_eq!(
        info_503.unwrap().retry_after_sec,
        8,
        "503 should lock for 8s"
    );
}
