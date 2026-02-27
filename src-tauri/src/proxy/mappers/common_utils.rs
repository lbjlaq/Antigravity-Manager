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

pub fn resolve_request_config(
    original_model: &str,
    mapped_model: &str,
    tools: &Option<Vec<Value>>,
    size: Option<&str>,    // [NEW] Image size parameter
    quality: Option<&str>, // [NEW] Image quality parameter
    image_size: Option<&str>, // [NEW] Direct imageSize parameter (e.g. "4K")
    body: Option<&Value>,  // [NEW] Request body for Gemini native imageConfig
) -> RequestConfig {
    // 1. Image Generation Check (Priority)
    if mapped_model.starts_with("gemini-3-pro-image") {
        // [RESOLVE #1694] Improved priority logic:
        // 1. First parse inferred config from model suffix and OpenAI parameters
        let (mut inferred_config, parsed_base_model) =
            parse_image_config_with_params(original_model, size, quality, image_size);

        // 2. Then merge with imageConfig from Gemini request body (if exists)
        if let Some(body_val) = body {
            if let Some(gen_config) = body_val.get("generationConfig") {
                if let Some(body_image_config) = gen_config.get("imageConfig") {
                    tracing::info!(
                        "[Common-Utils] Found imageConfig in body, merging with inferred config from suffix/params"
                    );
                    
                    if let Some(inferred_obj) = inferred_config.as_object_mut() {
                        if let Some(body_obj) = body_image_config.as_object() {
                            // Merge body_obj into inferred_obj
                            for (key, value) in body_obj {
                                // CRITICAL: Only allow body to override if inferred doesn't already have a high-priority value
                                // Specifically, if we inferred imageSize from -4k, don't let body downgrade it if it's missing or standard.
                                let is_size_downgrade = key == "imageSize" && 
                                    (value.as_str() == Some("1K") || value.is_null()) &&
                                    inferred_obj.contains_key("imageSize");

                                if !is_size_downgrade {
                                    inferred_obj.insert(key.clone(), value.clone());
                                } else {
                                    tracing::debug!("[Common-Utils] Shielding inferred imageSize from body downgrade");
                                }
                            }
                        }
                    }
                }
            }
        }

        tracing::info!(
            "[Common-Utils] Final Image Config for {}: {:?}",
            parsed_base_model, inferred_config
        );

        return RequestConfig {
            request_type: "image_gen".to_string(),
            inject_google_search: false,
            final_model: parsed_base_model,
            image_config: Some(inferred_config),
        };
    }

    // 检测是否有联网工具定义 (内置功能调用)
    let has_networking_tool = detects_networking_tool(tools);
    // 检测是否包含非联网工具 (如 MCP 本地工具)
    let _has_non_networking = contains_non_networking_tool(tools);

    // Strip -online suffix from original model if present (to detect networking intent)
    let is_online_suffix = original_model.ends_with("-online");

    // High-quality grounding allowlist (Only for models known to support search and be relatively 'safe')
    let _is_high_quality_model = mapped_model == "gemini-2.5-flash"
        || mapped_model == "gemini-1.5-pro"
        || mapped_model.starts_with("gemini-1.5-pro-")
        || mapped_model.starts_with("gemini-2.5-flash-")
        || mapped_model.starts_with("gemini-2.0-flash")
        || mapped_model.starts_with("gemini-3-")
        || mapped_model.starts_with("gemini-3.")
        || mapped_model.contains("claude-3-5-sonnet")
        || mapped_model.contains("claude-3-opus")
        || mapped_model.contains("claude-sonnet")
        || mapped_model.contains("claude-opus")
        || mapped_model.contains("claude-4");

    // Determine if we should enable networking
    // [FIX] 禁用基于模型的自动联网逻辑，防止图像请求被联网搜索结果覆盖。
    // 仅在用户显式请求联网时启用：1) -online 后缀 2) 携带联网工具定义
    let enable_networking = is_online_suffix || has_networking_tool;

    // The final model to send upstream should be the MAPPED model,
    // but if searching, we MUST ensure the model name is one the backend associates with search.
    // Force a stable search model for search requests.
    let mut final_model = mapped_model.trim_end_matches("-online").to_string();

    // [FIX] Map logic aliases back to physical model names for upstream compatibility
    final_model = match final_model.as_str() {
        "gemini-3-pro-preview" => "gemini-3.1-pro-high".to_string(), // 3.0 preview redirects to 3.1 High
        "gemini-3.1-pro-preview" => "gemini-3.1-pro-high".to_string(),
        "gemini-3-pro-image-preview" => "gemini-3-pro-image".to_string(),
        "gemini-3-flash-preview" => "gemini-3-flash".to_string(),
        _ => final_model,
    };

    // [FIX] Check allowlist before forcing downgrade
    // If networking is enabled but the model doesn't support search, fall back to Flash
    if enable_networking && !_is_high_quality_model {
        tracing::info!(
            "[Common-Utils] Downgrading {} to gemini-2.5-flash for web search (model not in search allowlist)",
            final_model
        );
        final_model = "gemini-2.5-flash".to_string();
    }

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

/// Legacy wrapper for backward compatibility and simple usage
#[allow(dead_code)]
pub fn parse_image_config(model_name: &str) -> (Value, String) {
    parse_image_config_with_params(model_name, None, None, None)
}

/// Extended version that accepts OpenAI size and quality parameters
///
/// This function supports parsing image configuration from:
/// 1. Direct imageSize parameter - takes highest priority
/// 2. OpenAI API parameters (size, quality) - medium priority
/// 3. Model name suffixes (e.g., -16x9, -4k) - fallback
///
/// # Arguments
/// * `model_name` - The model name (may contain suffixes like -16x9-4k)
/// * `size` - Optional OpenAI size parameter (e.g., "1280x720", "1792x1024")
/// * `quality` - Optional OpenAI quality parameter ("standard", "hd", "medium")
/// * `image_size` - Optional direct Gemini imageSize parameter ("2K", "4K")
///
/// # Returns
/// (image_config, clean_model_name) where image_config contains aspectRatio and optionally imageSize
pub fn parse_image_config_with_params(
    model_name: &str,
    size: Option<&str>,
    quality: Option<&str>,
    image_size: Option<&str>,
) -> (Value, String) {
    let mut aspect_ratio = "1:1";

    // 1. 优先从 size 参数解析宽高比
    if let Some(s) = size {
        aspect_ratio = calculate_aspect_ratio_from_size(s);
    } else {
        // 2. 回退到模型后缀解析（保持向后兼容）
        if model_name.contains("-21x9") || model_name.contains("-21-9") {
            aspect_ratio = "21:9";
        } else if model_name.contains("-16x9") || model_name.contains("-16-9") {
            aspect_ratio = "16:9";
        } else if model_name.contains("-9x16") || model_name.contains("-9-16") {
            aspect_ratio = "9:16";
        } else if model_name.contains("-4x3") || model_name.contains("-4-3") {
            aspect_ratio = "4:3";
        } else if model_name.contains("-3x4") || model_name.contains("-3-4") {
            aspect_ratio = "3:4";
        } else if model_name.contains("-3x2") || model_name.contains("-3-2") {
            aspect_ratio = "3:2";
        } else if model_name.contains("-2x3") || model_name.contains("-2-3") {
            aspect_ratio = "2:3";
        } else if model_name.contains("-5x4") || model_name.contains("-5-4") {
            aspect_ratio = "5:4";
        } else if model_name.contains("-4x5") || model_name.contains("-4-5") {
            aspect_ratio = "4:5";
        } else if model_name.contains("-1x1") || model_name.contains("-1-1") {
            aspect_ratio = "1:1";
        }
    }

    let mut config = serde_json::Map::new();
    config.insert("aspectRatio".to_string(), json!(aspect_ratio));

    // [NEW] 0. 最高优先级：直接使用 image_size 参数
    if let Some(is) = image_size {
        config.insert("imageSize".to_string(), json!(is.to_uppercase()));
    } else {
        // 3. 优先从 quality 参数解析分辨率
        if let Some(q) = quality {
            match q.to_lowercase().as_str() {
                "hd" | "4k" => {
                    config.insert("imageSize".to_string(), json!("4K"));
                }
                "medium" | "2k" => {
                    config.insert("imageSize".to_string(), json!("2K"));
                }
                "standard" | "1k" => {
                    config.insert("imageSize".to_string(), json!("1K"));
                }
                _ => {} // 其他值不设置，使用默认
            }
        } else {
            // 4. 回退到模型后缀解析（保持向后兼容）
            let is_hd = model_name.contains("-4k") || model_name.contains("-hd");
            let is_2k = model_name.contains("-2k");

            if is_hd {
                config.insert("imageSize".to_string(), json!("4K"));
            } else if is_2k {
                config.insert("imageSize".to_string(), json!("2K"));
            }
        }
    }

    let clean_model_name = clean_image_model_name(model_name);

    (
        serde_json::Value::Object(config),
        clean_model_name,
    )
}

/// Helper function to clean image model names by removing resolution/aspect-ratio suffixes.
/// E.g., "gemini-3.1-flash-image-16x9-4k" -> "gemini-3.1-flash-image"
fn clean_image_model_name(model_name: &str) -> String {
    let mut clean_name = model_name.to_lowercase();
    
    // Ordered list of known suffixes to strip
    let suffixes = [
        "-4k", "-2k", "-1k", "-hd", "-standard", "-medium",
        "-21x9", "-21-9", "-16x9", "-16-9", "-9x16", "-9-16",
        "-4x3", "-4-3", "-3x4", "-3-4", "-3x2", "-3-2",
        "-2x3", "-2-3", "-5x4", "-5-4", "-4x5", "-4-5",
        "-1x1", "-1-1"
    ];

    // Repeatedly strip suffixes until no more are found
    let mut changed = true;
    while changed {
        changed = false;
        for suffix in &suffixes {
            if clean_name.ends_with(suffix) {
                clean_name.truncate(clean_name.len() - suffix.len());
                changed = true;
            }
        }
    }

    clean_name
}

/// 动态计算宽高比（解决硬编码问题）
///
/// 从 "WIDTHxHEIGHT" 格式的字符串解析并计算宽高比，
/// 使用容差匹配常见的标准比例。
///
/// # Arguments
/// * `size` - 尺寸字符串，格式为 "WIDTHxHEIGHT" (e.g., "1280x720", "1792x1024")
///
/// # Returns
/// 标准宽高比字符串 ("1:1", "16:9", "9:16", "4:3", "3:4", "21:9")
fn calculate_aspect_ratio_from_size(size: &str) -> &'static str {
    // 0. Explicitly check known aspect ratios first
    match size {
        "21:9" => return "21:9",
        "16:9" => return "16:9",
        "9:16" => return "9:16",
        "4:3" => return "4:3",
        "3:4" => return "3:4",
        "3:2" => return "3:2",
        "2:3" => return "2:3",
        "5:4" => return "5:4",
        "4:5" => return "4:5",
        "1:1" => return "1:1",
        _ => {}
    }

    if let Some((w_str, h_str)) = size.split_once('x') {
        if let (Ok(width), Ok(height)) = (w_str.parse::<f64>(), h_str.parse::<f64>()) {
            if width > 0.0 && height > 0.0 {
                let ratio = width / height;

                // 容差匹配常见比例（容差 0.05，避免 3:4 和 2:3 重叠）
                if (ratio - 21.0 / 9.0).abs() < 0.05 {
                    return "21:9";
                }
                if (ratio - 16.0 / 9.0).abs() < 0.05 {
                    return "16:9";
                }
                if (ratio - 4.0 / 3.0).abs() < 0.05 {
                    return "4:3";
                }
                if (ratio - 3.0 / 4.0).abs() < 0.05 {
                    return "3:4";
                }
                if (ratio - 9.0 / 16.0).abs() < 0.05 {
                    return "9:16";
                }
                if (ratio - 3.0 / 2.0).abs() < 0.05 {
                    return "3:2";
                }
                if (ratio - 2.0 / 3.0).abs() < 0.05 {
                    return "2:3";
                }
                if (ratio - 5.0 / 4.0).abs() < 0.05 {
                    return "5:4";
                }
                if (ratio - 4.0 / 5.0).abs() < 0.05 {
                    return "4:5";
                }
                if (ratio - 1.0).abs() < 0.05 {
                    return "1:1";
                }
            }
        }
    }

    "1:1" // 默认回退
}

