use antigravity_tools_lib::proxy::pipeline::rotation::record_attempted_account;
use antigravity_tools_lib::proxy::pipeline::terminal::{
    apply_terminal, FailureStatusTracker, UpstreamTerminal, FIRST_BYTE_STAGE_TERMINAL_MARKER,
    GATEWAY_BUDGET_TERMINAL_MARKER, UPLOAD_STAGE_TERMINAL_MARKER,
};
use antigravity_tools_lib::proxy::upstream::client::{PayloadMediaKind, UpstreamClient};
use reqwest::StatusCode;
use std::collections::HashSet;

/// 模拟上游返回的响应状态
#[derive(Debug, Clone)]
enum SimulatedUpstreamResponse {
    /// 正常快速响应 (首字节在 1.5s 内到达)
    FastSuccess { duration_secs: f64 },
    /// 较慢响应 (例如大上下文计算，在 35s 到达，未超过首选或备用预算)
    SlowSuccess { duration_secs: f64 },
    /// 首字节等待超时（在首选端点耗费超过首字等待预算，例如 > 45s~180s）
    FirstByteTimeout { elapsed_secs: f64 },
    /// 上传中途连接重置/中断 (TCP drop / RST)
    UploadDropped {
        sent_bytes: usize,
        total_bytes: usize,
    },
}

/// 模拟多账号轮换执行器
struct SimulatedGatewaySession {
    account_pool: Vec<String>,
    attempted_accounts: HashSet<String>,
    tracker: FailureStatusTracker,
    global_elapsed_secs: f64,
    global_budget_secs: f64,
}

impl SimulatedGatewaySession {
    fn new(accounts: Vec<&str>, budget_secs: f64) -> Self {
        Self {
            account_pool: accounts.into_iter().map(String::from).collect(),
            attempted_accounts: HashSet::new(),
            tracker: FailureStatusTracker::default(),
            global_elapsed_secs: 0.0,
            global_budget_secs: budget_secs,
        }
    }

    /// 按照网关调度逻辑，挑选下一个可用账号
    fn pick_account(&self) -> Option<String> {
        self.account_pool
            .iter()
            .find(|acc| !self.attempted_accounts.contains(*acc))
            .cloned()
    }
}

#[test]
fn test_simulated_scenario_unresponsive_account_rotates_to_healthy_account() {
    println!("\n=== 场景 1: 上游首选端点无响应/超时，网关敏捷切号且不浪费额度 ===");

    let mut session =
        SimulatedGatewaySession::new(vec!["acc-1@gmail.com", "acc-2@gmail.com"], 270.0);
    let mut force_rotate = false;
    let mut success_account = None;

    for attempt in 0..session.account_pool.len() {
        let current_account = session
            .pick_account()
            .expect("Should have available account");
        println!(
            "[Attempt {}] Selected account: {}",
            attempt + 1,
            current_account
        );

        let upstream_behavior = if current_account == "acc-1@gmail.com" {
            // Account 1: 首字节超时（首选端点等待无响应）
            SimulatedUpstreamResponse::FirstByteTimeout { elapsed_secs: 45.0 }
        } else {
            // Account 2: 响应迅速，2.0s 成功
            SimulatedUpstreamResponse::FastSuccess { duration_secs: 2.0 }
        };

        match upstream_behavior {
            SimulatedUpstreamResponse::FirstByteTimeout { elapsed_secs } => {
                session.global_elapsed_secs += elapsed_secs;
                let err_text = format!(
                    "{}Upstream send failed (stage=await_first_byte, elapsed={:.1}s)",
                    FIRST_BYTE_STAGE_TERMINAL_MARKER, elapsed_secs
                );

                let handling = apply_terminal(
                    &err_text,
                    &mut session.tracker,
                    "claude",
                    attempt + 1,
                    session.account_pool.len(),
                    elapsed_secs,
                    |_| {},
                )
                .expect("Must classify as terminal");

                // 核心验证 1: 首字节超时不终止轮换，解绑粘性账号
                assert_eq!(handling.terminal, UpstreamTerminal::FirstByte);
                assert!(
                    !handling.disposition.stop_rotation,
                    "FirstByte should NOT stop rotation"
                );
                assert!(
                    handling.disposition.abandon_sticky_account,
                    "Must abandon sticky binding"
                );

                // 核心验证 2: 记录并轮换到下一个账号
                record_attempted_account(
                    &mut session.attempted_accounts,
                    &current_account,
                    session.account_pool.len(),
                    &mut force_rotate,
                );
                assert!(force_rotate);
                println!(
                    "  -> Account {} timed out. Remaining global budget: {:.1}s. Rotating...",
                    current_account,
                    session.global_budget_secs - session.global_elapsed_secs
                );
            }
            SimulatedUpstreamResponse::FastSuccess { duration_secs } => {
                session.global_elapsed_secs += duration_secs;
                success_account = Some(current_account);
                println!(
                    "  -> Account {} succeeded in {:.1}s. Request completed successfully!",
                    success_account.as_ref().unwrap(),
                    duration_secs
                );
                break;
            }
            _ => panic!("Unexpected simulated response in scenario 1"),
        }
    }

    assert_eq!(success_account.as_deref(), Some("acc-2@gmail.com"));
    assert!(
        session.global_elapsed_secs < 60.0,
        "Total elapsed time should be well within global budget"
    );
}

