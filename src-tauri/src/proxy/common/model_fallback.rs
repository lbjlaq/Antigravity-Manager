// 模型轮询降级映射模块
// 定义当主模型不可用时的备选模型列表
//
// 设计原则：
// 1. Claude/GPT 模型不可用时，优先切换到 Gemini Pro，再是 Flash
// 2. Gemini 模型之间互为备选
// 3. 智谱 AI 作为最后兜底（需要 z.ai 配置）

/// 模型家族分类
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModelFamily {
    /// Claude 系列 (Opus, Sonnet, Haiku)
    Claude,
    /// GPT 系列 (GPT-4, GPT-5, o1, o3)
    Gpt,
    /// Gemini 系列 (Pro, Flash)
    Gemini,
    /// 智谱系列 (GLM)
    Zhipu,
    /// 其他/未知
    Unknown,
}

/// 根据模型 ID 判断其所属家族
pub fn get_model_family(model: &str) -> ModelFamily {
    let model_lower = model.to_lowercase();
    
    if model_lower.contains("claude") || model_lower.contains("opus") || 
       model_lower.contains("sonnet") || model_lower.contains("haiku") {
        ModelFamily::Claude
    } else if model_lower.contains("gpt") || model_lower.contains("o1") || 
              model_lower.contains("o3") || model_lower.contains("o4") {
        ModelFamily::Gpt
    } else if model_lower.contains("gemini") || model_lower.starts_with("models/") {
        ModelFamily::Gemini
    } else if model_lower.contains("glm") || model_lower.contains("zhipu") {
        ModelFamily::Zhipu
    } else {
        ModelFamily::Unknown
    }
}

/// 获取模型的备选列表
/// 
/// 核心逻辑：
/// - Claude/GPT 模型 -> [Gemini 2.5 Pro, Gemini 2.5 Flash]
/// - Gemini Pro 模型 -> [Gemini 2.5 Flash, Claude Sonnet 4]
/// - Gemini Flash 模型 -> [Gemini 2.5 Pro, Claude Sonnet 4]
/// - 智谱模型 -> [Gemini 2.5 Pro, Gemini 2.5 Flash]
/// 
/// 注意：返回的模型 ID 需要与系统支持的模型名称一致
pub fn get_fallback_models(primary_model: &str) -> Vec<&'static str> {
    let family = get_model_family(primary_model);
    let model_lower = primary_model.to_lowercase();
    
    match family {
        ModelFamily::Claude | ModelFamily::Gpt => {
            // Claude/GPT -> 优先使用 Gemini Pro，然后 Flash
            // 这是主要使用场景：当 Claude 额度用完时切换到 Gemini
            vec![
                "gemini-2.5-pro",
                "gemini-2.5-flash",
            ]
        }
        ModelFamily::Gemini => {
            if model_lower.contains("flash") {
                // Gemini Flash -> 优先使用 Pro，然后 Claude
                vec![
                    "gemini-2.5-pro",
                    "claude-sonnet-4-20250514",
                ]
            } else {
                // Gemini Pro/其他 -> 优先使用 Flash，然后 Claude
                vec![
                    "gemini-2.5-flash",
                    "claude-sonnet-4-20250514",
                ]
            }
        }
        ModelFamily::Zhipu => {
            // 智谱模型 -> 优先使用 Gemini
            vec![
                "gemini-2.5-pro",
                "gemini-2.5-flash",
            ]
        }
        ModelFamily::Unknown => {
            // 未知模型 -> 默认使用 Gemini
            vec![
                "gemini-2.5-pro",
                "gemini-2.5-flash",
            ]
        }
    }
}

/// 获取智谱兜底模型列表
/// 仅当所有其他模型（包括 Gemini）都不可用时使用
/// 需要配置 z.ai 才能使用
pub fn get_zhipu_fallback_models() -> Vec<&'static str> {
    vec![
        "glm-4-plus",
        "glm-4-flash",
    ]
}

/// 判断模型是否为高容量模型（适合用于降级）
/// Gemini 模型通常有更高的配额限制
#[allow(dead_code)]
pub fn is_high_capacity_model(model: &str) -> bool {
    let model_lower = model.to_lowercase();
    
    model_lower.contains("gemini") ||
    model_lower.contains("flash") ||
    model_lower.contains("glm")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_model_family() {
        // Claude 家族
        assert_eq!(get_model_family("claude-opus-4-5-thinking"), ModelFamily::Claude);
        assert_eq!(get_model_family("claude-sonnet-4-20250514"), ModelFamily::Claude);
        assert_eq!(get_model_family("claude-3-5-haiku"), ModelFamily::Claude);
        
        // GPT 家族
        assert_eq!(get_model_family("gpt-4"), ModelFamily::Gpt);
        assert_eq!(get_model_family("gpt-5"), ModelFamily::Gpt);
        assert_eq!(get_model_family("o1-preview"), ModelFamily::Gpt);
        
        // Gemini 家族
        assert_eq!(get_model_family("gemini-2.5-pro"), ModelFamily::Gemini);
        assert_eq!(get_model_family("gemini-2.5-flash"), ModelFamily::Gemini);
        
        // 智谱家族
        assert_eq!(get_model_family("glm-4-plus"), ModelFamily::Zhipu);
    }

    #[test]
    fn test_get_fallback_models_for_claude() {
        let fallbacks = get_fallback_models("claude-opus-4-5-thinking");
        assert!(!fallbacks.is_empty());
        assert!(fallbacks[0].contains("gemini"));
        assert_eq!(fallbacks[0], "gemini-2.5-pro");
        assert_eq!(fallbacks[1], "gemini-2.5-flash");
    }

    #[test]
    fn test_get_fallback_models_for_gemini_pro() {
        let fallbacks = get_fallback_models("gemini-2.5-pro");
        assert!(!fallbacks.is_empty());
        // Gemini Pro 的第一个备选应该是 Flash
        assert!(fallbacks[0].contains("flash"));
    }

    #[test]
    fn test_get_fallback_models_for_gemini_flash() {
        let fallbacks = get_fallback_models("gemini-2.5-flash");
        assert!(!fallbacks.is_empty());
        // Gemini Flash 的第一个备选应该是 Pro
        assert!(fallbacks[0].contains("pro"));
    }
}
