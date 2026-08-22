//! Context Manager Module
//!
//! Responsible for estimating token usage and purifying context (stripping thinking blocks)
//! to prevent "Prompt is too long" errors and avoid invalid signatures.

use super::caveman_cleaner::CavemanCleaner;
use super::claude::models::{ClaudeRequest, ContentBlock, Message, MessageContent, SystemPrompt};
use super::openai::models::{OpenAIMessage, OpenAIRequest};
use super::rtk_cleaner::RtkCleaner;
use serde_json::{json, Value};
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

/// Estimate token cost for an image from an OpenAI-format image_url.
/// Handles both base64 data URLs and remote URLs.
/// Gemini counts standard images at ~258 tokens. For very large images
/// (>1MB base64 payload), we scale up since high-res images tokenize higher.
fn estimate_image_tokens_from_url(url: &str) -> u32 {
    const BASE_IMAGE_TOKENS: u32 = 258;

    if url.starts_with("data:") {
        // data:image/png;base64,<data>
        // Extract the base64 portion after the comma
        if let Some(comma_pos) = url.find(',') {
            let base64_len = url.len() - comma_pos - 1;
            // Approximate raw bytes: base64 encodes 3 bytes into 4 chars
            let raw_bytes = (base64_len * 3) / 4;

            // Gemini: standard images = 258 tokens
            // High-res images (>1MB) scale higher, up to ~10k tokens for very large ones
            if raw_bytes > 4_000_000 {
                // >4MB: very high resolution
                10_000
            } else if raw_bytes > 1_000_000 {
                // 1-4MB: high resolution, scale linearly
                let factor = raw_bytes as f32 / 1_000_000.0;
                (BASE_IMAGE_TOKENS as f32 * factor * 4.0).ceil() as u32
            } else {
                BASE_IMAGE_TOKENS
            }
        } else {
            BASE_IMAGE_TOKENS
        }
    } else {
        // Remote URL: assume standard resolution
        BASE_IMAGE_TOKENS
    }
}

/// Estimate token cost for audio from a URL.
/// Audio is tokenized at roughly 32 tokens per second.
/// From base64, we estimate duration from payload size.
fn estimate_media_tokens_from_url(url: &str) -> u32 {
    const BASE_AUDIO_TOKENS: u32 = 500; // ~15 seconds default

    if url.starts_with("data:") {
        if let Some(comma_pos) = url.find(',') {
            let base64_len = url.len() - comma_pos - 1;
            let raw_bytes = (base64_len * 3) / 4;
            // Rough estimate: 16kHz, 16-bit mono audio ≈ 32KB/s
            // 32 tokens/second
            let estimated_seconds = raw_bytes as f32 / 32_000.0;
            let tokens = (estimated_seconds * 32.0).ceil() as u32;
            tokens.max(64) // minimum 64 tokens for any audio
        } else {
            BASE_AUDIO_TOKENS
        }
    } else {
        BASE_AUDIO_TOKENS
    }
}

