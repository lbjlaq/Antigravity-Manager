// 模型名称映射
use std::collections::HashMap;
use once_cell::sync::Lazy;

static CLAUDE_TO_GEMINI: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();

    // 直接支持的模型
    m.insert("claude-opus-4-5-thinking", "claude-opus-4-5-thinking");
    m.insert("claude-sonnet-4-5", "claude-sonnet-4-5");
    m.insert("claude-sonnet-4-5-thinking", "claude-sonnet-4-5-thinking");

    // 别名映射
    m.insert("claude-sonnet-4-5-20250929", "claude-sonnet-4-5-thinking");
    m.insert("claude-3-5-sonnet-20241022", "claude-sonnet-4-5");
    m.insert("claude-3-5-sonnet-20240620", "claude-sonnet-4-5");
    m.insert("claude-opus-4", "claude-opus-4-5-thinking");
    m.insert("claude-opus-4-5-20251101", "claude-opus-4-5-thinking");
    m.insert("claude-haiku-4", "claude-sonnet-4-5");
    m.insert("claude-3-haiku-20240307", "claude-sonnet-4-5");
    m.insert("claude-haiku-4-5-20251001", "claude-sonnet-4-5");
    // OpenAI 协议映射表
    m.insert("gpt-4", "gemini-2.5-pro");
    m.insert("gpt-4-turbo", "gemini-2.5-pro");
    m.insert("gpt-4-turbo-preview", "gemini-2.5-pro");
    m.insert("gpt-4-0125-preview", "gemini-2.5-pro");
    m.insert("gpt-4-1106-preview", "gemini-2.5-pro");
    m.insert("gpt-4-0613", "gemini-2.5-pro");

    m.insert("gpt-4o", "gemini-2.5-pro");
    m.insert("gpt-4o-2024-05-13", "gemini-2.5-pro");
    m.insert("gpt-4o-2024-08-06", "gemini-2.5-pro");

    m.insert("gpt-4o-mini", "gemini-2.5-flash");
    m.insert("gpt-4o-mini-2024-07-18", "gemini-2.5-flash");

    m.insert("gpt-3.5-turbo", "gemini-2.5-flash");
    m.insert("gpt-3.5-turbo-16k", "gemini-2.5-flash");
    m.insert("gpt-3.5-turbo-0125", "gemini-2.5-flash");
    m.insert("gpt-3.5-turbo-1106", "gemini-2.5-flash");
    m.insert("gpt-3.5-turbo-0613", "gemini-2.5-flash");

    // Gemini 协议映射表
    m.insert("gemini-2.5-flash-lite", "gemini-2.5-flash-lite");
    m.insert("gemini-2.5-flash-thinking", "gemini-2.5-flash-thinking");
    m.insert("gemini-3-pro-low", "gemini-3-pro-preview");
    m.insert("gemini-3-pro-high", "gemini-3-pro-preview");
    m.insert("gemini-3-pro-preview", "gemini-3-pro-preview");
    m.insert("gemini-3-pro", "gemini-3-pro-preview");  // [FIX PR #368] 统一映射到 preview
    m.insert("gemini-2.5-flash", "gemini-2.5-flash");
    m.insert("gemini-3-flash", "gemini-3-flash");
    m.insert("gemini-3-pro-image", "gemini-3-pro-image");


    m
});

pub fn map_claude_model_to_gemini(input: &str) -> String {
    // 1. Check exact match in map
    if let Some(mapped) = CLAUDE_TO_GEMINI.get(input) {
        return mapped.to_string();
    }

    // 2. Pass-through known prefixes (gemini-, -thinking) to support dynamic suffixes
    if input.starts_with("gemini-") || input.contains("thinking") {
        return input.to_string();
    }

    // [NEW] Intelligent fallback based on model keywords
    let lower = input.to_lowercase();
    if lower.contains("opus") {
        return "gemini-3-pro-preview".to_string();
    }

    // 3. Fallback to default
    "claude-sonnet-4-5".to_string()
}

/// 获取所有内置支持的模型列表关键字
pub fn get_supported_models() -> Vec<String> {
    CLAUDE_TO_GEMINI.keys().map(|s| s.to_string()).collect()
}