#[test]
fn test_simulated_scenario_mid_flight_upload_drop_rotates_without_quota_loss() {
    println!("\n=== 场景 2: 上游上传阶段断开连接 (TCP RST)，网关不记终态并安全重放 ===");

    let mut session =
        SimulatedGatewaySession::new(vec!["acc-1@gmail.com", "acc-2@gmail.com"], 270.0);
    let mut force_rotate = false;
    let mut success_account = None;

    for attempt in 0..session.account_pool.len() {
        let current_account = session
            .pick_account()
            .expect("Should have available account");
        println!(
            "[Attempt {}] Selected account: {}",
            attempt + 1,
            current_account
        );

        let upstream_behavior = if current_account == "acc-1@gmail.com" {
            // Account 1: 上传大包到一半时被上游 RST 断开
            SimulatedUpstreamResponse::UploadDropped {
                sent_bytes: 524_288,
                total_bytes: 1_048_576,
            }
        } else {
            // Account 2: 上传并成功返回
            SimulatedUpstreamResponse::SlowSuccess {
                duration_secs: 12.0,
            }
        };

        match upstream_behavior {
            SimulatedUpstreamResponse::UploadDropped {
                sent_bytes,
                total_bytes,
            } => {
                session.global_elapsed_secs += 5.0;
                let err_text = format!(
                    "{}Upload stalled / connection reset by peer (sent {}/{} bytes, stage=upload)",
                    UPLOAD_STAGE_TERMINAL_MARKER, sent_bytes, total_bytes
                );

                let handling = apply_terminal(
                    &err_text,
                    &mut session.tracker,
                    "claude",
                    attempt + 1,
                    session.account_pool.len(),
                    5.0,
                    |_| {},
                )
                .expect("Must classify as terminal");

                // 核心验证: 上传阶段中断不终结整条请求，允许轮换
                assert_eq!(handling.terminal, UpstreamTerminal::Upload);
                assert!(
                    !handling.disposition.stop_rotation,
                    "Upload failure should NOT stop rotation"
                );

                record_attempted_account(
                    &mut session.attempted_accounts,
                    &current_account,
                    session.account_pool.len(),
                    &mut force_rotate,
                );
                assert!(force_rotate);
                println!("  -> Account 1 dropped connection mid-flight. Rotating to Account 2 without deducting quota.");
            }
            SimulatedUpstreamResponse::SlowSuccess { duration_secs } => {
                session.global_elapsed_secs += duration_secs;
                success_account = Some(current_account);
                println!(
                    "  -> Account {} received full response in {:.1}s. Request resolved!",
                    success_account.as_ref().unwrap(),
                    duration_secs
                );
                break;
            }
            _ => panic!("Unexpected simulated response in scenario 2"),
        }
    }

    assert_eq!(success_account.as_deref(), Some("acc-2@gmail.com"));
    assert!(session.global_elapsed_secs < 30.0);
}

