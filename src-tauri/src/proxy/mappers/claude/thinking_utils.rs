use super::models::{Message, MessageContent, ContentBlock};
use tracing::info;

#[derive(Debug, Default)]
pub struct ConversationState {
    pub in_tool_loop: bool,
    pub interrupted_tool: bool,
    pub last_assistant_idx: Option<usize>,
}

/// Analyze the conversation to detect tool loops or interrupted tool calls
pub fn analyze_conversation_state(messages: &[Message]) -> ConversationState {
    let mut state = ConversationState::default();

    if messages.is_empty() {
        return state;
    }

    // Find last assistant message index
    for (i, msg) in messages.iter().enumerate().rev() {
        if msg.role == "assistant" {
            state.last_assistant_idx = Some(i);
            break;
        }
    }

    // Check if the very last message is a Tool Result (User role with ToolResult block)
    if let Some(last_msg) = messages.last() {
        if last_msg.role == "user" {
           if let MessageContent::Array(blocks) = &last_msg.content {
               if blocks.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. })) {
                   state.in_tool_loop = true;
               }
           }
        }
    }

    state
}

/// Recover from broken tool loops by removing them from history
///
/// When client strips valid thinking blocks (leaving only ToolUse), and we are in a tool loop,
/// API will reject the request because "Assistant message must start with thinking".
/// We cannot fake the signature.
///
/// [FIXED] Instead of injecting synthetic messages (which cause context bloat),
/// we remove the broken ToolUse/ToolResult pair from history to prevent the loop.
///
/// [CRITICAL FIX #498] Only applies to models that support thinking (contain "-thinking" in name).
/// Models like gemini-2.5-flash do NOT use thinking blocks, so we should NOT remove
/// ToolResult messages for them - otherwise the AI will keep re-calling the same tool.
pub fn close_tool_loop_for_thinking(messages: &mut Vec<Message>, model: &str) {
    // Check if model supports thinking (contains "-thinking" in name)
    let model_supports_thinking = model.contains("-thinking");

    if !model_supports_thinking {
        // This model doesn't use thinking blocks - do NOT remove messages
        // Otherwise we'll break the tool loop for non-thinking models!
        return;
    }

    let state = analyze_conversation_state(messages);

    if !state.in_tool_loop {
        return;
    }

    // Check if last assistant message has a thinking block
    let mut has_thinking = false;
    let assistant_idx = state.last_assistant_idx;

    if let Some(idx) = assistant_idx {
        if let Some(msg) = messages.get(idx) {
             if let MessageContent::Array(blocks) = &msg.content {
                 has_thinking = blocks.iter().any(|b| matches!(b, ContentBlock::Thinking { .. }));
             }
        }
    }

    // If we are in a tool loop BUT the assistant message has no thinking block (it was stripped or missing),
    // we must break the loop by removing the broken pair.
    // This ONLY applies to thinking-enabled models!
    if !has_thinking {
        info!(
            "[Thinking-Recovery] Detected broken tool loop for thinking model '{}' (ToolResult without preceding Thinking). Removing from history.",
            model
        );

        // Remove ToolResult (last user message) and ToolUse (last assistant message)
        // This prevents the loop and avoids context bloat
        if messages.len() >= 2 {
            let removed_count = 2;
            messages.truncate(messages.len() - removed_count);
            info!("[Thinking-Recovery] Removed {} messages (broken tool loop pair)", removed_count);
        }
    }
}
