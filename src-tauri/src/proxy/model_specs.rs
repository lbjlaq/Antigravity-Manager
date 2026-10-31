use crate::proxy::token_manager::ProxyToken;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpec {
    pub max_output_tokens: Option<u64>,
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
            // [FIX #3215] 保护输出上限：避免某些 agent 专属通道配置的 2048 小限额卡死模型长文本生成能力
            return limit.max(8192);
        }
        // 如果原始 ID 没找到，尝试用归一化后的 ID 找
        if let Some(&limit) = t.model_limits.get(model_id) {
            return limit.max(8192);
        }
        // [NEW] 剥离档位后缀后再次尝试从 model_limits 读取
        let base = crate::proxy::common::model_mapping::strip_tier_suffix(model_id);
        if let Some(&limit) = t.model_limits.get(&base) {
            return limit.max(8192);
        }
    }

    // 2. 官方模型目录（唯一权威）
    if let Some(info) = crate::models::OfficialModelCatalog::get(model_id) {
        if let Some(limit) = info.max_output_tokens.filter(|v| *v > 0) {
            return limit as u64;
        }
    }

    // 3. 静态 JSON（历史兼容兜底）
    if let Some(spec) = SPECS.models.get(&std_id) {
        if let Some(limit) = spec.max_output_tokens {
            return limit;
        }
    }

    // 4. 全局兜底
    65535
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

/// 检查是否为 Gemini >= 4.0 系列新一代高级模型（纯数值/语义版本通配，不依赖硬编码代号）
pub fn is_gemini_v4_or_above(model: &str) -> bool {
    let lower = model.to_lowercase();
    if !lower.contains("gemini") {
        return false;
    }
    if let Some(idx) = lower.find("gemini-") {
        let rest = &lower[idx + "gemini-".len()..];
        let version_part: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if let Ok(ver) = version_part.parse::<f32>() {
            return ver >= 4.0;
        }
    }
    // 通配检测：匹配类似 gemini-4、gemini-5 等主版本形态
    if let Some(idx) = lower.find("gemini-") {
        let after = &lower[idx + "gemini-".len()..];
        if let Some(first_char) = after.chars().next() {
            if let Some(digit) = first_char.to_digit(10) {
                return digit >= 4;
            }
        }
    }
    false
}

/// 从 Claude 模型 ID 或名称中提取 (major, minor) 版本元组。
/// 自动跳过 8 位日期快照 (如 20251001, 20241022) 与非版本标识，
/// 支持单数字大版本（如 `claude-5-opus` -> (5, 0)）、双数字版本（`claude-opus-4-6` -> (4, 6)）以及点号/下划线分隔。
pub fn parse_claude_version_tuple(model: &str) -> Option<(u32, u32)> {
    let lower = model.to_lowercase();
    let pos = lower.find("claude")?;
    let rest = &lower[pos..];
    let tokens: Vec<&str> = rest
        .split(|c: char| c == '-' || c == '_' || c == '.')
        .collect();
    let mut detected: Option<(u32, u32)> = None;
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i];
        if token.len() == 8 && (token.starts_with("202") || token.starts_with("201")) {
            i += 1;
            continue;
        }
        if let Ok(major) = token.parse::<u32>() {
            if major >= 1000 {
                i += 1;
                continue;
            }
            let mut minor = 0;
            if i + 1 < tokens.len() {
                let next_tok = tokens[i + 1];
                if !(next_tok.len() == 8
                    && (next_tok.starts_with("202") || next_tok.starts_with("201")))
                {
                    if let Ok(m) = next_tok.parse::<u32>() {
                        if m < 1000 {
                            minor = m;
                            i += 1;
                        }
                    }
                }
            }
            let v = (major, minor);
            detected = Some(detected.map_or(v, |cur| cur.max(v)));
        } else if let Some(v) = crate::proxy::common::model_mapping::parse_version_tuple(token) {
            detected = Some(detected.map_or(v, |cur| cur.max(v)));
        }
        i += 1;
    }
    if detected.is_none() {
        if let Some(ver) = crate::proxy::common::model_mapping::parse_version_tuple(rest) {
            detected = Some(ver);
        }
    }
    detected
}

