// Model specifications registry — loads model parameters from JSON config
// to eliminate hardcoded model-specific values across the codebase.

use once_cell::sync::Lazy;
use serde::Deserialize;
use std::collections::HashMap;

/// Default model specification values (fallback when model not found in config)
#[derive(Debug, Deserialize, Clone)]
pub struct ModelDefaults {
    pub max_output_tokens: u64,
    pub thinking_budget_cap: u64,
    pub default_thinking_budget_gemini: u64,
    pub default_thinking_budget_claude: u64,
    pub thinking_overhead: u64,
    pub thinking_min_overhead: u64,
    pub image_thinking_overhead: u64,
    pub image_thinking_min_overhead: u64,
    pub adaptive_max_output_tokens: u64,
    pub adaptive_safe_budget: u64,
    pub adaptive_default_max_output: u64,
    pub opus_fixed_budget: u64,
    pub opus_max_output_tokens: u64,
    pub safe_max_output_cap: u64,
}

/// Per-model specification
#[derive(Debug, Deserialize, Clone)]
pub struct ModelSpec {
    pub max_output_tokens: u64,
    pub thinking_budget_cap: Option<u64>,
    pub supports_thinking: bool,
    pub thinking_budget_limited: bool,
    pub supports_search: Option<bool>,
    pub fixed_thinking_budget: Option<u64>,
    pub fixed_max_output_tokens: Option<u64>,
}

/// Top-level config structure matching model_specs.json
#[derive(Debug, Deserialize, Clone)]
pub struct ModelSpecsConfig {
    pub defaults: ModelDefaults,
    pub model_aliases: HashMap<String, String>,
    pub models: HashMap<String, ModelSpec>,
}

/// Compiled default config (from build-time embedded JSON)
static MODEL_SPECS: Lazy<ModelSpecsConfig> = Lazy::new(|| {
    let json_str = include_str!("../../resources/model_specs.json");
    serde_json::from_str(json_str).expect("Failed to parse embedded model_specs.json")
});

/// Get a reference to the global model specs config
pub fn get_config() -> &'static ModelSpecsConfig {
    &MODEL_SPECS
}

/// Get the defaults
pub fn defaults() -> &'static ModelDefaults {
    &MODEL_SPECS.defaults
}

/// Resolve a model alias to its physical model name.
/// Returns the alias target if found, otherwise returns the input unchanged.
pub fn resolve_alias(model: &str) -> String {
    MODEL_SPECS
        .model_aliases
        .get(model)
        .cloned()
        .unwrap_or_else(|| model.to_string())
}

/// Look up a model spec by name (with prefix matching).
/// First tries exact match, then tries prefix matching (longest prefix wins).
pub fn lookup(model_name: &str) -> Option<&'static ModelSpec> {
    let lower = model_name.to_lowercase();

    // 1. Exact match
    if let Some(spec) = MODEL_SPECS.models.get(&lower) {
        return Some(spec);
    }

    // 2. Prefix match (longest wins)
    let mut best_match: Option<(&str, &ModelSpec)> = None;
    for (key, spec) in &MODEL_SPECS.models {
        if lower.contains(key.as_str()) {
            if best_match.map_or(true, |(k, _)| key.len() > k.len()) {
                best_match = Some((key, spec));
            }
        }
    }

    best_match.map(|(_, spec)| spec)
}

/// Get max_output_tokens for a model (with fallback to default)
pub fn max_output_tokens(model_name: &str) -> u64 {
    lookup(model_name)
        .map(|s| s.max_output_tokens)
        .unwrap_or(MODEL_SPECS.defaults.max_output_tokens)
}

/// Get thinking_budget_cap for a model (with fallback to default)
pub fn thinking_budget_cap(model_name: &str) -> u64 {
    lookup(model_name)
        .and_then(|s| s.thinking_budget_cap)
        .unwrap_or(MODEL_SPECS.defaults.thinking_budget_cap)
}

/// Check if a model has limited thinking budget (needs capping)
pub fn is_thinking_budget_limited(model_name: &str) -> bool {
    lookup(model_name)
        .map(|s| s.thinking_budget_limited)
        .unwrap_or_else(|| {
            // Fallback heuristic for unknown models
            let lower = model_name.to_lowercase();
            (lower.contains("gemini") && !lower.contains("-image"))
                || lower.contains("flash")
                || lower.ends_with("-thinking")
        })
}

