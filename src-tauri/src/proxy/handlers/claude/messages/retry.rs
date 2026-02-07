//! Error handling and retry logic for thinking signature failures.

use crate::proxy::mappers::claude::models::{ClaudeRequest, ContentBlock, MessageContent};
use tokio::time::Duration;
use tracing::debug;

/// Check if error indicates a thinking signature failure.
pub fn is_thinking_signature_error(error_text: &str) -> bool {
    error_text.contains("Invalid `signature`")
        || error_text.contains("thinking.signature: Field required")
        || error_text.contains("thinking.thinking: Field required")
        || error_text.contains("thinking.signature")
        || error_text.contains("thinking.thinking")
        || error_text.contains("Corrupted thought signature")
        || error_text.contains("failed to deserialise")
        || error_text.contains("Invalid signature")
        || error_text.contains("thinking block")
        || error_text.contains("Found `text`")
        || error_text.contains("Found 'text'")
        || error_text.contains("must be `thinking`")
        || error_text.contains("must be 'thinking'")
}

/// Check if error indicates context too long.
pub fn is_context_too_long_error(error_text: &str) -> bool {
    error_text.contains("too long") || error_text.contains("exceeds") || error_text.contains("limit")
}

/// Handle thinking signature error by removing thinking blocks.
pub fn handle_thinking_signature_error(request: &mut ClaudeRequest, trace_id: &str) {
    // Append repair prompt to last user message
    if let Some(last_msg) = request.messages.last_mut() {
        if last_msg.role == "user" {
            let repair_prompt = "\n\n[System Recovery] Your previous output contained an invalid signature. Please regenerate the response without the corrupted signature block.";

            match &mut last_msg.content {
                MessageContent::String(s) => {
                    s.push_str(repair_prompt);
                }
                MessageContent::Array(blocks) => {
                    blocks.push(ContentBlock::Text {
                        text: repair_prompt.to_string(),
                    });
                }
            }
            debug!("[{}] Appended repair prompt to last user message", trace_id);
        }
    }

    // Convert thinking blocks to text
    for msg in request.messages.iter_mut() {
        if let MessageContent::Array(blocks) = &mut msg.content {
            let mut new_blocks = Vec::with_capacity(blocks.len());
            for block in blocks.drain(..) {
                match block {
                    ContentBlock::Thinking { thinking, .. } => {
                        if !thinking.is_empty() {
                            debug!(
                                "[Fallback] Converting thinking block to text (len={})",
                                thinking.len()
                            );
                            new_blocks.push(ContentBlock::Text { text: thinking });
                        }
                    }
                    ContentBlock::RedactedThinking { .. } => {
                        // Discard redacted thinking
                    }
                    _ => new_blocks.push(block),
                }
            }
            *blocks = new_blocks;
        }
    }

    // Close tool loop
    crate::proxy::mappers::claude::thinking_utils::close_tool_loop_for_thinking(
        &mut request.messages,
    );

    // Normalize model name
    if request.model.contains("claude-") {
        let mut m = request.model.clone();
        m = m.replace("-thinking", "");
        if m.contains("claude-sonnet-4-5-") {
            m = "claude-sonnet-4-5".to_string();
        } else if m.contains("claude-opus-4-6-") || m.contains("claude-opus-4.6") {
            m = "claude-opus-4-6".to_string();
        } else if m.contains("claude-opus-4-5-") || m.contains("claude-opus-4-") {
            m = "claude-opus-4-5".to_string();
        }
        request.model = m;
    }
}

/// Get retry delay for thinking signature errors.
pub fn get_thinking_retry_delay() -> Duration {
    Duration::from_millis(200)
}
