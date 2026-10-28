//! 请求级全局预算与重放护栏（协议无关的 Pipeline 横切能力）
//!
//! # 为什么放在 Pipeline 而不是各 handler
//!
//! 四大协议适配器（OpenAI Responses / OpenAI Chat Completions / Anthropic Claude /
//! Google Gemini）最终都汇聚到同一个厂商无关的上游客户端
//! `upstream::client::call_v1_internal_with_headers`。把预算与重放护栏收敛在这里，
//! 可以保证四条链路**只有一份实现、一个上限**，不会随协议各自漂移。
//!
//! # 三件职责
//!
//! 1. **时间预算**：入口中间件创建截止时刻；上游客户端把每次调用的内部预算夹到剩余值；
//!    耗尽即返回终态错误，各 handler 统一“不罚账号、不跨账号重放、直接 504”。
//! 2. **重放护栏**：同一个请求内对**大 body** 的上游调用次数设上限，
//!    避免 429/5xx 轮换把同一份几百 KB 的报文体反复上传（带宽维度的笛卡尔积）。
//! 3. **规模提示**：handler 把真实估算 token 数写进来，供首字节预算按真实 prefill
//!    规模选取，而不是只靠 `body_len / 3.5` 猜（工具 schema 大或含 base64 图片时会低估）。

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::task_local;

/// 默认全局预算（秒）
///
/// 取 270s：比 Claude Code 的 300s 首包看门狗留 30s 余量，仍能在客户端放弃前
/// 返回结构化 504；同时比原来的 240s 多 30s，给超大上下文 prefill 与大体积上传留空间。
pub const DEFAULT_GATEWAY_REQUEST_BUDGET_SECS: u64 = 270;

/// 单个上游尝试允许的最小剩余预算（秒）：低于它就不再发起新尝试
pub const MIN_ATTEMPT_BUDGET_SECS: u64 = 5;

/// 认定为“大 body”的重放护栏阈值（字节）
///
/// 256KB ≈ 73K token。低于它的重传代价可接受，不启用次数上限。
pub const REPLAY_GUARD_BODY_BYTES: usize = 256 * 1024;

/// 大 body 在**同一个客户端请求内**允许的上游调用次数上限（含首次）
///
/// 超过即视为“跨账号重放放大”，直接终态失败，交由客户端决定是否重试。
pub const MAX_LARGE_BODY_UPSTREAM_CALLS: usize = 2;

task_local! {
    /// 当前请求的上下文（仅由请求入口中间件注入）
    static REQUEST_CONTEXT: Arc<RequestContext>;
}

/// 一条客户端请求在网关内部的运行期上下文
pub struct RequestContext {
    deadline: std::time::Instant,
    upstream_calls: AtomicUsize,
    estimated_tokens: AtomicU32,
}

impl RequestContext {
    pub fn new(deadline: std::time::Instant) -> Arc<Self> {
        Arc::new(Self {
            deadline,
            upstream_calls: AtomicUsize::new(0),
            estimated_tokens: AtomicU32::new(0),
        })
    }

    fn remaining_secs(&self) -> u64 {
        self.deadline
            .saturating_duration_since(std::time::Instant::now())
            .as_secs()
    }

    /// 记录一次上游调用并返回其序号（1-based）
    fn register_upstream_call(&self) -> usize {
        self.upstream_calls.fetch_add(1, Ordering::Relaxed) + 1
    }
}

