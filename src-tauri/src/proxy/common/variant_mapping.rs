//! Canonical model + variant → real model ID + real params.
//!
//! OpenCode 配置 canonical 模型（如 `gemini-3.5-flash`）带 variants（low/medium/high）。
//! 收到请求后，AM 根据 canonical + 档位映射到目标模型 ID
//! （如 `gemini-3-flash-agent`），并用已校准的 thinkingBudget / maxOutputTokens
//! 替换客户端传来的值，保证中转请求参数与上游服务一致。
//!
//! 所有数值为已校准的目标模型参数。

/// Variant tier inferred from the client's `thinking.budget_tokens`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariantTier {
    Low,
    Medium,
    High,
}

/// How an alias selects a variant tier within its canonical Gemini family.
#[derive(Debug, Clone, Copy)]
pub enum AliasPolicy {
    HonorTier,
    Fixed(VariantTier),
}

/// Static metadata and tier routing for one canonical Gemini model family.
pub struct CanonicalFamily {
    pub canonical_id: &'static str,
    pub display_name: &'static str,
    pub context_limit: u32,
    pub output_limit: u32,
    pub input_modalities: &'static [&'static str],
    pub output_modalities: &'static [&'static str],
    pub reasoning: bool,
    pub tiers: &'static [(VariantTier, RealModelSpec)],
    pub aliases: &'static [(&'static str, AliasPolicy)],
}

/// A resolved real model with its verified request params.
#[derive(Debug, Clone, Copy)]
pub struct RealModelSpec {
    /// The real model ID to put in the upstream `model` field.
    pub id: &'static str,
    /// verified thinkingBudget (0 means no thinking).
    pub thinking_budget: u32,
    /// verified maxOutputTokens.
    pub max_output_tokens: u32,
    /// Whether to include thoughts in the response.
    pub include_thoughts: bool,
    /// Whether a Claude client-selected thinking budget may pass through.
    pub preserve_client_budget: bool,
}

impl RealModelSpec {
    /// Select the upstream thinking budget for this resolved model.
    pub const fn effective_thinking_budget(&self, client_budget: Option<u32>) -> u32 {
        if self.preserve_client_budget {
            match client_budget {
                Some(budget) => budget,
                None => self.thinking_budget,
            }
        } else {
            self.thinking_budget
        }
    }

    /// 该模型是否支持思考（以官方模型结构体为准）
    ///
    /// `thinking_budget == 0` 只表示“不写数字预算”（等级通道 / 自适应 / 官方关闭），
    /// **不能**据此判定“模型不支持思考”。历史上适配器把两者混为一谈，
    /// 导致 thinkingLevel 通道模型（3.8-flash-high / 4-argon / 5.5 / flash-lite）
    /// 被误当作非思考模型而清空工具集。
    pub fn supports_thinking(&self) -> bool {
        match crate::models::OfficialModelCatalog::get(self.id) {
            Some(info) => info.supports_thinking.unwrap_or(self.include_thoughts),
            None => self.include_thoughts,
        }
    }

    /// 需要写入下游请求的数字预算；0（不写预算）与自适应返回 None。
    pub fn budget_tokens_option(&self, client_budget: Option<u32>) -> Option<u32> {
        match self.effective_thinking_budget(client_budget) {
            0 => None,
            budget => Some(budget),
        }
    }
}

/// 归一化动态模型 ID 为 `&'static str`（进程内去重）
///
/// `RealModelSpec.id` 是 `&'static str`；历史上每次解析都 `Box::leak` 一份，
/// 导致每请求泄漏一段模型名。这里按模型 ID 去重：同一 ID 只泄漏一次，
/// 泄漏量被限幅在「不同模型 ID 的数量」以内。
fn intern_model_id(id: String) -> &'static str {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    static INTERNED: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let map = INTERNED.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = match map.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(existing) = guard.get(&id) {
        return *existing;
    }
    let leaked: &'static str = Box::leak(id.clone().into_boxed_str());
    guard.insert(id, leaked);
    leaked
}

/// 用官方模型结构体覆盖静态 SPEC 中的预算 / 输出上限 / 思考支持。
///
/// 静态 SPEC 只保留 id、别名与 `preserve_client_budget`；数值参数一律以官方目录为准，
/// 避免同一模型在不同代码路径上出现两套权威值。
fn apply_official_params(mut spec: RealModelSpec) -> RealModelSpec {
    let Some(info) = crate::models::OfficialModelCatalog::get(spec.id) else {
        return spec;
    };

    if let Some(budget) = info.thinking_budget {
        // > 0：预算通道；-1：自适应（不写数字）；0：官方关闭
        spec.thinking_budget = if budget > 0 { budget as u32 } else { 0 };
    } else if info.thinking_level.is_some() {
        // 等级通道：没有数字预算
        spec.thinking_budget = 0;
    }

    if let Some(max_out) = info.max_output_tokens.filter(|m| *m > 0) {
        spec.max_output_tokens = max_out as u32;
    }

    if let Some(supports) = info.supports_thinking {
        spec.include_thoughts = supports;
    }

    spec
}

/// Infer the variant tier from the client-sent `thinking.budget_tokens`.
///
/// OpenCode derives budgetTokens from its model capabilities (max = budget-1,
/// high = floor(budget/2)), so the exact value is not the real budget —
/// we only use its magnitude to guess which tier the user selected. When no budget
/// is present we default to High (the most capable tier).
pub fn infer_tier(budget_tokens: Option<u32>) -> VariantTier {
    match budget_tokens {
        // 1024 是 Anthropic Claude SDK 规范的最小默认值，绝不能误判为 Low 导致思考截断！
        Some(1024) => VariantTier::High,
        Some(b) if b < 2000 => VariantTier::Low,
        Some(b) if b < 7000 => VariantTier::Medium,
        _ => VariantTier::High,
    }
}

/// Parse a client effort into a variant tier.
///
/// 词表统一由 [`crate::proxy::model_specs::normalize_client_thinking_level`] 提供，
/// `default` 表示“未显式指定档位”→ None（交给统一默认决策链），
/// 不再把它当作 Medium；`min/standard/extreme` 等别名也随之可用。
pub fn tier_from_effort(effort: Option<&str>) -> Option<VariantTier> {
    match crate::proxy::model_specs::normalize_client_thinking_level(effort?)? {
        "LOW" => Some(VariantTier::Low),
        "MEDIUM" => Some(VariantTier::Medium),
        "HIGH" => Some(VariantTier::High),
        _ => None,
    }
}

