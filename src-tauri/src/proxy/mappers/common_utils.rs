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
    size: Option<&str>,       // [NEW] Image size parameter
    quality: Option<&str>,    // [NEW] Image quality parameter
    image_size: Option<&str>, // [NEW] Direct imageSize parameter (e.g. "4K")
    body: Option<&Value>,     // [NEW] Request body for Gemini native imageConfig
) -> RequestConfig {
    // 1. Image Generation Check (Priority)
    // Detect via the original requested alias OR the account-resolved model name, because the
    // dynamic model rewrite may turn "gemini-3-pro-image" into e.g. "gemini-3.1-flash-image".
    if original_model.to_lowercase().contains("-image") || mapped_model.contains("-image") {
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
                                let is_size_downgrade = key == "imageSize"
                                    && (value.as_str() == Some("1K") || value.is_null())
                                    && inferred_obj.contains_key("imageSize");

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
            parsed_base_model,
            inferred_config
        );

        // Prefer the account-resolved concrete image model (mapped_model) for the upstream
        // call; fall back to the parsed base of the requested alias if it wasn't resolved.
        let upstream_model = if mapped_model.contains("-image") {
            mapped_model.to_string()
        } else {
            parsed_base_model
        };
        return RequestConfig {
            request_type: "image_gen".to_string(),
            inject_google_search: false,
            final_model: upstream_model,
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
        || mapped_model.starts_with("gemini-3.5-")
        || mapped_model.starts_with("gemini-pro-")
        || mapped_model.starts_with("gemini-3-flash")
        || mapped_model.starts_with("gemini-3.5-flash")
        || mapped_model.starts_with("agent")
        || mapped_model.contains("claude-3-5-sonnet")
        || mapped_model.contains("claude-3-opus")
        || mapped_model.contains("claude-sonnet")
        || mapped_model.contains("claude-opus")
        || mapped_model.contains("claude-4")
        || crate::proxy::model_specs::is_gemini_v3_or_above(mapped_model);

    // Determine if we should enable networking
    // [FIX] 禁用基于模型的自动联网逻辑，防止图像请求被联网搜索结果覆盖。
    // 仅在用户显式请求联网时启用：1) -online 后缀 2) 携带联网工具定义
    let enable_networking = is_online_suffix || has_networking_tool;

    // The final model to send upstream should be the MAPPED model,
    // but if searching, we MUST ensure the model name is one the backend associates with search.
    // Force a stable search model for search requests.
    let mut final_model = mapped_model.trim_end_matches("-online").to_string();

    // Map explicit preview aliases that have stable physical counterparts.
    // Note: gemini-3-pro-preview / gemini-3.1-pro-preview are intentionally NOT forced
    // to *-high here; dynamic runtime rewrite is handled after account selection.
    final_model = match final_model.as_str() {
        "gemini-3-pro-image-preview" => "gemini-3-pro-image".to_string(),
        "gemini-3-flash-preview" => "gemini-3-flash".to_string(),
        _ => final_model,
    };

    // [FIX] 不再强行将模型降级为 gemini-2.5-flash，彻底杜绝静默降级
    if enable_networking && !_is_high_quality_model {
        tracing::debug!(
            "[Common-Utils] Request enables web search for model {}",
            final_model
        );
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

/// Parse image configuration while rejecting an explicit, unsupported `imageSize` value.
/// API handlers should use this variant so invalid client input becomes a boundary error.
pub fn try_parse_image_config_with_params(
    model_name: &str,
    size: Option<&str>,
    quality: Option<&str>,
    image_size: Option<&str>,
) -> Result<(Value, String), String> {
    let image_size = normalize_image_size(image_size)?;
    Ok(parse_image_config_with_normalized_params(
        model_name, size, quality, image_size,
    ))
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
    // Legacy internal callers cannot return an HTTP boundary error. Invalid explicit values are
    // ignored here; public API handlers use `try_parse_image_config_with_params` instead.
    let image_size = normalize_image_size(image_size).ok().flatten();
    parse_image_config_with_normalized_params(model_name, size, quality, image_size)
}

fn parse_image_config_with_normalized_params(
    model_name: &str,
    size: Option<&str>,
    quality: Option<&str>,
    image_size: Option<&'static str>,
) -> (Value, String) {
    let mut aspect_ratio = "1:1";

    // 1. 优先从 size 参数解析宽高比
    if let Some(parsed_ratio) = size.and_then(image_aspect_ratio_from_size) {
        aspect_ratio = parsed_ratio;
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
    if let Some(image_size) = image_size {
        config.insert("imageSize".to_string(), json!(image_size));
    } else {
        // 3. 优先从 quality 参数解析分辨率
        if let Some(image_size) = quality.and_then(image_size_from_quality) {
            config.insert("imageSize".to_string(), json!(image_size));
        } else {
            // 4. 回退到模型后缀解析（保持向后兼容）
            let is_hd = model_name.contains("-4k") || model_name.contains("-hd");
            let is_2k = model_name.contains("-2k");
            let is_1k = model_name.contains("-1k") || model_name.contains("-standard");

            if is_hd {
                config.insert("imageSize".to_string(), json!("4K"));
            } else if is_2k {
                config.insert("imageSize".to_string(), json!("2K"));
            } else if is_1k {
                config.insert("imageSize".to_string(), json!("1K"));
            }
        }
    }

    let clean_model_name = clean_image_model_name(model_name);

    (serde_json::Value::Object(config), clean_model_name)
}

fn normalize_image_size(image_size: Option<&str>) -> Result<Option<&'static str>, String> {
    let Some(image_size) = image_size.map(str::trim) else {
        return Ok(None);
    };

    if image_size.is_empty() || image_size.eq_ignore_ascii_case("auto") {
        return Ok(None);
    }

    match image_size.to_ascii_lowercase().as_str() {
        "1k" => Ok(Some("1K")),
        "2k" => Ok(Some("2K")),
        "4k" => Ok(Some("4K")),
        _ => Err("Invalid image_size: expected one of 1K, 2K, 4K, or auto".to_string()),
    }
}

fn image_size_from_quality(quality: &str) -> Option<&'static str> {
    match quality.trim().to_ascii_lowercase().as_str() {
        "low" | "standard" | "1k" => Some("1K"),
        "medium" | "2k" => Some("2K"),
        "high" | "hd" | "4k" => Some("4K"),
        "auto" | "" => None,
        _ => None,
    }
}

/// Helper function to clean image model names by removing resolution/aspect-ratio suffixes.
/// E.g., "gemini-3.1-flash-image-16x9-4k" -> "gemini-3.1-flash-image"
fn clean_image_model_name(model_name: &str) -> String {
    let mut clean_name = model_name.to_lowercase();

    // Ordered list of known suffixes to strip
    let suffixes = [
        "-4k",
        "-2k",
        "-1k",
        "-hd",
        "-standard",
        "-medium",
        "-21x9",
        "-21-9",
        "-16x9",
        "-16-9",
        "-9x16",
        "-9-16",
        "-4x3",
        "-4-3",
        "-3x4",
        "-3-4",
        "-3x2",
        "-3-2",
        "-2x3",
        "-2-3",
        "-5x4",
        "-5-4",
        "-4x5",
        "-4-5",
        "-1x1",
        "-1-1",
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
pub fn image_aspect_ratio_from_size(size: &str) -> Option<&'static str> {
    let size = size.trim();
    if size.is_empty() || size.eq_ignore_ascii_case("auto") {
        return None;
    }

    // 0. Explicitly check known aspect ratios first
    match size {
        "21:9" => return Some("21:9"),
        "16:9" => return Some("16:9"),
        "9:16" => return Some("9:16"),
        "4:3" => return Some("4:3"),
        "3:4" => return Some("3:4"),
        "3:2" => return Some("3:2"),
        "2:3" => return Some("2:3"),
        "5:4" => return Some("5:4"),
        "4:5" => return Some("4:5"),
        "1:1" => return Some("1:1"),
        _ => {}
    }

    if let Some((w_str, h_str)) = size.split_once('x') {
        if let (Ok(width), Ok(height)) = (w_str.parse::<f64>(), h_str.parse::<f64>()) {
            if width > 0.0 && height > 0.0 {
                let ratio = width / height;

                // 容差匹配常见比例（容差 0.05，避免 3:4 和 2:3 重叠）
                if (ratio - 21.0 / 9.0).abs() < 0.05 {
                    return Some("21:9");
                }
                if (ratio - 16.0 / 9.0).abs() < 0.05 {
                    return Some("16:9");
                }
                if (ratio - 4.0 / 3.0).abs() < 0.05 {
                    return Some("4:3");
                }
                if (ratio - 3.0 / 4.0).abs() < 0.05 {
                    return Some("3:4");
                }
                if (ratio - 9.0 / 16.0).abs() < 0.05 {
                    return Some("9:16");
                }
                if (ratio - 3.0 / 2.0).abs() < 0.05 {
                    return Some("3:2");
                }
                if (ratio - 2.0 / 3.0).abs() < 0.05 {
                    return Some("2:3");
                }
                if (ratio - 5.0 / 4.0).abs() < 0.05 {
                    return Some("5:4");
                }
                if (ratio - 4.0 / 5.0).abs() < 0.05 {
                    return Some("4:5");
                }
                if (ratio - 1.0).abs() < 0.05 {
                    return Some("1:1");
                }
            }
        }
    }

    None
}

fn calculate_aspect_ratio_from_size(size: &str) -> &'static str {
    image_aspect_ratio_from_size(size).unwrap_or("1:1")
}

/// Inject current googleSearch tool and ensure no duplicate legacy search tools.
/// When client-defined function tools are present, skips googleSearch to avoid client-side empty/unknown tool dispatch errors.
pub fn inject_google_search_tool(body: &mut Value, _mapped_model: Option<&str>) {
    if let Some(obj) = body.as_object_mut() {
        let tools_entry = obj.entry("tools").or_insert_with(|| json!([]));
        if let Some(tools_arr) = tools_entry.as_array_mut() {
            let has_functions = tools_arr.iter().any(|t| {
                t.as_object().map_or(false, |o| {
                    o.contains_key("functionDeclarations")
                        || o.contains_key("function_declarations")
                })
            });

            // [STABILITY GUARD] 如果客户端自身已经定义了函数工具 (functionDeclarations / function_declarations)，
            // 不强行注入 googleSearch 工具。防止服务端接地调用导致客户端无法分发、空工具调用或报未知工具错误。
            if has_functions {
                tracing::debug!(
                    "Skipping googleSearch injection: functionDeclarations present, avoiding client tool dispatch conflicts"
                );
                return;
            }

            // 首先清理掉已存在的 googleSearch 或 googleSearchRetrieval，以防重复产生冲突
            tools_arr.retain(|t| {
                if let Some(o) = t.as_object() {
                    !(o.contains_key("googleSearch")
                        || o.contains_key("google_search")
                        || o.contains_key("googleSearchRetrieval"))
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
                            let keywords = [
                                "web_search",
                                "google_search",
                                "google_search_retrieval",
                                "builtin_web_search",
                            ];
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

/// 检测是否携带任何工具定义 (无论是本地函数还是联网工具)
pub fn has_any_tools(tools: &Option<Vec<Value>>) -> bool {
    if let Some(list) = tools {
        !list.is_empty()
    } else {
        false
    }
}

/// 检查 contents 中是否包含工具调用或工具返回结果 (表明处于多轮 Agent 会话中)
pub fn contents_has_tool_interactions(contents: &Value) -> bool {
    if let Some(arr) = contents.as_array() {
        for msg in arr {
            if let Some(parts) = msg.get("parts").and_then(|p| p.as_array()) {
                for part in parts {
                    if part.get("functionCall").is_some()
                        || part.get("functionResponse").is_some()
                        || part.get("tool_use").is_some()
                        || part.get("tool_result").is_some()
                    {
                        return true;
                    }
                }
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
        let config =
            resolve_request_config("gpt-4o", "gemini-2.5-flash", &None, None, None, None, None);
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
        let config = resolve_request_config(
            "gemini-3-flash-online",
            "gemini-3-flash",
            &None,
            None,
            None,
            None,
            None,
        );
        assert_eq!(config.request_type, "web_search");
        assert!(config.inject_google_search);
        assert_eq!(config.final_model, "gemini-3-flash");
    }

    #[test]
    fn test_default_no_grounding() {
        let config = resolve_request_config(
            "claude-sonnet",
            "gemini-3-flash",
            &None,
            None,
            None,
            None,
            None,
        );
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
        let (config_hd, model_hd) =
            parse_image_config_with_params("gemini-3-pro-image", None, Some("hd"), None);
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
        let (config_combined, model_combined) = parse_image_config_with_params(
            "gemini-3-pro-image",
            Some("1920x1080"),
            Some("hd"),
            None,
        );
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
        assert_eq!(
            clean_image_model_name("gemini-3.1-flash-image"),
            "gemini-3.1-flash-image"
        );
        assert_eq!(
            clean_image_model_name("gemini-3.1-flash-image-4k"),
            "gemini-3.1-flash-image"
        );
        assert_eq!(
            clean_image_model_name("gemini-3-pro-image-16x9"),
            "gemini-3-pro-image"
        );
        assert_eq!(
            clean_image_model_name("gemini-3-pro-image-16x9-4k"),
            "gemini-3-pro-image"
        );
        // Test varying order
        assert_eq!(
            clean_image_model_name("gemini-3.1-flash-image-4k-16x9"),
            "gemini-3.1-flash-image"
        );
        assert_eq!(
            clean_image_model_name("gemini-3.1-flash-image-16-9-hd"),
            "gemini-3.1-flash-image"
        );
        assert_eq!(
            clean_image_model_name("gemini-3.1-flash-image-2k-9x16"),
            "gemini-3.1-flash-image"
        );
        assert_eq!(
            clean_image_model_name("gemini-3.1-flash-image-1x1"),
            "gemini-3.1-flash-image"
        );
        assert_eq!(
            clean_image_model_name("gemini-3.1-flash-image-standard"),
            "gemini-3.1-flash-image"
        );
        assert_eq!(
            clean_image_model_name("gemini-3.1-flash-image-medium"),
            "gemini-3.1-flash-image"
        );
        assert_eq!(
            clean_image_model_name("gemini-3.1-flash-image-21-9-4k"),
            "gemini-3.1-flash-image"
        );
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
        assert_eq!(
            image_config["imageSize"], "4K",
            "Should shield inferred 4K from body downgrade"
        );
        assert_eq!(
            image_config["aspectRatio"], "1:1",
            "Should take aspectRatio from body"
        );

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
        assert_eq!(
            image_config_2["aspectRatio"], "1:1",
            "Body should be allowed to override aspectRatio"
        );
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

    #[test]
    fn image_quality_aliases_map_to_unified_image_sizes() {
        let cases = [
            ("low", "1K"),
            ("standard", "1K"),
            ("1k", "1K"),
            ("medium", "2K"),
            ("2k", "2K"),
            ("high", "4K"),
            ("hd", "4K"),
            ("4k", "4K"),
        ];
        for (quality, expected) in cases {
            let (config, _) = try_parse_image_config_with_params(
                "gemini-3.1-flash-image",
                None,
                Some(quality),
                None,
            )
            .expect("quality alias must parse");
            assert_eq!(config["imageSize"], expected, "quality={quality}");
        }
    }

    #[test]
    fn image_size_priority_and_auto_fallback_are_enforced() {
        let (explicit, _) = try_parse_image_config_with_params(
            "gemini-3.1-flash-image-1k",
            None,
            Some("high"),
            Some("2k"),
        )
        .expect("case-insensitive explicit image size");
        assert_eq!(explicit["imageSize"], "2K");

        let (quality, _) = try_parse_image_config_with_params(
            "gemini-3.1-flash-image-1k",
            None,
            Some("medium"),
            None,
        )
        .expect("quality overrides suffix");
        assert_eq!(quality["imageSize"], "2K");

        for quality in [Some("auto"), Some(""), None] {
            let (fallback, _) = try_parse_image_config_with_params(
                "gemini-3.1-flash-image-4k",
                None,
                quality,
                Some("auto"),
            )
            .expect("auto values fall back to suffix");
            assert_eq!(fallback["imageSize"], "4K");
        }

        let (upstream_default, _) =
            try_parse_image_config_with_params("gemini-3.1-flash-image", None, Some("auto"), None)
                .expect("auto without suffix uses upstream default");
        assert!(upstream_default.get("imageSize").is_none());

        assert!(try_parse_image_config_with_params(
            "gemini-3.1-flash-image",
            None,
            None,
            Some("8K"),
        )
        .is_err());
    }

    #[test]
    fn test_detect_mime_from_bytes() {
        assert_eq!(
            detect_mime_from_bytes(b"\x89PNG\r\n\x1a\n\0\0\0"),
            Some("image/png")
        );
        assert_eq!(
            detect_mime_from_bytes(b"\xff\xd8\xff\xe0\0\x10JFIF"),
            Some("image/jpeg")
        );
        assert_eq!(
            detect_mime_from_bytes(b"GIF89a\x01\0\x01\0"),
            Some("image/gif")
        );
        assert_eq!(
            detect_mime_from_bytes(b"RIFF\0\0\0\0WEBPVP8 "),
            Some("image/webp")
        );
        assert_eq!(
            detect_mime_from_bytes(b"%PDF-1.7\n%"),
            Some("application/pdf")
        );
        assert_eq!(detect_mime_from_bytes(b"invalid"), None);
    }

    #[test]
    fn test_validate_and_sanitize_inline_data() {
        // 1. Empty data
        assert_eq!(
            validate_and_sanitize_inline_data(Some("image/png"), ""),
            None
        );
        assert_eq!(validate_and_sanitize_inline_data(None, "   "), None);

        // 2. Corrupted short data (like the +A== in the incident)
        assert_eq!(
            validate_and_sanitize_inline_data(Some("image/png"), "+A=="),
            None
        );
        assert_eq!(validate_and_sanitize_inline_data(None, "AQ=="), None);

        // 3. Invalid base64 characters
        assert_eq!(
            validate_and_sanitize_inline_data(Some("image/png"), "not-valid-base64!@#$"),
            None
        );

        // 4. Valid PNG base64 (8 bytes magic header)
        let valid_png_b64 = "iVBORw0KGgo=";
        let res = validate_and_sanitize_inline_data(Some("image/png"), valid_png_b64);
        assert!(res.is_some());
        let (mime, data) = res.unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(data, valid_png_b64);

        // 5. Valid PNG with omitted mime type (should auto-detect from magic bytes)
        let res_no_mime = validate_and_sanitize_inline_data(None, valid_png_b64);
        assert!(res_no_mime.is_some());
        assert_eq!(res_no_mime.unwrap().0, "image/png");
    }

    #[test]
    fn test_create_gemini_inline_part() {
        let valid_png_b64 = "iVBORw0KGgo=";
        let valid_part = create_gemini_inline_part(Some("image/png"), valid_png_b64, "Image");
        assert!(valid_part.get("inlineData").is_some());
        assert_eq!(valid_part["inlineData"]["mimeType"], "image/png");

        let bad_part = create_gemini_inline_part(Some("image/png"), "+A==", "Image");
        assert!(bad_part.get("inlineData").is_none());
        assert_eq!(
            bad_part["text"],
            "[Image: invalid or corrupted data omitted]"
        );
    }

    #[test]
    fn test_sanitize_gemini_payload_inline_data() {
        let valid_png_b64 = "iVBORw0KGgo=";
        let mut payload = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [
                        { "text": "Hello" },
                        { "inlineData": { "mimeType": "image/png", "data": "+A==" } }, // corrupt
                        { "inlineData": { "mimeType": "image/png", "data": "" } },     // empty
                        { "inlineData": { "mimeType": "image/png", "data": valid_png_b64 } } // valid
                    ]
                }
            ]
        });

        let sanitized_count = sanitize_gemini_payload_inline_data(&mut payload);
        assert_eq!(sanitized_count, 2);

        let parts = payload["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0]["text"], "Hello");
        assert_eq!(
            parts[1]["text"],
            "[Image/Data: invalid or corrupted inline payload omitted]"
        );
        assert_eq!(
            parts[2]["text"],
            "[Image/Data: invalid or corrupted inline payload omitted]"
        );
        assert!(parts[3].get("inlineData").is_some());
        assert_eq!(parts[3]["inlineData"]["data"], valid_png_b64);
    }
}

/// [FIX] Parse markdown base64 images from text and split into Gemini parts
/// This prevents base64 reflection bloat where generated images are sent back as huge text strings
pub fn parse_markdown_images_to_parts(text: &str) -> Vec<Value> {
    let mut parts = Vec::new();
    // Match ![...](data:image/...;base64,...)
    if let Ok(re) = regex::Regex::new(r"!\[.*?\]\(data:(image/[^;]+);base64,([a-zA-Z0-9+/=]+)\)") {
        let mut last_match = 0;

        for cap in re.captures_iter(text) {
            let m = cap.get(0).unwrap();

            // Add preceding text
            if m.start() > last_match {
                let preceding = &text[last_match..m.start()];
                if !preceding.trim().is_empty() {
                    parts.push(json!({"text": preceding}));
                }
            }

            // Add inlineData image
            let mime = cap.get(1).unwrap().as_str();
            let b64 = cap.get(2).unwrap().as_str();
            let part = create_gemini_inline_part(Some(mime), b64, "Markdown Image");
            parts.push(part);

            last_match = m.end();
        }

        // Add remaining text
        if last_match < text.len() {
            let remaining = &text[last_match..];
            if !remaining.trim().is_empty() {
                parts.push(json!({"text": remaining}));
            }
        }

        if parts.is_empty() && !text.trim().is_empty() {
            parts.push(json!({"text": text}));
        }

        return parts;
    }

    if !text.trim().is_empty() {
        parts.push(json!({"text": text}));
    }

    parts
}

/// [FIX] Inject explicit tool mapping instructions for Gemini to read SKILL.md
pub fn enhance_gemini_skills_prompt(text: &str) -> String {
    let mut enhanced = text.to_string();
    let warning_note = "\n\n**[CRITICAL INSTRUCTION FOR GEMINI - HOW TO READ SKILL.md]**\nYou do NOT have a direct `view_file` or `read_file` tool.\nTo \"open and read its SKILL.md completely\" as instructed above, you MUST use the `shell_command` tool.\nFor example, run the following command in PowerShell:\n`Get-Content -Raw -Path \"C:\\Users\\...\\SKILL.md\"`\nDo NOT guess other non-existent reading tools. You must use `shell_command`!\n\n";

    // Inject before </skills_instructions> or </skills>
    if enhanced.contains("</skills_instructions>") {
        enhanced = enhanced.replace(
            "</skills_instructions>",
            &format!("{}</skills_instructions>", warning_note),
        );
    } else if enhanced.contains("</skills>") {
        enhanced = enhanced.replace("</skills>", &format!("{}</skills>", warning_note));
    }

    enhanced
}

/// Detect common MIME types from magic bytes
pub fn detect_mime_from_bytes(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"%PDF-") {
        Some("application/pdf")
    } else if bytes.len() >= 12
        && (&bytes[4..12] == b"ftypheic"
            || &bytes[4..12] == b"ftypmif1"
            || &bytes[4..12] == b"ftypheix")
    {
        Some("image/heic")
    } else {
        None
    }
}

/// Validates and sanitizes inline base64 data (images/documents) for Gemini upstream.
/// Returns `Some((mime_type, sanitized_b64))` if valid, or `None` if corrupt/empty/too small.
pub fn validate_and_sanitize_inline_data(
    mime_type: Option<&str>,
    b64_data: &str,
) -> Option<(String, String)> {
    let clean_b64 = b64_data.trim();
    if clean_b64.is_empty() {
        return None;
    }

    use base64::{engine::general_purpose::STANDARD, Engine as _};

    // Try decoding base64 to check validity and magic bytes
    let decoded_bytes = match STANDARD.decode(clean_b64) {
        Ok(bytes) => bytes,
        Err(_) => {
            use base64::engine::general_purpose::URL_SAFE;
            match URL_SAFE.decode(clean_b64) {
                Ok(bytes) => bytes,
                Err(_) => return None,
            }
        }
    };

    if decoded_bytes.is_empty() {
        return None;
    }

    let declared_mime = mime_type.map(str::trim).filter(|m| !m.is_empty());

    let is_audio_or_video = declared_mime
        .map(|m| m.starts_with("video/") || m.starts_with("audio/"))
        .unwrap_or(false);

    if !is_audio_or_video {
        // For images/documents, require at least 5 decoded bytes and 8 base64 chars
        if clean_b64.len() < 8 || decoded_bytes.len() < 5 {
            return None;
        }
    }

    // Detect MIME from magic bytes if possible
    let inferred_mime = detect_mime_from_bytes(&decoded_bytes);

    let final_mime = match (mime_type.map(str::trim), inferred_mime) {
        (Some(m), _)
            if !m.is_empty()
                && (m.starts_with("image/")
                    || m.starts_with("application/")
                    || m.starts_with("audio/")
                    || m.starts_with("video/")) =>
        {
            m.to_string()
        }
        (_, Some(inferred)) => inferred.to_string(),
        (Some(m), _) if !m.is_empty() => m.to_string(),
        _ => "image/jpeg".to_string(), // fallback default
    };

    Some((final_mime, clean_b64.to_string()))
}

/// Helper to create a Gemini inlineData part or fallback text if invalid
pub fn create_gemini_inline_part(
    mime_type: Option<&str>,
    b64_data: &str,
    fallback_label: &str,
) -> Value {
    if let Some((valid_mime, valid_data)) = validate_and_sanitize_inline_data(mime_type, b64_data) {
        json!({
            "inlineData": {
                "mimeType": valid_mime,
                "data": valid_data
            }
        })
    } else {
        tracing::warn!(
            "[Image-Defense] Omitted invalid or corrupt base64 data (len: {}, mime: {:?})",
            b64_data.len(),
            mime_type
        );
        json!({
            "text": format!("[{}: invalid or corrupted data omitted]", fallback_label)
        })
    }
}

/// Sanitizes any inlineData in an entire Gemini JSON request payload in-place.
/// Replaces invalid inlineData / inline_data parts with placeholder text parts.
pub fn sanitize_gemini_payload_inline_data(body: &mut Value) -> usize {
    let mut total_sanitized = 0;

    let mut sanitize_parts = |parts: &mut Vec<Value>| {
        for part in parts.iter_mut() {
            if let Some(obj) = part.as_object_mut() {
                let inline_key = if obj.contains_key("inlineData") {
                    Some("inlineData")
                } else if obj.contains_key("inline_data") {
                    Some("inline_data")
                } else {
                    None
                };

                if let Some(key) = inline_key {
                    let inline_obj = obj.get(key).and_then(Value::as_object);
                    let mime = inline_obj
                        .and_then(|o| o.get("mimeType").or_else(|| o.get("mime_type")))
                        .and_then(Value::as_str);
                    let data = inline_obj
                        .and_then(|o| o.get("data"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();

                    if let Some((valid_mime, valid_data)) =
                        validate_and_sanitize_inline_data(mime, data)
                    {
                        // Ensure mimeType and data are normalized
                        obj.insert(
                            "inlineData".to_string(),
                            json!({
                                "mimeType": valid_mime,
                                "data": valid_data
                            }),
                        );
                        if key == "inline_data" {
                            obj.remove("inline_data");
                        }
                    } else {
                        total_sanitized += 1;
                        tracing::warn!(
                            "[Payload-Defense] Sanitized invalid inlineData part (len: {}, mime: {:?}) into text placeholder",
                            data.len(),
                            mime
                        );
                        *part = json!({
                            "text": "[Image/Data: invalid or corrupted inline payload omitted]"
                        });
                    }
                }
            }
        }
    };

    if let Some(contents) = body.get_mut("contents").and_then(Value::as_array_mut) {
        for content in contents.iter_mut() {
            if let Some(parts) = content.get_mut("parts").and_then(Value::as_array_mut) {
                sanitize_parts(parts);
            }
        }
    }

    if let Some(sys) = body
        .get_mut("systemInstruction")
        .and_then(Value::as_object_mut)
    {
        if let Some(parts) = sys.get_mut("parts").and_then(Value::as_array_mut) {
            sanitize_parts(parts);
        }
    }

    total_sanitized
}

/// Check if two model strings are compatible (same family)
pub fn is_model_compatible(cached: &str, target: &str) -> bool {
    let c = cached.to_lowercase();
    let t = target.to_lowercase();

    if c == t {
        return true;
    }

    // Claude 全系列通用兼容：凡是同属 Claude 家族模型，直接判定签名兼容（面向未来任何 Claude 5/新模型及变体）
    if c.contains("claude") && t.contains("claude") {
        return true;
    }

    // Gemini models: strict family match required for signatures
    if c.contains("gemini-1.5-pro") && t.contains("gemini-1.5-pro") {
        return true;
    }
    if c.contains("gemini-1.5-flash") && t.contains("gemini-1.5-flash") {
        return true;
    }
    if c.contains("gemini-2.0-flash") && t.contains("gemini-2.0-flash") {
        return true;
    }
    if c.contains("gemini-2.0-pro") && t.contains("gemini-2.0-pro") {
        return true;
    }
    if c.contains("gemini-3") && t.contains("gemini-3") {
        let c_flash = c.contains("flash");
        let t_flash = t.contains("flash");
        let c_pro = c.contains("pro");
        let t_pro = t.contains("pro");
        if c_flash == t_flash && c_pro == t_pro {
            return true;
        }
        if c_flash && t_flash {
            return true;
        }
        if c_pro && t_pro {
            return true;
        }
    }
    if c.contains("gemini-3.7") && t.contains("gemini-3.7") {
        return true;
    }

    false
}

pub fn model_keeps_thinking_without_signature(mapped_model: &str) -> bool {
    let m = mapped_model.to_lowercase();
    m.contains("flash") || m.contains("gemini-pro-agent")
}

/// [JEIKCODE SYNTHETIC USER REMINDER]
/// 将对话中途动态插入的系统消息就地包装为 `<system-reminder>` 标签块。
/// 提示词采用英文，明确告知模型：本内容为系统层注入的背景提醒，并非本轮用户输入，
/// 从而保证用户原始 query 完整透传，同时全局顶层 systemInstruction 保持绝对冻结以稳定 KV Cache。
pub fn wrap_in_system_reminder(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with("<system-reminder>") && trimmed.ends_with("</system-reminder>") {
        return trimmed.to_string();
    }
    format!(
        "<system-reminder>\nBefore the user's request for this turn, the system provides the following reminder for your awareness. Please note that this is from prior system messages, not spoken by the user:\n{}\n</system-reminder>",
        trimmed
    )
}

/// [DEFENSE] 通用中转报文保底文本（温和提示继续分析，避免触发 Agent 误进入修改阶段）
pub const TRANSIT_DEFENSE_FALLBACK_TEXT: &str = "Please continue your analysis.";

/// [DEFENSE] 通用中转报文保底防御节点（协议无关性）
/// 确保发给 Google Gemini 的报文末尾轮次严格符合规范：
/// 1. 自动兼容平铺 payload 或包含 "request" 包装的 payload；
/// 2. 若 contents 为空，追加 {"role": "user", "parts": [{"text": TRANSIT_DEFENSE_FALLBACK_TEXT}]}；
/// 3. 若末尾轮次为 "model"（缺失用户轮次），追加 {"role": "user", "parts": [{"text": TRANSIT_DEFENSE_FALLBACK_TEXT}]}；
/// 4. 若末尾轮次为 "user" 且其 parts 为空、或仅含有空文本 / "(no content)" / "·" 且无工具/图片，规范化填充为 [{"text": TRANSIT_DEFENSE_FALLBACK_TEXT}]；
/// 5. 修复中间轮次中 parts 为空的情况，防止 Google 返回 400 "parts must not be empty"。
pub fn ensure_gemini_payload_ends_with_user(body: &mut Value) -> bool {
    let contents = if let Some(contents) = body
        .get_mut("request")
        .and_then(|r| r.get_mut("contents"))
        .and_then(|c| c.as_array_mut())
    {
        contents
    } else if let Some(contents) = body.get_mut("contents").and_then(|c| c.as_array_mut()) {
        contents
    } else {
        return false;
    };

    let mut modified = false;

    // 防御 1: contents 整体为空
    if contents.is_empty() {
        tracing::warn!("[Defense] Gemini contents array is empty, appending fallback user turn");
        contents.push(json!({
            "role": "user",
            "parts": [{ "text": TRANSIT_DEFENSE_FALLBACK_TEXT }]
        }));
        return true;
    }

    // 防御 2: 修复历史/中间轮次中可能存在的 parts 为空
    for turn in contents.iter_mut() {
        let is_model = turn
            .get("role")
            .and_then(|r| r.as_str())
            .map(|r| r == "model" || r == "assistant")
            .unwrap_or(false);
        if let Some(parts) = turn.get_mut("parts").and_then(|p| p.as_array_mut()) {
            if parts.is_empty() {
                modified = true;
                if is_model {
                    parts.push(json!({ "text": "..." }));
                } else {
                    parts.push(json!({ "text": TRANSIT_DEFENSE_FALLBACK_TEXT }));
                }
            }
        }
    }

    // 防御 3: 检查末尾轮次
    let need_append_user = if let Some(last_turn) = contents.last_mut() {
        let role = last_turn.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if role == "model" || role == "assistant" {
            true
        } else {
            if let Some(parts) = last_turn.get_mut("parts").and_then(|p| p.as_array_mut()) {
                let has_substantive_part = parts.iter().any(|part| {
                    if part.get("functionCall").is_some()
                        || part.get("functionResponse").is_some()
                        || part.get("inlineData").is_some()
                        || part.get("fileData").is_some()
                    {
                        return true;
                    }
                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                        let t = text.trim();
                        !t.is_empty() && t != "(no content)" && t != "·"
                    } else {
                        false
                    }
                });

                if !has_substantive_part {
                    tracing::warn!(
                        "[Defense] Last user turn has no substantive content, normalizing to '{}'",
                        TRANSIT_DEFENSE_FALLBACK_TEXT
                    );
                    *parts = vec![json!({ "text": TRANSIT_DEFENSE_FALLBACK_TEXT })];
                    modified = true;
                }
            }
            false
        }
    } else {
        false
    };

    if need_append_user {
        tracing::warn!(
            "[Defense] Gemini payload ended with model turn, appending user turn with '{}'",
            TRANSIT_DEFENSE_FALLBACK_TEXT
        );
        contents.push(json!({
            "role": "user",
            "parts": [{ "text": TRANSIT_DEFENSE_FALLBACK_TEXT }]
        }));
        modified = true;
    }

    modified
}

/// 安全地按最大字节数截断字符串切片，保证切片边界严格对齐在 UTF-8 字符边界上。
/// 若 max_bytes 恰好落在多字节字符中间，会自动向左回退到最近的合法字符边界。
pub fn safe_truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// 安全地按最大字符数 (Unicode 标量值) 截断字符串切片。
/// 如果字符总数超过 max_chars，截取前 max_chars 个字符对应的有效切片。
pub fn safe_truncate_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
}

#[cfg(test)]
mod defense_tests {
    use super::*;

    #[test]
    fn test_ensure_gemini_payload_ends_with_user_empty() {
        let mut payload = json!({
            "contents": []
        });
        assert!(ensure_gemini_payload_ends_with_user(&mut payload));
        let contents = payload["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(
            contents[0]["parts"][0]["text"],
            TRANSIT_DEFENSE_FALLBACK_TEXT
        );
    }

    #[test]
    fn test_ensure_gemini_payload_ends_with_user_model_ending() {
        let mut payload = json!({
            "request": {
                "contents": [
                    { "role": "user", "parts": [{ "text": "hello" }] },
                    { "role": "model", "parts": [{ "text": "hi there" }] }
                ]
            }
        });
        assert!(ensure_gemini_payload_ends_with_user(&mut payload));
        let contents = payload["request"]["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[2]["role"], "user");
        assert_eq!(
            contents[2]["parts"][0]["text"],
            TRANSIT_DEFENSE_FALLBACK_TEXT
        );
    }

    #[test]
    fn test_ensure_gemini_payload_ends_with_user_no_content() {
        let mut payload = json!({
            "contents": [
                { "role": "user", "parts": [{ "text": "(no content)" }] }
            ]
        });
        assert!(ensure_gemini_payload_ends_with_user(&mut payload));
        let contents = payload["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(
            contents[0]["parts"][0]["text"],
            TRANSIT_DEFENSE_FALLBACK_TEXT
        );
    }

    #[test]
    fn test_ensure_gemini_payload_ends_with_user_valid_untouched() {
        let mut payload = json!({
            "contents": [
                { "role": "user", "parts": [{ "text": "valid message" }] }
            ]
        });
        assert!(!ensure_gemini_payload_ends_with_user(&mut payload));
        let contents = payload["contents"].as_array().unwrap();
        assert_eq!(contents[0]["parts"][0]["text"], "valid message");
    }

    #[test]
    fn test_wrap_in_system_reminder() {
        use super::wrap_in_system_reminder;

        // Empty content returns empty string
        assert_eq!(wrap_in_system_reminder("   "), "");

        // Raw text gets wrapped with English reminder header
        let wrapped = wrap_in_system_reminder("Current date: 2026-09-19");
        assert!(wrapped.starts_with("<system-reminder>\nBefore the user's request for this turn"));
        assert!(wrapped.contains("Current date: 2026-09-19"));
        assert!(wrapped.ends_with("</system-reminder>"));

        // Already wrapped content is untouched (no double wrapping)
        let already = "<system-reminder>\nsome text\n</system-reminder>";
        assert_eq!(wrap_in_system_reminder(already), already);
    }

    #[test]
    fn test_safe_truncate_str_utf8_boundaries() {
        // "你好世界" 每个汉字 3 字节，共 12 字节:
        // '你': 0..3, '好': 3..6, '世': 6..9, '界': 9..12
        let text = "你好世界";
        assert_eq!(safe_truncate_str(text, 0), "");
        assert_eq!(safe_truncate_str(text, 1), ""); // 落在 '你' 中间，回退到 0
        assert_eq!(safe_truncate_str(text, 2), ""); // 落在 '你' 中间，回退到 0
        assert_eq!(safe_truncate_str(text, 3), "你");
        assert_eq!(safe_truncate_str(text, 4), "你"); // 落在 '好' 中间，回退到 3
        assert_eq!(safe_truncate_str(text, 5), "你");
        assert_eq!(safe_truncate_str(text, 6), "你好");
        assert_eq!(safe_truncate_str(text, 12), "你好世界");
        assert_eq!(safe_truncate_str(text, 100), "你好世界");

        // 验证 Issue #3493 场景：第 57 字节落在 3 字节中文字符内部
        // 构造 55 字节 ASCII + "中文测试"（每个 3 字节）
        // "中文测试" 从索引 55 开始: '中' (55..58)
        // 索引 57 正好落在 '中' 的中间 (55..58)
        let mut s3493 = "a".repeat(55);
        s3493.push_str("中文测试");
        assert!(!s3493.is_char_boundary(57));
        let truncated = safe_truncate_str(&s3493, 57);
        assert_eq!(truncated.len(), 55);
        assert_eq!(truncated, "a".repeat(55));

        // Emoji 测试 (4 字节: 🦀 0..4)
        let emoji = "🦀🦀";
        assert_eq!(safe_truncate_str(emoji, 2), "");
        assert_eq!(safe_truncate_str(emoji, 4), "🦀");
        assert_eq!(safe_truncate_str(emoji, 6), "🦀");
        assert_eq!(safe_truncate_str(emoji, 8), "🦀🦀");
    }

    #[test]
    fn test_safe_truncate_chars_utf8() {
        let text = "你好世界，Rust编程！";
        assert_eq!(safe_truncate_chars(text, 0), "");
        assert_eq!(safe_truncate_chars(text, 2), "你好");
        assert_eq!(safe_truncate_chars(text, 4), "你好世界");
        assert_eq!(safe_truncate_chars(text, 5), "你好世界，");
        assert_eq!(safe_truncate_chars(text, 100), text);

        let emoji_text = "🎉Hello世界🦀";
        assert_eq!(safe_truncate_chars(emoji_text, 1), "🎉");
        assert_eq!(safe_truncate_chars(emoji_text, 6), "🎉Hello");
        assert_eq!(safe_truncate_chars(emoji_text, 8), "🎉Hello世界");
        assert_eq!(safe_truncate_chars(emoji_text, 9), "🎉Hello世界🦀");
    }
}
