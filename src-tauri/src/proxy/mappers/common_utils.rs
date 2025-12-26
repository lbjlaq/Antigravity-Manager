// Common utilities for request mapping across all protocols
// Provides unified grounding/networking logic

use serde_json::{json, Value};

/// Request configuration after grounding resolution
#[derive(Debug, Clone)]
pub struct RequestConfig {
    /// The request type: "agent", "web_search", or "image_gen"
    pub request_type: String,
    /// Whether to inject the googleSearch tool
    pub inject_google_search: bool,
    /// The final model name (with suffixes stripped)
    pub final_model: String,
    /// Image generation configuration (if request_type is image_gen)
    pub image_config: Option<Value>,
}

/// Resolve request configuration based on original and mapped model names.
/// 
/// Rules:
/// 1. If model is gemini-3-pro-image*, parse suffixes and set type to image_gen
/// 2. If original model ends with "-online", force web_search
/// 3. If mapped model is in high-quality allowlist (2.5-flash, 1.5-pro), enable web_search
/// 4. Otherwise, default to "agent" type
pub fn resolve_request_config(original_model: &str, mapped_model: &str) -> RequestConfig {
    // 1. Image Generation Check (Priority)
    // Any model starting with gemini-3-pro-image should be mapped to the base model
    // and use "image_gen" request type.
    if mapped_model.starts_with("gemini-3-pro-image") {
        let (image_config, parsed_base_model) = parse_image_config(original_model);
        
        return RequestConfig {
            request_type: "image_gen".to_string(),
            inject_google_search: false,
            // Always use the base model name for upstream
            final_model: parsed_base_model, 
            image_config: Some(image_config),
        };
    }

    // Strip -online suffix from original model if present (to detect networking intent)
    let is_online_suffix = original_model.ends_with("-online");
    
    // The final model to send upstream should be the MAPPED model, 
    // but we strip any legacy suffixes if they leaked into the mapping
    let final_model = mapped_model.trim_end_matches("-online").to_string();

    // High-quality grounding allowlist
    let is_high_quality_model = mapped_model == "gemini-2.5-flash"
        || mapped_model == "gemini-1.5-pro"
        || mapped_model.starts_with("gemini-1.5-pro-")
        || mapped_model.starts_with("gemini-2.5-flash-");

    // Determine if we should enable networking
    let enable_networking = is_online_suffix || is_high_quality_model;

    RequestConfig {
        request_type: if enable_networking {
            "web_search".to_string()
        } else {
            "agent".to_string()
        },
        inject_google_search: enable_networking,
        final_model,
        image_config: None,
    }
}

/// Parse image configuration from model name suffixes
/// Returns (image_config, clean_model_name)
fn parse_image_config(model_name: &str) -> (Value, String) {
    let mut aspect_ratio = "1:1";
    let _image_size = "1024x1024"; // Default, not explicitly sent unless 4k/hd

    if model_name.contains("-16x9") { aspect_ratio = "16:9"; }
    else if model_name.contains("-9x16") { aspect_ratio = "9:16"; }
    else if model_name.contains("-4x3") { aspect_ratio = "4:3"; }
    else if model_name.contains("-3x4") { aspect_ratio = "3:4"; }
    else if model_name.contains("-1x1") { aspect_ratio = "1:1"; }

    let is_hd = model_name.contains("-4k") || model_name.contains("-hd");

    let mut config = serde_json::Map::new();
    config.insert("aspectRatio".to_string(), json!(aspect_ratio));
    
    if is_hd {
        config.insert("imageSize".to_string(), json!("4K"));
    }

    // The upstream model must be EXACTLY "gemini-3-pro-image"
    (serde_json::Value::Object(config), "gemini-3-pro-image".to_string())
}

/// Inject the googleSearch tool into the request body if not already present
pub fn inject_google_search_tool(body: &mut Value) {
    if let Some(obj) = body.as_object_mut() {
        let tools_entry = obj.entry("tools").or_insert_with(|| json!([]));
        if let Some(tools_arr) = tools_entry.as_array_mut() {
            let has_search = tools_arr.iter().any(|t| t.get("googleSearch").is_some());
            if !has_search {
                tools_arr.push(json!({"googleSearch": {}}));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_high_quality_model_auto_grounding() {
        let config = resolve_request_config("gpt-4o", "gemini-2.5-flash");
        assert_eq!(config.request_type, "web_search");
        assert!(config.inject_google_search);
        assert_eq!(config.final_model, "gemini-2.5-flash");
    }

    #[test]
    fn test_online_suffix_force_grounding() {
        let config = resolve_request_config("gemini-3-flash-online", "gemini-3-flash");
        assert_eq!(config.request_type, "web_search");
        assert!(config.inject_google_search);
        assert_eq!(config.final_model, "gemini-3-flash");
    }

    #[test]
    fn test_default_no_grounding() {
        let config = resolve_request_config("claude-sonnet", "gemini-3-flash");
        assert_eq!(config.request_type, "agent");
        assert!(!config.inject_google_search);
    }

    #[test]
    fn test_image_model_excluded() {
        let config = resolve_request_config("gemini-3-pro-image", "gemini-3-pro-image");
        assert_eq!(config.request_type, "image_gen");
        assert!(!config.inject_google_search);
    }
}
