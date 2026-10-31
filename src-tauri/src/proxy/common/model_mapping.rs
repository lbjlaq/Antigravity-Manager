// 模型名称映射
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::collections::HashMap;

// 动态官方废弃模型转发表 (old_model_id -> new_model_id)
pub static DYNAMIC_MODEL_FORWARDING_RULES: Lazy<DashMap<String, String>> =
    Lazy::new(|| DashMap::new());

pub fn update_dynamic_forwarding_rules(old_model: String, new_model: String) {
    if !DYNAMIC_MODEL_FORWARDING_RULES.contains_key(&old_model) {
        crate::modules::logger::log_info(&format!(
            "[Mapping] Registered automatic forwarding rule: {} -> {}",
            old_model, new_model
        ));
    }
    DYNAMIC_MODEL_FORWARDING_RULES.insert(old_model, new_model);
}

static CLAUDE_TO_GEMINI: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();

    // ── 官方支持模型的无后缀/变体归位（仅保留真实支持模型的后缀派生还原，全面下线过时硬编码兼容重定向）──
    m.insert("claude-sonnet-4-6-thinking", "claude-sonnet-4-6");
    m.insert("claude-opus-4-6", "claude-opus-4-6-thinking");
    m.insert("gpt-oss-120b", "gpt-oss-120b-medium");
    // 网关内部摘要虚拟任务路由至官方轻量模型
    m.insert("internal-background-task", "gemini-3.1-flash-lite");

    m
});

/// 旧客户端仍在请求带点号的 Claude 版本（`claude-opus-4.6`、`claude-sonnet-4.5`、
/// `claude-open-4.x`）。服务端目录只认连字符形态（`claude-opus-4-6` 等）。
/// 这里只改写 Claude ID：去掉供应商标前缀，把短版本号里的点换成连字符。
pub fn canonicalize_claude_client_model_id(input: &str) -> String {
    let mut id = input.trim().to_lowercase();
    for prefix in ["anthropic/", "models/"] {
        if let Some(rest) = id.strip_prefix(prefix) {
            id = rest.to_string();
        }
    }
    // [NEW Claude Code 2.1.293] 去除方括号上下文窗口后缀（如 [1m], [200k], [128k] 等）
    if let Some(bracket_idx) = id.find('[') {
        if let Some(close_idx) = id[bracket_idx..].find(']') {
            let before = &id[..bracket_idx];
            let after = &id[bracket_idx + close_idx + 1..];
            id = format!("{}{}", before, after);
        }
    }
    if id.contains("claude-open-") {
        id = id.replace("claude-open-", "claude-opus-");
    }
    if !id.contains("claude") {
        return input.trim().to_string();
    }

    let bytes = id.as_bytes();
    let mut out = String::with_capacity(id.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'.' {
                let dot = i;
                let mut j = i + 1;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                // 4.6 / 3.5 这类短版本号。八位日期里没有点，不会被改写。
                if j > dot + 1 && j - (dot + 1) <= 2 {
                    out.push_str(&id[start..dot]);
                    out.push('-');
                    out.push_str(&id[dot + 1..j]);
                    i = j;
                    continue;
                }
            }
            out.push_str(&id[start..i]);
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }

    // 规范化 Claude 5 裸大版本标识至基准版本 (如 claude-opus-5 -> claude-opus-5-5, claude-opus-5-low -> claude-opus-5-5-low)
    for family in ["opus", "sonnet", "haiku"] {
        let bare = format!("claude-{}-5", family);
        if out == bare {
            out = format!("claude-{}-5-5", family);
        } else if let Some(rest) = out.strip_prefix(&format!("{}-", bare)) {
            if !rest.starts_with(|c: char| c.is_ascii_digit()) {
                out = format!("claude-{}-5-5-{}", family, rest);
            }
        }
        let reversed_bare = format!("claude-5-{}", family);
        if out == reversed_bare {
            out = format!("claude-{}-5-5", family);
        } else if let Some(rest) = out.strip_prefix(&format!("{}-", reversed_bare)) {
            if !rest.starts_with(|c: char| c.is_ascii_digit()) {
                out = format!("claude-{}-5-5-{}", family, rest);
            }
        }
    }

    // 兼容 Claude 5 Fable 及其反向写法与裸别名 (claude-5-fable / claude-fable -> claude-fable-5)
    if out == "claude-5-fable" || out == "claude-fable" {
        out = "claude-fable-5".to_string();
    } else if let Some(rest) = out.strip_prefix("claude-5-fable-") {
        if !rest.starts_with(|c: char| c.is_ascii_digit()) {
            out = format!("claude-fable-5-{}", rest);
        }
    } else if let Some(rest) = out.strip_prefix("claude-fable-") {
        if !rest.starts_with(|c: char| c.is_ascii_digit()) {
            out = format!("claude-fable-5-{}", rest);
        }
    }

    out
}

/// Map model names to canonical upstream model IDs (zero legacy hardcoding)
pub fn map_claude_model_to_gemini(input: &str) -> String {
    let canonical = canonicalize_claude_client_model_id(input);

    // 0. 优先匹配上游返回的动态模型转发表
    if let Some(mapped) = DYNAMIC_MODEL_FORWARDING_RULES.get(canonical.as_str()) {
        return mapped.value().clone();
    }

    // 1. 精确匹配标准后缀派生还原表
    if let Some(mapped) = CLAUDE_TO_GEMINI.get(canonical.as_str()) {
        return mapped.to_string();
    }

    // 2. 已知前缀或官方物理 ID 原样透传
    if canonical.starts_with("gemini-")
        || canonical.contains("thinking")
        || canonical.contains("claude")
    {
        return canonical;
    }

    input.trim().to_string()
}

/// 解析形如 "4.6", "4-6", "3.10", "3-10", "3" 的 (major, minor) 版本元组。
/// 严格过滤 8 位日期快照 (如 20241022, 20250219) 以及大于等于 1000 的年份/非语义版本数字。
pub fn parse_version_tuple(s: &str) -> Option<(u32, u32)> {
    let s = s.trim().trim_start_matches('v');
    let mut parts = s.split(|c: char| c == '.' || c == '-' || c == '_');
    let major_token = parts.next()?;
    if major_token.len() == 8 && (major_token.starts_with("202") || major_token.starts_with("201"))
    {
        return None;
    }
    let major = major_token.parse::<u32>().ok()?;
    if major >= 1000 {
        return None;
    }
    let minor = if let Some(minor_token) = parts.next() {
        if minor_token.len() == 8
            && (minor_token.starts_with("202") || minor_token.starts_with("201"))
        {
            0
        } else if let Ok(m) = minor_token.parse::<u32>() {
            if m >= 1000 {
                0
            } else {
                m
            }
        } else {
            0
        }
    } else {
        0
    };
    Some((major, minor))
}

