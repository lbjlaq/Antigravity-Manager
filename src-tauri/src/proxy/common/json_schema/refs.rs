//! $ref and $defs flattening logic for JSON Schema.
//!
//! This module handles the expansion of JSON Schema references ($ref)
//! by replacing them with their actual definitions from $defs/definitions.

use serde_json::Value;

/// Recursively collect all $defs and definitions from all levels.
///
/// MCP tool schemas may define $defs at any nested level, not just root.
/// This function traverses the entire schema and collects all definitions
/// into a unified map.
///
/// # Arguments
/// * `value` - The schema value to traverse
/// * `defs` - The map to collect definitions into
pub fn collect_all_defs(value: &Value, defs: &mut serde_json::Map<String, Value>) {
    if let Value::Object(map) = value {
        // Collect $defs at current level
        if let Some(Value::Object(d)) = map.get("$defs") {
            for (k, v) in d {
                // Avoid overwriting existing definitions (first defined wins)
                defs.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        // Collect definitions (Draft-07 style)
        if let Some(Value::Object(d)) = map.get("definitions") {
            for (k, v) in d {
                defs.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        // Recursively process all child nodes
        for (key, v) in map {
            // Skip $defs/definitions themselves to avoid reprocessing
            if key != "$defs" && key != "definitions" {
                collect_all_defs(v, defs);
            }
        }
    } else if let Value::Array(arr) = value {
        for item in arr {
            collect_all_defs(item, defs);
        }
    }
}

/// Recursively expand $ref references.
///
/// Replaces each $ref with the content from the corresponding definition.
/// If a $ref cannot be resolved, it falls back to a permissive string type
/// to avoid API 400 errors.
///
/// # Arguments
/// * `map` - The object map to process
/// * `defs` - The collected definitions to resolve references from
pub fn flatten_refs(map: &mut serde_json::Map<String, Value>, defs: &serde_json::Map<String, Value>) {
    // Check and replace $ref
    if let Some(Value::String(ref_path)) = map.remove("$ref") {
        // Parse reference name (e.g., #/$defs/MyType -> MyType)
        let ref_name = ref_path.split('/').last().unwrap_or(&ref_path);

        if let Some(def_schema) = defs.get(ref_name) {
            // Merge definition content into current map
            if let Value::Object(def_map) = def_schema {
                for (k, v) in def_map {
                    // Only insert if current map doesn't have the key (avoid override)
                    map.entry(k.clone()).or_insert_with(|| v.clone());
                }

                // Recursively process content just merged (may contain more $refs)
                // Note: May infinite loop on circular references, but tool defs are usually DAGs
                flatten_refs(map, defs);
            }
        } else {
            // [FIX #952] Unresolvable $ref: convert to permissive string type
            // This is better than failing the request - at least tool calls can proceed
            map.insert("type".to_string(), serde_json::json!("string"));
            let hint = format!("(Unresolved $ref: {})", ref_path);
            let desc_val = map
                .entry("description".to_string())
                .or_insert_with(|| Value::String(String::new()));
            if let Value::String(s) = desc_val {
                if !s.contains(&hint) {
                    if !s.is_empty() {
                        s.push(' ');
                    }
                    s.push_str(&hint);
                }
            }
        }
    }

    // Traverse child nodes
    for (_, v) in map.iter_mut() {
        if let Value::Object(child_map) = v {
            flatten_refs(child_map, defs);
        } else if let Value::Array(arr) = v {
            for item in arr {
                if let Value::Object(item_map) = item {
                    flatten_refs(item_map, defs);
                }
            }
        }
    }
}
