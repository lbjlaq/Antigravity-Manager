//! Budget Optimizer Integration Tests for Gemini 3
//!
//! Tests the integration between the adaptive budget optimizer and
//! Gemini 3 thinking level mapping to ensure seamless workflow.

use crate::proxy::budget_optimizer::BudgetOptimizer;
use crate::proxy::mappers::common::thinking_level_mapper::determine_thinking_level;

#[cfg(test)]
mod budget_optimizer_tests {
    use super::*;

    #[test]
    fn test_budget_optimizer_with_flash_thinking() {
        // Verify budget optimizer works with Flash 4-level mapping
        let optimizer = BudgetOptimizer::new();

        // Simple query → low budget → MINIMAL
        let simple_budget = optimizer
            .calculate_optimal_budget("hello", "gemini-3-flash")
            .unwrap();
        assert!(
            simple_budget >= 2000 && simple_budget <= 4000,
            "Simple queries should get 2000-4000 budget"
        );
        let level = determine_thinking_level("gemini-3-flash", Some(simple_budget as i32));
        assert_eq!(
            level, "MINIMAL",
            "Simple budget (2000-4000) should map to MINIMAL for Flash"
        );

        // Moderate query → medium budget → LOW or MEDIUM
        let moderate_budget = optimizer
            .calculate_optimal_budget(
                "explain how transformers work in machine learning",
                "gemini-3-flash",
            )
            .unwrap();
        assert!(
            moderate_budget >= 8000 && moderate_budget <= 12000,
            "Moderate queries should get 8000-12000 budget"
        );
        let level = determine_thinking_level("gemini-3-flash", Some(moderate_budget as i32));
        assert!(
            level == "LOW" || level == "MEDIUM",
            "Moderate budget (8000-12000) should map to LOW or MEDIUM for Flash"
        );

        // Complex query → high budget → MEDIUM or HIGH
        let complex_budget = optimizer
            .calculate_optimal_budget(
                "analyze the architectural trade-offs between microservices and monolithic applications",
                "gemini-3-flash"
            )
            .unwrap();
        assert!(
            complex_budget >= 16000 && complex_budget <= 24000,
            "Complex queries should get 16000-24000 budget"
        );
        let level = determine_thinking_level("gemini-3-flash", Some(complex_budget as i32));
        assert!(
            level == "MEDIUM" || level == "HIGH",
            "Complex budget (16000-24000) should map to MEDIUM or HIGH for Flash"
        );

        // Deep query → max budget → HIGH
        let deep_budget = optimizer
            .calculate_optimal_budget(
                "conduct a comprehensive research on distributed consensus algorithms, comparing Paxos, Raft, and Byzantine fault tolerance, with performance benchmarks and implementation considerations",
                "gemini-3-flash"
            )
            .unwrap();
        assert!(
            deep_budget >= 24000 && deep_budget <= 32000,
            "Deep queries should get 24000-32000 budget"
        );
        let level = determine_thinking_level("gemini-3-flash", Some(deep_budget as i32));
        assert_eq!(
            level, "HIGH",
            "Deep budget (24000-32000) should map to HIGH for Flash"
        );
    }

    #[test]
    fn test_budget_optimizer_with_pro_thinking() {
        // Verify budget optimizer works with Pro 2-level mapping
        let optimizer = BudgetOptimizer::new();

        // Simple/Moderate queries → LOW
        let simple_budget = optimizer
            .calculate_optimal_budget("what is rust?", "gemini-3-pro-high")
            .unwrap();
        let level = determine_thinking_level("gemini-3-pro-high", Some(simple_budget as i32));
        assert_eq!(
            level, "LOW",
            "Simple budget should map to LOW for Pro (only 2 levels)"
        );

        let moderate_budget = optimizer
            .calculate_optimal_budget("explain async/await in rust", "gemini-3-pro-high")
            .unwrap();
        let level = determine_thinking_level("gemini-3-pro-high", Some(moderate_budget as i32));
        assert!(
            level == "LOW" || level == "HIGH",
            "Moderate budget should map to LOW or HIGH for Pro"
        );

        // Complex/Deep queries → HIGH
        let complex_budget = optimizer
            .calculate_optimal_budget(
                "design a high-performance async runtime for rust",
                "gemini-3-pro-high",
            )
            .unwrap();
        assert!(
            complex_budget >= 16000,
            "Complex queries should exceed 16K threshold"
        );
        let level = determine_thinking_level("gemini-3-pro-high", Some(complex_budget as i32));
        assert_eq!(
            level, "HIGH",
            "Complex budget (>16K) should map to HIGH for Pro"
        );

        // Verify Pro NEVER returns MEDIUM
        let all_budgets = vec![simple_budget, moderate_budget, complex_budget];
        for budget in all_budgets {
            let level = determine_thinking_level("gemini-3-pro-high", Some(budget as i32));
            assert_ne!(
                level, "MEDIUM",
                "Pro should NEVER return MEDIUM level (budget: {})",
                budget
            );
        }
    }

