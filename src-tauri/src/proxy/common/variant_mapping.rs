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

/// Parse a supported Anthropic SDK output effort into a Gemini variant tier.
pub fn tier_from_effort(effort: Option<&str>) -> Option<VariantTier> {
    match effort {
        Some("low") => Some(VariantTier::Low),
        Some("medium") => Some(VariantTier::Medium),
        Some("high") => Some(VariantTier::High),
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
            .map(|(_, spec)| *spec);
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
    if matches!(
        key.as_str(),
        "gemini-3.1-flash-lite"
            | "gemini-2.5-flash-lite"
            | "gemini-2.5-flash"
            | "gemini-2.5-flash-thinking"
    ) {
        return Some(SPEC_31_FLASH_LITE);
    }
    if key == "claude-sonnet-4-6" {
        return Some(SPEC_CLAUDE_SONNET_46);
    }
    if matches!(key.as_str(), "claude-opus-4-6-thinking" | "claude-opus-4-6") {
        return Some(SPEC_CLAUDE_OPUS_46);
    }
    if key == "gpt-oss-120b-medium" {
        return Some(SPEC_GPT_OSS_120B);
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
    // 其次遵从显式传递的 effort 档位；无显式档位时若有 budget 则推断档位，无 budget 则一律默认 High
    let tier = if let Some(nt) = name_tier {
        nt
    } else if let Some(et) = explicit_tier {
        et
    } else if let Some(bt) = budget_tokens {
        infer_tier(Some(bt))
    } else {
        VariantTier::High
    };

    // 2. 尝试从已知家族注册表中匹配
    if let Some(spec) = resolve_real_model(canonical, tier) {
        return Some(spec);
    }

    // 3. 通用动态解析 (治本):
    // 针对任何 >= 3.0 的 Gemini 衍生模型（如 gemini-3.8-flash, gemini-3.9-flash, gemini-4.0-flash 等），
    // 即使未在 GEMINI_FAMILIES 中硬编码，也能自动根据档位生成完整规范的 RealModelSpec，
    // 确保思考模式无缝开启，预算自动校准，绝不降级。
    let is_gemini_3_family = is_v3 && (lower.contains("flash") || lower.contains("pro") || lower.contains("agent"));
    if is_gemini_3_family {
        let dynamic_tier = if let Some(nt) = name_tier {
            nt
        } else if let Some(et) = explicit_tier {
            et
        } else if lower.contains("high") || lower.contains("agent") || lower.contains("pro") {
            VariantTier::High
        } else if lower.contains("low") {
            VariantTier::Low
        } else {
            // medium 或者不带档位后缀，统一赋予 4000 (Medium 内置逆向规范)
            VariantTier::Medium
        };
        let budget = match dynamic_tier {
            VariantTier::High => {
                if lower.contains("pro") {
                    10001
                } else {
                    10000
                }
            }
            VariantTier::Medium => {
                if lower.contains("pro") {
                    10001
                } else {
                    4000
                }
            }
            VariantTier::Low => {
                if lower.contains("pro") {
                    1001
                } else {
                    1000
                }
            }
        };
        let max_output_tokens = if lower.contains("pro") { 65535 } else { 65536 };
        let id: &'static str = Box::leak(canonical.to_string().into_boxed_str());
        return Some(RealModelSpec {
            id,
            thinking_budget: budget,
            max_output_tokens,
            include_thoughts: true,
            preserve_client_budget: false,
        });
    }

    None
}

/// Top-level compatibility resolver using the client thinking budget.
pub fn resolve(canonical: &str, budget_tokens: Option<u32>) -> Option<RealModelSpec> {
    resolve_with_tier(canonical, None, budget_tokens)
}

// ── verified real model specs (from upstream spec) ──
// gemini-3.5-flash family (maxOutputTokens = 65536)
const SPEC_35_FLASH_EXTRA_LOW: RealModelSpec = RealModelSpec {
    id: "gemini-3.5-flash-extra-low",
    thinking_budget: 1000,
    max_output_tokens: 65536,
    include_thoughts: true,
    preserve_client_budget: false,
};
const SPEC_35_FLASH_LOW: RealModelSpec = RealModelSpec {
    id: "gemini-3.5-flash-low",
    thinking_budget: 4000,
    max_output_tokens: 65536,
    include_thoughts: true,
    preserve_client_budget: false,
};
const SPEC_3_FLASH_AGENT: RealModelSpec = RealModelSpec {
    id: "gemini-3-flash-agent",
    thinking_budget: 10000,
    max_output_tokens: 65536,
    include_thoughts: true,
    preserve_client_budget: false,
};

// gemini-3.1-pro family (maxOutputTokens = 65535 — note the off-by-one vs Flash)
const SPEC_31_PRO_LOW: RealModelSpec = RealModelSpec {
    id: "gemini-3.1-pro-low",
    thinking_budget: 1001,
    max_output_tokens: 65535,
    include_thoughts: true,
    preserve_client_budget: false,
};
const SPEC_PRO_AGENT: RealModelSpec = RealModelSpec {
    id: "gemini-pro-agent",
    thinking_budget: 10001,
    max_output_tokens: 65535,
    include_thoughts: true,
    preserve_client_budget: false,
};

// Non-variant models
const SPEC_31_FLASH_LITE: RealModelSpec = RealModelSpec {
    id: "gemini-3.1-flash-lite",
    thinking_budget: 0,
    max_output_tokens: 16384,
    include_thoughts: false,
    preserve_client_budget: false,
};
const SPEC_CLAUDE_SONNET_46: RealModelSpec = RealModelSpec {
    id: "claude-sonnet-4-6",
    thinking_budget: 1024,
    max_output_tokens: 64000,
    include_thoughts: true,
    preserve_client_budget: true,
};
const SPEC_CLAUDE_OPUS_46: RealModelSpec = RealModelSpec {
    id: "claude-opus-4-6-thinking",
    thinking_budget: 1024,
    max_output_tokens: 64000,
    include_thoughts: true,
    preserve_client_budget: true,
};
const SPEC_GPT_OSS_120B: RealModelSpec = RealModelSpec {
    id: "gpt-oss-120b-medium",
    thinking_budget: 8192,
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
            ("gemini-3.5-flash-high", AliasPolicy::Fixed(VariantTier::High)),
            (
                "gemini-3.5-flash-medium",
                AliasPolicy::Fixed(VariantTier::Medium),
            ),
            ("gemini-3.5-flash-low", AliasPolicy::Fixed(VariantTier::Low)),
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
            (VariantTier::Medium, SPEC_PRO_AGENT),
            (VariantTier::High, SPEC_PRO_AGENT),
        ],
        aliases: &[
            ("gemini-3.1-pro-high", AliasPolicy::Fixed(VariantTier::High)),
            ("gemini-pro", AliasPolicy::HonorTier),
            ("gemini-3.1-pro-low", AliasPolicy::Fixed(VariantTier::Low)),
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_specs_preserve_the_client_budget() {
        assert!(SPEC_CLAUDE_SONNET_46.preserve_client_budget);
        assert!(SPEC_CLAUDE_OPUS_46.preserve_client_budget);
        assert_eq!(
            SPEC_CLAUDE_OPUS_46.effective_thinking_budget(Some(32_768)),
            32_768
        );
        assert_eq!(SPEC_CLAUDE_OPUS_46.effective_thinking_budget(None), 1_024);
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
            10_000
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
        // High (default)
        let s = resolve("gemini-3.5-flash", None).unwrap();
        assert_eq!(s.id, "gemini-3-flash-agent");
        assert_eq!(s.thinking_budget, 10000);
        assert_eq!(s.max_output_tokens, 65536);

        // Medium
        let s = resolve("gemini-3.5-flash", Some(4000)).unwrap();
        assert_eq!(s.id, "gemini-3.5-flash-low");
        assert_eq!(s.thinking_budget, 4000);

        // Low
        let s = resolve("gemini-3.5-flash", Some(1000)).unwrap();
        assert_eq!(s.id, "gemini-3.5-flash-extra-low");
        assert_eq!(s.thinking_budget, 1000);
    }

    #[test]
    fn test_resolve_37_flash_variants() {
        // 未带档位后缀的 Flash 模型默认赋予 medium (4000)
        let s = resolve("gemini-3.7-flash", None).unwrap();
        assert_eq!(s.id, "gemini-3.7-flash");
        assert_eq!(s.thinking_budget, 4000);
        assert_eq!(s.max_output_tokens, 65536);

        // High
        let s_high = resolve("gemini-3.7-flash-high", None).unwrap();
        assert_eq!(s_high.id, "gemini-3.7-flash-high");
        assert_eq!(s_high.thinking_budget, 10000);

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
            assert_eq!(s.thinking_budget, 10000);
        }
    }

    #[test]
    fn test_resolve_38_flash_variants() {
        // 未带档位后缀的 Flash 模型默认赋予 medium (4000)
        let s = resolve("gemini-3.8-flash", None).unwrap();
        assert_eq!(s.id, "gemini-3.8-flash");
        assert_eq!(s.thinking_budget, 4000);
        assert_eq!(s.max_output_tokens, 65536);

        // High
        let s_high = resolve("gemini-3.8-flash-high", None).unwrap();
        assert_eq!(s_high.id, "gemini-3.8-flash-high");
        assert_eq!(s_high.thinking_budget, 10000);

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
            assert_eq!(s.thinking_budget, 10000);
        }
    }

    #[test]
    fn test_dynamic_unregistered_gemini_3_family() {
        // Any unregistered Gemini >= 3 model resolves dynamically without hardcoded registry
        // 未带档位后缀的 Flash 模型默认赋予 medium (4000)
        let s = resolve("gemini-3.9-flash", None).unwrap();
        assert_eq!(s.id, "gemini-3.9-flash");
        assert_eq!(s.thinking_budget, 4000);
        assert_eq!(s.max_output_tokens, 65536);

        // Explicit tier in name
        let s_med = resolve("gemini-3.9-flash-medium", None).unwrap();
        assert_eq!(s_med.id, "gemini-3.9-flash-medium");
        assert_eq!(s_med.thinking_budget, 4000);

        let s_low = resolve("gemini-3.9-flash-low", None).unwrap();
        assert_eq!(s_low.id, "gemini-3.9-flash-low");
        assert_eq!(s_low.thinking_budget, 1000);

        // Budget never downgrades unregistered -high model
        for budget in [None, Some(0), Some(1000), Some(1024)] {
            let s = resolve("gemini-3.9-flash-high", budget).unwrap();
            assert_eq!(s.id, "gemini-3.9-flash-high");
            assert_eq!(s.thinking_budget, 10000);
        }

        // Pro model defaults to 10001
        let s = resolve("gemini-3.9-pro", None).unwrap();
        assert_eq!(s.id, "gemini-3.9-pro");
        assert_eq!(s.thinking_budget, 10001);
    }

    #[test]
    fn tier_from_effort_accepts_only_anthropic_sdk_values() {
        assert_eq!(tier_from_effort(Some("low")), Some(VariantTier::Low));
        assert_eq!(tier_from_effort(Some("medium")), Some(VariantTier::Medium));
        assert_eq!(tier_from_effort(Some("high")), Some(VariantTier::High));
        assert_eq!(tier_from_effort(Some("max")), None);
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
                "gemini-pro-agent",
            ),
            (
                "gemini-3.1-pro",
                VariantTier::High,
                1_000,
                "gemini-pro-agent",
            ),
        ] {
            let spec = resolve_with_tier(canonical, Some(tier), Some(budget))
                .expect("canonical Gemini tier must resolve");
            assert_eq!(spec.id, expected_id);
        }
    }

    #[test]
    fn gemini_3_flash_alias_follows_tier() {
        check(
            "gemini-3-flash",
            Some(0),
            "gemini-3.5-flash-extra-low",
            1000,
            65536,
        );
        check(
            "gemini-3-flash",
            Some(4000),
            "gemini-3.5-flash-low",
            4000,
            65536,
        );
        check("gemini-3-flash", None, "gemini-3-flash-agent", 10000, 65536);
    }

    #[test]
    fn test_resolve_31_pro_variants() {
        let s = resolve("gemini-3.1-pro", None).unwrap();
        assert_eq!(s.id, "gemini-pro-agent");
        assert_eq!(s.thinking_budget, 10001);
        assert_eq!(s.max_output_tokens, 65535); // Pro uses 65535, not 65536

        let s = resolve("gemini-3.1-pro", Some(1001)).unwrap();
        assert_eq!(s.id, "gemini-3.1-pro-low");
        assert_eq!(s.thinking_budget, 1001);
    }

    #[test]
    fn test_resolve_non_variant_models() {
        let s = resolve("gemini-3.1-flash-lite", None).unwrap();
        assert_eq!(s.id, "gemini-3.1-flash-lite");
        assert_eq!(s.thinking_budget, 0);
        assert!(!s.include_thoughts);

        let s = resolve("claude-sonnet-4-6", None).unwrap();
        assert_eq!(s.id, "claude-sonnet-4-6");
        assert_eq!(s.thinking_budget, 1024);
        assert_eq!(s.max_output_tokens, 64000);
    }

    #[test]
    fn test_unknown_model_returns_none() {
        assert!(resolve("some-unknown-model", None).is_none());
    }

    #[test]
    fn test_case_insensitive() {
        let s = resolve("GEMINI-3.5-FLASH", None).unwrap();
        assert_eq!(s.id, "gemini-3-flash-agent");
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
        // None → High → gemini-3-flash-agent
        check(
            "gemini-3.5-flash",
            None,
            "gemini-3-flash-agent",
            10000,
            65536,
        );
        // Some(0) → Low → gemini-3.5-flash-extra-low
        check(
            "gemini-3.5-flash",
            Some(0),
            "gemini-3.5-flash-extra-low",
            1000,
            65536,
        );
        // Some(4000) → Medium → gemini-3.5-flash-low
        check(
            "gemini-3.5-flash",
            Some(4000),
            "gemini-3.5-flash-low",
            4000,
            65536,
        );
        // Some(7000) → High → gemini-3-flash-agent
        check(
            "gemini-3.5-flash",
            Some(7000),
            "gemini-3-flash-agent",
            10000,
            65536,
        );

        // ── gemini-3.5-flash-high ──────────────────────────────────────
        // 显式 -high 锁定最高档位，绝不接受 budget 降级
        check(
            "gemini-3.5-flash-high",
            None,
            "gemini-3-flash-agent",
            10000,
            65536,
        );
        check(
            "gemini-3.5-flash-high",
            Some(0),
            "gemini-3-flash-agent",
            10000,
            65536,
        );
        check(
            "gemini-3.5-flash-high",
            Some(4000),
            "gemini-3-flash-agent",
            10000,
            65536,
        );
        check(
            "gemini-3.5-flash-high",
            Some(7000),
            "gemini-3-flash-agent",
            10000,
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
        // None → High → gemini-pro-agent
        check("gemini-3.1-pro", None, "gemini-pro-agent", 10001, 65535);
        // Some(0) → Low → gemini-3.1-pro-low
        check("gemini-3.1-pro", Some(0), "gemini-3.1-pro-low", 1001, 65535);
        // Some(4000) → Medium → High fallback → gemini-pro-agent
        check(
            "gemini-3.1-pro",
            Some(4000),
            "gemini-pro-agent",
            10001,
            65535,
        );
        // Some(7000) → High → gemini-pro-agent
        check(
            "gemini-3.1-pro",
            Some(7000),
            "gemini-pro-agent",
            10001,
            65535,
        );

        // ── gemini-3.1-pro-high (same canonical arm) ───────────────────
        check(
            "gemini-3.1-pro-high",
            None,
            "gemini-pro-agent",
            10001,
            65535,
        );
        check(
            "gemini-3.1-pro-high",
            Some(0),
            "gemini-pro-agent",
            10001,
            65535,
        );
        check(
            "gemini-3.1-pro-high",
            Some(4000),
            "gemini-pro-agent",
            10001,
            65535,
        );
        check(
            "gemini-3.1-pro-high",
            Some(7000),
            "gemini-pro-agent",
            10001,
            65535,
        );

        // ── gemini-pro (same canonical arm) ────────────────────────────
        check("gemini-pro", None, "gemini-pro-agent", 10001, 65535);
        check("gemini-pro", Some(0), "gemini-3.1-pro-low", 1001, 65535);
        check("gemini-pro", Some(4000), "gemini-pro-agent", 10001, 65535);
        check("gemini-pro", Some(7000), "gemini-pro-agent", 10001, 65535);

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
            check(c, None, "gemini-3.1-flash-lite", 0, 16384);
            // budget should not affect non-variant resolution
            check(c, Some(0), "gemini-3.1-flash-lite", 0, 16384);
            check(c, Some(7000), "gemini-3.1-flash-lite", 0, 16384);
        };
        check_lite("gemini-3.1-flash-lite");
        check_lite("gemini-2.5-flash-lite");
        check_lite("gemini-2.5-flash");
        check_lite("gemini-2.5-flash-thinking");

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
            "gemini-3-flash-agent",
            10000,
            65536,
        );
        check(
            "Gemini-3.5-Flash",
            None,
            "gemini-3-flash-agent",
            10000,
            65536,
        );
        check("GEMINI-3.1-PRO", None, "gemini-pro-agent", 10001, 65535);
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
        // ── gemini-3.1-pro-high (Fixed(High) → always gemini-pro-agent, never downgraded) ────
        // None → High → gemini-pro-agent
        check(
            "gemini-3.1-pro-high",
            None,
            "gemini-pro-agent",
            10001,
            65535,
        );
        // Some(0) → High (Fixed, no downgrade) → gemini-pro-agent
        check(
            "gemini-3.1-pro-high",
            Some(0),
            "gemini-pro-agent",
            10001,
            65535,
        );
        // Some(4000) → High (Fixed, no downgrade) → gemini-pro-agent
        check(
            "gemini-3.1-pro-high",
            Some(4000),
            "gemini-pro-agent",
            10001,
            65535,
        );
        // Some(7000) → High → gemini-pro-agent
        check(
            "gemini-3.1-pro-high",
            Some(7000),
            "gemini-pro-agent",
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
        check("gemini-pro", None, "gemini-pro-agent", 10001, 65535);
        check("gemini-pro", Some(0), "gemini-3.1-pro-low", 1001, 65535);
        check("gemini-pro", Some(4000), "gemini-pro-agent", 10001, 65535);
        check("gemini-pro", Some(7000), "gemini-pro-agent", 10001, 65535);

        // ── gemini-3-flash (HonorTier → same as gemini-3.5-flash) ──────
        check("gemini-3-flash", None, "gemini-3-flash-agent", 10000, 65536);
        check(
            "gemini-3-flash",
            Some(0),
            "gemini-3.5-flash-extra-low",
            1000,
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
            "gemini-3-flash-agent",
            10000,
            65536,
        );
    }
}