/// Resolve a canonical model + tier to the real model + params.
///
/// Returns None for models that have no variant split (use
/// [`resolve_non_variant_model`] for those).
pub fn resolve_real_model(canonical: &str, tier: VariantTier) -> Option<RealModelSpec> {
    // Normalize: lowercase, trim a trailing variant-like suffix the client may have appended.
    let key = canonical.to_lowercase();
    // 强制防御：凡是以 -high / -medium / -low 结尾的模型名，档位具有绝对优先级，绝不接受任何降级
    let forced_tier = if key.ends_with("-high") {
        Some(VariantTier::High)
    } else if key.ends_with("-medium") {
        Some(VariantTier::Medium)
    } else if key.ends_with("-low") || key.ends_with("-extra-low") {
        Some(VariantTier::Low)
    } else {
        None
    };

    for family in GEMINI_FAMILIES {
        let is_canonical = family.canonical_id == key.as_str();
        let alias_match = family
            .aliases
            .iter()
            .find(|(alias, _)| *alias == key.as_str());

        if !is_canonical && alias_match.is_none() {
            continue;
        }

        let resolved_tier = if let Some(ft) = forced_tier {
            ft
        } else if is_canonical {
            tier
        } else if let Some((_, policy)) = alias_match {
            match *policy {
                AliasPolicy::HonorTier => tier,
                AliasPolicy::Fixed(fixed_tier) => fixed_tier,
            }
        } else {
            continue;
        };

        return family
            .tiers
            .iter()
            .find(|(candidate_tier, _)| *candidate_tier == resolved_tier)
            .map(|(_, spec)| apply_official_params(*spec));
    }

    None
}

/// Resolve a canonical model that has NO variant split — use real ID + params directly.
///
/// Covers models the user may configure without variants (flash-lite, claude, etc.).
/// Also accepts the real IDs themselves (idempotent passthrough).
pub fn resolve_non_variant_model(model: &str) -> Option<RealModelSpec> {
    let key = model.to_lowercase();
    // gemini-3.1-flash-lite: checkpoint-only model, no thinking.
    // [RETIRED] 旧版 Gemini 2.x（gemini-2.5-flash / 2.5-flash-lite / 2.5-flash-thinking）已全面下线，
    // 不再保留任何内部别名映射，避免它们在门禁前被“升级”成合法模型而绕过校验。
    if key == "gemini-3.1-flash-lite" {
        return Some(apply_official_params(SPEC_31_FLASH_LITE));
    }
    if key == "claude-sonnet-4-6" {
        return Some(apply_official_params(SPEC_CLAUDE_SONNET_46));
    }
    if matches!(key.as_str(), "claude-opus-4-6-thinking" | "claude-opus-4-6") {
        return Some(apply_official_params(SPEC_CLAUDE_OPUS_46));
    }
    if key == "gpt-oss-120b-medium" {
        return Some(apply_official_params(SPEC_GPT_OSS_120B));
    }
    None
}

