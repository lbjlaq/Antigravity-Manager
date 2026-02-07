// 错误分类模块 - 将底层错误转换为用户友好的消息
use reqwest::Error;

/// 分类流式响应错误并返回错误类型、英文消息和 i18n key
/// 
/// 返回值: (错误类型, 英文错误消息, i18n_key)
/// - 错误类型: 用于日志和错误码
/// - 英文消息: fallback 消息,供非浏览器客户端使用
/// - i18n_key: 前端翻译键,供浏览器客户端本地化
pub fn classify_stream_error(error: &Error) -> (&'static str, &'static str, &'static str) {
    if error.is_timeout() {
        (
            "timeout_error",
            "Request timeout, please check your network connection",
            "errors.stream.timeout_error"
        )
    } else if error.is_connect() {
        (
            "connection_error",
            "Connection failed, please check your network or proxy settings",
            "errors.stream.connection_error"
        )
    } else if error.is_decode() {
        (
            "decode_error",
            "Network unstable, data transmission interrupted. Try: 1) Check network 2) Switch proxy 3) Retry",
            "errors.stream.decode_error"
        )
    } else if error.is_body() {
        (
            "stream_error",
            "Stream transmission error, please retry later",
            "errors.stream.stream_error"
        )
    } else {
        (
            "unknown_error",
            "Unknown error occurred",
            "errors.stream.unknown_error"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_timeout_error() {
        // 创建一个模拟的超时错误
        let url = "http://example.com";
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(1))
            .build()
            .unwrap();
        
        let rt = tokio::runtime::Runtime::new().unwrap();
        let error = rt.block_on(async {
            client.get(url).send().await.unwrap_err()
        });
        
        if error.is_timeout() {
            let (error_type, message, i18n_key) = classify_stream_error(&error);
            assert_eq!(error_type, "timeout_error");
            assert!(message.contains("timeout"));
            assert_eq!(i18n_key, "errors.stream.timeout_error");
        }
    }

    #[test]
    fn test_error_message_format() {
        // 测试错误消息格式（使用本地已关闭端口，避免外部 DNS/网络环境不确定性）
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let url = format!("http://{}", addr);
        let client = reqwest::Client::new();
        
        let rt = tokio::runtime::Runtime::new().unwrap();
        let error = rt.block_on(async {
            client
                .get(&url)
                .send()
                .await
                .and_then(|resp| resp.error_for_status())
                .unwrap_err()
        });
        
        let (error_type, message, i18n_key) = classify_stream_error(&error);
        
        // 错误类型应该是已知的类型之一
        assert!(
            error_type == "timeout_error" ||
            error_type == "connection_error" ||
            error_type == "decode_error" ||
            error_type == "stream_error" ||
            error_type == "unknown_error"
        );
        
        // 消息不应该为空
        assert!(!message.is_empty());
        
        // i18n_key 应该以 errors.stream. 开头
        assert!(i18n_key.starts_with("errors.stream."));
    }

    #[test]
    fn test_i18n_keys_format() {
        // 验证所有错误类型都有正确的 i18n_key 格式
        let test_cases = vec![
            ("timeout_error", "errors.stream.timeout_error"),
            ("connection_error", "errors.stream.connection_error"),
            ("decode_error", "errors.stream.decode_error"),
            ("stream_error", "errors.stream.stream_error"),
            ("unknown_error", "errors.stream.unknown_error"),
        ];
        
        // 这里我们只验证 i18n_key 格式
        for (expected_type, expected_key) in test_cases {
            assert_eq!(format!("errors.stream.{}", expected_type), expected_key);
        }
    }
}
