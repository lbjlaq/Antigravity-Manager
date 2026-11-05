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
/// 4MB ≈ 1M token 文本或大型多模态请求。低于它的常规长文本长上下文（例如 900KB~2MB）
/// 在遭遇上游偶发超时或瞬态限流时，允许正常跨账号轮换容灾；
/// 高于 4MB 的巨型报文（如高分辨率视频/巨幅多图）则受严格的调用次数护栏保护。
pub const REPLAY_GUARD_BODY_BYTES: usize = 4 * 1024 * 1024;

/// 大 body 在**同一个客户端请求内**允许的上游调用次数上限（含首次）
///
/// 允许最多 3 次尝试（含首选账号与 2 次跨账号故障转移）。超过即视为“跨账号重放放大”，
/// 直接终态失败，交由客户端决定是否重试。
pub const MAX_LARGE_BODY_UPSTREAM_CALLS: usize = 3;

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

    /// 派生一个“共享截止时刻、但独立计数/独立估算”的子上下文
    ///
    /// 用于同一请求内并行的子任务（例如一次生成 n 张图片）：它们共享同一个 270s 总预算，
    /// 但各自是不同的 body，不能共用「大 body 重放次数」计数器——
    /// 否则正常的并行会被误判成“重放放大”，第 3 张图就会被护栏拦下。
    pub fn fork(&self) -> Arc<Self> {
        Arc::new(Self {
            deadline: self.deadline,
            upstream_calls: AtomicUsize::new(0),
            estimated_tokens: AtomicU32::new(self.estimated_tokens.load(Ordering::Relaxed)),
        })
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

/// 在「可选」上下文上挂载请求预算（`None` 时直接执行）
///
/// task_local 不会跨 `tokio::spawn` 传递：图片生成等 `JoinSet` 子任务必须在**spawn 前**
/// 捕获 `current()`，再在子任务内用本函数恢复，否则既看不到截止时刻，也不会被重放护栏计数。
pub async fn scope_optional<F: std::future::Future>(
    ctx: Option<Arc<RequestContext>>,
    fut: F,
) -> F::Output {
    match ctx {
        Some(ctx) => scope(ctx, fut).await,
        None => fut.await,
    }
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

/// 把「响应头之后仍要等上游」的某个阶段超时夹到本请求剩余预算内（预留 2s 收尾余量）
///
/// 用于 peek 首个 chunk 之类的二次等待：这些阶段如果不受全局预算约束，
/// 「270s 总预算」就只能约束单次调用，实际总时长可以叠加到 270s 之外。
pub fn clamp_secs(budget_secs: u64) -> u64 {
    match remaining_secs() {
        Some(remaining) => {
            budget_secs.min(remaining.saturating_sub(2).max(MIN_ATTEMPT_BUDGET_SECS))
        }
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

/// 读取当前生效的大 body 判定门限（优先遵从用户在 GUI 配置的 large_body_threshold_mb，默认保底 4MB）
pub fn replay_guard_threshold_bytes() -> usize {
    let cfg_bytes = crate::proxy::get_upload_timeout_config().large_body_threshold_bytes();
    if cfg_bytes > 0 {
        cfg_bytes
    } else {
        REPLAY_GUARD_BODY_BYTES
    }
}

/// 登记一次上游调用，并在“大 body + 超出重放上限”时拒绝
///
/// 返回 `Err(已发生的调用次数)` 表示本次调用被护栏拦下。
pub fn register_upstream_call(body_len: usize) -> Result<(), usize> {
    let Some(ctx) = current() else {
        return Ok(());
    };
    let call_no = ctx.register_upstream_call();
    let threshold = replay_guard_threshold_bytes();
    if body_len >= threshold && call_no > MAX_LARGE_BODY_UPSTREAM_CALLS {
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
            // 大 body：前 MAX_LARGE_BODY_UPSTREAM_CALLS 次放行，超额则拒绝
            for _ in 0..MAX_LARGE_BODY_UPSTREAM_CALLS {
                assert!(register_upstream_call(REPLAY_GUARD_BODY_BYTES).is_ok());
            }
            assert_eq!(
                register_upstream_call(REPLAY_GUARD_BODY_BYTES),
                Err(MAX_LARGE_BODY_UPSTREAM_CALLS + 1),
                "large-body upstream call beyond limit must be blocked"
            );
        })
        .await;

        scope(RequestContext::new(deadline_now()), async {
            // 小 body（如低于 4MB 的常规请求）：不设次数上限
            for _ in 0..20 {
                assert!(register_upstream_call(1024).is_ok());
            }
        })
        .await;
    }

    #[tokio::test]
    async fn fork_shares_deadline_but_resets_replay_counter() {
        // 并行子任务（如一次生成 n 张图）共享同一个截止时刻，但重放计数必须各算各的：
        // 否则第 4 张图就会被“大 body 重放上限”误拦。
        let parent = RequestContext::new(deadline_now());
        let child = parent.fork();
        assert_eq!(child.deadline, parent.deadline);

        scope(parent, async move {
            for _ in 0..MAX_LARGE_BODY_UPSTREAM_CALLS {
                assert!(register_upstream_call(REPLAY_GUARD_BODY_BYTES).is_ok());
            }
            assert!(register_upstream_call(REPLAY_GUARD_BODY_BYTES).is_err());

            scope(child, async {
                for _ in 0..MAX_LARGE_BODY_UPSTREAM_CALLS {
                    assert!(register_upstream_call(REPLAY_GUARD_BODY_BYTES).is_ok());
                }
            })
            .await;
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
