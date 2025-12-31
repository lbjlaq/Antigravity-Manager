// Shared constants for proxy mappers.

/// Gemini tool-call signature sentinel.
///
/// Gemini 3+ validates thoughtSignature for tool calls; some clients strip the field.
/// This sentinel instructs the backend to skip signature validation.
pub const GEMINI_SKIP_SIGNATURE: &str = "skip_thought_signature_validator";

