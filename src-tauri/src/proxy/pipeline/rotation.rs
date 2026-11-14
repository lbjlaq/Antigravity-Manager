//! 请求级「重试 / 退避 / 轮换」裁决（协议无关的 Pipeline 横切能力）
//!
//! # 为什么放在 Pipeline
//!
//! 四个协议适配器（OpenAI Responses / OpenAI Chat Completions / Anthropic Claude /
//! Google Gemini）在拿到上游失败结果后，回答的都是同一组问题：
//!
//! 1. 这个状态码值不值得重试？退避多久？
//! 2. 应该原地重试（Grace Retry）还是换账号？
//! 3. 单账号池与多账号池的差异如何统一？
//!
//! 这些判断只依赖「状态码 + 错误文本 + 池规模 + 已重试标记」，与协议线缆格式无关，
//! 因此统一收敛到这里。适配器只负责把错误文本交进来、按 [`RetryStrategy`] 执行副作用
//! （罚号、解绑粘性、丢弃图片许可），不再各自维护一份退避表。
//!
//! # 兼容入口
//!
//! `handlers::common` 仍保留同名再导出，避免一次性改动全部调用点；
//! 新代码请直接引用本模块。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::proxy::pipeline::policy::{Structured429Signal, UpstreamClassification};
use crate::proxy::pipeline::terminal::TerminalNextStep;
use crate::proxy::sticky_config::SchedulingMode;
use tokio::time::sleep;
use tracing::{debug, info};

/// 请求级 429（无 retryDelay）首轮原地 GraceRetry 等待时间（毫秒）。
static REQUEST_429_GRACE_MS: AtomicU64 = AtomicU64::new(1500);

/// 更新请求级 429 GraceRetry 等待时间（由调度配置热更新）。
pub fn set_request_429_grace_ms(ms: u64) {
    REQUEST_429_GRACE_MS.store(ms.clamp(200, 10_000), Ordering::Relaxed);
}

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

/// CacheFirst 模式下对上游瞬态服务端错误（500/503/529）的单请求同账号重试上限（不含首次请求）。
const CACHE_FIRST_SERVER_ERROR_MAX_RETRIES: usize = 2;

/// CacheFirst 模式下对可恢复 429 限流的单请求同账号原地等待重试上限（不含首次请求）。
const CACHE_FIRST_RATE_LIMIT_MAX_RETRIES: usize = 2;

/// 一次非 2xx 上游 HTTP 响应的最终执行计划（由流水线按调度模式统一裁决）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRetryPlan {
    pub strategy: RetryStrategy,
    pub next: TerminalNextStep,
    /// 是否解绑当前会话与该账号的粘性绑定
    pub abandon_sticky_account: bool,
}

#[derive(Debug, Default)]
pub struct RequestRetryState {
    grace_retried_accounts: HashSet<String>,
    /// 已在本次请求内因上游终态原地重放过的账号（每账号每请求至多一次）
    terminal_retried_accounts: HashSet<String>,
    /// CacheFirst 模式下各账号在本次请求内因瞬态服务端错误（500/503/529）已原地重试的次数
    server_error_retries: HashMap<String, usize>,
    /// CacheFirst 模式下各账号在本次请求内因可恢复 429 已原地等待重试的次数
    rate_limit_retries: HashMap<String, usize>,
}

impl RequestRetryState {
    /// 上游终态（首字节 / 上传 / 传输层断连 / 全局预算 / 重放上限）后的统一裁决
    ///
    /// 四个协议适配器共用：调度模式只在这里参与决策，适配器只执行返回的计划。
    /// - `mode`：当前调度模式；
    /// - `cache_bound`：请求是否承载上游 KV 缓存（文本对话为真；生图为假，
    ///   不走原地重放，CacheFirst 下换号重试但不解绑原会话绑定）。
    pub fn plan_terminal(
        &mut self,
        account_id: &str,
        disposition: crate::proxy::pipeline::terminal::TerminalDisposition,
        mode: crate::proxy::sticky_config::SchedulingMode,
        cache_bound: bool,
    ) -> crate::proxy::pipeline::terminal::TerminalRetryPlan {
        let retry_left = !self.terminal_retried_accounts.contains(account_id);
        let plan = disposition.plan(mode, cache_bound, retry_left);
        if plan.next == crate::proxy::pipeline::terminal::TerminalNextStep::RetrySameAccount {
            self.terminal_retried_accounts
                .insert(account_id.to_string());
        }
        plan
    }