/// Inject current googleSearch tool and ensure no duplicate legacy search tools
pub fn inject_google_search_tool(body: &mut Value) {
    if let Some(obj) = body.as_object_mut() {
        let tools_entry = obj.entry("tools").or_insert_with(|| json!([]));
        if let Some(tools_arr) = tools_entry.as_array_mut() {
            // [安全校验] 如果数组中已经包含 functionDeclarations，严禁注入 googleSearch
            // 因为 Gemini v1internal 不支持在一次请求中混用 search 和 functions
            let has_functions = tools_arr.iter().any(|t| {
                t.as_object()
                    .map_or(false, |o| o.contains_key("functionDeclarations"))
            });

            if has_functions {
                tracing::debug!(
                    "Skipping googleSearch injection due to existing functionDeclarations"
                );
                return;
            }

            // 首先清理掉已存在的 googleSearch 或 googleSearchRetrieval，以防重复产生冲突
            tools_arr.retain(|t| {
                if let Some(o) = t.as_object() {
                    !(o.contains_key("googleSearch") || o.contains_key("googleSearchRetrieval"))
                } else {
                    true
                }
            });

            // 注入统一的 googleSearch (v1internal 规范)
            tools_arr.push(json!({
                "googleSearch": {}
            }));
        }
    }
}

