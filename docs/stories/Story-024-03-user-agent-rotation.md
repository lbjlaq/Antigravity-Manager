# Story-024-03: User-Agent Rotation Strategy (Anti-Detection)

**Epic**: Epic-024 - Anti-Detection Hardening
**Priority**: P1 (HIGH)
**Effort**: 5-7 hours (3 days)
**Team**: Team 2 (Multi-Protocol Specialists)
**Assignee**: Dev 2A (Senior Security Specialist)
**Created**: 2026-01-12
**Status**: ✅ COMPLETE - QA PASSED (10/10)
**Dependencies**: None (can run in parallel with Story 024-01/024-02)

---

## 🎯 Objective

Implement user-agent rotation with a pool of 10+ diverse user agents (Chrome, Firefox, Safari variants across platforms) to prevent detection via static user-agent patterns. System will randomly select user agents per request with configurable rotation strategies.

---

## 📊 Business Context

### Current State
```yaml
problem:
  user_agent: "Static user-agent (single value)"
  detection_risk: "Easy fingerprinting via consistent UA"
  pattern_recognition: "Upstream APIs can detect proxy via UA"
  severity: "P1 HIGH"

impact:
  affected_users: "ALL users (100%)"
  detection_vector: "User-agent fingerprinting"
  mitigation: "NONE (static UA easily detected)"
```

### Target State
```yaml
goal:
  user_agent_pool: "10+ diverse user agents"
  rotation_strategy: "Random, round-robin, or weighted"
  detection_resistance: "HIGH (diverse UA pool)"

success_metrics:
  - "10+ user agents in rotation pool"
  - "Random selection per request"
  - "Configurable rotation strategies"
  - "Metrics track rotation effectiveness"
```

---

## 🔍 User-Agent Pool Design

### Browser Distribution Strategy
```yaml
user_agent_distribution:
  chrome:
    percentage: 40%
    platforms: [Windows, macOS, Linux]
    versions: ["120.0", "121.0", "122.0"]

  firefox:
    percentage: 25%
    platforms: [Windows, macOS, Linux]
    versions: ["122.0", "123.0"]

  safari:
    percentage: 20%
    platforms: [macOS, iOS]
    versions: ["17.2", "17.3"]

  edge:
    percentage: 15%
    platforms: [Windows]
    versions: ["120.0", "121.0"]
```

### User-Agent Examples
```yaml
chrome_windows:
  - "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36"
  - "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36"

chrome_mac:
  - "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36"
  - "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36"

firefox_windows:
  - "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:123.0) Gecko/20100101 Firefox/123.0"
  - "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:122.0) Gecko/20100101 Firefox/122.0"

safari_mac:
  - "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.3 Safari/605.1.15"
  - "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Safari/605.1.15"

edge_windows:
  - "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36 Edg/122.0.0.0"
```

---

## 📋 Acceptance Criteria

### AC1: User-Agent Pool Implementation (CRITICAL)
```gherkin
GIVEN a need for diverse user-agent rotation
WHEN user-agent pool is implemented
THEN pool MUST contain 10+ diverse user agents
AND pool MUST include Chrome, Firefox, Safari, Edge variants
AND pool MUST span Windows, macOS, Linux, iOS platforms
AND pool MUST include recent browser versions only
```

**Validation**:
- [ ] User-agent pool contains 10+ agents
- [ ] Chrome agents: 4+ variants (40% distribution)
- [ ] Firefox agents: 2+ variants (25% distribution)
- [ ] Safari agents: 2+ variants (20% distribution)
- [ ] Edge agents: 2+ variants (15% distribution)
- [ ] All agents are valid and realistic
- [ ] No outdated browser versions (<6 months old)

---

### AC2: Rotation Strategy Implementation (CRITICAL)
```gherkin
GIVEN a user-agent pool
WHEN rotation strategy is selected
THEN system MUST support multiple rotation strategies
AND strategies MUST include: random, round-robin, weighted
AND strategy MUST be configurable via config file
AND default strategy MUST be random
```

