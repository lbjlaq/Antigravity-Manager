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
    // 5abc8a6f 自适应限流：单账号没有明确 delay 时，采用保底 GraceRetry 等待 (3000ms)，避免闪电刷死
    assert!(
        matches!(strategy, RetryStrategy::GraceRetry(d) if d == Duration::from_millis(3000)),
        "Expected GraceRetry(3000ms), got {:?}",
        strategy
    );
}

#[test]
fn test_retry_strategy_503() {
    let strategy = determine_retry_strategy(503, "", false);
    // 5abc8a6f 自适应退避：单账号或已遍历全池采用 ExponentialBackoff { base_ms: 5000, max_ms: 30000 }
    assert!(
        matches!(
            strategy,
            RetryStrategy::ExponentialBackoff {
                base_ms: 5000,
                max_ms: 30000
            }
        ),
        "Expected ExponentialBackoff {{ base_ms: 5000, max_ms: 30000 }}, got {:?}",
        strategy
    );
}

#[test]
fn test_retry_strategy_529() {
    let strategy = determine_retry_strategy(529, "", false);
    // 5abc8a6f 自适应退避：单账号或已遍历全池采用 ExponentialBackoff { base_ms: 5000, max_ms: 30000 }
    assert!(
        matches!(
            strategy,
            RetryStrategy::ExponentialBackoff {
                base_ms: 5000,
                max_ms: 30000
            }
        ),
        "Expected ExponentialBackoff {{ base_ms: 5000, max_ms: 30000 }}, got {:?}",
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

#[test]
fn test_adaptive_retry_request_level_429_aborts_early_to_protect_remaining_accounts() {
    use crate::proxy::handlers::common::determine_retry_strategy_adaptive;
    // 请求级速率限制（无具体配额枯竭关键字，无明确重置时间）
    let request_level_429 =
        r#"{"error":{"code":429,"message":"Rate limit exceeded: too many concurrent requests."}}"#;

    let pool_size = 3;
    // 允许前 2 个账号快切逃逸尝试 (attempt 0, 1: 50ms)
    for attempt in 0..2 {
        let s = determine_retry_strategy_adaptive(
            429,
            request_level_429,
            None,
            false,
            true,
            attempt,
            pool_size,
        );
        assert_eq!(
            s,
            RetryStrategy::FixedDelay(Duration::from_millis(50)),
            "Attempt {} should fast rotate in round 1",
            attempt
        );
    }

    // attempt 2: 达到请求级 429 逃逸上限 (min(pool_size, 2) = 2)，终止进一步轮换，保护第 3 个账号
    let s_abort =
        determine_retry_strategy_adaptive(429, request_level_429, None, false, true, 2, pool_size);
    assert_eq!(s_abort, RetryStrategy::NoRetry);
}

#[test]
fn test_adaptive_retry_explicit_hard_quota_rotates_full_pool_and_backs_off() {
    use crate::proxy::handlers::common::determine_retry_strategy_adaptive;
    // 明确的账号级硬配额耗尽返回（包含确切的配额耗尽关键字）
    let hard_quota_429 = r#"{"error":{"code":429,"message":"You have exceeded your current quota. Please check your plan and billing details."}}"#;

    let pool_size = 3;
    // Round 1 (attempt 0, 1, 2): 账号级额度枯竭，全池快切寻找有额度账号
    for attempt in 0..pool_size {
        let s = determine_retry_strategy_adaptive(
            429,
            hard_quota_429,
            None,
            false,
            true,
            attempt,
            pool_size,
        );
        assert_eq!(
            s,
            RetryStrategy::FixedDelay(Duration::from_millis(50)),
            "Attempt {} should fast rotate in round 1",
            attempt
        );
    }

    // Round 2 (attempt 3): 全池账号均耗尽，激活第二轮温和退避 (2000ms)，绝不误杀为 NoRetry
    let s_round2 =
        determine_retry_strategy_adaptive(429, hard_quota_429, None, false, true, 3, pool_size);
    assert_eq!(
        s_round2,
        RetryStrategy::FixedDelay(Duration::from_millis(2000)),
        "Round 2 attempt 3 should enter gentle linear backoff"
    );
}

#[test]
fn test_adaptive_retry_google_resource_exhausted_aborts_early_to_protect_pool() {
    use crate::proxy::handlers::common::determine_retry_strategy_adaptive;
    // [Issue #3506] Google Gemini / Vertex 官方标准 429 报错（无明确重置时间与配额周期）
    let google_quota_429 =
        r#"{"error":{"code":429,"message":"Resource has been exhausted (e.g. check quota)."}}"#;

    let pool_size = 5;
    // Attempt 0: 允许首个账号尝试后快切
    let s0 =
        determine_retry_strategy_adaptive(429, google_quota_429, None, false, true, 0, pool_size);
    assert_eq!(
        s0,
        RetryStrategy::FixedDelay(Duration::from_millis(50)),
        "Attempt 0 should fast rotate"
    );

    // Attempt 1: 允许第 2 个账号尝试后快切
    let s1 =
        determine_retry_strategy_adaptive(429, google_quota_429, None, false, true, 1, pool_size);
    assert_eq!(
        s1,
        RetryStrategy::FixedDelay(Duration::from_millis(50)),
        "Attempt 1 should fast rotate"
    );

    // Attempt 2: 连续 2 个账号遭遇无明确延迟的通用 429，判定为请求级流控或恶性 Payload，
    // 必须立即熔断返回 NoRetry，保护剩余 3 个健康账号不被级联锁定为 RateLimitExceeded
    let s2 =
        determine_retry_strategy_adaptive(429, google_quota_429, None, false, true, 2, pool_size);
    assert_eq!(
        s2,
        RetryStrategy::NoRetry,
        "Attempt 2 must abort to protect remaining accounts in pool from cascade lockout"
    );
}

#[test]
fn test_adaptive_retry_single_account_never_spins_50ms_on_hard_quota() {
    use crate::proxy::handlers::common::determine_retry_strategy_adaptive;
    let google_quota_429 =
        r#"{"error":{"code":429,"message":"Resource has been exhausted (e.g. check quota)."}}"#;

    // 单账号 (pool_size = 1): 遇到硬配额耗尽也绝不能返回 50ms 闪电空转，必须执行 >= 3000ms 的退避保护
    let s = determine_retry_strategy_adaptive(429, google_quota_429, None, false, true, 0, 1);
    match s {
        RetryStrategy::GraceRetry(d) => assert!(d >= Duration::from_millis(3000)),
        RetryStrategy::FixedDelay(d) => assert!(d >= Duration::from_millis(3000)),
        other => panic!("Single account must back off >= 3s, got {:?}", other),
    }
}

#[test]
fn test_adaptive_retry_multi_account_round_1_micro_gap_takes_grace_retry_issue_3329() {
    use crate::proxy::handlers::common::determine_retry_strategy_adaptive;
    // Google Imagen 3 (gemini-3.1-flash-image) 标准 429 报错报文 (1.173612231s 重置时间)
    let imagen_429 = r#"{
        "error": {
            "code": 429,
            "message": "You have exhausted your capacity on this model. Your quota will reset after 1s.",
            "status": "RESOURCE_EXHAUSTED",
            "details": [
                {
                    "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                    "reason": "RATE_LIMIT_EXCEEDED",
                    "metadata": {
                        "model": "gemini-3.1-flash-image",
                        "quotaResetDelay": "1.173612231s"
                    }
                },
                {
                    "@type": "type.googleapis.com/google.rpc.RetryInfo",
                    "retryDelay": "1.173612231s"
                }
            ]
        }
    }"#;

    // 多账号 Round 1 (attempt = 0, pool_size = 6):
    // 遇到微间隙 (<= 2500ms，此处 1174ms + 200ms buffer = 1374ms)，
    // 绝不能在第 1 轮就 50ms 闪电切号导致 300ms 内全池 6 个账号全被打穿锁入冷却！
    // 必须优先在当前账号执行 GraceRetry(1374ms)，精准命中 1.17s 后的配额刷新，原地满血复活！
    let s = determine_retry_strategy_adaptive(429, imagen_429, None, false, true, 0, 6);
    match s {
        RetryStrategy::GraceRetry(d) => assert_eq!(d, Duration::from_millis(1374)),
        other => panic!("Expected GraceRetry(1374ms), got {:?}", other),
    }

    // 当该账号的 GraceRetry 已被消耗 (allow_grace_retry = false)：
    // 允许顺畅回退至第 1 轮的 50ms 闪电切号，避免死锁
    let s_fallback = determine_retry_strategy_adaptive(429, imagen_429, None, false, false, 0, 6);
    assert_eq!(
        s_fallback,
        RetryStrategy::FixedDelay(Duration::from_millis(50)),
        "GraceRetry 已消耗时应顺畅回退至全池轮换"
    );
}
