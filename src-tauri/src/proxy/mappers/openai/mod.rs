// OpenAI mapper 模块
// 负责 OpenAI ↔ Gemini 协议转换

pub mod collector; // [NEW]
pub mod context_blocks;
pub mod interaction_ledger;
pub mod models;
pub mod request;
pub mod response;
pub mod streaming;

pub use models::*;
pub use request::*;
pub use response::*;

/// Split a flattened Responses tool name back into its Codex namespace fields.
///
/// Codex represents MCP servers as namespaces such as `mcp__node_repl`. The
/// Gemini upstream receives the flattened wire name `mcp__node_repl__js`.
/// Because the namespace itself contains `__`, MCP names must be split at the
/// last delimiter. Other namespace tools retain their existing first-delimiter
/// behavior.
pub(crate) fn split_namespace_tool_name(qualified_name: &str) -> (String, Option<String>) {
    let name = qualified_name.trim();

    if let Some(rest) = name.strip_prefix("mcp__") {
        if let Some(pos) = rest.rfind("__") {
            if pos > 0 && pos + 2 < rest.len() {
                let namespace = format!("mcp__{}", &rest[..pos]);
                let actual_name = rest[pos + 2..].to_string();
                return (actual_name, Some(namespace));
            }
        }
        return (name.to_string(), None);
    }

    if let Some(pos) = name.find("__") {
        if pos > 0 && pos + 2 < name.len() {
            let namespace = name[..pos].to_string();
            let actual_name = name[pos + 2..].to_string();
            return (actual_name, Some(namespace));
        }
    }
    (name.to_string(), None)
}

#[cfg(test)]
mod namespace_tool_name_tests {
    use super::split_namespace_tool_name;

    #[test]
    fn splits_mcp_namespace_at_last_delimiter() {
        assert_eq!(
            split_namespace_tool_name("mcp__node_repl__js"),
            ("js".to_string(), Some("mcp__node_repl".to_string()))
        );
        assert_eq!(
            split_namespace_tool_name("mcp__codex_apps__github__search"),
            (
                "search".to_string(),
                Some("mcp__codex_apps__github".to_string())
            )
        );
    }

    #[test]
    fn preserves_existing_namespace_and_plain_tool_behavior() {
        assert_eq!(
            split_namespace_tool_name("multi_agent_v1__spawn_agent"),
            (
                "spawn_agent".to_string(),
                Some("multi_agent_v1".to_string())
            )
        );
        assert_eq!(
            split_namespace_tool_name("mcp__standalone_tool"),
            ("mcp__standalone_tool".to_string(), None)
        );
        assert_eq!(
            split_namespace_tool_name("exec_command"),
            ("exec_command".to_string(), None)
        );
    }
}
