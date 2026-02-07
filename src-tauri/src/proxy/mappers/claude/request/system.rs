// System Instruction Builder

use serde_json::{json, Value};
use crate::proxy::mappers::claude::models::SystemPrompt;

/// Build System Instruction with dynamic identity mapping and prompt isolation
pub fn build_system_instruction(
    system: &Option<SystemPrompt>,
    _model_name: &str,
    has_mcp_tools: bool,
) -> Option<Value> {
    let mut parts = Vec::new();

    // Antigravity identity instruction
    let antigravity_identity = "You are Antigravity, a powerful agentic AI coding assistant designed by the Google Deepmind team working on Advanced Agentic Coding.\n\
    You are pair programming with a USER to solve their coding task. The task may require creating a new codebase, modifying or debugging an existing codebase, or simply answering a question.\n\
    **Absolute paths only**\n\
    **Proactiveness**";

    // Check if user already provided Antigravity identity
    let mut user_has_antigravity = false;
    if let Some(sys) = system {
        match sys {
            SystemPrompt::String(text) => {
                if text.contains("You are Antigravity") {
                    user_has_antigravity = true;
                }
            }
            SystemPrompt::Array(blocks) => {
                for block in blocks {
                    if block.block_type == "text" && block.text.contains("You are Antigravity") {
                        user_has_antigravity = true;
                        break;
                    }
                }
            }
        }
    }

    // Inject Antigravity identity if not provided by user
    if !user_has_antigravity {
        parts.push(json!({"text": antigravity_identity}));
    }

    // Add user's system prompt
    if let Some(sys) = system {
        match sys {
            SystemPrompt::String(text) => {
                parts.push(json!({"text": text}));
            }
            SystemPrompt::Array(blocks) => {
                for block in blocks {
                    if block.block_type == "text" {
                        parts.push(json!({"text": block.text}));
                    }
                }
            }
        }
    }

    // MCP XML Bridge: If there are mcp__ prefixed tools, inject special calling protocol
    if has_mcp_tools {
        let mcp_xml_prompt = "\n\
        ==== MCP XML Tool Calling Protocol (Workaround) ====\n\
        When you need to call MCP tools (names starting with `mcp__`):\n\
        1) Prefer XML format: Output `<mcp__tool_name>{\"arg\":\"value\"}</mcp__tool_name>`.\n\
        2) Must output XML block directly, no markdown wrapper, content is JSON formatted params.\n\
        3) This method has better connectivity and fault tolerance for large result returns.\n\
        ===========================================";
        parts.push(json!({"text": mcp_xml_prompt}));
    }

    // Add end marker if no user-provided Antigravity identity
    if !user_has_antigravity {
        parts.push(json!({"text": "\n--- [SYSTEM_PROMPT_END] ---"}));
    }

    Some(json!({
        "role": "user",
        "parts": parts
    }))
}
