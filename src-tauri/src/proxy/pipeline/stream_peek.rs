//! 上游「响应头之后、首个数据块之前」的统一等待器（协议无关的 Pipeline 横切能力）
//!
//! # 为什么放在 Pipeline
//!
//! 四条链路在这里回答的是同一个问题：**拿到响应头以后，还愿意为第一个数据块等多久**。
//! 过去 claude / openai(chat) / openai(responses) 各自手写 120 行阻塞循环，
//! 且分档表与 `upstream::client` 的 TTFT 表各写一份、数值还不完全一致
//! （handler 侧 30/60/90/120/180，client 侧 30/60/90/120/150）。
//! 现在只保留**一张表、一份循环**：
//!
//! - [`adaptive_ttft_secs`]：按未缓存 token 规模给出 TTFT 下限，`upstream::client` 与
//!   peek 共用；
//! - [`peek_budget_secs`]：把压缩 / 生图 / 思考三类宽限叠加到同一张表；
//! - [`peek_first_data`]：统一跳过心跳、识别错误事件、区分超时 / 空流 / 上游错误。
//!
//! # 与终态策略的关系
//!
//! `Timeout` 属于「上游排队 / 大上下文 prefill 慢」，**不是账号故障**：
//! 调用方必须映射为 [`super::terminal::UpstreamTerminal::FirstByte`]（504、不罚号、
//! 不跨账号重放），而不是换号重来——否则同一份 body 会被反复从头 prefill。

use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::pin::Pin;
use std::time::Duration;

/// 首包等待的结果
#[derive(Debug)]
pub enum PeekOutcome {
    /// 拿到首个真实数据块（心跳 / 空块已被跳过）
    Data { bytes: Bytes, waited_ms: f64 },
    /// 首包预算耗尽：上游排队或大上下文 prefill 慢
    Timeout { budget_secs: u64, waited_ms: f64 },
    /// 上游在首包前正常结束（空响应）
    Eof { waited_ms: f64 },
    /// 上游在首包前返回流错误
    StreamError { error: String, waited_ms: f64 },
    /// 首包即协议错误事件（如 `event: error` / `data: {"error": ...}`）
    ErrorEvent { waited_ms: f64 },
}

impl PeekOutcome {
    /// 是否属于「时间问题」——调用方据此走 FirstByte 终态而不是换号
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout { .. })
    }
}

/// 按 token 规模给出 TTFT 下限（秒）
///
/// 与 `upstream::client::calculate_first_byte_wait_secs` 同源：握手阶段的
/// 首字节预算与「响应头之后等首 chunk」的预算必须使用同一张表，否则会出现
/// “预算 30s / 阈值 120s”这类跨层错配。
pub fn adaptive_ttft_secs(estimated_tokens: u32) -> u64 {
    match estimated_tokens {
        t if t >= 200_000 => 150,
        t if t >= 100_000 => 120,
        t if t >= 50_000 => 90,
        t if t >= 20_000 => 60,
        _ => 30,
    }
}

/// 首包等待预算（秒）
///
/// 优先级：压缩摘要 > 生图 > 思考模型 > 常规会话；最终由调用方再经
/// [`super::budget::clamp_secs`] 夹到本请求剩余全局预算内。
pub fn peek_budget_secs(
    is_compaction: bool,
    is_image_gen: bool,
    is_thinking: bool,
    estimated_tokens: Option<u32>,
) -> u64 {
    if is_compaction {
        return 180;
    }
    if is_image_gen {
        return 90;
    }
    let tokens = estimated_tokens.unwrap_or(0);
    if is_thinking {
        // 深度思考模型预热耗时长，设置 60s 宽限底线，防止过早误杀
        return match tokens {
            t if t >= 100_000 => 120,
            t if t >= 50_000 => 90,
            _ => 60,
        };
    }
    adaptive_ttft_secs(tokens)
}

