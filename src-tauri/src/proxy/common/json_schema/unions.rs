//! Union type handling for JSON Schema (allOf, anyOf, oneOf).
//!
//! This module provides utilities for merging and simplifying union types
//! to make them compatible with Gemini's stricter schema requirements.

use serde_json::Value;

/// Merge all sub-schemas in an allOf array.
///
/// Combines properties, required fields, and other attributes from all
/// sub-schemas into the parent map.
///
/// # Arguments
/// * `map` - The object map containing the allOf to merge
pub fn merge_all_of(map: &mut serde_json::Map<String, Value>) {
    if let Some(Value::Array(all_of)) = map.remove("allOf") {
        let mut merged_properties = serde_json::Map::new();
        let mut merged_required = std::collections::HashSet::new();
        let mut other_fields = serde_json::Map::new();

        for sub_schema in all_of {
            if let Value::Object(sub_map) = sub_schema {
                // Merge properties
                if let Some(Value::Object(props)) = sub_map.get("properties") {
                    for (k, v) in props {
                        merged_properties.insert(k.clone(), v.clone());
                    }
                }

                // Merge required
                if let Some(Value::Array(reqs)) = sub_map.get("required") {
                    for req in reqs {
                        if let Some(s) = req.as_str() {
                            merged_required.insert(s.to_string());
                        }
                    }
                }

                // Merge other fields (first occurrence wins)
                for (k, v) in sub_map {
                    if k != "properties"
                        && k != "required"
                        && k != "allOf"
                        && !other_fields.contains_key(&k)
                    {
                        other_fields.insert(k, v);
                    }
                }
            }
        }

        // Apply merged fields
        for (k, v) in other_fields {
            if !map.contains_key(&k) {
                map.insert(k, v);
            }
        }

        if !merged_properties.is_empty() {
            let existing_props = map
                .entry("properties".to_string())
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
            if let Value::Object(existing_map) = existing_props {
                for (k, v) in merged_properties {
                    existing_map.entry(k).or_insert(v);
                }
            }
        }

        if !merged_required.is_empty() {
            let existing_reqs = map
                .entry("required".to_string())
                .or_insert_with(|| Value::Array(Vec::new()));
            if let Value::Array(req_arr) = existing_reqs {
                let mut current_reqs: std::collections::HashSet<String> = req_arr
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                for req in merged_required {
                    if current_reqs.insert(req.clone()) {
                        req_arr.push(Value::String(req));
                    }
                }
            }
        }
    }
}

/// Append a hint to the description field.
///
/// Reference: CLIProxyAPI's Lazy Hint strategy.
///
/// # Arguments
/// * `map` - The object map to modify
/// * `hint` - The hint string to append
pub fn append_hint_to_description(map: &mut serde_json::Map<String, Value>, hint: String) {
    let desc_val = map
        .entry("description".to_string())
        .or_insert_with(|| Value::String("".to_string()));

    if let Value::String(s) = desc_val {
        if s.is_empty() {
            *s = hint;
        } else if !s.contains(&hint) {
            *s = format!("{} {}", s, hint);
        }
    }
}

/// Calculate complexity score for a schema branch (for anyOf/oneOf selection).
///
/// Scoring: Object (3) > Array (2) > Scalar (1) > Null (0)
///
/// # Arguments
/// * `val` - The schema value to score
///
/// # Returns
/// The complexity score
pub fn score_schema_option(val: &Value) -> i32 {
    if let Value::Object(obj) = val {
        if obj.contains_key("properties")
            || obj.get("type").and_then(|t| t.as_str()) == Some("object")
        {
            return 3;
        }
        if obj.contains_key("items") || obj.get("type").and_then(|t| t.as_str()) == Some("array") {
            return 2;
        }
        if let Some(type_str) = obj.get("type").and_then(|t| t.as_str()) {
            if type_str != "null" {
                return 1;
            }
        }
    }
    0
}

/// Get the type name from a schema.
///
/// # Arguments
/// * `schema` - The schema to extract type from
///
/// # Returns
/// The type name if determinable
pub fn get_schema_type_name(schema: &Value) -> Option<String> {
    if let Value::Object(obj) = schema {
        // Prefer explicit type field
        if let Some(type_val) = obj.get("type") {
            if let Some(s) = type_val.as_str() {
                return Some(s.to_string());
            }
        }

        // Infer type from structure
        if obj.contains_key("properties") {
            return Some("object".to_string());
        }
        if obj.contains_key("items") {
            return Some("array".to_string());
        }
    }

    None
}

/// Extract the best non-null schema branch from an anyOf/oneOf union.
///
/// Returns: (best_schema, all_possible_types)
/// Reference: CLIProxyAPI's selectBest logic.
///
/// # Arguments
/// * `union_array` - The array of union options
///
/// # Returns
/// The best schema and list of all possible types
pub fn extract_best_schema_from_union(union_array: &Vec<Value>) -> Option<(Value, Vec<String>)> {
    let mut best_option: Option<&Value> = None;
    let mut best_score = -1;
    let mut all_types = Vec::new();

    for item in union_array {
        let score = score_schema_option(item);

        // Collect type information
        if let Some(type_str) = get_schema_type_name(item) {
            if !all_types.contains(&type_str) {
                all_types.push(type_str);
            }
        }

        if score > best_score {
            best_score = score;
            best_option = Some(item);
        }
    }

    best_option.cloned().map(|schema| (schema, all_types))
}