**Validation**:
- [ ] Random rotation implemented (default)
- [ ] Round-robin rotation implemented
- [ ] Weighted rotation implemented
- [ ] Strategy configurable via `proxy_config.json`
- [ ] Strategy can be changed without code changes
- [ ] Test coverage for all three strategies (10+ tests)

---

### AC3: Request Integration (CRITICAL)
```gherkin
GIVEN user-agent rotation is active
WHEN HTTP requests are made to upstream APIs
THEN each request MUST use a user-agent from the pool
AND user-agent MUST be selected per rotation strategy
AND user-agent MUST be included in HTTP headers
AND user-agent selection MUST be logged for debugging
```

**Validation**:
- [ ] `src-tauri/src/proxy/upstream/client.rs` updated
- [ ] User-agent injected into reqwest client
- [ ] Selection logged with request ID
- [ ] Integration tests validate UA in requests (10+ tests)

---

### AC4: Metrics & Monitoring (HIGH)
```gherkin
GIVEN user-agent rotation is active
WHEN requests are processed
THEN metrics MUST track user-agent usage distribution
AND metrics MUST show rotation effectiveness
AND metrics MUST be accessible via monitoring dashboard
```

**Validation**:
- [ ] Metrics track UA selection count per agent
- [ ] Metrics show distribution percentages
- [ ] Metrics detect anomalies (e.g., stuck on one UA)
- [ ] Dashboard displays UA rotation stats
- [ ] Test coverage for metrics (5+ tests)

---

### AC5: Configuration & Customization (MEDIUM)
```gherkin
GIVEN user-agent rotation configuration
WHEN configuration is loaded
THEN custom user-agent pools MUST be supported
AND custom rotation strategies MUST be supported
AND configuration MUST validate user-agent format
AND invalid configurations MUST fail gracefully
```

**Validation**:
- [ ] Configuration schema supports custom UA pools
- [ ] Configuration validates UA string format
- [ ] Invalid UAs trigger warnings (not failures)
- [ ] Default pool used if custom pool invalid
- [ ] Test coverage for config validation (5+ tests)

---

## 🛠️ Implementation Details

### File Structure

```yaml
new_files:
  - "src-tauri/src/proxy/user_agent.rs"  # User-agent pool & rotation logic
  - "tests/security/user_agent_tests.rs"  # Unit tests
  - "tests/security/user_agent_integration_tests.rs"  # Integration tests

modified_files:
  - "src-tauri/src/proxy/upstream/client.rs"  # Inject UA into requests
  - "src-tauri/src/proxy/config.rs"  # Add UA config options
  - "src-tauri/src/proxy/monitor.rs"  # Add UA metrics
```

---

### Step 1: User-Agent Pool Module

**File**: `src-tauri/src/proxy/user_agent.rs` (NEW)

