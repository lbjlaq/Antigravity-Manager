use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use once_cell::sync::Lazy;
use crate::proxy::token_manager::ProxyToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpec {
    pub max_output_tokens: Option<u64>,
    pub thinking_budget: Option<u64>,
    pub is_thinking: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpecsConfig {
    models: HashMap<String, ModelSpec>,
    aliases: HashMap<String, String>,
}

static SPECS: Lazy<SpecsConfig> = Lazy::new(|| {
    let json_str = include_str!("../../resources/model_specs.json");
    serde_json::from_str(json_str).expect("Failed to parse model_specs.json")
});

/// 获取归一化后的模型 ID (基于别名)
pub fn resolve_alias(model_id: &str) -> String {
    SPECS.aliases.get(model_id).cloned().unwrap_or_else(|| model_id.to_string())
}

/// 获取模型输出 Token 限额 (动态优先)
pub fn get_max_output_tokens(model_id: &str, token: Option<&ProxyToken>) -> u64 {
    let std_id = resolve_alias(model_id);
    
    // 1. 尝试从账号动态数据中读取
    if let Some(t) = token {
        if let Some(&limit) = t.model_limits.get(&std_id) {
            return limit;
        }
        // 如果原始 ID 没找到，尝试用归一化后的 ID 找
        if let Some(&limit) = t.model_limits.get(model_id) {
            return limit;
        }
    }
    
    // 2. 回退到静态 JSON
    if let Some(spec) = SPECS.models.get(&std_id) {
        if let Some(limit) = spec.max_output_tokens {
            return limit;
        }
    }

    // 3. 全局兜底
    65535
}

/// 获取思维链预算 (动态优先)
pub fn get_thinking_budget(model_id: &str, _token: Option<&ProxyToken>) -> u64 {
    let std_id = resolve_alias(model_id);
    
    // 1. 优先尝试从 token 的 quota 信息中推断 (如果以后 quota 返回了具体 budget)
    // 目前 ProxyToken 结构体暂未直接缓存每个模型的 thinking_budget，
    // 但可以通过 model_limits 比例或直接从 JSON 补全。
    
    // 2. 静态 JSON 配置
    if let Some(spec) = SPECS.models.get(&std_id) {
        if let Some(budget) = spec.thinking_budget {
            return budget;
        }
    }

    // 3. 默认安全限额
    24576
}

/// 判断是否为思维模型
#[allow(dead_code)]
pub fn is_thinking_model(model_id: &str) -> bool {
    let std_id = resolve_alias(model_id);
    if let Some(spec) = SPECS.models.get(&std_id) {
        return spec.is_thinking.unwrap_or(false);
    }
    model_id.contains("-thinking") || model_id.contains("thinking")
}