/// 在给定截止时间前等待首个真实数据块
///
/// - `is_heartbeat`：跳过 SSE 注释 / ping 帧，不重置截止时间；
/// - `has_error_event`：识别首包即错误事件的协议方言（由适配器提供）。
///
/// 函数不做任何轮换决策：超时返回 [`PeekOutcome::Timeout`]，由调用方按
/// FirstByte 终态处理。
pub async fn peek_first_data<S, E>(
    stream: &mut Pin<Box<S>>,
    budget_secs: u64,
    mut is_heartbeat: impl FnMut(&Bytes) -> bool,
    has_error_event: impl Fn(&Bytes) -> bool,
) -> PeekOutcome
where
    S: Stream<Item = Result<Bytes, E>> + Send + ?Sized,
    E: std::fmt::Display,
{
    let started = std::time::Instant::now();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(budget_secs);

    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return PeekOutcome::Timeout {
                budget_secs,
                waited_ms: started.elapsed().as_secs_f64() * 1000.0,
            };
        }

        let remaining = deadline - now;
        match tokio::time::timeout(remaining, stream.next()).await {
            Ok(Some(Ok(bytes))) => {
                if bytes.is_empty() {
                    continue;
                }
                if is_heartbeat(&bytes) {
                    continue;
                }
                let waited_ms = started.elapsed().as_secs_f64() * 1000.0;
                if has_error_event(&bytes) {
                    return PeekOutcome::ErrorEvent { waited_ms };
                }
                return PeekOutcome::Data { bytes, waited_ms };
            }
            Ok(Some(Err(e))) => {
                return PeekOutcome::StreamError {
                    error: e.to_string(),
                    waited_ms: started.elapsed().as_secs_f64() * 1000.0,
                };
            }
            Ok(None) => {
                return PeekOutcome::Eof {
                    waited_ms: started.elapsed().as_secs_f64() * 1000.0,
                };
            }
            Err(_) => {
                return PeekOutcome::Timeout {
                    budget_secs,
                    waited_ms: started.elapsed().as_secs_f64() * 1000.0,
                };
            }
        }
    }
}

/// SSE 协议方言（用于心跳 / 错误事件识别）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamDialect {
    Anthropic,
    Gemini,
    OpenAi,
}

/// 判断一个 SSE 数据块是否为心跳 / 非数据帧
///
/// 四个协议过去各写一份内联谓词；探针与 peek 共用同一份判据，
/// 避免“探针把心跳当首块”或“peek 把心跳当数据”的漂移。
pub fn is_heartbeat(bytes: &[u8], dialect: StreamDialect) -> bool {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim_start();
    if trimmed.starts_with(':') || trimmed.starts_with("data: :") {
        return true;
    }
    match dialect {
        StreamDialect::Anthropic => {
            text.contains("\"type\":\"ping\"") || text.contains("\"type\": \"ping\"")
        }
        // Gemini 的 `__cloudCodeMeta` 是流首元数据帧，不是首 token
        StreamDialect::Gemini => text.contains("__cloudCodeMeta"),
        StreamDialect::OpenAi => false,
    }
}

/// TTFT 的唯一语义口径：从**最终生效的上游尝试开始**（含建连/上传/排队）
/// 到**首个上游数据块**。
///
/// 响应头到达时间不是独立指标：v1internal 的响应头与首块同源，流式链路为了
/// 保活会提前返回响应头。因此：
/// - QoS 与 `X-Timing-Ttft-Ms` 共用这一个值；
/// - 流式响应在发头时还不知道该值，`X-Timing-Ttft-Ms` 写 `-1`（未知），
///   响应头耗时改用独立的 `X-Timing-Headers-Ms` 表达；
/// - 非流式响应在返回前已知，两个头都写真实值。
pub const TTFT_UNKNOWN_MS: f64 = -1.0;