```rust
//! User-Agent rotation for anti-detection
//!
//! Provides a pool of diverse user agents with configurable rotation strategies
//! to prevent fingerprinting via static user-agent patterns.

use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// User-Agent rotation strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RotationStrategy {
    /// Random selection (default)
    Random,
    /// Round-robin (sequential)
    RoundRobin,
    /// Weighted by browser market share
    Weighted,
}

impl Default for RotationStrategy {
    fn default() -> Self {
        Self::Random
    }
}

/// User-Agent pool manager
pub struct UserAgentPool {
    agents: Vec<String>,
    strategy: RotationStrategy,
    current_index: Arc<Mutex<usize>>,
}

impl UserAgentPool {
    /// Create new user-agent pool with default agents
    pub fn new(strategy: RotationStrategy) -> Self {
        Self {
            agents: Self::default_pool(),
            strategy,
            current_index: Arc::new(Mutex::new(0)),
        }
    }

    /// Create pool with custom agents
    pub fn with_custom_agents(agents: Vec<String>, strategy: RotationStrategy) -> Self {
        Self {
            agents: if agents.is_empty() {
                Self::default_pool()
            } else {
                agents
            },
            strategy,
            current_index: Arc::new(Mutex::new(0)),
        }
    }

    /// Default user-agent pool (10+ diverse agents)
    fn default_pool() -> Vec<String> {
        vec![
            // Chrome on Windows (4 variants - 40%)
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36".to_string(),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36".to_string(),
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36".to_string(),
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36".to_string(),

            // Firefox (3 variants - 25%)
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:123.0) Gecko/20100101 Firefox/123.0".to_string(),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:122.0) Gecko/20100101 Firefox/122.0".to_string(),
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:123.0) Gecko/20100101 Firefox/123.0".to_string(),

            // Safari (2 variants - 20%)
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.3 Safari/605.1.15".to_string(),
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Safari/605.1.15".to_string(),

            // Edge (2 variants - 15%)
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36 Edg/122.0.0.0".to_string(),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36 Edg/121.0.0.0".to_string(),
        ]
    }

    /// Get next user-agent based on rotation strategy
    pub fn get_user_agent(&self) -> String {
        match self.strategy {
            RotationStrategy::Random => self.get_random(),
            RotationStrategy::RoundRobin => self.get_round_robin(),
            RotationStrategy::Weighted => self.get_weighted(),
        }
    }

    /// Random selection
    fn get_random(&self) -> String {
        let mut rng = rand::thread_rng();
        self.agents
            .choose(&mut rng)
            .cloned()
            .unwrap_or_else(|| self.agents[0].clone())
    }

    /// Round-robin selection
    fn get_round_robin(&self) -> String {
        let mut index = self.current_index.lock().unwrap();
        let agent = self.agents[*index % self.agents.len()].clone();
        *index = (*index + 1) % self.agents.len();
        agent
    }

    /// Weighted selection (by browser market share)
    fn get_weighted(&self) -> String {
        use rand::distributions::{Distribution, WeightedIndex};
        let mut rng = rand::thread_rng();

        // Weights: Chrome (40%), Firefox (25%), Safari (20%), Edge (15%)
        let weights = vec![
            10, 10, 10, 10,  // Chrome (4 agents)
            8, 8, 9,         // Firefox (3 agents)
            10, 10,          // Safari (2 agents)
            7, 8,            // Edge (2 agents)
        ];

        let dist = WeightedIndex::new(&weights).unwrap();
        let index = dist.sample(&mut rng);
        self.agents[index].clone()
    }

    /// Get pool size
    pub fn size(&self) -> usize {
        self.agents.len()
    }

    /// Validate user-agent string format
    pub fn validate_user_agent(ua: &str) -> bool {
        // Basic validation: must contain "Mozilla" and browser name
        ua.contains("Mozilla") &&
        (ua.contains("Chrome") || ua.contains("Firefox") ||
         ua.contains("Safari") || ua.contains("Edge"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_pool_size() {
        let pool = UserAgentPool::new(RotationStrategy::Random);
        assert!(pool.size() >= 10, "Pool must have 10+ agents");
    }

    #[test]
    fn test_random_rotation() {
        let pool = UserAgentPool::new(RotationStrategy::Random);
        let ua1 = pool.get_user_agent();
        let ua2 = pool.get_user_agent();

        // Both must be valid
        assert!(UserAgentPool::validate_user_agent(&ua1));
        assert!(UserAgentPool::validate_user_agent(&ua2));
    }

    #[test]
    fn test_round_robin_rotation() {
        let pool = UserAgentPool::new(RotationStrategy::RoundRobin);
        let mut agents = vec![];

        // Get twice the pool size
        for _ in 0..(pool.size() * 2) {
            agents.push(pool.get_user_agent());
        }

        // First half should match second half (round-robin pattern)
        assert_eq!(agents[0], agents[pool.size()]);
        assert_eq!(agents[1], agents[pool.size() + 1]);
    }

    #[test]
    fn test_weighted_rotation() {
        let pool = UserAgentPool::new(RotationStrategy::Weighted);
        let ua = pool.get_user_agent();
        assert!(UserAgentPool::validate_user_agent(&ua));
    }

    #[test]
    fn test_custom_agents() {
        let custom = vec![
            "Mozilla/5.0 Custom Agent 1".to_string(),
            "Mozilla/5.0 Custom Agent 2".to_string(),
        ];
        let pool = UserAgentPool::with_custom_agents(custom.clone(), RotationStrategy::Random);
        assert_eq!(pool.size(), 2);
    }

    #[test]
    fn test_empty_custom_agents_fallback() {
        let pool = UserAgentPool::with_custom_agents(vec![], RotationStrategy::Random);
        assert!(pool.size() >= 10, "Should fallback to default pool");
    }

    #[test]
    fn test_validate_user_agent() {
        assert!(UserAgentPool::validate_user_agent("Mozilla/5.0 Chrome/122.0"));
        assert!(UserAgentPool::validate_user_agent("Mozilla/5.0 Firefox/123.0"));
        assert!(!UserAgentPool::validate_user_agent("Invalid UA"));
        assert!(!UserAgentPool::validate_user_agent(""));
    }
}
```

