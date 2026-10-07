use crate::proxy::token_manager::ProxyToken;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    SPECS
        .aliases
        .get(model_id)
        .cloned()
        .unwrap_or_else(|| model_id.to_string())
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

/// 获取思维链预算 (动态优先，根据模型 ID 档位字典自动填充)
pub fn get_thinking_budget(model_id: &str, _token: Option<&ProxyToken>) -> u64 {
    let lower_raw = model_id.to_lowercase();
    if is_bare_gemini_v3_flash(&lower_raw) {
        return 4000;
    }
    let std_id = resolve_alias(model_id);
    let lower = std_id.to_lowercase();

    // 1. 显式档位匹配（支持 Flash / Pro 的 high, medium, low, extra-low, max 等分级）
    if lower.contains("high") || lower.contains("agent") || lower.contains("max") {
        if lower.contains("pro") {
            return 10001; // Google 官方 gemini-3.1-pro-high / gemini-pro-agent 规范预算
        }
        return 10000; // gemini-3.x-flash-high 满血思考规范预算
    }
    if lower.contains("medium") {
        return 4000; // gemini-3.x-flash-medium 内置逆向规范预算
    }
    if lower.contains("extra-low") {
        return 1000; // gemini-3.5-flash-extra-low 规范预算
    }
    if lower.contains("low") {
        if lower.contains("pro") {
            return 1001; // gemini-3.1-pro-low 规范预算
        }
        return 1000; // gemini-3.x-flash-low 规范预算
    }

    // 2. 静态 JSON 配置 (model_specs.json)
    if let Some(spec) = SPECS.models.get(&std_id) {
        if let Some(budget) = spec.thinking_budget {
            return budget;
        }
    }

    // 3. 所有 Gemini >= 3.0 未带显式后缀的 Flash 模型：
    // medium 或者不带档位后缀，统一默认赋予 4000（内置逆向标准）
    if is_gemini_v3_or_above(&std_id) && lower.contains("flash") {
        return 4000;
    }

    // 4. 传统模型系列默认限额
    if lower.contains("claude") {
        16384
    } else if lower.contains("2.5-flash") || lower.contains("2.0-flash") {
        24576
    } else if lower.contains("pro") {
        49152
    } else {
        24576
    }
}

/// 判断是否为 >= 3.0 的无后缀 Gemini Pro 裸模型（如 gemini-3.1-pro, gemini-3-pro 等）
pub fn is_bare_gemini_pro(model: &str) -> bool {
    let lower = model.to_lowercase();
    if !lower.contains("gemini") || !lower.contains("pro") || lower.contains("flash") {
        return false;
    }
    if !is_gemini_v3_or_above(model) {
        return false;
    }
    !(lower.ends_with("-high")
        || lower.ends_with("-medium")
        || lower.ends_with("-low")
        || lower.ends_with("-extra-low")
        || lower.ends_with("-tiered")
        || lower.ends_with("-preview")
        || lower.ends_with("-agent")
        || lower.ends_with("-thinking")
        || lower.ends_with("-image")
        || lower.contains("-high-")
        || lower.contains("-medium-")
        || lower.contains("-low-")
        || lower.contains("thinking")
        || lower.contains("exp"))
}

/// 判断模型是否命中显式启发式档位后缀（-high, -medium, -low, -extra-low, -max, -agent, -thinking 等）
pub fn is_explicit_heuristic_tier_model(model_id: &str) -> bool {
    let lower_raw = model_id.to_lowercase();
    if is_bare_gemini_v3_flash(&lower_raw) || is_bare_gemini_pro(&lower_raw) {
        return false;
    }
    let std_id = resolve_alias(model_id);
    let lower = std_id.to_lowercase();
    lower_raw.ends_with("-high")
        || lower_raw.ends_with("-medium")
        || lower_raw.ends_with("-low")
        || lower_raw.ends_with("-extra-low")
        || lower_raw.ends_with("-max")
        || lower_raw.ends_with("-thinking")
        || lower_raw.ends_with("-agent")
        || lower_raw.contains("-high-")
        || lower_raw.contains("-medium-")
        || lower_raw.contains("-low-")
        || lower_raw.contains("-extra-low-")
        || lower_raw.contains("-max-")
        || lower_raw.contains("-agent")
        || lower_raw.contains("-thinking")
        || (!is_bare_gemini_v3_flash(&lower)
            && !is_bare_gemini_pro(&lower)
            && (lower.ends_with("-high")
                || lower.ends_with("-medium")
                || lower.ends_with("-low")
                || lower.ends_with("-extra-low")
                || lower.ends_with("-max")
                || lower.ends_with("-thinking")
                || lower.ends_with("-agent")
                || lower.contains("-high-")
                || lower.contains("-medium-")
                || lower.contains("-low-")
                || lower.contains("-extra-low-")
                || lower.contains("-max-")
                || lower.contains("-agent")
                || lower.contains("-thinking")))
}

