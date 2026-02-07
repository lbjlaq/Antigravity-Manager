// Thinking Mode Detection and Validation

use crate::proxy::mappers::claude::models::{ContentBlock, Message, MessageContent};

/// Minimum length for a valid thought_signature
pub const MIN_SIGNATURE_LENGTH: usize = 50;

/// Check if thinking mode should be disabled due to incompatible history
///
/// Scenario: If the last Assistant message is in a Tool Use flow but has no Thinking block,
/// it means this is a flow initiated by a non-Thinking model. Forcing Thinking on would cause:
/// "final assistant message must start with a thinking block" error.
/// We cannot fake valid Thinking (due to signature issues), so we must disable Thinking for this turn.
pub fn should_disable_thinking_due_to_history(messages: &[Message]) -> bool {
    // Reverse search for the last Assistant message
    for msg in messages.iter().rev() {
        if msg.role == "assistant" {
            if let MessageContent::Array(blocks) = &msg.content {
                let has_tool_use = blocks
                    .iter()
                    .any(|b| matches!(b, ContentBlock::ToolUse { .. }));
                let has_thinking = blocks
                    .iter()
                    .any(|b| matches!(b, ContentBlock::Thinking { .. }));

                // If has tool call but no Thinking block -> incompatible
                if has_tool_use && !has_thinking {
                    tracing::info!("[Thinking-Mode] Detected ToolUse without Thinking in history. Requesting disable.");
                    return true;
                }
            }
            // Only check the most recent Assistant message
            return false;
        }
    }
    false
}

/// Check if thinking mode should be enabled by default for a given model
///
/// Claude Code v2.0.67+ enables thinking by default for Opus 4.5 models.
/// [NEW] PR #1641: Also enables thinking for Opus 4.6 models.
pub fn should_enable_thinking_by_default(model: &str) -> bool {
    let model_lower = model.to_lowercase();

    // [NEW] Enable thinking by default for Opus 4.6 variants (PR #1641)
    if model_lower.contains("opus-4-6") || model_lower.contains("opus-4.6") {
        tracing::debug!(
            "[Thinking-Mode] Auto-enabling thinking for Opus 4.6 model: {}",
            model
        );
        return true;
    }

    // Enable thinking by default for Opus 4.5 variants
    if model_lower.contains("opus-4-5") || model_lower.contains("opus-4.5") {
        tracing::debug!(
            "[Thinking-Mode] Auto-enabling thinking for Opus 4.5 model: {}",
            model
        );
        return true;
    }

    // [FIX #1557] Enable thinking by default for Gemini Pro families.
    if model_lower.contains("gemini-2.0-pro") || model_lower.contains("gemini-3-pro") {
        tracing::debug!(
            "[Thinking-Mode] Auto-enabling thinking for Gemini Pro model: {}",
            model
        );
        return true;
    }

    // Also enable for explicit thinking model variants
    if model_lower.contains("-thinking") {
        return true;
    }

    false
}

/// Check if we have any valid signature available for function calls
/// This prevents Gemini 3 Pro from rejecting requests due to missing thought_signature
pub fn has_valid_signature_for_function_calls(
    messages: &[Message],
    global_sig: &Option<String>,
    session_id: &str,
) -> bool {
    // 1. Check global store (deprecated but kept for compatibility)
    if let Some(sig) = global_sig {
        if sig.len() >= MIN_SIGNATURE_LENGTH {
            tracing::debug!(
                "[Signature-Check] Found valid signature in global store (len: {})",
                sig.len()
            );
            return true;
        }
    }

    // 2. Check Session Cache - critical for retry scenarios
    if let Some(sig) = crate::proxy::SignatureCache::global().get_session_signature(session_id) {
        if sig.len() >= MIN_SIGNATURE_LENGTH {
            tracing::info!(
                "[Signature-Check] Found valid signature in SESSION cache (session: {}, len: {})",
                session_id,
                sig.len()
            );
            return true;
        }
    }

    // 3. Check if any message has a thinking block with valid signature
    for msg in messages.iter().rev() {
        if msg.role == "assistant" {
            if let MessageContent::Array(blocks) = &msg.content {
                for block in blocks {
                    if let ContentBlock::Thinking {
                        signature: Some(sig),
                        ..
                    } = block
                    {
                        if sig.len() >= MIN_SIGNATURE_LENGTH {
                            tracing::debug!(
                                "[Signature-Check] Found valid signature in message history (len: {})",
                                sig.len()
                            );
                            return true;
                        }
                    }
                }
            }
        }
    }

    tracing::warn!(
        "[Signature-Check] No valid signature found (session: {}, checked: global store, session cache, message history)",
        session_id
    );
    false
}

/// Check if two model strings are compatible (same family)
pub fn is_model_compatible(cached: &str, target: &str) -> bool {
    let c = cached.to_lowercase();
    let t = target.to_lowercase();

    if c == t {
        return true;
    }

    // Claude models are more permissive
    if c.contains("claude-3-5") && t.contains("claude-3-5") {
        return true;
    }
    if c.contains("claude-3-7") && t.contains("claude-3-7") {
        return true;
    }

    // Gemini models: strict family match required for signatures
    if c.contains("gemini-1.5-pro") && t.contains("gemini-1.5-pro") {
        return true;
    }
    if c.contains("gemini-1.5-flash") && t.contains("gemini-1.5-flash") {
        return true;
    }
    if c.contains("gemini-2.0-flash") && t.contains("gemini-2.0-flash") {
        return true;
    }
    if c.contains("gemini-2.0-pro") && t.contains("gemini-2.0-pro") {
        return true;
    }

    // Fallback: strict match required
    false
}