/// 在首个真实数据块上触发一次 TTFT 回调（用于 QoS 计量）。
///
/// 为什么不能在上游返回响应头时上报：v1internal 的响应头与首 chunk 不是同一时刻，
/// 流式链路为了保活会立即返回响应头；把“握手时间”当作 TTFT 会系统性压低流式样本，
/// 让同桶 QoS 基线失去意义。这里统一以“首个非心跳数据块”为准，与非流式 peek 口径一致。
///
/// - `is_heartbeat`：跳过 SSE 注释 / ping 帧；
/// - `on_first_data`：只执行一次，参数为从 `start` 到首块的真实耗时（毫秒）。
pub fn wrap_stream_with_ttft_probe<S, E, H, P>(
    stream: S,
    start: std::time::Instant,
    is_heartbeat: H,
    on_first_data: P,
) -> impl Stream<Item = Result<Bytes, E>> + Send
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: Send + 'static,
    H: Fn(&[u8]) -> bool + Send + 'static,
    P: FnOnce(f64) + Send + 'static,
{
    async_stream::stream! {
        let mut probe = Some(on_first_data);
        let mut stream = Box::pin(stream);
        while let Some(item) = stream.next().await {
            if probe.is_some() {
                if let Ok(bytes) = &item {
                    if !bytes.is_empty() && !is_heartbeat(bytes) {
                        let ttft_ms = start.elapsed().as_secs_f64() * 1000.0;
                        if let Some(cb) = probe.take() {
                            cb(ttft_ms);
                        }
                    }
                }
            }
            yield item;
        }
    }
}

/// 识别 Anthropic（Claude）SSE 流中的「错误事件」（首包即错误时必须换号重试）
///
/// 唯一实现在 Pipeline：状态机为「`event: error` 标记 + 紧随其后的 `data:` 载荷」
/// 或 `data:` 载荷自身声明 `type: error` / 带 `error` 字段。
/// 适配器只负责把原始字节块交进来，不再各写一份 `contains`。
pub fn has_anthropic_error_event(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    let mut saw_error_event = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "event: error" {
            saw_error_event = true;
        } else if let Some(data) = trimmed.strip_prefix("data:") {
            if saw_error_event {
                return true;
            }
            if let Ok(payload) = serde_json::from_str::<Value>(data.trim()) {
                if payload.get("type").and_then(Value::as_str) == Some("error")
                    || payload.get("error").is_some_and(|e| !e.is_null())
                {
                    return true;
                }
            }
        } else if trimmed.is_empty() {
            saw_error_event = false;
        }
    }
    false
}

