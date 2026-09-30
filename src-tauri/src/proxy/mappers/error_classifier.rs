// 错误分类模块 - 将上游字节流错误转换为协议无关的类型与用户消息。
// 各协议适配器只负责套自己的 SSE 信封，不得再按原文猜类型。

/// 协议无关的流错误分类结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassifiedStreamError {
    /// 错误类型，写入各协议信封的 type / code
    pub error_type: &'static str,
    /// 英文 fallback 消息，供非浏览器客户端使用
    pub message: &'static str,
    /// 前端翻译键
    pub i18n_key: &'static str,
}

impl ClassifiedStreamError {
    pub const TIMEOUT: Self = Self {
        error_type: "timeout_error",
        message: "Request timeout, please check your network connection",
        i18n_key: "errors.stream.timeout_error",
    };
    pub const CONNECTION: Self = Self {
        error_type: "connection_error",
        message: "Connection failed, please check your network or proxy settings",
        i18n_key: "errors.stream.connection_error",
    };
    pub const DECODE: Self = Self {
        error_type: "decode_error",
        message: "Network unstable, data transmission interrupted. Try: 1) Check network 2) Switch proxy 3) Retry",
        i18n_key: "errors.stream.decode_error",
    };
    pub const STREAM: Self = Self {
        error_type: "stream_error",
        message: "Stream interrupted before completion, please retry",
        i18n_key: "errors.stream.stream_error",
    };
    pub const UNKNOWN: Self = Self {
        error_type: "unknown_error",
        message: "Unknown error occurred",
        i18n_key: "errors.stream.unknown_error",
    };
}

/// 分类上游字节流错误。判定只看错误文本，与客户端协议无关。
pub fn classify_stream_error<E: std::fmt::Display>(error: &E) -> ClassifiedStreamError {
    let error_str = error.to_string().to_lowercase();

    // 1. 超时优先：`connection timed out` 同时带 connection 与 timeout。
    if error_str.contains("timeout")
        || error_str.contains("timed out")
        || error_str.contains("deadline")
    {
        return ClassifiedStreamError::TIMEOUT;
    }

    // 2. 握手 / DNS / 代理：HTTP body 从未成功打开。
    if is_connect_failure(&error_str) {
        return ClassifiedStreamError::CONNECTION;
    }

    // 3. 中途掐断：HTTP 已成功，body 传到一半被对端或代理切断。
    // 必须放在笼统的 `connection` 匹配之前，否则会误报成网络/代理故障。
    if is_incomplete_body(&error_str) {
        return ClassifiedStreamError::STREAM;
    }

    // 4. 剩余带 connection 的握手失败（未覆盖的变体）。
    if error_str.contains("connection") {
        return ClassifiedStreamError::CONNECTION;
    }

    // 5. JSON / SSE 解析失败。不要用裸 `decode`：hyper 的
    // `error decoding response body` 属于中途掐断，已在步骤 3 处理。
    if error_str.contains("parse")
        || error_str.contains("invalid json")
        || error_str.contains("syntax error")
        || error_str.contains("at line ")
        || error_str.contains("eof while parsing")
    {
        return ClassifiedStreamError::DECODE;
    }

    if error_str.contains("stream") || error_str.contains("body") {
        return ClassifiedStreamError::STREAM;
    }

    ClassifiedStreamError::UNKNOWN
}

/// 带调用点的内部流错误报告。各协议只把 `client_message()` 写入自己的信封。
#[derive(Debug, Clone)]
pub struct StreamErrorReport {
    pub classified: ClassifiedStreamError,
    pub adapter: &'static str,
    pub function: &'static str,
    pub file: &'static str,
    pub line: u32,
    pub params: String,
    pub raw: String,
}

impl StreamErrorReport {
    pub fn call_site(&self) -> String {
        format!("{}:{}", self.file, self.line)
    }

    /// 客户端可见诊断：类型说明 + 函数 + 调用点 + 入参 + 脱敏原文。
    pub fn client_message(&self) -> String {
        format!(
            "{} | fn={} | at={} | params={{ {} }} | raw={}",
            self.classified.message,
            self.function,
            self.call_site(),
            self.params,
            self.raw
        )
    }
}

/// 记录并返回带定位的流错误。`#[track_caller]` 把调用点钉在适配器里那一行。
#[track_caller]
pub fn report_stream_error(
    adapter: &'static str,
    function: &'static str,
    error: &impl std::fmt::Display,
    params: impl std::fmt::Display,
) -> StreamErrorReport {
    let loc = std::panic::Location::caller();
    let classified = classify_stream_error(error);
    let report = StreamErrorReport {
        classified,
        adapter,
        function,
        file: loc.file(),
        line: loc.line(),
        params: params.to_string(),
        raw: sanitize_raw(&error.to_string()),
    };
    tracing::error!(
        adapter = report.adapter,
        function = report.function,
        call_site = %report.call_site(),
        params = %report.params,
        error_type = report.classified.error_type,
        raw = %report.raw,
        "upstream stream interrupted"
    );
    report
}

/// 非流式收集器使用的诊断消息。
#[track_caller]
pub fn format_stream_error<E: std::fmt::Display>(error: &E) -> String {
    report_stream_error("collector", "collect_stream_to_json", error, "-").client_message()
}

/// SSE/JSON 原文预览，截断并脱敏后写入诊断。
pub fn preview_payload(raw: &str) -> String {
    let truncated = crate::proxy::mappers::common_utils::safe_truncate_str(raw.trim(), 160);
    sanitize_raw(&truncated)
}

