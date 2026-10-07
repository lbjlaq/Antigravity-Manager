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

    // ── Claude 系列核心标准映射 (基准线 >= 4.6) ──
    m.insert("claude-sonnet-4-6", "claude-sonnet-4-6");
    m.insert("claude-sonnet-4-6-thinking", "claude-sonnet-4-6-thinking");
    m.insert("claude-opus-4-6", "claude-opus-4-6-thinking");
    m.insert("claude-opus-4-6-thinking", "claude-opus-4-6-thinking");
    // 兼容历史老旧 Claude 模型重定向至 4.6
    m.insert("claude-sonnet-4-5", "claude-sonnet-4-6");
    m.insert("claude-sonnet-4-5-thinking", "claude-sonnet-4-6-thinking");
    m.insert("claude-opus-4-5-thinking", "claude-opus-4-6-thinking");
    m.insert("claude-haiku-4-5", "claude-sonnet-4-6");
    m.insert("claude-haiku-4", "claude-sonnet-4-6");
    m.insert("claude-3-5-sonnet", "claude-sonnet-4-6");
    m.insert("claude-3-5-sonnet-latest", "claude-sonnet-4-6");
    m.insert("claude-3-7-sonnet", "claude-sonnet-4-6-thinking");
    m.insert("claude-3-7-sonnet-latest", "claude-sonnet-4-6-thinking");

    // ── OpenAI 核心映射 (转至当前基准模型) ──
    m.insert("gpt-oss-120b-medium", "gpt-oss-120b-medium");
    m.insert("gpt-4o", "gemini-3.8-flash-high");
    m.insert("gpt-4o-mini", "gemini-3.8-flash-high");
    m.insert("gpt-4-turbo", "gemini-3.8-flash-high");
    m.insert("gpt-4", "gemini-3.8-flash-high");
    m.insert("gpt-3.5-turbo", "gemini-3.6-flash-medium");

    // Gemini 协议与 Claude 兼容映射表
    m.insert("gemini-2.5-flash-lite", "gemini-2.5-flash");
    m.insert("gemini-2.5-flash-thinking", "gemini-2.5-flash-thinking");
    m.insert("gemini-2.5-flash", "gemini-2.5-flash");
    m.insert("gemini-3-flash", "gemini-3-flash");

    // Gemini 3.8 Flash 系列
    m.insert("claude-3.8-flash-high", "gemini-3.8-flash-high");
    m.insert("claude-3.8-flash-medium", "gemini-3.8-flash-medium");
    m.insert("claude-3.8-flash-low", "gemini-3.8-flash-low");
    m.insert("claude-3.8-flash", "gemini-3.8-flash-medium");

    // Gemini 3.7 Flash 系列
    m.insert("claude-3.7-flash-high", "gemini-3.7-flash-high");
    m.insert("claude-3.7-flash-medium", "gemini-3.7-flash-medium");
    m.insert("claude-3.7-flash-low", "gemini-3.7-flash-low");
    m.insert("claude-3.7-flash", "gemini-3.7-flash-high");

    // Gemini 3.6 Flash 系列
    m.insert("claude-3.6-flash-high", "gemini-3.6-flash-high");
    m.insert("claude-3.6-flash-medium", "gemini-3.6-flash-medium");
    m.insert("claude-3.6-flash-low", "gemini-3.6-flash-low");
    m.insert("claude-3.6-flash", "gemini-3.6-flash-medium");

    // ── Gemini 核心标准映射 ──
    m.insert("gemini-3.8-flash-tiered", "gemini-3.8-flash-tiered");
    m.insert("gemini-3.8-flash-high", "gemini-3.8-flash-high");
    m.insert("gemini-3.8-flash-medium", "gemini-3.8-flash-medium");
    m.insert("gemini-3.8-flash-low", "gemini-3.8-flash-low");
    m.insert("gemini-3.7-flash-tiered", "gemini-3.7-flash-tiered");
    m.insert("gemini-3.7-flash-high", "gemini-3.7-flash-high");
    m.insert("gemini-3.7-flash-medium", "gemini-3.7-flash-medium");
    m.insert("gemini-3.7-flash-low", "gemini-3.7-flash-low");

    m.insert("gemini-3.6-flash-tiered", "gemini-3.6-flash-tiered");
    m.insert("gemini-3.6-flash-high", "gemini-3.6-flash-high");
    m.insert("gemini-3.6-flash-medium", "gemini-3.6-flash-medium");
    m.insert("gemini-3.6-flash-low", "gemini-3.6-flash-low");

    m.insert("gemini-3.5-flash", "gemini-3.5-flash");
    m.insert("gemini-3.5-flash-low", "gemini-3.5-flash-low");
    m.insert("gemini-3.5-flash-extra-low", "gemini-3.5-flash-extra-low");

    // 下游公开名。generate 只接受内部 id：high → gemini-pro-agent，low 保持 gemini-3.1-pro-low。
    // 不要登记反向规则（gemini-pro-agent → gemini-3.1-pro-high），否则会覆盖 Variant 的真实 id。
    m.insert("gemini-3.1-pro-high", "gemini-pro-agent");
    m.insert("gemini-3.1-pro-low", "gemini-3.1-pro-low");
    m.insert("gemini-3.1-pro", "gemini-pro-agent");
    m.insert("gemini-3.1-pro-preview", "gemini-pro-agent");
    m.insert("gemini-3-pro-high", "gemini-pro-agent");
    m.insert("gemini-3-pro-low", "gemini-3.1-pro-low");
    m.insert("gemini-3-pro", "gemini-pro-agent");

    m.insert("gemini-3.1-flash-lite", "gemini-3.1-flash-lite");
    m.insert("gemini-3.1-flash-image", "gemini-3.1-flash-image");
    m.insert("gemini-3-pro-image", "gemini-3-pro-image");

    // 历史淘汰模型重定向。保留旧版请求兼容性，平滑路由到新版健康模型。gemini-3-flash-agent 是 3.5 Flash high 的真实上游 id，保持透传。
    m.insert("gemini-3-flash", "gemini-3.8-flash-high");
    m.insert("gemini-1.5-pro", "gemini-pro-agent");
    m.insert("gemini-2.0-pro", "gemini-pro-agent");
    m.insert("gemini-2.5-pro", "gemini-pro-agent");
    m.insert("gemini-1.5-flash", "gemini-3.8-flash-high");
    m.insert("gemini-2.0-flash", "gemini-3.8-flash-high");
    m.insert("gemini-2.5-flash", "gemini-3.6-flash-medium");
    m.insert("gemini-2.5-flash-thinking", "gemini-3.6-flash-medium");
    m.insert("gemini-2.5-flash-lite", "gemini-3.1-flash-lite");
    m.insert("gemini-3.5-flash-lite", "gemini-3.1-flash-lite");

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

    out
}

