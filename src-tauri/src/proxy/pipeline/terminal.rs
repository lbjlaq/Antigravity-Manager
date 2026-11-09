//! 上游「终态失败」的统一分类与处置（协议无关的 Pipeline 横切能力）
//!
//! # 为什么放在 Pipeline
//!
//! 四个协议适配器（OpenAI Responses / OpenAI Chat Completions / Anthropic Claude /
//! Google Gemini）在上游返回 `Err` 时都必须回答同样三个问题：
//!
//! 1. 返回给客户端什么 HTTP 状态码？
//! 2. 该不该把这个账号记一次瞬态失败（30s 冷却）？
//! 3. 该不该继续换端点 / 换账号重放同一份 body？
//!
//! 这三问过去在 claude / openai(chat) / openai(responses) / gemini 各写一份，结果同一件事
//! 给出两种答案：claude 返 504，其余三个返 502；图片路径干脆没有判断，照样轮换账号。
//! 现在收敛到这里：适配器只调用一次 [`classify`]，再读 [`TerminalDisposition`]，
//! 不再自己 `contains` 内部标记，也不再自己决定状态码。
//!
//! # 标记从哪来
//!
//! 终态判定发生在唯一的上游咽喉 `upstream::client::call_v1_internal_with_headers`，
//! 它把判定结果以标记前缀写进错误文本（沿用 `Result<_, String>` 的既有形状），
//! 穿过适配器时由本模块识别。标记只用于内部链路控制，写给客户端前必须经
//! `upstream::client::strip_internal_markers` 剔除。

use axum::http::StatusCode;

/// 上传阶段被截断（上传停滞 / 上传预算耗尽 / 大 body 单次尝试策略）
pub const UPLOAD_STAGE_TERMINAL_MARKER: &str = "[upstream_upload_stage_terminal]";

/// 统一首字延迟耗尽（上游排队 / 大上下文 prefill 慢，**不是账号故障**）
pub const FIRST_BYTE_STAGE_TERMINAL_MARKER: &str = "[upstream_first_byte_terminal]";

/// 请求级全局预算耗尽（[`super::budget`] 的 270s 上限）
pub const GATEWAY_BUDGET_TERMINAL_MARKER: &str = "[upstream_gateway_budget_terminal]";

/// 同一请求内的大 body 上游调用次数达到上限（重放护栏）
pub const REPLAY_CAP_TERMINAL_MARKER: &str = "[upstream_replay_cap_terminal]";

/// 上传完成后、首字节到达前的传输层断连（连接复位 / HTTP2 GOAWAY / SendRequest），
/// 且发生在首字节预算之内：既不是“上游慢”，也不是账号故障
pub const TRANSPORT_RESET_TERMINAL_MARKER: &str = "[upstream_transport_reset_terminal]";

/// 全部内部标记：写给客户端前逐一剔除
pub const ALL_TERMINAL_MARKERS: [&str; 5] = [
    UPLOAD_STAGE_TERMINAL_MARKER,
    FIRST_BYTE_STAGE_TERMINAL_MARKER,
    GATEWAY_BUDGET_TERMINAL_MARKER,
    REPLAY_CAP_TERMINAL_MARKER,
    TRANSPORT_RESET_TERMINAL_MARKER,
];

/// 上游终态失败的分类
///
/// 共同点：**都不是账号故障**，也都不值得再重放同一份 body。
/// 区分它们只是为了状态码与排障措辞（阶段时间线 / 日志）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamTerminal {
    /// 首字节预算耗尽：上游排队或大上下文 prefill 慢
    FirstByte,
    /// 上传阶段被截断：重传只会再把同一份大 body 从头传一遍
    Upload,
    /// 全局请求预算耗尽：整条请求已经到点
    GlobalBudget,
    /// 大 body 重放次数达到上限：护栏主动拒绝
    ReplayCap,
    /// 上传已完成、首字节预算未耗尽时传输层断连（共享连接复位等），与账号无关
    TransportReset,
}