/// Resolve a model with an optional explicit variant tier.
///
/// An explicit tier takes precedence over the client thinking budget.
pub fn resolve_with_tier(
    canonical: &str,
    explicit_tier: Option<VariantTier>,
    budget_tokens: Option<u32>,
) -> Option<RealModelSpec> {
    // 1. 无变体划分的独立模型优先匹配（如 gpt-oss-120b-medium, claude-sonnet-4-6 等）
    if let Some(spec) = resolve_non_variant_model(canonical) {
        return Some(spec);
    }

    let lower = canonical.to_lowercase();
    let name_tier = if lower.ends_with("-high") {
        Some(VariantTier::High)
    } else if lower.ends_with("-medium") {
        Some(VariantTier::Medium)
    } else if lower.ends_with("-low") || lower.ends_with("-extra-low") {
        Some(VariantTier::Low)
    } else {
        None
    };

    let is_v3 = crate::proxy::model_specs::is_gemini_v3_or_above(canonical);

    // 显式模型名档位（-high, -medium, -low）具有绝对最高优先级，绝不接受任何 budget 降级；
    // 其次遵从显式传递的 effort 档位；
    // [USER RULE] 对于 Gemini >= 3 的裸模型，由客户端思考强度 (effort) 接管，思考预算字段 (budget_tokens) 直接忽略！
    // 客户端未传 effort 时绝不按 budget_tokens 推断档位；预算/等级一律以官方模型结构体为准；
    // 传统/非 v3 模型无显式档位时若有 budget 则推断档位，无 budget 则默认 Medium。
    let tier = if let Some(nt) = name_tier {
        nt
    } else if let Some(et) = explicit_tier {
        et
    } else if is_v3 {
        VariantTier::Medium
    } else if let Some(bt) = budget_tokens {
        infer_tier(Some(bt))
    } else {
        VariantTier::Medium
    };

    // 2. 尝试从已知家族注册表中匹配
    if let Some(spec) = resolve_real_model(canonical, tier) {
        return Some(spec);
    }

    // 3. 通用动态解析 (治本):
    // 针对任何 >= 3.0 的 Gemini 衍生模型（如 gemini-3.8-flash, gemini-3.9-flash, gemini-4.0-flash 等），
    // 即使未在 GEMINI_FAMILIES 中硬编码，也能自动根据档位生成完整规范的 RealModelSpec，
    // 确保思考模式无缝开启，预算自动校准，绝不降级。
    let is_gemini_3_family =
        is_v3 && (lower.contains("flash") || lower.contains("pro") || lower.contains("agent"));
    if is_gemini_3_family {
        let dynamic_tier = if let Some(nt) = name_tier {
            nt
        } else if let Some(et) = explicit_tier {
            et
        } else if lower.contains("high") || lower.contains("agent") || lower.contains("pro") {
            VariantTier::High
        } else if lower.contains("low") {
            VariantTier::Low
        } else if lower.contains("medium") {
            VariantTier::Medium
        } else {
            // [USER RULE] 裸模型未显式传档位时，默认按 high 档位路由
            VariantTier::High
        };

        // 3.x Flash 裸模型依据思考档位路由为 {base}-high / -low / -medium / -tiered，
        // 未显式指定档位时遵循统一决策链（tiered 优先 -> medium 次之 -> high 下的一个等级 -> 保底首个档位）
        let resolved_id = if crate::proxy::model_specs::is_bare_gemini_v3_flash(canonical) {
            let eff_str = if explicit_tier.is_some() || name_tier.is_some() {
                match dynamic_tier {
                    VariantTier::High => Some("high"),
                    VariantTier::Low => Some("low"),
                    VariantTier::Medium => Some("medium"),
                }
            } else {
                None
            };
            crate::proxy::model_specs::resolve_bare_flash_route(canonical, eff_str).unwrap_or_else(
                || {
                    let suffix = match dynamic_tier {
                        VariantTier::High => "high",
                        VariantTier::Low => "low",
                        VariantTier::Medium => "medium",
                    };
                    format!("{}-{}", canonical, suffix)
                },
            )
        } else {
            canonical.to_string()
        };

        // 档位参数一律由官方模型目录推导（`apply_official_params`）。
        // 拿不到官方结构体时保持中性默认：0 = 不伪造预算，65536 = 常见输出上限。
        let id: &'static str = intern_model_id(resolved_id);
        return Some(apply_official_params(RealModelSpec {
            id,
            thinking_budget: 0,
            max_output_tokens: 65536,
            include_thoughts: true,
            preserve_client_budget: false,
        }));
    }

    // 4. 纯通用分档模型动态规格解析 (Dynamic RealModelSpec Factory):
    // 针对任何具备档位后缀（-high, -medium, -low, -tiered）或可从官方目录解析为分档的任意品牌模型，
    // 动态生成 RealModelSpec，彻底告别静态 SPEC 常量维护：
    let has_available_tiers =
        !crate::models::OfficialModelCatalog::collect_tiers_for_base(&lower).is_empty();

    let is_tiered_model = name_tier.is_some() || explicit_tier.is_some() || has_available_tiers;
    if is_tiered_model {
        let dynamic_tier = if let Some(nt) = name_tier {
            nt
        } else if let Some(et) = explicit_tier {
            et
        } else {
            // Claude 默认 Medium，其余按阶梯默认
            if lower.contains("claude") {
                VariantTier::Medium
            } else {
                VariantTier::High
            }
        };

        // 如果是裸模型，动态解析出带档位后缀的真实 ID；未显式传档位时透传 None 走默认决策链
        let eff_str = if explicit_tier.is_some() || name_tier.is_some() {
            match dynamic_tier {
                VariantTier::High => Some("high"),
                VariantTier::Low => Some("low"),
                VariantTier::Medium => Some("medium"),
            }
        } else {
            None
        };

        let resolved_id =
            crate::proxy::model_specs::resolve_bare_tiered_model_route(canonical, eff_str)
                .unwrap_or_else(|| {
                    if lower.contains("claude") {
                        crate::proxy::common::model_mapping::canonicalize_claude_client_model_id(
                            canonical,
                        )
                    } else {
                        canonical.to_string()
                    }
                });

        let official_info = crate::models::OfficialModelCatalog::get(&resolved_id)
            .or_else(|| crate::models::OfficialModelCatalog::get(canonical));

        let max_output_tokens = official_info
            .as_ref()
            .and_then(|info| info.max_output_tokens)
            .map(|v| v as u32)
            .unwrap_or_else(|| {
                if lower.contains("claude") {
                    128_000
                } else if lower.contains("pro") {
                    65535
                } else {
                    65536
                }
            });

        let thinking_budget = official_info
            .as_ref()
            .and_then(|info| info.thinking_budget)
            .filter(|&v| v > 0)
            .map(|v| v as u32)
            .unwrap_or(0);

        let supports_thinking = official_info
            .as_ref()
            .and_then(|info| info.supports_thinking)
            .unwrap_or(true);

        let is_claude = lower.contains("claude");
        let id: &'static str = intern_model_id(resolved_id);
        return Some(apply_official_params(RealModelSpec {
            id,
            thinking_budget,
            max_output_tokens,
            include_thoughts: supports_thinking,
            preserve_client_budget: is_claude,
        }));
    }

    None
}

/// 纯 spec 解析：输入必须是 router 已经决定好的物理模型。
///
/// 不做任何档位默认 / 别名改写；若解析结果与输入 id 不一致，返回 None，
/// 交给流水线按官方目录处理。这样 variant 不再是第二路由器。
pub fn resolve_spec_exact(model: &str, client_budget: Option<u32>) -> Option<RealModelSpec> {
    for family in GEMINI_FAMILIES {
        for (_, spec) in family.tiers {
            if spec.id.eq_ignore_ascii_case(model) {
                return Some(apply_official_params(*spec));
            }
        }
    }
    let spec = resolve_with_tier(model, None, client_budget)?;
    if spec.id.eq_ignore_ascii_case(model) {
        Some(spec)
    } else {
        None
    }
}

/// Top-level compatibility resolver using the client thinking budget.
pub fn resolve(canonical: &str, budget_tokens: Option<u32>) -> Option<RealModelSpec> {
    resolve_with_tier(canonical, None, budget_tokens)
}

/// 已经是 Variant 解析出的上游真实模型 id。
/// 请求路由不能再把它映回客户端公开名，否则上游会收到无法生成的别名。
pub fn is_physical_upstream_id(model: &str) -> bool {
    let key = model.trim();
    if key.ends_with("-high") || key.ends_with("-medium") || key.ends_with("-low") {
        return true;
    }
    GEMINI_FAMILIES.iter().any(|family| {
        family
            .tiers
            .iter()
            .any(|(_, spec)| spec.id.eq_ignore_ascii_case(key))
    })
}

// ── verified real model specs (from upstream spec) ──
// gemini-3.5-flash family (maxOutputTokens = 65536)
const SPEC_35_FLASH_EXTRA_LOW: RealModelSpec = RealModelSpec {
    id: "gemini-3.5-flash-extra-low",
    thinking_budget: 0,
    max_output_tokens: 65536,
    include_thoughts: true,
    preserve_client_budget: false,
};
const SPEC_35_FLASH_LOW: RealModelSpec = RealModelSpec {
    id: "gemini-3.5-flash-low",
    thinking_budget: 0,
    max_output_tokens: 65536,
    include_thoughts: true,
    preserve_client_budget: false,
};
const SPEC_3_FLASH_AGENT: RealModelSpec = RealModelSpec {
    id: "gemini-3-flash-agent",
    thinking_budget: 0,
    max_output_tokens: 65536,
    include_thoughts: true,
    preserve_client_budget: false,
};

