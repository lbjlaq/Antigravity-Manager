use serde_json::Value;

/// MCP 工具适配器 trait
///
/// 为不同的 MCP 工具提供定制化的 Schema 处理策略。
/// 每个工具可以实现自己的适配器来处理特定的需求。
pub trait ToolAdapter: Send + Sync {
    /// 判断该适配器是否匹配给定的工具名称
    ///
    /// # Arguments
    /// * `tool_name` - 工具名称,通常格式为 "mcp__provider__function"
    ///
    /// # Returns
    /// 如果匹配返回 true,否则返回 false
    fn matches(&self, tool_name: &str) -> bool;

    /// 在通用清洗前执行的预处理
    ///
    /// 可以在这里添加工具特定的字段处理、提示添加等
    ///
    /// # Arguments
    /// * `schema` - 待处理的 JSON Schema
    ///
    /// # Returns
    /// 成功返回 Ok(()), 失败返回错误信息
    fn pre_process(&self, _schema: &mut Value) -> Result<(), String> {
        Ok(())
    }

    /// 在通用清洗后执行的后处理
    ///
    /// 可以在这里进行最终的调整和优化
    ///
    /// # Arguments
    /// * `schema` - 已清洗的 JSON Schema
    ///
    /// # Returns
    /// 成功返回 Ok(()), 失败返回错误信息
    fn post_process(&self, _schema: &mut Value) -> Result<(), String> {
        Ok(())
    }
}

/// 辅助函数: 向 Schema 的 description 字段追加提示
pub fn append_hint_to_schema(schema: &mut Value, hint: &str) {
    if let Value::Object(map) = schema {
        let desc_val = map
            .entry("description".to_string())
            .or_insert_with(|| Value::String("".to_string()));

        if let Value::String(s) = desc_val {
            if s.is_empty() {
                *s = hint.to_string();
            } else if !s.contains(hint) {
                *s = format!("{} {}", s, hint);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct TestAdapter;

    impl ToolAdapter for TestAdapter {
        fn matches(&self, tool_name: &str) -> bool {
            tool_name.starts_with("test__")
        }

        fn pre_process(&self, schema: &mut Value) -> Result<(), String> {
            append_hint_to_schema(schema, "[Test Adapter]");
            Ok(())
        }
    }

    #[test]
    fn test_adapter_matches() {
        let adapter = TestAdapter;
        assert!(adapter.matches("test__function"));
        assert!(!adapter.matches("other__function"));
    }

    #[test]
    fn test_append_hint() {
        let mut schema = json!({"type": "string"});
        append_hint_to_schema(&mut schema, "Test hint");
        assert_eq!(schema["description"], "Test hint");

        append_hint_to_schema(&mut schema, "Another hint");
        assert_eq!(schema["description"], "Test hint Another hint");
    }
}