/// 一次终态失败的处置结果
///
/// 适配器不再自己写 `if status == 504` / `if !first_byte_terminal`，只读这几个字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalDisposition {
    /// 返回给客户端的 HTTP 状态码
    pub status: StatusCode,
    /// 是否把账号记一次瞬态失败（进入 30s 冷却）
    pub penalize_account: bool,
    /// 是否继续换端点 / 换账号重放同一份 body
    pub stop_rotation: bool,
    /// 是否解绑当前会话与该账号的粘性绑定（避免客户端下次重试继续撞上同一个慢账号）
    pub abandon_sticky_account: bool,
}

impl UpstreamTerminal {
    /// 稳定标识（日志 / 阶段时间线 / metrics 分桶用）
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstByte => "first_byte_timeout",
            Self::Upload => "upload_aborted",
            Self::GlobalBudget => "gateway_budget_exhausted",
            Self::ReplayCap => "replay_cap_reached",
            Self::TransportReset => "transport_reset",
        }
    }

    /// 写进错误文本的内部标记
    pub fn marker(self) -> &'static str {
        match self {
            Self::FirstByte => FIRST_BYTE_STAGE_TERMINAL_MARKER,
            Self::Upload => UPLOAD_STAGE_TERMINAL_MARKER,
            Self::GlobalBudget => GATEWAY_BUDGET_TERMINAL_MARKER,
            Self::ReplayCap => REPLAY_CAP_TERMINAL_MARKER,
            Self::TransportReset => TRANSPORT_RESET_TERMINAL_MARKER,
        }
    }

    /// 统一处置策略
    ///
    /// - 时间类终态（首字节 / 上传 / 全局预算）统一 504：客户端与监控都按
    ///   `DEADLINE_EXCEEDED` 处理，不再出现“Claude 504、其余 502”的双口径。
    /// - 重放上限是“我们主动不再重传”，与时间无关，用 503 表达“稍后重试”。
    /// - 四者都不罚账号：前三种是“等得不够久”，重放上限是“我们主动止损”，都不是账号故障。
    /// - 首字节超时、上传中断与全局预算耗尽时，虽然不惩罚账号，但解绑当前会话粘性绑定（abandon_sticky_account = true），
    ///   确保客户端或后续轮换能够自动调度到号池中的其他健康账号，彻底消除“卡在同一账号循环死等”问题。
    /// - 轮换控制（stop_rotation）：
    ///   * 全局总预算耗尽（GlobalBudget）或重放上限（ReplayCap）时彻底终止轮换（stop_rotation = true）；
    ///   * 首字节中断/超时（FirstByte）与上传异常（Upload）发生时，谷歌未完成生成不扣减额度，且请求预算若有结余，
    ///     允许网关在号池内安全轮换至后续健康账号（stop_rotation = false），避免单账号网络抖动或集群假死直接导致整条请求死掉。
    /// - 传输层断连（TransportReset）不是时间问题，回 502；处置与首字节一致（不罚号、可继续）。
    /// - 以上是调度模式无关的「默认处置」；CacheFirst 下是否解绑 / 换号由 [`TerminalDisposition::plan`] 裁决。
    pub fn disposition(self) -> TerminalDisposition {
        TerminalDisposition {
            status: match self {
                Self::FirstByte | Self::Upload | Self::GlobalBudget => StatusCode::GATEWAY_TIMEOUT,
                Self::ReplayCap => StatusCode::SERVICE_UNAVAILABLE,
                Self::TransportReset => StatusCode::BAD_GATEWAY,
            },
            penalize_account: false,
            stop_rotation: matches!(self, Self::GlobalBudget | Self::ReplayCap),
            abandon_sticky_account: matches!(
                self,
                Self::FirstByte | Self::Upload | Self::GlobalBudget | Self::TransportReset
            ),
        }
    }
}