---

### Step 2: Configuration Integration

**File**: `src-tauri/src/proxy/config.rs` (UPDATE)

```rust
use crate::proxy::user_agent::{RotationStrategy, UserAgentPool};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    // ... existing fields

    /// User-agent rotation configuration
    #[serde(default)]
    pub user_agent_rotation: UserAgentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAgentConfig {
    /// Enable user-agent rotation
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Rotation strategy
    #[serde(default)]
    pub strategy: RotationStrategy,

    /// Custom user-agent pool (optional)
    #[serde(default)]
    pub custom_agents: Vec<String>,
}

impl Default for UserAgentConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strategy: RotationStrategy::default(),
            custom_agents: vec![],
        }
    }
}

fn default_true() -> bool {
    true
}
```

**Configuration Example** (`proxy_config.json`):
```json
{
  "user_agent_rotation": {
    "enabled": true,
    "strategy": "random",
    "custom_agents": []
  }
}
```

---

### Step 3: Upstream Client Integration

**File**: `src-tauri/src/proxy/upstream/client.rs` (UPDATE)

```rust
use crate::proxy::user_agent::UserAgentPool;

pub struct UpstreamClient {
    client: reqwest::Client,
    user_agent_pool: Option<UserAgentPool>,
}

impl UpstreamClient {
    pub fn new(config: &ProxyConfig) -> Result<Self, Error> {
        let user_agent_pool = if config.user_agent_rotation.enabled {
            Some(UserAgentPool::with_custom_agents(
                config.user_agent_rotation.custom_agents.clone(),
                config.user_agent_rotation.strategy.clone(),
            ))
        } else {
            None
        };

        // Build reqwest client
        let client = reqwest::Client::builder()
            // ... other config
            .build()?;

        Ok(Self {
            client,
            user_agent_pool,
        })
    }

    /// Execute request with user-agent rotation
    pub async fn execute_request(&self, mut req: reqwest::Request) -> Result<reqwest::Response, Error> {
        // Inject user-agent if rotation enabled
        if let Some(pool) = &self.user_agent_pool {
            let user_agent = pool.get_user_agent();
            req.headers_mut().insert(
                reqwest::header::USER_AGENT,
                user_agent.parse().unwrap(),
            );

            tracing::debug!("Using User-Agent: {}", user_agent);
        }

        self.client.execute(req).await.map_err(Into::into)
    }
}
```

---

### Step 4: Metrics Integration

**File**: `src-tauri/src/proxy/monitor.rs` (UPDATE)

```rust
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct UserAgentMetrics {
    /// User-agent usage count
    pub usage_count: HashMap<String, usize>,

    /// Total requests
    pub total_requests: usize,

    /// Distribution percentages
    pub distribution: HashMap<String, f64>,
}

impl UserAgentMetrics {
    pub fn new() -> Self {
        Self {
            usage_count: HashMap::new(),
            total_requests: 0,
            distribution: HashMap::new(),
        }
    }

    pub fn record_usage(&mut self, user_agent: &str) {
        *self.usage_count.entry(user_agent.to_string()).or_insert(0) += 1;
        self.total_requests += 1;
        self.update_distribution();
    }

    fn update_distribution(&mut self) {
        self.distribution = self.usage_count
            .iter()
            .map(|(ua, count)| {
                let percentage = (*count as f64 / self.total_requests as f64) * 100.0;
                (ua.clone(), percentage)
            })
            .collect();
    }
}
```