/// Claude 主版本低于当前服务端基准 4.6 时，按家族收到 4-6。
fn legacy_claude_family_target(id: &str) -> Option<&'static str> {
    if !id.contains("claude") {
        return None;
    }
    let mut version: Option<f32> = None;
    let tokens: Vec<&str> = id.split('-').collect();
    for window in tokens.windows(2) {
        if let (Ok(major), Ok(minor)) = (window[0].parse::<u32>(), window[1].parse::<u32>()) {
            if minor < 100 {
                version = Some(major as f32 + (minor as f32) / 10.0);
            }
        }
    }
    let below_baseline = version.is_some_and(|ver| ver < 4.6);
    if !below_baseline {
        return None;
    }
    let thinking = id.contains("thinking");
    if id.contains("opus") {
        return Some("claude-opus-4-6-thinking");
    }
    if id.contains("sonnet") || id.contains("haiku") {
        return Some(if thinking {
            "claude-sonnet-4-6-thinking"
        } else {
            "claude-sonnet-4-6"
        });
    }
    None
}

/// Map Claude model names to Gemini model names
///
/// # 映射策略
/// 1. **精确匹配**: 检查 CLAUDE_TO_GEMINI 映射表
/// 2. **已知前缀透传**: gemini-* 和 *-thinking 模型直接透传
/// 3. **[NEW] 直接透传**: 未知模型 ID 直接传递给 Google API (支持体验未发布模型)
pub fn map_claude_model_to_gemini(input: &str) -> String {
    let canonical = canonicalize_claude_client_model_id(input);

    // 1. 精确匹配标准映射表
    if let Some(mapped) = CLAUDE_TO_GEMINI.get(canonical.as_str()) {
        return mapped.to_string();
    }

    // 2. 兼容历史老版本 ID 重定向 (不泄漏到外部列表)
    match canonical.as_str() {
        "claude-3-5-sonnet-20241022" | "claude-3-5-sonnet-20240620" | "claude-3-haiku-20240307" => {
            return "claude-sonnet-4-6".to_string()
        }
        "claude-opus-4" | "claude-opus-4-5-20251101" | "claude-opus-4-6-20260201" => {
            return "claude-opus-4-6-thinking".to_string()
        }
        "gemini-1.5-pro"
        | "gemini-2.0-pro"
        | "gemini-2.5-pro"
        | "gemini-3-pro"
        | "gemini-3-pro-preview"
        | "gemini-3.1-pro-preview"
        | "gemini-3.1-pro"
        | "gemini-3-pro-high"
        | "gemini-3.1-pro-high" => return "gemini-pro-agent".to_string(),
        "gemini-3-pro-low" => return "gemini-3.1-pro-low".to_string(),
        "gemini-1.5-flash" | "gemini-2.0-flash" | "gemini-3-flash" => {
            return "gemini-3.8-flash-high".to_string()
        }
        "gemini-2.5-flash" | "gemini-2.5-flash-thinking" => {
            return "gemini-3.6-flash-medium".to_string()
        }
        "gemini-2.5-flash-lite" | "gemini-3.5-flash-lite" => {
            return "gemini-3.1-flash-lite".to_string()
        }
        "internal-background-task" => return "gemini-3.1-flash-lite".to_string(),
        _ => {}
    }

    if let Some(target) = legacy_claude_family_target(&canonical) {
        return target.to_string();
    }

    // 3. Known prefixes (gemini-, -thinking) pass-through
    if canonical.starts_with("gemini-") || canonical.contains("thinking") {
        return canonical;
    }

    // 4. 直接透传未知模型 ID
    if canonical.contains("claude") {
        return canonical;
    }
    input.to_string()
}