    /// 非 2xx HTTP 响应的统一流水线重试/轮换裁决（四协议共用）
    ///
    /// - 在 `CacheFirst` 且承载上下文 KV 缓存（`cache_bound == true`）时：
    ///   - 瞬态服务端错误（500/503/529）不属于账号故障：绝不解绑、绝不换号，在当前绑定账号上按单账号退避
    ///     最多重试 `CACHE_FIRST_SERVER_ERROR_MAX_RETRIES` 次（共 3 次尝试），耗尽后直接终止并保留绑定；
    ///   - 429 仅在硬配额枯竭或冷却超过 `max_wait_seconds` 时解绑换号；否则在 `max_wait_seconds` 与剩余预算内
    ///     原地等待并继续复用原绑定账号，绝不丢弃 KV 缓存；
    ///   - 401/403/404 与 400 地区限制等账号级/端点级异常保持正常解绑/轮换语义。
    /// - 其余调度模式或无缓存请求（如生图 `cache_bound == false`）严格保持既有自适应策略与轮换逻辑。
    #[allow(clippy::too_many_arguments)]
    pub fn plan_http_retry(
        &mut self,
        account_id: &str,
        status_code: u16,
        error_text: &str,
        retry_after: Option<&str>,
        retried_without_thinking: bool,
        attempt: usize,
        pool_size: usize,
        mode: SchedulingMode,
        cache_bound: bool,
        max_wait_seconds: u64,
        allow_grace_policy: bool,
        classification: &UpstreamClassification,
    ) -> HttpRetryPlan {
        if mode == SchedulingMode::CacheFirst && cache_bound {
            if matches!(status_code, 500 | 503 | 529)
                && !is_model_not_found_error(status_code, error_text)
            {
                let retries_used = self
                    .server_error_retries
                    .get(account_id)
                    .copied()
                    .unwrap_or(0);
                let delay_ms = match status_code {
                    500 => 1000 * (retries_used as u64 + 1),
                    _ => (1000 * 2_u64.pow(retries_used as u32)).min(4000),
                };
                let budget_ok = crate::proxy::pipeline::budget::remaining_secs()
                    .map(|rem| rem > (delay_ms / 1000) + 2)
                    .unwrap_or(true);
                if retries_used < CACHE_FIRST_SERVER_ERROR_MAX_RETRIES && budget_ok {
                    *self
                        .server_error_retries
                        .entry(account_id.to_string())
                        .or_insert(0) += 1;
                    return HttpRetryPlan {
                        strategy: RetryStrategy::FixedDelay(Duration::from_millis(delay_ms)),
                        next: TerminalNextStep::RetrySameAccount,
                        abandon_sticky_account: false,
                    };
                }
                return HttpRetryPlan {
                    strategy: RetryStrategy::NoRetry,
                    next: TerminalNextStep::Stop,
                    abandon_sticky_account: false,
                };
            }

            if status_code == 429 {
                let parsed_delay = crate::proxy::upstream::retry::parse_retry_delay_with_source(
                    error_text,
                    retry_after,
                );
                let is_hard_quota = is_hard_quota_429(error_text, parsed_delay.as_ref());
                let max_wait_ms = max_wait_seconds.saturating_mul(1000);
                let delay_exceeds_max_wait = parsed_delay
                    .as_ref()
                    .map(|d| max_wait_seconds == 0 || d.actual_wait_ms() > max_wait_ms)
                    .unwrap_or(false);

                if !is_hard_quota && !delay_exceeds_max_wait {
                    let retries_used = self
                        .rate_limit_retries
                        .get(account_id)
                        .copied()
                        .unwrap_or(0);
                    let wait_ms = if let Some(delay) = parsed_delay.as_ref() {
                        delay.actual_wait_ms()
                    } else if retries_used == 0 {
                        REQUEST_429_GRACE_MS.load(Ordering::Relaxed)
                    } else {
                        3000
                    };
                    let budget_ok = crate::proxy::pipeline::budget::remaining_secs()
                        .map(|rem| rem > (wait_ms / 1000) + 2)
                        .unwrap_or(true);
                    if retries_used < CACHE_FIRST_RATE_LIMIT_MAX_RETRIES && budget_ok {
                        *self
                            .rate_limit_retries
                            .entry(account_id.to_string())
                            .or_insert(0) += 1;
                        self.grace_retried_accounts.insert(account_id.to_string());
                        return HttpRetryPlan {
                            strategy: RetryStrategy::GraceRetry(Duration::from_millis(wait_ms)),
                            next: TerminalNextStep::RetrySameAccount,
                            abandon_sticky_account: false,
                        };
                    }
                    return HttpRetryPlan {
                        strategy: RetryStrategy::NoRetry,
                        next: TerminalNextStep::Stop,
                        abandon_sticky_account: false,
                    };
                }
            }
        }

        let strategy = self.determine_strategy_adaptive_with_policy(
            account_id,
            status_code,
            error_text,
            retry_after,
            retried_without_thinking,
            attempt,
            pool_size,
            allow_grace_policy,
        );
        let next = match decide_rotation(status_code, Some(&strategy), false, false) {
            RotationDecision::RetrySame => TerminalNextStep::RetrySameAccount,
            RotationDecision::Rotate => TerminalNextStep::Rotate,
            RotationDecision::Stop => TerminalNextStep::Stop,
        };
        HttpRetryPlan {
            strategy,
            next,
            abandon_sticky_account: next != TerminalNextStep::RetrySameAccount
                && classification.abandons_sticky_account(),
        }
    }

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
        self.determine_strategy_adaptive_with_policy(
            account_id,
            status_code,
            error_text,
            retry_after,
            retried_without_thinking,
            attempt,
            pool_size,
            true,
        )
    }

    /// 是否允许在当前账号「原地等待」（调度模式策略由调用方注入）
    ///
    /// 与 [`Self::determine_strategy_adaptive`] 的唯一差异是 `allow_grace_policy`：
    /// 它只表达调度模式（CacheFirst / Balance / PerformanceFirst）是否接受原地等待，
    /// 不改变各状态码的档位表。四个协议共用同一份「每账号一次 GraceRetry」记忆，
    /// 保证同一账号在一次客户端请求内不会被反复 GraceRetry 直到全局预算耗尽。
    #[allow(clippy::too_many_arguments)]
    pub fn determine_strategy_adaptive_with_policy(
        &mut self,
        account_id: &str,
        status_code: u16,
        error_text: &str,
        retry_after: Option<&str>,
        retried_without_thinking: bool,
        attempt: usize,
        pool_size: usize,
        allow_grace_policy: bool,
    ) -> RetryStrategy {
        let allow_grace_retry =
            allow_grace_policy && !self.grace_retried_accounts.contains(account_id);
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

/// 调度模式是否允许「在当前账号原地等待」（GraceRetry）
///
/// 这是四个协议共享的唯一判定，避免每个适配器各写一份 `match SchedulingMode`：
/// - `CacheFirst`：优先保缓存，允许等待；
/// - `Balance`：只有单账号时不得不等，多账号立即换号；
/// - `PerformanceFirst`：拒绝等待，直接换号/退避。
///
/// 注意：它只决定“是否允许原地等待”，不改变状态码档位表；多账号 429 的微间隙
/// GraceRetry 仍由各自的 `RequestRetryState` 保证每账号一次。
pub fn scheduling_allows_grace(
    mode: crate::proxy::sticky_config::SchedulingMode,
    pool_size: usize,
) -> bool {
    use crate::proxy::sticky_config::SchedulingMode;
    match mode {
        SchedulingMode::CacheFirst => true,
        SchedulingMode::Balance => pool_size <= 1,
        SchedulingMode::PerformanceFirst => false,
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
        // 400 错误：
        // 1. 若为 Sandbox 地域限制 (User location is not supported)，允许在多账号/端点间快速重试逃逸至可用节点 (Prod)
        400 if lower.contains("user location is not supported") && attempt < pool_size => {
            tracing::warn!(
                "[Retry] 400 User location is not supported on attempt {}/{}; fast rotating to fallback endpoint/account",
                attempt + 1,
                pool_size
            );
            RetryStrategy::FixedDelay(Duration::from_millis(50))
        }

        // 2. 仅在特定 Thinking 签名失败时重试一次（判据统一由 pipeline::policy 提供）
        400 if !retried_without_thinking
            && crate::proxy::pipeline::UpstreamClassification::classify(400, error_text, None)
                .is_thought_signature_error() =>
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
            // 但若明确包含 multi-day/跨小时超长重置延迟 (如 quotaResetDelay: 68h / retryDelay > 30min)，则明确属于周期硬配额耗尽。
            let is_hard_quota_exhausted = is_hard_quota_429(error_text, parsed_delay.as_ref());

            // 多账号环境下，账号级硬配额枯竭在第一轮立即切换下一个账号（50ms 闪电切号，避免 CPU 空转并快速恢复响应）
            if is_hard_quota_exhausted && attempt < pool_size {
                tracing::info!(
                    "[Retry] Account hard quota exhausted detected on attempt {}/{}; fast rotating to next account in pool (50ms)",
                    attempt + 1,
                    pool_size
                );
                return RetryStrategy::FixedDelay(Duration::from_millis(50));
            }

            // 请求级 429 防早退与极速失败保护 (无明确重置时间且非账号硬配额耗尽)：
            // 在多账号池中，第一轮 (attempt < pool_size) 允许全池每一个可用健康账号均获得 1 次 150ms 执行机会，
            // 并支持第二轮 (pool_size <= attempt < pool_size * 2) 线性退避 (1.5s~3.5s) 恢复重试，
            // 解决大 Prompt 突发并发导致瞬态 429 时全池健康账号被 150ms 闪电过早误杀退出（Fixes #3506）。
            // 仅当第二轮全池遍历耗尽（attempt >= pool_size * 2）仍 429 时，立即快速失败退出 (NoRetry)。
            let is_request_level_429 = parsed_delay.is_none() && !is_hard_quota_exhausted;
            let request_level_429_max_attempts = (pool_size * 2).max(2);
            if is_request_level_429 && attempt >= request_level_429_max_attempts {
                tracing::warn!(
                    "[Retry] Request-level 429 persisted across 2 rounds ({} attempts); aborting to prevent excessive client stalls.",
                    attempt
                );
                return RetryStrategy::NoRetry;
            }

            // [Fixes #3901] 无 retryDelay 的请求级 429：首轮先原地短退避，严禁 150ms 全池轮换。
            // 否则一个小账号池会在数百毫秒内被逐一写入 5s/15s/30s 冷却，演变为全池连坐 503。
            if is_request_level_429 && attempt == 0 && allow_grace_retry {
                let grace_ms = REQUEST_429_GRACE_MS.load(Ordering::Relaxed);
                tracing::info!(
                    "[Retry] Request-level 429 without retryDelay: GraceRetry on current account ({}ms) to protect the pool",
                    grace_ms
                );
                return RetryStrategy::GraceRetry(Duration::from_millis(grace_ms));
            }

            // [FIX #3329, #3409] 关键防线：微间隙优先原地 GraceRetry (Micro-gap Grace Retry First)
            // 若上游返回了明确且极短的重置延迟 (<= 2500ms，例如 Google 图像生成典型的 1.17s quotaResetDelay)
            // 且该账号本轮尚未执行过 GraceRetry：
            // 绝不能在第 1 轮就无脑 50ms 闪电切号！
            // 因为 50ms 闪电切号会在 300ms 内把全池所有账号全部轮询打一遍，并将它们全部作为 RateLimitExceeded 封锁进入 RateLimitTracker，
            // 导致所有账号在第 2 轮全部处于限流冷却中并抛出 503 "All accounts limited"（生图成功率骤降至 13% 的根因）。
            // 优先在当前账号原地等待 actual_ms (如 1.37s) 并在上游配额刚恢复的瞬间重试，即可原地满血复活，完全避免全池被连坐封死！
            if let Some(delay) = parsed_delay {
                let actual_ms = delay.actual_wait_ms();
                if actual_ms <= 2500 && allow_grace_retry {
                    tracing::info!(
                        "[Retry] Multi-account 429 micro-gap ({}ms <= 2500ms): applying GraceRetry on current account instead of fast-rotating to protect pool",
                        actual_ms
                    );
                    return RetryStrategy::GraceRetry(Duration::from_millis(actual_ms));
                }
            }

            let is_first_round = attempt < pool_size;
            if is_first_round {
                // Round 1 (第一轮)：全池匀速轮换 (150ms)，留出微冷却时间，毫秒级逃逸至其他健康账号
                tracing::info!(
                    "[Retry] Multi-account Round 1 (attempt {}/{}): rotating to next account in pool (150ms)",
                    attempt + 1, pool_size
                );
                return RetryStrategy::FixedDelay(Duration::from_millis(150));
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

            // 第二轮未解析出具体 delay：线性温和退避 1.5s~3.5s，让瞬时并发与 TPM 高峰平息冷却
            let step = (attempt.saturating_sub(pool_size)) as u64;
            let backoff_ms = (1500 + step * 1000).min(3500);
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
                // 单账号或已遍历全池: 适度温和退避 (起始 1s，上限 4s，杜绝单次重试死等 30s 阻塞网关)
                RetryStrategy::ExponentialBackoff {
                    base_ms: 1000,
                    max_ms: 4000,
                }
            }
        }

        // 500 服务器内部错误
        500 => {
            // 线性退避：起始 1s，上限 3s
            RetryStrategy::LinearBackoff { base_ms: 1000 }
        }

        // 401/403 认证/权限错误：切换账号前给予极短缓冲
        401 | 403 => RetryStrategy::FixedDelay(Duration::from_millis(200)),

        // 其他错误：不重试
        _ => RetryStrategy::NoRetry,
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

/// 单次尝试后的统一处置：重试同一账号 / 换号 / 终止
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationDecision {
    /// 原地等待后重试同一账号（GraceRetry / 400 签名修复 / 自动自愈）
    RetrySame,
    /// 换号或换端点重放
    Rotate,
    /// 不再重试，按终态或最后一次错误返回
    Stop,
}

/// 统一裁决：终态 > 自愈 > 策略（Grace/换号）> 终止
pub fn decide_rotation(
    status_code: u16,
    strategy: Option<&RetryStrategy>,
    terminal_stop: bool,
    auto_heal_pending: bool,
) -> RotationDecision {
    if terminal_stop {
        return RotationDecision::Stop;
    }
    if auto_heal_pending {
        return RotationDecision::RetrySame;
    }
    match strategy {
        Some(RetryStrategy::GraceRetry(_)) => RotationDecision::RetrySame,
        Some(strategy) if !matches!(strategy, RetryStrategy::NoRetry) => {
            if should_rotate_account(status_code, Some(strategy)) {
                RotationDecision::Rotate
            } else {
                RotationDecision::RetrySame
            }
        }
        _ => RotationDecision::Stop,
    }
}

/// 判断是否应该轮换账号
///
/// [MODEL GATE] 网关自答（404 `model not exist` / 502 `no quota accounts`）永远不会到达这里：
/// `enforce_model_gate` 在选号之前就返回，且 `UpstreamClassification::classify` 优先把它们
/// 归为 `InternalGatewayMessage`（既不锁号也不解绑粘性会话）。此注释用于防止后续重构破防。
fn is_hard_quota_429(
    error_text: &str,
    parsed_delay: Option<&crate::proxy::upstream::retry::ParsedRetryDelay>,
) -> bool {
    let is_long_delay = parsed_delay
        .map(|d| d.actual_wait_ms() > 1_800_000)
        .unwrap_or(false);
    // [Fixes #3901] 结构化 429 信号优先于消息关键词：
    // - QuotaFailure → 账号级硬配额，快速切号；
    // - RetryInfo / 请求级 / 未知 details → 不作为硬配额，交给后续 GraceRetry/轮换逻辑；
    // - 无结构化 details 时才回退到关键词表。
    match UpstreamClassification::structured_429_signal(error_text) {
        Some(Structured429Signal::HardQuota) => true,
        Some(Structured429Signal::RequestLevel)
        | Some(Structured429Signal::ExplicitDelay)
        | Some(Structured429Signal::Unknown) => false,
        None => {
            let lower = error_text.to_lowercase();
            (parsed_delay.is_none() || is_long_delay)
                && (lower.contains("exceeded your current quota")
                    || lower.contains("insufficient_quota")
                    || lower.contains("credits")
                    || lower.contains("zero_quota")
                    || lower.contains("weekly quota")
                    || lower.contains("daily quota")
                    || lower.contains("per day")
                    || lower.contains("individual quota")
                    || lower.contains("quotaresetdelay")
                    || (lower.contains("quota_exhausted")
                        && (!lower.contains("resource has been exhausted")
                            || lower.contains("quotaresetdelay"))))
        }
    }
}

pub fn should_rotate_account(status_code: u16, strategy: Option<&RetryStrategy>) -> bool {
    // [NEW] 如果识别为 Grace Retry，则显式要求不轮换账号
    if let Some(RetryStrategy::GraceRetry(_)) = strategy {
        return false;
    }

    match status_code {
        // 这些错误是账号级别或特定节点配额的，需要轮换
        // [FIX #3485] 503/529 边缘节点熔断或特定账号负载过高，多账号时支持轮换逃逸，杜绝死锁死等
        // 注意：404 不在此列——它已被 `is_model_not_found_error` 前置归为终态（NoRetry），
        // 账号级灰度 / 权限不同步由 `policy::apply_model_not_found` 熔断单模型并换号承担。
        429 | 401 | 403 | 500 | 503 | 529 => true,
        // 若为 400 且返回了 50ms 快速退避策略（例如 user location is not supported），需要轮换账号/节点
        400 if matches!(strategy, Some(RetryStrategy::FixedDelay(d)) if *d == Duration::from_millis(50)) => {
            true
        }
        _ => false,
    }
}

/// [Issue #3414] 从形如 "All accounts limited. Wait 29s." 或其他明确冷却提示中解析等待秒数
pub fn extract_retry_after_seconds(error_text: &str) -> Option<u64> {
    // 查找 "Wait " (大写) 或 "wait " (小写)
    let pos = error_text
        .find("Wait ")
        .or_else(|| error_text.find("wait "))?;
    let rest = &error_text[pos + 5..];
    if let Some(s_pos) = rest.find('s') {
        if let Ok(sec) = rest[..s_pos].trim().parse::<u64>() {
            if sec > 0 {
                return Some(sec);
            }
        }
    }
    None
}

/// [Issue #3634] 为 Claude 协议收敛 Retry-After 秒数。
/// Claude Code 客户端对 Retry-After 设置了严格的 20,000ms (20s) 门禁：
/// 1. 若 Retry-After >= 20s 或缺失 (null)，立即触发 10~30 分钟 (1,800,000ms) 的过载/限流冷冻与 fastMode 熔断；
/// 2. 若 Retry-After < 20s (且非 null)，客户端执行快速原位重试循环 (Math.min(U$(...), 20000))。
///
/// 因此网关为 Claude 协议返回的 Retry-After 必须稳定收敛至 safe 范围 (默认 12s，兜底 8s)。
pub fn clamp_retry_after_for_claude(sec: Option<u64>) -> u64 {
    match sec {
        Some(s) if s > 0 => s.clamp(1, 12),
        _ => 8,
    }
}

/// 判断是否为模型不存在/不支持的错误
///
/// 判据统一由 [`crate::proxy::pipeline::UpstreamClassification`] 提供；
/// 网关自答（model not exist）也归为不可重试，但不属于“上游模型不存在”。
///
/// 注意：任何 404 都会归类为 `ModelNotFound` → 本函数返回 true →
/// [`determine_retry_strategy_adaptive`] 直接 `NoRetry`。账号级灰度/权限不同步的 404
/// 不再做 300ms 原地重试，而是由 `policy::apply_model_not_found` 标记单模型熔断并换号，
/// 号池轮完后终止，避免对同一份 body 反复 hammer。
pub fn is_model_not_found_error(status: u16, body: &str) -> bool {
    if crate::proxy::common::model_gate::is_gateway_self_answer(body) {
        return true;
    }
    crate::proxy::pipeline::UpstreamClassification::classify(status, body, None)
        .is_model_not_found()
}

/// 记录已尝试账号并触发强制轮换，在账号池全部尝试过时重置排除集合，
/// 确保后续轮换尝试可以重新考虑号池中的可用账号。
pub fn record_attempted_account(
    attempted_accounts: &mut HashSet<String>,
    account_id: &str,
    pool_size: usize,
    force_rotate: &mut bool,
) {
    *force_rotate = true;
    if pool_size > 1 && attempted_accounts.len() < pool_size {
        attempted_accounts.insert(account_id.to_string());
    } else if pool_size > 1 {
        attempted_accounts.clear();
        attempted_accounts.insert(account_id.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_retries_same_account_only_once_per_request() {
        use crate::proxy::pipeline::terminal::{TerminalNextStep, UpstreamTerminal};
        use crate::proxy::sticky_config::SchedulingMode;
        let d = UpstreamTerminal::TransportReset.disposition();

        // CacheFirst：原地重放一次，之后终止，全程绝不解绑、绝不换号
        let mut state = RequestRetryState::default();
        let first = state.plan_terminal("acc-1", d, SchedulingMode::CacheFirst, true);
        assert_eq!(first.next, TerminalNextStep::RetrySameAccount);
        assert!(!first.abandon_sticky_account);
        let second = state.plan_terminal("acc-1", d, SchedulingMode::CacheFirst, true);
        assert_eq!(second.next, TerminalNextStep::Stop);
        assert!(!second.abandon_sticky_account);

        // CacheFirst 生图：不原地重放，换号但保留原会话绑定
        let mut state = RequestRetryState::default();
        let image = state.plan_terminal("acc-1", d, SchedulingMode::CacheFirst, false);
        assert_eq!(image.next, TerminalNextStep::Rotate);
        assert!(!image.abandon_sticky_account);

        // Balance：传输层断连先原地重连一次，用完后回到解绑 + 换号
        let mut state = RequestRetryState::default();
        let first = state.plan_terminal("acc-1", d, SchedulingMode::Balance, true);
        assert_eq!(first.next, TerminalNextStep::RetrySameAccount);
        assert!(!first.abandon_sticky_account);
        let second = state.plan_terminal("acc-1", d, SchedulingMode::Balance, true);
        assert_eq!(second.next, TerminalNextStep::Rotate);
        assert!(second.abandon_sticky_account);
        // 换到新账号后仍各自拥有一次原地重连额度
        let other = state.plan_terminal("acc-2", d, SchedulingMode::Balance, true);
        assert_eq!(other.next, TerminalNextStep::RetrySameAccount);

        // PerformanceFirst：不建立会话绑定，传输层断连直接换号（既有语义）
        let mut state = RequestRetryState::default();
        let perf = state.plan_terminal("acc-1", d, SchedulingMode::PerformanceFirst, true);
        assert_eq!(perf.next, TerminalNextStep::Rotate);
    }

    #[test]
    fn any_404_is_terminal_and_never_retried() {
        // 404 无论是否携带文本提示，都统一按 ModelNotFound 处理：
        // 不再做 300ms 原地重试，账号级灰度由 policy 熔断 + 换号承担。
        for body in ["", "model not found", "account permission not synced"] {
            assert!(
                is_model_not_found_error(404, body),
                "404 must classify as model-not-found: {body:?}"
            );
            assert_eq!(
                determine_retry_strategy_adaptive(404, body, None, false, true, 0, 3),
                RetryStrategy::NoRetry,
                "404 must not rotate/retry: {body:?}"
            );
        }

        // 网关自答同样不重试
        assert_eq!(
            determine_retry_strategy_adaptive(502, "no quota accounts", None, false, true, 0, 3),
            RetryStrategy::NoRetry
        );
    }

    #[test]
    fn terminal_statuses_never_rotate() {
        // 404 已是终态：轮换判定函数不得再把它归入“需要换号”，
        // 以免未来在 404 重新可重试时意外变成“换号但不熔断”的半套行为。
        assert!(!should_rotate_account(404, None));
        // 其余账号级错误仍保持换号语义
        for status in [429u16, 401, 403, 500, 503, 529] {
            assert!(should_rotate_account(status, None), "{status}");
        }
    }

    #[test]
    fn decide_rotation_prefers_terminal_and_autoheal() {
        let grace = RetryStrategy::GraceRetry(Duration::from_secs(3));
        assert_eq!(
            decide_rotation(429, Some(&grace), true, false),
            RotationDecision::Stop
        );
        assert_eq!(
            decide_rotation(429, Some(&grace), false, true),
            RotationDecision::RetrySame
        );
        assert_eq!(
            decide_rotation(429, Some(&grace), false, false),
            RotationDecision::RetrySame
        );
        // 多账号 429 固定退避 → 换号
        let fixed = RetryStrategy::FixedDelay(Duration::from_millis(50));
        assert_eq!(
            decide_rotation(429, Some(&fixed), false, false),
            RotationDecision::Rotate
        );
        // 400 签名修复 (200ms) → 同号重试
        let sig_fixed = RetryStrategy::FixedDelay(Duration::from_millis(200));
        assert_eq!(
            decide_rotation(400, Some(&sig_fixed), false, false),
            RotationDecision::RetrySame
        );
        // NoRetry / None → 终止
        assert_eq!(
            decide_rotation(404, Some(&RetryStrategy::NoRetry), false, false),
            RotationDecision::Stop
        );
        assert_eq!(
            decide_rotation(502, None, false, false),
            RotationDecision::Stop
        );
    }

    #[test]
    fn grace_is_bounded_to_one_per_account_per_request() {
        // 模拟 Claude 旧缺陷：无状态裁决会让同一账号在每次 429 上反复 GraceRetry。
        // 统一到 RequestRetryState 后，同一账号第二次 429 必须降级为普通退避。
        let body = r#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"1s"}]}}"#;
        let mut state = RequestRetryState::default();

        let first = state
            .determine_strategy_adaptive_with_policy("acct-1", 429, body, None, false, 0, 1, true);
        assert!(
            matches!(first, RetryStrategy::GraceRetry(d) if d == Duration::from_millis(1200)),
            "first 429 must grace once with 200ms structured buffer, got {first:?}"
        );
        assert!(!should_rotate_account(429, Some(&first)));

        let second = state
            .determine_strategy_adaptive_with_policy("acct-1", 429, body, None, false, 0, 1, true);
        assert!(
            !matches!(second, RetryStrategy::GraceRetry(_)),
            "same account must not grace twice, got {second:?}"
        );

        // 另一个账号拥有独立的 Grace 额度
        let other = state
            .determine_strategy_adaptive_with_policy("acct-2", 429, body, None, false, 0, 1, true);
        assert!(matches!(other, RetryStrategy::GraceRetry(_)));
    }

    #[test]
    fn grace_policy_only_gates_waiting_not_the_tier_table() {
        let body = r#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"1s"}]}}"#;
        let mut state = RequestRetryState::default();

        // PerformanceFirst 等策略禁止原地等待：首个 429 也不得 GraceRetry
        let no_wait = state
            .determine_strategy_adaptive_with_policy("acct-1", 429, body, None, false, 0, 1, false);
        assert!(
            matches!(no_wait, RetryStrategy::FixedDelay(_)),
            "policy=false must not grace, got {no_wait:?}"
        );

        // 与 Grace 无关的档位不受策略影响
        assert_eq!(
            determine_retry_strategy_adaptive(503, "", None, false, true, 0, 1),
            RetryStrategy::ExponentialBackoff {
                base_ms: 1000,
                max_ms: 4000
            }
        );
    }

    #[test]
    fn record_attempted_account_manages_pool_rotation_and_reset() {
        let mut attempted = HashSet::new();
        let mut force_rotate = false;
        let pool_size = 3;

        record_attempted_account(&mut attempted, "acc-1", pool_size, &mut force_rotate);
        assert!(force_rotate);
        assert_eq!(attempted.len(), 1);
        assert!(attempted.contains("acc-1"));

        record_attempted_account(&mut attempted, "acc-2", pool_size, &mut force_rotate);
        assert_eq!(attempted.len(), 2);
        assert!(attempted.contains("acc-2"));

        record_attempted_account(&mut attempted, "acc-3", pool_size, &mut force_rotate);
        assert_eq!(attempted.len(), 3);

        // When attempted accounts count hits or exceeds pool_size, it resets and retains current account
        record_attempted_account(&mut attempted, "acc-3", pool_size, &mut force_rotate);
        assert_eq!(attempted.len(), 1);
        assert!(attempted.contains("acc-3"));
    }

    #[test]
    fn cache_first_text_keeps_bound_account_on_transient_500_503_529() {
        let mut state = RequestRetryState::default();
        for status in [500_u16, 503, 529] {
            let acc = format!("acc-{status}");
            let cls = UpstreamClassification::classify(status, "upstream error", None);
            let first = state.plan_http_retry(
                &acc,
                status,
                "upstream error",
                None,
                false,
                0,
                6,
                SchedulingMode::CacheFirst,
                true,
                180,
                true,
                &cls,
            );
            assert_eq!(first.next, TerminalNextStep::RetrySameAccount);
            assert!(!first.abandon_sticky_account);
            assert!(matches!(
                first.strategy,
                RetryStrategy::FixedDelay(d) if d == Duration::from_millis(1000)
            ));

            let second = state.plan_http_retry(
                &acc,
                status,
                "upstream error",
                None,
                false,
                0,
                6,
                SchedulingMode::CacheFirst,
                true,
                180,
                true,
                &cls,
            );
            assert_eq!(second.next, TerminalNextStep::RetrySameAccount);
            assert!(!second.abandon_sticky_account);
            assert!(matches!(
                second.strategy,
                RetryStrategy::FixedDelay(d) if d == Duration::from_millis(2000)
            ));

            let exhausted = state.plan_http_retry(
                &acc,
                status,
                "upstream error",
                None,
                false,
                0,
                6,
                SchedulingMode::CacheFirst,
                true,
                180,
                true,
                &cls,
            );
            assert_eq!(exhausted.next, TerminalNextStep::Stop);
            assert!(!exhausted.abandon_sticky_account);
            assert!(matches!(exhausted.strategy, RetryStrategy::NoRetry));
        }
    }

    #[test]
    fn balance_and_cache_first_image_still_rotate_on_500_503() {
        let mut state = RequestRetryState::default();
        let cls = UpstreamClassification::classify(500, "internal error", None);

        let balance_plan = state.plan_http_retry(
            "acc-1",
            500,
            "internal error",
            None,
            false,
            0,
            6,
            SchedulingMode::Balance,
            true,
            180,
            true,
            &cls,
        );
        assert_eq!(balance_plan.next, TerminalNextStep::Rotate);
        assert!(!balance_plan.abandon_sticky_account);

        let image_plan = state.plan_http_retry(
            "acc-1",
            500,
            "internal error",
            None,
            false,
            0,
            6,
            SchedulingMode::CacheFirst,
            false,
            180,
            true,
            &cls,
        );
        assert_eq!(image_plan.next, TerminalNextStep::Rotate);
        assert!(!image_plan.abandon_sticky_account);
    }

    #[test]
    fn cache_first_429_waits_in_place_within_max_wait_and_rotates_on_hard_quota_or_excess_delay() {
        let mut state = RequestRetryState::default();
        let soft_body = r#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"4s"}]}}"#;
        let soft_cls = UpstreamClassification::classify(429, soft_body, None);

        // 4s <= max_wait_seconds(180s): 原地等待，不解绑、不换号
        let soft_plan = state.plan_http_retry(
            "acc-1",
            429,
            soft_body,
            None,
            false,
            0,
            6,
            SchedulingMode::CacheFirst,
            true,
            180,
            true,
            &soft_cls,
        );
        assert_eq!(soft_plan.next, TerminalNextStep::RetrySameAccount);
        assert!(!soft_plan.abandon_sticky_account);
        assert!(matches!(soft_plan.strategy, RetryStrategy::GraceRetry(_)));

        // 硬配额枯竭: 立即解绑并轮换
        let hard_body = r#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","reason":"QUOTA_EXHAUSTED"}]}}"#;
        let hard_cls = UpstreamClassification::classify(429, hard_body, None);
        let hard_plan = state.plan_http_retry(
            "acc-2",
            429,
            hard_body,
            None,
            false,
            0,
            6,
            SchedulingMode::CacheFirst,
            true,
            180,
            true,
            &hard_cls,
        );
        assert_eq!(hard_plan.next, TerminalNextStep::Rotate);
        assert!(hard_plan.abandon_sticky_account);

        // 冷却超出 max_wait_seconds (300s > 180s): 解绑并轮换
        let long_body = r#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"300s"}]}}"#;
        let long_cls = UpstreamClassification::classify(429, long_body, None);
        let long_plan = state.plan_http_retry(
            "acc-3",
            429,
            long_body,
            None,
            false,
            0,
            6,
            SchedulingMode::CacheFirst,
            true,
            180,
            true,
            &long_cls,
        );
        assert_eq!(long_plan.next, TerminalNextStep::Rotate);
        assert!(long_plan.abandon_sticky_account);
    }
}
