use super::super::client_adapter::{
    get_user_agent, ClientAdapter, Protocol, SignatureBufferStrategy,
};
use axum::http::{HeaderMap, HeaderValue};

/// Opencode CLI 客户端适配器
///
/// Opencode 是一个支持多协议的 AI CLI 工具，支持：
/// - Anthropic
/// - OpenAI
/// - OA-Compatible
/// - Google/Gemini
///
/// 该适配器提供以下定制策略：
/// 1. FIFO 签名管理策略（适应多并发工具调用）
/// 2. 标准化 SSE 错误格式（通过客户端的 Zod 类型检查）
/// 3. 自动注入 `context-1m-2025-08-07` beta header
pub struct OpencodeAdapter;

impl ClientAdapter for OpencodeAdapter {
    fn matches(&self, headers: &HeaderMap) -> bool {
        get_user_agent(headers)
            .map(|ua| ua.to_lowercase().contains("opencode"))
            .unwrap_or(false)
    }

    fn bypass_signature_matching(&self) -> bool {
        // Opencode 对签名校验较为宽松
        false
    }

    fn let_it_crash(&self) -> bool {
        // Opencode 倾向于快速失败，减少不必要的重试
        true
    }

    fn signature_buffer_strategy(&self) -> SignatureBufferStrategy {
        // 使用 FIFO 策略以适应多并发工具调用
        SignatureBufferStrategy::Fifo
    }

    fn inject_beta_headers(&self, headers: &mut HeaderMap) {
        // 注入 context-1m beta header
        let value = HeaderValue::from_static("context-1m-2025-08-07");
        headers.insert("anthropic-beta", value);
    }

    fn supported_protocols(&self) -> Vec<Protocol> {
        vec![
            Protocol::Anthropic,
            Protocol::OpenAI,
            Protocol::OACompatible,
            Protocol::GoogleGemini,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opencode_adapter_matches() {
        let adapter = OpencodeAdapter;

        let mut headers = HeaderMap::new();
        headers.insert("user-agent", HeaderValue::from_static("opencode/1.0.0"));

        assert!(adapter.matches(&headers));
    }

    #[test]
    fn test_opencode_adapter_case_insensitive() {
        let adapter = OpencodeAdapter;

        let mut headers = HeaderMap::new();
        headers.insert("user-agent", HeaderValue::from_static("OpenCode/1.0.0"));

        assert!(adapter.matches(&headers));
    }

    #[test]
    fn test_opencode_adapter_no_match() {
        let adapter = OpencodeAdapter;

        let mut headers = HeaderMap::new();
        headers.insert("user-agent", HeaderValue::from_static("curl/7.68.0"));

        assert!(!adapter.matches(&headers));
    }

    #[test]
    fn test_opencode_adapter_strategies() {
        let adapter = OpencodeAdapter;

        assert!(adapter.let_it_crash());
        assert_eq!(
            adapter.signature_buffer_strategy(),
            SignatureBufferStrategy::Fifo
        );
    }

    #[test]
    fn test_opencode_adapter_protocols() {
        let adapter = OpencodeAdapter;

        let protocols = adapter.supported_protocols();
        assert_eq!(protocols.len(), 4);
        assert!(protocols.contains(&Protocol::Anthropic));
        assert!(protocols.contains(&Protocol::OpenAI));
        assert!(protocols.contains(&Protocol::OACompatible));
        assert!(protocols.contains(&Protocol::GoogleGemini));
    }

    #[test]
    fn test_opencode_adapter_beta_headers() {
        let adapter = OpencodeAdapter;

        let mut headers = HeaderMap::new();
        adapter.inject_beta_headers(&mut headers);

        assert!(headers.contains_key("anthropic-beta"));
        assert_eq!(
            headers.get("anthropic-beta").unwrap().to_str().unwrap(),
            "context-1m-2025-08-07"
        );
    }
}
