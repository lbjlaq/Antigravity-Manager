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
    tools: &Option<Vec<Value>>
) -> RequestConfig {
    // 1. Image Generation Check (Priority)
    if mapped_model.starts_with("gemini-3-pro-image") {
        let (image_config, parsed_base_model) = parse_image_config(original_model);
        
        return RequestConfig {
            request_type: "image_gen".to_string(),
            inject_google_search: false,
            final_model: parsed_base_model, 
            image_config: Some(image_config),
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
    // Based on ref_Antigravity2Api practice, we force a stable search model for search requests.
    let mut final_model = mapped_model.trim_end_matches("-online").to_string();
    if enable_networking {
        // If it's a thinking model (which doesn't support tools) or a Claude-style alias, 
        // fallback to gemini-2.5-flash which is the standard workhorse for search.
        if final_model.contains("thinking") || !final_model.starts_with("gemini-") {
            final_model = "gemini-2.5-flash".to_string();
        }
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

/// Inject current googleSearch tool and ensure no duplicate legacy search tools
pub fn inject_google_search_tool(body: &mut Value) {
    if let Some(obj) = body.as_object_mut() {
        let tools_entry = obj.entry("tools").or_insert_with(|| json!([]));
        if let Some(tools_arr) = tools_entry.as_array_mut() {
            // [安全校验] 如果数组中已经包含 functionDeclarations，严禁注入 googleSearch
            // 因为 Gemini v1internal 不支持在一次请求中混用 search 和 functions
            let has_functions = tools_arr.iter().any(|t| {
                t.as_object().map_or(false, |o| o.contains_key("functionDeclarations"))
            });

            if has_functions {
                tracing::info!("Skipping googleSearch injection due to existing functionDeclarations");
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
pub fn deep_clean_undefined(value: &mut Value) {
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
                deep_clean_undefined(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                deep_clean_undefined(v);
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
                if n == "web_search" || n == "google_search" || n == "web_search_20250305" || n == "google_search_retrieval" {
                    return true;
                }
            }

            if let Some(t) = tool.get("type").and_then(|v| v.as_str()) {
                if t == "web_search_20250305" || t == "google_search" || t == "web_search" || t == "google_search_retrieval" {
                    return true;
                }
            }

            // 2. OpenAI 嵌套风格: { "type": "function", "function": { "name": "..." } }
            if let Some(func) = tool.get("function") {
                if let Some(n) = func.get("name").and_then(|v| v.as_str()) {
                    let keywords = ["web_search", "google_search", "web_search_20250305", "google_search_retrieval"];
                    if keywords.contains(&n) {
                        return true;
                    }
                }
            }

            // 3. Gemini 原生风格: { "functionDeclarations": [ { "name": "..." } ] }
            if let Some(decls) = tool.get("functionDeclarations").and_then(|v| v.as_array()) {
                for decl in decls {
                    if let Some(n) = decl.get("name").and_then(|v| v.as_str()) {
                        if n == "web_search" || n == "google_search" || n == "google_search_retrieval" {
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
                 let keywords = ["web_search", "google_search", "web_search_20250305", "google_search_retrieval"];
                 if keywords.contains(&n) { is_networking = true; }
            } else if let Some(func) = tool.get("function") {
                 if let Some(n) = func.get("name").and_then(|v| v.as_str()) {
                     let keywords = ["web_search", "google_search", "web_search_20250305", "google_search_retrieval"];
                     if keywords.contains(&n) { is_networking = true; }
                 }
            } else if tool.get("googleSearch").is_some() || tool.get("googleSearchRetrieval").is_some() {
                is_networking = true;
            } else if tool.get("functionDeclarations").is_some() {
                // 如果是 Gemini 风格的 functionDeclarations，进去看一眼
                if let Some(decls) = tool.get("functionDeclarations").and_then(|v| v.as_array()) {
                    for decl in decls {
                        if let Some(n) = decl.get("name").and_then(|v| v.as_str()) {
                            let keywords = ["web_search", "google_search", "google_search_retrieval"];
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
        let config = resolve_request_config("gpt-4o", "gemini-2.5-flash", &None);
        assert_eq!(config.request_type, "web_search");
        assert!(config.inject_google_search);
        assert_eq!(config.final_model, "gemini-2.5-flash"); // 修正断言: final_model = mapped_model
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
        let config = resolve_request_config("gemini-3-flash-online", "gemini-3-flash", &None);
        assert_eq!(config.request_type, "web_search");
        assert!(config.inject_google_search);
        assert_eq!(config.final_model, "gemini-3-flash");
    }

    #[test]
    fn test_default_no_grounding() {
        let config = resolve_request_config("claude-sonnet", "gemini-3-flash", &None);
        assert_eq!(config.request_type, "agent");
        assert!(!config.inject_google_search);
    }

    #[test]
    fn test_image_model_excluded() {
        let config = resolve_request_config("gemini-3-pro-image", "gemini-3-pro-image", &None);
        assert_eq!(config.request_type, "image_gen");
        assert!(!config.inject_google_search);
    }
}
