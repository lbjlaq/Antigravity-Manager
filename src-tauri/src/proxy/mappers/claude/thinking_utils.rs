use super::models::{ContentBlock, Message, MessageContent};
use crate::proxy::SignatureCache;
use tracing::{debug, info, warn};

pub const MIN_SIGNATURE_LENGTH: usize = 50;
pub const SENTINEL_SIGNATURE: &str = crate::proxy::thinking_store::SENTINEL_SIGNATURE;

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

    // A tool loop starts if the assistant message has tool use blocks
    let has_tool_use = if let Some(idx) = state.last_assistant_idx {
        if let Some(msg) = messages.get(idx) {
            if let MessageContent::Array(blocks) = &msg.content {
                blocks
                    .iter()
                    .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    if !has_tool_use {
        return state;
    }

    // Check what follows the assistant's tool use
    if let Some(last_msg) = messages.last() {
        if last_msg.role == "user" {
            if let MessageContent::Array(blocks) = &last_msg.content {
                // Case 1: Final message is ToolResult -> Active Tool Loop
                if blocks
                    .iter()
                    .any(|b| matches!(b, ContentBlock::ToolResult { .. }))
                {
                    state.in_tool_loop = true;
                    debug!(
                        "[Thinking-Recovery] Active tool loop detected (last msg is ToolResult)."
                    );
                } else {
                    // Case 2: Final message is Text (User) -> Interrupted Tool
                    state.interrupted_tool = true;
                    debug!(
                        "[Thinking-Recovery] Interrupted tool detected (last msg is Text user)."
                    );
                }
            } else if let MessageContent::String(_) = &last_msg.content {
                // Case 2: Final message is String (User) -> Interrupted Tool
                state.interrupted_tool = true;
                debug!("[Thinking-Recovery] Interrupted tool detected (last msg is String user).");
            }
        }
    }

    // Check for interrupted tool: Last assistant message has ToolUse, but no corresponding ToolResult in next user msg
    // (This is harder to detect perfectly on a stateless request, but usually if we are
    //  in a state where we have ToolUse but the conversation seems "broken" or stripped)
    // Actually, in the proxy context, we typically see:
    // ... Assistant (ToolUse) -> User (ToolResult) : Normal Loop
    // ... Assistant (ToolUse) -> User (Text) : Interrupted (User cancelled)

    // For "Thinking Utils", we care about the case where valid signatures are missing.
    // If we are in a tool loop (last msg is ToolResult), and the *preceding* Assistant message
    // had its Thinking block stripped (due to invalid sig), then we are in a "Broken Tool Loop".
    // Gemini/Claude will reject a ToolResult if the preceding Assistant message didn't start with Thinking.

    state
}

/// Recover from broken tool loops or interrupted tool calls by injecting synthetic messages
pub fn close_tool_loop_for_thinking(messages: &mut Vec<Message>) {
    let state = analyze_conversation_state(messages);

    if !state.in_tool_loop && !state.interrupted_tool {
        return;
    }

    // Check if the last assistant message has a valid thinking block
    let mut has_valid_thinking = false;
    if let Some(idx) = state.last_assistant_idx {
        if let Some(msg) = messages.get(idx) {
            if let MessageContent::Array(blocks) = &msg.content {
                for block in blocks {
                    if let ContentBlock::Thinking {
                        thinking,
                        signature,
                        ..
                    } = block
                    {
                        if !thinking.is_empty()
                            && (signature.is_none()
                                || signature
                                    .as_ref()
                                    .map(|s| s.len() >= MIN_SIGNATURE_LENGTH || s == SENTINEL_SIGNATURE)
                                    .unwrap_or(true))
                        {
                            has_valid_thinking = true;
                            break;
                        }
                    }
                }
            }
        }
    }

    if !has_valid_thinking {
        if state.in_tool_loop {
            info!("[Thinking-Recovery] Broken tool loop (ToolResult without preceding Thinking). Recovery triggered.");

            // Insert acknowledging message to "close" the history turn
            messages.push(Message {
                role: "assistant".to_string(),
                content: MessageContent::Array(vec![ContentBlock::Text {
                    text: "[System: Tool execution completed. Proceeding to final response.]"
                        .to_string(),
                }]),
            });
            messages.push(Message {
                role: "user".to_string(),
                content: MessageContent::Array(vec![ContentBlock::Text {
                    text: "Please provide the final result based on the tool output above."
                        .to_string(),
                }]),
            });
        } else if state.interrupted_tool {
            info!(
                "[Thinking-Recovery] Interrupted tool call detected. Injecting synthetic closure."
            );

            // For interrupted tool, we need to insert the closure AFTER the assistant's tool use
            // but BEFORE the user's latest message.
            if let Some(idx) = state.last_assistant_idx {
                messages.insert(
                    idx + 1,
                    Message {
                        role: "assistant".to_string(),
                        content: MessageContent::Array(vec![ContentBlock::Text {
                            text: "[Tool call was interrupted by user.]".to_string(),
                        }]),
                    },
                );
            }
        }
    }
}

/// Get the model family origin of a signature
pub fn get_signature_family(signature: &str) -> Option<String> {
    SignatureCache::global().get_signature_family(signature)
}

/// [CRITICAL] Sanitize thinking blocks and check cross-model compatibility
pub fn filter_invalid_thinking_blocks_with_family(
    messages: &mut [Message],
    target_family: Option<&str>,
) {
    let mut stripped_count = 0;

    for msg in messages.iter_mut() {
        if msg.role != "assistant" {
            continue;
        }

        if let MessageContent::Array(blocks) = &mut msg.content {
            for block in blocks.iter_mut() {
                if let ContentBlock::Thinking { signature, .. } = block {
                    if let Some(s) = signature.as_deref() {
                        // Empty signature: keep thinking block, normalize signature to None for sentinel resolution
                        if s.is_empty() {
                            *signature = None;
                            continue;
                        }

                        // Signature too short: strip signature only if not sentinel, keep thinking content
                        if s.len() < MIN_SIGNATURE_LENGTH && s != SENTINEL_SIGNATURE {
                            info!("[Thinking-Sanitizer] Stripping too-short signature (len: {}), preserving thinking block", s.len());
                            *signature = None;
                            stripped_count += 1;
                            continue;
                        }

                        // Family compatibility check:
                        // Only drop if origin family is known AND strictly cross-family incompatible.
                        // If origin is not in cache (e.g. server restart or fresh session), TRUST valid signatures!
                        // Upstream Google Gemini validates its own signatures. Stripping valid signatures leads to 400 Bad Request.
                        if let Some(target) = target_family {
                            if let Some(origin_family) = get_signature_family(s) {
                                let origin_lc = origin_family.to_lowercase();
                                let target_lc = target.to_lowercase();
                                let is_incompatible = (target_lc == "gemini"
                                    && (origin_lc.starts_with("claude-3-opus")
                                        || origin_lc.contains("anthropic-native")))
                                    || (target_lc == "claude" && origin_lc.contains("gemini"));

                                if is_incompatible {
                                    warn!(
                                        "[Thinking-Sanitizer] Dropping cross-family signature from family '{}' for target '{}'",
                                        origin_family, target
                                    );
                                    *signature = None;
                                    stripped_count += 1;
                                }
                            }
                        }
                    }
                }
            }

            // Clean up completely empty thinking blocks without text or signature
            let original_len = blocks.len();
            blocks.retain(|b| {
                if let ContentBlock::Thinking { thinking, signature, .. } = b {
                    if thinking.trim().is_empty() && signature.is_none() {
                        return false;
                    }
                }
                true
            });

            // SAFETY: Claude API requires at least one block
            if blocks.is_empty() && original_len > 0 {
                blocks.push(ContentBlock::Text {
                    text: ".".to_string(),
                });
            }
        }
    }

    if stripped_count > 0 {
        info!(
            "[Thinking-Sanitizer] Sanitized {} invalid or incompatible thinking signatures (preserved thinking blocks)",
            stripped_count
        );
    }
}