/// 终态失败后的下一步（由调度模式裁决）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalNextStep {
    /// 换号重放（Balance / PerformanceFirst 的既有行为）
    Rotate,
    /// 保留粘性绑定，在当前账号上原地重放一次（CacheFirst）
    RetrySameAccount,
    /// 终止本次请求，把终态状态码交回客户端（绑定是否保留见 `abandon_sticky_account`）
    Stop,
}

/// 一次终态失败的最终执行计划
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalRetryPlan {
    pub next: TerminalNextStep,
    /// 是否解绑当前会话与该账号的粘性绑定
    pub abandon_sticky_account: bool,
}

impl TerminalDisposition {
    /// 按调度模式把默认处置落成执行计划（四协议共用的唯一裁决）
    ///
    /// - 非 CacheFirst：保持既有语义——按 `abandon_sticky_account` 解绑，
    ///   `stop_rotation` 为假时换号重放。
    /// - CacheFirst：终态都不是账号故障，换号必然让上游 KV 缓存全部失效。
    ///   因此**绝不解绑**；允许继续时在当前账号原地重放一次
    ///   （`same_account_retry_available` 由调用方保证每账号每请求至多一次），
    ///   用完或不允许继续时直接终止，让客户端下一次重试仍命中同一账号。
    pub fn plan(self, cache_first: bool, same_account_retry_available: bool) -> TerminalRetryPlan {
        if cache_first {
            let next = if !self.stop_rotation && same_account_retry_available {
                TerminalNextStep::RetrySameAccount
            } else {
                TerminalNextStep::Stop
            };
            return TerminalRetryPlan {
                next,
                abandon_sticky_account: false,
            };
        }
        TerminalRetryPlan {
            next: if self.stop_rotation {
                TerminalNextStep::Stop
            } else {
                TerminalNextStep::Rotate
            },
            abandon_sticky_account: self.abandon_sticky_account,
        }
    }
}

/// 从上游错误文本里识别终态分类；普通网络错误 / 上游 HTTP 错误返回 `None`
pub fn classify(error_text: &str) -> Option<UpstreamTerminal> {
    if error_text.contains(GATEWAY_BUDGET_TERMINAL_MARKER) {
        return Some(UpstreamTerminal::GlobalBudget);
    }
    if error_text.contains(FIRST_BYTE_STAGE_TERMINAL_MARKER) {
        return Some(UpstreamTerminal::FirstByte);
    }
    if error_text.contains(REPLAY_CAP_TERMINAL_MARKER) {
        return Some(UpstreamTerminal::ReplayCap);
    }
    if error_text.contains(UPLOAD_STAGE_TERMINAL_MARKER) {
        return Some(UpstreamTerminal::Upload);
    }
    if error_text.contains(TRANSPORT_RESET_TERMINAL_MARKER) {
        return Some(UpstreamTerminal::TransportReset);
    }
    None
}

/// 剔除仅用于内部链路控制与诊断的标记，避免泄漏到客户端错误报文
pub fn strip_markers(error_text: &str) -> String {
    let mut cleaned = error_text.to_string();
    for marker in ALL_TERMINAL_MARKERS {
        cleaned = cleaned.replace(marker, "");
    }
    cleaned
}

/// 一次客户端请求内的「结构化失败状态」聚合器
///
/// 只保留**非 429** 的最后一个失败状态：整池都被限流时才回 429，
/// 一旦混入网络 / 看门狗超时等硬失败，就以那个状态为准（避免把网关问题伪装成限流）。
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

    /// 记录一次终态失败（状态码由 [`UpstreamTerminal::disposition`] 统一给出）
    pub fn record_terminal(&mut self, terminal: UpstreamTerminal) {
        self.record(terminal.disposition().status);
    }

    pub fn has_failures(&self) -> bool {
        self.saw_failure
    }

    pub fn is_empty(&self) -> bool {
        !self.saw_failure
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

/// [`apply_terminal`] 的返回值：分类结果 + 统一处置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalHandling {
    pub terminal: UpstreamTerminal,
    pub disposition: TerminalDisposition,
}

