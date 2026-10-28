//! 请求阶段时间线（可观测性）
//!
//! 目标：任何一次请求（**包括被客户端中断的**）都能在日志里回答四个问题：
//! 1. 请求到没到网关；
//! 2. 卡在哪个阶段（选号 / 转换 / 上游尝试 / 等首字节 / 推流）；
//! 3. 每个阶段花了多久；
//! 4. 失败或被中断时，当时的端点/账号/预算是什么状态。
//!
//! # 日志落点（与既有排障路径保持一致）
//!
//! 全部复用同一条 `tracing` 管线，不新建文件、不新建 appender：
//! - 异常事件（重试/超时/降级/端点切换/账号切换/网关自答）：即时 `WARN` → 容器 stdout（`docker logs`）
//! - 网关侧致命失败：即时 `ERROR` → `logs/error.log`
//! - 请求终态：**仅当异常**（HTTP≥400 / 被取消 / 中途打过 alert）时输出一条聚合 `ERROR`；
//!   **成功但总耗时 ≥ 25s 的慢请求**输出一条聚合 `WARN`（可用 `ANTIGRAVITY_STAGE_TRACE_SLOW_SECS` 调阈值，
//!   `ANTIGRAVITY_STAGE_TRACE_SLOW_ERROR=1` 可升级进 error.log）；正常快速请求只留 `DEBUG`
//! - 设 `ANTIGRAVITY_STAGE_TRACE_VERBOSE=1` 时，所有静默阶段也镜像到 `ERROR`，
//!   用于聚焦排查某个时间窗（默认关闭）
//!
//! # 为什么需要它
//!
//! 2026-10-28 的事故里，客户端 180s 主动断开 → handler future 被 drop →
//! `monitor::log_request` 永远不会执行 → `proxy_logs.db` 与 `error.log` **双双零痕迹**。
//! 只有控制台还留着半截 `Sticky Session`。本模块用 `Drop` 兜底把这条链路补齐。

use axum::{extract::Request, middleware::Next, response::Response};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::task_local;

/// 单条请求最多保留的阶段标记数（防御异常长尾）
const MAX_MARKS: usize = 64;
/// 单条 detail 的最大字符数（避免一次把日志打爆）
const MAX_DETAIL_CHARS: usize = 240;

/// 是否把每个阶段也镜像到 `ERROR`（→ `error.log`），用于聚焦排查
static VERBOSE: Lazy<bool> = Lazy::new(|| {
    std::env::var("ANTIGRAVITY_STAGE_TRACE_VERBOSE")
        .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes" | "on"))
        .unwrap_or(false)
});

static SEQ: AtomicU64 = AtomicU64::new(1);

task_local! {
    /// 当前任务链路上的请求阶段时间线（中间件 scope 注入，handler / 上游客户端读取）
    pub static CURRENT_STAGE: Arc<StageTrace>;
}

