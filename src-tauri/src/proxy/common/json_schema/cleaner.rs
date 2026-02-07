//! Main JSON Schema cleaning logic for Gemini API compatibility.
//!
//! This module provides the core cleaning functions that transform arbitrary
//! JSON Schema into Gemini-compatible format.

use once_cell::sync::Lazy;
use serde_json::{json, Value};

use super::constraints::move_constraints_to_description;
use super::refs::{collect_all_defs, flatten_refs};
use super::unions::{append_hint_to_description, extract_best_schema_from_union, merge_all_of};
use crate::proxy::common::tool_adapter::ToolAdapter;
use crate::proxy::common::tool_adapters::PencilAdapter;

/// Global tool adapter registry.
///
/// All registered adapters are checked and applied during schema cleaning.
static TOOL_ADAPTERS: Lazy<Vec<Box<dyn ToolAdapter>>> = Lazy::new(|| {
    vec![
        Box::new(PencilAdapter),
        // Future adapters can be easily added:
        // Box::new(FilesystemAdapter),
        // Box::new(DatabaseAdapter),
    ]
});

/// Recursively clean JSON Schema to meet Gemini API requirements.
///
/// # Processing Steps
///
/// 1. Expand $ref and $defs: Replace references with actual definitions
/// 2. Remove unsupported fields: $schema, additionalProperties, format, default, etc.
/// 3. Handle union types: ["string", "null"] -> "string"
/// 4. Handle anyOf unions: anyOf: [{"type": "string"}, {"type": "null"}] -> "type": "string"
/// 5. Convert type field values to lowercase (Gemini v1internal requirement)
/// 6. Remove numeric validation fields: multipleOf, exclusiveMinimum, etc.
pub fn clean_json_schema(value: &mut Value) {
    // 0. Preprocessing: Expand $ref (Schema Flattening)
    // [FIX #952] Recursively collect all $defs/definitions from all levels
    let mut all_defs = serde_json::Map::new();
    collect_all_defs(value, &mut all_defs);

    // Remove root-level $defs/definitions (backward compatibility)
    if let Value::Object(map) = value {
        map.remove("$defs");
        map.remove("definitions");
    }

    // [FIX #952] Always run flatten_refs, even if defs is empty
    // This catches and handles unresolvable $refs (fallback to string type)
    if let Value::Object(map) = value {
        flatten_refs(map, &all_defs);
    }

    // Recursive cleaning
    clean_json_schema_recursive(value, true);
}

/// Schema cleaning with tool adapter support.
///
/// This is the recommended entry point, supporting tool-specific optimizations.
///
/// # Arguments
/// * `value` - The JSON Schema to clean
/// * `tool_name` - Tool name for adapter matching
///
/// # Processing Flow
/// 1. Find matching tool adapter
/// 2. Execute adapter pre-processing (tool-specific optimizations)
/// 3. Execute common cleaning logic
/// 4. Execute adapter post-processing (final adjustments)
pub fn clean_json_schema_for_tool(value: &mut Value, tool_name: &str) {
    // 1. Find matching adapter
    let adapter = TOOL_ADAPTERS.iter().find(|a| a.matches(tool_name));

    // 2. Execute pre-processing
    if let Some(adapter) = adapter {
        let _ = adapter.pre_process(value);
    }

    // 3. Execute common cleaning
    clean_json_schema(value);

    // 4. Execute post-processing
    if let Some(adapter) = adapter {
        let _ = adapter.post_process(value);
    }
}

