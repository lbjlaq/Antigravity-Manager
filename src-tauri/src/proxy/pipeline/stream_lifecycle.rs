//! 出站流生命周期通用驱动器（模板方法模式）
//!
//! 统一接管所有出站协议（Claude、OpenAI 等）的通用流式生命周期：
//! 1. 3s 细粒度保活心跳（`: ping\n\n`）；
//! 2. 120s 空闲静默安全熔断（`streaming_sliding_secs = 120`）；
//! 3. 网络中断统一拦截与状态判断（`has_content` / `has_thinking`）；
//! 4. 协议守卫（首包报错后严禁追加终结帧）；
//! 5. 尾部数据安全 Flush（修复 #1732）。
//!
//! 各协议发散层适配器仅实现 `ProtocolStreamHandler` 专有渲染 Trait。

use bytes::{Bytes, BytesMut};
use futures::{Stream, StreamExt};
use std::pin::Pin;

use crate::proxy::mappers::error_classifier::StreamErrorReport;

/// 出站协议特定帧渲染 Trait
pub trait ProtocolStreamHandler: Send + 'static {
    /// 可选：在流开始前发送的初始握手帧（例如 Codex 的 response.created）
    fn initial_frames(&mut self) -> Vec<Bytes> {
        Vec::new()
    }

    /// 处理上游 SSE 的单行数据（例如 "data: {...}"）
    /// 返回需要向下游发送的字节块序列
    fn process_line(&mut self, line: &str) -> Vec<Bytes>;

    /// 检查是否已有正文内容输出给下游（用于判断中断时是优雅截断还是首包不可重试错误）
    fn has_content(&self) -> bool;

    /// 可选：检查是否已有思考内容输出（例如 Claude 的 thinking 块，用于后思考恢复通道）
    fn has_thinking(&self) -> bool {
        false
    }

    /// 检查协议是否已进入不可逆的终结状态（例如 Claude 发送了 message_stop，或流已结束）
    /// 一旦返回 true，生命周期驱动器必须立即退出轮询循环并断开下游连接，
    /// 严禁继续等待上游 TCP EOF 或发送心跳，防止客户端界面持续转圈计时或报错。
    fn is_terminal(&self) -> bool {
        false
    }

    /// 流正常结束调用，产出协议专属的终结帧
    /// （Claude 下发 message_delta(end_turn) + message_stop，OpenAI 下发 [DONE] 及可选 usage chunk）
    fn emit_finalize(&mut self) -> Vec<Bytes>;

    /// 首包发生致命错误/超时且 has_content() == false && has_thinking() == false 时调用
    /// 返回协议专属的非重试终端错误帧（Claude 下发 api_error，OpenAI 下发 error frame）
    fn emit_initial_error(&mut self, error_report: &StreamErrorReport) -> Vec<Bytes>;

    /// 可选：检查当前是否正处于思考块输出中（例如 Claude 的 Thinking block 处于打开状态）
    fn is_thinking_active(&self) -> bool {
        false
    }

    /// 可选钩子：心跳帧自定义渲染（缺省为标准 SSE 注释 `: ping\n\n`）
    fn heartbeat_frame(&mut self) -> Bytes {
        Bytes::from(": ping\n\n")
    }

    /// 可选钩子：流终止前提交会话状态（如 thinking_acc.commit(session_id)）
    fn on_finish(&mut self) {}
}

/// 流生命周期通用配置
#[derive(Debug, Clone)]
pub struct StreamLifecycleConfig {
    pub heartbeat_secs: u64,
    /// 首字等待宽限（涵盖排队、冷启动、超大 Prefill 及压缩任务，默认 180s）
    pub initial_ttft_secs: u64,
    /// 状态切换等待宽限（思考结束至首个正文/工具调用出字，重置基准点，默认 180s）
    pub transition_secs: u64,
    /// 稳态推流滑动超时（正文或思考吐字中相邻 token 最大间隔，默认 120s）
    pub streaming_sliding_secs: u64,
    pub adapter_name: &'static str,
    pub function_name: &'static str,
    pub trace_info: String,
}

impl StreamLifecycleConfig {
    pub fn new(
        adapter_name: &'static str,
        function_name: &'static str,
        trace_info: String,
    ) -> Self {
        let timeouts = crate::proxy::get_stream_timeout_config();
        Self {
            heartbeat_secs: 3,
            initial_ttft_secs: timeouts.initial_ttft_secs,
            transition_secs: timeouts.transition_secs,
            streaming_sliding_secs: timeouts.streaming_sliding_secs,
            adapter_name,
            function_name,
            trace_info,
        }
    }

