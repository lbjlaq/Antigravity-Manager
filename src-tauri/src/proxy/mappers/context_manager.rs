//! Context Manager Module
//!
//! Responsible for estimating token usage and purifying context (stripping thinking blocks)
//! to prevent "Prompt is too long" errors and avoid invalid signatures.

use super::claude::models::{ClaudeRequest, ContentBlock, Message, MessageContent, SystemPrompt};
use tracing::{debug, info};

/// Helper to estimate tokens from text with multi-language awareness
///
/// Improved estimation algorithm:
/// - ASCII/English: ~4 characters per token
/// - Unicode/CJK: ~1.5 characters per token (Chinese, Japanese, Korean are tokenized differently)
/// - Adds 15% safety margin to prevent underestimation
fn estimate_tokens_from_str(s: &str) -> u32 {
    if s.is_empty() {
        return 0;
    }

    let mut ascii_chars = 0u32;
    let mut unicode_chars = 0u32;

    for c in s.chars() {
        if c.is_ascii() {
            ascii_chars += 1;
        } else {
            unicode_chars += 1;
        }
    }

    // ASCII: ~4 chars/token, Unicode/CJK: ~1.5 chars/token
    let ascii_tokens = (ascii_chars as f32 / 4.0).ceil() as u32;
    let unicode_tokens = (unicode_chars as f32 / 1.5).ceil() as u32;

    // Add 15% safety margin to account for tokenizer variations
    ((ascii_tokens + unicode_tokens) as f32 * 1.15).ceil() as u32
}

/// Strategy for context purification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurificationStrategy {
    /// Soft purification: Retains recent thinking blocks (~2 turns), removes older ones
    #[allow(dead_code)]
    Soft,
    /// Aggressive purification: Removes ALL thinking blocks to save maximum tokens
    Aggressive,
}

/// Context Manager implementation
pub struct ContextManager;

impl ContextManager {
    /// Purify message history based on the selected strategy
    ///
    /// This removes Thinking blocks completely (unlike compression which keeps placeholders/signatures)
    /// Used when context is critical or signatures are invalid.
    pub fn purify_history(messages: &mut Vec<Message>, strategy: PurificationStrategy) -> bool {
        let protected_last_n = match strategy {
            PurificationStrategy::Soft => 4, // Protect last ~2 turns (User-AI-User-AI)
            PurificationStrategy::Aggressive => 0, // No protection
        };

        Self::strip_thinking_blocks(messages, protected_last_n)
    }

    /// Internal helper to strip thinking blocks from messages outside the protected range
    fn strip_thinking_blocks(messages: &mut Vec<Message>, protected_last_n: usize) -> bool {
        let total_msgs = messages.len();
        if total_msgs == 0 {
            return false;
        }

        let start_protection_idx = total_msgs.saturating_sub(protected_last_n);
        let mut modified = false;

        for (i, msg) in messages.iter_mut().enumerate() {
            // Skip protected messages
            if i >= start_protection_idx {
                continue;
            }

            if msg.role == "assistant" {
                if let MessageContent::Array(blocks) = &mut msg.content {
                    let original_len = blocks.len();
                    // Retain only non-Thinking blocks
                    blocks.retain(|b| !matches!(b, ContentBlock::Thinking { .. }));

                    if blocks.len() != original_len {
                        modified = true;
                        debug!(
                            "[ContextManager] Stripped {} thinking blocks from message {}",
                            original_len - blocks.len(),
                            i
                        );
                    }
                }
            }
        }

        modified
    }
}

