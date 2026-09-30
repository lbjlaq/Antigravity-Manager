// Claude 辅助函数
// JSON Schema 清理、签名处理等

// 已移除未使用的 Value 导入

/// 将 JSON Schema 中的类型名称转为大写 (Gemini 要求)
/// 例如: "string" -> "STRING", "integer" -> "INTEGER"
// 已移除未使用的 uppercase_schema_types 函数

/// 根据模型名称获取上下文 Token 限制
pub fn get_context_limit_for_model(model: &str) -> u32 {
    if model.contains("pro") {
        2_097_152 // 2M for Pro
    } else if model.contains("flash") {
        1_048_576 // 1M for Flash
    } else {
        1_048_576 // Default 1M
    }
}

pub fn to_claude_usage(
    usage_metadata: &super::models::UsageMetadata,
    scaling_enabled: bool,
    context_limit: u32,
) -> super::models::Usage {
    let canonical = crate::proxy::pipeline::CanonicalUsage::new(
        usage_metadata.prompt_token_count.unwrap_or(0),
        usage_metadata.cached_content_token_count.unwrap_or(0),
        usage_metadata.candidates_token_count.unwrap_or(0),
        0,
    );
    let val = canonical.to_claude_usage(scaling_enabled, context_limit);
    super::models::Usage {
        input_tokens: val["input_tokens"].as_u64().unwrap_or(0) as u32,
        output_tokens: val["output_tokens"].as_u64().unwrap_or(0) as u32,
        cache_read_input_tokens: val["cache_read_input_tokens"].as_u64().map(|v| v as u32),
        cache_creation_input_tokens: Some(0),
        server_tool_use: None,
    }
}

/// 提取 thoughtSignature
// 已移除未使用的 extract_thought_signature 函数

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_claude_usage() {
        use super::super::models::UsageMetadata;

        let usage = UsageMetadata {
            prompt_token_count: Some(100),
            candidates_token_count: Some(50),
            total_token_count: Some(150),
            cached_content_token_count: None,
        };

        let claude_usage = to_claude_usage(&usage, true, 1_000_000);
        // 100 tokens is < 30k, minimal scaling
        assert!(claude_usage.input_tokens < 200);
        assert_eq!(claude_usage.output_tokens, 50);

        // 测试 50% 负载 (500k) - 应该显示 ~30%
        let usage_50 = UsageMetadata {
            prompt_token_count: Some(500_000),
            candidates_token_count: Some(10),
            total_token_count: Some(500_010),
            cached_content_token_count: None,
        };
        let res_50 = to_claude_usage(&usage_50, true, 1_000_000);
        // 50% * 0.6 = 30% of 195k = 58,500
        assert!(res_50.input_tokens > 55_000 && res_50.input_tokens < 62_000);

        // 测试 70% 负载 (700k) - 应该显示 ~50%
        let usage_70 = UsageMetadata {
            prompt_token_count: Some(700_000),
            candidates_token_count: Some(10),
            total_token_count: Some(700_010),
            cached_content_token_count: None,
        };
        let res_70 = to_claude_usage(&usage_70, true, 1_000_000);
        // 50% of 195k = 97,500
        assert!(res_70.input_tokens > 90_000 && res_70.input_tokens < 105_000);

        // 测试 85% 负载 (850k) - 应该显示 ~70%
        let usage_85 = UsageMetadata {
            prompt_token_count: Some(850_000),
            candidates_token_count: Some(10),
            total_token_count: Some(850_010),
            cached_content_token_count: None,
        };
        let res_85 = to_claude_usage(&usage_85, true, 1_000_000);
        // 70% of 195k = 136,500
        assert!(res_85.input_tokens > 130_000 && res_85.input_tokens < 145_000);

        // 测试 100% 负载 (1M) - 应该显示 ~97%
        let usage_100 = UsageMetadata {
            prompt_token_count: Some(1_000_000),
            candidates_token_count: Some(10),
            total_token_count: Some(1_000_010),
            cached_content_token_count: None,
        };
        let res_100 = to_claude_usage(&usage_100, true, 1_000_000);
        // 97% of 195k = 189,150
        assert!(res_100.input_tokens > 185_000 && res_100.input_tokens <= 190_000);
    }
}
