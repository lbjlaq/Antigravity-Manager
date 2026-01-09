// 模型名称映射
use std::collections::HashMap;
use once_cell::sync::Lazy;
use crate::proxy::config::{ModelFallbackPolicy, ModelPriority, ModelStrategy};

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
    m.insert("gemini-3-pro-low", "gemini-3-pro-low");
    m.insert("gemini-3-pro-high", "gemini-3-pro-high");
    m.insert("gemini-3-pro-preview", "gemini-3-pro-preview");
    m.insert("gemini-3-pro", "gemini-3-pro");  // [FIX PR #368] 添加基础模型支持
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
/// 优先级：Custom Mapping (精确/通配) > Group Mapping (家族) > System Mapping (内置插件)
/// 
/// # 参数
/// - `apply_claude_family_mapping`: 是否对 Claude 模型应用家族映射
///   - `true`: CLI 请求，应用家族映射（如 claude-sonnet-4-5 -> gemini-3-pro-high）
///   - `false`: 非 CLI 请求（如 Cherry Studio），跳过家族映射，直接穿透
pub fn resolve_model_route(
    original_model: &str,
    custom_mapping: &std::collections::HashMap<String, String>,
    openai_mapping: &std::collections::HashMap<String, String>,
    anthropic_mapping: &std::collections::HashMap<String, String>,
    apply_claude_family_mapping: bool,
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

    let lower_model = original_model.to_lowercase();

    // 3. 检查家族分组映射 (OpenAI 系)
    // GPT-4 系列 (含 GPT-4 经典, o1, o3 等, 排除 4o/mini/turbo)
    if (lower_model.starts_with("gpt-4") && !lower_model.contains("o") && !lower_model.contains("mini") && !lower_model.contains("turbo")) ||
       lower_model.starts_with("o1-") || lower_model.starts_with("o3-") || lower_model == "gpt-4" {
        if let Some(target) = openai_mapping.get("gpt-4-series") {
            crate::modules::logger::log_info(&format!("[Router] 使用 GPT-4 系列映射: {} -> {}", original_model, target));
            return target.clone();
        }
    }
    
    // GPT-4o / 3.5 系列 (均衡与轻量, 含 4o, mini, turbo)
    if lower_model.contains("4o") || lower_model.starts_with("gpt-3.5") || (lower_model.contains("mini") && !lower_model.contains("gemini")) || lower_model.contains("turbo") {
        if let Some(target) = openai_mapping.get("gpt-4o-series") {
            crate::modules::logger::log_info(&format!("[Router] 使用 GPT-4o/3.5 系列映射: {} -> {}", original_model, target));
            return target.clone();
        }
    }

    // GPT-5 系列 (gpt-5, gpt-5.1, gpt-5.2 等)
    if lower_model.starts_with("gpt-5") {
        // 优先使用 gpt-5-series 映射，如果没有则使用 gpt-4-series
        if let Some(target) = openai_mapping.get("gpt-5-series") {
            crate::modules::logger::log_info(&format!("[Router] 使用 GPT-5 系列映射: {} -> {}", original_model, target));
            return target.clone();
        }
        if let Some(target) = openai_mapping.get("gpt-4-series") {
            crate::modules::logger::log_info(&format!("[Router] 使用 GPT-4 系列映射 (GPT-5 fallback): {} -> {}", original_model, target));
            return target.clone();
        }
    }

    // 4. 检查家族分组映射 (Anthropic 系)
    if lower_model.starts_with("claude-") {
        // 对于内置表中已定义为直通的模型，跳过家族映射，直接返回
        if let Some(mapped) = CLAUDE_TO_GEMINI.get(original_model) {
            if *mapped == original_model {
                crate::modules::logger::log_info(&format!("[Router] 内置直通模型，跳过家族映射: {}", original_model));
                return original_model.to_string();
            }
        }
        
        // Haiku 智能降级策略（仅 CLI 生效）
        if apply_claude_family_mapping && lower_model.contains("haiku") {
            crate::modules::logger::log_info(&format!("[Router] Haiku 智能降级 (CLI): {} -> gemini-2.5-flash-lite", original_model));
            return "gemini-2.5-flash-lite".to_string();
        }

        let family_key = if lower_model.contains("4-5") || lower_model.contains("4.5") {
            "claude-4.5-series"
        } else if lower_model.contains("3-5") || lower_model.contains("3.5") {
            "claude-3.5-series"
        } else {
            "claude-default"
        };

        if let Some(target) = anthropic_mapping.get(family_key) {
            crate::modules::logger::log_warn(&format!("[Router] 使用 Anthropic 系列映射: {} -> {}", original_model, target));
            return target.clone();
        }
        
        // 兜底兼容旧版精确映射
        if let Some(target) = anthropic_mapping.get(original_model) {
            return target.clone();
        }
    }

    // 5. 下沉到系统默认映射逻辑
    let result = map_claude_model_to_gemini(original_model);
    if result != original_model {
        crate::modules::logger::log_info(&format!("[Router] 系统默认映射: {} -> {}", original_model, result));
    }
    result
}

