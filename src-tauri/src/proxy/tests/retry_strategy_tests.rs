//! 测试 determine_retry_strategy 和 should_rotate_account 的所有分支，
//! 重点覆盖 404 重试与账号轮换逻辑。

use crate::proxy::handlers::common::{
    determine_retry_strategy, should_rotate_account, RetryStrategy,
};
use std::time::Duration;

// ===== determine_retry_strategy =====

#[test]
fn test_retry_strategy_404() {
    let strategy = determine_retry_strategy(404, "", false);
    match strategy {
        RetryStrategy::FixedDelay(d) => assert_eq!(d, Duration::from_millis(300)),
        other => panic!("Expected FixedDelay(300ms), got {:?}", other),
    }
}

#[test]
fn test_retry_strategy_429_no_delay() {
    let strategy = determine_retry_strategy(429, "rate limited", false);
    assert!(
        matches!(strategy, RetryStrategy::LinearBackoff { base_ms: 5000 }),
        "Expected LinearBackoff {{ base_ms: 5000 }}, got {:?}",
        strategy
    );
}

#[test]
fn test_retry_strategy_503() {
    let strategy = determine_retry_strategy(503, "", false);
    assert!(
        matches!(
            strategy,
            RetryStrategy::ExponentialBackoff {
                base_ms: 10000,
                max_ms: 60000
            }
        ),
        "Expected ExponentialBackoff {{ base_ms: 10000, max_ms: 60000 }}, got {:?}",
        strategy
    );
}

#[test]
fn test_retry_strategy_529() {
    let strategy = determine_retry_strategy(529, "", false);
    assert!(
        matches!(
            strategy,
            RetryStrategy::ExponentialBackoff {
                base_ms: 10000,
                max_ms: 60000
            }
        ),
        "Expected ExponentialBackoff {{ base_ms: 10000, max_ms: 60000 }}, got {:?}",
        strategy
    );
}

#[test]
fn test_retry_strategy_500() {
    let strategy = determine_retry_strategy(500, "", false);
    assert!(
        matches!(strategy, RetryStrategy::LinearBackoff { base_ms: 3000 }),
        "Expected LinearBackoff {{ base_ms: 3000 }}, got {:?}",
        strategy
    );
}

#[test]
fn test_retry_strategy_401_403() {
    for status in [401, 403] {
        let strategy = determine_retry_strategy(status, "", false);
        match strategy {
            RetryStrategy::FixedDelay(d) => assert_eq!(d, Duration::from_millis(200)),
            other => panic!("Expected FixedDelay(200ms) for {}, got {:?}", status, other),
        }
    }
}

#[test]
fn test_retry_strategy_other() {
    for status in [200, 201, 301, 418, 502] {
        let strategy = determine_retry_strategy(status, "", false);
        assert!(
            matches!(strategy, RetryStrategy::NoRetry),
            "Expected NoRetry for {}, got {:?}",
            status,
            strategy
        );
    }
}

#[test]
fn test_retry_strategy_400_thinking_signature() {
    let signatures = [
        "Invalid `signature` for thinking",
        "Error with thinking.signature",
        "thinking.thinking block failed",
        "Corrupted thought signature detected",
    ];
    for sig in signatures {
        let strategy = determine_retry_strategy(400, sig, false);
        match strategy {
            RetryStrategy::FixedDelay(d) => assert_eq!(d, Duration::from_millis(200)),
            other => panic!(
                "Expected FixedDelay(200ms) for 400 + '{}', got {:?}",
                sig, other
            ),
        }
    }
}

#[test]
fn test_retry_strategy_400_no_signature() {
    let strategy = determine_retry_strategy(400, "bad request", false);
    assert!(
        matches!(strategy, RetryStrategy::NoRetry),
        "Expected NoRetry for 400 without signature, got {:?}",
        strategy
    );
}

// ===== should_rotate_account =====

#[test]
fn test_rotate_account_true_cases() {
    for status in [429, 401, 403, 404, 500, 503, 529] {
        assert!(
            should_rotate_account(status, None),
            "Expected should_rotate_account({}) == true",
            status
        );
    }
}

