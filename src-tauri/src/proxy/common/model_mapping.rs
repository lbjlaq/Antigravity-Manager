// 模型名称映射
use std::collections::HashMap;
use once_cell::sync::Lazy;

static CLAUDE_TO_GEMINI: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();

    // 直接支持的模型
    m.insert("claude-sonnet-4-6-thinking", "claude-sonnet-4-6-thinking");

    // 别名映射 / 重定向
    m.insert("claude-sonnet-4-6", "claude-sonnet-4-6-thinking");
    m.insert("claude-sonnet-4-6-20260219", "claude-sonnet-4-6-thinking");
    
    // Legacy Redirects (Sonnet 4.5 -> 4.6)
    m.insert("claude-sonnet-4-5", "claude-sonnet-4-6-thinking");
    m.insert("claude-sonnet-4-5-thinking", "claude-sonnet-4-6-thinking");
    m.insert("claude-sonnet-4-5-20250929", "claude-sonnet-4-6-thinking");

    m.insert("claude-3-5-sonnet-20241022", "claude-sonnet-4-6-thinking");
    m.insert("claude-3-5-sonnet-20240620", "claude-sonnet-4-6-thinking");

    // [Redirect] Opus 4.5 -> Opus 4.6 (Issue #1743)
    m.insert("claude-opus-4", "claude-opus-4-6-thinking");
    m.insert("claude-opus-4-5-thinking", "claude-opus-4-6-thinking");
    m.insert("claude-opus-4-5-20251101", "claude-opus-4-6-thinking");

    // Claude Opus 4.6
    m.insert("claude-opus-4-6-thinking", "claude-opus-4-6-thinking");
    m.insert("claude-opus-4-6", "claude-opus-4-6-thinking");
    m.insert("claude-opus-4-6-20260201", "claude-opus-4-6-thinking");

    m.insert("claude-haiku-4", "claude-sonnet-4-6-thinking");
    m.insert("claude-3-haiku-20240307", "claude-sonnet-4-6-thinking");
    m.insert("claude-haiku-4-5-20251001", "claude-sonnet-4-6-thinking");

    // OpenAI 协议映射表
    m.insert("gpt-4", "gemini-3.1-flash");
    m.insert("gpt-4-turbo", "gemini-3.1-flash");
    m.insert("gpt-4-turbo-preview", "gemini-3.1-flash");
    m.insert("gpt-4-0125-preview", "gemini-3.1-flash");
    m.insert("gpt-4-1106-preview", "gemini-3.1-flash");
    m.insert("gpt-4-0613", "gemini-3.1-flash");

    m.insert("gpt-4o", "gemini-3.1-flash");
    m.insert("gpt-4o-2024-05-13", "gemini-3.1-flash");
    m.insert("gpt-4o-2024-08-06", "gemini-3.1-flash");

    m.insert("gpt-4o-mini", "gemini-3.1-flash");
    m.insert("gpt-4o-mini-2024-07-18", "gemini-3.1-flash");

    m.insert("gpt-3.5-turbo", "gemini-3.1-flash");
    m.insert("gpt-3.5-turbo-16k", "gemini-3.1-flash");
    m.insert("gpt-3.5-turbo-0125", "gemini-3.1-flash");
    m.insert("gpt-3.5-turbo-1106", "gemini-3.1-flash");
    m.insert("gpt-3.5-turbo-0613", "gemini-3.1-flash");

    // Gemini 协议映射表
    m.insert("gemini-2.5-flash-lite", "gemini-3.1-flash");
    m.insert("gemini-2.5-flash-thinking", "gemini-2.5-flash-thinking");
    m.insert("gemini-3.1-pro-low", "gemini-3.1-pro-preview");
    m.insert("gemini-3.1-pro-high", "gemini-3.1-pro-preview");
    m.insert("gemini-3.1-pro-preview", "gemini-3.1-pro-preview");
    m.insert("gemini-3.1-pro", "gemini-3.1-pro-preview");
    m.insert("gemini-2.5-flash", "gemini-3.1-flash");
    m.insert("gemini-3-flash", "gemini-3.1-flash");
    m.insert("gemini-3.1-flash", "gemini-3.1-flash");
    m.insert("gemini-3-pro-image", "gemini-3-pro-image");

    // [New] Unified Virtual ID for Background Tasks (Title, Summary, etc.)
    m.insert("internal-background-task", "gemini-3.1-flash");


    m
});