/// 核心基准线过滤器：严格遵循官方客户端当前展示的旗舰模型基准线，全面下线非模型列表展示的过时/内部模型
/// 1. Gemini 4+ 系列：如 gemini-4-argon 及 high/medium/low 变体保留。
/// 2. Gemini Flash 系列：基准线 3.8（>= 3.8 保留，淘汰 2.5 / 3.0 / 3.5 / 3.6 / 3.7 等历史过渡版本）；保留官方专用轻量级 3.1-flash-lite 与 3.5-flash-lite。
/// 3. Claude 系列：基准线 4.6（4.6 与 5.5+ 保留，4.6 以下、haiku、fable 等全部淘汰）。
/// 4. GPT 系列：仅保留官方 gpt-oss-120b-medium 与派生裸模型 gpt-oss-120b，淘汰 gpt-4o 等历史旧模型。
/// 5. Pro / Image 系列：保留 >= 3.1 Pro 与 3.1-flash-image / 3-pro-image，淘汰 2.5 Pro 及内部 agent / tab 预览模型。
pub fn is_model_compliant_with_baseline(model: &str) -> bool {
    let lower = model.to_lowercase();
    let m = lower.trim();

    // 过滤禁用标记模型、内部任务、测试、tab 补全与旧版 agent 内部代号模型
    if m.contains("-disabled")
        || m.ends_with("_disabled")
        || m.starts_with("chat_")
        || m.starts_with("tab_")
        || m.contains("internal")
        || m.contains("-exp")
        || m.ends_with("-agent")
        || m == "gemini-3-flash"
    {
        return false;
    }

    // [2026-10-29] 统一基准：Gemini 主版本 < 3 全量下线（2.5-pro / 2.5-flash / 2.0 / 1.5 …）。
    // 官方 fetchAvailableModels 的字典里仍可能残留这些条目，但模型列表与门禁一律不得暴露。
    if let Some(rest) = m.strip_prefix("gemini-") {
        if let Some((major, _minor)) = parse_version_tuple(rest) {
            if major < 3 {
                return false;
            }
        }
    }

    // 特许放行的官方轻量级与图像专用模型，以及新一代 Gemini 4 Argon 旗舰模型
    if m.contains("argon")
        || m == "gemini-3.1-flash-lite"
        || m == "gemini-3.5-flash-lite"
        || m == "gemini-3.1-flash-image"
        || m == "gemini-3-pro-image"
    {
        return true;
    }

    // 1. Claude 系列：以 4.6 为基准线（支持 4.6 与 5.5+，支持 Haiku >= 4.5 与 Fable/Mythos >= 5.0），淘汰 4.6 以下旧模型与非法 thinking 后缀变体
    if m.contains("claude") {
        // claude-sonnet 系列与 Claude 5.x 分档模型不存在 -thinking 后缀变体
        if (m.starts_with("claude-sonnet-") || m.contains("-5-5")) && m.contains("thinking") {
            return false;
        }

        if let Some(ver) = crate::proxy::model_specs::parse_claude_version_tuple(&m) {
            if m.contains("fable") || m.contains("mythos") {
                return ver >= (5, 0);
            }
            return ver >= (4, 6);
        }
        return false;
    }

    // 2. GPT / OpenAI 系列：仅保留官方 gpt-oss-120b-medium 及其无后缀派生 gpt-oss-120b
    if m.contains("gpt") || m.contains("o1") || m.contains("o3") {
        return m == "gpt-oss-120b-medium" || m == "gpt-oss-120b";
    }

    // 3. 图像模型保底
    if m.contains("image") {
        return m == "gemini-3.1-flash-image" || m == "gemini-3-pro-image";
    }

    // 4. Gemini Pro 系列：以官方最新为准 (>= 3.1 Pro)，淘汰 2.5 Pro 及以下历史旧模型
    if m.contains("pro") {
        if m.contains("1.5")
            || m.contains("2.0")
            || m.contains("2.5")
            || m.contains("2-5")
            || m == "gemini-3-pro"
            || m.contains("preview")
        {
            return false;
        }
        if m.contains("3.1") {
            return true;
        }
        if let Some(pos) = m.find("gemini-") {
            let rest = &m[pos + 7..];
            if let Some(pro_pos) = rest.find("-pro") {
                if let Some(ver) = parse_version_tuple(&rest[..pro_pos]) {
                    return ver >= (3, 1);
                }
            }
        }
        return false;
    }

    // 5. Gemini Flash 系列：全面升级至 >= 3.8 旗舰基准线，淘汰 2.5 / 3.0 / 3.5 / 3.6 / 3.7 等旧版推理 Flash
    if m.contains("flash") {
        if m.contains("2.5")
            || m.contains("2-5")
            || m.contains("3.5")
            || m.contains("3-5")
            || m.contains("3.6")
            || m.contains("3-6")
            || m.contains("3.7")
            || m.contains("3-7")
        {
            return false;
        }

        if let Some(pos) = m.find("gemini-") {
            let rest = &m[pos + 7..];
            if let Some(flash_pos) = rest.find("-flash") {
                let ver_str = &rest[..flash_pos];
                if let Some(ver) = parse_version_tuple(ver_str) {
                    return ver >= (3, 8);
                }
            }
        }

        if m.contains("3.8") || m.contains("3-8") || m.contains("3.9") || m.contains("3-9") {
            return true;
        }

        return false;
    }

    // 6. Gemini 4+ 新一代代号模型（如 gemini-4-argon 及 high/medium/low）
    if m.starts_with("gemini-") {
        let rest = &m["gemini-".len()..];
        if let Some(ver) = parse_version_tuple(rest) {
            return ver.0 >= 4;
        }
        return false;
    }

    false
}

/// 获取所有内置支持的标准公开模型列表 (动态从官方模型结构体目录获取并严格过滤基准线)
pub fn get_supported_models() -> Vec<String> {
    crate::models::OfficialModelCatalog::all_model_ids()
        .into_iter()
        .filter(|id| is_model_compliant_with_baseline(id))
        .collect()
}

/// 动态获取所有可用模型列表 (全账号池实际支持模型的并集 + 档位后缀自动派生无后缀裸模型 ID)
pub async fn get_all_dynamic_models(
    custom_mapping: &tokio::sync::RwLock<std::collections::HashMap<String, String>>,
    token_manager: Option<&crate::proxy::token_manager::TokenManager>,
    only_raw_quota_models: bool,
) -> Vec<String> {
    use std::collections::HashSet;
    let mut model_ids = HashSet::new();
    let mut custom_keys = HashSet::new();

    // 1. 获取所有账号从官方 fetchAvailableModels 接口汇聚而来的真实可用模型并集
    // [2026-10-29] 严格取官方可用集合（不含内部保护桶名，也不含字典残留）
    if let Some(tm) = token_manager {
        for dynamic_model in tm.get_all_available_models() {
            if is_model_compliant_with_baseline(&dynamic_model) {
                model_ids.insert(dynamic_model);
            }
        }
    }

    // 如果未开启 only_raw_quota_models：
    // - 当账号池尚未加载任何模型（冷启动/无账号测试）时，使用官方权威目录基准模型兜底；
    // - 追加用户显式配置的自定义映射模型 (Custom)。
    if !only_raw_quota_models {
        if model_ids.is_empty() {
            for m in get_supported_models() {
                model_ids.insert(m);
            }
        }

        {
            let mapping = custom_mapping.read().await;
            let expose_map = crate::modules::config::load_app_config()
                .map(|c| c.proxy.custom_mapping_expose)
                .unwrap_or_default();
            for (key, target) in mapping.iter() {
                let exposed = expose_map
                    .get(key)
                    .copied()
                    .unwrap_or_else(|| !key.contains('*') && !target.starts_with("auto:"));
                if exposed && !key.contains('*') && !target.starts_with("auto:") {
                    custom_keys.insert(key.clone());
                    model_ids.insert(key.clone());
                }
            }
        }
    }

    // 4. 动态档位后缀剥离与裸模型派生（纯通用数据驱动）：
    // 若账号实际支持的模型包含分档或思考后缀（如 -high / -medium / -low / -extra-low / -tiered / -thinking），
    // 自动剥离后缀并衍生对应的无后缀裸模型名（如 gemini-4-argon, gemini-3.8-flash, claude-opus-5-5, claude-sonnet-5-5, claude-opus-4-6, gpt-oss-120b）。
    let mut derived_bare_models = HashSet::new();
    for id in &model_ids {
        for suffix in &[
            "-tiered",
            "-high",
            "-medium",
            "-low",
            "-extra-low",
            "-thinking",
        ] {
            if let Some(base) = id.strip_suffix(suffix) {
                if !base.is_empty() && is_model_compliant_with_baseline(base) {
                    derived_bare_models.insert(base.to_string());
                }
            }
        }
    }
    for bare in derived_bare_models {
        model_ids.insert(bare);
    }

    // 4.5 生图尺寸变体归一（仅默认模式，且必须账号**真实配额 > 0**）：
    // 部分账号只上报 `gemini-3.1-flash-image-4k` 这类尺寸变体；
    // 只要该变体真实配额 > 0，就把基底生图模型也登记进列表，避免“有额度却看不到模型”。
    // 零配额账号不会因此凭空出现（“默认列表不显示无配额模型”）。
    if !only_raw_quota_models {
        if let Some(tm) = token_manager {
            for raw in tm.models_with_positive_quota() {
                let base = strip_image_variant_suffix(&raw);
                if base != raw.to_lowercase()
                    && base.contains("image")
                    && is_model_compliant_with_baseline(&base)
                {
                    model_ids.insert(base);
                }
            }
        }
    }

    // 5. 应用官方基准线过滤，彻底剔除已淘汰的旧版模型与内部虚拟 ID
    let mut sorted_ids: Vec<_> = model_ids
        .into_iter()
        .filter(|id| custom_keys.contains(id) || is_model_compliant_with_baseline(id))
        .collect();
    sorted_ids.sort();
    sorted_ids
}