/// 权威解析思维链预算（全协议统一：处理启发式模型强制锁死 vs 裸模型接管客户端 effort）
pub fn resolve_authoritative_thinking_budget(
    model: &str,
    client_effort: Option<&str>,
    _client_budget: Option<u64>,
    token: Option<&ProxyToken>,
) -> u64 {
    // 1. 若为 Gemini < 3 的非思考模型，返回 0
    if is_gemini_under_v3(model) {
        return 0;
    }

    // 2. 启发式模型：绝对最高优先级，根据后缀字典强制返回，彻底忽略客户端 effort
    if is_explicit_heuristic_tier_model(model) {
        return get_thinking_budget(model, token);
    }

    // 3. 非 Gemini 3 系列（例如纯 Claude 或其他传统模型），走传统默认
    if !is_gemini_v3_or_above(model) && !model.to_lowercase().contains("gemini") {
        return get_thinking_budget(model, token);
    }

    // 4. 裸模型（不带显式档位后缀，如 gemini-3-flash, gemini-3.1-pro 等）
    let std_id = resolve_alias(model);
    let lower = std_id.to_lowercase();
    let is_pro = lower.contains("pro");

    // 由客户端 effort / thinkingLevel 接管：
    // high / max / xhigh → 10000 / 10001
    // medium / default / json字段不填 → 4000 / 10001
    // low / extra-low → 1000 / 1001
    // 若客户端试图关闭思考 (none / 0 / disabled) 或未传：绝不关闭思考，强制按 -medium 字典预算填充兜底！
    if let Some(effort) = client_effort {
        let eff_lower = effort.trim().to_lowercase();
        match eff_lower.as_str() {
            "high" | "max" | "xhigh" => {
                if is_pro {
                    10001
                } else {
                    10000
                }
            }
            "low" | "extra-low" => {
                if is_pro {
                    1001
                } else {
                    1000
                }
            }
            "medium" | "default" => {
                if is_pro {
                    10001
                } else {
                    4000
                }
            }
            "none" | "0" | "disabled" => {
                // 试图关闭思考：绝不关闭！强制以 -medium 字典预算填充兜底
                if is_pro {
                    10001
                } else {
                    4000
                }
            }
            _ => {
                // 未知 effort，默认 -medium 字典预算
                if is_pro {
                    10001
                } else {
                    4000
                }
            }
        }
    } else {
        // 客户端未传 effort 或 json 字段不填：默认以 -medium 字典预算填充兜底
        if is_pro {
            10001
        } else {
            4000
        }
    }
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

/// 判断是否为 Gemini 且主版本号 < 3.0 的模型（例如 gemini-2.5-flash, gemini-2.5-pro, gemini-2.0-flash, gemini-1.5-pro 等）
/// 此类模型在 Google 官方 API 上不支持 thinkingConfig / 思考参数，严禁注入思考配置，严禁回填思考块与哨兵签名。
pub fn is_gemini_under_v3(model: &str) -> bool {
    let lower = model.to_lowercase();
    if !lower.contains("gemini") {
        return false;
    }
    // 特殊别名/代理模型：gemini-pro-agent / gemini-flash-agent 属于 gemini-3 体系；-exp / thinking-exp 属于官方思维实验模型
    if lower.contains("agent")
        || lower.contains("-exp")
        || lower.contains("thinking-exp")
        || lower.contains("-thinking")
    {
        return false;
    }
    // 检查明确的 gemini-X 形式
    if let Some(idx) = lower.find("gemini-") {
        let rest = &lower[idx + "gemini-".len()..];
        let version_part: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if let Ok(ver) = version_part.parse::<f32>() {
            return ver < 3.0;
        }
    }
    // 兜底兼容 gemini-1 / gemini-2 形式
    lower.contains("gemini-1") || lower.contains("gemini-2")
}

/// 判断是否为 Gemini 3 及以上版本的模型（例如 gemini-3, gemini-3.1, gemini-3.7, gemini-3.8 等）
/// 此类模型支持并强行/默认开启思考模式。
pub fn is_gemini_v3_or_above(model: &str) -> bool {
    let lower = model.to_lowercase();
    if !lower.contains("gemini") {
        return false;
    }
    if lower.contains("agent") || lower == "gemini-pro" || lower == "gemini-flash" {
        return true;
    }
    if let Some(idx) = lower.find("gemini-") {
        let rest = &lower[idx + "gemini-".len()..];
        let version_part: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if let Ok(ver) = version_part.parse::<f32>() {
            return ver >= 3.0;
        }
    }
    lower.contains("gemini-3") || lower.contains("gemini-4")
}

/// 判断是否为 Tiered Flash 模型（如 gemini-3.8-flash-tiered）
pub fn is_tiered_flash_model(model: &str) -> bool {
    let model_id = model
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .to_ascii_lowercase();
    model_id
        .strip_prefix("gemini-")
        .and_then(|rest| rest.strip_suffix("-flash-tiered"))
        .is_some_and(|version| !version.is_empty())
}

/// 判断是否为 >= 3.0 的无后缀 Flash 衍生模型（如 gemini-3-flash, gemini-3.5-flash, gemini-3.6-flash, gemini-3.7-flash, gemini-3.8-flash, gemini-3.9-flash 等）
pub fn is_bare_gemini_v3_flash(model: &str) -> bool {
    let lower = model.to_lowercase();
    if !lower.contains("gemini") || !lower.contains("flash") {
        return false;
    }
    // 排除已有任何后缀或变体标记的模型
    if lower.ends_with("-high")
        || lower.ends_with("-medium")
        || lower.ends_with("-low")
        || lower.ends_with("-extra-low")
        || lower.ends_with("-tiered")
        || lower.ends_with("-preview")
        || lower.ends_with("-agent")
        || lower.ends_with("-thinking")
        || lower.ends_with("-image")
        || lower.ends_with("-lite")
        || lower.contains("-high-")
        || lower.contains("-medium-")
        || lower.contains("-low-")
        || lower.contains("-tiered-")
    {
        return false;
    }
    // 检查版本号是否 >= 3.0
    if let Some(idx) = lower.find("gemini-") {
        let rest = &lower[idx + "gemini-".len()..];
        let version_part: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if let Ok(ver) = version_part.parse::<f32>() {
            return ver >= 3.0;
        }
    }
    lower == "gemini-3-flash"
}

/// 兼容旧接口别名
pub fn is_bare_gemini_v36_or_above_flash(model: &str) -> bool {
    is_bare_gemini_v3_flash(model)
}

/// 检查是否为 Claude >= 5.0 版本模型（含裸模型与档位变体）
pub fn is_claude_v5_or_above(model: &str) -> bool {
    let lower = model.to_lowercase();
    if !lower.contains("claude") {
        return false;
    }
    if let Some(pos) = lower.find("claude") {
        let rest = &lower[pos..];
        let tokens: Vec<&str> = rest
            .split(|c: char| c == '-' || c == '_' || c == '.')
            .collect();
        for window in tokens.windows(2) {
            if let (Ok(major), Ok(minor)) = (window[0].parse::<u32>(), window[1].parse::<u32>()) {
                if major < 1000 && minor < 1000 {
                    return (major, minor) >= (5, 0);
                }
            }
        }
        if let Some(ver) = crate::proxy::common::model_mapping::parse_version_tuple(rest) {
            return ver >= (5, 0);
        }
    }
    false
}

/// 检查是否为 Claude >= 5.0 的裸模型（无 -low, -medium, -high 等档位或特性后缀）
pub fn is_bare_claude_tiered_model(model: &str) -> bool {
    let lower = model.to_lowercase();
    if !is_claude_v5_or_above(model) {
        return false;
    }
    !(lower.ends_with("-high")
        || lower.ends_with("-medium")
        || lower.ends_with("-low")
        || lower.ends_with("-extra-low")
        || lower.ends_with("-thinking")
        || lower.ends_with("-tiered")
        || lower.contains("-high-")
        || lower.contains("-medium-")
        || lower.contains("-low-"))
}

/// 档位权重定义：
/// lite / extra-low (0) < low (1) < default (2) < medium (3) < high (4) < xhigh (5) < max (6)
pub fn tier_weight(tier: &str) -> i32 {
    let clean = tier.trim().to_lowercase().replace('_', "-");
    match clean.as_str() {
        "lite" | "flash-lite" | "extra-low" | "minimal" => 0,
        "low" => 1,
        "default" => 2,
        "medium" => 3,
        "high" => 4,
        "xhigh" | "x-high" | "extreme" => 5,
        "max" => 6,
        _ => 3, // 未知档位默认权重等同 medium
    }
}

/// 根据默认档位决策原则，从可用档位数组中自适应选取默认档位：
/// 1. 优先找 tiered 模型（如存在 -tiered，走自适应思考）
/// 2. 其次找 medium
/// 3. 再次找大于 low 的最低等级（如在 ["low", "high"] 中命中 high）
/// 4. 否则若有等于 low 且确实存在时取 low
/// 5. 极端保底：仅从真实存在的可用档位中取首项，绝不虚构不存在的档位
pub fn pick_optimal_default_tier(available_tiers: &[String]) -> Option<String> {
    if available_tiers.is_empty() {
        return None;
    }

    // 1. 优先找 tiered 模型
    if let Some(t) = available_tiers
        .iter()
        .find(|t| t.eq_ignore_ascii_case("tiered"))
    {
        return Some(t.clone());
    }

    // 2. 其次找 medium
    if let Some(t) = available_tiers
        .iter()
        .find(|t| t.eq_ignore_ascii_case("medium"))
    {
        return Some(t.clone());
    }

    // 3. 再次找权重严格大于 low (weight > 1) 的最低等级
    let low_weight = tier_weight("low");
    let mut candidates_above_low: Vec<(&String, i32)> = available_tiers
        .iter()
        .map(|t| (t, tier_weight(t)))
        .filter(|(_, w)| *w > low_weight)
        .collect();

    if !candidates_above_low.is_empty() {
        // 取大于 low 的最低权重项（例如 high 权重为 4）
        candidates_above_low.sort_by_key(|(_, w)| *w);
        return Some(candidates_above_low[0].0.clone());
    }

    // 4. 若无大于 low 的等级，且 low 真实存在，取 low
    if let Some(t) = available_tiers
        .iter()
        .find(|t| t.eq_ignore_ascii_case("low"))
    {
        return Some(t.clone());
    }

    // 5. 极端保底：仅从真实存在的可用档位中取第一个，绝不虚构不存在的档位
    available_tiers.first().cloned()
}

/// 纯通用裸模型分档路由器 (Dynamic Tier Router)
/// 彻底消除品牌特判分支，完全由上游 OfficialModelCatalog 的可用档位数组与权重梯队驱动
pub struct DynamicTierRouter;

impl DynamicTierRouter {
    pub fn resolve(model: &str, client_effort: Option<&str>) -> Option<String> {
        let clean = model.trim().to_lowercase();

        // 1. 若当前模型名本身已经带有档位后缀（如 -high, -low, -tiered 等），说明已经是指向具名变体，不在此解析
        if clean.ends_with("-high")
            || clean.ends_with("-medium")
            || clean.ends_with("-low")
            || clean.ends_with("-extra-low")
            || clean.ends_with("-tiered")
            || clean.ends_with("-lite")
            || clean.ends_with("-agent")
        {
            return None;
        }

        // 规范化 base 名称（例如对于 claude-sonnet-5.5 规范化为 claude-sonnet-5-5）
        let base = if clean.contains("claude") {
            crate::proxy::common::model_mapping::canonicalize_claude_client_model_id(&clean)
        } else if clean == "gemini-3-flash" {
            "gemini-3.8-flash".to_string()
        } else {
            clean.clone()
        };

        // 1.5 过滤非活跃或未达基准线的模型（淘汰模型由系统映射重定向接管，不在此动态衍生）
        if !crate::proxy::common::model_mapping::is_model_compliant_with_baseline(&base) {
            return None;
        }

        // 2. 动态收集属于该 base 模型的所有可用档位后缀
        let available_tiers = crate::models::OfficialModelCatalog::collect_tiers_for_base(&base);

        if available_tiers.is_empty() {
            return None;
        }

        // 3. 客户端显式指定了思考强度：
        if let Some(eff_str) = client_effort.and_then(normalize_client_thinking_level) {
            let target_tier = eff_str.to_lowercase();
            // 若可用档位中精确包含该档位，直接命中
            if let Some(matched) = available_tiers
                .iter()
                .find(|t| t.eq_ignore_ascii_case(&target_tier))
            {
                if base.contains("3.5") && matched == "medium" {
                    return Some("gemini-3.5-flash".to_string());
                }
                return Some(format!("{}-{}", base, matched));
            }

            // 若客户端传了 high 但无 high，或传了 low 但无 low：
            // 必须严格在真实存在的 available_tiers 中按权重就近匹配，绝不虚构不存在的档位
            let target_w = tier_weight(&target_tier);
            if let Some(closest) = available_tiers
                .iter()
                .min_by_key(|t| (tier_weight(t) - target_w).abs())
            {
                if base.contains("3.5") && closest == "medium" {
                    return Some("gemini-3.5-flash".to_string());
                }
                return Some(format!("{}-{}", base, closest));
            }
            return None;
        }

        // 4. 客户端未指定思考强度：按统一默认档位决策原则自动选取
        // 遵循原则：tiered 优先 -> medium 次之 -> 向上取高于 low 的最低档位 -> 存在时保底 low
        let default_tier = match pick_optimal_default_tier(&available_tiers) {
            Some(t) => t,
            None => return None,
        };

        if base.contains("3.5") && (default_tier == "medium" || default_tier == "high") {
            return Some("gemini-3.5-flash".to_string());
        }

        Some(format!("{}-{}", base, default_tier))
    }
}

/// 通用裸模型档位路由器 (Unified Bare Model Tier Router)
pub fn resolve_bare_tiered_model_route(model: &str, client_effort: Option<&str>) -> Option<String> {
    DynamicTierRouter::resolve(model, client_effort)
}

/// 将 3.x Flash 裸模型（如 gemini-3.8-flash, gemini-3.7-flash 等）依据客户端思考档位解析为具名模型（兼容封装）
pub fn resolve_bare_flash_route(model: &str, client_effort: Option<&str>) -> Option<String> {
    DynamicTierRouter::resolve(model, client_effort)
}

/// 依据系统 Thinking Budget 配置以及当前模型与请求参数，在协议归一化后统一解析应当发送到上游的思考预算。
/// 返回：
/// 归一化客户端上送的思考等级字段（包括 max, xhigh, high, medium, low, min, extra-low, extra_low 等）
pub fn normalize_client_thinking_level(effort: &str) -> Option<&'static str> {
    let clean = effort.trim().to_lowercase().replace('_', "-");
    match clean.as_str() {
        "low" | "extra-low" | "min" | "minimal" | "lite" | "flash-lite" | "disabled" | "off"
        | "none" => Some("LOW"),
        "medium" | "normal" | "standard" => Some("MEDIUM"),
        "high" | "xhigh" | "x-high" | "max" | "extreme" => Some("HIGH"),
        _ => None,
    }
}