/// 动态获取所有可用模型列表 (包含内置与用户自定义)
pub async fn get_all_dynamic_models(
    custom_mapping: &tokio::sync::RwLock<std::collections::HashMap<String, String>>,
) -> Vec<String> {
    use std::collections::HashSet;
    let mut model_ids = HashSet::new();

    // 1. 获取所有内置映射模型
    for m in get_supported_models() {
        model_ids.insert(m);
    }

    // 2. 获取所有自定义映射模型 (Custom)
    {
        let mapping = custom_mapping.read().await;
        for key in mapping.keys() {
            model_ids.insert(key.clone());
        }
    }

    // 5. 确保包含常用的 Gemini/画画模型 ID
    model_ids.insert("gemini-3-pro-low".to_string());
    
    // [NEW] Issue #247: Dynamically generate all Image Gen Combinations
    let base = "gemini-3-pro-image";
    let resolutions = vec!["", "-2k", "-4k"];
    let ratios = vec!["", "-1x1", "-4x3", "-3x4", "-16x9", "-9x16", "-21x9"];
    
    for res in resolutions {
        for ratio in ratios.iter() {
            let mut id = base.to_string();
            id.push_str(res);
            id.push_str(ratio);
            model_ids.insert(id);
        }
    }

    model_ids.insert("gemini-2.0-flash-exp".to_string());
    model_ids.insert("gemini-2.5-flash".to_string());
    model_ids.insert("gemini-2.5-pro".to_string());
    model_ids.insert("gemini-3-flash".to_string());
    model_ids.insert("gemini-3-pro-high".to_string());
    model_ids.insert("gemini-3-pro-low".to_string());


    let mut sorted_ids: Vec<_> = model_ids.into_iter().collect();
    sorted_ids.sort();
    sorted_ids
}

/// 通配符匹配辅助函数
/// 支持简单的 * 通配符匹配
/// 
/// # 示例
/// - `gpt-4*` 匹配 `gpt-4`, `gpt-4-turbo`, `gpt-4-0613` 等
/// - `claude-3-5-sonnet-*` 匹配所有 3.5 sonnet 版本
/// - `*-thinking` 匹配所有以 `-thinking` 结尾的模型
fn wildcard_match(pattern: &str, text: &str) -> bool {
    if let Some(star_pos) = pattern.find('*') {
        let prefix = &pattern[..star_pos];
        let suffix = &pattern[star_pos + 1..];
        text.starts_with(prefix) && text.ends_with(suffix)
    } else {
        pattern == text
    }
}

/// 核心模型路由解析引擎
/// 优先级：精确匹配 > 通配符匹配 > 系统默认映射
/// 
/// # 参数
/// - `original_model`: 原始模型名称
/// - `custom_mapping`: 用户自定义映射表
/// 
/// # 返回
/// 映射后的目标模型名称
pub fn resolve_model_route(
    original_model: &str,
    custom_mapping: &std::collections::HashMap<String, String>,
) -> String {
    // 1. 精确匹配 (最高优先级)
    if let Some(target) = custom_mapping.get(original_model) {
        crate::modules::logger::log_info(&format!("[Router] 精确映射: {} -> {}", original_model, target));
        return target.clone();
    }
    
    // 2. 通配符匹配
    for (pattern, target) in custom_mapping.iter() {
        if pattern.contains('*') && wildcard_match(pattern, original_model) {
            crate::modules::logger::log_info(&format!("[Router] 通配符映射: {} -> {} (规则: {})", original_model, target, pattern));
            return target.clone();
        }
    }
    
    // 3. 系统默认映射
    let result = map_claude_model_to_gemini(original_model);
    if result != original_model {
        crate::modules::logger::log_info(&format!("[Router] 系统默认映射: {} -> {}", original_model, result));
    }
    result
}

/// Normalize any physical model name to one of the 3 standard protection IDs.
/// This ensures quota protection works consistently regardless of API versioning or request variations.
/// 
/// Standard IDs:
/// - `gemini-3-flash`: All Flash variants (1.5-flash, 2.5-flash, 3-flash, etc.)
/// - `gemini-3-pro-high`: All Pro variants (1.5-pro, 2.5-pro, etc.)
/// - `claude-sonnet-4-5`: All Claude Sonnet variants (3-5-sonnet, sonnet-4-5, etc.)
/// 
/// Returns `None` if the model doesn't match any of the 3 protected categories.
pub fn normalize_to_standard_id(model_name: &str) -> Option<String> {
    let lower = model_name.to_lowercase();
    
    // Gemini Flash variants
    if lower.contains("flash") && lower.contains("gemini") {
        return Some("gemini-3-flash".to_string());
    }
    
    // Gemini Pro variants (including thinking models)
    if lower.contains("gemini") && (lower.contains("pro") || lower.contains("1.5-pro") || lower.contains("2.5-pro")) {
        return Some("gemini-3-pro-preview".to_string());
    }
    
    // Claude Sonnet variants
    if lower.contains("claude") && lower.contains("sonnet") {
        return Some("claude-sonnet-4-5".to_string());
    }
    
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_mapping() {
        assert_eq!(
            map_claude_model_to_gemini("claude-3-5-sonnet-20241022"),
            "claude-sonnet-4-5"
        );
        assert_eq!(
            map_claude_model_to_gemini("claude-opus-4"),
            "claude-opus-4-5-thinking"
        );
        // Test gemini pass-through (should not be caught by "mini" rule)
        assert_eq!(
            map_claude_model_to_gemini("gemini-2.5-flash-mini-test"),
            "gemini-2.5-flash-mini-test"
        );
        assert_eq!(
            map_claude_model_to_gemini("unknown-model"),
            "claude-sonnet-4-5"
        );
    }
}
