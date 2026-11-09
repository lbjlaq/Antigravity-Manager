use super::models::{ContentBlock, Message, MessageContent};
use crate::proxy::SignatureCache;
use tracing::{info, warn};

pub const MIN_SIGNATURE_LENGTH: usize = 32;
pub const SENTINEL_SIGNATURE: &str = crate::proxy::thinking_store::SENTINEL_SIGNATURE;

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
                            let target_lc = target.to_lowercase();
                            if let Some(origin_family) = get_signature_family(s) {
                                let origin_lc = origin_family.to_lowercase();
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
                            } else if target_lc.contains("gemini")
                                && !crate::proxy::thinking_store::is_likely_gemini_signature(s)
                            {
                                warn!(
                                    "[Thinking-Sanitizer] Dropping unknown non-Gemini signature (len: {}) for target '{}'",
                                    s.len(), target
                                );
                                *signature = None;
                                stripped_count += 1;
                            }
                        }
                    }
                }
            }

            // Clean up completely empty thinking blocks without text or signature
            let original_len = blocks.len();
            blocks.retain(|b| {
                if let ContentBlock::Thinking {
                    thinking,
                    signature,
                    ..
                } = b
                {
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
