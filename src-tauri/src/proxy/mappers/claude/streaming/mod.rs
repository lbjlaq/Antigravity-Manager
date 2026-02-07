//! Claude streaming response transformation (Gemini SSE → Claude SSE).
//!
//! This module handles the conversion of Gemini's streaming responses
//! to Claude's SSE format, including state machine management and
//! content block processing.
//!
//! # Module Structure
//!
//! - `state` - StreamingState state machine and BlockType enum
//! - `processor` - PartProcessor for handling individual parts
//! - `remapper` - Function call argument remapping for Gemini → Claude

mod processor;
mod remapper;
mod state;

#[cfg(test)]
mod tests;

// Re-export public API
pub use processor::PartProcessor;
pub use state::{BlockType, StreamingState};