#[test]
fn test_rotate_account_false_cases() {
    for status in [400, 200, 502] {
        assert!(
            !should_rotate_account(status, None),
            "Expected should_rotate_account({}) == false",
            status
        );
    }
}

// ===== 自适应多轮次与单账号等待综合回归单测 (Refs #3485) =====

#[test]
fn test_calculate_max_retry_attempts_adaptive() {
    use crate::proxy::handlers::common::calculate_max_retry_attempts;
    assert_eq!(calculate_max_retry_attempts(0), 3);
    assert_eq!(calculate_max_retry_attempts(1), 3);
    assert_eq!(calculate_max_retry_attempts(2), 4);
    assert_eq!(calculate_max_retry_attempts(3), 6);
    assert_eq!(calculate_max_retry_attempts(5), 10);
    assert_eq!(calculate_max_retry_attempts(8), 12);
    assert_eq!(calculate_max_retry_attempts(20), 12);
}

#[test]
fn test_adaptive_retry_single_account_waits_quota_reset_delay() {
    use crate::proxy::handlers::common::determine_retry_strategy_adaptive;
    let err_json = r#"{"error":{"message":"Resource has been exhausted (e.g. check quota).","details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","quotaResetDelay":"4s"}]}}"#;

    // 单账号 (pool_size = 1): 绝不 50ms 闪电刷死，必须原地 GraceRetry 等待 quotaResetDelay (4s + 200ms = 4200ms)
    let strategy = determine_retry_strategy_adaptive(429, err_json, None, false, true, 0, 1);
    match strategy {
        RetryStrategy::GraceRetry(d) => assert_eq!(d, Duration::from_millis(4200)),
        other => panic!("Expected GraceRetry(4200ms), got {:?}", other),
    }
}

#[test]
fn test_adaptive_retry_multi_account_round_1_fast_rotates() {
    use crate::proxy::handlers::common::determine_retry_strategy_adaptive;
    let err_json = r#"{"error":{"message":"Resource has been exhausted","details":[{"quotaResetDelay":"4s"}]}}"#;

    // 多账号 Round 1 (attempt = 0, pool_size = 3): 闪电轮换 (50ms)，优先切向号池中其他健康账号
    let strategy = determine_retry_strategy_adaptive(429, err_json, None, false, true, 0, 3);
    match strategy {
        RetryStrategy::FixedDelay(d) => assert_eq!(d, Duration::from_millis(50)),
        other => panic!("Expected FixedDelay(50ms), got {:?}", other),
    }
}

#[test]
fn test_adaptive_retry_multi_account_round_2_small_gap_micro_waits() {
    use crate::proxy::handlers::common::determine_retry_strategy_adaptive;
    let err_json = r#"{"error":{"message":"Resource has been exhausted","details":[{"quotaResetDelay":"3s"}]}}"#;

    // 多账号 Round 2 (attempt = 3, pool_size = 3): 全池已试过一遍，遇到小间隙 (3s <= 5s)，小等并原地重试
    let strategy = determine_retry_strategy_adaptive(429, err_json, None, false, true, 3, 3);
    match strategy {
        RetryStrategy::GraceRetry(d) => assert_eq!(d, Duration::from_millis(3200)),
        other => panic!("Expected GraceRetry(3200ms), got {:?}", other),
    }
}

#[test]
fn test_adaptive_retry_multi_account_round_2_large_gap_rotates_if_more_than_two_accounts() {
    use crate::proxy::handlers::common::determine_retry_strategy_adaptive;
    let err_json = r#"{"error":{"message":"Resource has been exhausted","details":[{"quotaResetDelay":"15s"}]}}"#;

    // 多账号 Round 2 (attempt = 3, pool_size = 3): 延迟为 15s (> 5s) 且还有其他账号，继续闪电轮换寻找已恢复账号
    let strategy = determine_retry_strategy_adaptive(429, err_json, None, false, true, 3, 3);
    match strategy {
        RetryStrategy::FixedDelay(d) => assert_eq!(d, Duration::from_millis(50)),
        other => panic!("Expected FixedDelay(50ms), got {:?}", other),
    }
}
