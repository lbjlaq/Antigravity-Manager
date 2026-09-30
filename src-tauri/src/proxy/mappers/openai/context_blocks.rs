//! Pass client system/developer instructions through as Gemini `parts`.
//!
//! Client array/multi-message structure is preserved. This module must not join
//! blocks into one blob, rewrite prompt text, or inject gateway metadata such as
//! `<user_information>`.

use serde_json::{json, Value};

pub fn build_system_instruction_parts(
    system_instructions: &[String],
    global_prompt: Option<&str>,
) -> Vec<Value> {
    let mut parts = Vec::new();

    for instruction in system_instructions {
        if instruction.trim().is_empty() {
            continue;
        }
        let enhanced =
            crate::proxy::mappers::common_utils::enhance_gemini_skills_prompt(instruction);
        if enhanced.trim().is_empty() {
            continue;
        }
        parts.push(json!({ "text": enhanced }));
    }

    if let Some(prompt) = global_prompt.map(str::trim).filter(|s| !s.is_empty()) {
        let already_has_global = parts.iter().any(|p| {
            p.get("text")
                .and_then(|t| t.as_str())
                .map(|s| s.contains(prompt))
                .unwrap_or(false)
        });
        if !already_has_global {
            parts.push(json!({ "text": format!("{}\n\n", prompt) }));
        }
    }

    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_client_blocks_as_separate_parts() {
        let parts = build_system_instruction_parts(
            &[
                "<environment>env</environment>".into(),
                "<workflow_and_execution_discipline>wf</workflow_and_execution_discipline>".into(),
            ],
            None,
        );

        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["text"], "<environment>env</environment>");
        assert_eq!(
            parts[1]["text"],
            "<workflow_and_execution_discipline>wf</workflow_and_execution_discipline>"
        );
        let joined = serde_json::to_string(&parts).unwrap();
        assert!(!joined.contains("user_information"));
        assert!(!joined.contains("Request type:"));
        assert!(!joined.contains("Mapped model:"));
    }

    #[test]
    fn appends_global_prompt_as_its_own_part() {
        let parts = build_system_instruction_parts(&["client block".into()], Some("global extra"));
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["text"], "client block");
        assert_eq!(parts[1]["text"], "global extra\n\n");
    }

    #[test]
    fn skips_empty_blocks() {
        let parts = build_system_instruction_parts(&["  ".into(), "keep".into(), "".into()], None);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], "keep");
    }
}
