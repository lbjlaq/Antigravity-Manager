use super::client_adapters::OpencodeAdapter;
use axum::http::HeaderMap;
use once_cell::sync::Lazy;
use std::sync::Arc; // [NEW] Import Arc

/// 客户端适配器 trait
///
/// 为不同的客户端（如 opencode、Cherry Studio）提供定制化的协议处理策略。
/// 每个客户端可以实现自己的适配器来处理特定的需求。
///
/// # 设计原则
/// 1. **完全隔离**：适配器作为可选的增强层，不修改现有协议核心逻辑
/// 2. **向后兼容**：未匹配到适配器的请求完全按照现有流程处理
/// 3. **单文件修改**：客户端特定逻辑封装在各自的适配器文件中
pub trait ClientAdapter: Send + Sync {
    /// 判断该适配器是否匹配给定的请求
    ///
    /// # Arguments
    /// * `headers` - 请求头，通常通过 User-Agent 等字段识别客户端
    ///
    /// # Returns
    /// 如果匹配返回 true，否则返回 false
    fn matches(&self, headers: &HeaderMap) -> bool;

    /// 是否绕过签名校验
    ///
    /// 某些客户端可能不需要严格的 thinking 签名匹配
    #[allow(dead_code)]
    fn bypass_signature_matching(&self) -> bool {
        false
    }

    /// 是否采用 "let it crash" 哲学
    ///
    /// 减少不必要的重试和恢复逻辑，让错误快速暴露
    fn let_it_crash(&self) -> bool {
        false
    }

    /// 签名缓存策略
    ///
    /// 不同客户端可能需要不同的签名管理方式（FIFO/LIFO）
    fn signature_buffer_strategy(&self) -> SignatureBufferStrategy {
        SignatureBufferStrategy::Default
    }

    /// 注入客户端缺少的 Beta Header
    ///
    /// 某些客户端可能需要特定的 Beta Header 才能正常工作
    fn inject_beta_headers(&self, _headers: &mut HeaderMap) {
        // 默认不注入
    }

    /// 声明支持的协议
    ///
    /// 用于多协议客户端（如 opencode）
    #[allow(dead_code)]
    fn supported_protocols(&self) -> Vec<Protocol> {
        vec![Protocol::Anthropic] // 默认只支持 Anthropic
    }
}

/// 签名缓存策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureBufferStrategy {
    /// 默认策略（当前实现）
    Default,
    /// FIFO（先进先出）- 适用于多并发工具调用
    Fifo,
    /// LIFO（后进先出）- 适用于嵌套调用
    #[allow(dead_code)]
    Lifo,
}

/// 支持的协议类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Protocol {
    Anthropic,
    OpenAI,
    OACompatible,
    GoogleGemini,
}

/// 全局客户端适配器注册表
///
/// 所有注册的适配器都会在请求处理时被检查
pub static CLIENT_ADAPTERS: Lazy<Vec<Arc<dyn ClientAdapter>>> = Lazy::new(|| {
    vec![
        Arc::new(OpencodeAdapter),
        // 未来可以轻松添加更多适配器:
        // Arc::new(CherryStudioAdapter),
    ]
});

/// 辅助函数：从 HeaderMap 中提取 User-Agent
pub fn get_user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    struct TestAdapter;

    impl ClientAdapter for TestAdapter {
        fn matches(&self, headers: &HeaderMap) -> bool {
            get_user_agent(headers)
                .map(|ua| ua.contains("test-client"))
                .unwrap_or(false)
        }

        fn bypass_signature_matching(&self) -> bool {
            true
        }
    }

    #[test]
    fn test_adapter_matches() {
        let adapter = TestAdapter;

        let mut headers = HeaderMap::new();
        headers.insert("user-agent", HeaderValue::from_static("test-client/1.0"));

        assert!(adapter.matches(&headers));
        assert!(adapter.bypass_signature_matching());
    }

    #[test]
    fn test_adapter_no_match() {
        let adapter = TestAdapter;

        let mut headers = HeaderMap::new();
        headers.insert("user-agent", HeaderValue::from_static("other-client/1.0"));

        assert!(!adapter.matches(&headers));
    }

    #[test]
    fn test_get_user_agent() {
        let mut headers = HeaderMap::new();
        headers.insert("user-agent", HeaderValue::from_static("opencode/1.0"));

        assert_eq!(get_user_agent(&headers), Some("opencode/1.0".to_string()));
    }
}