/// 生图模型族变体归一化：剔除尺寸/分辨率/预览后缀，得到基底生图模型 ID。
///
/// 例：`gemini-3-pro-image-preview` → `gemini-3-pro-image`；
/// 　　`gemini-3.1-flash-image-4k-16x9` → `gemini-3.1-flash-image`。
pub fn strip_image_variant_suffix(id: &str) -> String {
    const SUFFIXES: &[&str] = &[
        "-preview",
        "-2k",
        "-4k",
        "-1k",
        "-8k",
        "-16x9",
        "-9x16",
        "-1x1",
        "-3x4",
        "-4x3",
        "-high",
        "-medium",
        "-low",
        "-extra-low",
        "-tiered",
    ];
    let mut base = id.trim().to_lowercase();
    let mut changed = true;
    while changed {
        changed = false;
        for suffix in SUFFIXES {
            if let Some(stripped) = base.strip_suffix(suffix) {
                if !stripped.is_empty() {
                    base = stripped.to_string();
                    changed = true;
                }
            }
        }
    }
    base
}

/// 去除模型展示名称中的档位括号后缀 (如 "Gemini 3.1 Pro (Low)" -> "Gemini 3.1 Pro")
pub fn clean_tier_from_display_name(dn: &str) -> String {
    let re_patterns = [
        " (Tiered)",
        " (Max)",
        " (XHigh)",
        " (High)",
        " (Medium)",
        " (Default)",
        " (Low)",
        " (Extra Low)",
        " (Preview)",
        " (Thinking)",
    ];
    let mut res = dn.to_string();
    for pat in &re_patterns {
        res = res.replace(pat, "");
    }
    res
}

/// 动态获取所有可用模型的元数据字典
///
/// 聚合逻辑：
/// 1. 获取所有账号的 Quota 动态元数据 (TokenManager::get_all_model_metadata)
/// 2. 剥离分档后缀（-high/-medium/-low/-tiered/-thinking 等），自动让派生裸模型继承能力元数据
/// 3. 从官方权威模型目录（OfficialModelCatalog）补充/合并权威元数据
/// 4. 自定义映射（custom_mapping）继承其目标模型的元数据
pub async fn get_all_dynamic_models_metadata(
    custom_mapping: &tokio::sync::RwLock<std::collections::HashMap<String, String>>,
    token_manager: Option<&crate::proxy::token_manager::TokenManager>,
    only_raw_quota_models: bool,
) -> std::collections::HashMap<String, crate::models::quota::ModelListingMeta> {
    let mut meta_map = if let Some(tm) = token_manager {
        tm.get_all_model_metadata()
    } else {
        std::collections::HashMap::new()
    };

    if !only_raw_quota_models && meta_map.is_empty() {
        for m in get_supported_models() {
            if let Some(official) = crate::models::OfficialModelCatalog::get(&m) {
                let official_meta = crate::models::quota::ModelListingMeta::from(&official);
                meta_map.entry(m).or_default().merge_from(&official_meta);
            }
        }
    }

    // 动态档位后缀剥离与裸模型元数据派生继承
    let mut derived_metas = std::collections::HashMap::new();
    for (name, meta) in &meta_map {
        for suffix in &[
            "-tiered",
            "-high",
            "-medium",
            "-low",
            "-extra-low",
            "-thinking",
        ] {
            if let Some(base) = name.strip_suffix(suffix) {
                if !base.is_empty() && is_model_compliant_with_baseline(base) {
                    let mut m = meta.clone();
                    if let Some(ref dn) = m.display_name {
                        m.display_name = Some(clean_tier_from_display_name(dn));
                    }
                    derived_metas.entry(base.to_string()).or_insert(m);
                }
            }
        }
    }
    for (k, v) in derived_metas {
        meta_map.entry(k).or_insert(v);
    }

    if !only_raw_quota_models {
        // 对已存在的模型补充官方权威目录元数据
        let existing_keys: Vec<String> = meta_map.keys().cloned().collect();
        for m in existing_keys {
            if let Some(official) = crate::models::OfficialModelCatalog::get(&m) {
                let official_meta = crate::models::quota::ModelListingMeta::from(&official);
                meta_map.entry(m).or_default().merge_from(&official_meta);
            }
        }

        // 自定义映射模型继承目标模型的元数据（非暴露与内部暗路由规则不注入）
        let mapping = custom_mapping.read().await;
        let expose_map = crate::modules::config::load_app_config()
            .map(|c| c.proxy.custom_mapping_expose)
            .unwrap_or_default();
        for (alias, target) in mapping.iter() {
            let exposed = expose_map
                .get(alias)
                .copied()
                .unwrap_or_else(|| !alias.contains('*') && !target.starts_with("auto:"));
            if !exposed || alias.contains('*') || target.starts_with("auto:") {
                continue;
            }
            let target_lower = target.to_lowercase();
            if let Some(target_meta) = meta_map.get(&target_lower).cloned() {
                meta_map.entry(alias.clone()).or_insert(target_meta);
            } else if let Some(official) = crate::models::OfficialModelCatalog::get(target) {
                let official_meta = crate::models::quota::ModelListingMeta::from(&official);
                meta_map.entry(alias.clone()).or_insert(official_meta);
            }
        }
    }

    // 5. 应用官方模型结构体参数覆盖：
    // [2026-10-29] 官方已把思考传输迁移到 thinkingLevel，不再下发 thinkingBudget 的模型
    // 一律不伪造预算（避免向客户端暴露一个实际不会生效的数字）。
    for (name, meta) in meta_map.iter_mut() {
        if meta.supports_thinking.unwrap_or(true) {
            let official_budget = crate::models::OfficialModelCatalog::get(name)
                .and_then(|info| info.thinking_budget)
                .map(|b| b as i32);

            meta.thinking_budget = official_budget.or(meta.thinking_budget);
        }
    }

    // 6. 最终校验：剔除已下线 / 不合格的字典残留（2.x、agent、internal、未经官方开通的条目）
    // 用户显式配置且开启对外暴露的自定义映射键除外（自定义别名必须原样生效）。
    {
        let mapping = custom_mapping.read().await;
        let expose_map = crate::modules::config::load_app_config()
            .map(|c| c.proxy.custom_mapping_expose)
            .unwrap_or_default();
        meta_map.retain(|name, _| {
            if let Some(target) = mapping.get(name) {
                let exposed = expose_map
                    .get(name)
                    .copied()
                    .unwrap_or_else(|| !name.contains('*') && !target.starts_with("auto:"));
                exposed && !name.contains('*') && !target.starts_with("auto:")
            } else {
                is_model_compliant_with_baseline(name)
            }
        });
    }

    meta_map
}

/// 动态查找指定的模型。
/// 支持处理 "models/" 前缀，优先精确匹配，兜底大小写不敏感匹配。
pub async fn find_dynamic_model(
    custom_mapping: &tokio::sync::RwLock<std::collections::HashMap<String, String>>,
    token_manager: Option<&crate::proxy::token_manager::TokenManager>,
    only_raw_quota_models: bool,
    requested_model: &str,
) -> Option<String> {
    let clean_model = requested_model
        .strip_prefix("models/")
        .unwrap_or(requested_model)
        .trim();

    let all_models =
        get_all_dynamic_models(custom_mapping, token_manager, only_raw_quota_models).await;

    // 1. 精确匹配
    if let Some(m) = all_models.iter().find(|&m| m == clean_model) {
        return Some(m.clone());
    }
    // 2. 忽略大小写匹配
    if let Some(m) = all_models
        .iter()
        .find(|&m| m.eq_ignore_ascii_case(clean_model))
    {
        return Some(m.clone());
    }
    None
}

/// Wildcard matching - supports multiple wildcards
///
/// **Note**: Matching is **case-sensitive**. Pattern `GPT-4*` will NOT match `gpt-4-turbo`.
///
/// Examples:
/// - `gpt-4*` matches `gpt-4`, `gpt-4-turbo` ✓
/// - `claude-*-sonnet-*` matches `claude-3-5-sonnet-20241022` ✓
/// - `*-thinking` matches `claude-opus-4-5-thinking` ✓
/// - `a*b*c` matches `a123b456c` ✓
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();

    // No wildcard - exact match
    if parts.len() == 1 {
        return pattern == text;
    }

    let mut text_pos = 0;

    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue; // Skip empty segments from consecutive wildcards
        }

        if i == 0 {
            // First segment must match start
            if !text[text_pos..].starts_with(part) {
                return false;
            }
            text_pos += part.len();
        } else if i == parts.len() - 1 {
            // Last segment must match end
            return text[text_pos..].ends_with(part);
        } else {
            // Middle segments - find next occurrence
            if let Some(pos) = text[text_pos..].find(part) {
                text_pos += pos + part.len();
            } else {
                return false;
            }
        }
    }

    true
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
    resolve_model_route_with_effort(original_model, custom_mapping, None)
}