// gemini-3.1-pro family (maxOutputTokens = 65535 — note the off-by-one vs Flash)
const SPEC_31_PRO_LOW: RealModelSpec = RealModelSpec {
    id: "gemini-3.1-pro-low",
    thinking_budget: 0,
    max_output_tokens: 65535,
    include_thoughts: true,
    preserve_client_budget: false,
};
const SPEC_31_PRO_HIGH: RealModelSpec = RealModelSpec {
    id: "gemini-3.1-pro-high",
    thinking_budget: 0,
    max_output_tokens: 65535,
    include_thoughts: true,
    preserve_client_budget: false,
};
#[allow(dead_code)]
const SPEC_PRO_AGENT: RealModelSpec = RealModelSpec {
    id: "gemini-pro-agent",
    thinking_budget: 0,
    max_output_tokens: 65535,
    include_thoughts: true,
    preserve_client_budget: false,
};

// Non-variant models
const SPEC_31_FLASH_LITE: RealModelSpec = RealModelSpec {
    id: "gemini-3.1-flash-lite",
    thinking_budget: 0,
    max_output_tokens: 65536,
    include_thoughts: false,
    preserve_client_budget: false,
};
const SPEC_CLAUDE_SONNET_46: RealModelSpec = RealModelSpec {
    id: "claude-sonnet-4-6",
    thinking_budget: 0,
    max_output_tokens: 64000,
    include_thoughts: true,
    preserve_client_budget: true,
};
const SPEC_CLAUDE_OPUS_46: RealModelSpec = RealModelSpec {
    id: "claude-opus-4-6-thinking",
    thinking_budget: 0,
    max_output_tokens: 64000,
    include_thoughts: true,
    preserve_client_budget: true,
};
const SPEC_GPT_OSS_120B: RealModelSpec = RealModelSpec {
    id: "gpt-oss-120b-medium",
    thinking_budget: 0,
    max_output_tokens: 32768,
    include_thoughts: true,
    preserve_client_budget: false,
};

