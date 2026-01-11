//! Thinking level mapper for Gemini 3 API
//!
//! Maps token budgets to thinking levels for Gemini 3.x models
//!
//! # API Compatibility
//! - Gemini 3.x: Uses `thinkingLevel` (enum: MINIMAL, LOW, MEDIUM, HIGH)
//! - Gemini 2.5: Uses `thinkingBudget` (integer: 1-32000)

/// Maps token budget to Gemini 3 thinking level
///
/// # Arguments
/// * `model` - The Gemini 3 model name (e.g., "gemini-3-flash", "gemini-3-pro-high")
/// * `budget` - Optional token budget from client (Some(16000) or None)
///
/// # Returns
/// * Thinking level as static string: "MINIMAL", "LOW", "MEDIUM", or "HIGH"
///
/// # Behavior
/// ## Flash Models (4 levels)
/// - MINIMAL: 0-4000 tokens
/// - LOW: 4001-10000 tokens
/// - MEDIUM: 10001-20000 tokens (Flash exclusive!)
/// - HIGH: 20001+ tokens
///
/// ## Pro Models (2 levels)
/// - LOW: 0-16000 tokens
/// - HIGH: 16001+ tokens
/// - Note: Pro models do NOT support MEDIUM level
///
/// ## Defaults (when budget is None)
/// - Flash: "MEDIUM" (balance cost/quality)
/// - Pro: "HIGH" (maximize quality)
///
/// # Examples
/// ```
/// use antigravity_tools_lib::proxy::mappers::common::thinking_level_mapper::determine_thinking_level;
///
/// // Flash with explicit budget
/// assert_eq!(determine_thinking_level("gemini-3-flash", Some(7000)), "LOW");
/// assert_eq!(determine_thinking_level("gemini-3-flash", Some(15000)), "MEDIUM");
///
/// // Pro with explicit budget
/// assert_eq!(determine_thinking_level("gemini-3-pro-high", Some(10000)), "LOW");
/// assert_eq!(determine_thinking_level("gemini-3-pro-high", Some(20000)), "HIGH");
///
/// // Defaults
/// assert_eq!(determine_thinking_level("gemini-3-flash", None), "MEDIUM");
/// assert_eq!(determine_thinking_level("gemini-3-pro-high", None), "HIGH");
/// ```
pub fn determine_thinking_level(model: &str, budget: Option<i32>) -> &'static str {
    // Default levels if no budget specified
    if budget.is_none() {
        return if model.contains("-flash") {
            "MEDIUM" // Flash: balance cost/quality
        } else {
            "HIGH" // Pro: maximize quality
        };
    }

    // Clamp budget to maximum of 32000 tokens
    let budget = budget.unwrap().min(32000);

    if model.contains("-flash") {
        // Flash: 4 levels (MINIMAL, LOW, MEDIUM, HIGH)
        match budget {
            0..=4000 => "MINIMAL",
            4001..=10000 => "LOW",
            10001..=20000 => "MEDIUM",
            _ => "HIGH",
        }
    } else {
        // Pro (High/Low): 2 levels only (LOW, HIGH)
        // CRITICAL: Pro does NOT support MEDIUM level
        match budget {
            0..=16000 => "LOW",
            _ => "HIGH",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flash_minimal_level() {
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(2000)),
            "MINIMAL",
            "Budget 2000 should map to MINIMAL for Flash"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(4000)),
            "MINIMAL",
            "Budget 4000 (boundary) should map to MINIMAL for Flash"
        );
    }

    #[test]
    fn test_flash_low_level() {
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(4001)),
            "LOW",
            "Budget 4001 should map to LOW for Flash"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(7000)),
            "LOW",
            "Budget 7000 should map to LOW for Flash"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(10000)),
            "LOW",
            "Budget 10000 (boundary) should map to LOW for Flash"
        );
    }

    #[test]
    fn test_flash_medium_level() {
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(10001)),
            "MEDIUM",
            "Budget 10001 should map to MEDIUM for Flash"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(15000)),
            "MEDIUM",
            "Budget 15000 should map to MEDIUM for Flash"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(20000)),
            "MEDIUM",
            "Budget 20000 (boundary) should map to MEDIUM for Flash"
        );
    }

    #[test]
    fn test_flash_high_level() {
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(20001)),
            "HIGH",
            "Budget 20001 should map to HIGH for Flash"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(25000)),
            "HIGH",
            "Budget 25000 should map to HIGH for Flash"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(32000)),
            "HIGH",
            "Budget 32000 (max) should map to HIGH for Flash"
        );
    }

    #[test]
    fn test_pro_low_level() {
        assert_eq!(
            determine_thinking_level("gemini-3-pro-high", Some(8000)),
            "LOW",
            "Budget 8000 should map to LOW for Pro High"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-pro-high", Some(16000)),
            "LOW",
            "Budget 16000 (boundary) should map to LOW for Pro High"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-pro-low", Some(10000)),
            "LOW",
            "Budget 10000 should map to LOW for Pro Low"
        );
    }

    #[test]
    fn test_pro_high_level() {
        assert_eq!(
            determine_thinking_level("gemini-3-pro-high", Some(16001)),
            "HIGH",
            "Budget 16001 should map to HIGH for Pro High"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-pro-high", Some(20000)),
            "HIGH",
            "Budget 20000 should map to HIGH for Pro High"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-pro-low", Some(20000)),
            "HIGH",
            "Budget 20000 should map to HIGH for Pro Low"
        );
    }

    #[test]
    fn test_flash_default_medium() {
        assert_eq!(
            determine_thinking_level("gemini-3-flash", None),
            "MEDIUM",
            "Flash default should be MEDIUM (balance cost/quality)"
        );
    }

    #[test]
    fn test_pro_default_high() {
        assert_eq!(
            determine_thinking_level("gemini-3-pro-high", None),
            "HIGH",
            "Pro High default should be HIGH (maximize quality)"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-pro-low", None),
            "HIGH",
            "Pro Low default should be HIGH (maximize quality)"
        );
    }

    #[test]
    fn test_budget_clamping_flash() {
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(35000)),
            "HIGH",
            "Budget >32000 should clamp to 32000, then map to HIGH for Flash"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(50000)),
            "HIGH",
            "Budget 50000 should clamp to 32000, then map to HIGH for Flash"
        );
    }

    #[test]
    fn test_budget_clamping_pro() {
        assert_eq!(
            determine_thinking_level("gemini-3-pro-high", Some(35000)),
            "HIGH",
            "Budget >32000 should clamp to 32000, then map to HIGH for Pro"
        );
    }

    #[test]
    fn test_medium_exclusive_to_flash() {
        // CRITICAL TEST: Pro should NEVER return MEDIUM
        let pro_budgets = vec![5000, 10000, 15000, 20000, 25000];
        for budget in pro_budgets {
            assert_ne!(
                determine_thinking_level("gemini-3-pro-high", Some(budget)),
                "MEDIUM",
                "Pro High should NEVER return MEDIUM (budget: {})",
                budget
            );
            assert_ne!(
                determine_thinking_level("gemini-3-pro-low", Some(budget)),
                "MEDIUM",
                "Pro Low should NEVER return MEDIUM (budget: {})",
                budget
            );
        }

        // Flash SHOULD return MEDIUM for appropriate budgets
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(15000)),
            "MEDIUM",
            "Flash should return MEDIUM for budget 15000"
        );
    }

    #[test]
    fn test_zero_budget() {
        assert_eq!(
            determine_thinking_level("gemini-3-flash", Some(0)),
            "MINIMAL",
            "Budget 0 should map to MINIMAL for Flash"
        );
        assert_eq!(
            determine_thinking_level("gemini-3-pro-high", Some(0)),
            "LOW",
            "Budget 0 should map to LOW for Pro"
        );
    }
}