/// Map Claude model names to Gemini model names
pub fn map_claude_model_to_gemini(input: &str) -> String {
    if let Some(mapped) = CLAUDE_TO_GEMINI.get(input) {
        return mapped.to_string();
    }
    if input.starts_with("gemini-") || input.contains("thinking") {
        return input.to_string();
    }
    input.to_string()
}

/// 获取所有内置支持的模型列表关键字
pub fn get_supported_models() -> Vec<String> {
    CLAUDE_TO_GEMINI.keys().map(|s| s.to_string()).collect()
}

/// 动态获取所有可用模型列表
pub async fn get_all_dynamic_models(
    custom_mapping: &tokio::sync::RwLock<std::collections::HashMap<String, String>>,
) -> Vec<String> {
    use std::collections::HashSet;
    let mut model_ids = HashSet::new();

    for m in get_supported_models() {
        model_ids.insert(m);
    }

    {
        let mapping = custom_mapping.read().await;
        for key in mapping.keys() {
            model_ids.insert(key.clone());
        }
    }

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

    model_ids.insert("gemini-3.1-flash".to_string());
    model_ids.insert("gemini-3.1-pro-high".to_string());
    model_ids.insert("gemini-3.1-pro-low".to_string());


    let mut sorted_ids: Vec<_> = model_ids.into_iter().collect();
    sorted_ids.sort();
    sorted_ids
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut text_pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !text[text_pos..].starts_with(part) {
                return false;
            }
            text_pos += part.len();
        } else if i == parts.len() - 1 {
            return text[text_pos..].ends_with(part);
        } else {
            if let Some(pos) = text[text_pos..].find(part) {
                text_pos += pos + part.len();
            } else {
                return false;
            }
        }
    }
    true
}

pub fn resolve_model_route(
    original_model: &str,
    custom_mapping: &std::collections::HashMap<String, String>,
) -> String {
    if let Some(target) = custom_mapping.get(original_model) {
        crate::modules::logger::log_info(&format!("[Router] 精确映射: {} -> {}", original_model, target));
        return target.clone();
    }
    let mut best_match: Option<(&str, &str, usize)> = None;
    for (pattern, target) in custom_mapping.iter() {
        if pattern.contains('*') && wildcard_match(pattern, original_model) {
            let specificity = pattern.chars().count() - pattern.matches('*').count();
            if best_match.is_none() || specificity > best_match.unwrap().2 {
                best_match = Some((pattern.as_str(), target.as_str(), specificity));
            }
        }
    }
    if let Some((pattern, target, _)) = best_match {
        crate::modules::logger::log_info(&format!(
            "[Router] Wildcard match: {} -> {} (rule: {})",
            original_model, target, pattern
        ));
        return target.to_string();
    }
    let result = map_claude_model_to_gemini(original_model);
    if result != original_model {
        crate::modules::logger::log_info(&format!("[Router] 系统默认映射: {} -> {}", original_model, result));
    }
    result
}

pub fn normalize_to_standard_id(model_name: &str) -> Option<String> {
    let lower = model_name.to_lowercase();
    if lower.starts_with("gemini-3-pro-image") {
        return Some("gemini-3-pro-image".to_string());
    }
    if lower.contains("flash") {
        return Some("gemini-3.1-flash".to_string());
    }
    if lower.contains("pro") && !lower.contains("image") {
        return Some("gemini-3.1-pro-high".to_string());
    }
    if lower.contains("claude") || lower.contains("opus") || lower.contains("sonnet") || lower.contains("haiku") {
        return Some("claude".to_string());
    }
    None
}
