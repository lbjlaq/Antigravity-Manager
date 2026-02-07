//! Constraint migration utilities for JSON Schema.
//!
//! This module handles the conversion of validation constraint fields
//! (which Gemini doesn't support) into description hints that preserve
//! the semantic information for the model.

use serde_json::Value;

use super::unions::append_hint_to_description;

/// Constraint fields not supported by Gemini but containing important semantic info.
/// These fields will be converted to description hints before removal.
pub const CONSTRAINT_FIELDS: &[(&str, &str)] = &[
    ("minLength", "minLen"),
    ("maxLength", "maxLen"),
    ("pattern", "pattern"),
    ("minimum", "min"),
    ("maximum", "max"),
    ("multipleOf", "multipleOf"),
    ("exclusiveMinimum", "exclMin"),
    ("exclusiveMaximum", "exclMax"),
    ("minItems", "minItems"),
    ("maxItems", "maxItems"),
    ("format", "format"),
];

/// Convert constraint fields to description hints.
///
/// Before removing constraint fields, this function preserves their semantic
/// information in the description, allowing the model to understand the constraints.
///
/// # Arguments
/// * `map` - The schema object map to process
///
/// # Example
/// Input: `{ "type": "string", "minLength": 1, "maxLength": 100 }`
/// Output: `{ "type": "string", "description": "[Constraint: minLen: 1, maxLen: 100]" }`
pub fn move_constraints_to_description(map: &mut serde_json::Map<String, Value>) {
    let mut hints = Vec::new();

    for (field, label) in CONSTRAINT_FIELDS {
        if let Some(val) = map.get(*field) {
            if !val.is_null() {
                let val_str = if let Some(s) = val.as_str() {
                    s.to_string()
                } else {
                    val.to_string()
                };
                hints.push(format!("{}: {}", label, val_str));
            }
        }
    }

    if !hints.is_empty() {
        let constraint_hint = format!("[Constraint: {}]", hints.join(", "));
        append_hint_to_description(map, constraint_hint);
    }
}