/// 适配器唯一需要调用的终态处置入口
///
/// 一次调用完成四件事，四个协议不再各写一份：
/// 1. 从错误文本分类终态；
/// 2. 把状态码记进 [`FailureStatusTracker`]；
/// 3. 打一条统一的阶段时间线（`upstream_terminal`），观测口径不再随协议漂移；
/// 4. 按策略决定是否罚号（`on_penalize` 由调用方注入账号层动作）。
///
/// 返回 `None` 表示不是终态，调用方继续走既有的换号 / 退避逻辑。
pub fn apply_terminal(
    error_text: &str,
    tracker: &mut FailureStatusTracker,
    protocol: &str,
    attempt: usize,
    max_attempts: usize,
    elapsed_secs: f64,
    on_penalize: impl FnOnce(&str),
) -> Option<TerminalHandling> {
    let terminal = classify(error_text)?;
    let disposition = terminal.disposition();
    tracker.record(disposition.status);

    crate::proxy::stage_trace::StageTrace::alert_current(
        "upstream_terminal",
        format!(
            "protocol={} attempt={}/{} kind={} status={} elapsed={:.1}s",
            protocol,
            attempt,
            max_attempts,
            terminal.as_str(),
            disposition.status.as_u16(),
            elapsed_secs
        ),
    );

    if disposition.penalize_account {
        on_penalize(error_text);
    }

    Some(TerminalHandling {
        terminal,
        disposition,
    })
}

/// 记录一次「首包预算耗尽」终态（504 / 不罚号 / 不轮换）
///
/// 供 peek 这类已经拿到响应头、但首块迟迟不到的阶段使用：
/// 语义与上游客户端触发的 [`UpstreamTerminal::FirstByte`] 完全一致。
pub fn record_first_byte_timeout(
    protocol: &str,
    budget_secs: u64,
    tracker: &mut FailureStatusTracker,
) {
    tracker.record_terminal(UpstreamTerminal::FirstByte);
    crate::proxy::stage_trace::StageTrace::alert_current(
        "upstream_terminal",
        format!(
            "protocol={} kind=first_byte_timeout status=504 budget={}s",
            protocol, budget_secs
        ),
    );
}

/// 请求级全局预算的循环闸门（四协议统一）
///
/// 预算耗尽时记终态、补一条 last_error 并留阶段时间线；返回 `true` 表示调用方
/// 应立即 `break` 退出轮换循环（不罚号、不跨账号重放）。
pub fn gate_exhausted(
    protocol: &str,
    attempt: usize,
    tracker: &mut FailureStatusTracker,
    last_error: &mut String,
) -> bool {
    if !super::budget::exhausted() {
        return false;
    }
    tracker.record_terminal(UpstreamTerminal::GlobalBudget);
    if last_error.is_empty() {
        *last_error = format!(
            "Gateway global request budget exceeded ({}s)",
            super::budget::gateway_request_budget_secs()
        );
    }
    crate::proxy::stage_trace::StageTrace::alert_current(
        "gateway_budget",
        format!(
            "protocol={} attempt={} budget={}s",
            protocol,
            attempt,
            super::budget::gateway_request_budget_secs()
        ),
    );
    true
}

