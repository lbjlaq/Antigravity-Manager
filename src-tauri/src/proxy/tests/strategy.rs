#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use crate::proxy::common::model_mapping::resolve_model_route_plan;
    use crate::proxy::config::{ModelStrategy, ModelFallbackPolicy, ModelPriority, ModelStickiness};

    #[test]
    fn test_family_mapping_with_strategy_candidates() {
        let mut anthropic_mapping = HashMap::new();
        anthropic_mapping.insert("claude-4.5-series".to_string(), "strategy:claude-45-fallback".to_string());

        let mut strategies = HashMap::new();
        strategies.insert(
            "claude-45-fallback".to_string(),
            ModelStrategy {
                candidates: vec![
                    "claude-opus-4-5-thinking".to_string(),
                    "gemini-3-pro-high".to_string(),
                ],
                policy: ModelFallbackPolicy::default(),
            },
        );

        let plan = resolve_model_route_plan(
            "claude-opus-4-5-20251101",
            &HashMap::new(),
            &HashMap::new(),
            &anthropic_mapping,
            &strategies,
            true,
        );

        assert_eq!(plan.primary, "claude-opus-4-5-thinking");
        assert_eq!(plan.fallbacks, vec!["gemini-3-pro-high".to_string()]);
        assert_eq!(plan.strategy_id.as_deref(), Some("claude-45-fallback"));
        assert_eq!(plan.policy.model_priority, ModelPriority::AccuracyFirst);
        assert_eq!(plan.policy.stickiness, ModelStickiness::Strong);
    }

    #[test]
    fn test_max_model_hops_applies_to_strategy() {
        let mut custom_mapping = HashMap::new();
        custom_mapping.insert("gpt-4".to_string(), "strategy:short-list".to_string());

        let mut strategies = HashMap::new();
        strategies.insert(
            "short-list".to_string(),
            ModelStrategy {
                candidates: vec![
                    "gemini-3-pro-high".to_string(),
                    "gemini-3-flash".to_string(),
                    "gemini-2.5-flash".to_string(),
                ],
                policy: ModelFallbackPolicy {
                    model_priority: ModelPriority::AccuracyFirst,
                    stickiness: ModelStickiness::Strong,
                    max_model_hops: Some(2),
                },
            },
        );

        let plan = resolve_model_route_plan(
            "gpt-4",
            &custom_mapping,
            &HashMap::new(),
            &HashMap::new(),
            &strategies,
            false,
        );

        assert_eq!(plan.max_models(), 2);
        assert_eq!(plan.candidates().len(), 3);
    }
}