/// Recursive implementation of schema cleaning.
///
/// Returns `true` if the node is effectively nullable (for removing from required).
pub(crate) fn clean_json_schema_recursive(value: &mut Value, is_schema_node: bool) -> bool {
    let mut is_effectively_nullable = false;

    match value {
        Value::Object(map) => {
            // 0. Merge allOf
            merge_all_of(map);

            // 0.5. Structure normalization
            // Fix MCP tools (like pencil) that misuse items for object properties.
            // If type=object or contains properties but also defines items,
            // Gemini will error because items can only appear in arrays.
            if map.get("type").and_then(|t| t.as_str()) == Some("object")
                || map.contains_key("properties")
            {
                if let Some(items) = map.remove("items") {
                    tracing::warn!("[Schema-Normalization] Found 'items' in an Object-like node. Moving content to 'properties'.");
                    let target_props =
                        map.entry("properties".to_string()).or_insert_with(|| json!({}));
                    if let Some(target_map) = target_props.as_object_mut() {
                        if let Some(source_map) = items.as_object() {
                            for (k, v) in source_map {
                                target_map.entry(k.clone()).or_insert_with(|| v.clone());
                            }
                        }
                    }
                }
            }

            // 1. Deep recursive processing of child items
            // Process properties (object)
            if let Some(Value::Object(props)) = map.get_mut("properties") {
                let mut nullable_keys = std::collections::HashSet::new();
                for (k, v) in props {
                    // Each property value must be an independent Schema node
                    if clean_json_schema_recursive(v, true) {
                        nullable_keys.insert(k.clone());
                    }
                }

                if !nullable_keys.is_empty() {
                    if let Some(Value::Array(req_arr)) = map.get_mut("required") {
                        req_arr.retain(|r| {
                            r.as_str()
                                .map(|s| !nullable_keys.contains(s))
                                .unwrap_or(true)
                        });
                        if req_arr.is_empty() {
                            map.remove("required");
                        }
                    }
                }

                // Implicit type injection: if properties exist but no type, set to object
                if !map.contains_key("type") {
                    map.insert("type".to_string(), Value::String("object".to_string()));
                }
            }

            // Process items (array)
            if let Some(items) = map.get_mut("items") {
                // items content must be an independent Schema node
                clean_json_schema_recursive(items, true);

                // Implicit type injection: if items exist but no type, set to array
                if !map.contains_key("type") {
                    map.insert("type".to_string(), Value::String("array".to_string()));
                }
            }

            // Fallback: clean regular objects without properties or items
            if !map.contains_key("properties") && !map.contains_key("items") {
                for (k, v) in map.iter_mut() {
                    // Exclude keywords
                    if k != "anyOf" && k != "oneOf" && k != "allOf" && k != "enum" && k != "type" {
                        clean_json_schema_recursive(v, false);
                    }
                }
            }

            // 1.5. Recursively clean each branch in anyOf/oneOf arrays
            // Must execute before merge logic to ensure merged branches are cleaned
            if let Some(Value::Array(any_of)) = map.get_mut("anyOf") {
                for branch in any_of.iter_mut() {
                    clean_json_schema_recursive(branch, true);
                }
            }
            if let Some(Value::Array(one_of)) = map.get_mut("oneOf") {
                for branch in one_of.iter_mut() {
                    clean_json_schema_recursive(branch, true);
                }
            }

            // 2. [FIX #815] Handle anyOf/oneOf unions: merge properties instead of removing
            let mut union_to_merge = None;
            if map.get("type").is_none()
                || map.get("type").and_then(|t| t.as_str()) == Some("object")
            {
                if let Some(Value::Array(any_of)) = map.get("anyOf") {
                    union_to_merge = Some(any_of.clone());
                } else if let Some(Value::Array(one_of)) = map.get("oneOf") {
                    union_to_merge = Some(one_of.clone());
                }
            }

            if let Some(union_array) = union_to_merge {
                if let Some((best_branch, all_types)) =
                    extract_best_schema_from_union(&union_array)
                {
                    if let Value::Object(branch_obj) = best_branch {
                        for (k, v) in branch_obj {
                            if k == "properties" {
                                if let Some(target_props) = map
                                    .entry("properties".to_string())
                                    .or_insert_with(|| Value::Object(serde_json::Map::new()))
                                    .as_object_mut()
                                {
                                    if let Some(source_props) = v.as_object() {
                                        for (pk, pv) in source_props {
                                            target_props
                                                .entry(pk.clone())
                                                .or_insert_with(|| pv.clone());
                                        }
                                    }
                                }
                            } else if k == "required" {
                                if let Some(target_req) = map
                                    .entry("required".to_string())
                                    .or_insert_with(|| Value::Array(Vec::new()))
                                    .as_array_mut()
                                {
                                    if let Some(source_req) = v.as_array() {
                                        for rv in source_req {
                                            if !target_req.contains(rv) {
                                                target_req.push(rv.clone());
                                            }
                                        }
                                    }
                                }
                            } else if !map.contains_key(&k) {
                                map.insert(k, v);
                            }
                        }
                    }

                    // Add type hint to description (reference: CLIProxyAPI)
                    if all_types.len() > 1 {
                        let type_hint = format!("Accepts: {}", all_types.join(" | "));
                        append_hint_to_description(map, type_hint);
                    }
                }
            }

            // 3. Check if current object is a JSON Schema node
            // Only apply whitelist filtering when object looks like Schema
            let allowed_fields = [
                "type",
                "description",
                "properties",
                "required",
                "items",
                "enum",
                "title",
            ];

            let has_standard_keyword = map.keys().any(|k| allowed_fields.contains(&k.as_str()));

            // Heuristic fix: if clearly a Schema node but no standard keywords, yet has other keys
            // We infer this is a "shorthand" object definition and move keys to properties
            let is_not_schema_payload =
                map.contains_key("functionCall") || map.contains_key("functionResponse");
            if is_schema_node && !has_standard_keyword && !map.is_empty() && !is_not_schema_payload
            {
                let mut properties = serde_json::Map::new();
                let keys: Vec<String> = map.keys().cloned().collect();
                for k in keys {
                    if let Some(v) = map.remove(&k) {
                        properties.insert(k, v);
                    }
                }
                map.insert("type".to_string(), Value::String("object".to_string()));
                map.insert("properties".to_string(), Value::Object(properties));

                // Recursively clean properties just moved
                if let Some(Value::Object(props_map)) = map.get_mut("properties") {
                    for v in props_map.values_mut() {
                        clean_json_schema_recursive(v, true);
                    }
                }
            }

            let looks_like_schema =
                (is_schema_node || has_standard_keyword) && !is_not_schema_payload;

            if looks_like_schema {
                // 4. Constraint migration: convert validation fields to description hints
                move_constraints_to_description(map);

                // 5. Whitelist filtering: physically remove Gemini-unsupported content
                let keys_to_remove: Vec<String> = map
                    .keys()
                    .filter(|k| !allowed_fields.contains(&k.as_str()))
                    .cloned()
                    .collect();
                for k in keys_to_remove {
                    map.remove(&k);
                }

                // 6. Handle empty Object
                if map.get("type").and_then(|t| t.as_str()) == Some("object") {
                    if !map.contains_key("properties") {
                        map.insert("properties".to_string(), serde_json::json!({}));
                    }
                }

                // 7. Required field alignment
                let valid_prop_keys: Option<std::collections::HashSet<String>> = map
                    .get("properties")
                    .and_then(|p| p.as_object())
                    .map(|obj| obj.keys().cloned().collect());

                if let Some(required_val) = map.get_mut("required") {
                    if let Some(req_arr) = required_val.as_array_mut() {
                        if let Some(keys) = &valid_prop_keys {
                            req_arr
                                .retain(|k| k.as_str().map(|s| keys.contains(s)).unwrap_or(false));
                        } else {
                            req_arr.clear();
                        }
                    }
                }

                if !map.contains_key("type") {
                    if map.contains_key("enum") {
                        map.insert("type".to_string(), Value::String("string".to_string()));
                    } else if map.contains_key("properties") {
                        map.insert("type".to_string(), Value::String("object".to_string()));
                    } else if map.contains_key("items") {
                        map.insert("type".to_string(), Value::String("array".to_string()));
                    }
                }

                // Compute fallback type early to avoid borrow conflicts
                let fallback = if map.contains_key("properties") {
                    "object"
                } else if map.contains_key("items") {
                    "array"
                } else {
                    "string"
                };

                // 8. Process type field
                if let Some(type_val) = map.get_mut("type") {
                    let mut selected_type = None;
                    match type_val {
                        Value::String(s) => {
                            let lower = s.to_lowercase();
                            if lower == "null" {
                                is_effectively_nullable = true;
                            } else {
                                selected_type = Some(lower);
                            }
                        }
                        Value::Array(arr) => {
                            for item in arr {
                                if let Value::String(s) = item {
                                    let lower = s.to_lowercase();
                                    if lower == "null" {
                                        is_effectively_nullable = true;
                                    } else if selected_type.is_none() {
                                        selected_type = Some(lower);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }

                    *type_val =
                        Value::String(selected_type.unwrap_or_else(|| fallback.to_string()));
                }

                if is_effectively_nullable {
                    let desc_val = map
                        .entry("description".to_string())
                        .or_insert_with(|| Value::String("".to_string()));
                    if let Value::String(s) = desc_val {
                        if !s.contains("nullable") {
                            if !s.is_empty() {
                                s.push(' ');
                            }
                            s.push_str("(nullable)");
                        }
                    }
                }

                // 9. Force enum values to strings
                if let Some(Value::Array(arr)) = map.get_mut("enum") {
                    for item in arr {
                        if !item.is_string() {
                            *item = Value::String(if item.is_null() {
                                "null".to_string()
                            } else {
                                item.to_string()
                            });
                        }
                    }
                }
            }
        }
        Value::Array(arr) => {
            // Recursively clean each element in the array
            for item in arr.iter_mut() {
                clean_json_schema_recursive(item, is_schema_node);
            }
        }
        _ => {}
    }

    is_effectively_nullable
}
