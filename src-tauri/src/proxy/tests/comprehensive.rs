#[cfg(test)]
mod tests {
    use crate::proxy::mappers::claude::models::{
        ClaudeRequest, Message, MessageContent, ThinkingConfig,
    };
    use crate::proxy::mappers::claude::request::transform_claude_request_in;

    // ==================================================================================
    // 场景一：首次 Thinking 请求 (P0-2 Fix)
    // 验证在没有历史签名的情况下，首次发起 Thinking 请求是否被放行 (Perimssive Mode)
    // ==================================================================================
    #[test]
    fn test_first_thinking_request_permissive_mode() {
        // 1. 构造一个全新的请求 (无历史消息)
        let req = ClaudeRequest {
            model: "claude-3-7-sonnet-20250219".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("Hello, please think.".to_string()),
            }],
            system: None,
            tools: None, // 无工具调用
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: Some(ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: Some(1024),
                effort: None,
                display: None,
            }),
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
            tool_choice: None,
        };

        // 2. 执行转换
        // 如果修复生效，这里应该成功返回，且 thinkingConfig 被保留
        let result =
            transform_claude_request_in(&req, "test-project", false, None, "test_session", None);
        assert!(result.is_ok(), "First thinking request should be allowed");

        let body = result.unwrap();
        let request = &body["request"];

        // 验证 thinkingConfig 是否存在 (即 thinking 模式未被禁用)
        let has_thinking_config = request
            .get("generationConfig")
            .and_then(|g| g.get("thinkingConfig"))
            .is_some();

        assert!(
            has_thinking_config,
            "Thinking config should be preserved for first request without tool calls"
        );
    }

    // ==================================================================================
    // 场景三：跨模型兼容性 (P1-5 Fix) - 模拟
    // 由于 request.rs 中的 is_model_compatible 是私有的，我们通过集成测试验证效果
    // ==================================================================================
    /*
       注意：由于 is_model_compatible 和缓存逻辑深度集成在 transform_claude_request_in 中，
       且依赖全局单例 SignatureCache，单元测试较难模拟 "缓存了旧签名但切换了模型" 的状态。
       这里主要通过验证 "不兼容签名被丢弃" 的副作用（即 thoughtSignature 字段消息）来测试。
       但由于 SignatureCache 是全局的，我们无法在测试中轻易预置状态。
       因此，此场景主要依赖 Verification Guide 中的手动测试。
       或者，我们可以测试 request.rs 中公开的某些 helper (如果有的话)，但目前没有。
    */
}