fn sanitize_raw(raw: &str) -> String {
    crate::proxy::upstream::client::sanitize_error_for_log(raw)
}

fn is_connect_failure(error_str: &str) -> bool {
    error_str.contains("trying to connect")
        || error_str.contains("failed to connect")
        || error_str.contains("error connecting")
        || error_str.contains("dns")
        || error_str.contains("failed to lookup")
        || error_str.contains("name or service not known")
        || error_str.contains("no route to host")
        || error_str.contains("network is unreachable")
        || error_str.contains("connection refused")
        || error_str.contains("proxy")
        || error_str.contains("socks")
        || error_str.contains("tls handshake")
        || error_str.contains("ssl handshake")
        || error_str.contains("certificate")
}

fn is_incomplete_body(error_str: &str) -> bool {
    error_str.contains("closed before message completed")
        || error_str.contains("incomplete message")
        || error_str.contains("error reading a body")
        || error_str.contains("decoding response body")
        || error_str.contains("unexpected eof")
        || error_str.contains("unexpected end of file")
        || error_str.contains("broken pipe")
        || error_str.contains("connection reset")
        || error_str.contains("reset by peer")
        || error_str.contains("connection aborted")
        || error_str.contains("forcibly closed")
        || error_str.contains("peer closed")
        || error_str.contains("os error 10054")
        || error_str.contains("os error 104")
        || error_str.contains("os error 32")
        || error_str.contains("os error 54")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(error: &str) -> ClassifiedStreamError {
        classify_stream_error(&error)
    }

    #[test]
    fn timeout_wins_over_connection_wording() {
        let classified = classify("Connection timed out after 30s");
        assert_eq!(classified, ClassifiedStreamError::TIMEOUT);
        assert_eq!(classified.i18n_key, "errors.stream.timeout_error");
    }

    #[test]
    fn connect_time_dns_is_connection_error() {
        let classified =
            classify("error trying to connect: dns error: failed to lookup address information");
        assert_eq!(classified, ClassifiedStreamError::CONNECTION);
    }

    #[test]
    fn connect_time_reset_is_connection_error() {
        let classified = classify("error trying to connect: connection reset by peer");
        assert_eq!(classified, ClassifiedStreamError::CONNECTION);
    }

    #[test]
    fn connection_refused_is_connection_error() {
        let classified = classify("error trying to connect: tcp connect error: connection refused");
        assert_eq!(classified, ClassifiedStreamError::CONNECTION);
    }

    #[test]
    fn incomplete_body_is_stream_error_not_connection_error() {
        let classified = classify(
            "error reading a body from connection: connection closed before message completed",
        );
        assert_eq!(classified, ClassifiedStreamError::STREAM);
        assert_eq!(
            classified.message,
            "Stream interrupted before completion, please retry"
        );
    }

    #[test]
    fn decoding_response_body_is_stream_error_not_decode_error() {
        let classified =
            classify("error decoding response body: connection closed before message completed");
        assert_eq!(classified, ClassifiedStreamError::STREAM);
    }

    #[test]
    fn midstream_reset_by_peer_is_stream_error() {
        let classified = classify(
            "error reading a body from connection: connection reset by peer (os error 104)",
        );
        assert_eq!(classified, ClassifiedStreamError::STREAM);
    }

    #[test]
    fn broken_pipe_is_stream_error() {
        assert_eq!(classify("broken pipe"), ClassifiedStreamError::STREAM);
    }

    #[test]
    fn json_parse_is_decode_error() {
        assert_eq!(
            classify("failed to parse json: expected value"),
            ClassifiedStreamError::DECODE
        );
        assert_eq!(
            classify("expected value at line 1 column 1"),
            ClassifiedStreamError::DECODE
        );
    }

    #[test]
    fn format_stream_error_includes_location_and_raw() {
        let msg = format_stream_error(
            &"error reading a body from connection: connection closed before message completed",
        );
        assert!(msg.contains("Stream interrupted before completion, please retry"));
        assert!(msg.contains("fn=collect_stream_to_json"));
        assert!(msg.contains("at="));
        assert!(msg.contains("closed before message completed"));
    }

    #[test]
    fn report_includes_function_params_and_call_site() {
        let report = report_stream_error(
            "openai",
            "create_openai_sse_stream_with_anchor",
            &"error reading a body from connection: connection closed before message completed",
            "model=gemini-3.8-flash-high session=abc buffer_bytes=12",
        );
        let msg = report.client_message();
        assert_eq!(report.classified, ClassifiedStreamError::STREAM);
        assert!(msg.contains("fn=create_openai_sse_stream_with_anchor"));
        assert!(msg.contains("model=gemini-3.8-flash-high"));
        assert!(msg.contains("session=abc"));
        assert!(msg.contains("error_classifier.rs"));
        assert!(msg.contains("closed before message completed"));
        assert!(!msg.contains("network or proxy"));
    }

    #[test]
    fn i18n_keys_match_error_types() {
        for classified in [
            ClassifiedStreamError::TIMEOUT,
            ClassifiedStreamError::CONNECTION,
            ClassifiedStreamError::DECODE,
            ClassifiedStreamError::STREAM,
            ClassifiedStreamError::UNKNOWN,
        ] {
            assert_eq!(
                classified.i18n_key,
                format!("errors.stream.{}", classified.error_type)
            );
            assert!(!classified.message.is_empty());
        }
    }
}