/// 识别 OpenAI（Chat / Responses / Codex）SSE 流中的「错误事件」
///
/// 唯一实现在 Pipeline：`data:` 载荷声明 `type` 为 `error` / `response.failed`，
/// 或携带非空 `error` 字段。
pub fn has_openai_error_event(bytes: &[u8]) -> bool {
    String::from_utf8_lossy(bytes).lines().any(|line| {
        let Some(data) = line.trim_start().strip_prefix("data:") else {
            return false;
        };
        let Ok(payload) = serde_json::from_str::<Value>(data.trim()) else {
            return false;
        };

        matches!(
            payload.get("type").and_then(Value::as_str),
            Some("error" | "response.failed")
        ) || payload.get("error").is_some_and(|error| !error.is_null())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    fn boxed(
        items: Vec<Result<Bytes, String>>,
    ) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>> {
        Box::pin(stream::iter(items))
    }

    #[test]
    fn error_event_detectors_are_protocol_specific() {
        // Anthropic：`event: error` + 紧随的 data 帧
        assert!(has_anthropic_error_event(
            b"event: error\ndata: {\"type\":\"error\"}\n\n"
        ));
        // Anthropic：data 载荷自带 type=error
        assert!(has_anthropic_error_event(
            b"data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\"}}\n\n"
        ));
        // 普通正文不得误判
        assert!(!has_anthropic_error_event(
            b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"an error happened\"}}\n\n"
        ));

        // OpenAI：type=error / response.failed / error 字段
        assert!(has_openai_error_event(b"data: {\"type\":\"error\"}\n\n"));
        assert!(has_openai_error_event(
            b"data: {\"type\":\"response.failed\"}\n\n"
        ));
        assert!(has_openai_error_event(
            b"data: {\"error\":{\"message\":\"boom\"}}\n\n"
        ));
        assert!(!has_openai_error_event(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"error rate\"}}]}\n\n"
        ));
        // 空事件帧不触发
        assert!(!has_openai_error_event(b"event: error\n"));
    }

    #[tokio::test]
    async fn skips_heartbeats_and_returns_first_real_chunk() {
        let mut s = boxed(vec![
            Ok(Bytes::from(": ping\n\n")),
            Ok(Bytes::from("")),
            Ok(Bytes::from("data: {\"x\":1}\n\n")),
        ]);
        let out = peek_first_data(&mut s, 5, |b| b.starts_with(b": "), |_| false).await;
        match out {
            PeekOutcome::Data { bytes, .. } => {
                assert_eq!(&bytes[..], b"data: {\"x\":1}\n\n");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn reports_protocol_error_event_before_data() {
        let mut s = boxed(vec![Ok(Bytes::from(
            "data: {\"error\":{\"message\":\"boom\"}}\n\n",
        ))]);
        let out = peek_first_data(
            &mut s,
            5,
            |_| false,
            |b| b.windows(7).any(|w| w == b"\"error\""),
        )
        .await;
        assert!(matches!(out, PeekOutcome::ErrorEvent { .. }));
    }

    #[tokio::test]
    async fn reports_eof_and_stream_error() {
        let mut empty = boxed(vec![]);
        assert!(matches!(
            peek_first_data(&mut empty, 5, |_| false, |_| false).await,
            PeekOutcome::Eof { .. }
        ));

        let mut broken = boxed(vec![Err("connection reset".to_string())]);
        assert!(matches!(
            peek_first_data(&mut broken, 5, |_| false, |_| false).await,
            PeekOutcome::StreamError { .. }
        ));
    }

    #[tokio::test]
    async fn timeout_is_flagged_as_terminal_time_problem() {
        // 用 1s 预算 + 一个永不就绪的流验证超时分支
        let pending = Box::pin(futures::stream::pending::<Result<Bytes, String>>());
        let mut s: Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>> = pending;
        let out = peek_first_data(&mut s, 1, |_| false, |_| false).await;
        assert!(out.is_timeout(), "expected timeout, got {out:?}");
    }

    #[tokio::test]
    async fn ttft_probe_fires_once_on_first_real_chunk() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let hits = Arc::new(AtomicUsize::new(0));
        let hits_cb = Arc::clone(&hits);
        let mut stream = wrap_stream_with_ttft_probe(
            stream::iter(vec![
                Ok::<Bytes, String>(Bytes::from(": ping\n\n")),
                Ok(Bytes::from("data: {\"x\":1}\n\n")),
                Ok(Bytes::from("data: {\"x\":2}\n\n")),
            ]),
            std::time::Instant::now(),
            |b: &[u8]| b.starts_with(b":"),
            move |_ms| {
                hits_cb.fetch_add(1, Ordering::SeqCst);
            },
        );
        futures::pin_mut!(stream);
        while stream.next().await.is_some() {}
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "probe must fire exactly once"
        );
    }

    #[test]
    fn heartbeat_dialects_are_shared() {
        assert!(is_heartbeat(b": ping\n\n", StreamDialect::OpenAi));
        assert!(is_heartbeat(b": ping\n\n", StreamDialect::Anthropic));
        assert!(!is_heartbeat(b"data: {\"x\":1}\n\n", StreamDialect::OpenAi));
        assert!(is_heartbeat(
            b"event: ping\ndata: {\"type\": \"ping\"}\n\n",
            StreamDialect::Anthropic
        ));
        assert!(is_heartbeat(
            b"data: {\"__cloudCodeMeta\":{}}\n\n",
            StreamDialect::Gemini
        ));
        assert!(!is_heartbeat(b"data: {\"x\":1}\n\n", StreamDialect::Gemini));
    }

    #[test]
    fn budget_table_is_single_floor() {
        assert_eq!(peek_budget_secs(true, false, false, None), 180);
        assert_eq!(peek_budget_secs(false, true, false, None), 90);
        assert_eq!(peek_budget_secs(false, false, true, None), 60);
        assert_eq!(peek_budget_secs(false, false, true, Some(120_000)), 120);
        assert_eq!(peek_budget_secs(false, false, false, Some(210_000)), 150);
        assert_eq!(peek_budget_secs(false, false, false, None), 30);
    }
}