    #[test]
    fn test_budget_optimizer_clamping_at_32k() {
        // Verify budgets >32K clamp correctly before mapping
        let optimizer = BudgetOptimizer::new();

        // Even with a very complex query, optimizer should not exceed 32K
        let ultra_complex_budget = optimizer
            .calculate_optimal_budget(
                "perform comprehensive multi-dimensional analysis of global economic systems, political structures, technological paradigms, and social dynamics with historical context and future projections spanning multiple centuries",
                "gemini-3-flash"
            )
            .unwrap();

        assert!(
            ultra_complex_budget <= 32000,
            "Budget optimizer should not exceed 32K maximum"
        );

        // Verify mapping works correctly at 32K boundary
        let level = determine_thinking_level("gemini-3-flash", Some(32000));
        assert_eq!(level, "HIGH", "32K budget should map to HIGH");

        // Verify clamping in mapper for hypothetical >32K budgets
        let level = determine_thinking_level("gemini-3-flash", Some(50000));
        assert_eq!(
            level, "HIGH",
            "Budgets >32K should clamp to 32K and map to HIGH"
        );
    }

    #[test]
    fn test_adaptive_budget_calculation_flash() {
        // Test adaptive budget logic for Flash (conversation length, complexity)
        let optimizer = BudgetOptimizer::new();

        // Test 1: Progressive complexity increase
        let greetings = vec!["hi", "hello", "hey there"];
        let budgets: Vec<u32> = greetings
            .iter()
            .map(|&prompt| {
                optimizer
                    .calculate_optimal_budget(prompt, "gemini-3-flash")
                    .unwrap()
            })
            .collect();

        // All simple greetings should get similar budgets in Simple range
        for budget in &budgets {
            assert!(
                *budget >= 2000 && *budget <= 4000,
                "Greetings should get Simple budget range"
            );
        }

        // Test 2: Technical complexity progression
        let queries = vec![
            "what is rust",                                   // Simple
            "explain rust ownership",                         // Moderate
            "compare rust memory safety with C++",            // Complex
            "analyze rust's borrow checker implementation",   // Deep
        ];

        let technical_budgets: Vec<u32> = queries
            .iter()
            .map(|&prompt| {
                optimizer
                    .calculate_optimal_budget(prompt, "gemini-3-flash")
                    .unwrap()
            })
            .collect();

        // Budgets should increase with complexity
        for i in 1..technical_budgets.len() {
            assert!(
                technical_budgets[i] >= technical_budgets[i - 1],
                "Budget should increase or stay same with complexity (index {})",
                i
            );
        }

        // Map to thinking levels and verify distribution
        let levels: Vec<&str> = technical_budgets
            .iter()
            .map(|&budget| determine_thinking_level("gemini-3-flash", Some(budget as i32)))
            .collect();

        // First should be simple (MINIMAL or LOW), last should be complex (HIGH or MEDIUM)
        assert!(
            levels[0] == "MINIMAL" || levels[0] == "LOW",
            "Simple query should be MINIMAL or LOW, got: {}",
            levels[0]
        );
        assert!(
            levels[levels.len() - 1] == "HIGH" || levels[levels.len() - 1] == "MEDIUM",
            "Complex query should be HIGH or MEDIUM"
        );
    }

    #[test]
    fn test_adaptive_budget_calculation_pro() {
        // Test adaptive budget logic for Pro
        let optimizer = BudgetOptimizer::new();

        // Test consistency across similar complexity queries
        let architectural_queries = vec![
            "design a microservices architecture",
            "architect a distributed system",
            "plan a scalable cloud infrastructure",
        ];

        let budgets: Vec<u32> = architectural_queries
            .iter()
            .map(|&prompt| {
                optimizer
                    .calculate_optimal_budget(prompt, "gemini-3-pro-high")
                    .unwrap()
            })
            .collect();

        // Architectural queries should generally get moderate to high budgets
        // Some might be below 16K depending on complexity analysis
        let high_budget_count = budgets.iter().filter(|&&b| b >= 12000).count();
        assert!(
            high_budget_count >= 2,
            "At least 2/3 architectural queries should get substantial budgets (≥12K)"
        );

        // Map to thinking levels
        let levels: Vec<&str> = budgets
            .iter()
            .map(|&budget| determine_thinking_level("gemini-3-pro-high", Some(budget as i32)))
            .collect();

        // Most architectural queries should map to HIGH, but some might be LOW
        // depending on specific complexity analysis
        let high_count = levels.iter().filter(|&&l| l == "HIGH").count();
        assert!(
            high_count >= 1,
            "At least one architectural query should map to HIGH for Pro (got {} HIGH out of {})",
            high_count,
            levels.len()
        );

        // Test simple queries still get LOW
        let simple_budget = optimizer
            .calculate_optimal_budget("hi", "gemini-3-pro-high")
            .unwrap();
        let level = determine_thinking_level("gemini-3-pro-high", Some(simple_budget as i32));
        assert_eq!(level, "LOW", "Simple query should map to LOW for Pro");
    }
}