/// 将任意请求模型名/虚拟路由别名解析为最终被转发和执行的真实上游模型名 (Real Upstream Target Model)
pub fn resolve_real_forwarded_model(input: &str) -> String {
    let raw = input.trim();
    let lower = raw.to_lowercase();

    // 1. 如果包含动态转发规则，先解析
    if let Some(target) = DYNAMIC_MODEL_FORWARDING_RULES.get(raw) {
        return target.clone();
    }
    if let Some(target) = CLAUDE_TO_GEMINI.get(raw) {
        let mapped = target.to_string();
        if mapped != raw {
            return resolve_real_forwarded_model(&mapped);
        }
    }

    // 2. 虚拟的 "claude-*-flash-*" 或带有 flash 的模型 (实为 Google Gemini Flash 转发)
    if lower.contains("flash") {
        if lower.contains("image") {
            return "gemini-3.1-flash-image".to_string();
        }
        if lower.contains("3.8") || lower.contains("3-8") {
            if lower.contains("high") { return "gemini-3.8-flash-high".to_string(); }
            if lower.contains("low") { return "gemini-3.8-flash-low".to_string(); }
            if lower.contains("tiered") { return "gemini-3.8-flash-tiered".to_string(); }
            return "gemini-3.8-flash-medium".to_string();
        }
        if lower.contains("3.7") || lower.contains("3-7") {
            if lower.contains("high") { return "gemini-3.7-flash-high".to_string(); }
            if lower.contains("medium") { return "gemini-3.7-flash-medium".to_string(); }
            if lower.contains("low") { return "gemini-3.7-flash-low".to_string(); }
            if lower.contains("tiered") { return "gemini-3.7-flash-tiered".to_string(); }
            return "gemini-3.7-flash-high".to_string();
        }
        if lower.contains("3.6") || lower.contains("3-6") {
            if lower.contains("high") { return "gemini-3.6-flash-high".to_string(); }
            if lower.contains("low") { return "gemini-3.6-flash-low".to_string(); }
            if lower.contains("tiered") { return "gemini-3.6-flash-tiered".to_string(); }
            return "gemini-3.6-flash-medium".to_string();
        }
        if lower.contains("3.5") || lower.contains("3-5") {
            if lower.contains("extra-low") { return "gemini-3.5-flash-extra-low".to_string(); }
            return "gemini-3.5-flash-low".to_string();
        }
        if lower.contains("2.5") || lower.contains("2-5") {
            return "gemini-2.5-flash".to_string();
        }
        if lower.contains("2.0") || lower.contains("2-0") {
            return "gemini-2.0-flash".to_string();
        }
        return "gemini-3-flash".to_string();
    }

    // 3. Pro 系列处理 (包含代理内部的 gemini-3.1-pro-high, claude-3.1-pro-high 等)
    if lower.contains("pro") && (lower.contains("gemini") || lower.contains("claude") || lower.contains("gpt") || lower.starts_with("g-") || lower.starts_with("c-")) {
        if lower.contains("image") {
            return "gemini-3-pro-image".to_string();
        }
        if lower.contains("high") {
            return "gemini-pro-agent".to_string();
        }
        if lower.contains("low") {
            return "gemini-3.1-pro-low".to_string();
        }
        if lower.contains("3.1") || lower.contains("3-1") {
            return "gemini-pro-agent".to_string();
        }
        if lower.contains("2.5") || lower.contains("2-5") {
            return "gemini-2.5-pro".to_string();
        }
        return "gemini-pro-agent".to_string();
    }

    // 4. Claude 官方真实模型系列
    if lower.contains("sonnet") {
        if lower.contains("4-6") || lower.contains("4.6") {
            return "claude-sonnet-4-6".to_string();
        }
        if lower.contains("3-7") || lower.contains("3.7") {
            return "claude-sonnet-4-6-thinking".to_string();
        }
        if lower.contains("3-5") || lower.contains("3.5") || lower.contains("4-5") || lower.contains("4.5") {
            return "claude-sonnet-4-6".to_string();
        }
        return "claude-sonnet-4-6".to_string();
    }

    if lower.contains("opus") {
        if lower.contains("4-6") || lower.contains("4.6") {
            return "claude-opus-4-6-thinking".to_string();
        }
        return "claude-opus-4-6-thinking".to_string();
    }

    if lower.contains("haiku") {
        return "claude-sonnet-4-6".to_string();
    }

    // 5. GPT 伪模型 / 兼容别名
    if lower.starts_with("gpt-") {
        if lower.contains("4o-mini") {
            return "gemini-2.5-flash".to_string();
        }
        if lower.contains("4o") || lower.contains("gpt-4") || lower.contains("gpt-5") {
            return "gemini-2.5-flash".to_string();
        }
        return "gemini-2.5-flash".to_string();
    }

    // 6. 清理内部后缀 (如 -high, -low, -tiered, -extra-low, -agent)
    let cleaned = raw
        .trim_end_matches("-high")
        .trim_end_matches("-low")
        .trim_end_matches("-medium")
        .trim_end_matches("-tiered")
        .trim_end_matches("-extra-low")
        .trim_end_matches("-agent");

    if cleaned != raw && !cleaned.is_empty() {
        return cleaned.to_string();
    }

    raw.to_string()
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
        } else {
            let m = minor_token.parse::<u32>().ok()?;
            if m >= 1000 {
                0
            } else {
                m
            }
        }
    } else {
        0
    };
    Some((major, minor))
}

