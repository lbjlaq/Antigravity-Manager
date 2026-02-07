//! Function call argument remapping for Gemini → Claude compatibility.
//!
//! Gemini sometimes uses different parameter names than specified in tool schemas.
//! This module provides remapping logic to fix common hallucinations.

use serde_json::{json, Value};

/// Known parameter remappings for Gemini → Claude compatibility.
///
/// Gemini sometimes uses different parameter names than specified in tool schema.
/// This function remaps common mismatches to ensure tool calls work correctly.
pub fn remap_function_call_args(name: &str, args: &mut Value) {
    // Debug log incoming tool usage
    if let Some(obj) = args.as_object() {
        tracing::debug!("[Streaming] Tool Call: '{}' Args: {:?}", name, obj);
    }

    // Claude Code CLI's EnterPlanMode tool must not have any parameters
    // Proxy-injected reason parameter causes InputValidationError
    if name == "EnterPlanMode" {
        if let Some(obj) = args.as_object_mut() {
            obj.clear();
        }
        return;
    }

    if let Some(obj) = args.as_object_mut() {
        // Case-insensitive matching for tool names
        match name.to_lowercase().as_str() {
            "grep" | "search" | "search_code_definitions" | "search_code_snippets" => {
                remap_grep_args(obj);
            }
            "glob" => {
                remap_glob_args(obj);
            }
            "read" => {
                remap_read_args(obj);
            }
            "ls" => {
                remap_ls_args(obj);
            }
            other => {
                remap_generic_args(other, obj);
            }
        }
    }
}

/// Remap arguments for Grep/Search tools.
fn remap_grep_args(obj: &mut serde_json::Map<String, Value>) {
    // Gemini hallucination: maps parameter description to "description" field
    if let Some(desc) = obj.remove("description") {
        if !obj.contains_key("pattern") {
            obj.insert("pattern".to_string(), desc);
            tracing::debug!("[Streaming] Remapped Grep: description → pattern");
        }
    }

    // Gemini uses "query", Claude Code expects "pattern"
    if let Some(query) = obj.remove("query") {
        if !obj.contains_key("pattern") {
            obj.insert("pattern".to_string(), query);
            tracing::debug!("[Streaming] Remapped Grep: query → pattern");
        }
    }

    // Claude Code uses "path" (string), NOT "paths" (array)!
    remap_paths_to_path(obj, "Grep");
}

/// Remap arguments for Glob tool.
fn remap_glob_args(obj: &mut serde_json::Map<String, Value>) {
    // Gemini hallucination: maps parameter description to "description" field
    if let Some(desc) = obj.remove("description") {
        if !obj.contains_key("pattern") {
            obj.insert("pattern".to_string(), desc);
            tracing::debug!("[Streaming] Remapped Glob: description → pattern");
        }
    }

    // Gemini uses "query", Claude Code expects "pattern"
    if let Some(query) = obj.remove("query") {
        if !obj.contains_key("pattern") {
            obj.insert("pattern".to_string(), query);
            tracing::debug!("[Streaming] Remapped Glob: query → pattern");
        }
    }

    // Claude Code uses "path" (string), NOT "paths" (array)!
    remap_paths_to_path(obj, "Glob");
}

/// Remap arguments for Read tool.
fn remap_read_args(obj: &mut serde_json::Map<String, Value>) {
    // Gemini might use "path" vs "file_path"
    if let Some(path) = obj.remove("path") {
        if !obj.contains_key("file_path") {
            obj.insert("file_path".to_string(), path);
            tracing::debug!("[Streaming] Remapped Read: path → file_path");
        }
    }
}

/// Remap arguments for LS tool.
fn remap_ls_args(obj: &mut serde_json::Map<String, Value>) {
    // LS tool: ensure "path" parameter exists
    if !obj.contains_key("path") {
        obj.insert("path".to_string(), json!("."));
        tracing::debug!("[Streaming] Remapped LS: default path → \".\"");
    }
}

/// Generic argument remapping for unknown tools.
fn remap_generic_args(tool_name: &str, obj: &mut serde_json::Map<String, Value>) {
    // [Issue #785] Generic Property Mapping for all tools
    // If a tool has "paths" (array of 1) but no "path", convert it.
    let mut path_to_inject = None;
    if !obj.contains_key("path") {
        if let Some(paths) = obj.get("paths").and_then(|v| v.as_array()) {
            if paths.len() == 1 {
                if let Some(p) = paths[0].as_str() {
                    path_to_inject = Some(p.to_string());
                }
            }
        }
    }

    if let Some(path) = path_to_inject {
        obj.insert("path".to_string(), json!(path));
        tracing::debug!(
            "[Streaming] Probabilistic fix for tool '{}': paths[0] → path(\"{}\")",
            tool_name,
            path
        );
    }
    tracing::debug!(
        "[Streaming] Unmapped tool call processed via generic rules: {} (keys: {:?})",
        tool_name,
        obj.keys()
    );
}

/// Helper: Convert "paths" array to single "path" string.
fn remap_paths_to_path(obj: &mut serde_json::Map<String, Value>, tool_name: &str) {
    if !obj.contains_key("path") {
        if let Some(paths) = obj.remove("paths") {
            let path_str = if let Some(arr) = paths.as_array() {
                arr.get(0)
                    .and_then(|v| v.as_str())
                    .unwrap_or(".")
                    .to_string()
            } else if let Some(s) = paths.as_str() {
                s.to_string()
            } else {
                ".".to_string()
            };
            obj.insert("path".to_string(), serde_json::json!(path_str));
            tracing::debug!(
                "[Streaming] Remapped {}: paths → path(\"{}\")",
                tool_name,
                path_str
            );
        } else {
            // Default to current directory if missing
            obj.insert("path".to_string(), json!("."));
            tracing::debug!("[Streaming] Added default path: \".\"");
        }
    }
}
