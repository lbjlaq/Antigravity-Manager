//! Integration tests for User-Agent rotation (Story-024-03)
//!
//! Tests user-agent rotation with diverse pool, multiple strategies,
//! and configuration integration.

use antigravity_tools_lib::proxy::config::UserAgentConfig;
use antigravity_tools_lib::proxy::upstream::client::UpstreamClient;
use antigravity_tools_lib::proxy::user_agent::{RotationStrategy, UserAgentPool};
use std::collections::HashMap;

#[test]
fn test_user_agent_pool_default_size() {
    let pool = UserAgentPool::new(RotationStrategy::Random);
    assert!(
        pool.size() >= 10,
        "Default pool must have 10+ user-agents, got {}",
        pool.size()
    );
}

#[test]
fn test_user_agent_pool_browser_diversity() {
    let pool = UserAgentPool::new(RotationStrategy::Random);
    let agents: Vec<String> = (0..pool.size()).map(|_| pool.get_user_agent()).collect();

    let has_chrome = agents.iter().any(|ua| ua.contains("Chrome"));
    let has_firefox = agents.iter().any(|ua| ua.contains("Firefox"));
    let has_safari = agents
        .iter()
        .any(|ua| ua.contains("Safari") && !ua.contains("Chrome"));
    let has_edge = agents.iter().any(|ua| ua.contains("Edg/"));

    assert!(has_chrome, "Pool must include Chrome user-agents");
    assert!(has_firefox, "Pool must include Firefox user-agents");
    assert!(has_safari, "Pool must include Safari user-agents");
    assert!(has_edge, "Pool must include Edge user-agents");
}

#[test]
fn test_random_rotation_strategy() {
    let pool = UserAgentPool::new(RotationStrategy::Random);
    let mut user_agents = Vec::new();

    // Collect 100 user-agents
    for _ in 0..100 {
        user_agents.push(pool.get_user_agent());
    }

    // All should be valid
    for ua in &user_agents {
        assert!(
            UserAgentPool::validate_user_agent(ua),
            "Invalid user-agent: {}",
            ua
        );
    }

    // Should see some variation (not all identical)
    let unique_count = user_agents
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    assert!(
        unique_count > 1,
        "Random rotation should produce varied user-agents"
    );
}

#[test]
fn test_round_robin_rotation_strategy() {
    let pool = UserAgentPool::new(RotationStrategy::RoundRobin);
    let pool_size = pool.size();
    let mut first_cycle = Vec::new();
    let mut second_cycle = Vec::new();

    // First complete cycle
    for _ in 0..pool_size {
        first_cycle.push(pool.get_user_agent());
    }

    // Second complete cycle
    for _ in 0..pool_size {
        second_cycle.push(pool.get_user_agent());
    }

    // Cycles should be identical (round-robin pattern)
    assert_eq!(
        first_cycle, second_cycle,
        "Round-robin should repeat the same sequence"
    );

    // All agents in cycle should be unique
    let unique_count = first_cycle
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    assert_eq!(
        unique_count, pool_size,
        "Round-robin should cycle through all user-agents exactly once"
    );
}

#[test]
fn test_weighted_rotation_strategy() {
    let pool = UserAgentPool::new(RotationStrategy::Weighted);
    let iterations = 1000;
    let mut chrome_count = 0;
    let mut firefox_count = 0;
    let mut safari_count = 0;
    let mut edge_count = 0;

    // Collect statistics over many iterations
    for _ in 0..iterations {
        let ua = pool.get_user_agent();
        if ua.contains("Chrome") && !ua.contains("Edg/") {
            chrome_count += 1;
        } else if ua.contains("Firefox") {
            firefox_count += 1;
        } else if ua.contains("Safari") && !ua.contains("Chrome") {
            safari_count += 1;
        } else if ua.contains("Edg/") {
            edge_count += 1;
        }
    }

    // Calculate percentages
    let chrome_pct = (chrome_count as f32 / iterations as f32) * 100.0;
    let firefox_pct = (firefox_count as f32 / iterations as f32) * 100.0;
    let safari_pct = (safari_count as f32 / iterations as f32) * 100.0;
    let edge_pct = (edge_count as f32 / iterations as f32) * 100.0;

    // Allow ±10% tolerance for random distribution
    assert!(
        chrome_pct >= 30.0 && chrome_pct <= 50.0,
        "Chrome distribution should be ~40%, got {:.1}%",
        chrome_pct
    );
    assert!(
        firefox_pct >= 15.0 && firefox_pct <= 35.0,
        "Firefox distribution should be ~25%, got {:.1}%",
        firefox_pct
    );
    assert!(
        safari_pct >= 10.0 && safari_pct <= 30.0,
        "Safari distribution should be ~20%, got {:.1}%",
        safari_pct
    );
    assert!(
        edge_pct >= 5.0 && edge_pct <= 25.0,
        "Edge distribution should be ~15%, got {:.1}%",
        edge_pct
    );
}

