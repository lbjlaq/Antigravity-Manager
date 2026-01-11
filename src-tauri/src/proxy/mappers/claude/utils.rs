// Claude 辅助函数
// JSON Schema 清理、签名处理等

// 已移除未使用的 Value 导入

// 将 JSON Schema 中的类型名称转为大写 (Gemini 要求)
// 例如: "string" -> "STRING", "integer" -> "INTEGER"
// 已移除未使用的 uppercase_schema_types 函数

/// 从 Gemini UsageMetadata 转换为 Claude Usage
pub fn to_claude_usage(usage_metadata: &super::models::UsageMetadata) -> super::models::Usage {
    let prompt_tokens = usage_metadata.prompt_token_count.unwrap_or(0);
    let cached_tokens = usage_metadata.cached_content_token_count.unwrap_or(0);

    super::models::Usage {
        // input_tokens 应该排除缓存的部分
        input_tokens: prompt_tokens.saturating_sub(cached_tokens),
        output_tokens: usage_metadata.candidates_token_count.unwrap_or(0),
        // 缓存统计
        cache_read_input_tokens: if cached_tokens > 0 {
            Some(cached_tokens)
        } else {
            None
        },
        cache_creation_input_tokens: Some(0), // Gemini 不提供此字段,设为 0
        server_tool_use: None,
    }
}

/// 提取 thoughtSignature
// 已移除未使用的 extract_thought_signature 函数

#[cfg(test)]
mod tests {
    use super::*;
    // 移除了未使用的 serde_json::json

    // 已移除对 uppercase_schema_types 的过期测试

    #[test]
    fn test_to_claude_usage() {
        use super::super::models::UsageMetadata;

        let usage = UsageMetadata {
            prompt_token_count: Some(100),
            candidates_token_count: Some(50),
            total_token_count: Some(150),
            cached_content_token_count: None,
        };

        let claude_usage = to_claude_usage(&usage);
        assert_eq!(claude_usage.input_tokens, 100);
        assert_eq!(claude_usage.output_tokens, 50);
    }
}