/// Get the default thinking budget for a model
pub fn default_thinking_budget(model_name: &str) -> u64 {
    let lower = model_name.to_lowercase();
    if lower.contains("claude") {
        MODEL_SPECS.defaults.default_thinking_budget_claude
    } else {
        MODEL_SPECS.defaults.default_thinking_budget_gemini
    }
}

/// Get thinking overhead (added to budget to compute maxOutputTokens)
pub fn thinking_overhead(is_image_gen: bool) -> u64 {
    if is_image_gen {
        MODEL_SPECS.defaults.image_thinking_overhead
    } else {
        MODEL_SPECS.defaults.thinking_overhead
    }
}

/// Get minimum thinking overhead
pub fn thinking_min_overhead(is_image_gen: bool) -> u64 {
    if is_image_gen {
        MODEL_SPECS.defaults.image_thinking_min_overhead
    } else {
        MODEL_SPECS.defaults.thinking_min_overhead
    }
}

/// Cap maxOutputTokens to the model's limit
pub fn cap_max_output_tokens(model_name: &str, requested: u64) -> u64 {
    let limit = max_output_tokens(model_name);
    if requested > limit {
        tracing::debug!(
            "[ModelSpecs] Capped maxOutputTokens from {} to {} for model {}",
            requested,
            limit,
            model_name
        );
        limit
    } else {
        requested
    }
}

/// Cap thinking budget to the model's limit
pub fn cap_thinking_budget(model_name: &str, requested: u64) -> u64 {
    let cap = thinking_budget_cap(model_name);
    if is_thinking_budget_limited(model_name) && requested > cap {
        tracing::debug!(
            "[ModelSpecs] Capped thinking budget from {} to {} for model {}",
            requested,
            cap,
            model_name
        );
        cap
    } else {
        requested
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_loads() {
        let config = get_config();
        assert!(!config.models.is_empty());
        assert!(config.defaults.max_output_tokens > 0);
    }

    #[test]
    fn test_alias_resolution() {
        assert_eq!(resolve_alias("gemini-3-flash-preview"), "gemini-3-flash");
        assert_eq!(
            resolve_alias("gemini-3-pro-preview"),
            "gemini-3.1-pro-high"
        );
        // Unknown model returns itself
        assert_eq!(resolve_alias("unknown-model"), "unknown-model");
    }

    #[test]
    fn test_lookup_exact() {
        let spec = lookup("gemini-3-flash").unwrap();
        assert_eq!(spec.max_output_tokens, 65536);
        assert!(spec.thinking_budget_limited);
    }

    #[test]
    fn test_lookup_prefix() {
        // Should match "gemini-3-flash" via contains
        let spec = lookup("gemini-3-flash-preview").unwrap();
        assert_eq!(spec.max_output_tokens, 65536);
    }

    #[test]
    fn test_max_output_tokens_cap() {
        // gemini-3-flash: limit is 65536
        assert_eq!(cap_max_output_tokens("gemini-3-flash", 131072), 65536);
        assert_eq!(cap_max_output_tokens("gemini-3-flash", 32000), 32000);

        // Unknown model: uses default 131072
        assert_eq!(cap_max_output_tokens("unknown-model", 131072), 131072);
    }

    #[test]
    fn test_thinking_budget_cap() {
        // gemini-3-flash: limited to 24576
        assert_eq!(cap_thinking_budget("gemini-3-flash", 32000), 24576);
        assert_eq!(cap_thinking_budget("gemini-3-flash", 16000), 16000);
    }

    #[test]
    fn test_default_thinking_budget() {
        assert_eq!(default_thinking_budget("gemini-3-flash"), 24576);
        assert_eq!(default_thinking_budget("claude-sonnet-4-6"), 16000);
    }

    #[test]
    fn test_opus_has_fixed_values() {
        let spec = lookup("claude-opus-4-6-thinking").unwrap();
        assert_eq!(spec.fixed_thinking_budget, Some(24576));
        assert_eq!(spec.fixed_max_output_tokens, Some(57344));
    }
}