/// Estimate token cost for Gemini inlineData (base64 images/audio in Gemini format).
/// mime_type determines the estimation strategy. data_len is the length of the base64 string.
fn estimate_inline_data_tokens(mime_type: &str, data_len: usize) -> u32 {
    if mime_type.starts_with("image/") {
        // Same logic as estimate_image_tokens_from_url for base64
        let raw_bytes = (data_len * 3) / 4;
        if raw_bytes > 4_000_000 {
            10_000
        } else if raw_bytes > 1_000_000 {
            let factor = raw_bytes as f32 / 1_000_000.0;
            (258.0 * factor * 4.0).ceil() as u32
        } else {
            258
        }
    } else if mime_type.starts_with("audio/") {
        let raw_bytes = (data_len * 3) / 4;
        let estimated_seconds = raw_bytes as f32 / 32_000.0;
        (estimated_seconds * 32.0).ceil().max(64.0) as u32
    } else if mime_type.starts_with("video/") {
        // Video: very expensive, rough estimate
        let raw_bytes = (data_len * 3) / 4;
        let estimated_seconds = raw_bytes as f32 / 500_000.0; // rough video bitrate
        (estimated_seconds * 300.0).ceil().max(258.0) as u32 // ~300 tokens/sec for video
    } else {
        // Unknown media: treat as text approximation
        estimate_tokens_from_str(&format!("[binary data: {} bytes]", data_len))
    }
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

        let mut modified = Self::strip_thinking_blocks(messages, protected_last_n);

        // Apply Caveman cleaning to older message contents to save tokens
        let total_msgs = messages.len();
        let start_protection_idx = total_msgs.saturating_sub(protected_last_n);
        for (i, msg) in messages.iter_mut().enumerate() {
            if i >= start_protection_idx {
                continue;
            }
            if msg.role == "user" || msg.role == "assistant" {
                match &mut msg.content {
                    MessageContent::String(s) => {
                        let cleaned = CavemanCleaner::clean(s);
                        if cleaned != *s {
                            *s = cleaned;
                            modified = true;
                        }
                    }
                    MessageContent::Array(blocks) => {
                        for block in blocks {
                            if let ContentBlock::Text { text } = block {
                                let cleaned = CavemanCleaner::clean(text);
                                if cleaned != *text {
                                    *text = cleaned;
                                    modified = true;
                                }
                            }
                        }
                    }
                }
            }
        }

        modified
    }

    /// Clean tool result messages using RtkCleaner
    pub fn clean_tool_message(msg: &mut Message) -> bool {
        let mut modified = false;
        if let MessageContent::Array(blocks) = &mut msg.content {
            for block in blocks {
                if let ContentBlock::ToolResult { content, .. } = block {
                    if let Some(s) = content.as_str() {
                        let cleaned = RtkCleaner::clean(s, 48);
                        if cleaned != s {
                            *content = serde_json::Value::String(cleaned);
                            modified = true;
                        }
                    }
                }
            }
        }
        modified
    }

    /// Clean OpenAI tool message using RtkCleaner
    pub fn clean_openai_tool_message(msg: &mut OpenAIMessage) -> bool {
        if msg.role == "tool" {
            if let Some(ref mut content) = msg.content {
                match content {
                    crate::proxy::mappers::openai::models::OpenAIContent::String(s) => {
                        let cleaned = RtkCleaner::clean(s, 48);
                        if cleaned != *s {
                            *s = cleaned;
                            return true;
                        }
                    }
                    crate::proxy::mappers::openai::models::OpenAIContent::Array(blocks) => {
                        let mut modified = false;
                        for block in blocks {
                            if let crate::proxy::mappers::openai::models::OpenAIContentBlock::Text { text } = block {
                                let cleaned = RtkCleaner::clean(text, 48);
                                if cleaned != *text {
                                    *text = cleaned;
                                    modified = true;
                                }
                            }
                        }
                        return modified;
                    }
                }
            }
        }
        false
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

    /// Extract last valid signature for OpenAI using SignatureCache
    pub fn extract_last_openai_valid_signature(session_id: &str) -> Option<String> {
        crate::proxy::signature_cache::SignatureCache::global().get_session_signature(session_id)
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
        let mut modified = false;

        // Clean retained tool messages using RtkCleaner
        for msg in messages.iter_mut() {
            if Self::clean_tool_message(msg) {
                modified = true;
            }
        }

        if tool_rounds.len() <= keep_last_n_rounds {
            return modified; // No trimming needed, but might have cleaned some messages
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

    /// Restore reasoning text for assistant messages from cache in OpenAI format
    pub fn restore_openai_reasoning_content(messages: &mut Vec<OpenAIMessage>, session_id: &str) {
        let mut assistant_turn_count = 0;
        for msg in messages.iter_mut() {
            if msg.role == "assistant" {
                let is_missing = msg
                    .reasoning_content
                    .as_ref()
                    .map(|s| s.is_empty() || s == "[undefined]")
                    .unwrap_or(true);

                if is_missing {
                    if let Some(cached_reasoning) = crate::proxy::SignatureCache::global()
                        .get_session_reasoning(session_id, assistant_turn_count)
                    {
                        tracing::debug!(
                            "[OpenAI-Reasoning] Restored reasoning for assistant turn {} (len: {})",
                            assistant_turn_count,
                            cached_reasoning.len()
                        );
                        msg.reasoning_content = Some(cached_reasoning);
                    }
                }
                assistant_turn_count += 1;
            }
        }
    }

    /// Purify reasoning text in OpenAI history to save tokens
    pub fn purify_openai_history(
        messages: &mut Vec<OpenAIMessage>,
        strategy: PurificationStrategy,
    ) -> bool {
        let protected_last_n = match strategy {
            PurificationStrategy::Soft => 4, // Keep thinking for recent 4 messages (approx 2 turns)
            PurificationStrategy::Aggressive => 0,
        };
        let total_msgs = messages.len();
        if total_msgs == 0 {
            return false;
        }
        let start_protection_idx = total_msgs.saturating_sub(protected_last_n);
        let mut modified = false;

        for (i, msg) in messages.iter_mut().enumerate() {
            if i >= start_protection_idx {
                continue;
            }
            if msg.role == "assistant" && msg.reasoning_content.is_some() {
                // [FIX] If the assistant message contains tool calls, do NOT strip its reasoning_content/signature.
                // Otherwise, the signature chain is broken, and Google API throws a 400 thought_signature error for historical tool calls.
                let has_tool_calls = msg
                    .tool_calls
                    .as_ref()
                    .map(|tc| !tc.is_empty())
                    .unwrap_or(false);
                if !has_tool_calls {
                    tracing::debug!(
                        "[ContextManager] Purifying reasoning_content of message {} (len: {})",
                        i,
                        msg.reasoning_content.as_ref().unwrap().len()
                    );
                    msg.reasoning_content = None;
                    modified = true;
                }
            }
            if msg.role == "user" || msg.role == "assistant" {
                if let Some(ref mut content) = msg.content {
                    match content {
                        crate::proxy::mappers::openai::models::OpenAIContent::String(s) => {
                            let cleaned = CavemanCleaner::clean(s);
                            if cleaned != *s {
                                *s = cleaned;
                                modified = true;
                            }
                        }
                        crate::proxy::mappers::openai::models::OpenAIContent::Array(blocks) => {
                            for block in blocks {
                                if let crate::proxy::mappers::openai::models::OpenAIContentBlock::Text { text } = block {
                                    let cleaned = CavemanCleaner::clean(text);
                                    if cleaned != *text {
                                        *text = cleaned;
                                        modified = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        modified
    }

    /// Trim old tool messages in OpenAI format, keeping only the last N rounds
    pub fn trim_openai_tool_messages(
        messages: &mut Vec<OpenAIMessage>,
        keep_last_n_rounds: usize,
    ) -> bool {
        let tool_rounds = identify_openai_tool_rounds(messages);
        let mut modified = false;

        // Clean retained tool messages using RtkCleaner
        for msg in messages.iter_mut() {
            if Self::clean_openai_tool_message(msg) {
                modified = true;
            }
        }

        if tool_rounds.len() <= keep_last_n_rounds {
            return modified;
        }

        let rounds_to_remove = tool_rounds.len() - keep_last_n_rounds;
        let mut indices_to_remove = std::collections::HashSet::new();

        for round in tool_rounds.iter().take(rounds_to_remove) {
            for idx in &round.indices {
                indices_to_remove.insert(*idx);
            }
        }

        let mut removed_count = 0;
        for idx in (0..messages.len()).rev() {
            if indices_to_remove.contains(&idx) {
                messages.remove(idx);
                removed_count += 1;
            }
        }

        if removed_count > 0 {
            info!(
                "[ContextManager] [OpenAI] Trimmed {} tool messages, kept last {} rounds",
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

struct OpenAIToolRound {
    _assistant_index: usize,
    _tool_indices: Vec<usize>,
    indices: Vec<usize>,
}

fn identify_openai_tool_rounds(messages: &[OpenAIMessage]) -> Vec<OpenAIToolRound> {
    let mut rounds = Vec::new();
    let mut current_round: Option<OpenAIToolRound> = None;

    for (i, msg) in messages.iter().enumerate() {
        if msg.role == "assistant"
            && msg.tool_calls.is_some()
            && !msg.tool_calls.as_ref().unwrap().is_empty()
        {
            if let Some(round) = current_round.take() {
                rounds.push(round);
            }
            current_round = Some(OpenAIToolRound {
                _assistant_index: i,
                _tool_indices: Vec::new(),
                indices: vec![i],
            });
        } else if msg.role == "tool" || msg.role == "function" || msg.tool_call_id.is_some() {
            if let Some(ref mut round) = current_round {
                round._tool_indices.push(i);
                round.indices.push(i);
            }
        } else if msg.role == "user" {
            if let Some(round) = current_round.take() {
                rounds.push(round);
            }
        }
    }
    if let Some(round) = current_round {
        rounds.push(round);
    }
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
impl ContextManager {
    /// Estimate token usage for an OpenAI Request
    pub fn estimate_openai_token_usage(request: &OpenAIRequest) -> u32 {
        let mut total = 0;

        // System or developer messages, tools definitions, prompt
        if let Some(prompt) = &request.prompt {
            total += estimate_tokens_from_str(prompt);
        }
        if let Some(instructions) = &request.instructions {
            total += estimate_tokens_from_str(instructions);
        }

        for msg in &request.messages {
            total += 4; // msg overhead

            if let Some(ref content) = msg.content {
                match content {
                    crate::proxy::mappers::openai::models::OpenAIContent::String(s) => {
                        total += estimate_tokens_from_str(s);
                    }
                    crate::proxy::mappers::openai::models::OpenAIContent::Array(blocks) => {
                        for block in blocks {
                            match block {
                                crate::proxy::mappers::openai::models::OpenAIContentBlock::Text { text } => {
                                    total += estimate_tokens_from_str(text);
                                }
                                crate::proxy::mappers::openai::models::OpenAIContentBlock::ImageUrl { image_url } => {
                                    // Gemini counts images at ~258 tokens for standard resolution.
                                    // For base64 data URLs, estimate higher based on payload size
                                    // since very large images can consume significantly more tokens.
                                    total += estimate_image_tokens_from_url(&image_url.url);
                                }
                                crate::proxy::mappers::openai::models::OpenAIContentBlock::AudioUrl { audio_url } => {
                                    // Audio is tokenized at ~32 tokens per second (~25 bytes/token from base64)
                                    total += estimate_media_tokens_from_url(&audio_url.url);
                                }
                                crate::proxy::mappers::openai::models::OpenAIContentBlock::InputAudio { input_audio } => {
                                    // input_audio 携带裸 base64，直接按 inlineData 估算
                                    total += estimate_inline_data_tokens(
                                        &input_audio.mime_type(),
                                        input_audio.data.len(),
                                    );
                                }
                            }
                        }
                    }
                }
            }

            if let Some(ref reasoning) = msg.reasoning_content {
                total += estimate_tokens_from_str(reasoning);
                total += 100; // signature/thinking overhead
            }

            if let Some(ref tool_calls) = msg.tool_calls {
                for tc in tool_calls {
                    total += 20;
                    if let Some(ref func) = tc.function {
                        total += estimate_tokens_from_str(&func.name);
                        total += estimate_tokens_from_str(&func.arguments);
                    }
                }
            }

            if let Some(ref name) = msg.name {
                total += estimate_tokens_from_str(name);
            }
        }

        if let Some(tools) = &request.tools {
            for tool in tools {
                if let Ok(json_str) = serde_json::to_string(tool) {
                    total += estimate_tokens_from_str(&json_str);
                }
            }
        }

        if let Some(thinking) = &request.thinking {
            if let Some(budget) = thinking.budget_tokens {
                total += budget;
            }
        }

        total
    }

    /// Compress thinking content in OpenAI request while keeping it in a lightweight representation
    pub fn compress_openai_thinking_preserve_signature(
        messages: &mut Vec<OpenAIMessage>,
        protected_last_n: usize,
    ) -> bool {
        let total_msgs = messages.len();
        if total_msgs == 0 {
            return false;
        }

        let start_protection_idx = total_msgs.saturating_sub(protected_last_n);
        let mut compressed_count = 0;

        for (i, msg) in messages.iter_mut().enumerate() {
            if i >= start_protection_idx {
                continue;
            }

            if msg.role == "assistant" {
                if let Some(ref mut reasoning) = msg.reasoning_content {
                    // [FIX] If the assistant message contains tool calls, do NOT strip its reasoning_content/signature.
                    let has_tool_calls = msg
                        .tool_calls
                        .as_ref()
                        .map(|tc| !tc.is_empty())
                        .unwrap_or(false);
                    if !has_tool_calls && reasoning.len() > 10 {
                        *reasoning = "...".to_string();
                        compressed_count += 1;
                    }
                }
            }
        }

        compressed_count > 0
    }

    /// Estimate token usage for a Gemini Request represented as serde_json::Value
    pub fn estimate_gemini_token_usage(body: &Value) -> u32 {
        let mut total = 0;

        // systemInstruction
        if let Some(sys_inst) = body.get("systemInstruction") {
            if let Some(parts) = sys_inst.get("parts").and_then(|p| p.as_array()) {
                for part in parts {
                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                        total += estimate_tokens_from_str(text);
                    }
                }
            }
        }

        // contents
        if let Some(contents) = body.get("contents").and_then(|c| c.as_array()) {
            for msg in contents {
                total += 4; // msg overhead
                if let Some(parts) = msg.get("parts").and_then(|p| p.as_array()) {
                    for part in parts {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            total += estimate_tokens_from_str(text);
                        }
                        if let Some(thought) = part.get("thought").and_then(|t| t.as_bool()) {
                            if thought {
                                total += 100; // thinking overhead
                            }
                        }
                        // inlineData: base64-encoded images/audio embedded in the request
                        if let Some(inline_data) = part.get("inlineData") {
                            let mime = inline_data
                                .get("mimeType")
                                .and_then(|m| m.as_str())
                                .unwrap_or("");
                            let data_len = inline_data
                                .get("data")
                                .and_then(|d| d.as_str())
                                .map(|s| s.len())
                                .unwrap_or(0);
                            total += estimate_inline_data_tokens(mime, data_len);
                        }
                        if let Some(fc) = part.get("functionCall") {
                            total += 20;
                            if let Some(name) = fc.get("name").and_then(|n| n.as_str()) {
                                total += estimate_tokens_from_str(name);
                            }
                            if let Some(args) = fc.get("args") {
                                if let Ok(json_str) = serde_json::to_string(args) {
                                    total += estimate_tokens_from_str(&json_str);
                                }
                            }
                        }
                        if let Some(fr) = part.get("functionResponse") {
                            total += 10;
                            if let Some(name) = fr.get("name").and_then(|n| n.as_str()) {
                                total += estimate_tokens_from_str(name);
                            }
                            if let Some(resp) = fr.get("response") {
                                if let Ok(json_str) = serde_json::to_string(resp) {
                                    total += estimate_tokens_from_str(&json_str);
                                }
                            }
                        }
                    }
                }
            }
        }

        // tools
        if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
            for tool in tools {
                if let Ok(json_str) = serde_json::to_string(tool) {
                    total += estimate_tokens_from_str(&json_str);
                }
            }
        }

        total
    }

    /// Trim old tool messages in Gemini request body, keeping only the last N rounds
    pub fn trim_gemini_tool_messages(body: &mut Value, keep_last_n_rounds: usize) -> bool {
        if let Some(contents) = body.get_mut("contents").and_then(|c| c.as_array_mut()) {
            let mut tool_rounds = Vec::new();
            let mut current_round: Option<OpenAIToolRound> = None;

            for (i, msg) in contents.iter().enumerate() {
                let role = msg
                    .get("role")
                    .and_then(|r| r.as_str())
                    .unwrap_or("unknown");
                let has_function_call = msg
                    .get("parts")
                    .and_then(|p| p.as_array())
                    .map(|arr| arr.iter().any(|part| part.get("functionCall").is_some()))
                    .unwrap_or(false);
                let has_function_response = msg
                    .get("parts")
                    .and_then(|p| p.as_array())
                    .map(|arr| {
                        arr.iter()
                            .any(|part| part.get("functionResponse").is_some())
                    })
                    .unwrap_or(false);

                if role == "model" && has_function_call {
                    if let Some(round) = current_round.take() {
                        tool_rounds.push(round);
                    }
                    current_round = Some(OpenAIToolRound {
                        _assistant_index: i,
                        _tool_indices: Vec::new(),
                        indices: vec![i],
                    });
                } else if role == "user" && has_function_response {
                    if let Some(ref mut round) = current_round {
                        round._tool_indices.push(i);
                        round.indices.push(i);
                    }
                } else if role == "user" {
                    if let Some(round) = current_round.take() {
                        tool_rounds.push(round);
                    }
                }
            }
            if let Some(round) = current_round {
                tool_rounds.push(round);
            }

            // 对保留下来的工具消息部分进行 RTK 日志降噪 (就地修改)
            for msg in contents.iter_mut() {
                if let Some(parts) = msg.get_mut("parts").and_then(|p| p.as_array_mut()) {
                    for part in parts {
                        if let Some(fr) = part.get_mut("functionResponse") {
                            if let Some(resp_obj) =
                                fr.get_mut("response").and_then(|r| r.as_object_mut())
                            {
                                for (_key, val) in resp_obj.iter_mut() {
                                    if let Some(s) = val.as_str() {
                                        let cleaned = RtkCleaner::clean(s, 48);
                                        if cleaned != s {
                                            *val = json!(cleaned);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if tool_rounds.len() <= keep_last_n_rounds {
                return false;
            }

            let rounds_to_remove = tool_rounds.len() - keep_last_n_rounds;
            let mut indices_to_remove = std::collections::HashSet::new();

            for round in tool_rounds.iter().take(rounds_to_remove) {
                for idx in &round.indices {
                    indices_to_remove.insert(*idx);
                }
            }

            let mut removed_count = 0;
            for idx in (0..contents.len()).rev() {
                if indices_to_remove.contains(&idx) {
                    contents.remove(idx);
                    removed_count += 1;
                }
            }

            if removed_count > 0 {
                info!(
                    "[ContextManager] [Gemini] Trimmed {} tool messages, kept last {} rounds",
                    removed_count, keep_last_n_rounds
                );
            }
            removed_count > 0
        } else {
            false
        }
    }

    /// Compress thinking in Gemini request body, keeping it in a lightweight representation
    pub fn compress_gemini_thinking_preserve_signature(
        body: &mut Value,
        protected_last_n: usize,
    ) -> bool {
        if let Some(contents) = body.get_mut("contents").and_then(|c| c.as_array_mut()) {
            let total_turns = contents.len();
            if total_turns == 0 {
                return false;
            }

            let start_protection_idx = total_turns.saturating_sub(protected_last_n);
            let mut compressed_count = 0;

            for (i, msg) in contents.iter_mut().enumerate() {
                if i >= start_protection_idx {
                    continue;
                }

                let role = msg
                    .get("role")
                    .and_then(|r| r.as_str())
                    .unwrap_or("unknown");
                if role == "model" {
                    if let Some(parts) = msg.get_mut("parts").and_then(|p| p.as_array_mut()) {
                        for part in parts {
                            if let Some(obj) = part.as_object_mut() {
                                if let Some(thought) = obj.get("thought").and_then(|t| t.as_bool())
                                {
                                    if thought {
                                        if let Some(text) =
                                            obj.get_mut("text").and_then(|t| t.as_str())
                                        {
                                            if text.len() > 10 {
                                                obj.insert("text".to_string(), json!("..."));
                                                compressed_count += 1;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            compressed_count > 0
        } else {
            false
        }
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