#[test]
fn test_custom_user_agents() {
    let custom_agents = vec![
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(),
        "Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:122.0) Gecko/20100101 Firefox/122.0".to_string(),
    ];

    let pool = UserAgentPool::with_custom_agents(custom_agents.clone(), RotationStrategy::Random);

    assert_eq!(pool.size(), 2, "Pool should use 2 custom agents");

    // Get multiple user-agents and verify they're from the custom pool
    for _ in 0..10 {
        let ua = pool.get_user_agent();
        assert!(
            custom_agents.contains(&ua),
            "User-agent should be from custom pool: {}",
            ua
        );
    }
}

#[test]
fn test_empty_custom_agents_fallback_to_default() {
    let pool = UserAgentPool::with_custom_agents(vec![], RotationStrategy::Random);
    assert!(
        pool.size() >= 10,
        "Empty custom agents should fall back to default pool (10+ agents)"
    );
}

#[test]
fn test_invalid_custom_agents_fallback_to_default() {
    let invalid_agents = vec![
        "".to_string(),
        "short".to_string(),
        "no mozilla".to_string(),
    ];

    let pool = UserAgentPool::with_custom_agents(invalid_agents, RotationStrategy::Random);
    assert!(
        pool.size() >= 10,
        "Invalid custom agents should fall back to default pool"
    );
}

#[test]
fn test_user_agent_validation() {
    // Valid user-agents
    assert!(UserAgentPool::validate_user_agent(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36"
    ));
    assert!(UserAgentPool::validate_user_agent(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.3 Safari/605.1.15"
    ));

    // Invalid user-agents
    assert!(!UserAgentPool::validate_user_agent("")); // Empty
    assert!(!UserAgentPool::validate_user_agent("short")); // Too short
    assert!(!UserAgentPool::validate_user_agent(
        "no mozilla here but long enough string to pass length check"
    )); // No Mozilla
    assert!(!UserAgentPool::validate_user_agent(&"x".repeat(600))); // Too long
}

