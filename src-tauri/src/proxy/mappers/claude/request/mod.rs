// Claude Request Transformation Module
// Converts Claude requests to Gemini v1internal format

mod cleanup;
mod contents;
mod generation;
mod safety;
mod sorting;
mod system;
mod thinking;
mod tools;
mod transform;

// Re-export main transformation function
pub use transform::transform_claude_request_in;

// Re-export cleanup utilities (used by handlers)
pub use cleanup::clean_cache_control_from_messages;
pub use cleanup::clean_thinking_fields_recursive;

// Re-export sorting utilities (used by handlers)
pub use sorting::merge_consecutive_messages;

// Re-export safety configuration
pub use safety::SafetyThreshold;

// Internal re-exports for use within this module

#[cfg(test)]
mod tests;