/// 检查是否为 Claude >= 5.0 版本模型（含裸模型与档位变体）
pub fn is_claude_v5_or_above(model: &str) -> bool {
    parse_claude_version_tuple(model).map_or(false, |ver| ver >= (5, 0))
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
/// 3. 再次找「high 下的一个等级」（权重严格低于 high 的最高档，如 [low, high] → low）
/// 4. 极端保底：仅从真实存在的可用档位中取首项，绝不虚构不存在的档位
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

    // 3. 没有 tiered/medium：优先取严格高于 low 的最低档
    //    （Pro 官方只有 low/high → high，与 variant 的 Medium→Pro high 契约一致）
    if let Some(low_weight) = available_tiers
        .iter()
        .find(|t| t.eq_ignore_ascii_case("low"))
        .map(|t| tier_weight(t))
    {
        let mut above_low: Vec<(&String, i32)> = available_tiers
            .iter()
            .map(|t| (t, tier_weight(t)))
            .filter(|(_, w)| *w > low_weight)
            .collect();
        if !above_low.is_empty() {
            above_low.sort_by_key(|(_, w)| *w);
            return Some(above_low[0].0.clone());
        }
    }

    // 4. 无 low 档：取 high 下的最高档（保守默认）
    let high_weight = tier_weight("high");
    let mut below_high: Vec<(&String, i32)> = available_tiers
        .iter()
        .map(|t| (t, tier_weight(t)))
        .filter(|(_, w)| *w < high_weight)
        .collect();

    if !below_high.is_empty() {
        below_high.sort_by_key(|(_, w)| std::cmp::Reverse(*w));
        return Some(below_high[0].0.clone());
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

        // 2.5 显式关闭思考：仅裸模型能走到这里（具名档位已在步骤 1 返回）。
        // 官方不允许真正关闭思考，因此降为**官方档位数组中的最低档**（tiered 不参与）。
        if let Some(raw) = client_effort.map(|s| s.trim().to_lowercase()) {
            if matches!(raw.as_str(), "none" | "off" | "disabled" | "false" | "0") {
                let lowest = available_tiers
                    .iter()
                    .filter(|t| !t.eq_ignore_ascii_case("tiered"))
                    .min_by_key(|t| tier_weight(t));
                if let Some(tier) = lowest {
                    return Some(format!("{}-{}", base, tier));
                }
            }
        }

        // 3. 客户端显式指定了思考强度：**只接受官方真实存在的档位后缀**；
        // 其余（含 default / 不认识的词 / 该家族不存在的档位）一律走第 4 步的默认档位。
        if let Some(raw) = client_effort.map(|s| s.trim().to_lowercase().replace('_', "-")) {
            let is_explicit_tier =
                !raw.is_empty() && !matches!(raw.as_str(), "default" | "none" | "off" | "disabled");

            if is_explicit_tier {
                // ① 原始值直接命中（如 extra-low / xhigh 若官方真存在）
                if let Some(matched) = available_tiers
                    .iter()
                    .find(|t| t.eq_ignore_ascii_case(&raw))
                {
                    return Some(format!("{}-{}", base, matched));
                }
                // ② 语义等价映射后再命中（如 xhigh/max → high、minimal → low）
                if let Some(level) = normalize_client_thinking_level(&raw) {
                    let level_lower = level.to_lowercase();
                    if let Some(matched) = available_tiers
                        .iter()
                        .find(|t| t.eq_ignore_ascii_case(&level_lower))
                    {
                        return Some(format!("{}-{}", base, matched));
                    }
                }
                // ③ 不属于官方实际后缀档位 → 不猜、不就近，直接走默认档位
            }
        }

        // 4. 未指定 / default / 非官方实际档位：按统一默认档位决策
        // tiered → medium → 「high 下的一个等级」→ 首个真实档位
        let default_tier = match pick_optimal_default_tier(&available_tiers) {
            Some(t) => t,
            None => return None,
        };

        Some(format!("{}-{}", base, default_tier))
    }
}