#[derive(Debug, Clone)]
pub struct ModelRoutePlan {
    pub primary: String,
    pub fallbacks: Vec<String>,
    pub policy: ModelFallbackPolicy,
    pub strategy_id: Option<String>,
}

impl ModelRoutePlan {
    pub fn candidates(&self) -> Vec<String> {
        let mut list = Vec::new();
        if !self.primary.is_empty() {
            list.push(self.primary.clone());
        }
        for fb in &self.fallbacks {
            if !fb.is_empty() {
                list.push(fb.clone());
            }
        }
        list
    }

    pub fn max_models(&self) -> usize {
        let count = self.candidates().len();
        match self.policy.max_model_hops {
            Some(hops) if hops > 0 => hops.min(count),
            _ => count.max(1),
        }
    }

    pub fn is_capacity_first(&self) -> bool {
        self.policy.model_priority == ModelPriority::CapacityFirst
    }
}

fn extract_strategy_id(value: &str) -> Option<&str> {
    value.strip_prefix("strategy:")
}

pub fn resolve_model_route_plan(
    original_model: &str,
    custom_mapping: &std::collections::HashMap<String, String>,
    openai_mapping: &std::collections::HashMap<String, String>,
    anthropic_mapping: &std::collections::HashMap<String, String>,
    model_strategies: &std::collections::HashMap<String, ModelStrategy>,
    apply_claude_family_mapping: bool,
) -> ModelRoutePlan {
    let target = resolve_model_route(
        original_model,
        custom_mapping,
        openai_mapping,
        anthropic_mapping,
        apply_claude_family_mapping,
    );

    if let Some(strategy_id) = extract_strategy_id(&target) {
        if let Some(strategy) = model_strategies.get(strategy_id) {
            let mut candidates: Vec<String> = strategy
                .candidates
                .iter()
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty() && !c.starts_with("strategy:"))
                .collect();
            if !candidates.is_empty() {
                let primary = candidates.remove(0);
                return ModelRoutePlan {
                    primary,
                    fallbacks: candidates,
                    policy: strategy.policy.clone(),
                    strategy_id: Some(strategy_id.to_string()),
                };
            }
            crate::modules::logger::log_warn(&format!(
                "[Router] Strategy '{}' has no valid candidates, falling back to default mapping.",
                strategy_id
            ));
        } else {
            crate::modules::logger::log_warn(&format!(
                "[Router] Strategy '{}' not found, falling back to default mapping.",
                strategy_id
            ));
        }
    }

    ModelRoutePlan {
        primary: if target.starts_with("strategy:") {
            map_claude_model_to_gemini(original_model)
        } else {
            target
        },
        fallbacks: Vec::new(),
        policy: ModelFallbackPolicy::default(),
        strategy_id: None,
    }
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

    #[test]
    fn test_strategy_route_plan_resolves_candidates_and_policy() {
        let mut custom_mapping = HashMap::new();
        custom_mapping.insert("gpt-4".to_string(), "strategy:test-strategy".to_string());

        let mut strategies = HashMap::new();
        strategies.insert(
            "test-strategy".to_string(),
            ModelStrategy {
                candidates: vec![
                    "gemini-3-pro-high".to_string(),
                    "gemini-3-flash".to_string(),
                ],
                policy: ModelFallbackPolicy {
                    model_priority: ModelPriority::CapacityFirst,
                    stickiness: crate::proxy::config::ModelStickiness::Weak,
                    max_model_hops: Some(1),
                },
            },
        );

        let plan = resolve_model_route_plan(
            "gpt-4",
            &custom_mapping,
            &HashMap::new(),
            &HashMap::new(),
            &strategies,
            false,
        );

        assert_eq!(plan.primary, "gemini-3-pro-high");
        assert_eq!(plan.fallbacks, vec!["gemini-3-flash".to_string()]);
        assert_eq!(plan.strategy_id.as_deref(), Some("test-strategy"));
        assert!(plan.is_capacity_first());
        assert_eq!(plan.max_models(), 1);
    }

    #[test]
    fn test_strategy_route_plan_missing_strategy_falls_back() {
        let mut custom_mapping = HashMap::new();
        custom_mapping.insert("claude-3-5-sonnet-20241022".to_string(), "strategy:missing".to_string());

        let plan = resolve_model_route_plan(
            "claude-3-5-sonnet-20241022",
            &custom_mapping,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            false,
        );

        assert_eq!(plan.primary, "claude-sonnet-4-5");
        assert!(plan.fallbacks.is_empty());
        assert!(plan.strategy_id.is_none());
    }
}
