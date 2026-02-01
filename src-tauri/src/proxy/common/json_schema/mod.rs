//! JSON Schema cleaning and transformation utilities for Gemini API compatibility.
//!
//! This module provides comprehensive JSON Schema preprocessing to ensure
//! compatibility with Google's Gemini API, which has stricter requirements
//! than standard JSON Schema validators.
//!
//! # Module Structure
//!
//! - `cleaner` - Main entry points for schema cleaning
//! - `refs` - $ref/$defs flattening logic
//! - `unions` - allOf/anyOf/oneOf merging
//! - `types` - Type fixing for tool call arguments
//! - `constraints` - Constraint migration to description hints
//!
//! # Usage
//!
//! ```rust
//! use crate::proxy::common::json_schema::{clean_json_schema, clean_json_schema_for_tool};
//!
//! let mut schema = serde_json::json!({
//!     "type": "object",
//!     "properties": {
//!         "name": { "type": "string", "minLength": 1 }
//!     }
//! });
//!
//! clean_json_schema(&mut schema);
//! // Schema is now Gemini-compatible
//! ```

mod cleaner;
mod constraints;
mod refs;
mod types;
mod unions;

#[cfg(test)]
mod tests;

// Re-export main public API
pub use cleaner::{clean_json_schema, clean_json_schema_for_tool};
pub use types::fix_tool_call_args;