/// 该模型家族在官方目录中真实存在的思考档位后缀（低→高排序，**不含 tiered**）。
///
/// 用于 agent 一键配置里的 effort 选单：裸模型（如 `gemini-3.8-flash`）应当只暴露
/// `low / medium / high` 这三个官方真实存在的档位；`default` 一律表示“不指定，交给网关默认路由”。
pub fn available_efforts_for(model_id: &str) -> Vec<String> {
    let base = crate::proxy::common::model_mapping::strip_tier_suffix(model_id);
    let tiers = crate::models::OfficialModelCatalog::collect_tiers_for_base(&base);
    let mut out = Vec::new();
    for want in ["extra-low", "low", "medium", "high"] {
        if tiers.iter().any(|t| t.eq_ignore_ascii_case(want)) {
            out.push(want.to_string());
        }
    }
    // 单一档位不是“可选强度”（如 gpt-oss-120b-medium 的 medium 只是模型名的一部分），
    // 只有真正存在多个可选档位时才暴露 effort 选单。
    if out.len() < 2 {
        return Vec::new();
    }
    out
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
    _client_budget: Option<u64>,
    tb_config: &crate::proxy::config::ThinkingBudgetConfig,
    _token: Option<&ProxyToken>,
) -> Option<i64> {
    use crate::proxy::config::ThinkingBudgetMode;

    if is_gemini_under_v3(model) {
        return None;
    }

    // 官方目录未收录的模型：不再维护任何按系列 / 档位的硬编码预算表。
    // 仅当用户显式选择 Custom 模式时，才使用用户配置的数字；否则保持自适应（None）。
    if tb_config.mode != ThinkingBudgetMode::Custom {
        return None;
    }

    // 24576 是历史配置的“未设置”哨兵值，不能当作真实预算下发。
    if tb_config.custom_value > 0 && tb_config.custom_value != 24576 {
        return Some(tb_config.custom_value as i64);
    }

    let lower = resolve_alias(model).to_lowercase();
    // 档位优先级：模型名后缀（绝对优先）> 客户端 effort > 配置 effort
    let level = if lower.ends_with("-high") || lower.ends_with("-max") || lower.ends_with("-agent")
    {
        Some("HIGH")
    } else if lower.ends_with("-low") || lower.ends_with("-extra-low") || lower.ends_with("-lite") {
        Some("LOW")
    } else if lower.ends_with("-medium") || lower.ends_with("-default") {
        Some("MEDIUM")
    } else {
        client_effort
            .or(tb_config.effort.as_deref())
            .and_then(normalize_client_thinking_level)
    };

    let configured = if lower.contains("claude") {
        match level {
            Some("LOW") => tb_config.claude_low,
            Some("HIGH") => tb_config.claude_high,
            _ => tb_config.claude_medium,
        }
    } else if lower.contains("pro") {
        match level {
            Some("LOW") => tb_config.pro_low,
            _ => tb_config.pro_high,
        }
    } else {
        match level {
            Some("LOW") => tb_config.flash_low,
            Some("HIGH") => tb_config.flash_high,
            _ => tb_config.flash_medium,
        }
    };

    if configured > 0 {
        Some(configured as i64)
    } else {
        None
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
    fn pick_default_tier_prefers_above_low_without_medium() {
        let tiers = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // Pro 官方只有 low/high：默认必须回到 high，与 variant 的 Medium→Pro high 契约一致
        assert_eq!(
            pick_optimal_default_tier(&tiers(&["low", "high"])),
            Some("high".to_string())
        );
        assert_eq!(
            pick_optimal_default_tier(&tiers(&["extra-low", "low", "high"])),
            Some("high".to_string())
        );
        // 有 medium / tiered 时仍按优先级
        assert_eq!(
            pick_optimal_default_tier(&tiers(&["low", "medium", "high"])),
            Some("medium".to_string())
        );
        assert_eq!(
            pick_optimal_default_tier(&tiers(&["tiered", "high"])),
            Some("tiered".to_string())
        );
        assert_eq!(
            pick_optimal_default_tier(&tiers(&["low"])),
            Some("low".to_string())
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
    fn test_unknown_model_budget_is_config_driven_or_adaptive() {
        use crate::proxy::config::{ThinkingBudgetConfig, ThinkingBudgetMode};

        // 非 Custom：保持自适应，不伪造数字预算
        let mut tb = ThinkingBudgetConfig::default();
        tb.mode = ThinkingBudgetMode::Default;
        assert_eq!(
            resolve_custom_budget("gemini-3.7-flash-tiered", None, None, &tb, None),
            None
        );

        // Custom + 明确单值：使用用户配置
        tb.mode = ThinkingBudgetMode::Custom;
        tb.custom_value = 8192;
        assert_eq!(
            resolve_custom_budget("gemini-3.7-flash-tiered", None, None, &tb, None),
            Some(8192)
        );

        // Custom 但单值为历史哨兵：按 effort 取用户档位配置；未配置(-1) → 自适应
        tb.custom_value = 24576;
        assert_eq!(
            resolve_custom_budget("gemini-3.8-flash", Some("high"), None, &tb, None),
            None
        );
        tb.flash_high = 16384;
        assert_eq!(
            resolve_custom_budget("gemini-3.8-flash", Some("high"), None, &tb, None),
            Some(16384)
        );

        // 客户端数字预算永远不直控
        assert_eq!(
            resolve_custom_budget("gemini-3.8-flash", Some("low"), Some(9999), &tb, None),
            None
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

    #[test]
    fn test_claude_v5_detection() {
        assert!(is_claude_v5_or_above("claude-fable-5"));
        assert!(is_claude_v5_or_above("claude-fable-5-1"));
        assert!(is_claude_v5_or_above("claude-opus-5-5"));
        assert!(is_claude_v5_or_above("claude-opus-5-5-high"));
        assert!(is_claude_v5_or_above("claude-haiku-5-5"));
        assert!(is_claude_v5_or_above("claude-5-opus"));
        assert!(!is_claude_v5_or_above("claude-opus-4-6"));
        assert!(!is_claude_v5_or_above("claude-haiku-4-5"));
        assert!(!is_claude_v5_or_above("claude-3-5-sonnet"));

        assert!(is_bare_claude_tiered_model("claude-fable-5"));
        assert!(is_bare_claude_tiered_model("claude-fable-5-1"));
        assert!(is_bare_claude_tiered_model("claude-opus-5-5"));
        assert!(!is_bare_claude_tiered_model("claude-opus-5-5-high"));
        assert!(!is_bare_claude_tiered_model("claude-opus-4-6"));
    }

    #[test]
    fn test_parse_claude_version_tuple() {
        assert_eq!(parse_claude_version_tuple("claude-fable-5"), Some((5, 0)));
        assert_eq!(parse_claude_version_tuple("claude-fable-5-1"), Some((5, 1)));
        assert_eq!(parse_claude_version_tuple("claude-opus-5-5"), Some((5, 5)));
        assert_eq!(
            parse_claude_version_tuple("claude-opus-5-5-high"),
            Some((5, 5))
        );
        assert_eq!(
            parse_claude_version_tuple("claude-haiku-4-5-20251001"),
            Some((4, 5))
        );
        assert_eq!(
            parse_claude_version_tuple("claude-opus-4-6-thinking"),
            Some((4, 6))
        );
        assert_eq!(parse_claude_version_tuple("claude-opus-4-7"), Some((4, 7)));
        assert_eq!(
            parse_claude_version_tuple("claude-3-5-sonnet-20241022"),
            Some((3, 5))
        );
        assert_eq!(parse_claude_version_tuple("gemini-3.8-flash"), None);
    }
}
