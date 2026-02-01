//! Type fixing utilities for tool call arguments.
//!
//! This module provides functions to automatically convert tool call argument
//! values to match their schema-defined types, handling common mismatches like
//! strings being passed where numbers are expected.

use serde_json::Value;

/// Fix tool call argument types to match schema definitions.
///
/// Automatically converts argument values to their expected types:
/// - "123" -> 123 (string -> number/integer)
/// - "true" -> true (string -> boolean)
/// - 123 -> "123" (number -> string)
///
/// # Arguments
/// * `args` - Tool call arguments object (modified in place)
/// * `schema` - Tool parameter schema definition (typically the `parameters` object)
///
/// # Example
/// ```ignore
/// let mut args = json!({"port": "8080", "enabled": "true"});
/// let schema = json!({
///     "properties": {
///         "port": {"type": "integer"},
///         "enabled": {"type": "boolean"}
///     }
/// });
/// fix_tool_call_args(&mut args, &schema);
/// // args is now {"port": 8080, "enabled": true}
/// ```
pub fn fix_tool_call_args(args: &mut Value, schema: &Value) {
    if let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) {
        if let Some(args_obj) = args.as_object_mut() {
            for (key, value) in args_obj.iter_mut() {
                if let Some(prop_schema) = properties.get(key) {
                    fix_single_arg_recursive(value, prop_schema);
                }
            }
        }
    }
}

/// Recursively fix a single argument's type.
///
/// Handles nested objects and arrays recursively.
fn fix_single_arg_recursive(value: &mut Value, schema: &Value) {
    // 1. Handle nested objects (properties)
    if let Some(nested_props) = schema.get("properties").and_then(|p| p.as_object()) {
        if let Some(value_obj) = value.as_object_mut() {
            for (key, nested_value) in value_obj.iter_mut() {
                if let Some(nested_schema) = nested_props.get(key) {
                    fix_single_arg_recursive(nested_value, nested_schema);
                }
            }
        }
        return;
    }

    // 2. Handle arrays (items)
    let schema_type = schema
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_lowercase();
    if schema_type == "array" {
        if let Some(items_schema) = schema.get("items") {
            if let Some(arr) = value.as_array_mut() {
                for item in arr {
                    fix_single_arg_recursive(item, items_schema);
                }
            }
        }
        return;
    }

    // 3. Handle primitive type conversion
    match schema_type.as_str() {
        "number" | "integer" => {
            // String -> Number
            if let Some(s) = value.as_str() {
                // [SAFETY] Protect version numbers or codes with leading zeros (e.g., "01", "007")
                if s.starts_with('0') && s.len() > 1 && !s.starts_with("0.") {
                    return;
                }

                // Prefer parsing as integer first
                if let Ok(i) = s.parse::<i64>() {
                    *value = Value::Number(serde_json::Number::from(i));
                } else if let Ok(f) = s.parse::<f64>() {
                    if let Some(n) = serde_json::Number::from_f64(f) {
                        *value = Value::Number(n);
                    }
                }
            }
        }
        "boolean" => {
            // String -> Boolean
            if let Some(s) = value.as_str() {
                match s.to_lowercase().as_str() {
                    "true" | "1" | "yes" | "on" => *value = Value::Bool(true),
                    "false" | "0" | "no" | "off" => *value = Value::Bool(false),
                    _ => {}
                }
            } else if let Some(n) = value.as_i64() {
                // Number 1/0 -> Boolean
                if n == 1 {
                    *value = Value::Bool(true);
                } else if n == 0 {
                    *value = Value::Bool(false);
                }
            }
        }
        "string" => {
            // Non-string -> String (prevent clients from passing numbers to text fields)
            if !value.is_string() && !value.is_null() && !value.is_object() && !value.is_array() {
                *value = Value::String(value.to_string());
            }
        }
        _ => {}
    }
}