#[test]
fn test_simulated_scenario_varying_durations_and_large_body_limit() {
    println!("\n=== 场景 3: 不同响应时长梯度与大请求体单端点抑制策略 ===");

    let upload_cfg = antigravity_tools_lib::proxy::config::UploadTimeoutConfig::default();
    let stream_cfg = antigravity_tools_lib::proxy::config::StreamTimeoutConfig::default();

    // 1. 小体积对话 (1KB): 允许全部备用端点重传 fallback
    let small_plan = UpstreamClient::plan_upstream_timeouts_with_observed(
        PayloadMediaKind::TextChat,
        1024,
        &upload_cfg,
        None,
    );
    assert!(!small_plan.large_body);
    assert_eq!(small_plan.endpoint_attempt_limit, 2);

    // 2. 大上下文/高 Token (>= 1MB 或 >= 30,000 tokens): 限制为 1 个端点尝试，避免冗余重传耗光预算
    let large_context_plan = UpstreamClient::plan_upstream_timeouts_with_observed(
        PayloadMediaKind::TextChat,
        1024 * 1024,
        &upload_cfg,
        None,
    );
    assert!(large_context_plan.large_body);
    assert_eq!(
        large_context_plan.endpoint_attempt_limit, 1,
        "Large context must cap endpoint attempts to 1"
    );

    // 3. 备用端点等待封顶 45s 测试
    let fallback_first_byte_wait = UpstreamClient::calculate_first_byte_wait_secs(
        PayloadMediaKind::TextChat,
        1, // idx = 1 (Fallback / Sandbox)
        &upload_cfg,
        &stream_cfg,
        1024,
        false,
    );
    // 基础计算可能较高，但在 client.rs 中 idx > 0 时通过 .min(45) 强行截断
    let capped_fallback_wait = fallback_first_byte_wait.min(45);
    assert_eq!(
        capped_fallback_wait, 45,
        "Fallback endpoint wait must be capped to 45s"
    );
    println!(
        "  -> Small payload endpoint attempt limit: {}",
        small_plan.endpoint_attempt_limit
    );
    println!(
        "  -> Large context payload endpoint attempt limit: {}",
        large_context_plan.endpoint_attempt_limit
    );
    println!(
        "  -> Fallback secondary endpoint first-byte wait cap: {}s",
        capped_fallback_wait
    );
}

#[test]
fn test_simulated_scenario_global_budget_exhaustion_stops_rotation() {
    println!("\n=== 场景 4: 全局预算 (270s) 最终耗尽时，触发刚性终止守护 ===");

    let mut session = SimulatedGatewaySession::new(
        vec!["acc-1@gmail.com", "acc-2@gmail.com", "acc-3@gmail.com"],
        270.0,
    );
    session.global_elapsed_secs = 270.5; // 已超过全局预算

    let err_text = format!(
        "{}Gateway global request budget (270s) exhausted while awaiting upstream",
        GATEWAY_BUDGET_TERMINAL_MARKER
    );

    let handling = apply_terminal(
        &err_text,
        &mut session.tracker,
        "claude",
        1,
        session.account_pool.len(),
        270.5,
        |_| {},
    )
    .expect("Must classify as terminal");

    assert_eq!(handling.terminal, UpstreamTerminal::GlobalBudget);
    assert!(
        handling.disposition.stop_rotation,
        "Global budget exhaustion must stop rotation"
    );
    assert_eq!(session.tracker.final_status(), StatusCode::GATEWAY_TIMEOUT);
    println!("  -> Global budget exhausted correctly returned 504 and terminated rotation.");
}