/// 深度迭代清理客户端发送的 [undefined] 脏字符串，防止 Gemini 接口校验失败
pub fn deep_clean_undefined(value: &mut Value, depth: usize) {
    if depth > 10 {
        return;
    }
    match value {
        Value::Object(map) => {
            // 移除值为 "[undefined]" 的键
            map.retain(|_, v| {
                if let Some(s) = v.as_str() {
                    s != "[undefined]"
                } else {
                    true
                }
            });
            // 递归处理嵌套
            for v in map.values_mut() {
                deep_clean_undefined(v, depth + 1);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                deep_clean_undefined(v, depth + 1);
            }
        }
        _ => {}
    }
}

/// Detects if the tool list contains a request for networking/web search.
/// Supported keywords: "web_search", "google_search", "web_search_20250305"
pub fn detects_networking_tool(tools: &Option<Vec<Value>>) -> bool {
    if let Some(list) = tools {
        for tool in list {
            // 1. 直发风格 (Claude/Simple OpenAI/Anthropic Builtin/Vertex): { "name": "..." } 或 { "type": "..." }
            if let Some(n) = tool.get("name").and_then(|v| v.as_str()) {
                if n == "web_search"
                    || n == "google_search"
                    || n == "web_search_20250305"
                    || n == "google_search_retrieval"
                    || n == "builtin_web_search"
                {
                    return true;
                }
            }

            if let Some(t) = tool.get("type").and_then(|v| v.as_str()) {
                if t == "web_search_20250305"
                    || t == "google_search"
                    || t == "web_search"
                    || t == "google_search_retrieval"
                    || t == "builtin_web_search"
                {
                    return true;
                }
            }

            // 2. OpenAI 嵌套风格: { "type": "function", "function": { "name": "..." } }
            if let Some(func) = tool.get("function") {
                if let Some(n) = func.get("name").and_then(|v| v.as_str()) {
                    let keywords = [
                        "web_search",
                        "google_search",
                        "web_search_20250305",
                        "google_search_retrieval",
                        "builtin_web_search",
                    ];
                    if keywords.contains(&n) {
                        return true;
                    }
                }
            }

            // 3. Gemini 原生风格: { "functionDeclarations": [ { "name": "..." } ] }
            if let Some(decls) = tool.get("functionDeclarations").and_then(|v| v.as_array()) {
                for decl in decls {
                    if let Some(n) = decl.get("name").and_then(|v| v.as_str()) {
                        if n == "web_search"
                            || n == "google_search"
                            || n == "google_search_retrieval"
                            || n == "builtin_web_search"
                        {
                            return true;
                        }
                    }
                }
            }

            // 4. Gemini googleSearch 声明 (含 googleSearchRetrieval 变体)
            if tool.get("googleSearch").is_some() || tool.get("googleSearchRetrieval").is_some() {
                return true;
            }
        }
    }
    false
}

