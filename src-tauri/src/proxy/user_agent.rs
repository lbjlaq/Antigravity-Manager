// User-Agent rotation module for anti-detection (Story-024-03)
// Provides diverse user agent pool with multiple rotation strategies

use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// Rotation strategy for user-agent selection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RotationStrategy {
    /// Random selection from pool (default)
    Random,
    /// Sequential round-robin rotation
    RoundRobin,
    /// Weighted selection based on browser market share
    Weighted,
}

impl Default for RotationStrategy {
    fn default() -> Self {
        Self::Random
    }
}

/// User-Agent pool manager with configurable rotation strategies
pub struct UserAgentPool {
    agents: Vec<String>,
    strategy: RotationStrategy,
    current_index: Arc<Mutex<usize>>,
}

impl UserAgentPool {
    /// Create a new user-agent pool with default agents and specified strategy
    pub fn new(strategy: RotationStrategy) -> Self {
        Self {
            agents: default_pool(),
            strategy,
            current_index: Arc::new(Mutex::new(0)),
        }
    }

    /// Create a user-agent pool with custom agents and strategy
    /// Falls back to default pool if custom agents are empty or invalid
    pub fn with_custom_agents(custom_agents: Vec<String>, strategy: RotationStrategy) -> Self {
        let agents = if custom_agents.is_empty() {
            tracing::info!("Custom user-agent pool empty, using default pool");
            default_pool()
        } else {
            // Filter invalid user agents
            let valid_agents: Vec<String> = custom_agents
                .into_iter()
                .filter(|ua| Self::validate_user_agent(ua))
                .collect();

            if valid_agents.is_empty() {
                tracing::warn!("All custom user-agents invalid, using default pool");
                default_pool()
            } else {
                tracing::info!("Using {} custom user-agents", valid_agents.len());
                valid_agents
            }
        };

        Self {
            agents,
            strategy,
            current_index: Arc::new(Mutex::new(0)),
        }
    }

    /// Get a user-agent string based on the configured rotation strategy
    pub fn get_user_agent(&self) -> String {
        match self.strategy {
            RotationStrategy::Random => self.get_random(),
            RotationStrategy::RoundRobin => self.get_round_robin(),
            RotationStrategy::Weighted => self.get_weighted(),
        }
    }

    /// Get a random user-agent from the pool
    fn get_random(&self) -> String {
        let mut rng = rand::thread_rng();
        let index = rng.gen_range(0..self.agents.len());
        self.agents[index].clone()
    }

    /// Get the next user-agent in round-robin order
    fn get_round_robin(&self) -> String {
        let mut index = self.current_index.lock().unwrap();
        let current = *index;
        *index = (current + 1) % self.agents.len();
        self.agents[current].clone()
    }

    /// Get a weighted user-agent based on browser market share
    /// Chrome: 40%, Firefox: 25%, Safari: 20%, Edge: 15%
    fn get_weighted(&self) -> String {
        use rand::distributions::{Distribution, WeightedIndex};
        let mut rng = rand::thread_rng();

        // Weights matching browser market share: Chrome (40%), Firefox (25%), Safari (20%), Edge (15%)
        // Default pool: 4 Chrome, 3 Firefox, 2 Safari, 2 Edge
        let weights = vec![
            10, 10, 10, 10, // Chrome (4 agents) - 40%
            8, 8, 9, // Firefox (3 agents) - 25%
            10, 10, // Safari (2 agents) - 20%
            7, 8, // Edge (2 agents) - 15%
        ];

        // Adjust weights if pool size differs from default
        let actual_weights = if weights.len() != self.agents.len() {
            // If custom pool, use equal weights
            vec![1; self.agents.len()]
        } else {
            weights
        };

        let dist = WeightedIndex::new(&actual_weights).unwrap();
        let index = dist.sample(&mut rng);
        self.agents[index].clone()
    }

    /// Get the number of user-agents in the pool
    pub fn size(&self) -> usize {
        self.agents.len()
    }

    /// Validate a user-agent string
    /// Returns true if the user-agent appears to be valid
    pub fn validate_user_agent(ua: &str) -> bool {
        // Basic validation: not empty, reasonable length, contains Mozilla
        !ua.trim().is_empty() && ua.len() >= 50 && ua.len() <= 500 && ua.contains("Mozilla")
    }
}