/// 慢请求阈值（毫秒）：单次阶段/整条请求超过它就要留痕（否则无法回溯“25s 区间”的可疑请求）
///
/// 默认 25000ms；设 `ANTIGRAVITY_STAGE_TRACE_SLOW_SECS=0` 可关闭。
static SLOW_MS: Lazy<u128> = Lazy::new(|| {
    std::env::var("ANTIGRAVITY_STAGE_TRACE_SLOW_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|secs| secs as u128 * 1000)
        .unwrap_or(25_000)
});

/// 慢请求是否升级为 ERROR（→ error.log）。默认 false（只进 docker logs）
static SLOW_AS_ERROR: Lazy<bool> = Lazy::new(|| {
    std::env::var("ANTIGRAVITY_STAGE_TRACE_SLOW_ERROR")
        .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes" | "on"))
        .unwrap_or(false)
});

/// 当前慢请求阈值（毫秒）；`0` = 已关闭
pub fn slow_threshold_ms() -> u128 {
    *SLOW_MS
}

#[derive(Debug, Clone)]
struct StageMark {
    elapsed_ms: u128,
    stage: String,
    detail: String,
    abnormal: bool,
}

/// 一次请求的阶段时间线
///
/// **日志策略：只记异常，正常静默。**
/// - `mark()`：只进内存缓冲，不输出；仅在请求最终异常时随聚合行一起输出
/// - `alert()`：异常事件，立即输出一条 WARN（重试/超时/降级/端点切换/账号切换…）
/// - `finish_*()`：正常请求不输出（仅 DEBUG）；异常请求输出一条聚合 ERROR → `error.log`
pub struct StageTrace {
    id: String,
    protocol: &'static str,
    started: Instant,
    marks: Mutex<Vec<StageMark>>,
    alerts: AtomicU64,
    finished: AtomicBool,
}

impl StageTrace {
    /// 创建一条请求时间线
    pub fn start(protocol: &'static str) -> Arc<Self> {
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let id = format!("{:06x}{:04x}", (nanos ^ seq) & 0x00ff_ffff, seq & 0xffff);
        Arc::new(Self {
            id,
            protocol,
            started: Instant::now(),
            marks: Mutex::new(Vec::with_capacity(16)),
            alerts: AtomicU64::new(0),
            finished: AtomicBool::new(false),
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    /// 静默打点：只入内存缓冲，**不输出**。
    ///
    /// 用于正常路径（received / account / transform / upstream_call / attempt0），
    /// 仅在本次请求最终异常时，由聚合行一并吐出。
    pub fn mark(&self, stage: &str, detail: impl Into<String>) {
        self.push_mark(stage, detail, false);
    }

    /// 异常事件打点：入缓冲，并**立即**输出一条 WARN。
    ///
    /// 用于重试 / 超时 / 降级 / 端点切换 / 账号切换 / 上游错误等
    /// “本身就该被看到”的事件。
    pub fn alert(&self, stage: &str, detail: impl Into<String>) {
        self.push_mark(stage, detail, true);
        self.alerts.fetch_add(1, Ordering::Relaxed);
    }

    /// 异常事件（网关侧致命）：立即输出 ERROR → `error.log`
    pub fn alert_error(&self, stage: &str, detail: impl Into<String>) {
        let detail: String = detail.into();
        self.push_mark(stage, detail.clone(), true);
        self.alerts.fetch_add(1, Ordering::Relaxed);
        tracing::error!(
            target: "stage_trace",
            request = %self.id,
            proto = self.protocol,
            stage = stage,
            detail = %detail,
            "[Stage:alert] {} | {}",
            stage,
            detail
        );
    }

    /// 慢事件：立即输出 WARN，但**不**把整条请求升级为异常（成功但慢 ≠ 失败）
    pub fn notice(&self, stage: &str, detail: impl Into<String>) {
        let detail: String = detail.into();
        self.push_mark(stage, detail.clone(), false);
        tracing::warn!(
            target: "stage_trace",
            request = %self.id,
            proto = self.protocol,
            stage = stage,
            detail = %detail,
            "[Stage:slow] {} | {}",
            stage,
            detail
        );
    }

    fn push_mark(&self, stage: &str, detail: impl Into<String>, abnormal: bool) {
        if self.is_finished() {
            return;
        }
        let detail = truncate(detail.into());
        let elapsed_ms = self.started.elapsed().as_millis();
        {
            let mut marks = self.marks.lock();
            if marks.len() < MAX_MARKS {
                marks.push(StageMark {
                    elapsed_ms,
                    stage: stage.to_string(),
                    detail: detail.clone(),
                    abnormal,
                });
            }
        }

        // 默认静默；VERBOSE 时逐阶段镜像到 error.log
        if *VERBOSE {
            tracing::error!(
                target: "stage_trace",
                request = %self.id,
                proto = self.protocol,
                stage = stage,
                elapsed_ms = elapsed_ms,
                detail = %detail,
                "[Stage:verbose] {} @{}ms {}",
                stage,
                elapsed_ms,
                detail
            );
            return;
        }

        if abnormal {
            tracing::warn!(
                target: "stage_trace",
                request = %self.id,
                proto = self.protocol,
                stage = stage,
                elapsed_ms = elapsed_ms,
                detail = %detail,
                "[Stage:alert] {} @{}ms {}",
                stage,
                elapsed_ms,
                detail
            );
        }
    }

    pub fn has_alerts(&self) -> bool {
        self.alerts.load(Ordering::Relaxed) > 0
    }

    /// 正常终态：仅 DEBUG 输出（默认过滤，不污染日志）
    pub fn finish_ok(&self, outcome: &str, status: u16) {
        self.finish(outcome, status, false);
    }

    /// 失败终态：`ERROR` 聚合行（写 error.log）
    pub fn finish_err(&self, outcome: &str, status: u16) {
        self.finish(outcome, status, true);
    }

    fn finish(&self, outcome: &str, status: u16, is_error: bool) {
        if self.finished.swap(true, Ordering::AcqRel) {
            return;
        }
        let total_ms = self.started.elapsed().as_millis();
        let timeline = self.render_timeline();
        // 异常判定：HTTP 错误 / 取消 / 中途打过 alert
        let abnormal = is_error || self.has_alerts();
        // 慢请求判定：即使成功也要留痕（25s 区间是最容易出事的区间）
        let slow = *SLOW_MS > 0 && total_ms >= *SLOW_MS;
        if abnormal || (slow && *SLOW_AS_ERROR) {
            tracing::error!(
                target: "stage_trace",
                request = %self.id,
                proto = self.protocol,
                outcome = outcome,
                status = status,
                total_ms = total_ms,
                alerts = self.alerts.load(Ordering::Relaxed),
                slow = slow,
                timeline = %timeline,
                "[Stage] outcome={} status={} total={}ms alerts={} slow={} | {} | {}",
                outcome,
                status,
                total_ms,
                self.alerts.load(Ordering::Relaxed),
                slow,
                self.protocol,
                timeline
            );
        } else if slow {
            tracing::warn!(
                target: "stage_trace",
                request = %self.id,
                proto = self.protocol,
                outcome = outcome,
                status = status,
                total_ms = total_ms,
                slow_threshold_ms = *SLOW_MS,
                timeline = %timeline,
                "[Stage:slow] outcome={} status={} total={}ms >= {}ms | {} | {}",
                outcome,
                status,
                total_ms,
                *SLOW_MS,
                self.protocol,
                timeline
            );
        } else {
            tracing::debug!(
                target: "stage_trace",
                request = %self.id,
                proto = self.protocol,
                outcome = outcome,
                status = status,
                total_ms = total_ms,
                timeline = %timeline,
                "[Stage] outcome={} status={} total={}ms | {} | {}",
                outcome,
                status,
                total_ms,
                self.protocol,
                timeline
            );
        }
    }

    fn render_timeline(&self) -> String {
        let marks = self.marks.lock();
        if marks.is_empty() {
            return "(no marks)".to_string();
        }
        marks
            .iter()
            .map(|m| {
                // 异常点用 `!` 前缀标记，一眼区分“正常经过”与“出过事”
                let flag = if m.abnormal { "!" } else { "" };
                if m.detail.is_empty() {
                    format!("{}{}@{}ms", flag, m.stage, m.elapsed_ms)
                } else {
                    format!("{}{}@{}ms({})", flag, m.stage, m.elapsed_ms, m.detail)
                }
            })
            .collect::<Vec<_>>()
            .join(" -> ")
    }

    /// 生成取消兜底守卫：handler 在产出响应前被 drop（客户端断开 / 任务取消）时落 error.log
    pub fn guard(self: &Arc<Self>) -> StageTraceGuard {
        StageTraceGuard {
            trace: Arc::clone(self),
        }
    }

    /// 在给定 future 上挂载任务局部时间线（中间件使用）
    pub async fn scope<F: std::future::Future>(trace: Arc<Self>, fut: F) -> F::Output {
        CURRENT_STAGE.scope(trace, fut).await
    }

    /// 取当前任务链路上的时间线（handler / 上游客户端使用）
    pub fn current() -> Option<Arc<StageTrace>> {
        CURRENT_STAGE.try_with(Arc::clone).ok()
    }

    /// 在当前任务链路上打静默点；无时间线时静默跳过
    pub fn mark_current(stage: &str, detail: impl Into<String>) {
        if let Some(trace) = Self::current() {
            trace.mark(stage, detail);
        }
    }

    /// 在当前任务链路上打异常点（立即 WARN）
    pub fn alert_current(stage: &str, detail: impl Into<String>) {
        if let Some(trace) = Self::current() {
            trace.alert(stage, detail);
        }
    }

    /// 在当前任务链路上打网关侧致命点（立即 ERROR → error.log）
    pub fn alert_current_error(stage: &str, detail: impl Into<String>) {
        if let Some(trace) = Self::current() {
            trace.alert_error(stage, detail);
        }
    }

    /// 在当前任务链路上打慢事件（立即 WARN，不升级为异常）
    pub fn notice_current(stage: &str, detail: impl Into<String>) {
        if let Some(trace) = Self::current() {
            trace.notice(stage, detail);
        }
    }
}

/// 取消兜底：正常结束会先 `finish_*`，此后 drop 为 no-op
pub struct StageTraceGuard {
    trace: Arc<StageTrace>,
}

impl Drop for StageTraceGuard {
    fn drop(&mut self) {
        if !self.trace.is_finished() {
            // 关键黑洞：客户端断开 / 网关内部任务被取消时，handler future 直接消失，
            // `monitor::log_request` 与 `proxy_logs.db` 都不会留下任何痕迹。
            self.trace.finish_err("canceled", 499);
        }
    }
}

/// 请求级阶段时间线中间件
///
/// 放在代理路由的**最外层**，因此：
/// - 鉴权/IP 拦截也会留下 `received` 与终态；
/// - 整条下游调用链（handler → 选号 → 转换 → 上游客户端）共享同一个任务局部时间线；
/// - 客户端中途断开时，中间件 future 被 drop → 守卫落一条 `canceled` 到 error.log。
pub async fn stage_trace_middleware(request: Request, next: Next) -> Response {
    // CORS 预检不产生业务阶段，跳过以免持噪
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }

    let path = request.uri().path().to_string();
    let protocol = protocol_label(&path);
    // 仅跟踪 AI 推理路径；健康检查 / 内部预热 / 埋点等高频杂项直接透传，避免刷爆日志
    if protocol == "other" {
        return next.run(request).await;
    }
    let trace = StageTrace::start(protocol);
    let guard = trace.guard();

    trace.mark(
        "received",
        format!(
            "method={} path={} ua={}",
            request.method(),
            path,
            request
                .headers()
                .get(axum::http::header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("-")
        ),
    );

    let response = StageTrace::scope(
        Arc::clone(&trace),
        // Pipeline 级全局预算：四大协议共用同一个截止时刻（与上游客户端夹取同源）
        crate::proxy::pipeline::budget::scope(
            crate::proxy::pipeline::budget::RequestContext::new(
                crate::proxy::pipeline::budget::deadline_now(),
            ),
            next.run(request),
        ),
    )
    .await;
    let status = response.status().as_u16();
    if status >= 400 {
        trace.finish_err("http_error", status);
    } else {
        trace.finish_ok("response", status);
    }
    drop(guard);
    response
}

fn protocol_label(path: &str) -> &'static str {
    if path.starts_with("/v1/messages") {
        "anthropic"
    } else if path.starts_with("/v1/chat/completions") {
        "openai_chat"
    } else if path.starts_with("/v1/responses") {
        "openai_responses"
    } else if path.starts_with("/v1beta") {
        "gemini"
    } else if path.starts_with("/v1/images") {
        "openai_images"
    } else {
        "other"
    }
}

fn truncate(mut detail: String) -> String {
    if detail.chars().count() <= MAX_DETAIL_CHARS {
        return detail;
    }
    let kept: String = detail.chars().take(MAX_DETAIL_CHARS).collect();
    detail = kept;
    detail.push('…');
    detail
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_labels_cover_all_public_routes() {
        assert_eq!(protocol_label("/v1/messages"), "anthropic");
        assert_eq!(protocol_label("/v1/chat/completions"), "openai_chat");
        assert_eq!(protocol_label("/v1beta/models/x:generateContent"), "gemini");
        assert_eq!(protocol_label("/health"), "other");
    }

    #[test]
    fn timeline_renders_marks_and_finish_is_idempotent() {
        let trace = StageTrace::start("test");
        trace.mark("a", "one");
        trace.mark("b", "");
        let rendered = trace.render_timeline();
        assert!(rendered.contains("a@"));
        assert!(rendered.contains("(one)"));
        assert!(rendered.contains("b@"));

        trace.finish_ok("response", 200);
        assert!(trace.is_finished());
        // 二次 finish 不应覆盖首个终态
        trace.finish_err("error", 500);
        assert!(trace.is_finished());
    }

    #[test]
    fn cancel_guard_only_fires_once_and_not_after_finish() {
        let trace = StageTrace::start("test");
        {
            let _guard = trace.guard();
        }
        assert!(trace.is_finished());

        let trace = StageTrace::start("test");
        trace.finish_ok("response", 200);
        {
            let _guard = trace.guard();
        }
        assert!(trace.is_finished());
    }

    #[test]
    fn detail_is_truncated_to_bound() {
        let long = "x".repeat(MAX_DETAIL_CHARS + 50);
        let cut = truncate(long);
        assert!(cut.chars().count() <= MAX_DETAIL_CHARS + 1);
    }
}