impl ContextManager {
    /// Estimate token usage for a Claude Request
    ///
    /// This is a lightweight estimation, not a precise count.
    /// It iterates through all messages and blocks to sum up estimated tokens.
    pub fn estimate_token_usage(request: &ClaudeRequest) -> u32 {
        let mut total = 0;

        // System prompt
        if let Some(sys) = &request.system {
            match sys {
                SystemPrompt::String(s) => total += estimate_tokens_from_str(s),
                SystemPrompt::Array(blocks) => {
                    for block in blocks {
                        total += estimate_tokens_from_str(&block.text);
                    }
                }
            }
        }

        // Messages
        for msg in &request.messages {
            // Message overhead
            total += 4;

            match &msg.content {
                MessageContent::String(s) => {
                    total += estimate_tokens_from_str(s);
                }
                MessageContent::Array(blocks) => {
                    for block in blocks {
                        match block {
                            ContentBlock::Text { text } => {
                                total += estimate_tokens_from_str(text);
                            }
                            ContentBlock::Thinking { thinking, .. } => {
                                total += estimate_tokens_from_str(thinking);
                                // Signature overhead
                                total += 100;
                            }
                            ContentBlock::RedactedThinking { data } => {
                                total += estimate_tokens_from_str(data);
                            }
                            ContentBlock::ToolUse { name, input, .. } => {
                                total += 20; // Function call overhead
                                total += estimate_tokens_from_str(name);
                                if let Ok(json_str) = serde_json::to_string(input) {
                                    total += estimate_tokens_from_str(&json_str);
                                }
                            }
                            ContentBlock::ToolResult { content, .. } => {
                                total += 10; // Result overhead
                                             // content is serde_json::Value
                                if let Some(s) = content.as_str() {
                                    total += estimate_tokens_from_str(s);
                                } else if let Some(arr) = content.as_array() {
                                    for item in arr {
                                        if let Some(text) =
                                            item.get("text").and_then(|t| t.as_str())
                                        {
                                            total += estimate_tokens_from_str(text);
                                        }
                                    }
                                } else {
                                    // Fallback for objects or other types
                                    if let Ok(s) = serde_json::to_string(content) {
                                        total += estimate_tokens_from_str(&s);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Tools definition overhead (rough estimate)
        if let Some(tools) = &request.tools {
            for tool in tools {
                if let Ok(json_str) = serde_json::to_string(tool) {
                    total += estimate_tokens_from_str(&json_str);
                }
            }
        }

        // Thinking budget overhead if enabled
        if let Some(thinking) = &request.thinking {
            if let Some(budget) = thinking.budget_tokens {
                // Reserve budget in estimation
                total += budget;
            }
        }

        total
    }

    // ===== [Layer 2] Thinking Content Compression + Signature Preservation =====
    // Borrowed from learn-claude-code's "append-only log" principle
    // This layer compresses thinking text but PRESERVES signatures
    // Advantage: Signature chain remains intact, tool calls won't break
    // Disadvantage: Still breaks Prompt Cache (modifies content)

    /// Compress thinking content while preserving signatures
    ///
    /// This function:
    /// 1. Keeps signatures intact (critical for tool call chain)
    /// 2. Compresses thinking text to "..." placeholder
    /// 3. Protects the last N messages from compression
    ///
    /// Returns true if any thinking blocks were compressed
    pub fn compress_thinking_preserve_signature(
        messages: &mut Vec<Message>,
        protected_last_n: usize,
    ) -> bool {
        let total_msgs = messages.len();
        if total_msgs == 0 {
            return false;
        }

        let start_protection_idx = total_msgs.saturating_sub(protected_last_n);
        let mut compressed_count = 0;
        let mut total_chars_saved = 0;

        for (i, msg) in messages.iter_mut().enumerate() {
            // Skip protected messages
            if i >= start_protection_idx {
                continue;
            }

            // Only process assistant messages
            if msg.role == "assistant" {
                if let MessageContent::Array(blocks) = &mut msg.content {
                    for block in blocks.iter_mut() {
                        if let ContentBlock::Thinking {
                            thinking,
                            signature,
                            ..
                        } = block
                        {
                            // Key logic: Only compress if signature exists
                            // This ensures we don't lose unsigned thinking blocks
                            if signature.is_some() && thinking.len() > 10 {
                                let original_len = thinking.len();
                                *thinking = "...".to_string();
                                compressed_count += 1;
                                total_chars_saved += original_len - 3;

                                debug!(
                                    "[ContextManager] [Layer-2] Compressed thinking: {} → 3 chars (signature preserved)",
                                    original_len
                                );
                            }
                        }
                    }
                }
            }
        }

        if compressed_count > 0 {
            let estimated_tokens_saved = (total_chars_saved as f32 / 3.5).ceil() as u32;
            info!(
                "[ContextManager] [Layer-2] Compressed {} thinking blocks (saved ~{} tokens, signatures preserved)",
                compressed_count, estimated_tokens_saved
            );
        }

        compressed_count > 0
    }

    // ===== [Layer 3 Helper] Extract Last Valid Signature =====
    // Used by Layer 3 to preserve signature when generating XML summary

    /// Extract the last valid thinking signature from message history
    ///
    /// This is critical for Layer 3 (Fork + Summary) to preserve the signature chain.
    /// The signature will be embedded in the XML summary and restored after fork.
    ///
    /// Returns None if no valid signature found (length >= 50)
    pub fn extract_last_valid_signature(messages: &[Message]) -> Option<String> {
        // Iterate in reverse to find the most recent signature
        for msg in messages.iter().rev() {
            if msg.role == "assistant" {
                if let MessageContent::Array(blocks) = &msg.content {
                    for block in blocks {
                        if let ContentBlock::Thinking {
                            signature: Some(sig),
                            ..
                        } = block
                        {
                            // Minimum signature length check (same as SignatureCache)
                            if sig.len() >= 50 {
                                debug!(
                                    "[ContextManager] [Layer-3] Extracted last valid signature (len: {})",
                                    sig.len()
                                );
                                return Some(sig.clone());
                            }
                        }
                    }
                }
            }
        }

        debug!("[ContextManager] [Layer-3] No valid signature found in history");
        None
    }

    // ===== [Layer 1] Tool Message Intelligent Trimming =====
    // Borrowed from Practical-Guide-to-Context-Engineering
    // This layer removes old tool call/result pairs while preserving recent ones
    // Advantage: Does NOT break Prompt Cache (only removes messages, doesn't modify content)

    /// Trim old tool messages, keeping only the last N rounds
    ///
    /// A "tool round" consists of:
    /// - An assistant message with tool_use
    /// - One or more user messages with tool_result
    ///
    /// Returns true if any messages were removed
    pub fn trim_tool_messages(messages: &mut Vec<Message>, keep_last_n_rounds: usize) -> bool {
        let tool_rounds = identify_tool_rounds(messages);

        if tool_rounds.len() <= keep_last_n_rounds {
            return false; // No trimming needed
        }

        // Identify indices to remove (older rounds)
        let rounds_to_remove = tool_rounds.len() - keep_last_n_rounds;
        let mut indices_to_remove = std::collections::HashSet::new();

        for round in tool_rounds.iter().take(rounds_to_remove) {
            for idx in &round.indices {
                indices_to_remove.insert(*idx);
            }
        }

        // Remove in reverse order to avoid index shifting
        let mut removed_count = 0;
        for idx in (0..messages.len()).rev() {
            if indices_to_remove.contains(&idx) {
                messages.remove(idx);
                removed_count += 1;
            }
        }

        if removed_count > 0 {
            info!(
                "[ContextManager] [Layer-1] Trimmed {} tool messages, kept last {} rounds",
                removed_count, keep_last_n_rounds
            );
        }

        removed_count > 0
    }
}

/// Represents a tool call round (assistant tool_use + user tool_result(s))
#[derive(Debug)]
struct ToolRound {
    _assistant_index: usize,
    tool_result_indices: Vec<usize>,
    indices: Vec<usize>, // All indices in this round
}

/// Identify tool call rounds in the message history
fn identify_tool_rounds(messages: &[Message]) -> Vec<ToolRound> {
    let mut rounds = Vec::new();
    let mut current_round: Option<ToolRound> = None;

    for (i, msg) in messages.iter().enumerate() {
        match msg.role.as_str() {
            "assistant" => {
                if has_tool_use(&msg.content) {
                    // Save previous round if exists
                    if let Some(round) = current_round.take() {
                        rounds.push(round);
                    }
                    // Start new round
                    current_round = Some(ToolRound {
                        _assistant_index: i,
                        tool_result_indices: Vec::new(),
                        indices: vec![i],
                    });
                }
            }
            "user" => {
                if let Some(ref mut round) = current_round {
                    if has_tool_result(&msg.content) {
                        round.tool_result_indices.push(i);
                        round.indices.push(i);
                    } else {
                        // Normal user message ends the current round
                        rounds.push(current_round.take().unwrap());
                    }
                }
            }
            _ => {}
        }
    }

    // Save last round if exists
    if let Some(round) = current_round {
        rounds.push(round);
    }

    debug!(
        "[ContextManager] Identified {} tool rounds in {} messages",
        rounds.len(),
        messages.len()
    );

    rounds
}

/// Check if message content contains tool_use
fn has_tool_use(content: &MessageContent) -> bool {
    if let MessageContent::Array(blocks) = content {
        blocks
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
    } else {
        false
    }
}

/// Check if message content contains tool_result
fn has_tool_result(content: &MessageContent) -> bool {
    if let MessageContent::Array(blocks) = content {
        blocks
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolResult { .. }))
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a request since Default is not implemented
    fn create_test_request() -> ClaudeRequest {
        ClaudeRequest {
            model: "claude-3-5-sonnet".into(),
            messages: vec![],
            system: None,
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
        }
    }

    #[test]
    fn test_estimate_tokens() {
        let mut req = create_test_request();
        req.messages = vec![Message {
            role: "user".into(),
            content: MessageContent::String("Hello World".into()),
        }];

        let tokens = ContextManager::estimate_token_usage(&req);
        assert!(tokens > 0);
        assert!(tokens < 50);
    }

    #[test]
    fn test_purify_history_soft() {
        // Construct history of 6 messages (indices 0-5)
        // 0: Assistant (Ancient) -> Should be purified
        // 1: User
        // 2: Assistant (Old) -> Should be protected (index 2 >= 6-4=2)
        // 3: User
        // 4: Assistant (Recent) -> Should be protected
        // 5: User

        let mut messages = vec![
            Message {
                role: "assistant".into(),
                content: MessageContent::Array(vec![
                    ContentBlock::Thinking {
                        thinking: "ancient".into(),
                        signature: None,
                        cache_control: None,
                    },
                    ContentBlock::Text { text: "A0".into() },
                ]),
            },
            Message {
                role: "user".into(),
                content: MessageContent::String("Q1".into()),
            },
            Message {
                role: "assistant".into(),
                content: MessageContent::Array(vec![
                    ContentBlock::Thinking {
                        thinking: "old".into(),
                        signature: None,
                        cache_control: None,
                    },
                    ContentBlock::Text { text: "A1".into() },
                ]),
            },
            Message {
                role: "user".into(),
                content: MessageContent::String("Q2".into()),
            },
            Message {
                role: "assistant".into(),
                content: MessageContent::Array(vec![
                    ContentBlock::Thinking {
                        thinking: "recent".into(),
                        signature: None,
                        cache_control: None,
                    },
                    ContentBlock::Text { text: "A2".into() },
                ]),
            },
            Message {
                role: "user".into(),
                content: MessageContent::String("current".into()),
            },
        ];

        ContextManager::purify_history(&mut messages, PurificationStrategy::Soft);

        // 0: Ancient -> Filtered
        if let MessageContent::Array(blocks) = &messages[0].content {
            assert_eq!(blocks.len(), 1);
            if let ContentBlock::Text { text } = &blocks[0] {
                assert_eq!(text, "A0");
            } else {
                panic!("Wrong block");
            }
        }

        // 2: Old -> Protected
        if let MessageContent::Array(blocks) = &messages[2].content {
            assert_eq!(blocks.len(), 2);
        }
    }

    #[test]
    fn test_purify_history_aggressive() {
        let mut messages = vec![Message {
            role: "assistant".into(),
            content: MessageContent::Array(vec![
                ContentBlock::Thinking {
                    thinking: "thought".into(),
                    signature: None,
                    cache_control: None,
                },
                ContentBlock::Text {
                    text: "text".into(),
                },
            ]),
        }];

        ContextManager::purify_history(&mut messages, PurificationStrategy::Aggressive);

        if let MessageContent::Array(blocks) = &messages[0].content {
            assert_eq!(blocks.len(), 1);
            assert!(matches!(blocks[0], ContentBlock::Text { .. }));
        }
    }
}