/// 各协议对外状态码方言（统一处置之后仍需保持的协议层差异）
///
/// 绝大多数状态码四协议一致；仅 Anthropic 客户端有自己的语义映射：
/// - `403 Forbidden` → `503`：告诉 Claude Code 这是上游权限/容量问题，
///   而不是请求本身被拒（避免其触发 `permission_error` 硬失败）；
/// - `529 Overloaded` → `429`：Anthropic 专属的过载语义在客户端等同限流。
///
/// 放在这里是为了让“状态码”只有一个决策入口；适配器不再各写一份 `match`。
pub fn protocol_status(protocol: &str, status: u16) -> StatusCode {
    match (protocol, status) {
        ("claude", 403) => StatusCode::SERVICE_UNAVAILABLE,
        ("claude", 529) => StatusCode::TOO_MANY_REQUESTS,
        _ => StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_status_keeps_anthropic_dialect_only() {
        // Anthropic 方言：403 → 503、529 → 429
        assert_eq!(
            protocol_status("claude", 403),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            protocol_status("claude", 529),
            StatusCode::TOO_MANY_REQUESTS
        );
        // 其余协议与其余状态码保持原语义
        for protocol in ["openai", "responses", "gemini"] {
            assert_eq!(protocol_status(protocol, 403), StatusCode::FORBIDDEN);
            assert_eq!(
                protocol_status(protocol, 529),
                StatusCode::from_u16(529).unwrap()
            );
        }
        assert_eq!(
            protocol_status("claude", 500),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn classifies_each_terminal_marker() {
        assert_eq!(
            classify(&format!("{}boom", FIRST_BYTE_STAGE_TERMINAL_MARKER)),
            Some(UpstreamTerminal::FirstByte)
        );
        assert_eq!(
            classify(&format!("{}boom", UPLOAD_STAGE_TERMINAL_MARKER)),
            Some(UpstreamTerminal::Upload)
        );
        assert_eq!(
            classify(&format!("{}boom", GATEWAY_BUDGET_TERMINAL_MARKER)),
            Some(UpstreamTerminal::GlobalBudget)
        );
        assert_eq!(
            classify(&format!("{}boom", REPLAY_CAP_TERMINAL_MARKER)),
            Some(UpstreamTerminal::ReplayCap)
        );
        assert_eq!(
            classify(&format!("{}boom", TRANSPORT_RESET_TERMINAL_MARKER)),
            Some(UpstreamTerminal::TransportReset)
        );
        assert_eq!(classify("ordinary network error"), None);
        assert_eq!(classify(""), None);
    }

    #[test]
    fn time_like_terminals_all_return_504_and_never_penalize() {
        for terminal in [
            UpstreamTerminal::FirstByte,
            UpstreamTerminal::Upload,
            UpstreamTerminal::GlobalBudget,
        ] {
            let disposition = terminal.disposition();
            // 四协议同口径：时间类终态一律 504（DEADLINE_EXCEEDED）
            assert_eq!(
                disposition.status,
                StatusCode::GATEWAY_TIMEOUT,
                "{terminal:?}"
            );
            assert!(!disposition.penalize_account, "{terminal:?}");
            assert!(
                disposition.abandon_sticky_account,
                "all time-like terminals must abandon sticky account: {terminal:?}"
            );
        }

        // 仅全局预算耗尽与重放上限强制终止账号轮换；首字节与上传异常允许在预算结余时轮换至后续健康账号
        assert!(!UpstreamTerminal::FirstByte.disposition().stop_rotation);
        assert!(!UpstreamTerminal::Upload.disposition().stop_rotation);
        assert!(UpstreamTerminal::GlobalBudget.disposition().stop_rotation);

        // 重放上限不是时间问题，用 503 表达“稍后重试”，同样不罚账号
        let replay = UpstreamTerminal::ReplayCap.disposition();
        assert_eq!(replay.status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(!replay.penalize_account);
        assert!(replay.stop_rotation);
        assert!(!replay.abandon_sticky_account);
    }

    #[test]
    fn transport_reset_is_502_and_never_penalizes() {
        let d = UpstreamTerminal::TransportReset.disposition();
        assert_eq!(d.status, StatusCode::BAD_GATEWAY);
        assert!(!d.penalize_account);
        assert!(!d.stop_rotation);
    }

    #[test]
    fn non_cache_first_plan_keeps_legacy_rotation() {
        for terminal in [
            UpstreamTerminal::FirstByte,
            UpstreamTerminal::Upload,
            UpstreamTerminal::TransportReset,
        ] {
            let plan = terminal.disposition().plan(false, true);
            assert_eq!(plan.next, TerminalNextStep::Rotate, "{terminal:?}");
            assert!(plan.abandon_sticky_account, "{terminal:?}");
        }
        let plan = UpstreamTerminal::GlobalBudget
            .disposition()
            .plan(false, true);
        assert_eq!(plan.next, TerminalNextStep::Stop);
    }

    #[test]
    fn cache_first_plan_never_unbinds_and_retries_same_account_once() {
        for terminal in [
            UpstreamTerminal::FirstByte,
            UpstreamTerminal::Upload,
            UpstreamTerminal::TransportReset,
        ] {
            let first = terminal.disposition().plan(true, true);
            assert_eq!(
                first.next,
                TerminalNextStep::RetrySameAccount,
                "{terminal:?}"
            );
            assert!(!first.abandon_sticky_account, "{terminal:?}");

            let second = terminal.disposition().plan(true, false);
            assert_eq!(second.next, TerminalNextStep::Stop, "{terminal:?}");
            assert!(!second.abandon_sticky_account, "{terminal:?}");
        }
        for terminal in [UpstreamTerminal::GlobalBudget, UpstreamTerminal::ReplayCap] {
            let plan = terminal.disposition().plan(true, true);
            assert_eq!(plan.next, TerminalNextStep::Stop, "{terminal:?}");
            assert!(!plan.abandon_sticky_account, "{terminal:?}");
        }
    }

    #[test]
    fn strip_markers_removes_every_internal_marker() {
        let text = format!(
            "{}a {}b {}c {}d {}e",
            UPLOAD_STAGE_TERMINAL_MARKER,
            FIRST_BYTE_STAGE_TERMINAL_MARKER,
            GATEWAY_BUDGET_TERMINAL_MARKER,
            REPLAY_CAP_TERMINAL_MARKER,
            TRANSPORT_RESET_TERMINAL_MARKER
        );
        let cleaned = strip_markers(&text);
        for marker in ALL_TERMINAL_MARKERS {
            assert!(!cleaned.contains(marker), "{marker} leaked: {cleaned}");
        }
    }

    #[test]
    fn failure_tracker_prefers_non_rate_limit_status() {
        let mut only_429 = FailureStatusTracker::default();
        only_429.record(StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(only_429.final_status(), StatusCode::TOO_MANY_REQUESTS);

        let mut mixed = FailureStatusTracker::default();
        mixed.record(StatusCode::TOO_MANY_REQUESTS);
        mixed.record_terminal(UpstreamTerminal::FirstByte);
        assert_eq!(mixed.final_status(), StatusCode::GATEWAY_TIMEOUT);

        // 没有任何失败记录时回 502（保持既有语义）
        assert_eq!(
            FailureStatusTracker::default().final_status(),
            StatusCode::BAD_GATEWAY
        );
    }

    #[test]
    fn apply_terminal_records_unified_disposition() {
        let mut tracker = FailureStatusTracker::default();
        let mut penalized = false;
        let handling = apply_terminal(
            &format!("{}boom", FIRST_BYTE_STAGE_TERMINAL_MARKER),
            &mut tracker,
            "claude",
            1,
            3,
            12.5,
            |_| penalized = true,
        )
        .expect("terminal must be classified");
        assert_eq!(handling.terminal, UpstreamTerminal::FirstByte);
        assert_eq!(handling.disposition.status, StatusCode::GATEWAY_TIMEOUT);
        assert!(!penalized, "time-like terminals must never penalize");
        assert_eq!(tracker.final_status(), StatusCode::GATEWAY_TIMEOUT);

        // 非终态错误不得被吞掉
        let mut tracker = FailureStatusTracker::default();
        assert!(apply_terminal(
            "ordinary network error",
            &mut tracker,
            "gemini",
            1,
            3,
            1.0,
            |_| {}
        )
        .is_none());
        assert_eq!(tracker.final_status(), StatusCode::BAD_GATEWAY);
    }
}