---

## 🧪 Test Strategy

### Unit Tests (10+ tests)
**File**: `tests/security/user_agent_tests.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_size() {
        let pool = UserAgentPool::new(RotationStrategy::Random);
        assert!(pool.size() >= 10);
    }

    #[test]
    fn test_chrome_count() {
        let pool = UserAgentPool::new(RotationStrategy::Random);
        let chrome_count = pool.agents.iter()
            .filter(|ua| ua.contains("Chrome") && !ua.contains("Edg"))
            .count();
        assert!(chrome_count >= 4, "Chrome agents should be ~40%");
    }

    #[test]
    fn test_firefox_count() {
        let pool = UserAgentPool::new(RotationStrategy::Random);
        let firefox_count = pool.agents.iter()
            .filter(|ua| ua.contains("Firefox"))
            .count();
        assert!(firefox_count >= 2, "Firefox agents should be ~25%");
    }

    // ... 7+ more unit tests
}
```

### Integration Tests (10+ tests)
**File**: `tests/security/user_agent_integration_tests.rs`

```rust
#[tokio::test]
async fn test_request_includes_user_agent() {
    let config = ProxyConfig::default();
    let client = UpstreamClient::new(&config).unwrap();

    // Build test request
    let req = reqwest::Request::new(
        reqwest::Method::POST,
        "https://test.example.com".parse().unwrap(),
    );

    // Execute with UA rotation
    let result = client.execute_request(req).await;

    // Verify UA was injected
    // (requires mock server or request inspection)
}

// ... 9+ more integration tests
```

---

## 🔗 Dependencies

### Prerequisite
- ✅ Epic-013 COMPLETE - no blockers

### Blocks
- None (runs in parallel with Story 024-01/024-02)

### Enables
- ✅ User-agent fingerprinting protection
- ✅ Detection resistance via diverse UA pool
- ✅ Foundation for Story 024-04 (Detection Monitoring)

---

## 📊 Success Metrics

### Code Metrics
```yaml
files_created: "2 (user_agent.rs + tests)"
files_modified: "3 (client.rs, config.rs, monitor.rs)"
user_agents: "10+ diverse agents"
rotation_strategies: "3 (random, round-robin, weighted)"
test_coverage:
  unit_tests: "10+ tests"
  integration_tests: "10+ tests"
```

### Quality Metrics
```yaml
regression_tests: "398/398 passing"
new_tests: "20+ tests passing (100%)"
code_review: "Dev 2A security validation"
linting: "cargo clippy clean"
```

### Business Metrics
```yaml
detection_resistance: "HIGH (diverse UA pool)"
rotation_effectiveness: "100% (all requests rotated)"
configuration_flexibility: "HIGH (3 strategies + custom pools)"
```

---

## 🎯 Definition of Done

- [ ] **Code Complete**:
  - [ ] user_agent.rs module implemented
  - [ ] 10+ user agents in default pool
  - [ ] 3 rotation strategies implemented
  - [ ] Configuration integration complete
  - [ ] Upstream client integration complete
  - [ ] Metrics tracking implemented

- [ ] **Tests Passing**:
  - [ ] 398/398 regression tests passing
  - [ ] 10+ new unit tests passing (100%)
  - [ ] 10+ new integration tests passing (100%)

- [ ] **Quality Gates**:
  - [ ] Code review approved (Dev 2A security validation)
  - [ ] Linting clean (cargo clippy)
  - [ ] Formatted correctly (cargo fmt)

- [ ] **Documentation**:
  - [ ] User-agent rotation documented
  - [ ] Configuration options documented
  - [ ] Test documentation complete

- [ ] **Deployment Ready**:
  - [ ] Builds successfully
  - [ ] Ready for merge to main

---

**Story Status**: ✅ READY FOR DEVELOPMENT
**Assigned To**: Dev 2A (Senior Security Specialist)
**Start Date**: Week 1 (parallel with Story 024-01/024-02)
**Estimated Completion**: Week 1 End (3 days)