/// 解析权威思考链 Token 预算
/// - Some(budget): 表示需要向上游注入具体数值的 thinkingBudget
/// - None: 表示不注入 thinkingBudget，仅开启 includeThoughts（官方自适应模式，或配置为 -1 的自适应档位）
pub fn resolve_custom_budget(
    model: &str,
    client_effort: Option<&str>,
    client_budget: Option<u64>,
    tb_config: &crate::proxy::config::ThinkingBudgetConfig,
    token: Option<&ProxyToken>,
) -> Option<i64> {
    use crate::proxy::config::{ThinkingBudgetMode, ThinkingControlSource};

    // 1. 若为 Gemini < 3 的非思考模型，返回 None（不支持任何思考配置）
    if is_gemini_under_v3(model) {
        return None;
    }

    // 2. 顶级大选择：客户端直接控制（危险模式）
    if tb_config.control_source == ThinkingControlSource::Client {
        if let Some(b) = client_budget {
            return Some(b as i64);
        }
        return None;
    }

    // 3. 顶级大选择：网关权威控制
    let std_id = resolve_alias(model);
    let lower = std_id.to_lowercase();
    let is_claude = lower.contains("claude");
    let is_legacy_thinking = lower.contains("gemini") && lower.contains("thinking");

    // 3.0 早期思维实验模型（如 gemini-2.0-flash-thinking, gemini-2.0-flash-thinking-exp, gemini-2.0-pro-thinking-exp 等）
    if is_legacy_thinking {
        let max_cap = get_thinking_budget(&std_id, token) as i64;
        if tb_config.mode == ThinkingBudgetMode::Custom
            && tb_config.custom_value != 24576
            && tb_config.custom_value > 0
        {
            let custom = tb_config.custom_value as i64;
            return Some(if custom > max_cap { max_cap } else { custom });
        }
        if max_cap > 0 {
            return Some(max_cap);
        } else {
            return None;
        }
    }

    let is_pro = lower.contains("pro");
    let is_flash =
        is_tiered_flash_model(&std_id) || lower.contains("flash") || is_gemini_v3_or_above(&std_id);

    // 兼容历史单值测试逻辑：仅针对未指定显式档位后缀且非 tiered 的裸模型生效
    if tb_config.mode == ThinkingBudgetMode::Custom
        && tb_config.custom_value != 24576
        && tb_config.custom_value > 0
        && !is_tiered_flash_model(&std_id)
        && !lower.contains("-high")
        && !lower.contains("-low")
        && !lower.contains("-medium")
    {
        return Some(tb_config.custom_value as i64);
    }

    // 3.1 Claude 系列
    if is_claude {
        if tb_config.claude_mode == ThinkingBudgetMode::Default {
            return None;
        }
        let eff = client_effort.map(|s| s.trim().to_lowercase().replace('_', "-"));
        let has_explicit_suffix = lower.contains("-low")
            || lower.contains("haiku")
            || lower.contains("-med")
            || lower.contains("-medium")
            || lower.contains("-high")
            || lower.contains("-max");

        let is_low = lower.contains("-low")
            || lower.contains("haiku")
            || (!has_explicit_suffix && matches!(eff.as_deref(), Some("low") | Some("extra-low")));
        let is_med = lower.contains("-med")
            || lower.contains("-medium")
            || (!has_explicit_suffix && matches!(eff.as_deref(), Some("medium") | Some("default")));
        let is_high = lower.contains("-high")
            || lower.contains("-max")
            || (!has_explicit_suffix
                && matches!(
                    eff.as_deref(),
                    Some("high") | Some("max") | Some("xhigh") | Some("x-high")
                ));

        if is_low {
            if tb_config.claude_low > 0 {
                Some(tb_config.claude_low as i64)
            } else {
                None
            }
        } else if is_med {
            if tb_config.claude_medium > 0 {
                Some(tb_config.claude_medium as i64)
            } else {
                None
            }
        } else if is_high {
            if tb_config.claude_high > 0 {
                Some(tb_config.claude_high as i64)
            } else {
                None
            }
        } else {
            // 针对未指定档位后缀的 Claude 思考模型（如 claude-sonnet-4-6、claude-opus-4-6-thinking）：
            // claude_budget=0 表示"未显式配置，使用官方模型结构体默认值"（对标 flash_high=-1 语义）。
            // 返回 None 时，configure_inbound_thinking 会 fallback 到 official_info.thinking_budget（官方默认 1024）。
            if tb_config.claude_budget > 0 {
                Some(tb_config.claude_budget as i64)
            } else if tb_config.claude_budget < 0 {
                None // claude_budget < 0 → 上游自适应
            } else {
                None // claude_budget = 0 → 使用官方模型结构体默认值
            }
        }
    } else if is_pro {
        // 3.2 Gemini Pro 系列（与 Gemini 系列统一配置，继承 Low 与 High 档位）
        if tb_config.pro_mode == ThinkingBudgetMode::Default
            || tb_config.flash_mode == ThinkingBudgetMode::Default
        {
            return None;
        }
        let eff = client_effort.map(|s| s.trim().to_lowercase().replace('_', "-"));
        let has_explicit_suffix = lower.contains("-low")
            || lower.ends_with("-low")
            || lower.contains("-high")
            || lower.ends_with("-high");

        let is_low = lower.contains("-low")
            || lower.ends_with("-low")
            || (!has_explicit_suffix && matches!(eff.as_deref(), Some("low") | Some("extra-low")));

        if is_low {
            if tb_config.pro_low > 0 {
                Some(tb_config.pro_low as i64)
            } else if tb_config.flash_low > 0 {
                Some(tb_config.flash_low as i64)
            } else {
                None
            }
        } else {
            // High 档位（包含未指定 effort、medium 等，均由 High 档位接管保证 Pro 深度推理）
            if tb_config.pro_high > 0 {
                Some(tb_config.pro_high as i64)
            } else if tb_config.flash_high > 0 {
                Some(tb_config.flash_high as i64)
            } else {
                None
            }
        }
    } else if is_flash {
        // 3.3 Gemini Flash 系列
        if is_tiered_flash_model(&std_id) || lower.contains("tiered") {
            // [TIERED-THINKING] Tiered 自适应模型：
            // 不动模型名，严格按照 tiered 模型的 low / high / medium 的预算填写
            let client_level = client_effort.and_then(normalize_client_thinking_level);
            if let Some(level) = client_level {
                match level {
                    "LOW" => {
                        if tb_config.flash_low > 0 {
                            Some(tb_config.flash_low as i64)
                        } else {
                            Some(1000) // 官方 low 默认值 1000
                        }
                    }
                    "HIGH" => {
                        if tb_config.flash_high > 0 {
                            Some(tb_config.flash_high as i64)
                        } else {
                            Some(-1) // 官方 high 默认值 -1 (无上限自适应深度思考)
                        }
                    }
                    _ => {
                        // MEDIUM
                        if tb_config.flash_medium > 0 {
                            Some(tb_config.flash_medium as i64)
                        } else {
                            Some(4000) // 官方 medium 默认值 4000
                        }
                    }
                }
            } else {
                // 客户端未传思考参数：看网关 flash_tiered 配置，未配或为 -1 则走官方默认 -1
                if tb_config.flash_mode == ThinkingBudgetMode::Default {
                    Some(-1)
                } else if tb_config.flash_tiered > 0 {
                    Some(tb_config.flash_tiered as i64)
                } else {
                    Some(-1)
                }
            }
        } else {
            // [NON-TIERED FLASH] 具名非 Tiered 模型（如 gemini-3.8-flash-high, gemini-3.8-flash-low, gemini-3.8-flash-medium 等）：
            // 1. 若网关配置为 Default 默认模式，返回 None，交由 configure_inbound_thinking 回落到官方模型结构体默认值
            if tb_config.flash_mode == ThinkingBudgetMode::Default {
                return None;
            }

            // 2. 根据模型自身后缀或 client_effort 匹配档位配置：
            // 模型名后缀（-low, -medium, -high）具有绝对最高优先级，仅在 bare 裸模型时才使用 client_effort 映射！
            let is_bare = is_bare_gemini_v3_flash(model);
            let eff_level = client_effort.and_then(normalize_client_thinking_level);

            let is_low = lower.contains("-low")
                || lower.ends_with("-low")
                || lower.contains("-extra-low")
                || (is_bare && matches!(eff_level, Some("LOW")));

            let is_medium = lower.contains("-medium")
                || lower.ends_with("-medium")
                || (is_bare && matches!(eff_level, Some("MEDIUM")));

            let is_high = lower.contains("-high")
                || lower.ends_with("-high")
                || lower.contains("-max")
                || lower.contains("agent")
                || (is_bare && matches!(eff_level, Some("HIGH")));

            if is_low {
                if tb_config.flash_low > 0 {
                    Some(tb_config.flash_low as i64)
                } else {
                    Some(1000) // 官方 low 默认值 1000
                }
            } else if is_medium {
                if tb_config.flash_medium > 0 {
                    Some(tb_config.flash_medium as i64)
                } else {
                    Some(4000) // 官方 medium 默认值 4000
                }
            } else if is_high {
                if tb_config.flash_high > 0 {
                    Some(tb_config.flash_high as i64)
                } else {
                    Some(-1) // 官方 high 默认值 -1
                }
            } else {
                // 未指定任何档位的默认平衡档位 (Medium)
                if tb_config.flash_medium > 0 {
                    Some(tb_config.flash_medium as i64)
                } else {
                    Some(4000) // 官方 medium 默认值 4000
                }
            }
        }
    } else {
        // 3.4 传统模型（如 gemini-2.0-flash-thinking-exp 等）
        let b = get_thinking_budget(&std_id, token);
        if b > 0 {
            Some(b as i64)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gemini_version_checks() {
        assert!(is_gemini_under_v3("gemini-2.5-flash"));
        assert!(is_gemini_under_v3("gemini-2.5-flash-lite"));
        assert!(is_gemini_under_v3("gemini-2.5-pro"));
        assert!(is_gemini_under_v3("gemini-2.0-flash"));
        assert!(is_gemini_under_v3("gemini-1.5-pro"));
        assert!(!is_gemini_under_v3("gemini-3.7-flash-high"));
        assert!(!is_gemini_under_v3("gemini-3-flash"));
        assert!(!is_gemini_under_v3("gemini-3-pro"));
        assert!(!is_gemini_under_v3("gemini-3.1-pro"));
        assert!(!is_gemini_under_v3("gemini-3.8-flash"));
        assert!(!is_gemini_under_v3("gemini-pro-agent"));
        assert!(!is_gemini_under_v3("claude-3-7-sonnet"));

        assert!(!is_gemini_v3_or_above("gemini-2.5-flash"));
        assert!(!is_gemini_v3_or_above("gemini-2.0-flash"));
        assert!(is_gemini_v3_or_above("gemini-3.7-flash-high"));
        assert!(is_gemini_v3_or_above("gemini-3.7-flash"));
        assert!(is_gemini_v3_or_above("gemini-3-flash"));
        assert!(is_gemini_v3_or_above("gemini-3-pro"));
        assert!(is_gemini_v3_or_above("gemini-3.1-pro"));
        assert!(is_gemini_v3_or_above("gemini-3.8-flash"));
        assert!(is_gemini_v3_or_above("gemini-pro-agent"));
        assert!(!is_gemini_v3_or_above("claude-3-7-sonnet"));
    }

    #[test]
    fn test_gemini_thinking_budget() {
        // 显式 -high 后缀模型赋予 10000 满血预算
        assert_eq!(get_thinking_budget("gemini-3.7-flash-high", None), 10000);
        assert_eq!(get_thinking_budget("gemini-3.8-flash-high", None), 10000);
        assert_eq!(get_thinking_budget("gemini-3.9-flash-high", None), 10000);

        // 显式 -medium 后缀模型赋予 4000 预算
        assert_eq!(get_thinking_budget("gemini-3.7-flash-medium", None), 4000);
        assert_eq!(get_thinking_budget("gemini-3.8-flash-medium", None), 4000);

        // 显式 -low 后缀模型赋予 1000 预算
        assert_eq!(get_thinking_budget("gemini-3.7-flash-low", None), 1000);
        assert_eq!(get_thinking_budget("gemini-3.8-flash-low", None), 1000);

        // 未带显式档位后缀的 Gemini >= 3.0 模型，默认赋予 medium 预算 (4000)
        assert_eq!(get_thinking_budget("gemini-3.7-flash", None), 4000);
        assert_eq!(get_thinking_budget("gemini-3.8-flash", None), 4000);
        assert_eq!(get_thinking_budget("gemini-3.9-flash", None), 4000);

        // Pro 系列
        assert_eq!(get_thinking_budget("gemini-3.1-pro-high", None), 10001);
        assert_eq!(get_thinking_budget("gemini-pro-agent", None), 10001);
        assert_eq!(get_thinking_budget("gemini-3.1-pro-low", None), 1001);
    }

    #[test]
    fn test_resolve_authoritative_thinking_budget() {
        // 1. 启发式模型：强制按字典锁定，完全忽略客户端 effort
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3.7-flash-high", Some("low"), None, None),
            10000
        );
        assert_eq!(
            resolve_authoritative_thinking_budget(
                "gemini-3.7-flash-high",
                Some("none"),
                None,
                None
            ),
            10000
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3.1-pro-low", Some("high"), None, None),
            1001
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-pro-agent", Some("low"), None, None),
            10001
        );

        // 2. 裸模型 Flash 系列：接管客户端 effort
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3-flash", Some("high"), None, None),
            10000
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3-flash", Some("max"), None, None),
            10000
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3-flash", Some("xhigh"), None, None),
            10000
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3-flash", Some("low"), None, None),
            1000
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3-flash", Some("extra-low"), None, None),
            1000
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3-flash", Some("medium"), None, None),
            4000
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3-flash", Some("default"), None, None),
            4000
        );

        // 裸模型 Flash 系列：客户端不填或试图关闭，绝不关闭思考，强制回填 -medium (4000)
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3-flash", None, None, None),
            4000
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3-flash", Some("none"), None, None),
            4000
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3-flash", Some("disabled"), None, None),
            4000
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3-flash", Some("0"), None, None),
            4000
        );

        // 3. 裸模型 Pro 系列：接管客户端 effort
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3.1-pro", Some("high"), None, None),
            10001
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3.1-pro", Some("low"), None, None),
            1001
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3.1-pro", Some("medium"), None, None),
            10001
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3.1-pro", None, None, None),
            10001
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3.1-pro", Some("none"), None, None),
            10001
        );

        // 4. Gemini < 3 非思考模型：返回 0
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-2.5-flash", Some("high"), None, None),
            0
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-1.5-pro", None, None, None),
            0
        );

        // 5. 裸模型传入思考预算字段 (client_budget) 直接被彻底忽略，完全由客户端等级或服务器兜底接管
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3-flash", Some("high"), Some(1024), None),
            10000
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3-flash", None, Some(1024), None),
            4000
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3-flash", None, Some(32000), None),
            4000
        );
        assert_eq!(
            resolve_authoritative_thinking_budget("gemini-3.1-pro", None, Some(1000), None),
            10001
        );
    }

    #[test]
    fn test_bare_gemini_v36_or_above_flash() {
        assert!(is_bare_gemini_v36_or_above_flash("gemini-3.6-flash"));
        assert!(is_bare_gemini_v36_or_above_flash("gemini-3.7-flash"));
        assert!(is_bare_gemini_v36_or_above_flash("gemini-3.8-flash"));
        assert!(is_bare_gemini_v36_or_above_flash("gemini-3.9-flash"));
        assert!(is_bare_gemini_v36_or_above_flash("gemini-4.0-flash"));
        assert!(is_bare_gemini_v36_or_above_flash("GEMINI-3.7-FLASH"));

        assert!(is_bare_gemini_v36_or_above_flash("gemini-3.5-flash"));
        assert!(is_bare_gemini_v36_or_above_flash("gemini-3-flash"));
        assert!(!is_bare_gemini_v36_or_above_flash("gemini-2.5-flash"));

        // 已有显式后缀或变体标记的不命中
        assert!(!is_bare_gemini_v36_or_above_flash("gemini-3.7-flash-high"));
        assert!(!is_bare_gemini_v36_or_above_flash(
            "gemini-3.7-flash-medium"
        ));
        assert!(!is_bare_gemini_v36_or_above_flash("gemini-3.7-flash-low"));
        assert!(!is_bare_gemini_v36_or_above_flash(
            "gemini-3.7-flash-tiered"
        ));
        assert!(!is_bare_gemini_v36_or_above_flash(
            "gemini-3.8-flash-tiered"
        ));
        assert!(!is_bare_gemini_v36_or_above_flash("gemini-3.7-pro"));
    }

    #[test]
    fn test_tiered_flash_budget_and_client_effort() {
        let mut tb = crate::proxy::config::ThinkingBudgetConfig::default();
        tb.flash_mode = crate::proxy::config::ThinkingBudgetMode::Custom;
        tb.flash_low = 1024;
        tb.flash_medium = 4096;
        tb.flash_high = 16384;
        tb.flash_tiered = -1;

        // 1. 无思考参数，且 flash_tiered 为 -1：走官方默认 -1
        assert_eq!(
            resolve_custom_budget("gemini-3.7-flash-tiered", None, None, &tb, None),
            Some(-1)
        );

        // 2. 忽略客户端传入的数字 budget，无 effort 依然返回 Some(-1)
        assert_eq!(
            resolve_custom_budget("gemini-3.7-flash-tiered", None, Some(9999), &tb, None),
            Some(-1)
        );

        // 3. 客户端传入思考参数 low / medium / high：分别映射到网关设置的 flash_low, flash_medium, flash_high
        assert_eq!(
            resolve_custom_budget(
                "gemini-3.7-flash-tiered",
                Some("low"),
                Some(5000),
                &tb,
                None
            ),
            Some(1024)
        );
        assert_eq!(
            resolve_custom_budget(
                "gemini-3.7-flash-tiered",
                Some("extra-low"),
                None,
                &tb,
                None
            ),
            Some(1024)
        );
        assert_eq!(
            resolve_custom_budget("gemini-3.7-flash-tiered", Some("medium"), None, &tb, None),
            Some(4096)
        );
        assert_eq!(
            resolve_custom_budget("gemini-3.7-flash-tiered", Some("high"), None, &tb, None),
            Some(16384)
        );
        assert_eq!(
            resolve_custom_budget(
                "gemini-3.7-flash-tiered",
                Some("max"),
                Some(2048),
                &tb,
                None
            ),
            Some(16384)
        );

        // 4. 网关自定义 flash_tiered 为正数，且客户端无思考参数：返回自定义正数
        tb.flash_tiered = 8192;
        assert_eq!(
            resolve_custom_budget("gemini-3.7-flash-tiered", None, None, &tb, None),
            Some(8192)
        );

        // 5. 默认模式 (Default)，无思考参数：返回 Some(-1)（官方默认 -1）
        tb.flash_mode = crate::proxy::config::ThinkingBudgetMode::Default;
        assert_eq!(
            resolve_custom_budget("gemini-3.7-flash-tiered", None, None, &tb, None),
            Some(-1)
        );

        // 6. 具名非 tiered 模型 (gemini-3.7-flash-high)：在 Gateway 模式下锁死由模型后缀决定的档位，不受客户端 effort 篡改
        tb.flash_mode = crate::proxy::config::ThinkingBudgetMode::Custom;
        assert_eq!(
            resolve_custom_budget("gemini-3.7-flash-high", Some("low"), None, &tb, None),
            Some(16384) // 依然保持 high 档位设置 16384，绝不被客户端 low 降级！
        );
    }

    #[test]
    fn test_normalize_client_thinking_level_snake_case() {
        // 支持下划线形态（如 extra_low, x_high 等）与连字符形态等价解析
        assert_eq!(normalize_client_thinking_level("extra_low"), Some("LOW"));
        assert_eq!(normalize_client_thinking_level("extra-low"), Some("LOW"));
        assert_eq!(normalize_client_thinking_level("x_high"), Some("HIGH"));
        assert_eq!(normalize_client_thinking_level("x-high"), Some("HIGH"));
        assert_eq!(normalize_client_thinking_level("flash_lite"), Some("LOW"));
        assert_eq!(normalize_client_thinking_level("lite"), Some("LOW"));
        assert_eq!(normalize_client_thinking_level("disabled"), Some("LOW"));
        assert_eq!(normalize_client_thinking_level("off"), Some("LOW"));
        assert_eq!(normalize_client_thinking_level("none"), Some("LOW"));
        assert_eq!(normalize_client_thinking_level("medium"), Some("MEDIUM"));

        // 权重梯队兼容下划线
        assert_eq!(tier_weight("extra_low"), 0);
        assert_eq!(tier_weight("extra-low"), 0);
        assert_eq!(tier_weight("flash_lite"), 0);
        assert_eq!(tier_weight("low"), 1);
        assert_eq!(tier_weight("medium"), 3);
        assert_eq!(tier_weight("x_high"), 5);
        assert_eq!(tier_weight("high"), 4);

        // 裸模型路由支持 extra_low 映射
        assert_eq!(
            resolve_bare_tiered_model_route("gemini-3.8-flash", Some("extra_low")),
            Some("gemini-3.8-flash-low".to_string())
        );
        assert_eq!(
            resolve_bare_tiered_model_route("gemini-3.1-pro", Some("extra_low")),
            Some("gemini-3.1-pro-low".to_string())
        );
        assert_eq!(
            resolve_bare_tiered_model_route("gemini-3.1-pro", Some("x_high")),
            Some("gemini-3.1-pro-high".to_string())
        );
    }
}