/// 读取生效的全局预算（支持 `ANTIGRAVITY_GATEWAY_REQUEST_BUDGET_SECS` 覆盖，进程内缓存）
pub fn gateway_request_budget_secs() -> u64 {
    static EFFECTIVE: OnceLock<u64> = OnceLock::new();
    *EFFECTIVE.get_or_init(|| {
        let secs = std::env::var("ANTIGRAVITY_GATEWAY_REQUEST_BUDGET_SECS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|v| *v >= MIN_ATTEMPT_BUDGET_SECS)
            .unwrap_or(DEFAULT_GATEWAY_REQUEST_BUDGET_SECS);
        if secs > 300 {
            tracing::warn!(
                budget_secs = secs,
                "全局请求预算 {}s 已超过常见客户端首包看门狗（jeikcode 180s / Claude Code 300s）：\
                 超时场景会由客户端先断开，网关侧只能以 [Stage] outcome=canceled 留痕，无法返回结构化 504",
                secs
            );
        }
        secs
    })
}

/// 新建一个“从现在开始”的截止时刻
pub fn deadline_now() -> std::time::Instant {
    std::time::Instant::now() + Duration::from_secs(gateway_request_budget_secs())
}

/// 在给定 future 上挂载请求上下文
pub async fn scope<F: std::future::Future>(ctx: Arc<RequestContext>, fut: F) -> F::Output {
    REQUEST_CONTEXT.scope(ctx, fut).await
}

/// 读取当前请求上下文（后台任务为 None）
pub fn current() -> Option<Arc<RequestContext>> {
    REQUEST_CONTEXT.try_with(Arc::clone).ok()
}

/// 当前请求的截止时刻（无入口上下文时为 None）
pub fn current_deadline() -> Option<std::time::Instant> {
    current().map(|ctx| ctx.deadline)
}

/// 剩余预算（秒）；无入口上下文时返回 None（表示不受全局约束）
pub fn remaining_secs() -> Option<u64> {
    current().map(|ctx| ctx.remaining_secs())
}

/// 全局预算是否已不足以再发起一次上游尝试
pub fn exhausted() -> bool {
    remaining_secs()
        .map(|remaining| remaining < MIN_ATTEMPT_BUDGET_SECS)
        .unwrap_or(false)
}

/// 把单次上游调用的内部预算夹到“本请求剩余全局预算”
///
/// 没有入口上下文（后台任务）时原样返回，行为不变。
pub fn clamp_budget_secs(budget_secs: u64) -> u64 {
    match remaining_secs() {
        Some(remaining) => budget_secs.min(remaining.max(MIN_ATTEMPT_BUDGET_SECS)),
        None => budget_secs,
    }
}

/// handler 写入真实估算 token 数（供首字节预算按真实 prefill 规模取值）
///
/// 0 会被忽略：估算失败时保留已有值/回退到体积估算，避免把预算压到 1 token。
pub fn set_estimated_tokens(tokens: u32) {
    if tokens == 0 {
        return;
    }
    if let Some(ctx) = current() {
        ctx.estimated_tokens.store(tokens, Ordering::Relaxed);
    }
}

/// 读取 handler 写入的真实估算 token 数；未写入时返回 None
pub fn estimated_tokens() -> Option<u32> {
    current().and_then(|ctx| match ctx.estimated_tokens.load(Ordering::Relaxed) {
        0 => None,
        tokens => Some(tokens),
    })
}

/// 登记一次上游调用，并在“大 body + 超出重放上限”时拒绝
///
/// 返回 `Err(已发生的调用次数)` 表示本次调用被护栏拦下。
pub fn register_upstream_call(body_len: usize) -> Result<(), usize> {
    let Some(ctx) = current() else {
        return Ok(());
    };
    let call_no = ctx.register_upstream_call();
    if body_len >= REPLAY_GUARD_BODY_BYTES && call_no > MAX_LARGE_BODY_UPSTREAM_CALLS {
        return Err(call_no);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_budget_leaves_room_before_client_watchdog() {
        // 必须小于 Claude Code 的 300s 看门狗，才可能在客户端放弃前返回结构化 504
        assert!(DEFAULT_GATEWAY_REQUEST_BUDGET_SECS < 300);
        assert!(DEFAULT_GATEWAY_REQUEST_BUDGET_SECS > 240);
    }

    #[tokio::test]
    async fn scope_exposes_remaining_and_clamps_budget() {
        // 无上下文：不约束
        assert_eq!(remaining_secs(), None);
        assert!(!exhausted());
        assert_eq!(clamp_budget_secs(9_999), 9_999);

        let ctx = RequestContext::new(deadline_now());
        scope(ctx, async {
            let remaining = remaining_secs().expect("deadline must be visible");
            assert!(remaining >= DEFAULT_GATEWAY_REQUEST_BUDGET_SECS - 2);
            assert!(clamp_budget_secs(10_000) <= DEFAULT_GATEWAY_REQUEST_BUDGET_SECS);
        })
        .await;
    }

    #[tokio::test]
    async fn expired_deadline_is_reported_as_exhausted() {
        let expired = std::time::Instant::now() - Duration::from_secs(1);
        scope(RequestContext::new(expired), async {
            assert!(exhausted());
            assert_eq!(
                clamp_budget_secs(120),
                MIN_ATTEMPT_BUDGET_SECS,
                "exhausted budget must collapse to the minimal attempt window"
            );
        })
        .await;
    }

    #[tokio::test]
    async fn large_body_replay_is_capped_but_small_body_is_not() {
        scope(RequestContext::new(deadline_now()), async {
            // 大 body：前两次放行，第三次拒绝
            assert!(register_upstream_call(REPLAY_GUARD_BODY_BYTES).is_ok());
            assert!(register_upstream_call(REPLAY_GUARD_BODY_BYTES).is_ok());
            assert_eq!(
                register_upstream_call(REPLAY_GUARD_BODY_BYTES),
                Err(3),
                "third large-body upstream call must be blocked"
            );
        })
        .await;

        scope(RequestContext::new(deadline_now()), async {
            // 小 body：不设次数上限
            for _ in 0..20 {
                assert!(register_upstream_call(1024).is_ok());
            }
        })
        .await;
    }

    #[tokio::test]
    async fn estimated_tokens_roundtrip() {
        assert_eq!(estimated_tokens(), None);
        scope(RequestContext::new(deadline_now()), async {
            assert_eq!(estimated_tokens(), None);
            set_estimated_tokens(150_000);
            assert_eq!(estimated_tokens(), Some(150_000));
        })
        .await;
    }
}