    pub fn with_timeouts(
        mut self,
        initial_ttft_secs: u64,
        transition_secs: u64,
        streaming_sliding_secs: u64,
    ) -> Self {
        self.initial_ttft_secs = initial_ttft_secs;
        self.transition_secs = transition_secs;
        self.streaming_sliding_secs = streaming_sliding_secs;
        self
    }
}

/// 流式交互生命周期状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamPhase {
    /// 首字等待期（涵盖上游冷启动排队、大 Prompt prefill 及压缩任务）
    InitialTtft,
    /// 思考生成中
    Thinking,
    /// 状态切换过渡断崖（思考结束，正文或工具首字尚未到达；开启全新轮回）
    Transition,
    /// 稳态推流（持续吐正文或工具调用）
    Streaming,
}

/// 通用出站流生命周期驱动器（模板方法）
pub fn run_stream_lifecycle<S, E, H>(
    mut upstream_stream: Pin<Box<S>>,
    mut handler: H,
    config: StreamLifecycleConfig,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + ?Sized + 'static,
    E: std::fmt::Display + Send + 'static,
    H: ProtocolStreamHandler,
{
    use async_stream::stream;

    Box::pin(stream! {
        // 1. 发送可选的初始握手帧
        for frame in handler.initial_frames() {
            yield Ok(frame);
        }

        let mut buffer = BytesMut::new();
        let mut heartbeat_interval =
            tokio::time::interval(std::time::Duration::from_secs(config.heartbeat_secs));
        heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let mut phase = StreamPhase::InitialTtft;
        let mut last_activity = tokio::time::Instant::now();
        let mut error_emitted = false;

        loop {
            tokio::select! {
                next_chunk = upstream_stream.next() => {
                    match next_chunk {
                        Some(chunk_result) => {
                            match chunk_result {
                                Ok(chunk) => {
                                    buffer.extend_from_slice(&chunk);
                                    while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                                        let line_raw = buffer.split_to(pos + 1);
                                        let line_str = String::from_utf8_lossy(&line_raw);
                                        let line = line_str.trim();
                                        if line.is_empty() {
                                            continue;
                                        }

                                        let chunks = handler.process_line(line);
                                        for out_chunk in chunks {
                                            yield Ok(out_chunk);
                                        }

                                        // [关键终结状态机感知]
                                        // 若当前处理的数据行促使协议发射了终端终结帧（如 Claude 的 message_stop），
                                        // 立即终止解析并跳出生命周期循环，彻底断开下游响应体，阻止客户端因等待上游 TCP EOF 而悬挂计时。
                                        if handler.is_terminal() {
                                            break;
                                        }
                                    }

                                    if handler.is_terminal() {
                                        break;
                                    }

                                    // 状态机感知与多梯度超时刷新
                                    match phase {
                                        StreamPhase::InitialTtft => {
                                            if handler.has_thinking() {
                                                if handler.is_thinking_active() {
                                                    phase = StreamPhase::Thinking;
                                                } else if handler.has_content() {
                                                    phase = StreamPhase::Streaming;
                                                } else {
                                                    phase = StreamPhase::Transition;
                                                }
                                                last_activity = tokio::time::Instant::now();
                                            } else if handler.has_content() {
                                                phase = StreamPhase::Streaming;
                                                last_activity = tokio::time::Instant::now();
                                            } else {
                                                last_activity = tokio::time::Instant::now();
                                            }
                                        }
                                        StreamPhase::Thinking => {
                                            if !handler.is_thinking_active() {
                                                if handler.has_content() {
                                                    phase = StreamPhase::Streaming;
                                                    last_activity = tokio::time::Instant::now();
                                                } else {
                                                    // 思考结束，正文/工具调用首字尚未产生：跃迁至过渡断崖并重置基准点（全新轮回）
                                                    phase = StreamPhase::Transition;
                                                    last_activity = tokio::time::Instant::now();
                                                }
                                            } else {
                                                last_activity = tokio::time::Instant::now();
                                            }
                                        }
                                        StreamPhase::Transition => {
                                            if handler.has_content() {
                                                phase = StreamPhase::Streaming;
                                                last_activity = tokio::time::Instant::now();
                                            } else {
                                                last_activity = tokio::time::Instant::now();
                                            }
                                        }
                                        StreamPhase::Streaming => {
                                            last_activity = tokio::time::Instant::now();
                                        }
                                    }
                                }
                                Err(e) => {
                                    let report = crate::proxy::mappers::error_classifier::report_stream_error(
                                        config.adapter_name,
                                        config.function_name,
                                        &e,
                                        config.trace_info.clone(),
                                    );
                                    tracing::warn!(
                                        "[{}] {} upstream stream chunk error: {}",
                                        config.adapter_name,
                                        config.function_name,
                                        report.client_message()
                                    );

                                    // 若首包前发生异常（未产生任何正文与思考），下发协议专属的初始错误事件
                                    if !handler.has_content() && !handler.has_thinking() {
                                        let err_chunks = handler.emit_initial_error(&report);
                                        for c in err_chunks {
                                            yield Ok(c);
                                        }
                                    }
                                    error_emitted = true;
                                    // 无论处于何种阶段，中途发生上游网络断开时如实作为 Err 流出，
                                    // 严禁吞没错误伪造 end_turn 或 message_stop，交由客户端原生重试机制接管。
                                    yield Err(report.client_message());
                                    break;
                                }
                            }
                        }
                        None => {
                            // [关键契约守卫] 若上游到达 EOF 时仅产生了思考，从未产生任何正文或工具调用：
                            // 严禁作为正常 EOF 终结或伪造收尾帧！如实作为 Err 透传底层中断，
                            // 促使 Claude Code 等客户端状态机捕获 ConnectionLost 并自动触发原生重试。
                            if handler.has_thinking() && !handler.has_content() {
                                let report = crate::proxy::mappers::error_classifier::report_stream_error(
                                    config.adapter_name,
                                    config.function_name,
                                    &"upstream stream ended after thinking without content",
                                    config.trace_info.clone(),
                                );
                                tracing::warn!(
                                    "[{}] {} upstream stream reached EOF after thinking without content: {}",
                                    config.adapter_name,
                                    config.function_name,
                                    report.client_message()
                                );
                                error_emitted = true;
                                yield Err(report.client_message());
                                break;
                            }
                            break; // 上游流正常到达 EOF
                        }
                    }
                }
                _ = heartbeat_interval.tick() => {
                    let phase_timeout_secs = match phase {
                        StreamPhase::InitialTtft => config.initial_ttft_secs,
                        StreamPhase::Transition => config.transition_secs,
                        StreamPhase::Thinking | StreamPhase::Streaming => config.streaming_sliding_secs,
                    };

                    if last_activity.elapsed() >= std::time::Duration::from_secs(phase_timeout_secs) {
                        // [关键自适应跃迁：思考至正文过渡断崖检测]
                        // 若处于思考期，且静默时间达到稳态滑动超时，但正文/工具调用尚未产生，
                        // 说明思考吐字已完成，上游正处于生成正文或大工具调用的计算静默期（过渡断崖 Transition Cliff）。
                        // 此时绝不可直接超时误杀，而应自动跃迁至 Transition 状态，重置时间基准点，
                        // 赋予独立的 transition_secs (180s) 宽限期，并继续下发心跳保活。
                        if phase == StreamPhase::Thinking && !handler.has_content() {
                            tracing::info!(
                                "[{}] 思考流静默已达 {}s，检测到过渡断崖 (Transition Cliff)，自动跃迁至 Transition 状态并开启 {}s 宽限期",
                                config.adapter_name,
                                phase_timeout_secs,
                                config.transition_secs
                            );
                            phase = StreamPhase::Transition;
                            last_activity = tokio::time::Instant::now();
                            yield Ok(handler.heartbeat_frame());
                            continue;
                        }

                        let report = crate::proxy::mappers::error_classifier::report_stream_error(
                            config.adapter_name,
                            config.function_name,
                            &"stream idle timeout",
                            format!(
                                "{} phase={:?} timeout_secs={}",
                                config.trace_info, phase, phase_timeout_secs
                            ),
                        );
                        tracing::warn!(
                            "[{}] {} stream idle timeout after {}s in phase {:?}",
                            config.adapter_name,
                            config.function_name,
                            phase_timeout_secs,
                            phase
                        );

                        if !handler.has_content() && !handler.has_thinking() {
                            let err_chunks = handler.emit_initial_error(&report);
                            for c in err_chunks {
                                yield Ok(c);
                            }
                        }
                        error_emitted = true;
                        yield Err(report.client_message());
                        break;
                    }
                    yield Ok(handler.heartbeat_frame());
                }
            }
        }

        // 协议守卫：如果因错误退出，严格禁止追加发送终结帧，防止破坏客户端状态机
        if error_emitted {
            handler.on_finish();
            return;
        }

        // [关键契约守卫] 若流结束时仅产生了思考且未产生任何正文，严禁下发终结帧，如实透传 Err 触发重试
        if handler.has_thinking() && !handler.has_content() {
            let report = crate::proxy::mappers::error_classifier::report_stream_error(
                config.adapter_name,
                config.function_name,
                &"upstream stream ended after thinking without content",
                config.trace_info.clone(),
            );
            tracing::warn!(
                "[{}] {} stream ended after thinking without content: {}",
                config.adapter_name,
                config.function_name,
                report.client_message()
            );
            yield Err(report.client_message());
            handler.on_finish();
            return;
        }

        // 协议守卫：如果协议在流处理过程中已正常终结（例如已发送 message_stop），
        // 严禁冲刷残留缓冲区或再次调用 emit_finalize()，防止向客户端投毒多余数据帧导致 StreamMalformedEventError
        if handler.is_terminal() {
            handler.on_finish();
            return;
        }

        // [FIX #1732] 尾部数据安全 Flush：防止因末尾缺少换行符产生网络分片悬挂
        if !buffer.is_empty() {
            let line_str = String::from_utf8_lossy(&buffer);
            let line = line_str.trim();
            if !line.is_empty() {
                let chunks = handler.process_line(line);
                for out_chunk in chunks {
                    yield Ok(out_chunk);
                }
            }
            buffer.clear();
        }

        // 正常收尾
        let final_chunks = handler.emit_finalize();
        for c in final_chunks {
            yield Ok(c);
        }

        handler.on_finish();
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockHandler {
        processed_lines: Vec<String>,
        has_content: bool,
        has_thinking: bool,
        is_thinking_active: bool,
        finalized: bool,
        initial_error_emitted: bool,
        is_terminal: bool,
    }

    impl MockHandler {
        fn new(has_content: bool, has_thinking: bool) -> Self {
            Self {
                processed_lines: Vec::new(),
                has_content,
                has_thinking,
                is_thinking_active: false,
                finalized: false,
                initial_error_emitted: false,
                is_terminal: false,
            }
        }
    }

    impl ProtocolStreamHandler for MockHandler {
        fn process_line(&mut self, line: &str) -> Vec<Bytes> {
            self.processed_lines.push(line.to_string());
            if line.contains("thinking_chunk") {
                self.has_thinking = true;
                self.is_thinking_active = true;
            } else if line.contains("thinking_done") {
                self.has_thinking = true;
                self.is_thinking_active = false;
            } else if line.contains("terminal_stop") {
                self.has_content = true;
                self.is_terminal = true;
            } else {
                self.has_content = true;
            }
            vec![Bytes::from(format!("processed:{}\n", line))]
        }

        fn is_terminal(&self) -> bool {
            self.is_terminal
        }

        fn has_content(&self) -> bool {
            self.has_content
        }

        fn has_thinking(&self) -> bool {
            self.has_thinking
        }

        fn is_thinking_active(&self) -> bool {
            self.is_thinking_active
        }

        fn emit_finalize(&mut self) -> Vec<Bytes> {
            self.finalized = true;
            vec![Bytes::from("[finalized]\n")]
        }

        fn emit_initial_error(&mut self, _report: &StreamErrorReport) -> Vec<Bytes> {
            self.initial_error_emitted = true;
            vec![Bytes::from("[initial_error]\n")]
        }
    }

    #[tokio::test]
    async fn test_lifecycle_normal_flow_and_flush() {
        let mock_stream = async_stream::stream! {
            yield Ok::<_, String>(Bytes::from("data: hello\n"));
            yield Ok::<_, String>(Bytes::from("data: world_without_newline"));
        };

        let handler = MockHandler::new(false, false);
        let config = StreamLifecycleConfig::new("test", "test_fn", "trace_1".to_string());
        let mut stream = run_stream_lifecycle(Box::pin(mock_stream), handler, config);

        let mut output = Vec::new();
        while let Some(item) = stream.next().await {
            if let Ok(b) = item {
                output.push(String::from_utf8_lossy(&b).to_string());
            }
        }

        let full_output = output.join("");
        assert!(full_output.contains("processed:data: hello"));
        assert!(full_output.contains("processed:data: world_without_newline"));
        assert!(full_output.contains("[finalized]"));
        assert!(!full_output.contains("[initial_error]"));
    }

    #[tokio::test]
    async fn test_lifecycle_initial_error_stops_without_finalize() {
        let mock_stream = async_stream::stream! {
            yield Err::<Bytes, _>("connection failed before any data".to_string());
        };

        let handler = MockHandler::new(false, false);
        let config = StreamLifecycleConfig::new("test", "test_fn", "trace_2".to_string());
        let mut stream = run_stream_lifecycle(Box::pin(mock_stream), handler, config);

        let mut output = Vec::new();
        while let Some(item) = stream.next().await {
            if let Ok(b) = item {
                output.push(String::from_utf8_lossy(&b).to_string());
            }
        }

        let full_output = output.join("");
        assert!(full_output.contains("[initial_error]"));
        assert!(
            !full_output.contains("[finalized]"),
            "Must NOT finalize after initial error"
        );
    }

    #[tokio::test]
    async fn test_lifecycle_interruption_after_content_propagates_error_without_finalize() {
        let mock_stream = async_stream::stream! {
            yield Ok::<_, String>(Bytes::from("data: partial content\n"));
            yield Err::<Bytes, _>("connection reset mid stream".to_string());
        };

        let handler = MockHandler::new(false, false);
        let config = StreamLifecycleConfig::new("test", "test_fn", "trace_3".to_string());
        let mut stream = run_stream_lifecycle(Box::pin(mock_stream), handler, config);

        let mut output = Vec::new();
        let mut error_received = false;
        while let Some(item) = stream.next().await {
            match item {
                Ok(b) => output.push(String::from_utf8_lossy(&b).to_string()),
                Err(e) => {
                    error_received = true;
                    assert!(e.contains("connection reset mid stream"));
                }
            }
        }

        let full_output = output.join("");
        assert!(full_output.contains("processed:data: partial content"));
        assert!(
            !full_output.contains("[finalized]"),
            "Must NOT finalize when error occurs"
        );
        assert!(
            !full_output.contains("[initial_error]"),
            "Must NOT emit initial error when content exists"
        );
        assert!(error_received, "Stream must propagate Err on interruption");
    }

    #[tokio::test]
    async fn test_lifecycle_gradient_timeouts_and_transition() {
        let mock_stream = async_stream::stream! {
            yield Ok::<_, String>(Bytes::from("data: thinking_chunk\n"));
            yield Ok::<_, String>(Bytes::from("data: thinking_done\n"));
            yield Ok::<_, String>(Bytes::from("data: content_final\n"));
        };

        let handler = MockHandler::new(false, false);
        let config = StreamLifecycleConfig::new("test", "test_fn", "trace_gradient".to_string())
            .with_timeouts(5, 5, 2);
        let mut stream = run_stream_lifecycle(Box::pin(mock_stream), handler, config);

        let mut output = Vec::new();
        while let Some(item) = stream.next().await {
            if let Ok(b) = item {
                output.push(String::from_utf8_lossy(&b).to_string());
            }
        }

        let full_output = output.join("");
        assert!(full_output.contains("processed:data: thinking_chunk"));
        assert!(full_output.contains("processed:data: thinking_done"));
        assert!(full_output.contains("processed:data: content_final"));
        assert!(full_output.contains("[finalized]"));
        assert!(!full_output.contains("[initial_error]"));
    }

    #[tokio::test]
    async fn test_lifecycle_silent_transition_cliff_auto_promotion() {
        // 模拟真实网络环境中思考吐字完毕后无任何显式控制块，
        // 上游静默时间超过稳态推流超时（streaming_sliding_secs=1s）后才产生正文。
        // 验证状态机不会在 1s 被误杀，而是自动跃迁至 Transition 状态赋予 4s 宽限，成功收尾。
        let mock_stream = async_stream::stream! {
            yield Ok::<_, String>(Bytes::from("data: thinking_chunk\n"));
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            yield Ok::<_, String>(Bytes::from("data: content_final\n"));
        };

        let handler = MockHandler::new(false, false);
        let config = StreamLifecycleConfig::new("test", "test_fn", "trace_cliff".to_string())
            .with_timeouts(5, 4, 1);
        let mut stream = run_stream_lifecycle(Box::pin(mock_stream), handler, config);

        let mut output = Vec::new();
        while let Some(item) = stream.next().await {
            if let Ok(b) = item {
                output.push(String::from_utf8_lossy(&b).to_string());
            }
        }

        let full_output = output.join("");
        assert!(full_output.contains("processed:data: thinking_chunk"));
        assert!(full_output.contains("processed:data: content_final"));
        assert!(full_output.contains("[finalized]"));
        assert!(!full_output.contains("[truncated]"));
        assert!(!full_output.contains("[initial_error]"));
    }

    #[tokio::test]
    async fn test_lifecycle_terminates_immediately_when_handler_is_terminal_without_waiting_for_upstream_eof(
    ) {
        // 模拟上游在产生终端帧（terminal_stop）后，连接保持悬挂未关闭（例如长达 60s 不发送任何数据或 EOF）
        // 验证生命周期驱动器能立刻感知 handler.is_terminal()，毫秒级跳出循环并关闭下游流，绝不等待上游 EOF 或发送心跳
        let mock_stream = async_stream::stream! {
            yield Ok::<_, String>(Bytes::from("data: normal_token\n"));
            yield Ok::<_, String>(Bytes::from("data: terminal_stop\n"));
            // 上游流保持存活但处于长时间等待中（未发送 EOF）
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            yield Ok::<_, String>(Bytes::from("data: never_reached\n"));
        };

        let handler = MockHandler::new(false, false);
        let config = StreamLifecycleConfig::new("test", "test_fn", "trace_term".to_string())
            .with_timeouts(5, 5, 5);
        let mut stream = run_stream_lifecycle(Box::pin(mock_stream), handler, config);

        let mut output = Vec::new();
        // 收集已产出的帧，预期只包含 normal_token 和 terminal_stop，之后必须立即返回 None
        while let Ok(Some(item)) =
            tokio::time::timeout(std::time::Duration::from_millis(500), stream.next()).await
        {
            if let Ok(b) = item {
                output.push(String::from_utf8_lossy(&b).to_string());
            }
        }

        let full_output = output.join("");
        assert!(full_output.contains("processed:data: normal_token"));
        assert!(full_output.contains("processed:data: terminal_stop"));
        assert!(!full_output.contains("never_reached"));

        // 验证此时下游流已经完全关闭（直接返回 None，不会超时阻塞 500ms）
        let next_item =
            tokio::time::timeout(std::time::Duration::from_millis(100), stream.next()).await;
        assert_eq!(
            next_item,
            Ok(None),
            "Stream must terminate immediately upon handler.is_terminal() without waiting for upstream EOF"
        );
    }

    #[tokio::test]
    async fn test_lifecycle_thinking_only_interruption_propagates_error_without_finalize() {
        // 模拟上游只产生了思考块，随后由于网络异常或上游故障直接 EOF，未产出任何正文
        let mock_stream = async_stream::stream! {
            yield Ok::<_, String>(Bytes::from("data: thinking_chunk\n"));
            yield Ok::<_, String>(Bytes::from("data: thinking_done\n"));
            // 直接 EOF (None)
        };

        let handler = MockHandler::new(false, false);
        let config =
            StreamLifecycleConfig::new("test", "test_fn", "trace_thinking_only".to_string())
                .with_timeouts(5, 5, 5);
        let mut stream = run_stream_lifecycle(Box::pin(mock_stream), handler, config);

        let mut output = Vec::new();
        let mut error_propagated = false;
        while let Some(item) = stream.next().await {
            match item {
                Ok(b) => output.push(String::from_utf8_lossy(&b).to_string()),
                Err(e) => {
                    error_propagated = true;
                    assert!(e.contains("thinking"));
                }
            }
        }

        let full_output = output.join("");
        assert!(full_output.contains("processed:data: thinking_chunk"));
        assert!(full_output.contains("processed:data: thinking_done"));
        assert!(
            !full_output.contains("[finalized]"),
            "Must NOT emit finalize when stream only had thinking"
        );
        assert!(
            error_propagated,
            "Must propagate Err on thinking-only stream EOF to trigger client native retry"
        );
    }
}