/// 核心模型路由解析引擎（支持客户端思考档位感知）
/// 优先级：精确匹配 > 通配符匹配 > 3.x Flash 档位路由 > 系统默认映射
///
/// # 参数
/// - `original_model`: 原始模型名称
/// - `custom_mapping`: 用户自定义映射表
/// - `client_effort`: 客户端传入的思考档位（如 "low", "medium", "high"）
///
/// 仅从自定义映射与热更新重定向规则中解析模型路由（若未匹配自定义规则则返回 None）
pub fn resolve_custom_model_route(
    original_model: &str,
    custom_mapping: &std::collections::HashMap<String, String>,
) -> Option<String> {
    // 0. API 热更新废弃模型转发 (最高物理优先级，强制纠正)
    // 如果用户非要用已经被移除的模型，并且官方下发了 fallback path，我们在此拦截并纠正
    if let Some(forwarded) = DYNAMIC_MODEL_FORWARDING_RULES.get(original_model) {
        crate::modules::logger::log_info(&format!(
            "[Router] 官方淘汰重定向: {} -> {}",
            original_model,
            forwarded.value()
        ));
        return Some(forwarded.value().clone());
    }

    // 1. 自定义映射匹配 (次高优先级：精确匹配 -> 大小写不敏感匹配 -> 规范化 ID 匹配)
    if let Some(target) = custom_mapping.get(original_model) {
        crate::modules::logger::log_info(&format!(
            "[Router] 精确映射: {} -> {}",
            original_model, target
        ));
        return Some(target.clone());
    }

    let original_lower = original_model.to_lowercase();
    for (k, target) in custom_mapping.iter() {
        if k.to_lowercase() == original_lower {
            crate::modules::logger::log_info(&format!(
                "[Router] 自定义映射(大小写不敏感): {} -> {}",
                original_model, target
            ));
            return Some(target.clone());
        }
    }

    let canonical = canonicalize_claude_client_model_id(original_model);
    if let Some(target) = custom_mapping.get(&canonical) {
        crate::modules::logger::log_info(&format!(
            "[Router] 自定义映射(规范化): {} -> {}",
            original_model, target
        ));
        return Some(target.clone());
    }
    for (k, target) in custom_mapping.iter() {
        let k_canonical = canonicalize_claude_client_model_id(k);
        if k_canonical == canonical || k.to_lowercase() == canonical {
            crate::modules::logger::log_info(&format!(
                "[Router] 自定义映射(规范化对齐): {} -> {}",
                original_model, target
            ));
            return Some(target.clone());
        }
    }

    // 2. Wildcard match - most specific (highest non-wildcard chars) wins
    // Note: When multiple patterns have the SAME specificity, HashMap iteration order
    // determines the result (non-deterministic). Users can avoid this by making patterns
    // more specific. Future improvement: use IndexMap + frontend sorting for full control.
    let mut best_match: Option<(&str, &str, usize)> = None;

    for (pattern, target) in custom_mapping.iter() {
        if pattern.contains('*')
            && (wildcard_match(pattern, original_model)
                || wildcard_match(&pattern.to_lowercase(), &original_lower))
        {
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
        return Some(target.to_string());
    }

    None
}

/// 解析号池自适应暗路由目标 (auto:*)
///
/// 规则：
/// 1. `auto:gemini-flash-low`:
///    优先复用 `pick_background_model_from_pool` 动态挑选非 Pro 账号共享桶中最新 `gemini-<x.x>-flash-low`；
///    未同步或未找到时兜底返回最新官方基准轻量模型 `gemini-3.8-flash-low`。
/// 2. `auto:claude-opus`:
///    若号池拥有正配额的 `claude-opus-5-5`（或其档位变体），返回 `claude-opus-5-5`；
///    否则若号池拥有正配额的 `claude-opus-4-6` / `claude-opus-4-6-thinking`，返回 `claude-opus-4-6-thinking`；
///    若全池 Opus 耗尽，自动降级至池内其他正配额 Claude 思考模型（如 `claude-sonnet-5-5` 或 `claude-sonnet-4-6`）以防 502；
///    兜底返回 `claude-opus-5-5`。
/// 3. `auto:claude-opus-4-7`:
///    原生 4-7 让路直连；混合池保持 `claude-opus-4-6-thinking` 节省 5.5 额度；
///    纯 5.5 池或未来 4.6 下架后自动升级至 `claude-opus-5-5`；
///    兜底返回 `claude-opus-4-6-thinking`。
pub fn resolve_adaptive_target(
    target: &str,
    requested_model: &str,
    _effort: Option<&str>,
    token_manager: Option<&crate::proxy::token_manager::TokenManager>,
) -> String {
    if !target.starts_with("auto:") {
        return target.to_string();
    }

    match target {
        "auto:gemini-flash-low" => {
            if let Some(tm) = token_manager {
                if let Some(bg) =
                    crate::proxy::common::model_gate::pick_background_model_from_pool(tm)
                {
                    crate::modules::logger::log_info(&format!(
                        "[AdaptiveRoute] auto:gemini-flash-low (requested: {}) 动态挑选轻量模型: {}",
                        requested_model, bg
                    ));
                    return bg;
                }
            }
            "gemini-3.8-flash-low".to_string()
        }
        "auto:claude-opus" => {
            if let Some(tm) = token_manager {
                let positive_models = tm.models_with_positive_quota();
                // 1. 优先 5.5
                if positive_models
                    .iter()
                    .any(|m| m.contains("opus") && m.contains("5-5"))
                {
                    return "claude-opus-5-5".to_string();
                }
                // 2. 其次 4.6
                if positive_models
                    .iter()
                    .any(|m| m.contains("opus") && m.contains("4-6"))
                {
                    return "claude-opus-4-6-thinking".to_string();
                }
                // 3. 全池 Opus 耗尽保底：寻找其他正配额 Claude 模型防 502
                if let Some(other_claude) = positive_models
                    .iter()
                    .find(|m| m.contains("claude") || m.contains("sonnet"))
                {
                    crate::modules::logger::log_info(&format!(
                        "[AdaptiveRoute] auto:claude-opus (requested: {}) Opus无额度，保底调度至: {}",
                        requested_model, other_claude
                    ));
                    return other_claude.clone();
                }
            }
            "claude-opus-5-5".to_string()
        }
        "auto:claude-opus-4-7" => {
            if let Some(tm) = token_manager {
                let positive_models = tm.models_with_positive_quota();
                // 1. 原生 4-7 支持时直连
                if positive_models
                    .iter()
                    .any(|m| m.contains("opus") && m.contains("4-7"))
                {
                    return "claude-opus-4-7".to_string();
                }
                // 2. 优先 4-6 节省 5.5 配额
                if positive_models
                    .iter()
                    .any(|m| m.contains("opus") && m.contains("4-6"))
                {
                    return "claude-opus-4-6-thinking".to_string();
                }
                // 3. 纯 5.5 池或 4.6 下架时升级至 5.5
                if positive_models
                    .iter()
                    .any(|m| m.contains("opus") && m.contains("5-5"))
                {
                    return "claude-opus-5-5".to_string();
                }
            }
            "claude-opus-4-6-thinking".to_string()
        }
        _ => target.strip_prefix("auto:").unwrap_or(target).to_string(),
    }
}

/// 解析完整的模型路由（支持官方淘汰重定向、用户自定义映射、通配符映射、自适应暗路由、物理 ID 透传以及依据客户端思考档位进行通用裸模型解析）
///
/// # 参数
/// * `original_model` - 原始模型名称
/// * `custom_mapping` - 用户自定义映射表
/// * `client_effort` - 客户端指定的思考强度/档位（如 Some("low"), Some("high") 等）
/// * `token_manager` - 可选号池管理器指针，用于驱动 `auto:*` 自适应号池探测
///
/// # 返回
/// 映射后的目标模型名称
pub fn resolve_model_route_with_effort_and_pool(
    original_model: &str,
    custom_mapping: &std::collections::HashMap<String, String>,
    client_effort: Option<&str>,
    token_manager: Option<&crate::proxy::token_manager::TokenManager>,
) -> String {
    // 0 & 1 & 2. 优先匹配淘汰重定向与自定义映射（精确、大小写、规范化、通配符）
    if let Some(custom) = resolve_custom_model_route(original_model, custom_mapping) {
        // 自适应暗路由解析 (auto:*)
        let target = if custom.starts_with("auto:") {
            resolve_adaptive_target(&custom, original_model, client_effort, token_manager)
        } else {
            custom
        };

        // 裸模型自定义映射目标继续穿透客户端思考档位（如映射至 claude-opus-5-5 且 effort=high -> claude-opus-5-5-high）
        if let Some(routed) =
            crate::proxy::model_specs::resolve_bare_tiered_model_route(&target, client_effort)
        {
            crate::modules::logger::log_info(&format!(
                "[Router] 自定义映射目标依据思考档位路由: {} (target={}, effort={:?}) -> {}",
                original_model, target, client_effort, routed
            ));
            return map_claude_model_to_gemini(&routed);
        }

        return map_claude_model_to_gemini(&target);
    }

    // 3. 系统默认映射
    // Variant 已经写出的上游真实 id 在此停住，避免公开名反向覆盖（Issue #3551）。
    if crate::proxy::common::variant_mapping::is_physical_upstream_id(original_model) {
        // The physical-ID check also accepts tier suffixes on public aliases.
        // Normalize those aliases without selecting a different effort tier.
        return map_claude_model_to_gemini(original_model);
    }

    // 裸模型依据客户端思考档位通用路由（覆盖 3.x Flash 与 Claude >= 5.0）：
    // - 对于 Flash：默认 high，支持 low / medium
    // - 对于 Claude：默认 medium，支持 low / high
    let target_model = map_claude_model_to_gemini(original_model);
    let target_ref = if target_model != original_model {
        target_model.as_str()
    } else {
        original_model
    };

    if let Some(routed) =
        crate::proxy::model_specs::resolve_bare_tiered_model_route(target_ref, client_effort)
    {
        crate::modules::logger::log_info(&format!(
            "[Router] 裸模型依据思考档位通用路由: {} (target={}, effort={:?}) -> {}",
            original_model, target_ref, client_effort, routed
        ));
        return map_claude_model_to_gemini(&routed);
    }

    if target_model != original_model {
        crate::modules::logger::log_info(&format!(
            "[Router] 系统默认映射: {} -> {}",
            original_model, target_model
        ));
        return target_model;
    }
    target_model
}

pub fn resolve_model_route_with_effort(
    original_model: &str,
    custom_mapping: &std::collections::HashMap<String, String>,
    client_effort: Option<&str>,
) -> String {
    resolve_model_route_with_effort_and_pool(original_model, custom_mapping, client_effort, None)
}

/// Normalize any physical model name to one of the 3 standard protection IDs.
/// This ensures quota protection works consistently regardless of API versioning or request variations.
///
/// Standard IDs:
/// - `gemini-3-flash`: All Flash variants (1.5-flash, 2.5-flash, 3-flash, etc.)
/// - `gemini-3.1-flash-image`: Flash image generation/edit quota.
/// - `gemini-3-pro-high`: All Pro variants (1.5-pro, 2.5-pro, etc.)
/// - `gemini-3-pro-image`: Pro image generation quota.
/// - `claude-sonnet-4-5`: All Claude Sonnet variants (3-5-sonnet, sonnet-4-5, etc.)
///
/// Returns `None` if the model doesn't match any of the 3 protected categories.
/// 判断是否为 Gemini 3.5 Flash 及以上的高阶 Flash 模型（与 3.1 Pro 共享高级配额）
/// 严格语义通配：gemini-{ver}-flash*，当版本数值 ver >= 3.5 时生效（支持未来任意 3.10、4.x 等）
fn is_high_tier_flash(lower: &str) -> bool {
    if !lower.contains("flash") {
        return false;
    }

    if let Some(pos) = lower.find("gemini-") {
        let rest = &lower[pos + 7..];
        if let Some(flash_pos) = rest.find("-flash") {
            let ver_str = &rest[..flash_pos];
            if let Some(ver) = parse_version_tuple(ver_str) {
                return ver >= (3, 5);
            }
        }
    }

    false
}

/// 通用档位后缀剥离：从模型 ID 末尾去掉已知的思考档位/变体后缀，返回基底模型名。
///
/// 设计原则：只剥离最外层一个后缀（避免误切），大小写不敏感，不依赖任何硬编码模型名称。
///
/// 示例：
/// - `gemini-4-argon-high`        → `gemini-4-argon`
/// - `claude-sonnet-5-5-medium`   → `claude-sonnet-5-5`
/// - `gemini-3.8-flash-tiered`    → `gemini-3.8-flash`
/// - `gemini-3-flash-agent`       → `gemini-3-flash-agent`（无匹配后缀，原样返回）
/// - `some-model@default`         → `some-model`
pub fn strip_tier_suffix(model: &str) -> String {
    const TIER_SUFFIXES: &[&str] = &[
        "-extra-low",
        "-xhigh",
        "-high",
        "-medium",
        "-low",
        "-thinking",
        "-tiered",
        "@default",
    ];
    let lower = model.trim().to_lowercase();
    for suffix in TIER_SUFFIXES {
        if lower.ends_with(suffix) {
            // 剥离后缀（保留原始大小写前缀）
            let prefix_len = model.len() - suffix.len();
            return model[..prefix_len].to_string();
        }
    }
    model.trim().to_string()
}

pub fn normalize_to_standard_id(model_name: &str) -> Option<String> {
    let lower = model_name.to_lowercase();

    // 排除被标记禁用的变体条目（如 gemini-4-argon-disabled）
    if lower.contains("-disabled") || lower.ends_with("_disabled") {
        return None;
    }

    // 0. Gemini 4 Argon 系列（独立配额组与独立计费桶，所有档位变体如 high/medium/low/thinking 均归一化为基底 ID）
    if lower.contains("argon") {
        return Some("gemini-4-argon".to_string());
    }

    // 1. Image resources must keep Flash image and Pro image in separate quota buckets.
    // The quota API exposes `gemini-3.1-flash-image` separately, so grouping it under
    // `gemini-3-pro-image` makes available Flash image quota look exhausted.
    if lower.contains("image") {
        if lower.contains("flash") {
            return Some("gemini-3.1-flash-image".to_string());
        }
        return Some("gemini-3-pro-image".to_string());
    }

    // 2. 3.5 Flash 及以上的高阶 Flash 模型（如 3.5-flash, 3.7-flash, 3.8-flash 等）与 3.1 Pro 共享高级配额
    if is_high_tier_flash(&lower) {
        return Some("gemini-3-pro-high".to_string());
    }

    // 3. gemini-3-flash (包含普通 1.5-flash, 2.0-flash, 2.5-flash, 3.0-flash 等基础 flash 变体)
    if lower.contains("flash") {
        return Some("gemini-3-flash".to_string());
    }

    // 4. gemini-3-pro-high (包含 pro 变体)
    if lower.contains("pro") && !lower.contains("image") {
        return Some("gemini-3-pro-high".to_string());
    }

    // 4. Claude 系列 (上游同一账号下共享配额池，统一归一化为 'claude' 保护组)
    if lower.contains("claude")
        || lower.contains("opus")
        || lower.contains("sonnet")
        || lower.contains("haiku")
        || lower.contains("fable")
    {
        return Some("claude".to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_mapping() {
        assert_eq!(
            map_claude_model_to_gemini("claude-opus-4.6"),
            "claude-opus-4-6-thinking"
        );
        assert_eq!(
            map_claude_model_to_gemini("anthropic/claude-sonnet-4.6"),
            "claude-sonnet-4-6"
        );
        assert_eq!(
            map_claude_model_to_gemini("claude-sonnet-4.6-thinking"),
            "claude-sonnet-4-6"
        );
        assert_eq!(
            map_claude_model_to_gemini("claude-opus-4-6-thinking"),
            "claude-opus-4-6-thinking"
        );
        assert_eq!(
            map_claude_model_to_gemini("gpt-oss-120b"),
            "gpt-oss-120b-medium"
        );
        assert_eq!(
            map_claude_model_to_gemini("internal-background-task"),
            "gemini-3.1-flash-lite"
        );
        assert_eq!(map_claude_model_to_gemini("unknown-model"), "unknown-model");
    }

    #[tokio::test]
    async fn test_get_all_dynamic_models_only_raw_quota_models() {
        let custom_mapping = tokio::sync::RwLock::new(
            [(
                "my-custom-model".to_string(),
                "gemini-3.1-pro-high".to_string(),
            )]
            .into_iter()
            .collect(),
        );

        // When only_raw_quota_models is TRUE, custom_mapping should be filtered out
        let models_raw = get_all_dynamic_models(&custom_mapping, None, true).await;
        assert!(!models_raw.contains(&"my-custom-model".to_string()));

        // When only_raw_quota_models is FALSE, custom_mapping should be included
        let models_all = get_all_dynamic_models(&custom_mapping, None, false).await;
        assert!(models_all.contains(&"my-custom-model".to_string()));
        // 动态官方模型与自派生无后缀模型必须包含
        assert!(models_all.contains(&"claude-sonnet-4-6".to_string()));
        assert!(models_all.contains(&"claude-opus-4-6-thinking".to_string()));
        assert!(models_all.contains(&"claude-opus-4-6".to_string()));
        assert!(models_all.contains(&"claude-opus-5-5".to_string()));
        assert!(models_all.contains(&"claude-opus-5-5-high".to_string()));
        assert!(models_all.contains(&"claude-sonnet-5-5".to_string()));
        assert!(models_all.contains(&"gemini-4-argon".to_string()));
        assert!(models_all.contains(&"gemini-4-argon-high".to_string()));
        assert!(models_all.contains(&"gemini-3.8-flash".to_string()));
        assert!(models_all.contains(&"gemini-3.8-flash-high".to_string()));
        assert!(models_all.contains(&"gpt-oss-120b".to_string()));
        assert!(models_all.contains(&"gpt-oss-120b-medium".to_string()));
        // 非法硬编码或已淘汰模型绝不泄露暴露
        assert!(!models_all.contains(&"claude-sonnet-4-6-thinking".to_string()));
        assert!(!models_all.contains(&"gemini-2.5-flash".to_string()));
        assert!(!models_all.contains(&"gemini-3.7-flash-high".to_string()));
        assert!(!models_all.contains(&"gemini-pro-agent".to_string()));
    }

    #[tokio::test]
    async fn test_find_dynamic_model() {
        let custom_mapping = tokio::sync::RwLock::new(
            [(
                "custom-gpt4".to_string(),
                "gemini-3.8-flash-high".to_string(),
            )]
            .into_iter()
            .collect(),
        );

        // 1. 内置模型匹配
        let found = find_dynamic_model(&custom_mapping, None, false, "gemini-3.1-flash-lite").await;
        assert_eq!(found, Some("gemini-3.1-flash-lite".to_string()));

        // 2. 带 models/ 前缀匹配
        let found_prefix =
            find_dynamic_model(&custom_mapping, None, false, "models/gemini-3.1-flash-lite").await;
        assert_eq!(found_prefix, Some("gemini-3.1-flash-lite".to_string()));

        // 3. 自定义模型匹配
        let found_custom = find_dynamic_model(&custom_mapping, None, false, "custom-gpt4").await;
        assert_eq!(found_custom, Some("custom-gpt4".to_string()));

        // 4. 大小写宽容匹配
        let found_case =
            find_dynamic_model(&custom_mapping, None, false, "GEMINI-3.1-FLASH-LITE").await;
        assert_eq!(found_case, Some("gemini-3.1-flash-lite".to_string()));

        // 5. 不存在的模型
        let not_found =
            find_dynamic_model(&custom_mapping, None, false, "non-existent-model").await;
        assert_eq!(not_found, None);
    }

    #[test]
    fn test_mappings_continued() {
        assert_eq!(
            map_claude_model_to_gemini("gemini-3.1-pro-high"),
            "gemini-3.1-pro-high"
        );
        assert_eq!(
            map_claude_model_to_gemini("gemini-3.1-pro-low"),
            "gemini-3.1-pro-low"
        );
        assert_eq!(
            map_claude_model_to_gemini("gemini-3.1-flash-lite"),
            "gemini-3.1-flash-lite"
        );
        assert_eq!(
            map_claude_model_to_gemini("gemini-3.5-flash-lite"),
            "gemini-3.5-flash-lite"
        );
        assert_eq!(
            map_claude_model_to_gemini("internal-background-task"),
            "gemini-3.1-flash-lite"
        );
        let empty = HashMap::new();
        assert_eq!(
            resolve_model_route("gemini-3.1-flash-lite", &empty),
            "gemini-3.1-flash-lite"
        );

        // Test Normalization (Claude 系列全部归一化至统一保护组 'claude')
        assert_eq!(
            normalize_to_standard_id("claude-opus-4-6-thinking"),
            Some("claude".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("claude-sonnet-4-5"),
            Some("claude".to_string())
        );

        // [Regression] gemini-3-pro-image must NOT be grouped with gemini-3-pro-high
        assert_eq!(
            normalize_to_standard_id("gemini-3-pro-image"),
            Some("gemini-3-pro-image".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("gemini-3-pro-high"),
            Some("gemini-3-pro-high".to_string())
        );

        // [FIX #1955] Test normalization with image suffixes
        assert_eq!(
            normalize_to_standard_id("gemini-3-pro-image-4k"),
            Some("gemini-3-pro-image".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("gemini-3-pro-image-16x9"),
            Some("gemini-3-pro-image".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("gemini-3-pro-image-4k-16x9"),
            Some("gemini-3-pro-image".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("gemini-3.1-flash-image"),
            Some("gemini-3.1-flash-image".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("gemini-3.1-flash-image-4k"),
            Some("gemini-3.1-flash-image".to_string())
        );
    }

    #[test]
    fn test_wildcard_priority() {
        let mut custom = HashMap::new();
        custom.insert("gpt*".to_string(), "fallback".to_string());
        custom.insert("gpt-4*".to_string(), "specific".to_string());
        custom.insert("claude-opus-*".to_string(), "opus-default".to_string());
        custom.insert(
            "claude-opus*thinking".to_string(),
            "opus-thinking".to_string(),
        );

        // More specific pattern wins
        assert_eq!(resolve_model_route("gpt-4-turbo", &custom), "specific");
        assert_eq!(resolve_model_route("gpt-3.5", &custom), "fallback");
        // Suffix constraint is more specific than prefix-only
        assert_eq!(
            resolve_model_route("claude-opus-4-5-thinking", &custom),
            "opus-thinking"
        );
        assert_eq!(
            resolve_model_route("claude-opus-4", &custom),
            "opus-default"
        );
    }

    #[test]
    fn test_multi_wildcard_support() {
        let mut custom = HashMap::new();
        custom.insert(
            "claude-*-sonnet-*".to_string(),
            "sonnet-versioned".to_string(),
        );
        custom.insert("gpt-*-*".to_string(), "gpt-multi".to_string());
        custom.insert("*thinking*".to_string(), "has-thinking".to_string());

        // Multi-wildcard patterns should work
        assert_eq!(
            resolve_model_route("claude-3-5-sonnet-20241022", &custom),
            "sonnet-versioned"
        );
        assert_eq!(
            resolve_model_route("gpt-4-turbo-preview", &custom),
            "gpt-multi"
        );
        assert_eq!(
            resolve_model_route("claude-thinking-extended", &custom),
            "has-thinking"
        );

        // Negative case: *thinking* should NOT match models without "thinking"
        assert_eq!(
            resolve_model_route("random-model-name", &custom),
            "random-model-name" // Falls back to system default (pass-through)
        );
    }

    #[test]
    fn test_wildcard_edge_cases() {
        let mut custom = HashMap::new();
        custom.insert("prefix*".to_string(), "prefix-match".to_string());
        custom.insert("*".to_string(), "catch-all".to_string());
        custom.insert("a*b*c".to_string(), "multi-wild".to_string());

        // Specificity: "prefix*" (6) > "*" (0)
        assert_eq!(
            resolve_model_route("prefix-anything", &custom),
            "prefix-match"
        );
        // Catch-all has lowest specificity
        assert_eq!(resolve_model_route("random-model", &custom), "catch-all");
        // Multi-wildcard: "a*b*c" (3)
        assert_eq!(resolve_model_route("a-test-b-foo-c", &custom), "multi-wild");
    }

    #[test]
    fn test_gemini_3x_flash_wildcard_route() {
        let mut custom = HashMap::new();

        // 1. 3.x Flash 裸模型依据思考档位路由 (未指定时优先遵循决策链 tiered，显式传档位时路由至对应档位)
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.8-flash", &custom, Some("high")),
            "gemini-3.8-flash-high"
        );
        assert_eq!(
            resolve_model_route("gemini-3.8-flash", &custom),
            "gemini-3.8-flash-tiered"
        );
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.8-flash", &custom, Some("low")),
            "gemini-3.8-flash-low"
        );
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.8-flash", &custom, Some("medium")),
            "gemini-3.8-flash-medium"
        );
        // tiered 模型不动模型名
        assert_eq!(
            resolve_model_route("gemini-3.8-flash-tiered", &custom),
            "gemini-3.8-flash-tiered"
        );

        // 2. 通用通配符映射（用户自定义）
        custom.insert(
            "gemini-3.9-*".to_string(),
            "gemini-3.8-flash-tiered".to_string(),
        );
        assert_eq!(
            resolve_model_route("gemini-3.9-flash", &custom),
            "gemini-3.8-flash-tiered"
        );
    }

    #[test]
    fn test_is_model_compliant_with_baseline() {
        // Claude: >= 4.6 passes (4.6, 4.7 & 5.5), Fable >= 5.0
        assert!(is_model_compliant_with_baseline("claude-sonnet-4-6"));
        assert!(is_model_compliant_with_baseline("claude-opus-4-6-thinking"));
        assert!(is_model_compliant_with_baseline("claude-opus-4-6"));
        assert!(is_model_compliant_with_baseline("claude-opus-4-7"));
        assert!(is_model_compliant_with_baseline("claude-opus-5-5"));
        assert!(is_model_compliant_with_baseline("claude-opus-5-5-high"));
        assert!(is_model_compliant_with_baseline("claude-sonnet-5-5"));
        assert!(is_model_compliant_with_baseline("claude-sonnet-5-5-medium"));
        assert!(is_model_compliant_with_baseline("claude-haiku-5-5"));
        assert!(is_model_compliant_with_baseline("claude-fable-5"));
        assert!(is_model_compliant_with_baseline("claude-fable-5-1"));
        assert!(!is_model_compliant_with_baseline("claude-haiku-4-5"));
        assert!(!is_model_compliant_with_baseline(
            "claude-haiku-4-5-20251001"
        ));
        assert!(!is_model_compliant_with_baseline(
            "claude-sonnet-4-6-thinking"
        ));
        assert!(!is_model_compliant_with_baseline(
            "claude-opus-5-5-thinking"
        ));
        assert!(!is_model_compliant_with_baseline("claude-sonnet-4-5"));
        assert!(!is_model_compliant_with_baseline("claude-3-5-sonnet"));
        assert!(!is_model_compliant_with_baseline("claude-3-7-sonnet"));
        assert!(!is_model_compliant_with_baseline("claude-3-5-haiku"));
        assert!(!is_model_compliant_with_baseline("claude-3-haiku"));
        assert!(!is_model_compliant_with_baseline("claude-fable-4"));

        // GPT: gpt-oss-120b-medium & derived gpt-oss-120b pass
        assert!(is_model_compliant_with_baseline("gpt-oss-120b-medium"));
        assert!(is_model_compliant_with_baseline("gpt-oss-120b"));
        assert!(!is_model_compliant_with_baseline("gpt-4o"));
        assert!(!is_model_compliant_with_baseline("gpt-4"));
        assert!(!is_model_compliant_with_baseline("gpt-3.5-turbo"));

        // Gemini 4 Argon & Flash >= 3.8 pass, < 3.8 rejected (except 3.1/3.5-flash-lite)
        assert!(is_model_compliant_with_baseline("gemini-4-argon"));
        assert!(is_model_compliant_with_baseline("gemini-4-argon-high"));
        assert!(!is_model_compliant_with_baseline("gemini-4-argon-disabled"));
        assert!(!is_model_compliant_with_baseline(
            "gemini-3.8-flash-disabled"
        ));
        assert!(is_model_compliant_with_baseline("gemini-3.8-flash-high"));
        assert!(is_model_compliant_with_baseline("gemini-3.8-flash"));
        assert!(is_model_compliant_with_baseline("gemini-3.10-flash-high"));
        assert!(is_model_compliant_with_baseline("gemini-3.1-flash-lite"));
        assert!(is_model_compliant_with_baseline("gemini-3.5-flash-lite"));
        assert!(!is_model_compliant_with_baseline("gemini-3.7-flash-medium"));
        assert!(!is_model_compliant_with_baseline("gemini-3.6-flash-low"));
        assert!(!is_model_compliant_with_baseline("gemini-3.5-flash-low"));
        assert!(!is_model_compliant_with_baseline("gemini-2.5-flash"));
        assert!(!is_model_compliant_with_baseline("gemini-2.5-flash-lite"));
        assert!(!is_model_compliant_with_baseline(
            "gemini-2.5-flash-thinking"
        ));
        assert!(!is_model_compliant_with_baseline("gemini-1.5-flash"));
        assert!(!is_model_compliant_with_baseline("gemini-2.0-flash"));

        // is_high_tier_flash verification
        assert!(is_high_tier_flash("gemini-3.5-flash"));
        assert!(is_high_tier_flash("gemini-3.10-flash"));
        assert!(is_high_tier_flash("gemini-4.0-flash"));
        assert!(!is_high_tier_flash("gemini-3.0-flash"));
        assert!(!is_high_tier_flash("gemini-2.5-flash"));

        // Date snapshots must be filtered
        assert!(!is_model_compliant_with_baseline(
            "claude-3-5-sonnet-20241022"
        ));

        // Gemini Pro: 3.1 pass, 2.5/1.5/2.0/3.0 rejected
        assert!(is_model_compliant_with_baseline("gemini-3.1-pro-high"));
        assert!(is_model_compliant_with_baseline("gemini-3.1-pro-low"));
        assert!(is_model_compliant_with_baseline("gemini-3.1-pro"));
        assert!(!is_model_compliant_with_baseline("gemini-2.5-pro"));
        assert!(!is_model_compliant_with_baseline("gemini-1.5-pro"));
        assert!(!is_model_compliant_with_baseline("gemini-2.0-pro"));
        assert!(!is_model_compliant_with_baseline("gemini-3-pro"));
        assert!(!is_model_compliant_with_baseline("gemini-3.1-pro-preview"));

        // [2026-10-29] Gemini 主版本 < 3 全量下线：对外模型列表不得含任何 2.x（含 2.5 Pro）
        assert!(!is_model_compliant_with_baseline("gemini-2.5-pro"));
        assert!(
            !get_supported_models()
                .iter()
                .any(|m| m.starts_with("gemini-2.")),
            "对外模型列表不得包含任何 Gemini 2.x（含 2.5 Pro）"
        );

        // Image models
        assert!(is_model_compliant_with_baseline("gemini-3.1-flash-image"));
        assert!(is_model_compliant_with_baseline("gemini-3-pro-image"));

        // Internal / Deprecated models
        assert!(!is_model_compliant_with_baseline("chat_20706"));
        assert!(!is_model_compliant_with_baseline("chat_23310"));
        assert!(!is_model_compliant_with_baseline(
            "tab_jump_flash_lite_preview"
        ));
        assert!(!is_model_compliant_with_baseline("tab_flash_lite_preview"));
        assert!(!is_model_compliant_with_baseline("gemini-pro-agent"));
        assert!(!is_model_compliant_with_baseline("gemini-3-flash-agent"));
        assert!(!is_model_compliant_with_baseline("gemini-3-flash"));
        assert!(!is_model_compliant_with_baseline("gemini-3.5-flash"));
    }

    #[test]
    fn test_bare_model_tiered_routing() {
        let empty = HashMap::new();

        // 1. 衍生裸模型 gemini-3.1-pro 显式传递 effort
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.1-pro", &empty, Some("low")),
            "gemini-3.1-pro-low"
        );
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.1-pro", &empty, Some("high")),
            "gemini-3.1-pro-high"
        );

        for effort in ["medium", "xhigh", "max"] {
            assert_eq!(
                resolve_model_route_with_effort("gemini-3.1-pro", &empty, Some(effort)),
                "gemini-3.1-pro-high"
            );
        }

        // 2. 衍生裸模型 gemini-3.1-pro 缺省 effort 遵循 pick_optimal_default_tier 决策链 (取高于 low 的最低档 -> high)
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.1-pro", &empty, None),
            "gemini-3.1-pro-high"
        );

        // 3. 衍生裸模型 claude-opus-5-5 / gemini-4-argon 缺省 effort 遵循决策链 (取 medium)
        assert_eq!(
            resolve_model_route_with_effort("claude-opus-5-5", &empty, None),
            "claude-opus-5-5-medium"
        );
        assert_eq!(
            resolve_model_route_with_effort("claude-opus-5-5", &empty, Some("high")),
            "claude-opus-5-5-high"
        );
        assert_eq!(
            resolve_model_route_with_effort("gemini-4-argon", &empty, None),
            "gemini-4-argon-medium"
        );
        assert_eq!(
            resolve_model_route_with_effort("gemini-4-argon", &empty, Some("high")),
            "gemini-4-argon-high"
        );
        assert_eq!(
            resolve_model_route_with_effort("claude-opus-4-6", &empty, None),
            "claude-opus-4-6-thinking"
        );

        // 4. 显式档位保持不变
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.1-pro-low", &empty, None),
            "gemini-3.1-pro-low"
        );
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.1-pro-high", &empty, None),
            "gemini-3.1-pro-high"
        );
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.1-pro-high", &empty, Some("low")),
            "gemini-3.1-pro-high"
        );

        // 5. Claude Code 2.1.312 自适应暗路由模型档位路由 (基于 default_custom_mapping)
        let default_custom = crate::proxy::config::default_custom_mapping();
        assert_eq!(
            resolve_model_route_with_effort("claude-fable-5", &default_custom, None),
            "claude-opus-5-5-medium"
        );
        assert_eq!(
            resolve_model_route_with_effort("claude-fable-5", &default_custom, Some("high")),
            "claude-opus-5-5-high"
        );
        assert_eq!(
            resolve_model_route_with_effort("claude-fable-5-1", &default_custom, None),
            "claude-opus-5-5-medium"
        );
        assert_eq!(
            resolve_model_route_with_effort("claude-haiku-4-5", &default_custom, None),
            "gemini-3.8-flash-low"
        );
        assert_eq!(
            resolve_model_route_with_effort("claude-haiku-4-5-20251001", &default_custom, None),
            "gemini-3.8-flash-low"
        );
        assert_eq!(
            resolve_model_route_with_effort("claude-haiku-5-5", &default_custom, None),
            "gemini-3.8-flash-low"
        );
        assert_eq!(
            resolve_model_route_with_effort("claude-opus-4-7", &default_custom, None),
            "claude-opus-4-6-thinking"
        );
    }

    #[test]
    fn test_resolve_adaptive_target() {
        assert_eq!(
            resolve_adaptive_target("auto:gemini-flash-low", "claude-haiku-4-5", None, None),
            "gemini-3.8-flash-low"
        );
        assert_eq!(
            resolve_adaptive_target("auto:claude-opus", "claude-fable-5", None, None),
            "claude-opus-5-5"
        );
        assert_eq!(
            resolve_adaptive_target("auto:claude-opus-4-7", "claude-opus-4-7", None, None),
            "claude-opus-4-6-thinking"
        );
    }

    #[test]
    fn test_tiered_routing_preserves_custom_and_physical_model_ids() {
        let custom = HashMap::from([(
            "gemini-3.1-pro".to_string(),
            "custom-upstream-model".to_string(),
        )]);
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.1-pro", &custom, Some("high")),
            "custom-upstream-model"
        );

        // A physical ID selected by a protocol's variant resolver must not be
        // reinterpreted using a conflicting client effort.
        assert_eq!(
            resolve_model_route_with_effort("gemini-pro-agent", &HashMap::new(), Some("low")),
            "gemini-pro-agent"
        );
    }

    #[test]
    fn test_canonicalize_claude_5_major_models() {
        assert_eq!(
            canonicalize_claude_client_model_id("claude-opus-5"),
            "claude-opus-5-5"
        );
        assert_eq!(
            canonicalize_claude_client_model_id("claude-sonnet-5"),
            "claude-sonnet-5-5"
        );
        assert_eq!(
            canonicalize_claude_client_model_id("claude-haiku-5"),
            "claude-haiku-5-5"
        );
        assert_eq!(
            canonicalize_claude_client_model_id("anthropic/claude-opus-5"),
            "claude-opus-5-5"
        );
        assert_eq!(
            canonicalize_claude_client_model_id("claude-opus-5-low"),
            "claude-opus-5-5-low"
        );
        assert_eq!(
            canonicalize_claude_client_model_id("claude-fable-5[1m]"),
            "claude-fable-5"
        );
        assert_eq!(
            canonicalize_claude_client_model_id("claude-opus-5[1m]"),
            "claude-opus-5-5"
        );
        assert_eq!(
            canonicalize_claude_client_model_id("claude-5-fable"),
            "claude-fable-5"
        );
        assert_eq!(
            canonicalize_claude_client_model_id("anthropic/claude-fable-5[1m]"),
            "claude-fable-5"
        );
    }

    #[test]
    fn test_custom_mapping_case_insensitive_and_canonical() {
        let mut custom = HashMap::new();
        custom.insert(
            "claude-opus-5".to_string(),
            "gemini-3.8-flash-medium".to_string(),
        );

        // 大小写不敏感匹配
        assert_eq!(
            resolve_model_route_with_effort("Claude-Opus-5", &custom, None),
            "gemini-3.8-flash-medium"
        );
        // 精确匹配
        assert_eq!(
            resolve_model_route_with_effort("claude-opus-5", &custom, None),
            "gemini-3.8-flash-medium"
        );
    }

    #[test]
    fn test_normalize_to_standard_id_claude_unified_pool() {
        // Claude 系列在同一账号内共享配额池，统一归一化为 'claude' 保护组
        assert_eq!(
            normalize_to_standard_id("claude-opus-5-5-medium"),
            Some("claude".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("claude-opus-4-6-thinking"),
            Some("claude".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("claude-sonnet-5-5-high"),
            Some("claude".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("claude-sonnet-4-6"),
            Some("claude".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("claude-haiku-4-5"),
            Some("claude".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("claude-fable-5"),
            Some("claude".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("claude"),
            Some("claude".to_string())
        );
    }

    #[test]
    fn test_normalize_to_standard_id_argon_unified_pool() {
        // Gemini 4 Argon 系列独立计费与熔断，统一归一化为 'gemini-4-argon' 标准桶
        assert_eq!(
            normalize_to_standard_id("gemini-4-argon-high"),
            Some("gemini-4-argon".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("gemini-4-argon-medium"),
            Some("gemini-4-argon".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("gemini-4-argon-low"),
            Some("gemini-4-argon".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("gemini-4-argon-thinking"),
            Some("gemini-4-argon".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("gemini-4-argon"),
            Some("gemini-4-argon".to_string())
        );
        assert_eq!(normalize_to_standard_id("gemini-4-argon-disabled"), None);
    }

    #[test]
    fn test_clean_tier_from_display_name() {
        assert_eq!(
            clean_tier_from_display_name("Gemini 3.1 Pro (Low)"),
            "Gemini 3.1 Pro"
        );
        assert_eq!(
            clean_tier_from_display_name("Claude 3.7 Sonnet (High)"),
            "Claude 3.7 Sonnet"
        );
        assert_eq!(
            clean_tier_from_display_name("Gemini 3.7 Flash"),
            "Gemini 3.7 Flash"
        );
    }

    #[tokio::test]
    async fn test_get_all_dynamic_models_metadata_derives_bare_models() {
        let custom = tokio::sync::RwLock::new(std::collections::HashMap::new());
        let meta_map = get_all_dynamic_models_metadata(&custom, None, false).await;
        // 验证能从内置官方目录成功提取模型元数据
        if let Some(flash_meta) = meta_map.get("gemini-2.5-flash") {
            assert!(flash_meta.display_name.is_some() || flash_meta.supports_images.is_some());
        }
    }
}