/// 探测是否包含非联网相关的本地函数工具
pub fn contains_non_networking_tool(tools: &Option<Vec<Value>>) -> bool {
    if let Some(list) = tools {
        for tool in list {
            let mut is_networking = false;

            // 简单逻辑：如果它是一个函数声明且名字不是联网关键词，则视为非联网工具
            if let Some(n) = tool.get("name").and_then(|v| v.as_str()) {
                let keywords = [
                    "web_search",
                    "google_search",
                    "web_search_20250305",
                    "google_search_retrieval",
                    "builtin_web_search",
                ];
                if keywords.contains(&n) {
                    is_networking = true;
                }
            } else if let Some(func) = tool.get("function") {
                if let Some(n) = func.get("name").and_then(|v| v.as_str()) {
                    let keywords = [
                        "web_search",
                        "google_search",
                        "web_search_20250305",
                        "google_search_retrieval",
                        "builtin_web_search",
                    ];
                    if keywords.contains(&n) {
                        is_networking = true;
                    }
                }
            } else if tool.get("googleSearch").is_some()
                || tool.get("googleSearchRetrieval").is_some()
            {
                is_networking = true;
            } else if tool.get("functionDeclarations").is_some() {
                // 如果是 Gemini 风格的 functionDeclarations，进去看一眼
                if let Some(decls) = tool.get("functionDeclarations").and_then(|v| v.as_array()) {
                    for decl in decls {
                        if let Some(n) = decl.get("name").and_then(|v| v.as_str()) {
                            let keywords =
                                ["web_search", "google_search", "google_search_retrieval", "builtin_web_search"];
                            if !keywords.contains(&n) {
                                return true; // 发现本地函数
                            }
                        }
                    }
                }
                is_networking = true; // 即使全是联网，外层也标记为联网
            }

            if !is_networking {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_high_quality_model_auto_grounding() {
        // Auto-grounding is currently disabled by default due to conflict with image gen
        let config = resolve_request_config("gpt-4o", "gemini-2.5-flash", &None, None, None, None, None);
        assert_eq!(config.request_type, "agent");
        assert!(!config.inject_google_search);
    }

    #[test]
    fn test_gemini_native_tool_detection() {
        let tools = Some(vec![json!({
            "functionDeclarations": [
                { "name": "web_search", "parameters": {} }
            ]
        })]);
        assert!(detects_networking_tool(&tools));
    }

    #[test]
    fn test_online_suffix_force_grounding() {
        let config =
            resolve_request_config("gemini-3-flash-online", "gemini-3-flash", &None, None, None, None, None);
        assert_eq!(config.request_type, "web_search");
        assert!(config.inject_google_search);
        assert_eq!(config.final_model, "gemini-2.5-flash");
    }

    #[test]
    fn test_default_no_grounding() {
        let config = resolve_request_config("claude-sonnet", "gemini-3-flash", &None, None, None, None, None);
        assert_eq!(config.request_type, "agent");
        assert!(!config.inject_google_search);
    }

    #[test]
    fn test_image_model_excluded() {
        let config = resolve_request_config(
            "gemini-3-pro-image",
            "gemini-3-pro-image",
            &None,
            None,
            None,
            None,
            None,
        );
        assert_eq!(config.request_type, "image_gen");
        assert!(!config.inject_google_search);
    }

    #[test]
    fn test_image_2k_and_ultrawide_config() {
        // Test 2K
        let (config_2k, _) = parse_image_config("gemini-3-pro-image-2k");
        assert_eq!(config_2k["imageSize"], "2K");

        // Test 21:9
        let (config_21x9, _) = parse_image_config("gemini-3-pro-image-21x9");
        assert_eq!(config_21x9["aspectRatio"], "21:9");

        // Test Combined (if logic allows, though suffix parsing is greedy)
        let (config_combined, _) = parse_image_config("gemini-3-pro-image-2k-21x9");
        assert_eq!(config_combined["imageSize"], "2K");
        assert_eq!(config_combined["aspectRatio"], "21:9");

        // Test 4K + 21:9
        let (config_4k_wide, _) = parse_image_config("gemini-3-pro-image-4k-21x9");
        assert_eq!(config_4k_wide["imageSize"], "4K");
        assert_eq!(config_4k_wide["aspectRatio"], "21:9");
    }

    #[test]
    fn test_parse_image_config_with_openai_params() {
        // Test quality parameter mapping
        let (config_hd, model_hd) = parse_image_config_with_params("gemini-3-pro-image", None, Some("hd"), None);
        assert_eq!(config_hd["imageSize"], "4K");
        assert_eq!(config_hd["aspectRatio"], "1:1");
        assert_eq!(model_hd, "gemini-3-pro-image");

        let (config_medium, model_medium) =
            parse_image_config_with_params("gemini-3-pro-image", None, Some("medium"), None);
        assert_eq!(config_medium["imageSize"], "2K");
        assert_eq!(model_medium, "gemini-3-pro-image");

        let (config_standard, model_standard) =
            parse_image_config_with_params("gemini-3-pro-image", None, Some("standard"), None);
        assert_eq!(config_standard["imageSize"], "1K");
        assert_eq!(model_standard, "gemini-3-pro-image");

        // Test size parameter mapping with dynamic calculation
        let (config_16_9, model_16_9) =
            parse_image_config_with_params("gemini-3-pro-image", Some("1280x720"), None, None);
        assert_eq!(config_16_9["aspectRatio"], "16:9");
        assert_eq!(model_16_9, "gemini-3-pro-image");

        let (config_9_16, model_9_16) =
            parse_image_config_with_params("gemini-3-pro-image", Some("720x1280"), None, None);
        assert_eq!(config_9_16["aspectRatio"], "9:16");
        assert_eq!(model_9_16, "gemini-3-pro-image");

        let (config_4_3, model_4_3) =
            parse_image_config_with_params("gemini-3-pro-image", Some("800x600"), None, None);
        assert_eq!(config_4_3["aspectRatio"], "4:3");
        assert_eq!(model_4_3, "gemini-3-pro-image");

        // Test combined size + quality
        let (config_combined, model_combined) =
            parse_image_config_with_params("gemini-3-pro-image", Some("1920x1080"), Some("hd"), None);
        assert_eq!(config_combined["aspectRatio"], "16:9");
        assert_eq!(config_combined["imageSize"], "4K");
        assert_eq!(model_combined, "gemini-3-pro-image");

        // Test backward compatibility: model suffix takes precedence when no params
        let (config_compat, model_compat) =
            parse_image_config_with_params("gemini-3-pro-image-16x9-4k", None, None, None);
        assert_eq!(config_compat["aspectRatio"], "16:9");
        assert_eq!(config_compat["imageSize"], "4K");
        assert_eq!(model_compat, "gemini-3-pro-image");

        // Test parameter priority: params override model suffix
        let (config_override, model_override) = parse_image_config_with_params(
            "gemini-3-pro-image-1x1-2k",
            Some("1280x720"),
            Some("hd"),
            None,
        );
        assert_eq!(config_override["aspectRatio"], "16:9"); // from size param, not model suffix
        assert_eq!(config_override["imageSize"], "4K"); // from quality param, not model suffix
        assert_eq!(model_override, "gemini-3-pro-image");
    }

    #[test]
    fn test_clean_image_model_name() {
        assert_eq!(clean_image_model_name("gemini-3.1-flash-image"), "gemini-3.1-flash-image");
        assert_eq!(clean_image_model_name("gemini-3.1-flash-image-4k"), "gemini-3.1-flash-image");
        assert_eq!(clean_image_model_name("gemini-3-pro-image-16x9"), "gemini-3-pro-image");
        assert_eq!(clean_image_model_name("gemini-3-pro-image-16x9-4k"), "gemini-3-pro-image");
        // Test varying order
        assert_eq!(clean_image_model_name("gemini-3.1-flash-image-4k-16x9"), "gemini-3.1-flash-image");
        assert_eq!(clean_image_model_name("gemini-3.1-flash-image-16-9-hd"), "gemini-3.1-flash-image");
        assert_eq!(clean_image_model_name("gemini-3.1-flash-image-2k-9x16"), "gemini-3.1-flash-image");
        assert_eq!(clean_image_model_name("gemini-3.1-flash-image-1x1"), "gemini-3.1-flash-image");
        assert_eq!(clean_image_model_name("gemini-3.1-flash-image-standard"), "gemini-3.1-flash-image");
        assert_eq!(clean_image_model_name("gemini-3.1-flash-image-medium"), "gemini-3.1-flash-image");
        assert_eq!(clean_image_model_name("gemini-3.1-flash-image-21-9-4k"), "gemini-3.1-flash-image");
    }

    #[test]
    fn test_calculate_aspect_ratio_from_size() {
        // Test standard OpenAI sizes
        assert_eq!(calculate_aspect_ratio_from_size("1280x720"), "16:9");
        assert_eq!(calculate_aspect_ratio_from_size("1920x1080"), "16:9");
        assert_eq!(calculate_aspect_ratio_from_size("720x1280"), "9:16");
        assert_eq!(calculate_aspect_ratio_from_size("1080x1920"), "9:16");
        assert_eq!(calculate_aspect_ratio_from_size("1024x1024"), "1:1");
        assert_eq!(calculate_aspect_ratio_from_size("800x600"), "4:3");
        assert_eq!(calculate_aspect_ratio_from_size("600x800"), "3:4");
        assert_eq!(calculate_aspect_ratio_from_size("2560x1080"), "21:9");

        // [NEW] Test new aspect ratios
        assert_eq!(calculate_aspect_ratio_from_size("1500x1000"), "3:2");
        assert_eq!(calculate_aspect_ratio_from_size("1000x1500"), "2:3");
        assert_eq!(calculate_aspect_ratio_from_size("1250x1000"), "5:4");
        assert_eq!(calculate_aspect_ratio_from_size("1000x1250"), "4:5");

        // [NEW] Test direct aspect ratio strings
        assert_eq!(calculate_aspect_ratio_from_size("21:9"), "21:9");
        assert_eq!(calculate_aspect_ratio_from_size("16:9"), "16:9");
        assert_eq!(calculate_aspect_ratio_from_size("1:1"), "1:1");

        // Test edge cases
        assert_eq!(calculate_aspect_ratio_from_size("invalid"), "1:1");
        assert_eq!(calculate_aspect_ratio_from_size("1920x0"), "1:1");
        assert_eq!(calculate_aspect_ratio_from_size("0x1080"), "1:1");
        assert_eq!(calculate_aspect_ratio_from_size("abc x def"), "1:1");
    }

    #[test]
    fn test_image_config_merging_priority() {
        // Case 1: Body contains empty/default imageSize, suffix contains -4k
        // Expected: Should KEEP 4K from suffix
        let body = json!({
            "generationConfig": {
                "imageConfig": {
                    "aspectRatio": "1:1",
                    "imageSize": "1K" // Simulated downgrade from client
                }
            }
        });
        let config = resolve_request_config(
            "gemini-3-pro-image-4k",
            "gemini-3-pro-image",
            &None,
            None,
            None,
            None,
            Some(&body),
        );
        let image_config = config.image_config.unwrap();
        assert_eq!(image_config["imageSize"], "4K", "Should shield inferred 4K from body downgrade");
        assert_eq!(image_config["aspectRatio"], "1:1", "Should take aspectRatio from body");

        // Case 2: Suffix contains -16-9, Body contains aspectRatio: 1:1
        // Expected: Body overrides suffix for aspectRatio (since it's not a 'downgrade' shield case yet, only size is shielded)
        let body_2 = json!({
            "generationConfig": {
                "imageConfig": {
                    "aspectRatio": "1:1"
                }
            }
        });
        let config_2 = resolve_request_config(
            "gemini-3-pro-image-16x9",
            "gemini-3-pro-image",
            &None,
            None,
            None,
            None,
            Some(&body_2),
        );
        let image_config_2 = config_2.image_config.unwrap();
        assert_eq!(image_config_2["aspectRatio"], "1:1", "Body should be allowed to override aspectRatio");
    }

    #[test]
    fn test_image_size_priority() {
        // Case 1: imageSize param overrides quality
        // Expected: "4K" from imageSize param
        let (config_1, _) = parse_image_config_with_params(
            "gemini-3-pro-image",
            None,
            Some("standard"), // would be 1K
            Some("4K"),       // should override
        );
        assert_eq!(config_1["imageSize"], "4K");

        // Case 2: imageSize param overrides suffix
        // Expected: "2K" from imageSize param
        let (config_2, _) = parse_image_config_with_params(
            "gemini-3-pro-image-4k", // would be 4K
            None,
            None,
            Some("2K"), // should override
        );
        assert_eq!(config_2["imageSize"], "2K");

        // Case 3: imageSize param + size param + quality param
        // Expected: "4K" from imageSize, "16:9" from size
        let (config_3, _) = parse_image_config_with_params(
            "gemini-3-pro-image",
            Some("1920x1080"), // 16:9
            Some("standard"),  // 1K (ignored)
            Some("4K"),        // 4K (priority)
        );
        assert_eq!(config_3["imageSize"], "4K");
        assert_eq!(config_3["aspectRatio"], "16:9");
    }
}