/// Default user-agent pool with 11 diverse agents
/// Distribution: 4 Chrome (36%), 3 Firefox (27%), 2 Safari (18%), 2 Edge (18%)
fn default_pool() -> Vec<String> {
    vec![
        // Chrome Windows (2 variants)
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36".to_string(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36".to_string(),

        // Chrome macOS (2 variants)
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36".to_string(),
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36".to_string(),

        // Firefox Windows (2 variants)
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:123.0) Gecko/20100101 Firefox/123.0".to_string(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:122.0) Gecko/20100101 Firefox/122.0".to_string(),

        // Firefox macOS (1 variant)
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:123.0) Gecko/20100101 Firefox/123.0".to_string(),

        // Safari macOS (2 variants)
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.3 Safari/605.1.15".to_string(),
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Safari/605.1.15".to_string(),

        // Edge Windows (2 variants)
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36 Edg/122.0.0.0".to_string(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36 Edg/121.0.0.0".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_pool_size() {
        let pool = UserAgentPool::new(RotationStrategy::Random);
        assert!(
            pool.size() >= 10,
            "Default pool must have at least 10 user-agents, got {}",
            pool.size()
        );
    }

    #[test]
    fn test_default_pool_diversity() {
        let pool = UserAgentPool::new(RotationStrategy::Random);
        let agents = &pool.agents;

        // Check for different browser types
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
    fn test_random_rotation() {
        let pool = UserAgentPool::new(RotationStrategy::Random);
        let ua1 = pool.get_user_agent();
        let ua2 = pool.get_user_agent();

        assert!(!ua1.is_empty(), "User-agent should not be empty");
        assert!(!ua2.is_empty(), "User-agent should not be empty");

        // Random strategy might return same UA, just check validity
        assert!(UserAgentPool::validate_user_agent(&ua1));
        assert!(UserAgentPool::validate_user_agent(&ua2));
    }

    #[test]
    fn test_round_robin_rotation() {
        let pool = UserAgentPool::new(RotationStrategy::RoundRobin);
        let mut seen_agents = std::collections::HashSet::new();

        // Collect user-agents from multiple rotations
        for _ in 0..pool.size() {
            let ua = pool.get_user_agent();
            seen_agents.insert(ua);
        }

        // After one full cycle, we should have seen all agents
        assert_eq!(
            seen_agents.len(),
            pool.size(),
            "Round-robin should cycle through all user-agents"
        );
    }

    #[test]
    fn test_round_robin_sequence() {
        let pool = UserAgentPool::new(RotationStrategy::RoundRobin);
        let first_cycle: Vec<String> = (0..pool.size()).map(|_| pool.get_user_agent()).collect();
        let second_cycle: Vec<String> = (0..pool.size()).map(|_| pool.get_user_agent()).collect();

        // Second cycle should match first cycle
        assert_eq!(
            first_cycle, second_cycle,
            "Round-robin should repeat the same sequence"
        );
    }

    #[test]
    fn test_weighted_rotation() {
        let pool = UserAgentPool::new(RotationStrategy::Weighted);
        let mut chrome_count = 0;
        let mut firefox_count = 0;
        let mut safari_count = 0;
        let mut edge_count = 0;
        let iterations = 1000;

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

        // Check approximate distribution (with tolerance)
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
    fn test_custom_agents() {
        let custom = vec![
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(),
            "Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:122.0) Gecko/20100101 Firefox/122.0".to_string(),
        ];
        let pool = UserAgentPool::with_custom_agents(custom.clone(), RotationStrategy::Random);

        assert_eq!(pool.size(), 2, "Pool should use custom agents");

        let ua = pool.get_user_agent();
        assert!(
            custom.contains(&ua),
            "User-agent should be from custom pool"
        );
    }

    #[test]
    fn test_empty_custom_agents_fallback() {
        let pool = UserAgentPool::with_custom_agents(vec![], RotationStrategy::Random);
        assert!(
            pool.size() >= 10,
            "Empty custom agents should fall back to default pool"
        );
    }

    #[test]
    fn test_invalid_custom_agents_fallback() {
        let invalid = vec![
            "".to_string(),
            "short".to_string(),
            "no mozilla here".to_string(),
        ];
        let pool = UserAgentPool::with_custom_agents(invalid, RotationStrategy::Random);
        assert!(
            pool.size() >= 10,
            "Invalid custom agents should fall back to default pool"
        );
    }

    #[test]
    fn test_validate_user_agent() {
        // Valid user-agents
        assert!(UserAgentPool::validate_user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36"
        ));
        assert!(UserAgentPool::validate_user_agent(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.3 Safari/605.1.15"
        ));

        // Invalid user-agents
        assert!(!UserAgentPool::validate_user_agent(""));
        assert!(!UserAgentPool::validate_user_agent("short"));
        assert!(!UserAgentPool::validate_user_agent("no mozilla here"));
        assert!(!UserAgentPool::validate_user_agent(&"x".repeat(600))); // Too long
    }

    #[test]
    fn test_rotation_strategy_default() {
        let strategy = RotationStrategy::default();
        assert_eq!(strategy, RotationStrategy::Random);
    }

    #[test]
    fn test_pool_thread_safety() {
        use std::sync::Arc;
        use std::thread;

        let pool = Arc::new(UserAgentPool::new(RotationStrategy::RoundRobin));
        let mut handles = vec![];

        // Spawn multiple threads to test concurrent access
        for _ in 0..10 {
            let pool_clone = Arc::clone(&pool);
            let handle = thread::spawn(move || {
                for _ in 0..100 {
                    let _ = pool_clone.get_user_agent();
                }
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // If we get here without panicking, thread safety is working
        assert!(true);
    }

    #[test]
    fn test_all_user_agents_valid() {
        let pool = UserAgentPool::new(RotationStrategy::Random);
        for agent in &pool.agents {
            assert!(
                UserAgentPool::validate_user_agent(agent),
                "All default user-agents must be valid: {}",
                agent
            );
        }
    }

    #[test]
    fn test_default_pool_content() {
        let agents = default_pool();

        // Count each browser type
        let chrome_count = agents
            .iter()
            .filter(|ua| ua.contains("Chrome") && !ua.contains("Edg/"))
            .count();
        let firefox_count = agents.iter().filter(|ua| ua.contains("Firefox")).count();
        let safari_count = agents
            .iter()
            .filter(|ua| ua.contains("Safari") && !ua.contains("Chrome"))
            .count();
        let edge_count = agents.iter().filter(|ua| ua.contains("Edg/")).count();

        assert_eq!(chrome_count, 4, "Should have 4 Chrome variants");
        assert_eq!(firefox_count, 3, "Should have 3 Firefox variants");
        assert_eq!(safari_count, 2, "Should have 2 Safari variants");
        assert_eq!(edge_count, 2, "Should have 2 Edge variants");
        assert_eq!(agents.len(), 11, "Total should be 11 user-agents");
    }
}