/// 核心基准线过滤器：严格遵循官方客户端当前展示的模型基准线
/// 1. Gemini Flash 系列：基准线 3.5。版本 >= 3.5 保留；< 3.5 的除了 2.5 经典系列外全部淘汰。
/// 2. Claude 系列：基准线 4.6。版本 >= 4.6 保留；4.6 以下全部淘汰。
/// 3. GPT 系列：以官方最新公布为准 (gpt-oss-120b-medium)，淘汰 gpt-4o 等历史旧模型。
/// 4. Pro 系列：以官方最新为准 (gemini-3.1-pro-high, gemini-3.1-pro-low, gemini-2.5-pro, gemini-3-pro-image)。
pub fn is_model_compliant_with_baseline(model: &str) -> bool {
    let lower = model.to_lowercase();
    let m = lower.trim();

    // 过滤内部任务与测试模型
    if m.starts_with("chat_") || m.contains("internal") || m.contains("-exp") {
        return false;
    }

    // 特许放行的官方白名单模型（包含官方 agent 与 tab 预览模型）
    if m == "gemini-pro-agent"
        || m == "gemini-3-flash-agent"
        || m == "gemini-3-flash"
        || m == "tab_flash_lite_preview"
        || m == "tab_jump_flash_lite_preview"
        || m == "gemini-3.1-flash-lite"
        || m == "gemini-3.5-flash-lite"
    {
        return true;
    }

    // 1. Claude 系列：以 4.6 为基准线，4.6 以下全部淘汰
    if m.contains("claude") {
        if m.contains("4-6") || m.contains("4.6") {
            return true;
        }
        // 兼容未来可能发布的 >= 4.6 版本 (如 4.7+, 5.x)
        if let Some(pos) = m.find("claude") {
            let rest = &m[pos..];
            let tokens: Vec<&str> = rest.split(|c: char| c == '-' || c == '_').collect();
            let mut i = 0;
            let mut detected_version: Option<(u32, u32)> = None;
            while i < tokens.len() {
                let token = tokens[i];
                // 忽略纯日期快照 (如 20241022, 20250219)
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
                    let ver = (major, minor);
                    detected_version = Some(detected_version.map_or(ver, |v| v.max(ver)));
                } else if let Some(ver) = parse_version_tuple(token) {
                    detected_version = Some(detected_version.map_or(ver, |v| v.max(ver)));
                }
                i += 1;
            }

            if let Some(ver) = detected_version {
                return ver >= (4, 6);
            }
        }
        return false;
    }

    // 2. GPT / OpenAI 系列：以官方为准 (gpt-oss-120b-medium)
    if m.contains("gpt") || m.contains("o1") || m.contains("o3") {
        return m == "gpt-oss-120b-medium";
    }

    // 3. 图像模型保底
    if m.contains("image") {
        return m == "gemini-3.1-flash-image" || m == "gemini-3-pro-image";
    }

    // 4. Gemini Pro 系列：以官方最新为准 (3.1 Pro)，淘汰 2.5 Pro 及以下历史旧模型
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
        // 未知更高版本 pro (如 3.10, 4.x)
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

    // 5. Gemini Flash 系列：淘汰 2.5 全系列及已 503 的 3.5-flash-lite；放行 3.1-flash-lite 与 >= 3.5 版本
    if m.contains("flash") {
        // 2.5 全系列已退役淘汰
        if m.contains("2.5") || m.contains("2-5") {
            return false;
        }

        // 3.1-flash-lite 与 3.5-flash-lite 特别放行
        if m == "gemini-3.1-flash-lite" || m == "gemini-3.5-flash-lite" {
            return true;
        }

        // 解析版本号 (使用语义元组比较，正确支持 3.10 > 3.5 等双位数次版本)
        if let Some(pos) = m.find("gemini-") {
            let rest = &m[pos + 7..];
            if let Some(flash_pos) = rest.find("-flash") {
                let ver_str = &rest[..flash_pos];
                if let Some(ver) = parse_version_tuple(ver_str) {
                    return ver >= (3, 5);
                }
            }
        }

        // 容错匹配特定已知 >= 3.5 版本
        if m.contains("3.8")
            || m.contains("3-8")
            || m.contains("3.7")
            || m.contains("3-7")
            || m.contains("3.6")
            || m.contains("3-6")
            || m.contains("3.5")
            || m.contains("3-5")
        {
            return true;
        }

        return false;
    }

    // 其他未知非 Google 系列（如用户自定义映射），予以放行
    true
}