pub static GEMINI_FAMILIES: &[CanonicalFamily] = &[
    CanonicalFamily {
        canonical_id: "gemini-3.5-flash",
        display_name: "Gemini 3.5 Flash",
        context_limit: 1_000_000,
        output_limit: 65_536,
        input_modalities: &["text", "image", "audio", "video", "pdf"],
        output_modalities: &["text"],
        reasoning: true,
        tiers: &[
            (VariantTier::Low, SPEC_35_FLASH_EXTRA_LOW),
            (VariantTier::Medium, SPEC_35_FLASH_LOW),
            (VariantTier::High, SPEC_3_FLASH_AGENT),
        ],
        aliases: &[
            ("gemini-3.5-flash-high", AliasPolicy::HonorTier),
            ("gemini-3.5-flash-medium", AliasPolicy::HonorTier),
            ("gemini-3.5-flash-low", AliasPolicy::HonorTier),
            ("gemini-3-flash", AliasPolicy::HonorTier),
        ],
    },
    CanonicalFamily {
        canonical_id: "gemini-3.1-pro",
        display_name: "Gemini 3.1 Pro",
        context_limit: 1_048_576,
        output_limit: 65_535,
        input_modalities: &["text", "image", "audio", "video", "pdf"],
        output_modalities: &["text"],
        reasoning: true,
        tiers: &[
            (VariantTier::Low, SPEC_31_PRO_LOW),
            (VariantTier::Medium, SPEC_31_PRO_HIGH),
            (VariantTier::High, SPEC_31_PRO_HIGH),
        ],
        aliases: &[
            ("gemini-3.1-pro-high", AliasPolicy::HonorTier),
            ("gemini-pro", AliasPolicy::HonorTier),
            ("gemini-3.1-pro-low", AliasPolicy::HonorTier),
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_catalog_is_the_only_numeric_authority() {
        // 静态 SPEC 不得成为第二权威：flash-lite 的输出上限与思考支持取自官方目录
        let lite = resolve_non_variant_model("gemini-3.1-flash-lite").expect("static spec");
        assert!(
            lite.supports_thinking(),
            "official supportsThinking must win over static thinking_budget==0"
        );
        assert_eq!(lite.max_output_tokens, 65535);
        assert_eq!(
            lite.budget_tokens_option(None),
            None,
            "level channel must not fabricate a numeric budget"
        );

        // 等级通道 + thinkingBudget=-1 的档位模型：不写数字预算，但必须保留思考能力
        // （否则适配层会把它当非思考模型清空 tools）
        let high = resolve_with_tier("gemini-3.8-flash-high", None, None).expect("dynamic spec");
        assert_eq!(high.id, "gemini-3.8-flash-high");
        assert!(high.supports_thinking());
        assert_eq!(high.budget_tokens_option(None), None);
        assert_eq!(high.max_output_tokens, 65536);
    }

    #[test]
    fn claude_specs_preserve_the_client_budget() {
        assert!(SPEC_CLAUDE_SONNET_46.preserve_client_budget);
        assert!(SPEC_CLAUDE_OPUS_46.preserve_client_budget);
        assert_eq!(
            SPEC_CLAUDE_OPUS_46.effective_thinking_budget(Some(32_768)),
            32_768
        );
        assert_eq!(SPEC_CLAUDE_OPUS_46.effective_thinking_budget(None), 0);
    }

    #[test]
    fn non_claude_specs_keep_the_server_budget() {
        assert!(!SPEC_35_FLASH_EXTRA_LOW.preserve_client_budget);
        assert!(!SPEC_35_FLASH_LOW.preserve_client_budget);
        assert!(!SPEC_3_FLASH_AGENT.preserve_client_budget);
        assert!(!SPEC_31_PRO_LOW.preserve_client_budget);
        assert!(!SPEC_PRO_AGENT.preserve_client_budget);
        assert!(!SPEC_31_FLASH_LITE.preserve_client_budget);
        assert!(!SPEC_GPT_OSS_120B.preserve_client_budget);
        assert_eq!(
            SPEC_3_FLASH_AGENT.effective_thinking_budget(Some(32_768)),
            0
        );
    }

    #[test]
    fn test_infer_tier_ranges() {
        assert_eq!(infer_tier(None), VariantTier::High);
        assert_eq!(infer_tier(Some(0)), VariantTier::Low);
        assert_eq!(infer_tier(Some(1999)), VariantTier::Low);
        assert_eq!(infer_tier(Some(2000)), VariantTier::Medium);
        assert_eq!(infer_tier(Some(6999)), VariantTier::Medium);
        assert_eq!(infer_tier(Some(7000)), VariantTier::High);
        assert_eq!(infer_tier(Some(50000)), VariantTier::High);
    }

    #[test]
    fn test_resolve_35_flash_variants() {
        // Medium (default)
        let s = resolve("gemini-3.5-flash", None).unwrap();
        assert_eq!(s.id, "gemini-3.5-flash-low");
        assert_eq!(s.thinking_budget, 4000);
        assert_eq!(s.max_output_tokens, 65536);

        // Medium
        let s = resolve("gemini-3.5-flash", Some(4000)).unwrap();
        assert_eq!(s.id, "gemini-3.5-flash-low");
        assert_eq!(s.thinking_budget, 4000);

        // [USER RULE] 裸模型传入 budget (Some(1000)) 被直接忽略，仍返回 Medium (4000)
        let s = resolve("gemini-3.5-flash", Some(1000)).unwrap();
        assert_eq!(s.id, "gemini-3.5-flash-low");
        assert_eq!(s.thinking_budget, 4000);

        // Low 通过显式 effort 档位接管
        let s = resolve_with_tier("gemini-3.5-flash", Some(VariantTier::Low), None).unwrap();
        assert_eq!(s.id, "gemini-3.5-flash-extra-low");
        assert_eq!(s.thinking_budget, 1000);
    }

    #[test]
    fn test_resolve_37_flash_variants() {
        // 已下线 3.7 裸模型不走 tiered 派生，兜底回落 high
        let s = resolve("gemini-3.7-flash", None).unwrap();
        assert_eq!(s.id, "gemini-3.7-flash-high");
        // 官方 thinkingBudget=-1（自适应）：不再伪造 10000 数字预算
        assert_eq!(s.thinking_budget, 0);
        assert_eq!(s.max_output_tokens, 65536);

        // High
        let s_high = resolve("gemini-3.7-flash-high", None).unwrap();
        assert_eq!(s_high.id, "gemini-3.7-flash-high");
        assert_eq!(s_high.thinking_budget, 0);

        // Medium
        let s_med = resolve("gemini-3.7-flash-medium", None).unwrap();
        assert_eq!(s_med.id, "gemini-3.7-flash-medium");
        assert_eq!(s_med.thinking_budget, 4000);

        // Low
        let s_low = resolve("gemini-3.7-flash-low", None).unwrap();
        assert_eq!(s_low.id, "gemini-3.7-flash-low");
        assert_eq!(s_low.thinking_budget, 1000);
    }

    #[test]
    fn test_resolve_37_flash_high_never_downgraded_by_budget() {
        for budget in [None, Some(0), Some(1000), Some(1024), Some(4000)] {
            let s = resolve("gemini-3.7-flash-high", budget).unwrap();
            assert_eq!(s.id, "gemini-3.7-flash-high");
            assert_eq!(s.thinking_budget, 0);
        }
    }

    #[test]
    fn test_resolve_38_flash_variants() {
        // 无后缀的 3.x Flash 裸模型依据思考档位路由（未指定档位遵循统一决策链优先选择 tiered）
        let s = resolve("gemini-3.8-flash", None).unwrap();
        assert_eq!(s.id, "gemini-3.8-flash-tiered");
        // 官方 tiered = thinkingBudget -1：自适应，不写数字预算
        assert_eq!(s.thinking_budget, 0);
        assert_eq!(s.max_output_tokens, 65536);

        // Tiered 模型原样保留模型名
        let s_tiered = resolve("gemini-3.8-flash-tiered", None).unwrap();
        assert_eq!(s_tiered.id, "gemini-3.8-flash-tiered");

        // High
        let s_high = resolve("gemini-3.8-flash-high", None).unwrap();
        assert_eq!(s_high.id, "gemini-3.8-flash-high");
        assert_eq!(s_high.thinking_budget, 0);

        // Medium
        let s_med = resolve("gemini-3.8-flash-medium", None).unwrap();
        assert_eq!(s_med.id, "gemini-3.8-flash-medium");
        assert_eq!(s_med.thinking_budget, 4000);

        // Low
        let s_low = resolve("gemini-3.8-flash-low", None).unwrap();
        assert_eq!(s_low.id, "gemini-3.8-flash-low");
        assert_eq!(s_low.thinking_budget, 1000);

        // High never downgraded
        for budget in [None, Some(0), Some(1000), Some(1024)] {
            let s = resolve("gemini-3.8-flash-high", budget).unwrap();
            assert_eq!(s.id, "gemini-3.8-flash-high");
            assert_eq!(s.thinking_budget, 0);
        }
    }

    #[test]
    fn test_dynamic_unregistered_gemini_3_family() {
        // Any unregistered Gemini >= 3 model resolves dynamically without hardcoded registry
        // 裸模型依据思考档位路由（未指定默认 High）
        let s = resolve("gemini-3.9-flash", None).unwrap();
        assert_eq!(s.id, "gemini-3.9-flash-high");
        // 未收录模型不再内置预算表：0 = 不伪造数字预算
        assert_eq!(s.thinking_budget, 0);
        assert_eq!(s.max_output_tokens, 65536);

        // Explicit tier in name
        let s_med = resolve("gemini-3.9-flash-medium", None).unwrap();
        assert_eq!(s_med.id, "gemini-3.9-flash-medium");
        assert_eq!(s_med.thinking_budget, 0);

        let s_low = resolve("gemini-3.9-flash-low", None).unwrap();
        assert_eq!(s_low.id, "gemini-3.9-flash-low");
        assert_eq!(s_low.thinking_budget, 0);

        // Budget never downgrades unregistered -high model
        for budget in [None, Some(0), Some(1000), Some(1024)] {
            let s = resolve("gemini-3.9-flash-high", budget).unwrap();
            assert_eq!(s.id, "gemini-3.9-flash-high");
            assert_eq!(s.thinking_budget, 0);
        }

        // Pro model also has no fabricated budget
        let s = resolve("gemini-3.9-pro", None).unwrap();
        assert_eq!(s.id, "gemini-3.9-pro");
        assert_eq!(s.thinking_budget, 0);
    }

    #[test]
    fn tier_from_effort_accepts_only_anthropic_sdk_values() {
        assert_eq!(tier_from_effort(Some("low")), Some(VariantTier::Low));
        assert_eq!(tier_from_effort(Some("extra-low")), Some(VariantTier::Low));
        assert_eq!(tier_from_effort(Some("min")), Some(VariantTier::Low));
        assert_eq!(tier_from_effort(Some("medium")), Some(VariantTier::Medium));
        assert_eq!(
            tier_from_effort(Some("standard")),
            Some(VariantTier::Medium)
        );
        // `default` = 未显式指定档位：交给统一默认决策链（与 gemini / responses 对齐）
        assert_eq!(tier_from_effort(Some("default")), None);
        assert_eq!(tier_from_effort(Some("high")), Some(VariantTier::High));
        assert_eq!(tier_from_effort(Some("max")), Some(VariantTier::High));
        assert_eq!(tier_from_effort(Some("xhigh")), Some(VariantTier::High));
        assert_eq!(tier_from_effort(Some("extreme")), Some(VariantTier::High));
        assert_eq!(tier_from_effort(Some("unknown")), None);
        assert_eq!(tier_from_effort(None), None);
    }

    #[test]
    fn resolve_with_tier_prefers_explicit_effort_over_budget() {
        for (canonical, tier, budget, expected_id) in [
            (
                "gemini-3.5-flash",
                VariantTier::Low,
                10_000,
                "gemini-3.5-flash-extra-low",
            ),
            (
                "gemini-3.5-flash",
                VariantTier::Medium,
                1_000,
                "gemini-3.5-flash-low",
            ),
            (
                "gemini-3.5-flash",
                VariantTier::High,
                1_000,
                "gemini-3-flash-agent",
            ),
            (
                "gemini-3.1-pro",
                VariantTier::Low,
                10_000,
                "gemini-3.1-pro-low",
            ),
            (
                "gemini-3.1-pro",
                VariantTier::Medium,
                1_000,
                "gemini-3.1-pro-high",
            ),
            (
                "gemini-3.1-pro",
                VariantTier::High,
                1_000,
                "gemini-3.1-pro-high",
            ),
        ] {
            let spec = resolve_with_tier(canonical, Some(tier), Some(budget))
                .expect("canonical Gemini tier must resolve");
            assert_eq!(spec.id, expected_id);
        }
    }

    #[test]
    fn gemini_3_flash_alias_follows_tier() {
        // 裸模型 budget 字段被彻底忽略，由思考强度接管；未传 effort 默认 Medium
        check(
            "gemini-3-flash",
            Some(0),
            "gemini-3.5-flash-low",
            4000,
            65536,
        );
        check(
            "gemini-3-flash",
            Some(4000),
            "gemini-3.5-flash-low",
            4000,
            65536,
        );
        check("gemini-3-flash", None, "gemini-3.5-flash-low", 4000, 65536);

        // 显式 effort 档位依然严格接管
        let low_spec = resolve_with_tier("gemini-3-flash", Some(VariantTier::Low), None).unwrap();
        assert_eq!(low_spec.id, "gemini-3.5-flash-extra-low");
        let high_spec = resolve_with_tier("gemini-3-flash", Some(VariantTier::High), None).unwrap();
        assert_eq!(high_spec.id, "gemini-3-flash-agent");
    }

    #[test]
    fn test_resolve_31_pro_variants() {
        let s = resolve("gemini-3.1-pro", None).unwrap();
        assert_eq!(s.id, "gemini-3.1-pro-high");
        assert_eq!(s.thinking_budget, 10001);
        assert_eq!(s.max_output_tokens, 65535); // Pro uses 65535, not 65536

        // 裸模型 budget 字段被彻底忽略，统一默认 Medium (10001)
        let s = resolve("gemini-3.1-pro", Some(1001)).unwrap();
        assert_eq!(s.id, "gemini-3.1-pro-high");
        assert_eq!(s.thinking_budget, 10001);

        // 显式 effort Low 接管
        let low = resolve_with_tier("gemini-3.1-pro", Some(VariantTier::Low), None).unwrap();
        assert_eq!(low.id, "gemini-3.1-pro-low");
        assert_eq!(low.thinking_budget, 1001);
    }

    #[test]
    fn test_resolve_non_variant_models() {
        let s = resolve("gemini-3.1-flash-lite", None).unwrap();
        assert_eq!(s.id, "gemini-3.1-flash-lite");
        assert_eq!(s.thinking_budget, 0);
        assert!(s.include_thoughts);

        let s = resolve("claude-sonnet-4-6", None).unwrap();
        assert_eq!(s.id, "claude-sonnet-4-6");
        assert_eq!(s.thinking_budget, 1024);
        assert_eq!(s.max_output_tokens, 64000);

        // Gemini 3.1 Pro 衍生裸模型动态解析
        let s = resolve("gemini-3.1-pro-low", None).unwrap();
        assert_eq!(s.id, "gemini-3.1-pro-low");

        let s = resolve("gemini-3.1-pro-high", None).unwrap();
        assert_eq!(s.id, "gemini-3.1-pro-high");

        // 裸模型解析（缺省遵循决策链 -> Medium -> gemini-3.1-pro-high）
        let s = resolve("gemini-3.1-pro", None).unwrap();
        assert_eq!(s.id, "gemini-3.1-pro-high");

        // 裸模型带显式档位解析
        let s = resolve_with_tier("gemini-3.1-pro", Some(VariantTier::Low), None).unwrap();
        assert_eq!(s.id, "gemini-3.1-pro-low");
    }

    #[test]
    fn test_resolve_argon_variants_safeguard_negative_budget() {
        // 验证 gemini-4-argon-* 即使元数据中存在 thinkingBudget: -1 哨兵值，也不会溢出为 u32::MAX (4294967295)
        let s_high = resolve("gemini-4-argon-high", None).unwrap();
        assert_eq!(s_high.id, "gemini-4-argon-high");
        assert_eq!(s_high.thinking_budget, 0);

        let s_med = resolve("gemini-4-argon-medium", None).unwrap();
        assert_eq!(s_med.id, "gemini-4-argon-medium");
        assert_eq!(s_med.thinking_budget, 0);

        let s_low = resolve("gemini-4-argon-low", None).unwrap();
        assert_eq!(s_low.id, "gemini-4-argon-low");
        assert_eq!(s_low.thinking_budget, 0);
    }

    #[test]
    fn test_unknown_model_returns_none() {
        assert!(resolve("some-unknown-model", None).is_none());
    }

    #[test]
    fn test_case_insensitive() {
        let s = resolve("GEMINI-3.5-FLASH", None).unwrap();
        assert_eq!(s.id, "gemini-3.5-flash-low");
    }

    // ═════════════════════════════════════════════════════════════════════
    // baseline tests — capture current resolve() output as equivalence
    // anchors for future refactoring.  Do NOT change these assertions;
    // if a refactor breaks them, the refactor is wrong.
    // ═════════════════════════════════════════════════════════════════════

    /// Helper: call `resolve(canonical, budget)` and assert id / tb / mot.
    fn check(canonical: &str, budget: Option<u32>, id: &str, tb: u32, mot: u32) {
        let s = resolve(canonical, budget).unwrap_or_else(|| {
            panic!("resolve({canonical:?}, {budget:?}) unexpectedly returned None")
        });
        assert_eq!(s.id, id, "resolve({canonical:?}, {budget:?}).id");
        assert_eq!(
            s.thinking_budget, tb,
            "resolve({canonical:?}, {budget:?}).thinking_budget"
        );
        assert_eq!(
            s.max_output_tokens, mot,
            "resolve({canonical:?}, {budget:?}).max_output_tokens"
        );
    }

    #[test]
    fn baseline_resolve_35_flash_variants() {
        // ── gemini-3.5-flash (canonical) ───────────────────────────────
        // None → Medium → gemini-3.5-flash-low
        check(
            "gemini-3.5-flash",
            None,
            "gemini-3.5-flash-low",
            4000,
            65536,
        );
        // [USER RULE] 裸模型无论传入任何 budget，均彻底忽略，一律默认 Medium -> gemini-3.5-flash-low (4000)
        check(
            "gemini-3.5-flash",
            Some(0),
            "gemini-3.5-flash-low",
            4000,
            65536,
        );
        check(
            "gemini-3.5-flash",
            Some(4000),
            "gemini-3.5-flash-low",
            4000,
            65536,
        );
        check(
            "gemini-3.5-flash",
            Some(7000),
            "gemini-3.5-flash-low",
            4000,
            65536,
        );

        // ── gemini-3.5-flash-high ──────────────────────────────────────
        // 显式 -high 锁定最高档位，绝不接受 budget 降级
        check(
            "gemini-3.5-flash-high",
            None,
            "gemini-3-flash-agent",
            0,
            65536,
        );
        check(
            "gemini-3.5-flash-high",
            Some(0),
            "gemini-3-flash-agent",
            0,
            65536,
        );
        check(
            "gemini-3.5-flash-high",
            Some(4000),
            "gemini-3-flash-agent",
            0,
            65536,
        );
        check(
            "gemini-3.5-flash-high",
            Some(7000),
            "gemini-3-flash-agent",
            0,
            65536,
        );

        // ── gemini-3.5-flash-medium (fixed → SPEC_35_FLASH_LOW) ────────
        check(
            "gemini-3.5-flash-medium",
            None,
            "gemini-3.5-flash-low",
            4000,
            65536,
        );
        check(
            "gemini-3.5-flash-medium",
            Some(0),
            "gemini-3.5-flash-low",
            4000,
            65536,
        );
        check(
            "gemini-3.5-flash-medium",
            Some(4000),
            "gemini-3.5-flash-low",
            4000,
            65536,
        );
        check(
            "gemini-3.5-flash-medium",
            Some(7000),
            "gemini-3.5-flash-low",
            4000,
            65536,
        );

        // ── gemini-3.5-flash-low (fixed → SPEC_35_FLASH_EXTRA_LOW) ─────
        check(
            "gemini-3.5-flash-low",
            None,
            "gemini-3.5-flash-extra-low",
            1000,
            65536,
        );
        check(
            "gemini-3.5-flash-low",
            Some(0),
            "gemini-3.5-flash-extra-low",
            1000,
            65536,
        );
        check(
            "gemini-3.5-flash-low",
            Some(4000),
            "gemini-3.5-flash-extra-low",
            1000,
            65536,
        );
        check(
            "gemini-3.5-flash-low",
            Some(7000),
            "gemini-3.5-flash-extra-low",
            1000,
            65536,
        );
    }

    #[test]
    fn baseline_resolve_31_pro_variants() {
        // ── gemini-3.1-pro (canonical) ─────────────────────────────────
        // None → High → gemini-3.1-pro-high
        check("gemini-3.1-pro", None, "gemini-3.1-pro-high", 10001, 65535);
        // Some(0) → budget ignored → Medium/High → gemini-3.1-pro-high
        check(
            "gemini-3.1-pro",
            Some(0),
            "gemini-3.1-pro-high",
            10001,
            65535,
        );
        // Some(4000) → Medium → High fallback → gemini-3.1-pro-high
        check(
            "gemini-3.1-pro",
            Some(4000),
            "gemini-3.1-pro-high",
            10001,
            65535,
        );
        // Some(7000) → High → gemini-3.1-pro-high
        check(
            "gemini-3.1-pro",
            Some(7000),
            "gemini-3.1-pro-high",
            10001,
            65535,
        );

        // ── gemini-3.1-pro-high (same canonical arm) ───────────────────
        check(
            "gemini-3.1-pro-high",
            None,
            "gemini-3.1-pro-high",
            10001,
            65535,
        );
        check(
            "gemini-3.1-pro-high",
            Some(0),
            "gemini-3.1-pro-high",
            10001,
            65535,
        );
        check(
            "gemini-3.1-pro-high",
            Some(4000),
            "gemini-3.1-pro-high",
            10001,
            65535,
        );
        check(
            "gemini-3.1-pro-high",
            Some(7000),
            "gemini-3.1-pro-high",
            10001,
            65535,
        );

        // ── gemini-pro (same canonical arm) ────────────────────────────
        check("gemini-pro", None, "gemini-3.1-pro-high", 10001, 65535);
        check("gemini-pro", Some(0), "gemini-3.1-pro-high", 10001, 65535);
        check(
            "gemini-pro",
            Some(4000),
            "gemini-3.1-pro-high",
            10001,
            65535,
        );
        check(
            "gemini-pro",
            Some(7000),
            "gemini-3.1-pro-high",
            10001,
            65535,
        );

        // ── gemini-3.1-pro-low (fixed → SPEC_31_PRO_LOW) ──────────────
        check(
            "gemini-3.1-pro-low",
            None,
            "gemini-3.1-pro-low",
            1001,
            65535,
        );
        check(
            "gemini-3.1-pro-low",
            Some(0),
            "gemini-3.1-pro-low",
            1001,
            65535,
        );
        check(
            "gemini-3.1-pro-low",
            Some(4000),
            "gemini-3.1-pro-low",
            1001,
            65535,
        );
        check(
            "gemini-3.1-pro-low",
            Some(7000),
            "gemini-3.1-pro-low",
            1001,
            65535,
        );
    }

    #[test]
    fn baseline_resolve_non_variant_models() {
        // Gemini flash-lite family – all map to SPEC_31_FLASH_LITE
        let check_lite = |c: &str| {
            check(c, None, "gemini-3.1-flash-lite", 0, 65535);
            // budget should not affect non-variant resolution
            check(c, Some(0), "gemini-3.1-flash-lite", 0, 65535);
            check(c, Some(7000), "gemini-3.1-flash-lite", 0, 65535);
        };
        check_lite("gemini-3.1-flash-lite");
        // [RETIRED] Gemini 2.x 已下线，不得再命中任何内部别名映射
        assert!(resolve("gemini-2.5-flash-lite", None).is_none());
        // 新行为：gemini-2.5-flash 仍保留内部映射
        assert!(resolve("gemini-2.5-flash", None).is_some());
        assert!(resolve("gemini-2.5-flash-thinking", None).is_none());

        // ── Claude family ──────────────────────────────────────────────
        check("claude-sonnet-4-6", None, "claude-sonnet-4-6", 1024, 64000);
        // claude-opus-4-6-thinking and claude-opus-4-6 → same spec
        check(
            "claude-opus-4-6-thinking",
            None,
            "claude-opus-4-6-thinking",
            1024,
            64000,
        );
        check(
            "claude-opus-4-6",
            None,
            "claude-opus-4-6-thinking",
            1024,
            64000,
        );

        // ── GPT OSS ────────────────────────────────────────────────────
        check(
            "gpt-oss-120b-medium",
            None,
            "gpt-oss-120b-medium",
            8192,
            32768,
        );
    }

    #[test]
    fn baseline_unknown_model() {
        assert!(resolve("some-unknown-model", None).is_none());
        assert!(resolve("gemini-4.0-super", Some(4000)).is_none());
        assert!(resolve("", None).is_none());
    }

    #[test]
    fn baseline_case_insensitive() {
        // Canonical variant split model
        check(
            "GEMINI-3.5-FLASH",
            None,
            "gemini-3.5-flash-low",
            4000,
            65536,
        );
        check(
            "Gemini-3.5-Flash",
            None,
            "gemini-3.5-flash-low",
            4000,
            65536,
        );
        check("GEMINI-3.1-PRO", None, "gemini-3.1-pro-high", 10001, 65535);
        // Non-variant model
        check("CLAUDE-SONNET-4-6", None, "claude-sonnet-4-6", 1024, 64000);
        check(
            "GPT-OSS-120B-MEDIUM",
            None,
            "gpt-oss-120b-medium",
            8192,
            32768,
        );
    }

    // ═════════════════════════════════════════════════════════════════════
    // old catalog id fallback — these legacy top-level ids must keep
    // resolving through their family aliases.  See task-5-brief.md.
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn old_catalog_alias_fallback() {
        // ── gemini-3.1-pro-high (Fixed(High) → always gemini-3.1-pro-high, never downgraded) ────
        // None → High → gemini-3.1-pro-high
        check(
            "gemini-3.1-pro-high",
            None,
            "gemini-3.1-pro-high",
            10001,
            65535,
        );
        // Some(0) → High (Fixed, no downgrade) → gemini-3.1-pro-high
        check(
            "gemini-3.1-pro-high",
            Some(0),
            "gemini-3.1-pro-high",
            10001,
            65535,
        );
        // Some(4000) → High (Fixed, no downgrade) → gemini-3.1-pro-high
        check(
            "gemini-3.1-pro-high",
            Some(4000),
            "gemini-3.1-pro-high",
            10001,
            65535,
        );
        // Some(7000) → High → gemini-3.1-pro-high
        check(
            "gemini-3.1-pro-high",
            Some(7000),
            "gemini-3.1-pro-high",
            10001,
            65535,
        );

        // ── gemini-3.1-pro-low (Fixed(Low) → always gemini-3.1-pro-low)
        check(
            "gemini-3.1-pro-low",
            None,
            "gemini-3.1-pro-low",
            1001,
            65535,
        );
        check(
            "gemini-3.1-pro-low",
            Some(0),
            "gemini-3.1-pro-low",
            1001,
            65535,
        );
        check(
            "gemini-3.1-pro-low",
            Some(4000),
            "gemini-3.1-pro-low",
            1001,
            65535,
        );
        check(
            "gemini-3.1-pro-low",
            Some(7000),
            "gemini-3.1-pro-low",
            1001,
            65535,
        );

        // ── gemini-pro (HonorTier → same as gemini-3.1-pro) ────────────
        check("gemini-pro", None, "gemini-3.1-pro-high", 10001, 65535);
        check("gemini-pro", Some(0), "gemini-3.1-pro-high", 10001, 65535);
        check(
            "gemini-pro",
            Some(4000),
            "gemini-3.1-pro-high",
            10001,
            65535,
        );
        check(
            "gemini-pro",
            Some(7000),
            "gemini-3.1-pro-high",
            10001,
            65535,
        );

        // ── gemini-3-flash (HonorTier → same as gemini-3.5-flash) ──────
        check("gemini-3-flash", None, "gemini-3.5-flash-low", 4000, 65536);
        check(
            "gemini-3-flash",
            Some(0),
            "gemini-3.5-flash-low",
            4000,
            65536,
        );
        check(
            "gemini-3-flash",
            Some(4000),
            "gemini-3.5-flash-low",
            4000,
            65536,
        );
        check(
            "gemini-3-flash",
            Some(7000),
            "gemini-3.5-flash-low",
            4000,
            65536,
        );
    }
}
