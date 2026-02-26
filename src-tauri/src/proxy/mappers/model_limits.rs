// 模型输出 Token 限额管理
// 维护静态默认限额表，并提供统一的限额查询入口，供三个协议(OpenAI/Claude/Gemini)共享。
//
// 查询优先级：
//   1. 动态账号数据 (quota.models.max_output_tokens)
//   2. 静态默认表 (DEFAULT_MODEL_OUTPUT_LIMITS)
//   3. 通用兜底值 (131072)

use once_cell::sync::Lazy;
use std::collections::HashMap;

/// 静态默认模型输出限额表
/// 数据来源：Antigravity Manager 官方账号额度数据 (quota.models.max_output_tokens)
/// 后续只需在此处更新，三个协议同步生效。
static DEFAULT_MODEL_OUTPUT_LIMITS: Lazy<HashMap<&'static str, u64>> = Lazy::new(|| {
    let mut m = HashMap::new();

    // --- Gemini 3 系列 ---
    m.insert("gemini-3-flash",            65536);
    m.insert("gemini-3-pro-image",        65535);
    m.insert("gemini-3-pro-high",         65535);
    m.insert("gemini-3-pro-low",          65535);

    // --- Gemini 3.1 Pro 系列 ---
    m.insert("gemini-3.1-pro-preview",    65535);
    m.insert("gemini-3.1-pro-high",       65535);
    m.insert("gemini-3.1-pro-low",        65535);

    // --- Gemini 2.5 系列 ---
    m.insert("gemini-2.5-flash",          65535);
    m.insert("gemini-2.5-flash-thinking", 65535);
    m.insert("gemini-2.5-flash-lite",     65535);
    m.insert("gemini-2.5-pro",            65535);

    // --- Claude 系列 ---
    m.insert("claude-sonnet-4-6",         64000);
    m.insert("claude-opus-4-6-thinking",  64000);

    // --- GPT-OSS 系列 ---
    m.insert("gpt-oss-120b-medium",       32768);

    m
});

/// 通用兜底值：当模型不在静态表且无动态数据时使用
const DEFAULT_FALLBACK_LIMIT: u64 = 131072;

/// 获取模型的输出 Token 限额
///
/// # 参数
/// - `model_name`: 最终映射后的模型名 (final_model_name)
/// - `dynamic_limit`: 从账号额度数据中读取的动态限额 (优先级最高)
///
/// # 优先级
/// 动态账号数据 > 静态默认表 > 131072
///
/// # 示例
/// ```
/// // 账号有动态数据，优先使用
/// assert_eq!(get_model_output_limit("gemini-3-flash", Some(65536)), 65536);
///
/// // 无动态数据，使用静态默认表
/// assert_eq!(get_model_output_limit("gemini-3-flash", None), 65536);
///
/// // 模型不在静态表，使用兜底值
/// assert_eq!(get_model_output_limit("unknown-future-model", None), 131072);
/// ```
pub fn get_model_output_limit(model_name: &str, dynamic_limit: Option<u64>) -> u64 {
    dynamic_limit
        .or_else(|| DEFAULT_MODEL_OUTPUT_LIMITS.get(model_name).copied())
        .unwrap_or(DEFAULT_FALLBACK_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dynamic_priority() {
        // 动态数据优先，即使静态表有值也使用动态
        assert_eq!(get_model_output_limit("gemini-3-flash", Some(50000)), 50000);
    }

    #[test]
    fn test_static_fallback() {
        // 无动态数据时使用静态默认表
        assert_eq!(get_model_output_limit("gemini-3-flash", None), 65536);
        assert_eq!(get_model_output_limit("gemini-2.5-flash", None), 65535);
        assert_eq!(get_model_output_limit("claude-sonnet-4-6", None), 64000);
        assert_eq!(get_model_output_limit("gpt-oss-120b-medium", None), 32768);
    }

    #[test]
    fn test_global_fallback() {
        // 未知模型使用兜底值
        assert_eq!(get_model_output_limit("unknown-future-model", None), 131072);
        assert_eq!(get_model_output_limit("gemini-4-ultra", None), 131072);
    }

    #[test]
    fn test_dynamic_overrides_static() {
        // 账号动态数据与静态表不同时，动态数据优先
        assert_eq!(get_model_output_limit("claude-opus-4-6-thinking", Some(80000)), 80000);
    }
}