/// 获取所有内置支持的标准公开模型列表 (已清理过期实验模型、重复笛卡尔积及旧快照)
pub fn get_supported_models() -> Vec<String> {
    vec![
        // Gemini 3.8 系列
        "gemini-3.8-flash",
        "gemini-3.8-flash-high",
        "gemini-3.8-flash-medium",
        "gemini-3.8-flash-low",
        "gemini-3.8-flash-tiered",
        // Gemini 3.7 系列
        "gemini-3.7-flash",
        "gemini-3.7-flash-high",
        "gemini-3.7-flash-medium",
        "gemini-3.7-flash-low",
        "gemini-3.7-flash-tiered",
        // Gemini 3.6 系列
        "gemini-3.6-flash",
        "gemini-3.6-flash-high",
        "gemini-3.6-flash-medium",
        "gemini-3.6-flash-low",
        "gemini-3.6-flash-tiered",
        // Gemini 3.5 系列
        "gemini-3.5-flash",
        "gemini-3.5-flash-low",
        "gemini-3.5-flash-extra-low",
        // Gemini 3 系列
        "gemini-3-flash",
        "gemini-3-flash-agent",
        // Gemini 3.1 Pro 系列
        "gemini-3.1-pro",
        "gemini-3.1-pro-high",
        "gemini-3.1-pro-low",
        // Gemini 3.1 Flash Lite 系列 (轻量快速 1M 上下文模型)
        "gemini-3.1-flash-lite",
        // Gemini 图像生成主力模型
        "gemini-3.1-flash-image",
        "gemini-3-pro-image",
        // Claude 系列 (基准线 >= 4.6)
        "claude-sonnet-4-6",
        "claude-sonnet-4-6-thinking",
        "claude-opus-4-6",
        "claude-opus-4-6-thinking",
        // 官方 Agent 与预览模型
        "gemini-pro-agent",
        "tab_flash_lite_preview",
        "tab_jump_flash_lite_preview",
        // OpenAI 系列 (以官方为准)
        "gpt-oss-120b-medium",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

/// 动态获取所有可用模型列表 (包含内置与用户自定义与官方端点动态下发)
pub async fn get_all_dynamic_models(
    custom_mapping: &tokio::sync::RwLock<std::collections::HashMap<String, String>>,
    token_manager: Option<&crate::proxy::token_manager::TokenManager>,
    only_raw_quota_models: bool,
) -> Vec<String> {
    use std::collections::HashSet;
    let mut model_ids = HashSet::new();
    let mut custom_keys = HashSet::new();

    // 1. 获取所有账号从官方接口汇聚而来的动态模型 (Quota Models)
    if let Some(tm) = token_manager {
        for dynamic_model in tm.get_all_collected_models() {
            model_ids.insert(dynamic_model);
        }
    }

    // 1.5 动态档位后缀剥离与裸模型派生（纯通用数据驱动）：
    // 若上游模型包含分档后缀（如 -high / -medium / -low / -extra-low 等），
    // 自动剥离后缀并衍生对应的裸模型名，只要符合官方基准线，任何品牌家族均可自动派生。
    let mut derived_bare_models = HashSet::new();
    for id in &model_ids {
        for suffix in &["-tiered", "-high", "-medium", "-low", "-extra-low"] {
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

    // 如果未开启 only_raw_quota_models，则追加 custom_mapping 与内置标准公开模型
    if !only_raw_quota_models {
        // 2. 获取所有自定义映射模型 (Custom)
        {
            let mapping = custom_mapping.read().await;
            for key in mapping.keys() {
                custom_keys.insert(key.clone());
                model_ids.insert(key.clone());
            }
        }

        // 3. 获取所有内置标准模型
        for m in get_supported_models() {
            model_ids.insert(m);
        }
    }

    // 4. 应用官方基准线过滤，彻底剔除已淘汰的旧版模型与内部虚拟 ID
    // 自定义别名映射（custom_mapping）由用户显式指定，不受官方基准线过滤误杀
    let mut sorted_ids: Vec<_> = model_ids
        .into_iter()
        .filter(|id| custom_keys.contains(id) || is_model_compliant_with_baseline(id))
        .collect();
    sorted_ids.sort();
    sorted_ids
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

/// 解析完整的模型路由（支持官方淘汰重定向、用户自定义映射、通配符映射、物理 ID 透传以及依据客户端思考档位进行通用裸模型解析）
///
/// # 参数
/// * `original_model` - 原始模型名称
/// * `custom_mapping` - 用户自定义映射表
/// * `client_effort` - 客户端指定的思考强度/档位（如 Some("low"), Some("high") 等）
///
/// # 返回
/// 映射后的目标模型名称
pub fn resolve_model_route_with_effort(
    original_model: &str,
    custom_mapping: &std::collections::HashMap<String, String>,
    client_effort: Option<&str>,
) -> String {
    // 0 & 1 & 2. 优先匹配淘汰重定向与自定义映射（精确、大小写、规范化、通配符）
    if let Some(custom) = resolve_custom_model_route(original_model, custom_mapping) {
        return custom;
    }

    // 3. 系统默认映射
    // Variant 已经写出的上游真实 id 在此停住，避免公开名反向覆盖（Issue #3551）。
    if crate::proxy::common::variant_mapping::is_physical_upstream_id(original_model) {
        return original_model.to_string();
    }

    // [NEW] 裸模型依据客户端思考档位通用路由（覆盖 3.x Flash 与 Claude >= 5.0）：
    // - 对于 Flash：默认 high，支持 low / medium
    // - 对于 Claude：默认 medium，支持 low / high
    // 而显式指定了档位后缀的模型已被前置规则/CLAUDE_TO_GEMINI 原样保留
    if let Some(routed) =
        crate::proxy::model_specs::resolve_bare_tiered_model_route(original_model, client_effort)
    {
        crate::modules::logger::log_info(&format!(
            "[Router] 裸模型依据思考档位通用路由: {} (effort={:?}) -> {}",
            original_model, client_effort, routed
        ));
        return routed;
    }

    let result = map_claude_model_to_gemini(original_model);
    if result != original_model {
        crate::modules::logger::log_info(&format!(
            "[Router] 系统默认映射: {} -> {}",
            original_model, result
        ));
    }
    result
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

pub fn normalize_to_standard_id(model_name: &str) -> Option<String> {
    let lower = model_name.to_lowercase();

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
            map_claude_model_to_gemini("claude-3-5-sonnet-20241022"),
            "claude-sonnet-4-6"
        );
        // [Redirect] Sonnet 4.5 -> Sonnet 4.6
        assert_eq!(
            map_claude_model_to_gemini("claude-sonnet-4-5"),
            "claude-sonnet-4-6"
        );
        assert_eq!(
            map_claude_model_to_gemini("claude-sonnet-4-5-thinking"),
            "claude-sonnet-4-6-thinking"
        );
        assert_eq!(
            map_claude_model_to_gemini("claude-opus-4"),
            "claude-opus-4-6-thinking"
        );
        assert_eq!(
            map_claude_model_to_gemini("claude-opus-4.6"),
            "claude-opus-4-6-thinking"
        );
        assert_eq!(
            map_claude_model_to_gemini("claude-open-4.5"),
            "claude-opus-4-6-thinking"
        );
        assert_eq!(
            map_claude_model_to_gemini("anthropic/claude-sonnet-4.6"),
            "claude-sonnet-4-6"
        );
        assert_eq!(
            map_claude_model_to_gemini("claude-sonnet-4.6-thinking"),
            "claude-sonnet-4-6-thinking"
        );
        assert_eq!(
            map_claude_model_to_gemini("claude-haiku-4.5"),
            "claude-sonnet-4-6"
        );
        assert_eq!(
            map_claude_model_to_gemini("claude-opus-4-6-thinking"),
            "claude-opus-4-6-thinking"
        );
        // Test gemini pass-through (should not be caught by "mini" rule)
        assert_eq!(
            map_claude_model_to_gemini("gemini-2.5-flash-mini-test"),
            "gemini-2.5-flash-mini-test"
        );
        assert_eq!(map_claude_model_to_gemini("unknown-model"), "unknown-model");
        // 旧 Pro 公开名必须落到上游可生成的真实 id。
        assert_eq!(
            map_claude_model_to_gemini("gemini-3-pro-high"),
            "gemini-pro-agent"
        );
        assert_eq!(
            map_claude_model_to_gemini("gemini-3-pro-low"),
            "gemini-3.1-pro-low"
        );
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
            "gemini-pro-agent"
        );
        assert_eq!(
            map_claude_model_to_gemini("gemini-3.1-pro-low"),
            "gemini-3.1-pro-low"
        );
        // 裸 Pro / 旧别名默认落到 high 的真实上游 id
        assert_eq!(
            map_claude_model_to_gemini("gemini-3-pro"),
            "gemini-pro-agent"
        );
        assert_eq!(
            map_claude_model_to_gemini("gemini-3.1-pro"),
            "gemini-pro-agent"
        );
        // 真实上游 id 不得被系统默认映射映回公开名（Issue #3551）
        assert_eq!(
            map_claude_model_to_gemini("gemini-pro-agent"),
            "gemini-pro-agent"
        );
        assert_eq!(
            map_claude_model_to_gemini("gemini-3-flash-agent"),
            "gemini-3-flash-agent"
        );
        let empty = HashMap::new();
        assert_eq!(
            resolve_model_route("gemini-3.1-pro-high", &empty),
            "gemini-3.1-pro-high"
        );
        assert_eq!(
            resolve_model_route("gemini-pro-agent", &empty),
            "gemini-pro-agent"
        );
        assert_eq!(
            resolve_model_route("gemini-3-flash-agent", &empty),
            "gemini-3-flash-agent"
        );

        // 淘汰旧模型平滑重定向至健康新模型
        assert_eq!(
            map_claude_model_to_gemini("gemini-2.5-flash"),
            "gemini-3.6-flash-medium"
        );
        assert_eq!(
            map_claude_model_to_gemini("gemini-2.5-flash-thinking"),
            "gemini-3.6-flash-medium"
        );
        assert_eq!(
            map_claude_model_to_gemini("gemini-2.5-flash-lite"),
            "gemini-3.1-flash-lite"
        );
        assert_eq!(
            map_claude_model_to_gemini("gemini-3.5-flash-lite"),
            "gemini-3.1-flash-lite"
        );
        assert_eq!(
            map_claude_model_to_gemini("gemini-2.5-pro"),
            "gemini-pro-agent"
        );
        assert_eq!(
            map_claude_model_to_gemini("gemini-3.1-flash-lite"),
            "gemini-3.1-flash-lite"
        );
        assert_eq!(
            map_claude_model_to_gemini("internal-background-task"),
            "gemini-3.1-flash-lite"
        );
        assert_eq!(
            resolve_model_route("gemini-2.5-flash", &empty),
            "gemini-3.6-flash-medium"
        );
        assert_eq!(
            resolve_model_route("gemini-2.5-flash-lite", &empty),
            "gemini-3.1-flash-lite"
        );
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
        // Claude: >= 4.6 passes, < 4.6 rejected
        assert!(is_model_compliant_with_baseline("claude-sonnet-4-6"));
        assert!(is_model_compliant_with_baseline("claude-opus-4-6-thinking"));
        assert!(is_model_compliant_with_baseline("claude-sonnet-4-7"));
        assert!(!is_model_compliant_with_baseline("claude-sonnet-4-5"));
        assert!(!is_model_compliant_with_baseline("claude-3-5-sonnet"));
        assert!(!is_model_compliant_with_baseline("claude-3-7-sonnet"));
        assert!(!is_model_compliant_with_baseline("claude-haiku-4-5"));

        // GPT: only gpt-oss-120b-medium passes
        assert!(is_model_compliant_with_baseline("gpt-oss-120b-medium"));
        assert!(!is_model_compliant_with_baseline("gpt-4o"));
        assert!(!is_model_compliant_with_baseline("gpt-4"));
        assert!(!is_model_compliant_with_baseline("gpt-3.5-turbo"));

        // Gemini Flash: >= 3.5 passes, 3.1-flash-lite & 3.5-flash-lite pass, 2.5/1.5/2.0 rejected
        assert!(is_model_compliant_with_baseline("gemini-3.8-flash-high"));
        assert!(is_model_compliant_with_baseline("gemini-3.7-flash-medium"));
        assert!(is_model_compliant_with_baseline("gemini-3.6-flash-low"));
        assert!(is_model_compliant_with_baseline("gemini-3.5-flash-low"));
        assert!(is_model_compliant_with_baseline("gemini-3.10-flash-high"));
        assert!(is_model_compliant_with_baseline("gemini-3.1-flash-lite"));
        assert!(is_model_compliant_with_baseline("gemini-3.5-flash-lite"));
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
        assert!(!is_model_compliant_with_baseline("gemini-2.5-pro"));
        assert!(!is_model_compliant_with_baseline("gemini-1.5-pro"));
        assert!(!is_model_compliant_with_baseline("gemini-2.0-pro"));
        assert!(!is_model_compliant_with_baseline("gemini-3-pro"));
        assert!(!is_model_compliant_with_baseline("gemini-3.1-pro-preview"));

        // Image models
        assert!(is_model_compliant_with_baseline("gemini-3.1-flash-image"));
        assert!(is_model_compliant_with_baseline("gemini-3-pro-image"));

        // Internal / Deprecated models
        assert!(!is_model_compliant_with_baseline("chat_20706"));
        assert!(!is_model_compliant_with_baseline("chat_23310"));
        assert!(!is_model_compliant_with_baseline("gemini-2.5-flash"));
        assert!(!is_model_compliant_with_baseline("gemini-2.5-pro"));

        // Official Agent & Preview & Derived Bare models (Kept)
        assert!(is_model_compliant_with_baseline(
            "tab_jump_flash_lite_preview"
        ));
        assert!(is_model_compliant_with_baseline("tab_flash_lite_preview"));
        assert!(is_model_compliant_with_baseline("gemini-pro-agent"));
        assert!(is_model_compliant_with_baseline("gemini-3-flash-agent"));
        assert!(is_model_compliant_with_baseline("gemini-3-flash"));
        assert!(is_model_compliant_with_baseline("gemini-3.1-pro"));
        assert!(is_model_compliant_with_baseline("gemini-3.5-flash"));
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

        // 2. 衍生裸模型 gemini-3.1-pro 缺省 effort 遵循 pick_optimal_default_tier 决策链 (取高于 low 的最低档 -> high)
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.1-pro", &empty, None),
            "gemini-3.1-pro-high"
        );

        // 3. 衍生裸模型 gemini-3.5-flash 缺省 effort 遵循决策链 (取 low)
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.5-flash", &empty, None),
            "gemini-3.5-flash-low"
        );

        // 4. 显式档位变体原样透传
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.1-pro-low", &empty, None),
            "gemini-3.1-pro-low"
        );
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.1-pro-high", &empty, None),
            "gemini-3.1-pro-high"
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
}