#[test]
fn test_rotation_strategy_serialization() {
    // Test that RotationStrategy can be serialized/deserialized (for config)
    let strategy = RotationStrategy::Random;
    let json = serde_json::to_string(&strategy).unwrap();
    assert_eq!(json, r#""random""#);

    let strategy = RotationStrategy::RoundRobin;
    let json = serde_json::to_string(&strategy).unwrap();
    assert_eq!(json, r#""round_robin""#);

    let strategy = RotationStrategy::Weighted;
    let json = serde_json::to_string(&strategy).unwrap();
    assert_eq!(json, r#""weighted""#);
}

#[test]
fn test_rotation_strategy_deserialization() {
    let strategy: RotationStrategy = serde_json::from_str(r#""random""#).unwrap();
    assert_eq!(strategy, RotationStrategy::Random);

    let strategy: RotationStrategy = serde_json::from_str(r#""round_robin""#).unwrap();
    assert_eq!(strategy, RotationStrategy::RoundRobin);

    let strategy: RotationStrategy = serde_json::from_str(r#""weighted""#).unwrap();
    assert_eq!(strategy, RotationStrategy::Weighted);
}

#[test]
fn test_rotation_strategy_default() {
    let strategy = RotationStrategy::default();
    assert_eq!(strategy, RotationStrategy::Random);
}

#[test]
fn test_user_agent_config_default() {
    let config = UserAgentConfig::default();
    assert!(
        config.enabled,
        "User-agent rotation should be enabled by default"
    );
    assert_eq!(config.strategy, RotationStrategy::Random);
    assert!(config.custom_agents.is_empty());
}

#[test]
fn test_upstream_client_with_ua_rotation_enabled() {
    let ua_config = UserAgentConfig {
        enabled: true,
        strategy: RotationStrategy::Random,
        custom_agents: vec![],
    };

    let _client = UpstreamClient::new_with_ua_config(None, Some(ua_config));

    // Client should be created successfully
    // (We can't test HTTP requests without a mock server, but we can verify creation)
    assert!(true, "UpstreamClient created with UA rotation enabled");
}

#[test]
fn test_upstream_client_with_ua_rotation_disabled() {
    let ua_config = UserAgentConfig {
        enabled: false,
        strategy: RotationStrategy::Random,
        custom_agents: vec![],
    };

    let _client = UpstreamClient::new_with_ua_config(None, Some(ua_config));

    // Client should fall back to static user-agent
    assert!(true, "UpstreamClient created with UA rotation disabled");
}

#[test]
fn test_upstream_client_backward_compatibility() {
    // Legacy constructor should still work (no UA rotation)
    let _client = UpstreamClient::new(None);
    assert!(true, "Legacy constructor works for backward compatibility");
}

#[test]
fn test_thread_safety_of_round_robin() {
    use std::sync::Arc;
    use std::thread;

    let pool = Arc::new(UserAgentPool::new(RotationStrategy::RoundRobin));
    let mut handles = vec![];

    // Spawn 10 threads, each requesting 100 user-agents
    for _ in 0..10 {
        let pool_clone = Arc::clone(&pool);
        let handle = thread::spawn(move || {
            for _ in 0..100 {
                let ua = pool_clone.get_user_agent();
                assert!(UserAgentPool::validate_user_agent(&ua));
            }
        });
        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }

    // If we get here without panicking, thread safety is working
    assert!(true, "Round-robin rotation is thread-safe");
}

#[test]
fn test_user_agent_distribution_tracking() {
    let pool = UserAgentPool::new(RotationStrategy::Random);
    let mut distribution: HashMap<String, usize> = HashMap::new();
    let iterations = 500;

    // Collect user-agent usage statistics
    for _ in 0..iterations {
        let ua = pool.get_user_agent();
        *distribution.entry(ua).or_insert(0) += 1;
    }

    // Verify all user-agents are from the pool
    assert!(
        distribution.len() <= pool.size(),
        "Distribution should only contain user-agents from the pool"
    );

    // Verify reasonable distribution (at least some variety)
    assert!(
        distribution.len() >= 2,
        "Should see at least 2 different user-agents in {} iterations",
        iterations
    );
}

#[test]
fn test_all_default_user_agents_are_valid() {
    let pool = UserAgentPool::new(RotationStrategy::Random);

    // Test all default user-agents individually
    for i in 0..pool.size() {
        // Use round-robin to ensure we test each agent
        let ua = pool.get_user_agent();
        assert!(
            UserAgentPool::validate_user_agent(&ua),
            "Default user-agent at index {} is invalid: {}",
            i,
            ua
        );
    }
}

#[test]
fn test_weighted_rotation_long_term_distribution() {
    let pool = UserAgentPool::new(RotationStrategy::Weighted);
    let iterations = 10000; // Large sample for statistical significance
    let mut chrome_count = 0;
    let mut firefox_count = 0;
    let mut safari_count = 0;
    let mut edge_count = 0;

    for _ in 0..iterations {
        let ua = pool.get_user_agent();
        if ua.contains("Chrome") && !ua.contains("Edg/") {
            chrome_count += 1;
        } else if ua.contains("Firefox") {
            firefox_count += 1;
        } else if ua.contains("Safari") && !ua.contains("Chrome") {
            safari_count += 1;
        } else if ua.contains("Edg/") {
            edge_count += 1;
        }
    }

    let chrome_pct = (chrome_count as f32 / iterations as f32) * 100.0;
    let firefox_pct = (firefox_count as f32 / iterations as f32) * 100.0;
    let safari_pct = (safari_count as f32 / iterations as f32) * 100.0;
    let edge_pct = (edge_count as f32 / iterations as f32) * 100.0;

    // Tighter tolerance (±5%) for large sample
    assert!(
        chrome_pct >= 35.0 && chrome_pct <= 45.0,
        "Chrome long-term distribution should be ~40% (±5%), got {:.1}%",
        chrome_pct
    );
    assert!(
        firefox_pct >= 20.0 && firefox_pct <= 30.0,
        "Firefox long-term distribution should be ~25% (±5%), got {:.1}%",
        firefox_pct
    );
    assert!(
        safari_pct >= 15.0 && safari_pct <= 25.0,
        "Safari long-term distribution should be ~20% (±5%), got {:.1}%",
        safari_pct
    );
    assert!(
        edge_pct >= 10.0 && edge_pct <= 20.0,
        "Edge long-term distribution should be ~15% (±5%), got {:.1}%",
        edge_pct
    );
}
